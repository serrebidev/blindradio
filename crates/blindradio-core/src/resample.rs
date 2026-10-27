use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Async, FixedAsync, Indexing, PolynomialDegree, Resampler as _};

/// Input frames per rubato call: small, so buffered input adds at most ~1.5 ms of latency.
const CHUNK: usize = 64;

/// Interleaved stereo f32 rate converter.
pub struct Resampler {
	inner: Option<Async<f32>>,
	pending: Vec<f32>,
}

impl Resampler {
	pub fn new(in_rate: u32, out_rate: u32) -> Resampler {
		let inner = (in_rate != out_rate).then(|| {
			let ratio = f64::from(out_rate) / f64::from(in_rate);
			Async::new_poly(
				ratio,
				1.0,
				PolynomialDegree::Cubic,
				CHUNK,
				2,
				FixedAsync::Input,
			)
			.expect("sample rates must be non-zero")
		});
		Resampler {
			inner,
			pending: Vec::new(),
		}
	}

	pub fn process(&mut self, stereo_in: &[f32], out: &mut Vec<f32>) {
		let Some(inner) = &mut self.inner else {
			out.extend_from_slice(stereo_in);
			return;
		};
		self.pending.extend_from_slice(stereo_in);
		let frames = self.pending.len() / 2;
		let input =
			InterleavedSlice::new(&self.pending[..frames * 2], 2, frames).expect("stereo input");
		let mut used = 0;
		while frames - used >= inner.input_frames_next() {
			let start = out.len();
			let max = inner.output_frames_max();
			out.resize(start + max * 2, 0.0);
			let mut output =
				InterleavedSlice::new_mut(&mut out[start..], 2, max).expect("stereo output");
			let indexing = Indexing {
				input_offset: used,
				..Indexing::new()
			};
			let (read, written) = inner
				.process_into_buffer(&input, &mut output, Some(&indexing))
				.expect("buffers sized by the resampler");
			used += read;
			out.truncate(start + written * 2);
		}
		self.pending.drain(..used * 2);
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn sine(frames: usize, rate: f32) -> Vec<f32> {
		(0..frames)
			.flat_map(|i| {
				let s = (i as f32 * 440.0 * std::f32::consts::TAU / rate).sin() * 0.5;
				[s, s]
			})
			.collect()
	}

	#[test]
	fn resample_keeps_duration() {
		let input = sine(44100, 44100.0);
		let mut r = Resampler::new(44100, 48000);
		let mut out = Vec::new();
		// Packet-sized pieces, as the decoder hands them over.
		for chunk in input.chunks(1152 * 2) {
			r.process(chunk, &mut out);
		}
		let frames = out.len() / 2;
		assert!(frames.abs_diff(48000) <= 64, "{frames} frames");
	}

	#[test]
	fn resample_passthrough_is_identity() {
		let input = sine(1000, 48000.0);
		let mut r = Resampler::new(48000, 48000);
		let mut out = Vec::new();
		r.process(&input, &mut out);
		assert_eq!(out, input);
	}
}
