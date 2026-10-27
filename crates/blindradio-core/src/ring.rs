use std::collections::VecDeque;

use parking_lot::Mutex;

/// Interleaved stereo f32 buffer; `rate` is frames per second.
pub struct PcmRing {
	rate: u32,
	capacity: usize,
	// ponytail: mutex shared with audio callback, held microseconds; lock-free ring if underruns show up
	buf: Mutex<VecDeque<f32>>,
}

impl PcmRing {
	pub fn new(rate: u32, seconds: f32) -> PcmRing {
		let capacity = (rate as f32 * seconds) as usize * 2;
		PcmRing { rate, capacity, buf: Mutex::new(VecDeque::with_capacity(capacity)) }
	}

	pub fn push(&self, stereo: &[f32]) {
		let stereo = &stereo[stereo.len().saturating_sub(self.capacity)..];
		let mut buf = self.buf.lock();
		let excess = (buf.len() + stereo.len()).saturating_sub(self.capacity);
		buf.drain(..excess);
		buf.extend(stereo);
	}

	pub fn pop_into(&self, out: &mut [f32]) -> usize {
		let mut buf = self.buf.lock();
		let n = out.len().min(buf.len());
		for (o, s) in out.iter_mut().zip(buf.drain(..n)) {
			*o = s;
		}
		n
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
		(from..from + frames).flat_map(|i| [i as f32, i as f32]).collect()
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
		r.push(&vec![0.0; 48000 * 2 / 2]);
		assert_eq!(r.len_ms(), 500);
	}
}
