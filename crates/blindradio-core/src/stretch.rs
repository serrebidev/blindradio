//! Playback speed for time-shifted listening: catch up on a program
//! faster, or slow it down. Two ways, as on a cassette deck with a
//! pitch control or without one:
//!
//! - Keep pitch: WSOLA (waveform-similarity overlap-add), the method
//!   SoundTouch and browsers' playback rate use. Overlapping 40 ms
//!   pieces of the buffer, each nudged up to 12 ms to line up with the
//!   last one, so voices stay at their own pitch.
//! - Tape: reads the buffer faster or slower, pitch moves with speed.
//!
//! Reads the time-shift buffer directly, inside the audio callback: no
//! thread, no allocation after the first pass, a few percent of one core.
//! Leaves off seamlessly when reads would pass live: the frames it did
//! not fill continue at normal speed from the cursor it returns.

use std::ops::Range;

/// The overlap-add piece. Longer smears drums, shorter warbles voices.
const WINDOW_MS: u32 = 40;
/// How far a piece may move to line up with the last one.
const SEARCH_MS: u32 = 12;
/// The coarse search steps this many frames, then refines around the best.
const COARSE: usize = 4;

pub struct Stretch {
	/// Window frames, even.
	n: usize,
	search: usize,
	/// Rising half of a periodic Hann window: `w[i] + w[i + h] == 1`.
	rise: Vec<f32>,
	wsola: Option<Wsola>,
	/// Tape position in frames, fractional.
	tape: Option<f64>,
	src: Vec<f32>,
	cont: Vec<f32>,
}

struct Wsola {
	/// Falling half of the last piece, stereo, `h` frames.
	tail: Vec<f32>,
	/// Where the last piece would carry on unstretched: the tail's source.
	cont: u64,
	/// Where the next piece belongs, before alignment.
	ideal: f64,
	/// One finished hop, stereo; `done` samples of it already played.
	ready: Vec<f32>,
	done: usize,
}

/// Reads stereo frames from an absolute frame into the slice; returns samples written.
pub type Read<'a> = &'a dyn Fn(u64, &mut [f32]) -> usize;

impl Stretch {
	pub fn new(rate: u32) -> Stretch {
		let n = ((rate * WINDOW_MS / 1000) as usize).max(8) & !1;
		let h = n / 2;
		let rise = (0..h)
			.map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / n as f32).cos())
			.collect();
		Stretch {
			n,
			search: (rate * SEARCH_MS / 1000) as usize,
			rise,
			wsola: None,
			tape: None,
			src: Vec::new(),
			cont: Vec::new(),
		}
	}

	/// Forgets the position: the next render starts fresh at its cursor (a seek).
	pub fn reset(&mut self) {
		self.wsola = None;
		self.tape = None;
	}

	/// Fills `out` (stereo) from `cursor` at `speed` within `span`, the frames
	/// the buffer holds up to live. Returns frames written and the cursor
	/// playback stands at. Fewer frames than `out` holds: reads would pass
	/// live, state is dropped, carry on at normal speed from that cursor.
	pub fn render(
		&mut self,
		read: Read,
		span: Range<u64>,
		cursor: u64,
		speed: f32,
		keep_pitch: bool,
		out: &mut [f32],
	) -> (usize, u64) {
		if keep_pitch {
			self.tape = None;
			self.wsola_render(read, span, cursor, f64::from(speed), out)
		} else {
			if self.wsola.take().is_some() {
				// Leaving WSOLA mid-piece: its cursor is where raw audio carries on.
				self.tape = None;
			}
			self.tape_render(read, span, cursor, f64::from(speed), out)
		}
	}

	fn tape_render(&mut self, read: Read, span: Range<u64>, cursor: u64, speed: f64, out: &mut [f32]) -> (usize, u64) {
		let frames = out.len() / 2;
		let mut pos = self.tape.unwrap_or(cursor as f64).max(span.start as f64);
		let first = pos as u64;
		let end = span.end.min(first + (frames as f64 * speed).ceil() as u64 + 2);
		let len = end.saturating_sub(first) as usize;
		self.src.resize(len * 2, 0.0);
		let len = read(first, &mut self.src[..len * 2]) / 2;
		let mut i = 0;
		// ponytail: linear interpolation, no anti-alias filter; speeding up
		// folds content above rate/2/speed down. Mild at the speeds offered.
		while i < frames {
			let k = pos - first as f64;
			let j = k as usize;
			if j + 1 >= len {
				break;
			}
			let f = (k - j as f64) as f32;
			for c in 0..2 {
				let (a, b) = (self.src[2 * j + c], self.src[2 * j + 2 + c]);
				out[2 * i + c] = a + (b - a) * f;
			}
			pos += speed;
			i += 1;
		}
		if i < frames {
			self.tape = None;
			return (i, pos as u64);
		}
		self.tape = Some(pos);
		(frames, pos as u64)
	}

	fn wsola_render(&mut self, read: Read, span: Range<u64>, cursor: u64, speed: f64, out: &mut [f32]) -> (usize, u64) {
		let frames = out.len() / 2;
		let h = self.n / 2;
		if self.wsola.is_none() {
			// Start as if a piece ending at `cursor + h` just played: its tail is
			// the raw audio from the cursor, so the first hop joins without a seam.
			let mut tail = vec![0.0; h * 2];
			if cursor + h as u64 > span.end || read(cursor, &mut tail) < h * 2 {
				return (0, cursor);
			}
			for (i, s) in tail.as_chunks_mut::<2>().0.iter_mut().enumerate() {
				let g = 1.0 - self.rise[i];
				s[0] *= g;
				s[1] *= g;
			}
			self.wsola = Some(Wsola {
				tail,
				cont: cursor,
				ideal: cursor as f64 - h as f64 + h as f64 * speed,
				ready: vec![0.0; h * 2],
				done: h * 2,
			});
		}
		let mut i = 0;
		while i < frames {
			let w = self.wsola.as_ref().unwrap();
			if w.done == w.ready.len() && !self.piece(read, &span, speed) {
				let cont = self.wsola.take().unwrap().cont;
				return (i, cont);
			}
			let w = self.wsola.as_mut().unwrap();
			let n = (w.ready.len() - w.done).min((frames - i) * 2);
			out[i * 2..i * 2 + n].copy_from_slice(&w.ready[w.done..w.done + n]);
			w.done += n;
			i += n / 2;
		}
		(frames, self.wsola.as_ref().unwrap().cont)
	}

	/// Builds the next hop into `ready`; false when it would read past live.
	fn piece(&mut self, read: Read, span: &Range<u64>, speed: f64) -> bool {
		let (n, h) = (self.n, self.n / 2);
		let w = self.wsola.as_mut().unwrap();
		let target = w.ideal.round() as i64;
		let lo = (target - self.search as i64).max(span.start as i64);
		let hi = (target + self.search as i64).min(span.end as i64 - n as i64);
		if hi < lo || w.cont + h as u64 > span.end {
			return false;
		}
		let len = (hi - lo) as usize + n;
		self.src.resize(len * 2, 0.0);
		self.cont.resize(h * 2, 0.0);
		if read(lo as u64, &mut self.src) < len * 2 || read(w.cont, &mut self.cont) < h * 2 {
			return false;
		}
		let best = best_offset(&self.src, &self.cont, (hi - lo) as usize, h);
		let x = &self.src[best * 2..(best + n) * 2];
		for i in 0..h {
			let (r, f) = (self.rise[i], 1.0 - self.rise[i]);
			for c in 0..2 {
				w.ready[2 * i + c] = w.tail[2 * i + c] + r * x[2 * i + c];
				w.tail[2 * i + c] = f * x[2 * (h + i) + c];
			}
		}
		w.done = 0;
		w.cont = (lo as usize + best + h) as u64;
		w.ideal += h as f64 * speed;
		true
	}
}

/// The offset in `0..=max` whose first `h` frames of `src` best match `cont`:
/// normalized cross-correlation of the channel sums, coarse then fine.
fn best_offset(src: &[f32], cont: &[f32], max: usize, h: usize) -> usize {
	let mono = |s: &[f32], i: usize| s[2 * i] + s[2 * i + 1];
	let score = |off: usize, step: usize| {
		let (mut dot, mut energy) = (0.0f32, 1e-9f32);
		for i in (0..h).step_by(step) {
			let a = mono(src, off + i);
			dot += a * mono(cont, i);
			energy += a * a;
		}
		dot / energy.sqrt()
	};
	let pick = |range: std::ops::RangeInclusive<usize>, by: usize, step: usize| {
		range
			.step_by(by)
			.map(|o| (o, score(o, step)))
			.fold((0, f32::MIN), |b, c| if c.1 > b.1 { c } else { b })
			.0
	};
	let coarse = pick(0..=max, COARSE, 2);
	pick(coarse.saturating_sub(COARSE)..=(coarse + COARSE).min(max), 1, 1)
}

#[cfg(test)]
mod tests {
	use super::*;

	const RATE: u32 = 48_000;

	fn tone(frames: usize, hz: f32) -> Vec<f32> {
		(0..frames)
			.flat_map(|i| {
				let s = (std::f32::consts::TAU * hz * i as f32 / RATE as f32).sin() * 0.5;
				[s, s]
			})
			.collect()
	}

	/// Zero crossings per second of the left channel: the pitch heard.
	fn pitch(s: &[f32]) -> f32 {
		let l: Vec<f32> = s.iter().step_by(2).copied().collect();
		let crossings = l.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count();
		crossings as f32 * RATE as f32 / l.len() as f32
	}

	/// Plays `secs` of a 440 Hz tone at `speed` in 10 ms callbacks; returns the
	/// output and the source frames used.
	fn play(speed: f32, keep_pitch: bool, secs: usize) -> (Vec<f32>, u64) {
		let src = tone(RATE as usize * secs, 440.0);
		let read = |at: u64, out: &mut [f32]| {
			let from = (at as usize * 2).min(src.len());
			let n = out.len().min(src.len() - from);
			out[..n].copy_from_slice(&src[from..from + n]);
			n
		};
		let mut st = Stretch::new(RATE);
		let (mut cursor, mut heard) = (0, Vec::new());
		let mut buf = vec![0.0; 960];
		for _ in 0..100 {
			let (n, next) = st.render(&read, 0..(src.len() / 2) as u64, cursor, speed, keep_pitch, &mut buf);
			assert_eq!(n, 480, "never runs short well before live");
			heard.extend_from_slice(&buf);
			cursor = next;
		}
		(heard, cursor)
	}

	#[test]
	fn keep_pitch_moves_faster_at_the_same_pitch() {
		for speed in [0.5, 0.75, 1.25, 1.5, 2.0] {
			let (heard, used) = play(speed, true, 6);
			let want = RATE as f32 * speed;
			assert!((used as f32 - want).abs() < want * 0.03, "{speed}: used {used} for 1 s heard");
			// Skip the first window, where the start joins.
			let p = pitch(&heard[RATE as usize / 10..]);
			assert!((p - 440.0).abs() < 8.0, "{speed}: pitch {p}");
			assert!(heard.iter().all(|s| s.abs() <= 0.6), "{speed}: overlap stays in level");
		}
	}

	#[test]
	fn tape_moves_pitch_with_speed() {
		let (heard, used) = play(1.5, false, 3);
		assert!((used as f32 - 72_000.0).abs() < 10.0, "used {used}");
		let p = pitch(&heard);
		assert!((p - 660.0).abs() < 8.0, "pitch {p}");
	}

	#[test]
	fn stops_short_at_live_and_hands_back_the_cursor() {
		let src = tone(RATE as usize, 440.0);
		let read = |at: u64, out: &mut [f32]| {
			let from = at as usize * 2;
			let n = out.len().min(src.len().saturating_sub(from));
			out[..n].copy_from_slice(&src[from..from + n]);
			n
		};
		let live = RATE as u64 / 2;
		for keep_pitch in [true, false] {
			let mut st = Stretch::new(RATE);
			let (mut cursor, mut buf) = (0, vec![0.0; 960]);
			let short = loop {
				let (n, next) = st.render(&read, 0..live, cursor, 2.0, keep_pitch, &mut buf);
				cursor = next;
				if n < 480 {
					break n;
				}
			};
			assert!(short < 480 && cursor <= live && cursor > live - 3000, "{keep_pitch}: at {cursor}");
			assert!(st.wsola.is_none() && st.tape.is_none());
		}
	}
}
