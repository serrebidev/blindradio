use std::time::{Duration, Instant};

use aes::Aes128;
use aes::cipher::{BlockCipherDecrypt, KeyInit};
use bytes::Bytes;
use crossbeam_channel::Sender;
use m3u8_rs::{
	AlternativeMedia, AlternativeMediaType, KeyMethod, MediaPlaylist, Playlist, VariantStream,
};
use reqwest::Url;
use tokio::time::{sleep_until, timeout};

use crate::latm::{LatmToAdts, is_loas};
use crate::net::{http_err, not_a_stream, user_agent_for};
use crate::stream::send;
use crate::{CoreError, Result};

/// A live playlist that yields no new segment for this many target durations is lost.
const STALL_TARGETS: u32 = 3;
/// Request limit until the target duration is known.
const FIRST_FETCH: Duration = Duration::from_secs(5);
const MIN_FETCH: Duration = Duration::from_secs(2);

/// Plays an HLS playlist into `tx` as ADTS or MPEG audio. Returns `Ok` at the end
/// of an ended playlist or when the receiver goes away, `Err` when a live playlist stalls.
/// `first` is the body of `playlist_url` when the caller already has it (`net::open` read it).
pub async fn run_hls(
	client: reqwest::Client,
	playlist_url: String,
	first: Option<String>,
	tx: Sender<Bytes>,
	on_title: Box<dyn Fn(Option<String>) + Send + Sync>,
) -> Result<()> {
	let url = Url::parse(&playlist_url).map_err(|_| not_a_stream())?;
	let (mut base, playlist) = match first {
		Some(body) => (url.clone(), parse(body.as_bytes())?),
		None => fetch_playlist(&client, &url, FIRST_FETCH).await?,
	};
	let (url, playlist) = match playlist {
		Playlist::MasterPlaylist(m) => {
			let variant = pick_variant(&m.variants).ok_or_else(unsupported)?;
			let uri = audio_rendition(&m.alternatives, variant).unwrap_or(&variant.uri);
			let url = base.join(uri).map_err(|_| not_a_stream())?;
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
		// The Buffer setting: fewer segments back is closer to live, with less to ride out a slow fetch.
		next += playlist
			.segments
			.len()
			.saturating_sub(crate::ring::hls_behind()) as u64;
	}
	let mut loaded = Instant::now();
	let mut progress = loaded;
	let mut last_err = None;
	let mut init_sent = false;
	// EXT-X-KEY and a byte range without an offset both hold over from the segments before them.
	let mut key: Option<m3u8_rs::Key> = None;
	let mut key_bytes: Option<(Url, [u8; 16])> = None;
	let mut range_end = 0;
	loop {
		let mut fresh = false;
		if !init_sent
			&& let Some(map) = playlist.segments.iter().find_map(|s| s.map.as_ref())
		{
			// fMP4: the segments mean nothing to the decoder without their init segment.
			let url = base.join(&map.uri).map_err(|_| not_a_stream())?;
			let range = map.byte_range.as_ref().map(|r| (r.offset.unwrap_or(0), r.length));
			let (_, init) = get(&client, &url, range, limit).await?;
			if !send(&tx, init).await {
				return Ok(());
			}
			init_sent = true;
		}
		for (seq, segment) in (playlist.media_sequence..).zip(&playlist.segments) {
			if let Some(k) = &segment.key {
				key = (k.method != KeyMethod::None).then(|| k.clone());
			}
			let range = segment.byte_range.as_ref().map(|r| {
				let start = r.offset.unwrap_or(range_end);
				range_end = start + r.length;
				(start, r.length)
			});
			if seq < next {
				continue;
			}
			next = seq + 1;
			fresh = true;
			let decrypt = match &key {
				Some(k) => {
					let Some(bytes) = key_for(&client, &base, k, &mut key_bytes, limit).await
					else {
						continue;
					};
					Some((bytes, iv_for(k, seq)))
				}
				None => None,
			};
			let Some((audio, id3)) =
				fetch_segment(&client, &base, &segment.uri, range, decrypt, limit).await
			else {
				continue;
			};
			progress = Instant::now();
			// An ID3 TIT2 in the segment is the stream's own title; #EXTINF is the fallback.
			let extinf = segment.title.as_deref().map(str::trim).unwrap_or("");
			let title = id3.unwrap_or_else(|| extinf.to_owned());
			if !title.is_empty() {
				on_title(Some(title));
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
		// Nothing new means the next segment is due any moment: looking again sooner finds it up
		// to half a target earlier, which is what makes starting nearer the live edge safe.
		let wait = if fresh { target } else { target / 2 };
		sleep_until((loaded + wait).into()).await;
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

/// Decoder hint for the first audio from TS or `run_hls`: by its first bytes, else MPEG audio.
pub(crate) fn hint(audio: &[u8]) -> &'static str {
	match audio {
		[0xFF, b, ..] if b & 0xF6 == 0xF0 => "aac",
		[0x0B, 0x77, ..] => "ac3",
		[_, _, _, _, b'f', b't', b'y', b'p', ..]
		| [_, _, _, _, b's', b't', b'y', b'p', ..]
		| [_, _, _, _, b'm', b'o', b'o', b'f', ..] => "mp4",
		_ => "mp3",
	}
}

/// The key a segment needs, fetched once per key URI.
async fn key_for(
	client: &reqwest::Client,
	base: &Url,
	key: &m3u8_rs::Key,
	held: &mut Option<(Url, [u8; 16])>,
	limit: Duration,
) -> Option<[u8; 16]> {
	let url = base.join(key.uri.as_deref()?).ok()?;
	if let Some((at, bytes)) = held
		&& *at == url
	{
		return Some(*bytes);
	}
	let (_, body) = get(client, &url, None, limit).await.ok()?;
	let bytes: [u8; 16] = body.as_ref().try_into().ok()?;
	*held = Some((url, bytes));
	Some(bytes)
}

/// The IV a segment states, else its media sequence number, as HLS defines.
fn iv_for(key: &m3u8_rs::Key, seq: u64) -> [u8; 16] {
	let stated = key.iv.as_deref().and_then(|iv| {
		let hex = iv.strip_prefix("0x").or_else(|| iv.strip_prefix("0X"))?;
		u128::from_str_radix(hex, 16).ok()
	});
	stated.unwrap_or(u128::from(seq)).to_be_bytes()
}

/// AES-128-CBC with PKCS#7 padding, the whole segment at once. `None` when it is not valid.
fn decrypt_segment(data: &[u8], key: &[u8; 16], iv: [u8; 16]) -> Option<Vec<u8>> {
	let (blocks, rest) = data.as_chunks::<16>();
	if !rest.is_empty() || blocks.is_empty() {
		return None;
	}
	let cipher = Aes128::new(&(*key).into());
	let mut out = Vec::with_capacity(data.len());
	let mut previous = iv;
	for block in blocks {
		let mut plain = (*block).into();
		cipher.decrypt_block(&mut plain);
		out.extend(plain.iter().zip(previous).map(|(p, v)| p ^ v));
		previous = *block;
	}
	let pad = usize::from(*out.last()?);
	if !(1..=16).contains(&pad) || !out[out.len() - pad..].iter().all(|&b| usize::from(b) == pad) {
		return None;
	}
	out.truncate(out.len() - pad);
	Some(out)
}

fn unsupported() -> CoreError {
	CoreError::Decode("Unsupported format".into())
}

/// The audio-only variant with the highest bandwidth, else the lowest-bandwidth variant that may
/// carry audio, for the fastest start.
fn pick_variant(variants: &[VariantStream]) -> Option<&VariantStream> {
	const AUDIO: [&str; 5] = ["mp4a", "ac-3", "ec-3", "opus", "flac"];
	let is_audio = |c: &str| {
		let c = c.trim().to_ascii_lowercase();
		AUDIO.iter().any(|a| c.starts_with(a))
	};
	let playable = || variants.iter().filter(|v| !v.is_i_frame);
	let codecs = |v: &VariantStream| v.codecs.clone().unwrap_or_default();
	let audio_only = playable()
		.filter(|v| v.codecs.is_some() && codecs(v).split(',').all(is_audio))
		.max_by_key(|v| v.bandwidth);
	audio_only.or_else(|| {
		playable()
			.filter(|v| v.codecs.is_none() || codecs(v).split(',').any(is_audio))
			.min_by_key(|v| v.bandwidth)
	})
}

/// The playlist of the separate audio this variant names (EXT-X-MEDIA), the group's default
/// first. With one, the variant's own segments are video only and are never fetched. An
/// entry without a URI means the audio is muxed into the variant.
fn audio_rendition<'a>(
	alternatives: &'a [AlternativeMedia],
	variant: &VariantStream,
) -> Option<&'a String> {
	let group = variant.audio.as_ref()?;
	let mut audio: Vec<_> = alternatives
		.iter()
		.filter(|a| a.media_type == AlternativeMediaType::Audio && a.group_id == *group)
		.filter(|a| a.uri.is_some())
		.collect();
	audio.sort_by_key(|a| !a.default);
	audio.first()?.uri.as_ref()
}

/// A media playlist this player can play.
fn media(playlist: Playlist) -> Result<MediaPlaylist> {
	let Playlist::MediaPlaylist(p) = playlist else {
		return Err(not_a_stream());
	};
	// Plain AES-128 is ordinary HLS: the key is one more file the server hands out. SAMPLE-AES and
	// any other key format is DRM, and stays refused.
	let identity = |k: &m3u8_rs::Key| {
		k.keyformat
			.as_deref()
			.is_none_or(|f| f.eq_ignore_ascii_case("identity"))
	};
	let playable = |k: &m3u8_rs::Key| match k.method {
		KeyMethod::None => true,
		KeyMethod::AES128 => k.uri.is_some() && identity(k),
		_ => false,
	};
	if !p.segments.iter().all(|s| s.key.as_ref().is_none_or(playable)) {
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
	let (base, body) = get(client, url, None, limit).await?;
	Ok((base, parse(&body)?))
}

fn parse(body: &[u8]) -> Result<Playlist> {
	// The parser wants #EXTM3U as the very first bytes; sniffing tolerates a BOM and blank lines.
	let body = body.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(body);
	m3u8_rs::parse_playlist_res(body.trim_ascii_start()).map_err(|_| not_a_stream())
}

/// Tries a segment twice; `None` skips it. Returns its audio and any ID3 title it carries.
async fn fetch_segment(
	client: &reqwest::Client,
	base: &Url,
	uri: &str,
	range: Option<(u64, u64)>,
	decrypt: Option<([u8; 16], [u8; 16])>,
	limit: Duration,
) -> Option<(Bytes, Option<String>)> {
	let url = base.join(uri).ok()?;
	for _ in 0..2 {
		if let Ok((_, body)) = get(client, &url, range, limit).await {
			let body = match decrypt {
				Some((key, iv)) => decrypt_segment(&body, &key, iv)?.into(),
				None => body,
			};
			let (audio, title) = segment_audio(body);
			return (!audio.is_empty()).then_some((audio, title));
		}
	}
	None
}

/// `range` is a byte range's start and length, for segments cut from one file.
async fn get(
	client: &reqwest::Client,
	url: &Url,
	range: Option<(u64, u64)>,
	limit: Duration,
) -> Result<(Url, Bytes)> {
	let fetch = async {
		let mut request = client
			.get(url.clone())
			.header("User-Agent", user_agent_for(url.as_str()));
		if let Some((start, length)) = range {
			let last = start + length.max(1) - 1;
			request = request.header("Range", format!("bytes={start}-{last}"));
		}
		let response = request.send().await.map_err(http_err)?;
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
/// The title comes from ID3 TIT2: TS timed metadata, or the packed-audio tag.
fn segment_audio(body: Bytes) -> (Bytes, Option<String>) {
	if body.first() == Some(&0x47) && body.get(188).is_none_or(|&b| b == 0x47) {
		let mut demux = TsDemux::default();
		let audio = demux.feed(&body);
		return (audio.into(), demux.take_title());
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
	let skip = skip.min(body.len());
	(body.slice(skip..), id3_title(&body[..skip]))
}

/// Text of the first TIT2 frame in an ID3v2.3 or v2.4 tag.
fn id3_title(tag: &[u8]) -> Option<String> {
	let [b'I', b'D', b'3', version, _, _, size @ ..] = tag else {
		return None;
	};
	let synchsafe = |b: &[u8]| b.iter().fold(0, |n, &b| n << 7 | usize::from(b & 0x7F));
	let end = (10 + synchsafe(size.get(..4)?)).min(tag.len());
	let mut frames = tag.get(10..end)?;
	while let Some(id) = frames.get(..10) {
		let len = if *version >= 4 {
			synchsafe(&id[4..8])
		} else {
			id[4..8].iter().fold(0, |n, &b| n << 8 | usize::from(b))
		};
		let body = frames.get(10..10 + len)?;
		if &id[..4] == b"TIT2" {
			return text_frame(body);
		}
		if id[0] == 0 {
			return None;
		}
		frames = &frames[10 + len..];
	}
	None
}

/// An ID3 text frame body: encoding byte, then Latin-1, UTF-16 (BOM or BE) or UTF-8.
fn text_frame(body: &[u8]) -> Option<String> {
	let (&encoding, text) = body.split_first()?;
	let utf16 = |text: &[u8], be: bool| {
		let units: Vec<u16> = text
			.as_chunks::<2>()
			.0
			.iter()
			.map(|&b| {
				if be {
					u16::from_be_bytes(b)
				} else {
					u16::from_le_bytes(b)
				}
			})
			.collect();
		String::from_utf16_lossy(&units)
	};
	let s = match encoding {
		0 => text.iter().map(|&b| char::from(b)).collect(),
		1 => match text {
			[0xFF, 0xFE, rest @ ..] => utf16(rest, false),
			[0xFE, 0xFF, rest @ ..] => utf16(rest, true),
			_ => utf16(text, true),
		},
		2 => utf16(text, true),
		_ => String::from_utf8_lossy(text).into_owned(),
	};
	let s = s
		.trim_matches(|c: char| c == '\0' || c.is_whitespace())
		.to_owned();
	(!s.is_empty()).then_some(s)
}

/// Whether these bytes start like MPEG-TS, however the stream is labeled: three sync bytes a
/// packet apart, at any offset inside the first packet.
pub(crate) fn looks_like_ts(bytes: &[u8]) -> bool {
	(0..188).any(|i| {
		[i, i + 188, i + 376]
			.iter()
			.all(|&at| bytes.get(at) == Some(&0x47))
	})
}

/// The channel's name from the first service in a DVB SDT (PID 0x11, table 0x42), as DVBViewer
/// names channels.
// ponytail: an SDT is assumed to fit one packet, as PAT and PMT are; a long one reads as no name.
pub(crate) fn ts_service_name(ts: &[u8]) -> Option<String> {
	let start = (0..188).find(|&i| ts.get(i) == Some(&0x47) && ts.get(i + 188) == Some(&0x47))?;
	ts[start..].as_chunks::<188>().0.iter().find_map(|packet| {
		let (pid, unit_start, payload) = ts_payload(packet).filter(|_| packet[0] == 0x47)?;
		if pid != 0x11 || !unit_start {
			return None;
		}
		let section = psi_section(payload).filter(|s| s.first() == Some(&0x42))?;
		// Past the table header, then each service: id, flags, a length and its descriptors.
		let mut services = section.get(11..)?;
		while let [_, _, _, len_hi, len_lo, rest @ ..] = services {
			let len = usize::from(len_hi & 0x0F) << 8 | usize::from(*len_lo);
			let (mut descriptors, after) = rest.split_at_checked(len)?;
			while let [tag, size, rest @ ..] = descriptors {
				let (body, next) = rest.split_at_checked(usize::from(*size))?;
				if *tag == 0x48 {
					// Service type, the provider's name, then the service's.
					let provider = usize::from(*body.get(1)?);
					let name_len = usize::from(*body.get(2 + provider)?);
					let name = body.get(3 + provider..3 + provider + name_len)?;
					return Some(dvb_text(name));
				}
				descriptors = next;
			}
			services = after;
		}
		None
	})
}

/// A DVB string: a leading byte below 0x20 names the character set, and only UTF-8 (0x15) is
/// kept apart from the Latin-style default, which reads right for ASCII names.
fn dvb_text(bytes: &[u8]) -> String {
	match bytes {
		[0x15, rest @ ..] => String::from_utf8_lossy(rest).into_owned(),
		[0x10, _, _, rest @ ..] => rest.iter().map(|&b| char::from(b)).collect(),
		[0x01..=0x1F, rest @ ..] => rest.iter().map(|&b| char::from(b)).collect(),
		_ => bytes.iter().map(|&b| char::from(b)).collect(),
	}
}

/// MPEG-TS to audio, fed in pieces of any size. Takes the first audio stream of the first
/// program, concatenated without PES headers: ADTS AAC, MPEG audio, AC-3 or E-AC-3 as they are,
/// and LATM AAC rewritten as ADTS. Also keeps the ID3 timed metadata stream (type 0x15), which
/// carries titles.
// ponytail: PAT and PMT are assumed to fit one packet each (they do in practice).
#[derive(Default)]
pub(crate) struct TsDemux {
	pmt_pid: Option<u16>,
	audio_pid: Option<u16>,
	id3_pid: Option<u16>,
	/// A PMT was read, so a missing audio stream is known to be missing.
	pmt_seen: bool,
	/// Audio before the first PES start is the tail of a frame already lost.
	audio_started: bool,
	/// Set when the audio stream is LATM, by the PMT or its first bytes.
	latm: Option<LatmToAdts>,
	id3: Vec<u8>,
	/// Bytes of a packet that has not fully arrived.
	carry: Vec<u8>,
}

impl TsDemux {
	/// Returns the audio these bytes complete.
	pub(crate) fn feed(&mut self, bytes: &[u8]) -> Vec<u8> {
		self.carry.extend_from_slice(bytes);
		let buf = std::mem::take(&mut self.carry);
		let mut out = Vec::new();
		let mut at = 0;
		while let Some(packet) = buf.get(at..at + 188) {
			// Out of sync, or a stray 0x47: step until a packet is followed by another.
			if packet[0] != 0x47 || buf.get(at + 188).is_some_and(|&b| b != 0x47) {
				at += 1;
				continue;
			}
			self.packet(packet, &mut out);
			at += 188;
		}
		self.carry = buf[at..].to_vec();
		out
	}

	fn packet(&mut self, packet: &[u8], out: &mut Vec<u8>) {
		let Some((pid, start, payload)) = ts_payload(packet) else {
			return;
		};
		let data = |payload| {
			if start {
				pes_data(payload).unwrap_or_default()
			} else {
				payload
			}
		};
		if pid == 0 && start {
			if let Some(pmt) = psi_section(payload).and_then(pat_pmt_pid) {
				self.pmt_pid = Some(pmt);
			}
		} else if Some(pid) == self.pmt_pid && start {
			if let Some(section) = psi_section(payload) {
				let (audio, id3, latm) = pmt_pids(section);
				(self.audio_pid, self.id3_pid) = (audio, id3);
				if latm && self.latm.is_none() {
					self.latm = Some(LatmToAdts::default());
				}
				self.pmt_seen = true;
			}
		} else if Some(pid) == self.id3_pid {
			// Each PES starts a new tag; keep the latest.
			if start {
				self.id3.clear();
			}
			self.id3.extend_from_slice(data(payload));
		} else if Some(pid) == self.audio_pid {
			if !self.audio_started && start && is_loas(data(payload)) && self.latm.is_none() {
				// DVB's AAC descriptor leaves ADTS or LATM open; the bytes settle it.
				self.latm = Some(LatmToAdts::default());
			}
			self.audio_started |= start;
			if self.audio_started {
				match &mut self.latm {
					Some(latm) => latm.feed(data(payload), out),
					None => out.extend_from_slice(data(payload)),
				}
			}
		}
	}

	/// The program is known to hold no audio this player can decode.
	pub(crate) fn no_audio(&self) -> bool {
		self.pmt_seen && self.audio_pid.is_none()
	}

	/// The title of a complete ID3 tag, once.
	pub(crate) fn take_title(&mut self) -> Option<String> {
		let title = id3_title(&self.id3)?;
		self.id3.clear();
		Some(title)
	}
}

/// PID, payload-unit-start flag and payload of a whole TS packet.
fn ts_payload(p: &[u8]) -> Option<(u16, bool, &[u8])> {
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

/// Whether a stream of private data (type 0x06) is audio, by its descriptors: DVB's AC-3, E-AC-3
/// and AAC descriptors, or a registration naming AC-3 or E-AC-3.
fn private_is_audio(mut descriptors: &[u8]) -> bool {
	while let [tag, len, rest @ ..] = descriptors {
		let Some((body, after)) = rest.split_at_checked(usize::from(*len)) else {
			return false;
		};
		if matches!(tag, 0x6A | 0x7A | 0x7C)
			|| (*tag == 0x05 && matches!(body, [b'A', b'C', b'-', b'3', ..] | [b'E', b'A', b'C', b'3', ..]))
		{
			return true;
		}
		descriptors = after;
	}
	false
}

/// The first audio PID, the ID3 timed metadata PID, if the PMT lists them, and whether that audio
/// is LATM. A later audio stream of a kind this player cannot decode never shadows a good one.
fn pmt_pids(pmt: &[u8]) -> (Option<u16>, Option<u16>, bool) {
	let (mut audio, mut id3, mut latm) = (None, None, false);
	let info = pmt
		.get(10..12)
		.map_or(0, |b| usize::from(b[0] & 0x0F) << 8 | usize::from(b[1]));
	let mut streams = pmt.get(12 + info..).unwrap_or_default();
	while let [kind, pid_hi, pid_lo, len_hi, len_lo, rest @ ..] = streams {
		let pid = u16::from_be_bytes([pid_hi & 0x1F, *pid_lo]);
		let len = usize::from(len_hi & 0x0F) << 8 | usize::from(*len_lo);
		let descriptors = rest.get(..len).unwrap_or_default();
		match kind {
			// ADTS, MPEG-1 and MPEG-2 audio, ATSC's AC-3 and E-AC-3.
			0x0F | 0x03 | 0x04 | 0x81 | 0x87 if audio.is_none() => audio = Some(pid),
			0x11 if audio.is_none() => {
				audio = Some(pid);
				latm = true;
			}
			0x06 if audio.is_none() && private_is_audio(descriptors) => audio = Some(pid),
			0x15 if id3.is_none() => id3 = Some(pid),
			_ => {}
		}
		streams = rest.get(len..).unwrap_or_default();
	}
	(audio, id3, latm)
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
	fn audio_rendition_prefers_the_default_in_the_variants_group() {
		let audio = |group: &str, uri: Option<&str>, default| AlternativeMedia {
			media_type: AlternativeMediaType::Audio,
			group_id: group.into(),
			uri: uri.map(Into::into),
			default,
			..Default::default()
		};
		let all = [
			audio("other", Some("wrong"), true),
			audio("aac", Some("second"), false),
			audio("aac", Some("main"), true),
		];
		let mut v = variant("v", 10, None);
		assert_eq!(audio_rendition(&all, &v), None, "no group named");
		v.audio = Some("aac".into());
		assert_eq!(audio_rendition(&all, &v).map(String::as_str), Some("main"));
		let muxed = [audio("aac", None, true)];
		assert_eq!(audio_rendition(&muxed, &v), None, "muxed into the variant");
	}

	#[test]
	fn parse_tolerates_a_bom_and_leading_blank_lines() {
		let body = b"\xEF\xBB\xBF\r\n#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXTINF:4,\na.aac\n";
		assert!(matches!(parse(body), Ok(Playlist::MediaPlaylist(_))));
	}

	#[test]
	fn hint_tells_the_format_by_its_first_bytes() {
		assert_eq!(hint(&[0xFF, 0xF1, 0x50]), "aac");
		assert_eq!(hint(&[0xFF, 0xFB, 0x90]), "mp3");
		assert_eq!(hint(&[0x0B, 0x77, 0, 0]), "ac3");
		assert_eq!(hint(b"\0\0\0\x20ftypiso5"), "mp4");
	}

	#[test]
	fn pmt_finds_audio_by_type_and_by_descriptor() {
		// Header to the program info length (none), then the streams.
		let pmt = |streams: &[u8]| {
			let mut p = vec![0x02, 0xB0, 0, 0, 1, 0xC1, 0, 0, 0xE1, 0x01, 0xF0, 0];
			p.extend_from_slice(streams);
			p
		};
		let entry = |kind: u8, pid: u8, desc: &[u8]| {
			let mut e = vec![kind, 0xE1, pid, 0xF0, desc.len() as u8];
			e.extend_from_slice(desc);
			e
		};
		let of = |streams: Vec<Vec<u8>>| pmt_pids(&pmt(&streams.concat()));
		let (dvb_ac3, dvb_eac3) = ([0x6A, 1, 0], [0x7A, 1, 0]);
		assert_eq!(of(vec![entry(0x81, 0x10, &[])]).0, Some(0x110), "ATSC AC-3");
		assert_eq!(of(vec![entry(0x87, 0x10, &[])]).0, Some(0x110), "ATSC E-AC-3");
		assert_eq!(of(vec![entry(0x06, 0x10, &dvb_ac3)]).0, Some(0x110), "DVB AC-3");
		assert_eq!(of(vec![entry(0x06, 0x10, &dvb_eac3)]).0, Some(0x110), "DVB E-AC-3");
		let registered = [0x05, 4, b'A', b'C', b'-', b'3'];
		assert_eq!(of(vec![entry(0x06, 0x10, &registered)]).0, Some(0x110));
		let latm = of(vec![entry(0x11, 0x10, &[])]);
		assert_eq!((latm.0, latm.2), (Some(0x110), true), "LATM");
		// Teletext and subtitles are private data too, and H.264 is no audio.
		let other = vec![
			entry(0x1B, 0x10, &[]),
			entry(0x06, 0x11, &[0x56, 1, 0]),
			entry(0x06, 0x12, &[0x59, 1, 0]),
			entry(0x0F, 0x13, &[]),
		];
		assert_eq!(of(other).0, Some(0x113));
		// The first audio stays first, whatever follows.
		let two = vec![entry(0x06, 0x10, &dvb_ac3), entry(0x0F, 0x11, &[])];
		assert_eq!(of(two).0, Some(0x110));
	}

	#[test]
	fn aes128_segments_decrypt_and_bad_padding_is_refused() {
		use aes::cipher::BlockCipherEncrypt;
		let (key, iv) = ([7u8; 16], [9u8; 16]);
		let plain = b"sixteen byte msgtail".to_vec();
		let mut padded = plain.clone();
		let pad = 16 - plain.len() % 16;
		padded.resize(plain.len() + pad, pad as u8);
		let cipher = Aes128::new(&key.into());
		let mut sealed = Vec::new();
		let mut previous = iv;
		for block in padded.as_chunks::<16>().0 {
			let mut xored = [0u8; 16];
			for i in 0..16 {
				xored[i] = block[i] ^ previous[i];
			}
			let mut out = xored.into();
			cipher.encrypt_block(&mut out);
			previous.copy_from_slice(&out);
			sealed.extend_from_slice(&out);
		}
		assert_eq!(decrypt_segment(&sealed, &key, iv), Some(plain));
		assert_eq!(decrypt_segment(&sealed[..17], &key, iv), None, "partial block");
		assert_eq!(decrypt_segment(&sealed, &[1; 16], iv), None, "wrong key");
		let key_tag = |iv: Option<&str>| m3u8_rs::Key {
			method: KeyMethod::AES128,
			uri: Some("k".into()),
			iv: iv.map(Into::into),
			..Default::default()
		};
		assert_eq!(iv_for(&key_tag(None), 5), 5u128.to_be_bytes(), "from the sequence");
		assert_eq!(iv_for(&key_tag(Some("0x0A")), 5), 10u128.to_be_bytes());
	}

	#[test]
	fn only_plain_aes_is_played() {
		let with = |key: m3u8_rs::Key| {
			media(Playlist::MediaPlaylist(MediaPlaylist {
				segments: vec![m3u8_rs::MediaSegment {
					key: Some(key),
					..Default::default()
				}],
				..Default::default()
			}))
		};
		let aes = |format: Option<&str>| m3u8_rs::Key {
			method: KeyMethod::AES128,
			uri: Some("k".into()),
			keyformat: format.map(Into::into),
			..Default::default()
		};
		assert!(with(aes(None)).is_ok());
		assert!(with(aes(Some("identity"))).is_ok());
		assert!(with(aes(Some("com.apple.streamingkeydelivery"))).is_err(), "DRM");
		let sample = m3u8_rs::Key {
			method: KeyMethod::SampleAES,
			uri: Some("k".into()),
			..Default::default()
		};
		assert!(with(sample).is_err());
	}

	fn id3(version: u8, frames: &[(&[u8; 4], &[u8])]) -> Vec<u8> {
		let synchsafe = |n: usize| {
			[
				(n >> 21) as u8 & 0x7F,
				(n >> 14) as u8 & 0x7F,
				(n >> 7) as u8 & 0x7F,
				n as u8 & 0x7F,
			]
		};
		let mut body = Vec::new();
		for (id, data) in frames {
			body.extend_from_slice(*id);
			if version >= 4 {
				body.extend_from_slice(&synchsafe(data.len()));
			} else {
				body.extend_from_slice(&(data.len() as u32).to_be_bytes());
			}
			body.extend_from_slice(&[0, 0]);
			body.extend_from_slice(data);
		}
		let mut tag = vec![b'I', b'D', b'3', version, 0, 0];
		tag.extend_from_slice(&synchsafe(body.len()));
		tag.extend(body);
		tag
	}

	#[test]
	fn id3_title_reads_tit2_in_each_encoding() {
		let utf8 = id3(4, &[(b"TIT2", b"\x03Song - One\0")]);
		assert_eq!(id3_title(&utf8).as_deref(), Some("Song - One"));
		let latin1 = id3(3, &[(b"TPE1", b"\x00X"), (b"TIT2", b"\x00Caf\xe9")]);
		assert_eq!(id3_title(&latin1).as_deref(), Some("Caf\u{e9}"));
		let utf16 = id3(3, &[(b"TIT2", b"\x01\xff\xfeA\x00B\x00")]);
		assert_eq!(id3_title(&utf16).as_deref(), Some("AB"));
		assert_eq!(id3_title(&id3(4, &[(b"TALB", b"\x03x")])), None);
		assert_eq!(id3_title(b"not a tag"), None);
	}

	#[test]
	fn packed_audio_keeps_its_id3_title() {
		let mut seg = id3(4, &[(b"TIT2", b"\x03Packed")]);
		seg.extend_from_slice(&[0xFF, 0xF1, 0x50]);
		let (audio, title) = segment_audio(Bytes::from(seg));
		assert_eq!(&audio[..], &[0xFF, 0xF1, 0x50]);
		assert_eq!(title.as_deref(), Some("Packed"));
	}

	fn ts_packet(pid: u16, start: bool, payload: &[u8]) -> Vec<u8> {
		let mut p = vec![
			0x47,
			(u8::from(start) << 6) | (pid >> 8) as u8,
			pid as u8,
			0x10,
		];
		p.extend_from_slice(payload);
		p.resize(188, 0xFF);
		p
	}

	#[test]
	fn ts_demux_survives_any_chunking_and_a_late_start() {
		let pat = [
			0, 0x00, 0xB0, 13, 0, 1, 0xC1, 0, 0, 0, 1, 0xE1, 0x00, 0, 0, 0, 0,
		];
		let pmt = [
			0, 0x02, 0xB0, 18, 0, 1, 0xC1, 0, 0, 0xE1, 0x01, 0xF0, 0, 0x0F, 0xE1, 0x01, 0xF0, 0, 0,
			0, 0, 0,
		];
		let pes = |data: &[u8]| [&[0, 0, 1, 0xC0, 0, 0, 0x80, 0x80, 0][..], data].concat();
		// A full payload, so the demuxer's output is exact.
		let tail = [0xBB; 184];
		let ts = [
			// Joined mid-packet, then mid-frame: neither may reach the decoder.
			vec![0x11; 50],
			ts_packet(0x101, false, &[0xCC; 184]),
			ts_packet(0, true, &pat),
			ts_packet(0x100, true, &pmt),
			ts_packet(0x101, false, &[0xDD; 184]),
			ts_packet(0x101, true, &pes(&[0xFF, 0xF1, 0x50])),
			ts_packet(0x101, false, &tail),
		]
		.concat();
		assert!(looks_like_ts(&ts));
		assert!(!looks_like_ts(&[0xFF; 1024]));
		for size in [1, 7, 188, 200, ts.len()] {
			let mut demux = TsDemux::default();
			let audio: Vec<u8> = ts.chunks(size).flat_map(|c| demux.feed(c)).collect();
			assert!(audio.starts_with(&[0xFF, 0xF1, 0x50]), "chunks of {size}");
			assert!(audio.ends_with(&tail), "chunks of {size}");
			assert!(
				!audio.contains(&0xCC) && !audio.contains(&0xDD),
				"chunks of {size}"
			);
			assert!(!demux.no_audio());
		}
	}

	#[test]
	fn ts_demux_tells_when_the_program_has_no_audio() {
		let pat = [
			0, 0x00, 0xB0, 13, 0, 1, 0xC1, 0, 0, 0, 1, 0xE1, 0x00, 0, 0, 0, 0,
		];
		// H.264 only.
		let pmt = [
			0, 0x02, 0xB0, 18, 0, 1, 0xC1, 0, 0, 0xE1, 0x01, 0xF0, 0, 0x1B, 0xE1, 0x01, 0xF0, 0, 0,
			0, 0, 0,
		];
		let ts = [ts_packet(0, true, &pat), ts_packet(0x100, true, &pmt)].concat();
		let mut demux = TsDemux::default();
		assert!(demux.feed(&ts).is_empty());
		assert!(demux.no_audio());
	}

	#[test]
	fn ts_timed_id3_gives_the_title() {
		// PAT: program 1 on PMT PID 0x100. PMT: ADTS on 0x101, ID3 timed metadata (0x15) on 0x102.
		let pat = [
			0, 0x00, 0xB0, 13, 0, 1, 0xC1, 0, 0, 0, 1, 0xE1, 0x00, 0, 0, 0, 0,
		];
		let pmt = [
			0, 0x02, 0xB0, 23, 0, 1, 0xC1, 0, 0, 0xE1, 0x01, 0xF0, 0, 0x0F, 0xE1, 0x01, 0xF0, 0,
			0x15, 0xE1, 0x02, 0xF0, 0, 0, 0, 0, 0,
		];
		let pes = |data: &[u8]| [&[0, 0, 1, 0xBD, 0, 0, 0x80, 0x80, 0][..], data].concat();
		let tag = id3(4, &[(b"TIT2", b"\x03TS Title")]);
		let ts = [
			ts_packet(0, true, &pat),
			ts_packet(0x100, true, &pmt),
			ts_packet(0x102, true, &pes(&tag)),
			ts_packet(0x101, true, &pes(&[0xFF, 0xF1, 0x50])),
		]
		.concat();
		let (audio, title) = segment_audio(Bytes::from(ts));
		assert!(audio.starts_with(&[0xFF, 0xF1, 0x50]));
		assert_eq!(title.as_deref(), Some("TS Title"));
	}
}
