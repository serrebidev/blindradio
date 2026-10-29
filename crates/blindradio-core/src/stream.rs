use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use bytes::Bytes;
use crossbeam_channel::{Receiver, Sender, TryRecvError, TrySendError};
use parking_lot::Mutex;
use tokio::sync::oneshot;
use tokio::task::AbortHandle;
use tokio::time::{sleep, timeout};

use std::collections::HashMap;

use crate::Result;
use crate::decode::{ByteReader, Decoder, byte_pipe};
use crate::hls::{hint as hls_hint, run_hls};
use crate::net::{Opened, ResolveCache, open};
use crate::record::Recorder;
use crate::resample::Resampler;
use crate::ring::PcmRing;
use crate::station::Station;
use crate::timeshift::ShiftBuffer;

/// Side channels on a playing station's audio, registered by station id.
/// The stream loops re-read their entry per chunk, so attaching or removing
/// taps takes effect on a running stream at once.
#[derive(Clone, Default)]
pub struct Taps {
	/// Full time-shift buffer (minutes deep).
	pub shift: Option<Arc<ShiftBuffer>>,
	/// Short capture buffer feeding song recognition.
	pub capture: Option<Arc<ShiftBuffer>>,
	/// The recorder slot; the app starts and stops the recorder inside it
	/// while the decode loop writes.
	pub record: Option<Arc<Mutex<Option<Recorder>>>>,
}

pub type TapMap = Arc<Mutex<HashMap<String, Taps>>>;

pub fn taps_for(taps: &TapMap, station_id: &str) -> Taps {
	taps.lock().get(station_id).cloned().unwrap_or_default()
}

/// Connect, open and the first body bytes must all fit in this.
const START_BUDGET: Duration = Duration::from_secs(5);
/// A cached final URL gets this much before the station URL is tried instead.
const CACHED_BUDGET: Duration = Duration::from_secs(2);
const MIN_STATION_BUDGET: Duration = Duration::from_secs(3);
/// A connection that sends nothing for this long is treated as lost.
const STALL: Duration = Duration::from_secs(5);
/// Decoded-audio room per stream. Deliberately far above the mixer's starting
/// cushion: a stream that keeps running dry earns a much bigger refill before
/// it may sound again (see MixerCore), and the ring must be able to hold it.
const RING_SECONDS: f32 = 20.0;
/// End reason of an HLS playlist that finished (`#EXT-X-ENDLIST`).
const ENDED: &str = "Ended";

#[derive(Clone)]
pub struct EngineCtx {
	pub rt: tokio::runtime::Handle,
	pub client: reqwest::Client,
	pub cache: Arc<Mutex<ResolveCache>>,
	pub taps: TapMap,
	pub out_rate: u32,
	pub retry_window: Duration,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StreamEvent {
	FirstAudio,
	Title(Option<String>),
	Reconnecting,
	Reconnected,
	Failed(String),
}

/// An event can still arrive just after the stream is dropped; ignore events from dropped streams.
pub struct StationStream {
	pub ring: Arc<PcmRing>,
	pub station: Station,
	shared: Arc<Shared>,
	task: AbortHandle,
}

impl Drop for StationStream {
	fn drop(&mut self) {
		self.shared.alive.store(false, Ordering::Relaxed);
		// A decode thread waiting on a full ring would otherwise wait forever.
		self.ring.close();
		// Dropping the task's future drops the pipe sender, so the decode thread reads EOF and exits.
		self.task.abort();
	}
}

struct Shared {
	on_event: Box<dyn Fn(StreamEvent) + Send + Sync>,
	alive: AtomicBool,
	played: AtomicBool,
	/// `Reconnecting` was sent and no connection has played since.
	announced: AtomicBool,
}

impl Shared {
	fn emit(&self, event: StreamEvent) {
		if self.alive.load(Ordering::Relaxed) {
			(self.on_event)(event);
		}
	}
}

pub fn start(
	station: &Station,
	ctx: &EngineCtx,
	on_event: Box<dyn Fn(StreamEvent) + Send + Sync>,
) -> StationStream {
	let ring = Arc::new(PcmRing::new(ctx.out_rate, RING_SECONDS));
	let shared = Arc::new(Shared {
		on_event,
		alive: AtomicBool::new(true),
		played: AtomicBool::new(false),
		announced: AtomicBool::new(false),
	});
	let task = ctx
		.rt
		.spawn(run(
			station.url.clone(),
			station.id.clone(),
			ctx.clone(),
			ring.clone(),
			shared.clone(),
		))
		.abort_handle();
	StationStream {
		ring,
		station: station.clone(),
		shared,
		task,
	}
}

struct Conn {
	source: Source,
	first: Bytes,
	hint: Option<&'static str>,
}

enum Source {
	Http {
		response: reqwest::Response,
		metaint: Option<usize>,
	},
	Hls(Hls),
}

/// `run_hls` polled in place, so dropping the connection stops it.
struct Hls {
	task: Option<Pin<Box<dyn Future<Output = Result<()>> + Send>>>,
	audio: Receiver<Bytes>,
	titles: Receiver<Option<String>>,
	/// Why the task ended.
	end: String,
}

impl Hls {
	fn start(client: reqwest::Client, playlist_url: String, playlist: String) -> Hls {
		let (tx, audio) = crossbeam_channel::bounded(64);
		let (title_tx, titles) = crossbeam_channel::unbounded();
		let on_title = Box::new(move |t| drop(title_tx.send(t)));
		let task = run_hls(client, playlist_url, Some(playlist), tx, on_title);
		Hls {
			task: Some(Box::pin(task)),
			audio,
			titles,
			end: "No data".into(),
		}
	}

	/// Next segment's audio; `None` once `run_hls` has ended and all it sent is taken.
	async fn next(&mut self) -> Option<Bytes> {
		loop {
			match self.audio.try_recv() {
				Ok(bytes) => return Some(bytes),
				Err(TryRecvError::Disconnected) => return None,
				Err(TryRecvError::Empty) => {}
			}
			let task = self.task.as_mut()?;
			// crossbeam has no async recv, so the queue is checked every 10 ms while the task runs.
			if let Ok(result) = timeout(Duration::from_millis(10), task).await {
				// Dropping the task drops its sender; what it queued is still taken first.
				self.task = None;
				self.end = match result {
					Ok(()) => ENDED.into(),
					Err(e) => e.to_string(),
				};
			}
		}
	}
}

async fn run(
	url: String,
	station_id: String,
	ctx: EngineCtx,
	ring: Arc<PcmRing>,
	shared: Arc<Shared>,
) {
	let mut conn = match connect(&ctx, &url).await {
		Ok(c) => c,
		Err(reason) => return shared.emit(StreamEvent::Failed(reason)),
	};
	let mut title = None;
	// Start of the current outage and retries made in it; only a connection that plays ends it.
	let mut outage: Option<Instant> = None;
	let mut attempt = 0;
	loop {
		let (reason, produced) = play(conn, &ctx, &station_id, &ring, &shared, &mut title).await;
		// An ended HLS playlist (VOD) is over; reconnecting would replay it from the start.
		if !shared.played.load(Ordering::Relaxed) || reason == ENDED {
			return shared.emit(StreamEvent::Failed(reason));
		}
		if produced {
			outage = None;
			attempt = 0;
		}
		let since = *outage.get_or_insert_with(Instant::now);
		match reconnect(&url, &ctx, &ring, &shared, since, &mut attempt).await {
			Some(c) => conn = c,
			None => return shared.emit(StreamEvent::Failed("Lost".into())),
		}
	}
}

/// Tries the cached final URL briefly, evicting it on any failure, then the station URL.
async fn connect(ctx: &EngineCtx, url: &str) -> std::result::Result<Conn, String> {
	let began = Instant::now();
	let cached = ctx.cache.lock().get(url).filter(|c| c != url);
	if let Some(cached) = cached {
		if let Ok(Ok((conn, final_url))) = timeout(CACHED_BUDGET, open_audio(ctx, &cached)).await {
			ctx.cache.lock().put(url, &final_url);
			return Ok(conn);
		}
		ctx.cache.lock().remove(url);
	}
	let budget = START_BUDGET
		.saturating_sub(began.elapsed())
		.max(MIN_STATION_BUDGET);
	let (conn, final_url) = timeout(budget, open_audio(ctx, url))
		.await
		.unwrap_or_else(|_| Err("No data".to_owned()))?;
	ctx.cache.lock().put(url, &final_url);
	Ok(conn)
}

/// Opens `url` and waits for the first body bytes.
async fn open_audio(ctx: &EngineCtx, url: &str) -> std::result::Result<(Conn, String), String> {
	let (mut response, final_url) = match open(&ctx.client, url).await.map_err(|e| e.to_string())? {
		Opened::Audio {
			response,
			final_url,
		} => (response, final_url),
		Opened::Hls {
			playlist_url,
			playlist,
		} => {
			let mut hls = Hls::start(ctx.client.clone(), playlist_url.clone(), playlist);
			let Some(first) = hls.next().await else {
				return Err(hls.end);
			};
			let conn = Conn {
				hint: Some(hls_hint(&first)),
				source: Source::Hls(hls),
				first,
			};
			return Ok((conn, playlist_url));
		}
	};
	let first = match response.chunk().await {
		Ok(Some(c)) => c,
		_ => return Err("No data".to_owned()),
	};
	let header = |name| {
		response
			.headers()
			.get(name)
			.and_then(|v| v.to_str().ok())
			.map(|v| v.trim().to_ascii_lowercase())
	};
	let metaint = header("icy-metaint")
		.and_then(|v| v.parse().ok())
		.filter(|&m| m > 0);
	let hint = header("content-type").and_then(|ct| ext_hint(&ct));
	let conn = Conn {
		source: Source::Http { response, metaint },
		first,
		hint,
	};
	Ok((conn, final_url))
}

fn ext_hint(content_type: &str) -> Option<&'static str> {
	let ct = content_type.split(';').next()?.trim();
	Some(match ct {
		"audio/mpeg" | "audio/mp3" => "mp3",
		"audio/aac" | "audio/aacp" | "audio/x-aac" => "aac",
		"audio/ogg" | "application/ogg" | "audio/opus" => "ogg",
		"audio/flac" | "audio/x-flac" => "flac",
		_ => return None,
	})
}

/// Retries with 1, 2, 4, then 5 s gaps until `retry_window` after `since`. Says `Reconnecting`
/// once per outage, only if the ring runs dry; the decode thread says `Reconnected`.
async fn reconnect(
	url: &str,
	ctx: &EngineCtx,
	ring: &PcmRing,
	shared: &Shared,
	since: Instant,
	attempt: &mut u32,
) -> Option<Conn> {
	let watch = async {
		loop {
			if ring.len_ms() == 0 && !shared.announced.swap(true, Ordering::Relaxed) {
				shared.emit(StreamEvent::Reconnecting);
			}
			sleep(Duration::from_millis(100)).await;
		}
	};
	let retry = async {
		loop {
			if *attempt > 0 {
				let gap = Duration::from_secs(match *attempt {
					1 => 1,
					2 => 2,
					3 => 4,
					_ => 5,
				});
				if since.elapsed() + gap > ctx.retry_window {
					return None;
				}
				sleep(gap).await;
			}
			*attempt += 1;
			// The window is a deadline: an attempt still connecting when it closes gives up.
			let left = ctx.retry_window.saturating_sub(since.elapsed());
			match timeout(left, connect(ctx, url)).await {
				Ok(Ok(conn)) => return Some(conn),
				Ok(Err(_)) => {}
				Err(_) => return None,
			}
		}
	};
	tokio::select! {
		c = retry => c,
		_ = watch => unreachable!(),
	}
}

/// Plays one connection to its end; returns why it ended and whether it produced audio.
async fn play(
	mut conn: Conn,
	ctx: &EngineCtx,
	station_id: &str,
	ring: &Arc<PcmRing>,
	shared: &Arc<Shared>,
	title: &mut Option<String>,
) -> (String, bool) {
	let (tx, reader) = byte_pipe();
	let (done_tx, done_rx) = oneshot::channel();
	let (thread_ring, thread_shared) = (ring.clone(), shared.clone());
	let (out_rate, hint) = (ctx.out_rate, conn.hint);
	let (thread_taps, thread_id) = (ctx.taps.clone(), station_id.to_string());
	let produced = Arc::new(AtomicBool::new(false));
	let thread_produced = produced.clone();
	let spawned = std::thread::Builder::new()
		.name("decode".into())
		.spawn(move || {
			let result = decode(
				reader,
				hint,
				&thread_ring,
				out_rate,
				&thread_shared,
				&thread_produced,
				&thread_taps,
				&thread_id,
			);
			let _ = done_tx.send(result);
		});
	if spawned.is_err() {
		return ("No data".into(), false);
	}

	let net_reason = pump(&mut conn, tx, shared, title, &produced, &ctx.taps, station_id).await;
	// The sender is gone, so the decoder drains what it has and ends; waiting keeps two
	// connections from pushing into the ring at once.
	let decoded = done_rx.await;
	let reason = match (net_reason, decoded) {
		// The network ended first: a decoder error after that is only the truncated input.
		(Some(reason), _) => reason,
		(None, Ok(Err(e))) => e.to_string(),
		(None, _) => "No data".into(),
	};
	(reason, produced.load(Ordering::Relaxed))
}

/// Strips ICY metadata and feeds audio bytes to the decoder until the connection ends.
/// Returns why the network side ended, or `None` when the decoder went away first.
async fn pump(
	conn: &mut Conn,
	tx: Sender<Bytes>,
	shared: &Shared,
	title: &mut Option<String>,
	produced: &AtomicBool,
	taps: &TapMap,
	station_id: &str,
) -> Option<String> {
	// Bytes that never decode (endless junk under an audio content type) must not play silence forever.
	let decode_by = Instant::now() + START_BUDGET;
	let mut icy = match conn.source {
		Source::Http { metaint, .. } => metaint.map(Icy::new),
		Source::Hls(_) => None,
	};
	let mut chunk = std::mem::take(&mut conn.first);
	let mut audio = Vec::new();
	let mut titles = Vec::new();
	loop {
		match &mut icy {
			Some(icy) => icy.feed(chunk, &mut audio, &mut titles),
			None => audio.push(chunk),
		}
		if let Source::Hls(hls) = &conn.source {
			titles.extend(hls.titles.try_iter());
		}
		for t in titles.drain(..) {
			if t != *title {
				title.clone_from(&t);
				shared.emit(StreamEvent::Title(t));
			}
		}
		for bytes in audio.drain(..) {
			// An Original-format recording taps the clean encoded bytes here.
			if let Some(slot) = &taps_for(taps, station_id).record
				&& let Some(rec) = slot.lock().as_mut()
			{
				let _ = rec.write_bytes(&bytes);
			}
			if !send(&tx, bytes).await {
				return None;
			}
		}
		if !produced.load(Ordering::Relaxed) && Instant::now() > decode_by {
			return Some("Not a stream".into());
		}
		chunk = match &mut conn.source {
			Source::Http { response, .. } => match timeout(STALL, response.chunk()).await {
				Ok(Ok(Some(c))) => c,
				Ok(Ok(None)) | Err(_) => return Some("No data".into()),
				Ok(Err(_)) => return Some("Unreachable".into()),
			},
			// run_hls has its own stall limit, since segments arrive a target duration apart.
			Source::Hls(hls) => match hls.next().await {
				Some(c) => c,
				None => return Some(hls.end.clone()),
			},
		};
	}
}

/// Hands `bytes` to the decode thread; `false` once it is gone.
pub(crate) async fn send(tx: &Sender<Bytes>, mut bytes: Bytes) -> bool {
	// The decoder keeps up with any live stream; Full only lasts through a burst.
	loop {
		match tx.try_send(bytes) {
			Ok(()) => return true,
			Err(TrySendError::Full(back)) => {
				bytes = back;
				sleep(Duration::from_millis(10)).await;
			}
			Err(TrySendError::Disconnected(_)) => return false,
		}
	}
}

fn decode(
	reader: ByteReader,
	hint: Option<&str>,
	ring: &PcmRing,
	out_rate: u32,
	shared: &Shared,
	produced: &AtomicBool,
	taps: &TapMap,
	station_id: &str,
) -> Result<()> {
	let mut decoder = Decoder::new(Box::new(reader), hint)?;
	let mut rate = decoder.sample_rate();
	let mut resampler = Resampler::new(rate, out_rate);
	let (mut pcm, mut out) = (Vec::new(), Vec::new());
	while decoder.next_stereo(&mut pcm)? {
		// A chained Ogg stream can switch rates after a decoder reset.
		if decoder.sample_rate() != rate {
			rate = decoder.sample_rate();
			resampler = Resampler::new(rate, out_rate);
		}
		resampler.process(&pcm, &mut out);
		pcm.clear();
		if out.is_empty() {
			continue;
		}
		ring.push(&out);
		// Taps: time-shift and recognition buffers plus PCM recording formats.
		// A failing tap must never stop playback, so errors are dropped.
		let t = taps_for(taps, station_id);
		if let Some(b) = &t.shift {
			b.push(&out);
		}
		if let Some(b) = &t.capture {
			b.push(&out);
		}
		if let Some(slot) = &t.record
			&& let Some(rec) = slot.lock().as_mut()
		{
			let _ = rec.write_pcm(&out);
		}
		out.clear();
		if !produced.swap(true, Ordering::Relaxed) {
			if !shared.played.swap(true, Ordering::Relaxed) {
				shared.emit(StreamEvent::FirstAudio);
			} else if shared.announced.swap(false, Ordering::Relaxed) {
				shared.emit(StreamEvent::Reconnected);
			}
		}
	}
	Ok(())
}

/// ICY de-interleaver: after every `metaint` audio bytes comes a length byte (x16) and that
/// much metadata.
struct Icy {
	metaint: usize,
	state: IcyState,
	meta: Vec<u8>,
}

enum IcyState {
	Audio(usize),
	Length,
	Meta(usize),
}

impl Icy {
	fn new(metaint: usize) -> Icy {
		Icy {
			metaint,
			state: IcyState::Audio(metaint),
			meta: Vec::new(),
		}
	}

	fn feed(&mut self, mut chunk: Bytes, audio: &mut Vec<Bytes>, titles: &mut Vec<Option<String>>) {
		while !chunk.is_empty() {
			match self.state {
				IcyState::Audio(left) => {
					let n = left.min(chunk.len());
					audio.push(chunk.split_to(n));
					self.state = if n == left {
						IcyState::Length
					} else {
						IcyState::Audio(left - n)
					};
				}
				IcyState::Length => {
					let len = chunk.split_to(1)[0] as usize * 16;
					self.state = if len == 0 {
						IcyState::Audio(self.metaint)
					} else {
						IcyState::Meta(len)
					};
				}
				IcyState::Meta(left) => {
					let n = left.min(chunk.len());
					self.meta.extend_from_slice(&chunk.split_to(n));
					if n < left {
						self.state = IcyState::Meta(left - n);
						continue;
					}
					if let Some(t) = parse_title(&self.meta) {
						titles.push(t);
					}
					self.meta.clear();
					self.state = IcyState::Audio(self.metaint);
				}
			}
		}
	}
}

/// `Some(title)` if the block has a `StreamTitle`; the inner `None` means an empty title.
/// Titles can hold `';`, so the value runs to the `';` that starts the next field, else the last one.
fn parse_title(block: &[u8]) -> Option<Option<String>> {
	let end = block.iter().rposition(|&b| b != 0).map_or(0, |i| i + 1);
	let block = &block[..end];
	let text = match std::str::from_utf8(block) {
		Ok(s) => s.to_owned(),
		Err(_) => block.iter().map(|&b| char::from(b)).collect(),
	};
	let start = text.find("StreamTitle='")? + "StreamTitle='".len();
	let rest = &text[start..];
	let ends: Vec<usize> = rest.match_indices("';").map(|(i, _)| i).collect();
	let value = match ends.iter().find(|&&i| starts_field(&rest[i + 2..])) {
		Some(&i) => &rest[..i],
		None => match ends.last() {
			Some(&i) => &rest[..i],
			None => rest.strip_suffix('\'').unwrap_or(rest),
		},
	};
	let value = value.trim();
	Some((!value.is_empty()).then(|| value.to_owned()))
}

fn starts_field(s: &str) -> bool {
	s.split_once("='").is_some_and(|(key, _)| {
		!key.is_empty() && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
	})
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn title_runs_to_next_field_or_last_quote() {
		let t = |s: &str| parse_title(s.as_bytes()).unwrap();
		assert_eq!(
			t("StreamTitle='Rock';n'Roll';StreamUrl='http://x';\0\0"),
			Some("Rock';n'Roll".into())
		);
		assert_eq!(t("StreamTitle='It';s';"), Some("It';s".into()));
		assert_eq!(t("StreamTitle='  ';"), None);
		assert_eq!(parse_title(b"StreamUrl='x';"), None);
	}

	#[test]
	fn icy_split_across_chunks() {
		let mut wire = b"abcd".to_vec();
		let meta = b"StreamTitle='Hi';";
		wire.push(2);
		wire.extend_from_slice(meta);
		wire.resize(5 + 32, 0);
		wire.extend_from_slice(b"efgh\0ijkl");
		let mut icy = Icy::new(4);
		let (mut audio, mut titles) = (Vec::new(), Vec::new());
		for b in wire {
			icy.feed(Bytes::from(vec![b]), &mut audio, &mut titles);
		}
		assert_eq!(audio.concat(), b"abcdefghijkl");
		assert_eq!(titles, [Some("Hi".to_owned())]);
	}
}
