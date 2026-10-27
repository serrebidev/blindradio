//! Three-band equalizer: RBJ low shelf, peaking mid and high shelf, per channel.

use std::f64::consts::PI;

use crate::settings::{EqPreset, EqSettings};

const LOW_HZ: f64 = 120.0;
const MID_HZ: f64 = 1000.0;
const MID_Q: f64 = 0.9;
const HIGH_HZ: f64 = 6000.0;
const MAX_DB: i8 = 12;

impl EqPreset {
	/// Band gains in dB (bass, mid, treble); `None` for Custom.
	pub fn bands(self) -> Option<(i8, i8, i8)> {
		match self {
			EqPreset::Flat => Some((0, 0, 0)),
			EqPreset::BassBoost => Some((6, 0, 0)),
			EqPreset::TrebleBoost => Some((0, 0, 6)),
			EqPreset::Loudness => Some((5, -1, 4)),
			EqPreset::Voice => Some((-3, 4, 1)),
			EqPreset::Warm => Some((3, 1, -2)),
			EqPreset::Custom => None,
		}
	}

	pub fn label(self) -> &'static str {
		match self {
			EqPreset::Flat => "Flat",
			EqPreset::BassBoost => "Bass Boost",
			EqPreset::TrebleBoost => "Treble Boost",
			EqPreset::Loudness => "Loudness",
			EqPreset::Voice => "Voice",
			EqPreset::Warm => "Warm",
			EqPreset::Custom => "Custom",
		}
	}
}

pub fn describe(s: &EqSettings) -> String {
	format!(
		"Bass {} dB, Mid {} dB, Treble {} dB",
		s.bass, s.mid, s.treble
	)
}

#[derive(Clone, Copy, Default)]
struct Biquad {
	b0: f64,
	b1: f64,
	b2: f64,
	a1: f64,
	a2: f64,
	/// Transposed direct form II state per channel.
	z: [[f64; 2]; 2],
}

#[derive(Clone, Copy)]
enum Kind {
	LowShelf,
	Peak,
	HighShelf,
}

impl Biquad {
	/// Takes new coefficients and keeps the state, so a change does not click.
	fn tune(&mut self, kind: Kind, rate: f64, hz: f64, db: f64) {
		let a = 10f64.powf(db / 40.0);
		let w = 2.0 * PI * hz / rate;
		let (sin, cos) = w.sin_cos();
		let (b0, b1, b2, a0, a1, a2);
		match kind {
			Kind::Peak => {
				let alpha = sin / (2.0 * MID_Q);
				b0 = 1.0 + alpha * a;
				b1 = -2.0 * cos;
				b2 = 1.0 - alpha * a;
				a0 = 1.0 + alpha / a;
				a1 = -2.0 * cos;
				a2 = 1.0 - alpha / a;
			}
			Kind::LowShelf | Kind::HighShelf => {
				// Shelf slope 1.
				let alpha = sin / 2.0 * 2f64.sqrt();
				let t = 2.0 * a.sqrt() * alpha;
				if matches!(kind, Kind::LowShelf) {
					b0 = a * ((a + 1.0) - (a - 1.0) * cos + t);
					b1 = 2.0 * a * ((a - 1.0) - (a + 1.0) * cos);
					b2 = a * ((a + 1.0) - (a - 1.0) * cos - t);
					a0 = (a + 1.0) + (a - 1.0) * cos + t;
					a1 = -2.0 * ((a - 1.0) + (a + 1.0) * cos);
					a2 = (a + 1.0) + (a - 1.0) * cos - t;
				} else {
					b0 = a * ((a + 1.0) + (a - 1.0) * cos + t);
					b1 = -2.0 * a * ((a - 1.0) + (a + 1.0) * cos);
					b2 = a * ((a + 1.0) + (a - 1.0) * cos - t);
					a0 = (a + 1.0) - (a - 1.0) * cos + t;
					a1 = 2.0 * ((a - 1.0) - (a + 1.0) * cos);
					a2 = (a + 1.0) - (a - 1.0) * cos - t;
				}
			}
		}
		self.b0 = b0 / a0;
		self.b1 = b1 / a0;
		self.b2 = b2 / a0;
		self.a1 = a1 / a0;
		self.a2 = a2 / a0;
	}

	fn run(&mut self, ch: usize, x: f64) -> f64 {
		let z = &mut self.z[ch];
		let y = self.b0 * x + z[0];
		z[0] = self.b1 * x - self.a1 * y + z[1];
		z[1] = self.b2 * x - self.a2 * y;
		y
	}
}

pub struct Eq {
	rate: f64,
	active: bool,
	bands: [Biquad; 3],
}

impl Eq {
	pub fn new(rate: u32) -> Eq {
		Eq {
			rate: rate as f64,
			active: false,
			bands: [Biquad::default(); 3],
		}
	}

	pub fn set(&mut self, s: &EqSettings) {
		let db = |v: i8| v.clamp(-MAX_DB, MAX_DB);
		let gains = [db(s.bass), db(s.mid), db(s.treble)];
		self.active = s.enabled && gains.iter().any(|&g| g != 0);
		let spec = [
			(Kind::LowShelf, LOW_HZ),
			(Kind::Peak, MID_HZ),
			(Kind::HighShelf, HIGH_HZ),
		];
		for ((band, (kind, hz)), g) in self.bands.iter_mut().zip(spec).zip(gains) {
			band.tune(kind, self.rate, hz, g as f64);
		}
	}

	/// Interleaved stereo, in place.
	pub fn process(&mut self, stereo: &mut [f32]) {
		if !self.active {
			return;
		}
		for frame in stereo.as_chunks_mut::<2>().0 {
			for (ch, v) in frame.iter_mut().enumerate() {
				let mut x = *v as f64;
				for b in &mut self.bands {
					x = b.run(ch, x);
				}
				*v = x as f32;
			}
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	const RATE: u32 = 48_000;

	fn sine(hz: f32, frames: usize) -> Vec<f32> {
		(0..frames)
			.flat_map(|i| {
				let v = (2.0 * std::f32::consts::PI * hz * i as f32 / RATE as f32).sin() * 0.25;
				[v, v]
			})
			.collect()
	}

	fn settings(bass: i8, mid: i8, treble: i8) -> EqSettings {
		EqSettings {
			enabled: true,
			preset: EqPreset::Custom,
			bass,
			mid,
			treble,
		}
	}

	/// Gain in dB over the settled second half of a sine.
	fn gain_db(s: &EqSettings, hz: f32) -> f32 {
		let mut eq = Eq::new(RATE);
		eq.set(s);
		let input = sine(hz, RATE as usize);
		let mut out = input.clone();
		eq.process(&mut out);
		let rms = |v: &[f32]| (v.iter().map(|x| x * x).sum::<f32>() / v.len() as f32).sqrt();
		let h = input.len() / 2;
		20.0 * (rms(&out[h..]) / rms(&input[h..])).log10()
	}

	#[test]
	fn flat_is_passthrough() {
		let mut eq = Eq::new(RATE);
		eq.set(&settings(0, 0, 0));
		let input = sine(440.0, 1000);
		let mut out = input.clone();
		eq.process(&mut out);
		assert_eq!(out, input);
	}

	#[test]
	fn bass_boost_raises_lows_only() {
		let s = settings(6, 0, 0);
		assert!((gain_db(&s, 50.0) - 6.0).abs() < 0.7);
		assert!(gain_db(&s, 5000.0).abs() < 0.7);
	}

	#[test]
	fn treble_cut_lowers_highs() {
		let s = settings(0, 0, -6);
		assert!(gain_db(&s, 10_000.0) < -4.0);
		assert!(gain_db(&s, 100.0).abs() < 0.7);
	}

	#[test]
	fn disabled_is_passthrough() {
		let mut eq = Eq::new(RATE);
		eq.set(&EqSettings {
			enabled: false,
			..settings(12, 12, 12)
		});
		let input = sine(100.0, 1000);
		let mut out = input.clone();
		eq.process(&mut out);
		assert_eq!(out, input);
	}

	#[test]
	fn preset_values_and_labels() {
		let all = [
			(EqPreset::Flat, Some((0, 0, 0)), "Flat"),
			(EqPreset::BassBoost, Some((6, 0, 0)), "Bass Boost"),
			(EqPreset::TrebleBoost, Some((0, 0, 6)), "Treble Boost"),
			(EqPreset::Loudness, Some((5, -1, 4)), "Loudness"),
			(EqPreset::Voice, Some((-3, 4, 1)), "Voice"),
			(EqPreset::Warm, Some((3, 1, -2)), "Warm"),
			(EqPreset::Custom, None, "Custom"),
		];
		for (p, bands, label) in all {
			assert_eq!(p.bands(), bands);
			assert_eq!(p.label(), label);
		}
	}

	#[test]
	fn describe_exact() {
		assert_eq!(
			describe(&settings(6, 0, -3)),
			"Bass 6 dB, Mid 0 dB, Treble -3 dB"
		);
	}
}
