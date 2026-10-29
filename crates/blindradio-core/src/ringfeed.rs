//! The encoded ring's replay feed: decodes ring bytes into a PCM
//! window the mixer plays from, so pause, seek and the status bar keep
//! working exactly as they do with the standard timeshift buffer —
//! only the window is a small sliding slice of a much deeper encoded
//! history. Seeks or resumes that land before the window re-prime it
//! at the target (the tuner drives that through `prime`).

use std::io::Read;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use parking_lot::Mutex;

use crate::decode::Decoder;
use crate::encoded_ring::EncodedRing;
use crate::record::Recorder;
use crate::resample::Resampler;
use crate::stream::RingTap;
use crate::timeshift::{ShiftBuffer, ShiftStatus};

/// How much decoded audio the window holds, in seconds. Seeks inside
/// it are instant; deeper jumps re-prime. 180 s at 48 kHz is about
/// 33 MB, against hours of encoded history behind it.
pub const WINDOW_SECONDS: u64 = 180;

/// Where the live edge sits inside the window before a "go live"
/// re-prime, so a step back right after still lands in the window.
const LIVE_MARGIN_MS: u64 = 30_000;

/// A window's frame/time mapping: frame 0 of the window is `start_ms`
/// on the ring's session-time axis.
#[derive(Clone, Copy)]
pub struct WindowMap {
	pub start_ms: u64,
	pub rate: u32,
}

impl WindowMap {
	pub fn time_at(&self, frame: u64) -> u64 {
		self.start_ms + frame * 1000 / self.rate.max(1) as u64
	}

	pub fn frame_at(&self, time_ms: u64) -> u64 {
		time_ms.saturating_sub(self.start_ms) * self.rate.max(1) as u64 / 1000
	}
}

struct FeedCore {
	ring: Arc<EncodedRing>,
	tap: Arc<RingTap>,
	status: Arc<ShiftStatus>,
	record_slot: Option<Arc<Mutex<Option<Recorder>>>>,
	rate: u32,
	map: Mutex<WindowMap>,
	window: Mutex<Arc<ShiftBuffer>>,
	generation: Mutex<u64>,
	dead: Mutex<Arc<AtomicBool>>,
	thread: Mutex<Option<JoinHandle<()>>>,
}

pub struct RingFeed {
	core: Arc<FeedCore>,
}

impl RingFeed {
	pub fn new(
		ring: Arc<EncodedRing>,
		tap: Arc<RingTap>,
		status: Arc<ShiftStatus>,
		record_slot: Option<Arc<Mutex<Option<Recorder>>>>,
		rate: u32,
	) -> RingFeed {
		let feed = RingFeed {
			core: Arc::new(FeedCore {
				ring,
				tap,
				status,
				record_slot,
				rate,
				map: Mutex::new(WindowMap { start_ms: 0, rate }),
				window: Mutex::new(Arc::new(ShiftBuffer::with_seconds(
					rate,
					WINDOW_SECONDS,
				))),
				generation: Mutex::new(0),
				dead: Mutex::new(Arc::new(AtomicBool::new(true))),
				thread: Mutex::new(None),
			}),
		};
		feed.prime_inner(0, false);
		feed
	}

	pub fn status(&self) -> Arc<ShiftStatus> {
		self.core.status.clone()
	}

	pub fn window(&self) -> Arc<ShiftBuffer> {
		self.core.window.lock().clone()
	}

	pub fn window_start_ms(&self) -> u64 {
		self.core.map.lock().start_ms
	}

	/// Session time now, on the ring's axis.
	pub fn now_ms(&self) -> u64 {
		self.core.tap.now_ms()
	}

	/// The oldest time the ring still holds.
	pub fn ring_start_ms(&self) -> u64 {
		self.core.ring.start_ms().unwrap_or(0)
	}

	/// The playhead's session time: the mixer's cursor mapped through
	/// the window, or now while playing live.
	pub fn cursor_time(&self) -> u64 {
		let cursor = self.core.status.cursor.load(Ordering::Relaxed);
		if cursor == u64::MAX {
			return self.now_ms();
		}
		self.core.map.lock().time_at(cursor)
	}

	/// Restarts the window at `target_ms`: a fresh buffer whose frame 0
	/// is the target, decoded from the ring. Blocks briefly until the
	/// first audio lands (or the decoder gives up), so a following
	/// seek command in the mixer clamps against a non-empty buffer.
	/// Returns the new window for the tuner to attach.
	pub fn prime(&self, target_ms: u64) -> Arc<ShiftBuffer> {
		self.prime_inner(target_ms, true)
	}

	fn prime_inner(&self, target_ms: u64, wait: bool) -> Arc<ShiftBuffer> {
		self.kill_thread();
		let start = match self.core.ring.start_ms() {
			Some(ring_start) => target_ms.max(ring_start),
			None => target_ms,
		};
		let buffer = Arc::new(ShiftBuffer::with_seconds(self.core.rate, WINDOW_SECONDS));
		*self.core.map.lock() = WindowMap {
			start_ms: start,
			rate: self.core.rate,
		};
		*self.core.window.lock() = buffer.clone();
		let dead = Arc::new(AtomicBool::new(false));
		*self.core.dead.lock() = dead.clone();
		*self.core.generation.lock() += 1;
		let core = self.core.clone();
		let handle = thread::spawn(move || feed_loop(core, dead, start, buffer.clone()));
		*self.core.thread.lock() = Some(handle);
		if !wait {
			return self.window();
		}
		// Wait for the first half second of audio (or a dead thread).
		let buffer = self.window();
		for _ in 0..150 {
			if buffer.live_edge() >= self.core.rate as u64 / 2 {
				break;
			}
			if self.core.thread.lock().as_ref().is_some_and(|h| h.is_finished()) {
				break;
			}
			thread::sleep(Duration::from_millis(10));
		}
		buffer
	}

	/// Re-prime near the live edge for a "go live": the window starts a
	/// margin behind now and decoding races forward from there.
	pub fn prime_live(&self) -> Arc<ShiftBuffer> {
		self.prime_inner(self.now_ms().saturating_sub(LIVE_MARGIN_MS), false)
	}

	pub fn stop(&self) {
		self.kill_thread();
	}

	fn kill_thread(&self) {
		self.core.dead.lock().store(true, Ordering::Relaxed);
		if let Some(handle) = self.core.thread.lock().take() {
			let _ = handle.join();
		}
	}
}

/// Serves the ring's bytes in order from `from_ms`, waiting at the
/// live edge for more. Returning 0 (EOF) happens only when killed, so
/// a decoder reading from it ends and its thread can be joined.
struct RingReader {
	ring: Arc<EncodedRing>,
	from_ms: u64,
	seq: Option<u64>,
	buf: Vec<u8>,
	pos: usize,
	dead: Arc<AtomicBool>,
}

impl Read for RingReader {
	fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
		loop {
			if self.pos < self.buf.len() {
				let n = (self.buf.len() - self.pos).min(out.len());
				out[..n].copy_from_slice(&self.buf[self.pos..self.pos + n]);
				self.pos += n;
				return Ok(n);
			}
			if self.dead.load(Ordering::Relaxed) {
				return Ok(0);
			}
			let (bytes, seq) = match self.seq {
				None => match self.ring.serve_from(self.from_ms) {
					Some(found) => found,
					None => (Vec::new(), 0),
				},
				Some(seq) => self.ring.bytes_after_seq(seq),
			};
			if !bytes.is_empty() {
				self.seq = Some(seq);
				self.buf = bytes;
				self.pos = 0;
				continue;
			}
			thread::sleep(Duration::from_millis(50));
		}
	}
}

fn feed_loop(core: Arc<FeedCore>, dead: Arc<AtomicBool>, from_ms: u64, window: Arc<ShiftBuffer>) {
	let reader = RingReader {
		ring: core.ring.clone(),
		from_ms,
		seq: None,
		buf: Vec::new(),
		pos: 0,
		dead: dead.clone(),
	};
	// No format hint: symphonia probes the ring bytes themselves.
	let Ok(mut decoder) = Decoder::new(Box::new(reader), None) else {
		return;
	};
	let mut rate = decoder.sample_rate();
	let mut resampler = Resampler::new(rate, core.rate);
	let capacity = WINDOW_SECONDS * core.rate as u64;
	let (mut pcm, mut out) = (Vec::new(), Vec::new());
	// Original recordings follow the playhead: the last ring chunk
	// written to the recorder. None while not recording shifted audio.
	let mut rec_after: Option<u64> = None;
	loop {
		if dead.load(Ordering::Relaxed) {
			return;
		}
		// An Original recording takes the ring chunks the cursor has
		// passed, finished chunks only, each exactly once. (PCM
		// formats are tapped in the mixer; their recorders ignore
		// write_bytes, and an Original recorder ignores the mixer's
		// PCM, so the two never mix.)
		if let Some(slot) = &core.record_slot {
			let cursor = core.status.cursor.load(Ordering::Relaxed);
			let mut guard = slot.lock();
			if let Some(rec) = guard.as_mut()
				&& cursor != u64::MAX
			{
				let t = core.map.lock().time_at(cursor);
				let after = match rec_after {
					Some(a) => a,
					None => {
						// Start at the chunk the playhead sits in; the
						// sub(1) lets that chunk be written once it
						// ends (the session's very first chunk is the
						// one exception — it is skipped).
						let a = core.ring.seq_at(t).map_or(0, |s| s.saturating_sub(1));
						rec_after = Some(a);
						a
					}
				};
				let (bytes, last) = core.ring.chunks_ended_after_seq(after, t);
				if !bytes.is_empty() {
					let _ = rec.write_bytes(&bytes);
					rec_after = Some(last);
				}
			} else {
				rec_after = None;
			}
		}
		// Throttle: never decode far ahead of the mixer's cursor — the
		// window would slide out from under it. While live (cursor at
		// the edge marker) there is nothing to protect; the reader
		// paces us at the live edge instead.
		let cursor = core.status.cursor.load(Ordering::Relaxed);
		if cursor != u64::MAX {
			let ahead = window.live_edge().saturating_sub(cursor);
			if ahead > capacity * 9 / 10 {
				thread::sleep(Duration::from_millis(100));
				continue;
			}
		}
		match decoder.next_stereo(&mut pcm) {
			Ok(true) => {
				if decoder.sample_rate() != rate {
					rate = decoder.sample_rate();
					resampler = Resampler::new(rate, core.rate);
				}
				resampler.process(&pcm, &mut out);
				pcm.clear();
				if !out.is_empty() {
					window.push(&out);
					out.clear();
				}
			}
			// EOF is the kill path; an error means these bytes will
			// never decode — either way this generation is done.
			_ => return,
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn window_map_round_trips() {
		let map = WindowMap {
			start_ms: 60_000,
			rate: 48_000,
		};
		assert_eq!(map.time_at(0), 60_000);
		assert_eq!(map.time_at(48_000), 61_000);
		assert_eq!(map.frame_at(61_000), 48_000);
		assert_eq!(map.frame_at(30_000), 0);
	}
}
