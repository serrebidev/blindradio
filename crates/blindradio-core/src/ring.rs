use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::{Duration, Instant};

use parking_lot::Mutex;

/// How long a push into a full ring waits before trying again.
const PUSH_WAIT: Duration = Duration::from_millis(5);

/// Audio a ring holds before it starts playing, after a cold start, a warm swap or running
/// dry. Some servers dribble their first seconds in lumps up to a second apart (amperwave);
/// burst servers reach it within a few ms of first audio, so it costs them almost nothing.
pub const CUSHION_MS: u32 = 1000;
/// The most a stream that keeps running dry must refill before it may sound again, by default.
pub const REFILL_LIMIT_MS: u32 = 15_000;
/// How many segments behind the newest a live HLS stream starts, by default.
pub const HLS_BEHIND: u32 = 2;

static START_NOW: AtomicU32 = AtomicU32::new(CUSHION_MS);
static REFILL_NOW: AtomicU32 = AtomicU32::new(REFILL_LIMIT_MS);
static HLS_BEHIND_NOW: AtomicU32 = AtomicU32::new(HLS_BEHIND);

/// The Buffer settings. Process-wide, as the streams and the mixer that read them are made in
/// many places; a stream already open keeps the ring it was made with.
pub fn set_buffers(start_ms: u32, refill_limit_ms: u32, hls_behind: u32) {
	START_NOW.store(start_ms, Ordering::Relaxed);
	REFILL_NOW.store(refill_limit_ms, Ordering::Relaxed);
	HLS_BEHIND_NOW.store(hls_behind, Ordering::Relaxed);
}

/// Audio a ring holds before it starts playing.
pub fn start_ms() -> u32 {
	START_NOW.load(Ordering::Relaxed)
}

pub fn refill_limit_ms() -> u32 {
	REFILL_NOW.load(Ordering::Relaxed)
}

pub fn hls_behind() -> usize {
	HLS_BEHIND_NOW.load(Ordering::Relaxed) as usize
}

/// Interleaved stereo f32 buffer; `rate` is frames per second.
pub struct PcmRing {
	rate: u32,
	capacity: usize,
	// ponytail: mutex shared with audio callback, held microseconds; lock-free ring if underruns show up
	buf: Mutex<VecDeque<f32>>,
	closed: AtomicBool,
	/// When the last push ended, and the longest wait between pushes.
	lumps: Mutex<(Option<Instant>, u32)>,
}

impl PcmRing {
	pub fn new(rate: u32, seconds: f32) -> PcmRing {
		let rate = rate.max(1);
		let capacity = (rate as f32 * seconds) as usize * 2;
		PcmRing {
			rate,
			capacity,
			buf: Mutex::new(VecDeque::with_capacity(capacity)),
			closed: AtomicBool::new(false),
			lumps: Mutex::new((None, 0)),
		}
	}

	/// Whole frames only: a trailing odd sample is dropped so left and right never swap.
	/// A full ring makes the caller wait for room, so no audio is ever thrown away: streams
	/// that deliver in lumps (HLS segments, large Ogg pages, connect bursts) stay continuous,
	/// and a warm neighbour pauses its connection instead of skipping ahead. Once the ring is
	/// closed, what does not fit is discarded so the producer can finish.
	pub fn push(&self, stereo: &[f32]) {
		{
			let mut lumps = self.lumps.lock();
			if let Some(end) = lumps.0 {
				let gap = end.elapsed().as_millis().min(u128::from(u32::MAX)) as u32;
				lumps.1 = lumps.1.max(gap);
			}
		}
		let mut rest = &stereo[..stereo.len() & !1];
		loop {
			{
				let mut buf = self.buf.lock();
				let n = (self.capacity - buf.len()).min(rest.len());
				buf.extend(&rest[..n]);
				rest = &rest[n..];
			}
			if rest.is_empty() || self.closed.load(Ordering::Relaxed) {
				break;
			}
			std::thread::sleep(PUSH_WAIT);
		}
		// After the wait for room: a full ring is no gap in the stream.
		self.lumps.lock().0 = Some(Instant::now());
	}

	/// No one will read this ring again: pushes stop waiting.
	pub fn close(&self) {
		self.closed.store(true, Ordering::Relaxed);
	}

	pub fn pop_into(&self, out: &mut [f32]) -> usize {
		let mut buf = self.buf.lock();
		let n = out.len().min(buf.len()) & !1;
		for (o, s) in out.iter_mut().zip(buf.drain(..n)) {
			*o = s;
		}
		n
	}

	pub fn len_ms(&self) -> u32 {
		(self.frames() * 1000 / self.rate as u64) as u32
	}

	pub fn frames(&self) -> u64 {
		self.buf.lock().len() as u64 / 2
	}

	/// Drops all but the newest `keep_ms`, or more if this stream arrives in lumps further apart
	/// than that (HLS segments): trimming below the wait for the next lump only buys a gap, and
	/// each gap makes the mixer demand a deeper refill.
	pub fn trim_to_ms(&self, keep_ms: u32) {
		let keep_ms = keep_ms.max(self.lumps.lock().1);
		let keep = (self.rate as u64 * keep_ms as u64 / 1000) as usize * 2;
		let mut buf = self.buf.lock();
		let excess = buf.len().saturating_sub(keep);
		buf.drain(..excess);
	}

	pub fn clear(&self) {
		self.buf.lock().clear();
	}

	#[cfg(test)]
	pub(crate) fn is_closed(&self) -> bool {
		self.closed.load(Ordering::Relaxed)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn ramp(from: usize, frames: usize) -> Vec<f32> {
		(from..from + frames)
			.flat_map(|i| [i as f32, i as f32])
			.collect()
	}

	#[test]
	fn closed_ring_discards_what_does_not_fit() {
		let r = PcmRing::new(1000, 3.0);
		r.close();
		r.push(&ramp(0, 4000));
		assert_eq!(r.len_ms(), 3000);
		let mut out = [0.0; 2];
		r.pop_into(&mut out);
		assert_eq!(out, [0.0, 0.0], "the oldest audio is kept");
	}

	#[test]
	fn full_ring_waits_until_closed() {
		let r = std::sync::Arc::new(PcmRing::new(10, 1.0));
		let pusher = {
			let r = r.clone();
			std::thread::spawn(move || r.push(&ramp(0, 30)))
		};
		std::thread::sleep(Duration::from_millis(50));
		assert!(!pusher.is_finished(), "a warm ring pauses its producer");
		r.close();
		pusher.join().unwrap();
		assert_eq!(r.len_ms(), 1000);
	}

	#[test]
	fn trim_keeps_newest() {
		let r = PcmRing::new(1000, 3.0);
		r.push(&ramp(0, 1000));
		r.trim_to_ms(250);
		assert_eq!(r.len_ms(), 250);
		let mut out = [0.0; 2];
		r.pop_into(&mut out);
		assert_eq!(out[0], 750.0);
	}

	#[test]
	fn trim_keeps_enough_to_reach_the_next_lump() {
		let r = PcmRing::new(1000, 3.0);
		r.push(&ramp(0, 1000));
		std::thread::sleep(Duration::from_millis(400));
		r.push(&ramp(1000, 1000));
		r.trim_to_ms(100);
		let held = r.len_ms();
		assert!(held >= 400, "held {held} ms, against a 400 ms wait for audio");
		assert!(held < 1000, "held {held} ms: still trimmed");
	}

	#[test]
	fn pop_partial_returns_count() {
		let r = PcmRing::new(1000, 1.0);
		r.push(&[1.0, 2.0, 3.0, 4.0]);
		let mut out = [9.0; 6];
		assert_eq!(r.pop_into(&mut out), 4);
		assert_eq!(out, [1.0, 2.0, 3.0, 4.0, 9.0, 9.0]);
		r.clear();
		assert_eq!(r.pop_into(&mut out), 0);
	}

	#[test]
	fn len_ms_counts_stereo_frames() {
		let r = PcmRing::new(48000, 3.0);
		// 24000 frames of stereo is half a second at 48 kHz.
		r.push(&vec![0.0; 24000 * 2]);
		assert_eq!(r.len_ms(), 500);
	}

	#[test]
	fn playing_ring_waits_instead_of_dropping() {
		let r = std::sync::Arc::new(PcmRing::new(10, 1.0));
		let mut out = [0.0; 2];
		let pusher = {
			let r = r.clone();
			std::thread::spawn(move || r.push(&ramp(0, 30)))
		};
		let mut got = Vec::new();
		while got.len() < 60 {
			let n = r.pop_into(&mut out);
			got.extend_from_slice(&out[..n]);
			std::thread::sleep(Duration::from_millis(1));
		}
		pusher.join().unwrap();
		assert_eq!(got, ramp(0, 30), "every frame arrives, in order");
	}

	#[test]
	fn odd_lengths_keep_whole_frames() {
		let r = PcmRing::new(1000, 1.0);
		r.push(&[1.0, 2.0, 3.0]);
		r.push(&[4.0, 5.0]);
		let mut out = [0.0; 3];
		assert_eq!(r.pop_into(&mut out), 2);
		assert_eq!(out[..2], [1.0, 2.0]);
		assert_eq!(r.pop_into(&mut out), 2);
		assert_eq!(out[..2], [4.0, 5.0], "left stays left");
		assert_eq!(PcmRing::new(0, 1.0).len_ms(), 0);
	}
}
