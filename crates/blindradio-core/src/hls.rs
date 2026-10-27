use std::time::{Duration, Instant};

use bytes::Bytes;
use crossbeam_channel::Sender;
use m3u8_rs::{KeyMethod, MediaPlaylist, Playlist, VariantStream};
use reqwest::Url;
use tokio::time::{sleep_until, timeout};

use crate::net::{http_err, not_a_stream};
use crate::stream::send;
use crate::{CoreError, Result};

/// Live playback starts this many segments back from the newest, as cushion against a slow fetch.
const LIVE_EDGE: usize = 3;
/// A live playlist that yields no new segment for this many target durations is lost.
const STALL_TARGETS: u32 = 3;
/// Request limit until the target duration is known.
const FIRST_FETCH: Duration = Duration::from_secs(5);
const MIN_FETCH: Duration = Duration::from_secs(2);

/// Plays an HLS playlist into `tx` as ADTS or MPEG audio. Returns `Ok` at the end
/// of an ended playlist or when the receiver goes away, `Err` when a live playlist stalls.
pub async fn run_hls(
	client: reqwest::Client,
	playlist_url: String,
	tx: Sender<Bytes>,
	on_title: Box<dyn Fn(Option<String>) + Send + Sync>,
) -> Result<()> {
	let url = Url::parse(&playlist_url).map_err(|_| not_a_stream())?;
	let (mut base, playlist) = fetch_playlist(&client, &url, FIRST_FETCH).await?;
	let (url, playlist) = match playlist {
		Playlist::MasterPlaylist(m) => {
			let variant = pick_variant(&m.variants).ok_or_else(unsupported)?;
			let url = base.join(&variant.uri).map_err(|_| not_a_stream())?;
			let (b, p) = fetch_playlist(&client, &url, FIRST_FETCH).await?;
			base = b;
			(url, p)
		}
		p => (url, p),
	};
	let mut playlist = media(playlist)?;
	let target = Duration::from_secs(playlist.target_duration.max(1));
	let limit = target.max(MIN_FETCH);
	let mut next = playlist.media_sequence;
	if !playlist.end_list {
		next += playlist.segments.len().saturating_sub(LIVE_EDGE) as u64;
	}
	let mut loaded = Instant::now();
	let mut progress = loaded;
	let mut last_err = None;
	loop {
		for (seq, segment) in (playlist.media_sequence..).zip(&playlist.segments) {
			if seq < next {
				continue;
			}
			next = seq + 1;
			let Some(audio) = fetch_segment(&client, &base, &segment.uri, limit).await else {
				continue;
			};
			progress = Instant::now();
			let title = segment.title.as_deref().map(str::trim).unwrap_or("");
			if !title.is_empty() {
				on_title(Some(title.to_owned()));
			}
			if !send(&tx, audio).await {
				return Ok(());
			}
		}
		if playlist.end_list {
			return Ok(());
		}
		if progress.elapsed() > target * STALL_TARGETS {
			return Err(last_err.unwrap_or_else(|| CoreError::Http("No data".into())));
		}
		sleep_until((loaded + target).into()).await;
		let reloaded = fetch_playlist(&client, &url, limit).await;
		loaded = Instant::now();
		match reloaded {
			Ok((b, p)) => {
				playlist = media(p)?;
				base = b;
				last_err = None;
			}
			Err(e) => last_err = Some(e),
		}
	}
}

/// Decoder hint for audio sent by `run_hls`: ADTS is AAC, anything else MPEG audio.
pub(crate) fn hint(audio: &[u8]) -> &'static str {
	match audio {
		[0xFF, b, ..] if b & 0xF6 == 0xF0 => "aac",
		_ => "mp3",
	}
}

fn unsupported() -> CoreError {
	CoreError::Decode("Unsupported format".into())
}

/// The audio-only variant with the highest bandwidth, else the lowest-bandwidth variant that may
/// carry audio, for the fastest start.
// ponytail: audio in separate EXT-X-MEDIA renditions is not followed; add when a wanted network needs it.
fn pick_variant(variants: &[VariantStream]) -> Option<&VariantStream> {
	let playable = || variants.iter().filter(|v| !v.is_i_frame);
	let codecs = |v: &VariantStream| v.codecs.clone().unwrap_or_default();
	let audio_only = playable()
		.filter(|v| {
			v.codecs.is_some() && codecs(v).split(',').all(|c| c.trim().starts_with("mp4a"))
		})
		.max_by_key(|v| v.bandwidth);
	audio_only.or_else(|| {
		playable()
			.filter(|v| v.codecs.is_none() || codecs(v).contains("mp4a"))
			.min_by_key(|v| v.bandwidth)
	})
}

/// A media playlist this player can play.
fn media(playlist: Playlist) -> Result<MediaPlaylist> {
	let Playlist::MediaPlaylist(p) = playlist else {
		return Err(not_a_stream());
	};
	// ponytail: fMP4 HLS unsupported; add when a wanted network needs it
	let encrypted = |k: &m3u8_rs::Key| k.method != KeyMethod::None;
	if p.segments
		.iter()
		.any(|s| s.map.is_some() || s.key.as_ref().is_some_and(encrypted))
	{
		return Err(unsupported());
	}
	Ok(p)
}

/// Returns the URL after redirects, which relative URIs resolve against, and the parsed playlist.
async fn fetch_playlist(
	client: &reqwest::Client,
	url: &Url,
	limit: Duration,
) -> Result<(Url, Playlist)> {
	let (base, body) = get(client, url, limit).await?;
	let playlist = m3u8_rs::parse_playlist_res(&body).map_err(|_| not_a_stream())?;
	Ok((base, playlist))
}

/// Tries a segment twice; `None` skips it.
async fn fetch_segment(
	client: &reqwest::Client,
	base: &Url,
	uri: &str,
	limit: Duration,
) -> Option<Bytes> {
	let url = base.join(uri).ok()?;
	for _ in 0..2 {
		if let Ok((_, body)) = get(client, &url, limit).await {
			let audio = segment_audio(body);
			return (!audio.is_empty()).then_some(audio);
		}
	}
	None
}

async fn get(client: &reqwest::Client, url: &Url, limit: Duration) -> Result<(Url, Bytes)> {
	let fetch = async {
		let response = client.get(url.clone()).send().await.map_err(http_err)?;
		let status = response.status().as_u16();
		if status >= 400 {
			return Err(CoreError::Http(format!("HTTP {status}")));
		}
		let base = response.url().clone();
		Ok((base, response.bytes().await.map_err(http_err)?))
	};
	timeout(limit, fetch)
		.await
		.unwrap_or_else(|_| Err(CoreError::Http("No data".into())))
}

/// TS is demuxed; raw ADTS or MP3 only loses a leading ID3 tag (packed audio carries one).
fn segment_audio(body: Bytes) -> Bytes {
	if body.first() == Some(&0x47) && body.get(188).is_none_or(|&b| b == 0x47) {
		return ts_audio(&body).into();
	}
	let skip = match &body[..] {
		[b'I', b'D', b'3', _, _, flags, size @ ..] if size.len() >= 4 => {
			let size = size[..4]
				.iter()
				.fold(0, |n, &b| n << 7 | usize::from(b & 0x7F));
			let footer = if flags & 0x10 != 0 { 10 } else { 0 };
			10 + size + footer
		}
		_ => 0,
	};
	body.slice(skip.min(body.len())..)
}

/// The first audio stream of the first program: ADTS AAC (stream type 0x0F) or MPEG audio (0x03,
/// 0x04), concatenated without PES headers.
// ponytail: PAT and PMT are assumed to fit one packet each (they do in practice), and ID3 timed
// metadata (TIT2) is ignored; parse the 0x15 stream when a wanted network sends titles there.
fn ts_audio(ts: &[u8]) -> Vec<u8> {
	let (mut pmt_pid, mut audio_pid) = (None, None);
	let mut out = Vec::new();
	for packet in ts.as_chunks::<188>().0 {
		let Some((pid, start, payload)) = ts_payload(packet) else {
			continue;
		};
		if pid == 0 && start {
			pmt_pid = psi_section(payload).and_then(pat_pmt_pid);
		} else if Some(pid) == pmt_pid && start {
			audio_pid = psi_section(payload).and_then(pmt_audio_pid);
		} else if Some(pid) == audio_pid {
			let data = if start {
				pes_data(payload)
			} else {
				Some(payload)
			};
			out.extend_from_slice(data.unwrap_or_default());
		}
	}
	out
}

/// PID, payload-unit-start flag and payload of a TS packet.
fn ts_payload(p: &[u8; 188]) -> Option<(u16, bool, &[u8])> {
	if p[0] != 0x47 {
		return None;
	}
	let pid = u16::from_be_bytes([p[1] & 0x1F, p[2]]);
	let payload = match (p[3] >> 4) & 3 {
		1 => &p[4..],
		3 => p.get(5 + usize::from(p[4])..)?,
		_ => return None,
	};
	Some((pid, p[1] & 0x40 != 0, payload))
}

/// The PSI section starting in this payload, without its CRC.
fn psi_section(payload: &[u8]) -> Option<&[u8]> {
	let s = payload.get(1 + usize::from(*payload.first()?)..)?;
	let len = usize::from(*s.get(1)? & 0x0F) << 8 | usize::from(*s.get(2)?);
	s.get(..(3 + len).checked_sub(4)?)
}

fn pat_pmt_pid(pat: &[u8]) -> Option<u16> {
	pat.get(8..)?
		.as_chunks::<4>()
		.0
		.iter()
		.find(|e| e[..2] != [0, 0])
		.map(|e| u16::from_be_bytes([e[2] & 0x1F, e[3]]))
}

fn pmt_audio_pid(pmt: &[u8]) -> Option<u16> {
	let info = usize::from(*pmt.get(10)? & 0x0F) << 8 | usize::from(*pmt.get(11)?);
	let mut streams = pmt.get(12 + info..)?;
	while let [kind, pid_hi, pid_lo, len_hi, len_lo, rest @ ..] = streams {
		if matches!(kind, 0x0F | 0x03 | 0x04) {
			return Some(u16::from_be_bytes([pid_hi & 0x1F, *pid_lo]));
		}
		streams = rest.get(usize::from(len_hi & 0x0F) << 8 | usize::from(*len_lo)..)?;
	}
	None
}

/// Elementary stream data of a PES packet starting in this payload.
fn pes_data(p: &[u8]) -> Option<&[u8]> {
	if !p.starts_with(&[0, 0, 1]) {
		return None;
	}
	p.get(9 + usize::from(*p.get(8)?)..)
}

#[cfg(test)]
mod tests {
	use super::*;

	fn variant(uri: &str, bandwidth: u64, codecs: Option<&str>) -> VariantStream {
		VariantStream {
			uri: uri.into(),
			bandwidth,
			codecs: codecs.map(Into::into),
			..Default::default()
		}
	}

	#[test]
	fn picks_audio_only_else_cheapest_with_audio() {
		let video = |uri, bw| variant(uri, bw, Some("avc1.42e00a,mp4a.40.2"));
		let pick = |vs: &[VariantStream]| pick_variant(vs).map(|v| v.uri.clone());
		let audio = [
			video("v", 10),
			variant("a1", 50, Some("mp4a.40.5")),
			variant("a2", 90, Some("mp4a.40.2")),
		];
		assert_eq!(pick(&audio).as_deref(), Some("a2"));
		let videos = [
			video("hi", 90),
			variant("silent", 5, Some("avc1.42e00a")),
			video("lo", 30),
		];
		assert_eq!(pick(&videos).as_deref(), Some("lo"));
	}

	#[test]
	fn hint_tells_adts_from_mpeg_audio() {
		assert_eq!(hint(&[0xFF, 0xF1, 0x50]), "aac");
		assert_eq!(hint(&[0xFF, 0xFB, 0x90]), "mp3");
	}
}
