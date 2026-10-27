use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use bytes::Bytes;
use crossbeam_channel::{Sender, TrySendError};
use parking_lot::Mutex;
use tokio::sync::oneshot;
use tokio::task::AbortHandle;
use tokio::time::{sleep, timeout};

use crate::Result;
use crate::decode::{ByteReader, Decoder, byte_pipe};
use crate::net::{Opened, ResolveCache, open};
use crate::resample::Resampler;
use crate::ring::PcmRing;
use crate::station::Station;

/// Connect, open and the first body bytes must all fit in this.
const START_BUDGET: Duration = Duration::from_secs(5);
/// A connection that sends nothing for this long is treated as lost.
const STALL: Duration = Duration::from_secs(5);
const RING_SECONDS: f32 = 3.0;

#[derive(Clone)]
pub struct EngineCtx {
	pub rt: tokio::runtime::Handle,
	pub client: reqwest::Client,
	pub cache: Arc<Mutex<ResolveCache>>,
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

pub struct StationStream {
	pub ring: Arc<PcmRing>,
	pub station: Station,
	shared: Arc<Shared>,
	task: AbortHandle,
}

impl Drop for StationStream {
	fn drop(&mut self) {
		self.shared.alive.store(false, Ordering::Relaxed);
		// Dropping the task's future drops the pipe sender, so the decode thread reads EOF and exits.
		self.task.abort();
	}
}

struct Shared {
	on_event: Box<dyn Fn(StreamEvent) + Send + Sync>,
	alive: AtomicBool,
	played: AtomicBool,
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
	});
	let task = ctx
		.rt
		.spawn(run(
			station.url.clone(),
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
	response: reqwest::Response,
	first: Bytes,
	metaint: Option<usize>,
	hint: Option<&'static str>,
}

async fn run(url: String, ctx: EngineCtx, ring: Arc<PcmRing>, shared: Arc<Shared>) {
	let mut conn = match connect(&ctx, &url).await {
		Ok(c) => c,
		Err(reason) => return shared.emit(StreamEvent::Failed(reason)),
	};
	let mut title = None;
	// Start of the current outage and retries made in it; only a connection that plays ends it.
	let mut outage: Option<Instant> = None;
	let mut attempt = 0;
	loop {
		let (reason, produced) = play(conn, &ctx, &ring, &shared, &mut title).await;
		if !shared.played.load(Ordering::Relaxed) {
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

/// Opens the cached final URL, else the station URL, and waits for the first body bytes.
async fn connect(ctx: &EngineCtx, url: &str) -> std::result::Result<Conn, String> {
	let attempt = async {
		let cached = ctx.cache.lock().get(url);
		let opened = match cached {
			Some(c) => match open(&ctx.client, &c).await {
				Ok(o) => Ok(o),
				Err(_) => open(&ctx.client, url).await,
			},
			None => open(&ctx.client, url).await,
		};
		let (mut response, final_url) = match opened.map_err(|e| e.to_string())? {
			Opened::Audio {
				response,
				final_url,
			} => (response, final_url),
			// ponytail: HLS lands in Task 7 (HlsReader).
			Opened::Hls { .. } => return Err("HLS".to_owned()),
		};
		let first = match response.chunk().await {
			Ok(Some(c)) => c,
			_ => return Err("No data".to_owned()),
		};
		ctx.cache.lock().put(url, &final_url);
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
		Ok(Conn {
			response,
			first,
			metaint,
			hint,
		})
	};
	timeout(START_BUDGET, attempt)
		.await
		.unwrap_or_else(|_| Err("No data".to_owned()))
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
/// only if the ring runs dry meanwhile, and then `Reconnected` on success.
async fn reconnect(
	url: &str,
	ctx: &EngineCtx,
	ring: &PcmRing,
	shared: &Shared,
	since: Instant,
	attempt: &mut u32,
) -> Option<Conn> {
	let dry = AtomicBool::new(false);
	let watch = async {
		loop {
			if ring.len_ms() == 0 && !dry.swap(true, Ordering::Relaxed) {
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
			if let Ok(conn) = connect(ctx, url).await {
				return Some(conn);
			}
		}
	};
	let conn = tokio::select! {
		c = retry => c,
		_ = watch => unreachable!(),
	};
	if conn.is_some() && dry.load(Ordering::Relaxed) {
		shared.emit(StreamEvent::Reconnected);
	}
	conn
}

/// Plays one connection to its end; returns why it ended and whether it produced audio.
async fn play(
	mut conn: Conn,
	ctx: &EngineCtx,
	ring: &Arc<PcmRing>,
	shared: &Arc<Shared>,
	title: &mut Option<String>,
) -> (String, bool) {
	let (tx, reader) = byte_pipe();
	let (done_tx, done_rx) = oneshot::channel();
	let (thread_ring, thread_shared) = (ring.clone(), shared.clone());
	let (out_rate, hint) = (ctx.out_rate, conn.hint);
	std::thread::Builder::new()
		.name("decode".into())
		.spawn(move || {
			let mut produced = false;
			let result = decode(
				reader,
				hint,
				&thread_ring,
				out_rate,
				&thread_shared,
				&mut produced,
			);
			let _ = done_tx.send((result, produced));
		})
		.expect("spawn decode thread");

	let net_reason = pump(&mut conn, tx, shared, title).await;
	// The sender is gone, so the decoder drains what it has and ends; waiting keeps two
	// connections from pushing into the ring at once.
	match done_rx.await {
		Ok((Err(e), produced)) => (e.to_string(), produced),
		Ok((Ok(()), produced)) => (net_reason, produced),
		Err(_) => (net_reason, false),
	}
}

/// Strips ICY metadata and feeds audio bytes to the decoder until the connection ends.
async fn pump(
	conn: &mut Conn,
	tx: Sender<Bytes>,
	shared: &Shared,
	title: &mut Option<String>,
) -> String {
	let mut icy = conn.metaint.map(Icy::new);
	let mut chunk = std::mem::take(&mut conn.first);
	let mut audio = Vec::new();
	let mut titles = Vec::new();
	loop {
		match &mut icy {
			Some(icy) => icy.feed(chunk, &mut audio, &mut titles),
			None => audio.push(chunk),
		}
		for t in titles.drain(..) {
			if t != *title {
				title.clone_from(&t);
				shared.emit(StreamEvent::Title(t));
			}
		}
		for mut bytes in audio.drain(..) {
			// The decoder keeps up with any live stream; Full only lasts through a burst.
			loop {
				match tx.try_send(bytes) {
					Ok(()) => break,
					Err(TrySendError::Full(back)) => {
						bytes = back;
						sleep(Duration::from_millis(10)).await;
					}
					Err(TrySendError::Disconnected(_)) => return "No data".into(),
				}
			}
		}
		chunk = match timeout(STALL, conn.response.chunk()).await {
			Ok(Ok(Some(c))) => c,
			Ok(Ok(None)) | Err(_) => return "No data".into(),
			Ok(Err(_)) => return "Unreachable".into(),
		};
	}
}

fn decode(
	reader: ByteReader,
	hint: Option<&str>,
	ring: &PcmRing,
	out_rate: u32,
	shared: &Shared,
	produced: &mut bool,
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
		out.clear();
		*produced = true;
		if !shared.played.swap(true, Ordering::Relaxed) {
			shared.emit(StreamEvent::FirstAudio);
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
