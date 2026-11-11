use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use parking_lot::Mutex;

use crate::encoded_ring::EncodedRing;

/// Decoded audio kept behind live for time-shifting, as interleaved stereo
/// i16: about 11 MB per minute at 48 kHz, half the f32 cost. Frames are
/// addressed absolutely (frames pushed since the buffer was created), so a
/// read cursor stays meaningful while the oldest audio is trimmed away.
pub struct ShiftBuffer {
	rate: u32,
	capacity_frames: u64,
	inner: Mutex<ShiftInner>,
	/// Source bytes retained for Original recordings from decoded history.
	pub(crate) encoded: EncodedRing,
}

struct ShiftInner {
	buf: VecDeque<i16>,
	/// Frames ever pushed; the buffer holds `[pushed - len, pushed)`.
	pushed: u64,
}

impl ShiftBuffer {
	pub fn new(rate: u32, minutes: u32) -> ShiftBuffer {
		ShiftBuffer::with_seconds(rate, 60 * minutes.max(1) as u64)
	}

	pub fn with_seconds(rate: u32, seconds: u64) -> ShiftBuffer {
		let rate = rate.max(1);
		ShiftBuffer {
			rate,
			capacity_frames: rate as u64 * seconds.max(1),
			encoded: EncodedRing::new(usize::MAX),
			inner: Mutex::new(ShiftInner {
				buf: VecDeque::new(),
				pushed: 0,
			}),
		}
	}

	pub fn rate(&self) -> u32 {
		self.rate
	}

	/// The absolute frame index just past the newest audio.
	pub fn live_edge(&self) -> u64 {
		self.inner.lock().pushed
	}

	/// The oldest frame still held.
	pub fn start(&self) -> u64 {
		let inner = self.inner.lock();
		inner.pushed - (inner.buf.len() / 2) as u64
	}

	pub fn buffered_seconds(&self) -> f32 {
		(self.live_edge() - self.start()) as f32 / self.rate as f32
	}

	pub fn push(&self, stereo: &[f32]) {
		let frames = (stereo.len() & !1) / 2;
		if frames == 0 {
			return;
		}
		let mut inner = self.inner.lock();
		inner.buf.extend(
			stereo[..frames * 2]
				.iter()
				.map(|s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16),
		);
		inner.pushed += frames as u64;
		let held = (inner.buf.len() / 2) as u64;
		if held > self.capacity_frames {
			inner
				.buf
				.drain(..(held - self.capacity_frames) as usize * 2);
			let start = inner.pushed - (inner.buf.len() / 2) as u64;
			self.encoded.trim_before(start * 1000 / self.rate as u64);
		}
	}

	/// Stamp source bytes on this buffer's frame-time axis, before decoding.
	pub(crate) fn push_encoded(&self, bytes: &[u8]) {
		self.encoded
			.push(self.live_edge() * 1000 / self.rate as u64, bytes);
	}

	/// Copies up to `out.len()/2` frames starting at absolute frame `at` into
	/// `out` as f32; returns samples written. A cursor the trim has passed
	/// reads from the oldest frame still held.
	pub fn read_at(&self, at: u64, out: &mut [f32]) -> usize {
		let inner = self.inner.lock();
		let start = inner.pushed - (inner.buf.len() / 2) as u64;
		let from = at.max(start);
		let skip = ((from - start) as usize) * 2;
		let n = out.len().min(inner.buf.len().saturating_sub(skip)) & !1;
		for (i, o) in out.iter_mut().take(n).enumerate() {
			*o = inner.buf[skip + i] as f32 / i16::MAX as f32;
		}
		n
	}

	/// The newest `seconds` of audio as f32, for song recognition sampling.
	pub fn tail_f32(&self, seconds: f32) -> Vec<f32> {
		let frames = (self.rate as f32 * seconds) as usize;
		let mut out = vec![0.0; frames * 2];
		let edge = self.live_edge();
		let n = self.read_at(edge.saturating_sub(frames as u64), &mut out);
		out.truncate(n);
		out
	}

	/// Everything in the buffer as f32, oldest first (Save Buffer).
	pub fn snapshot_f32(&self) -> Vec<f32> {
		let frames = (self.live_edge() - self.start()) as usize;
		let mut out = vec![0.0; frames * 2];
		let n = self.read_at(self.start(), &mut out);
		out.truncate(n);
		out
	}
}

/// Playback position within a `ShiftBuffer`, written by the mixer on every
/// render and read by the app for the status bar and announcements.
#[derive(Default)]
pub struct ShiftStatus {
	/// Absolute frame the next shifted sample comes from; u64::MAX is live.
	pub cursor: AtomicU64,
	pub paused: AtomicBool,
	pub attached: AtomicBool,
	/// Frames in the buffer the live ring has not played yet, written by the
	/// mixer. Servers burst audio on connect (WNYC about 15 s): it lands here
	/// at once but plays out over time, and stays ahead of live playback.
	pub backlog: AtomicU64,
	/// The replay decoder is still racing to the newest byte, so the window's
	/// edge is short of the truth and the backlog cannot be taken off it.
	pub catching_up: AtomicBool,
	/// Counts playhead jumps (seek, attach, back to live): odd while one
	/// is being written, see `jump`. The live playhead also steps back a
	/// little between mixer passes (the edge grows on each decoder push,
	/// the backlog only per pass). Original recording re-anchors on a
	/// jump, never on that noise.
	pub seeks: AtomicU64,
}

impl ShiftStatus {
	pub fn new() -> ShiftStatus {
		ShiftStatus {
			cursor: AtomicU64::new(u64::MAX),
			paused: AtomicBool::new(false),
			attached: AtomicBool::new(false),
			backlog: AtomicU64::new(0),
			catching_up: AtomicBool::new(false),
			seeks: AtomicU64::new(0),
		}
	}

	/// Moves the playhead as a jump: seqlock, so a reader never pairs
	/// the new count with the old cursor (see `jumps`).
	pub fn jump(&self, cursor: u64) {
		self.seeks.fetch_add(1, Ordering::Relaxed);
		std::sync::atomic::fence(Ordering::Release);
		self.cursor.store(cursor, Ordering::Relaxed);
		self.seeks.fetch_add(1, Ordering::Release);
	}

	/// Reads `read` against the jump count: `Some((count, value))` when
	/// no jump was written meanwhile, None while one is (try again).
	pub fn jumps<T>(&self, read: impl FnOnce() -> T) -> Option<(u64, T)> {
		let before = self.seeks.load(Ordering::Acquire);
		if before % 2 == 1 {
			return None;
		}
		let value = read();
		std::sync::atomic::fence(Ordering::Acquire);
		(self.seeks.load(Ordering::Relaxed) == before).then_some((before, value))
	}

	/// The frame live playback stands at: what the ring plays next, not the
	/// newest audio received.
	pub fn live_frame(&self, buffer: &ShiftBuffer) -> u64 {
		let edge = buffer.live_edge();
		if self.catching_up.load(Ordering::Relaxed) {
			return edge;
		}
		edge.saturating_sub(self.backlog.load(Ordering::Relaxed))
	}

	/// Seconds behind live; 0 at the live edge.
	pub fn behind_seconds(&self, buffer: &ShiftBuffer) -> f32 {
		let cursor = self.cursor.load(Ordering::Relaxed);
		if cursor == u64::MAX {
			return 0.0;
		}
		self.live_frame(buffer).saturating_sub(cursor) as f32 / buffer.rate() as f32
	}

	/// How far behind live the buffer's oldest audio is.
	pub fn depth_seconds(&self, buffer: &ShiftBuffer) -> f32 {
		self.live_frame(buffer).saturating_sub(buffer.start()) as f32 / buffer.rate() as f32
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn encoded_history_keeps_the_same_time_range_as_decoded_audio() {
		let buffer = ShiftBuffer::with_seconds(1, 2);
		for byte in 0..5 {
			buffer.push_encoded(&[byte]);
			buffer.push(&[0.0, 0.0]);
		}
		assert_eq!(buffer.start(), 3);
		assert_eq!(buffer.encoded.span(0, u64::MAX), [3, 4]);
	}

	fn ramp(from: usize, frames: usize) -> Vec<f32> {
		(from..from + frames)
			.flat_map(|i| [i as f32 / 32768.0, i as f32 / 32768.0])
			.collect()
	}

	#[test]
	fn snapshot_holds_the_whole_buffer_oldest_first() {
		let b = ShiftBuffer::new(1000, 1);
		b.push(&ramp(0, 500));
		let snap = b.snapshot_f32();
		assert_eq!(snap.len(), 1000);
		assert!((snap[0]).abs() < 0.001);
		assert!((snap[998] - 499.0 / 32767.0).abs() < 0.001);
		b.push(&ramp(500, 59_600)); // overflows the 60 s ring: oldest dropped
		let snap = b.snapshot_f32();
		assert_eq!(snap.len(), 120_000);
		assert!((snap[0] - 100.0 / 32767.0).abs() < 0.01);
	}

	#[test]
	fn push_read_round_trip() {
		let b = ShiftBuffer::new(1000, 1);
		b.push(&ramp(0, 500));
		assert_eq!(b.live_edge(), 500);
		assert_eq!(b.start(), 0);
		let mut out = [0.0; 4];
		assert_eq!(b.read_at(250, &mut out), 4);
		assert!((out[0] - 250.0 / 32767.0).abs() < 0.001);
	}

	#[test]
	fn trim_drops_oldest_and_cursor_clamps() {
		let b = ShiftBuffer::new(1000, 1); // 60_000 frames
		b.push(&ramp(0, 70_000));
		assert_eq!(b.live_edge(), 70_000);
		assert_eq!(b.start(), 10_000);
		let mut out = [0.0; 2];
		b.read_at(0, &mut out); // cursor behind the trim reads the oldest held
		assert!((out[0] - 10_000.0 / 32767.0).abs() < 0.001);
	}

	#[test]
	fn tail_returns_newest() {
		let b = ShiftBuffer::new(1000, 1);
		b.push(&ramp(0, 3000));
		let tail = b.tail_f32(1.0);
		assert_eq!(tail.len(), 2000);
		assert!((tail[0] - 2000.0 / 32767.0).abs() < 0.001);
	}

	#[test]
	fn behind_seconds_tracks_cursor() {
		let b = ShiftBuffer::new(1000, 1);
		b.push(&ramp(0, 10_000));
		let s = ShiftStatus::new();
		assert_eq!(s.behind_seconds(&b), 0.0);
		s.cursor.store(4_000, Ordering::Relaxed);
		assert_eq!(s.behind_seconds(&b), 6.0);
		// 2 s the ring has yet to play: live stands at frame 8000, not at the edge.
		s.backlog.store(2_000, Ordering::Relaxed);
		assert_eq!(s.behind_seconds(&b), 4.0);
		assert_eq!(s.depth_seconds(&b), 8.0);
		// A replay window still decoding has no backlog to take off yet.
		s.catching_up.store(true, Ordering::Relaxed);
		assert_eq!(s.behind_seconds(&b), 6.0);
	}
}
