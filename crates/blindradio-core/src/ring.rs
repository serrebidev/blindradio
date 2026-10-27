use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use parking_lot::Mutex;

/// Interleaved stereo f32 buffer; `rate` is frames per second.
pub struct PcmRing {
	rate: u32,
	capacity: usize,
	// ponytail: mutex shared with audio callback, held microseconds; lock-free ring if underruns show up
	buf: Mutex<VecDeque<f32>>,
	/// Frames lost to overflow, for diagnosing skips.
	dropped: AtomicU64,
	born: Instant,
	/// Milliseconds after `born` of the last `pop_into`, or `NEVER`.
	last_pop_ms: AtomicU64,
}

/// A ring popped this recently is being played.
const DRAIN_GRACE: Duration = Duration::from_millis(200);
/// How long a push into a full, playing ring waits before trying again.
const PUSH_WAIT: Duration = Duration::from_millis(5);
const NEVER: u64 = u64::MAX;

impl PcmRing {
	pub fn new(rate: u32, seconds: f32) -> PcmRing {
		let rate = rate.max(1);
		let capacity = (rate as f32 * seconds) as usize * 2;
		PcmRing {
			rate,
			capacity,
			buf: Mutex::new(VecDeque::with_capacity(capacity)),
			dropped: AtomicU64::new(0),
			born: Instant::now(),
			last_pop_ms: AtomicU64::new(NEVER),
		}
	}

	/// Whole frames only: a trailing odd sample is dropped so left and right never swap.
	/// While something drains the ring (the playing station), a full ring makes the caller
	/// wait for room, so a burst faster than real time is never thrown away. An undrained
	/// ring (a warm neighbour) drops its oldest audio instead, staying at the live edge.
	pub fn push(&self, stereo: &[f32]) {
		let mut rest = &stereo[..stereo.len() & !1];
		while !rest.is_empty() {
			{
				let mut buf = self.buf.lock();
				let room = self.capacity - buf.len();
				let n = room.min(rest.len());
				buf.extend(&rest[..n]);
				rest = &rest[n..];
				if rest.is_empty() {
					return;
				}
				if !self.draining() {
					let cut = rest.len().saturating_sub(self.capacity);
					let rest = &rest[cut..];
					let excess = (buf.len() + rest.len()).saturating_sub(self.capacity);
					buf.drain(..excess);
					self.dropped
						.fetch_add(((cut + excess) / 2) as u64, Ordering::Relaxed);
					buf.extend(rest);
					return;
				}
			}
			std::thread::sleep(PUSH_WAIT);
		}
	}

	/// Popped from within `DRAIN_GRACE`: someone is playing this ring.
	fn draining(&self) -> bool {
		let last = self.last_pop_ms.load(Ordering::Relaxed);
		last != NEVER && self.ms_now().saturating_sub(last) < DRAIN_GRACE.as_millis() as u64
	}

	fn ms_now(&self) -> u64 {
		self.born.elapsed().as_millis() as u64
	}

	pub fn pop_into(&self, out: &mut [f32]) -> usize {
		self.last_pop_ms.store(self.ms_now(), Ordering::Relaxed);
		let mut buf = self.buf.lock();
		let n = out.len().min(buf.len()) & !1;
		for (o, s) in out.iter_mut().zip(buf.drain(..n)) {
			*o = s;
		}
		n
	}

	pub fn dropped_frames(&self) -> u64 {
		self.dropped.load(Ordering::Relaxed)
	}

	pub fn len_ms(&self) -> u32 {
		(self.buf.lock().len() as u64 / 2 * 1000 / self.rate as u64) as u32
	}

	pub fn trim_to_ms(&self, keep_ms: u32) {
		let keep = (self.rate as u64 * keep_ms as u64 / 1000) as usize * 2;
		let mut buf = self.buf.lock();
		let excess = buf.len().saturating_sub(keep);
		buf.drain(..excess);
	}

	pub fn clear(&self) {
		self.buf.lock().clear();
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
	fn push_beyond_capacity_drops_oldest() {
		let r = PcmRing::new(1000, 3.0);
		r.push(&ramp(0, 4000));
		assert_eq!(r.len_ms(), 3000);
		let mut out = [0.0; 2];
		r.pop_into(&mut out);
		assert_eq!(out, [1000.0, 1000.0]);
	}

	#[test]
	fn push_in_chunks_drops_oldest() {
		let r = PcmRing::new(1000, 3.0);
		for s in 0..4 {
			r.push(&ramp(s * 1000, 1000));
		}
		let mut out = [0.0; 2];
		r.pop_into(&mut out);
		assert_eq!(out[0], 1000.0);
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
		r.pop_into(&mut out);
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
		assert_eq!(r.dropped_frames(), 0);
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
