//! Three-band equalizer: RBJ low shelf, peaking mid and high shelf, per channel.

use std::f64::consts::PI;

use crate::settings::{EqPreset, EqSettings};

// Shelf corners sit wide (250 Hz / 3 kHz) on purpose: at 120 Hz / 6 kHz
// a boost only moved the far ends of the spectrum, and on laptop or TV
// speakers the change was barely felt (Brandon's report). Wide shelves
// move the bands small speakers actually reproduce.
const LOW_HZ: f64 = 250.0;
const MID_HZ: f64 = 1000.0;
const MID_Q: f64 = 0.9;
const HIGH_HZ: f64 = 3000.0;
/// Band gain limit in dB, either way.
pub const MAX_DB: i8 = 12;
/// On/off crossfade, as long as the mixer's warm-swap crossfade.
const FADE_MS: f64 = 25.0;

// The Loudness leveler (dynaudnorm-style dynamic gain, the consensus
// tool for live sources: mpv ships it as a filter, FFmpeg as
// dynaudnorm). A static EQ curve cannot make a quiet station louder
// than a loud one; a leveler can — it rides the gain so each stretch
// of audio peaks near a target.
/// What the leveler aims each stretch's peak at.
const TARGET_PEAK: f64 = 0.9;
/// Most gain the leveler may apply: +24 dB for genuinely quiet stations.
const MAX_GAIN: f64 = 16.0;
/// Least: a hot station is pulled down, never to silence.
const MIN_GAIN: f64 = 0.2;
/// Below this peak (−52 dBFS) a stretch counts as silence and is
/// left alone, so dead air and hiss are not amplified.
const SILENCE: f64 = 0.0025;
/// How far back the reference peak reaches.
const REF_SECONDS: f64 = 4.0;
/// Gain rises slowly (no pumping), falls fast (no blasting).
const UP_SECONDS: f64 = 1.2;
const DOWN_SECONDS: f64 = 0.05;
/// Soft-knee ceiling after leveling: approaches 1.0, never crosses.
const KNEE: f64 = 0.95;

impl EqPreset {
	/// Band gains in dB (bass, mid, treble); `None` for Custom.
	pub fn bands(self) -> Option<(i8, i8, i8)> {
		match self {
			EqPreset::Flat => Some((0, 0, 0)),
			EqPreset::BassBoost => Some((6, 0, 0)),
			EqPreset::TrebleBoost => Some((0, 0, 6)),
			EqPreset::BassAndTreble => Some((6, 0, 6)),
			// DVBViewer-style loudness: strong smiles at both ends, so
			// it is plainly heard even on speakers, not just headphones.
			EqPreset::Loudness => Some((8, -2, 6)),
			EqPreset::Voice => Some((-3, 4, 1)),
			EqPreset::Warm => Some((3, 1, -2)),
			EqPreset::Clarity => Some((-1, 3, 6)),
			// Small speakers reproduce neither deep bass nor air; put
			// the energy in the middle where they can play it.
			EqPreset::LaptopSpeakers => Some((-4, 4, 3)),
			EqPreset::Custom => None,
		}
	}

	pub fn label(self) -> &'static str {
		match self {
			EqPreset::Flat => "Flat",
			EqPreset::BassBoost => "Bass Boost",
			EqPreset::TrebleBoost => "Treble Boost",
			EqPreset::BassAndTreble => "Bass and Treble",
			EqPreset::Loudness => "Loudness",
			EqPreset::Voice => "Voice",
			EqPreset::Warm => "Warm",
			EqPreset::Clarity => "Clarity",
			EqPreset::LaptopSpeakers => "Laptop Speakers",
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
	/// Share of filtered signal in the output; eases toward `active` over `FADE_MS`, so
	/// switching the EQ on or off never clicks. 0 is bit-exact bypass.
	wet: f64,
	wet_step: f64,
	bands: [Biquad; 3],
	/// The Loudness preset's leveler is on (see the constants above).
	level: bool,
	/// The leveler's current gain; rides toward its target and back
	/// to 1.0 when the preset is left.
	lev_gain: f64,
	/// The leveler's reference peak: the loudest recent frame,
	/// decaying over REF_SECONDS.
	lev_ref: f64,
}

impl Eq {
	pub fn new(rate: u32) -> Eq {
		let rate = rate as f64;
		Eq {
			rate,
			active: false,
			wet: 0.0,
			wet_step: 1.0 / (rate * FADE_MS / 1000.0).max(1.0),
			bands: [Biquad::default(); 3],
			level: false,
			lev_gain: 1.0,
			lev_ref: 0.0,
		}
	}

	pub fn set(&mut self, s: &EqSettings) {
		let db = |v: i8| v.clamp(-MAX_DB, MAX_DB);
		let gains = [db(s.bass), db(s.mid), db(s.treble)];
		self.active = s.enabled && gains.iter().any(|&g| g != 0);
		self.level = s.enabled && s.preset == EqPreset::Loudness;
		if self.active && self.wet == 0.0 {
			// State left from long ago would ring in as a burst.
			for b in &mut self.bands {
				b.z = [[0.0; 2]; 2];
			}
		}
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
		// The leveler may still be riding its gain home after the
		// Loudness preset was left; that is work even with the EQ off.
		let settling = self.level || (self.lev_gain - 1.0).abs() > 0.002;
		if !self.active && self.wet == 0.0 && !settling {
			return;
		}
		let target = if self.active { 1.0 } else { 0.0 };
		for frame in stereo.as_chunks_mut::<2>().0 {
			self.wet += (target - self.wet).clamp(-self.wet_step, self.wet_step);
			let mut mixed = [0.0f64; 2];
			for (ch, v) in frame.iter_mut().enumerate() {
				let dry = *v as f64;
				let mut x = dry;
				for b in &mut self.bands {
					x = b.run(ch, x);
				}
				// Linear: wet and dry are the same signal, so equal-power would bulge.
				mixed[ch] = dry + (x - dry) * self.wet;
			}
			if settling {
				self.ride_level(&mut mixed);
			}
			for (ch, v) in frame.iter_mut().enumerate() {
				*v = mixed[ch] as f32;
			}
		}
	}

	/// One stereo frame of the Loudness leveler: measure the frame
	/// against the recent peak, ride the gain toward the target and
	/// keep the result under a soft ceiling. Channels are coupled —
	/// one gain for both — so the stereo image never wanders.
	fn ride_level(&mut self, frame: &mut [f64; 2]) {
		let peak = frame[0].abs().max(frame[1].abs());
		let decay = (-1.0 / (self.rate * REF_SECONDS)).exp();
		self.lev_ref = peak.max(self.lev_ref * decay);
		let desired = if !self.level {
			1.0
		} else if self.lev_ref > SILENCE {
			(TARGET_PEAK / self.lev_ref).clamp(MIN_GAIN, MAX_GAIN)
		} else {
			1.0
		};
		let tau = if desired < self.lev_gain {
			DOWN_SECONDS
		} else {
			UP_SECONDS
		};
		let k = 1.0 - (-1.0 / (self.rate * tau)).exp();
		self.lev_gain += (desired - self.lev_gain) * k;
		if !self.level && (self.lev_gain - 1.0).abs() <= 0.002 {
			self.lev_gain = 1.0;
			self.lev_ref = 0.0;
		}
		for v in frame.iter_mut() {
			*v *= self.lev_gain;
			let a = v.abs();
			if a > KNEE {
				*v = v.signum() * (KNEE + (1.0 - KNEE) * ((a - KNEE) / (1.0 - KNEE)).tanh());
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
			..EqSettings::default()
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

	/// Steepest sample-to-sample step on the left channel.
	fn max_step(v: &[f32]) -> f32 {
		let left: Vec<f32> = v.iter().step_by(2).copied().collect();
		left.windows(2)
			.map(|w| (w[1] - w[0]).abs())
			.fold(0.0, f32::max)
	}

	#[test]
	fn toggling_mid_sine_does_not_click() {
		let hz = 200.0;
		let on = settings(12, 0, 0);
		let off = EqSettings {
			enabled: false,
			..on.clone()
		};
		let flat = settings(0, 0, 0);
		// The bound is the sine's own steepest step, boosted or not.
		let mut reference = Eq::new(RATE);
		reference.set(&on);
		let mut wet = sine(hz, RATE as usize);
		reference.process(&mut wet);
		let bound = max_step(&wet[wet.len() / 2..]).max(max_step(&sine(hz, 1000))) * 1.1;
		// Half a second in, a quarter cycle past a zero crossing: the sine's peak.
		let cut = 2 * (RATE as usize / 2 + 60);
		for (first, second) in [(&on, &off), (&on, &flat), (&off, &on), (&flat, &on)] {
			let input = sine(hz, RATE as usize);
			let mut out = input.clone();
			let mut eq = Eq::new(RATE);
			eq.set(first);
			eq.process(&mut out[..cut]);
			eq.set(second);
			eq.process(&mut out[cut..]);
			let step = max_step(&out);
			assert!(step < bound, "{step} >= {bound}");
			if !eq.active {
				// Once the 25 ms fade is over, bypass is bit-exact again.
				let done = cut + 2 * 1200;
				assert_eq!(out[done..], input[done..]);
			}
		}
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
			(EqPreset::BassAndTreble, Some((6, 0, 6)), "Bass and Treble"),
			(EqPreset::Loudness, Some((8, -2, 6)), "Loudness"),
			(EqPreset::Voice, Some((-3, 4, 1)), "Voice"),
			(EqPreset::Warm, Some((3, 1, -2)), "Warm"),
			(EqPreset::Clarity, Some((-1, 3, 6)), "Clarity"),
			(EqPreset::LaptopSpeakers, Some((-4, 4, 3)), "Laptop Speakers"),
			(EqPreset::Custom, None, "Custom"),
		];
		for (p, bands, label) in all {
			assert_eq!(p.bands(), bands);
			assert_eq!(p.label(), label);
		}
	}

	#[test]
	fn loudness_moves_the_middle_bands_speakers_play() {
		// Brandon: DVBViewer's loudness is felt on speakers, ours was
		// not. Measured on this curve: 300 Hz +2.4 dB, 3 kHz +2.7 dB,
		// 100 Hz +7.8 dB — a smile small speakers can reproduce.
		let (b, m, t) = EqPreset::Loudness.bands().unwrap();
		let s = settings(b, m, t);
		assert!(gain_db(&s, 300.0) > 2.0, "low shelf reaches 300 Hz");
		assert!(gain_db(&s, 3000.0) > 2.0, "high shelf reaches 3 kHz");
	}

	fn loudness_settings() -> EqSettings {
		// Zero bands isolate the leveler: the dialog's Loudness bands
		// ride on top of it in the app.
		EqSettings {
			enabled: true,
			preset: EqPreset::Loudness,
			bass: 0,
			mid: 0,
			treble: 0,
			..EqSettings::default()
		}
	}

	fn scaled(hz: f32, factor: f32, seconds: usize) -> Vec<f32> {
		sine(hz, RATE as usize * seconds)
			.iter()
			.map(|v| v * factor)
			.collect()
	}

	fn peak(v: &[f32]) -> f32 {
		v.iter().map(|x| x.abs()).fold(0.0, f32::max)
	}

	#[test]
	fn loudness_levels_a_quiet_station_up() {
		// Brandon: quiet channels should come up to where loud ones
		// live. A 0.05-peak station ends up peaking near the target.
		let mut eq = Eq::new(RATE);
		eq.set(&loudness_settings());
		let mut out = scaled(440.0, 0.2, 3);
		eq.process(&mut out);
		let last = &out[out.len() - RATE as usize * 2..];
		assert!(peak(last) > 0.55, "quiet station lifted: {}", peak(last));
	}

	#[test]
	fn loudness_tames_a_hot_station_without_clipping() {
		let mut eq = Eq::new(RATE);
		eq.set(&loudness_settings());
		let mut out = scaled(440.0, 4.0, 3);
		eq.process(&mut out);
		assert!(peak(&out) <= 1.0, "never over full scale");
		let last = &out[out.len() - RATE as usize * 2..];
		assert!(peak(last) > 0.7, "hot station near target: {}", peak(last));
	}

	#[test]
	fn loudness_leaves_near_silence_alone() {
		// Under the silence floor the gain stays at 1: dead air and
		// hiss must not be amplified into noise.
		let mut eq = Eq::new(RATE);
		eq.set(&loudness_settings());
		let mut out = scaled(440.0, 0.004, 2);
		eq.process(&mut out);
		assert!(peak(&out) < 0.01, "silence untouched: {}", peak(&out));
	}

	#[test]
	fn loudness_gain_rides_home_when_the_preset_is_left() {
		let mut eq = Eq::new(RATE);
		eq.set(&loudness_settings());
		let mut out = scaled(440.0, 0.2, 2);
		eq.process(&mut out);
		assert!(peak(&out) > 0.3, "gain was up: {}", peak(&out));
		eq.set(&settings(0, 0, 0));
		let mut out = scaled(440.0, 0.2, 3);
		eq.process(&mut out);
		let last = &out[out.len() - RATE as usize * 2..];
		assert!(
			peak(last) < 0.08,
			"back at the station's own level: {}",
			peak(last)
		);
	}

	#[test]
	fn describe_exact() {
		assert_eq!(
			describe(&settings(6, 0, -3)),
			"Bass 6 dB, Mid 0 dB, Treble -3 dB"
		);
	}
}

