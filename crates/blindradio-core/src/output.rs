use std::f32::consts::FRAC_PI_2;
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, SampleFormat, SupportedBufferSize};
use crossbeam_channel::{Receiver, Sender, unbounded};
use parking_lot::Mutex;

use crate::Result;
use crate::error::CoreError;
use crate::ring::{CUSHION_MS, PcmRing};

/// Warm-swap crossfade length.
const CROSSFADE_MS: u64 = 25;
/// Ease-out at an underrun, so running dry does not click.
const TAIL_MS: u64 = 5;
/// Target stream buffer when the device range allows it.
const BUFFER_MS: u32 = 10;
/// Peak of the hiss between stations, before volume: soft, like a dial between stations.
// ponytail: fixed level and tone; make it a setting if listeners want it louder, quieter or off.
const HISS_LEVEL: f32 = 0.08;
/// One-pole lowpass on the hiss: dulls white noise toward a radio's softer static.
const HISS_TONE: f32 = 0.3;

pub enum MixerCmd {
	SetActive {
		ring: Option<Arc<PcmRing>>,
		token: u64,
	},
	Volume(f32),
	Mute(bool),
	/// Leave the active station for the space between stations: it fades out into soft hiss,
	/// which lasts until a `SetActive` ring has audio, so tuning never has dead air.
	/// `SetActive { ring: None }` (stop) is silence.
	Tune,
}

/// Where the playing ring stands.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Flow {
	/// Newly active or ran dry: waits for `cushion_ms` of audio, so playback never starts on
	/// a few hundred ms only to stop again, and one clean gap replaces a stutter.
	Buffering,
	Playing,
}

pub struct MixerCore {
	fade_frames: usize,
	tail_frames: usize,
	cushion_ms: u32,
	active: Option<Arc<PcmRing>>,
	flow: Flow,
	/// Frames the active ring has sounded since it (re)started; drives its fade-in.
	sounded: usize,
	/// Rings fading out, each with frames faded so far. A fast step can leave several.
	fading: Vec<(Arc<PcmRing>, usize)>,
	pending: Option<u64>,
	volume: f32,
	muted: bool,
	/// Between stations or waiting on the active one: hiss fills the gap.
	hiss: bool,
	hiss_level: f32,
	hiss_gain: f32,
	hiss_step: f32,
	noise: u32,
	noise_lp: f32,
	a: Vec<f32>,
	b: Vec<f32>,
}

impl MixerCore {
	pub fn new(rate: u32) -> MixerCore {
		MixerCore {
			fade_frames: (rate as u64 * CROSSFADE_MS / 1000) as usize,
			tail_frames: (rate as u64 * TAIL_MS / 1000) as usize,
			cushion_ms: CUSHION_MS,
			active: None,
			flow: Flow::Buffering,
			sounded: 0,
			fading: Vec::new(),
			pending: None,
			volume: 1.0,
			muted: false,
			hiss: false,
			hiss_level: HISS_LEVEL,
			hiss_gain: 0.0,
			hiss_step: 1.0 / (rate as u64 * CROSSFADE_MS / 1000).max(1) as f32,
			noise: 0x9E37_79B9,
			noise_lp: 0.0,
			a: Vec::new(),
			b: Vec::new(),
		}
	}

	pub fn handle(&mut self, cmd: MixerCmd) {
		match cmd {
			MixerCmd::Tune => {
				self.handle(MixerCmd::SetActive {
					ring: None,
					token: 0,
				});
				self.hiss = true;
			}
			MixerCmd::SetActive { ring, token } => {
				// Waiting on a station hisses; stop is silent.
				self.hiss = ring.is_some();
				let is = |a: &Arc<PcmRing>| ring.as_ref().is_some_and(|r| Arc::ptr_eq(a, r));
				// The same ring again keeps playing; draining it as both old and new would double its speed.
				if !self.active.as_ref().is_some_and(is) {
					if let Some(old) = self.active.take() {
						self.fading.push((old, 0));
					}
					self.fading.retain(|(f, _)| !is(f));
					self.active = ring;
					self.flow = Flow::Buffering;
					self.sounded = 0;
				}
				self.pending = Some(token);
			}
			MixerCmd::Volume(v) => self.volume = v.clamp(0.0, 1.0),
			MixerCmd::Mute(muted) => self.muted = muted,
		}
	}

	/// Fills `out` with stereo frames pulled from the active ring; the pending token is
	/// returned the first time the active ring actually yields samples.
	pub fn render(&mut self, out: &mut [f32]) -> Option<u64> {
		out.fill(0.0);
		let frames = out.len() / 2;
		let len = self.fade_frames.max(1) as f32;

		self.a.clear();
		self.a.resize(out.len(), 0.0);
		let mut got = 0;
		if let Some(ring) = &self.active {
			let level = ring.len_ms();
			if self.flow == Flow::Buffering && level > 0 && level >= self.cushion_ms {
				self.flow = Flow::Playing;
				self.sounded = 0;
			}
			if self.flow == Flow::Playing {
				got = ring.pop_into(&mut self.a) / 2;
				if got < frames {
					// Ran dry mid-buffer: ease out the last samples instead of cutting them.
					let tail = self.tail_frames.min(got).max(1) as f32;
					for (k, f) in (got.saturating_sub(self.tail_frames)..got).enumerate() {
						let g = 1.0 - (k + 1) as f32 / tail;
						self.a[2 * f] *= g;
						self.a[2 * f + 1] *= g;
					}
					self.flow = Flow::Buffering;
				}
			}
		}
		// Equal-power: the active ring enters as sin, from silence or across a crossfade.
		for f in 0..got {
			let p = (self.sounded + f).min(self.fade_frames) as f32 / len;
			let g = (p * FRAC_PI_2).sin();
			self.a[2 * f] *= g;
			self.a[2 * f + 1] *= g;
		}
		self.sounded += got;

		// Each fading ring leaves as cos on its own clock, so a new step never cuts an old fade short.
		for (ring, pos) in &mut self.fading {
			self.b.clear();
			self.b.resize(out.len(), 0.0);
			ring.pop_into(&mut self.b);
			for f in 0..frames {
				let p = (*pos + f).min(self.fade_frames) as f32 / len;
				let g = (p * FRAC_PI_2).cos();
				self.a[2 * f] += self.b[2 * f] * g;
				self.a[2 * f + 1] += self.b[2 * f + 1] * g;
			}
			*pos += frames;
		}
		let done = self.fade_frames;
		self.fading.retain(|(_, pos)| *pos < done);

		// Hiss covers every frame the active station does not, easing in and out over a crossfade.
		for f in 0..frames {
			let want = if self.hiss && f >= got { 1.0 } else { 0.0 };
			self.hiss_gain += (want - self.hiss_gain).clamp(-self.hiss_step, self.hiss_step);
			if self.hiss_gain > 0.0 {
				// xorshift32 white noise, lowpassed.
				self.noise ^= self.noise << 13;
				self.noise ^= self.noise >> 17;
				self.noise ^= self.noise << 5;
				let white = self.noise as f32 / u32::MAX as f32 * 2.0 - 1.0;
				self.noise_lp += (white - self.noise_lp) * HISS_TONE;
				let s = self.noise_lp * self.hiss_level * self.hiss_gain;
				self.a[2 * f] += s;
				self.a[2 * f + 1] += s;
			}
		}

		let vol = if self.muted { 0.0 } else { self.volume };
		for (o, &s) in out.iter_mut().zip(&self.a) {
			*o = s * vol;
		}

		if got > 0 { self.pending.take() } else { None }
	}
}

struct Callback {
	core: MixerCore,
	scratch: Vec<f32>,
}

/// Device output stream; built, driven and dropped on the UI thread.
pub struct Output {
	pub sample_rate: u32,
	cmd_tx: Sender<MixerCmd>,
	rendered_rx: Receiver<(u64, Instant)>,
	// Dropping the stream stops playback.
	_stream: cpal::Stream,
	// Not Send: Output stays on the thread that built it, so mixer commands come from one thread in order.
	_ui_thread: PhantomData<Rc<()>>,
}

impl Output {
	pub fn start_default() -> Result<Output> {
		let host = cpal::default_host();
		let device = host
			.default_output_device()
			.ok_or_else(|| CoreError::Output("no default output device".into()))?;
		let default = device
			.default_output_config()
			.map_err(|e| CoreError::Output(e.to_string()))?;
		if default.sample_format() != SampleFormat::F32 {
			return Err(CoreError::Output(format!(
				"unsupported sample format {:?}",
				default.sample_format()
			)));
		}
		let sample_rate = default.sample_rate();
		let mut config = default.config();
		let frames = sample_rate * BUFFER_MS / 1000;
		if let SupportedBufferSize::Range { min, max } = default.buffer_size()
			&& frames >= *min
			&& frames <= *max
		{
			config.buffer_size = BufferSize::Fixed(frames);
		}
		let channels = config.channels as usize;

		let state = Arc::new(Mutex::new(Callback {
			core: MixerCore::new(sample_rate),
			scratch: Vec::new(),
		}));
		let cb_state = state.clone();
		let (cmd_tx, cmd_rx) = unbounded();
		let (rendered_tx, rendered_rx) = unbounded();
		let stream = device
			.build_output_stream(
				config,
				move |data: &mut [f32], _| {
					let cb = &mut *cb_state.lock();
					while let Ok(cmd) = cmd_rx.try_recv() {
						cb.core.handle(cmd);
					}
					let channels = channels.max(1);
					let frames = data.len() / channels;
					if frames == 0 {
						return;
					}
					cb.scratch.clear();
					cb.scratch.resize(frames * 2, 0.0);
					let (core, scratch) = (&mut cb.core, &mut cb.scratch);
					if let Some(token) = core.render(scratch) {
						let _ = rendered_tx.try_send((token, Instant::now()));
					}
					// Silence first, so a tail shorter than one frame never replays stale samples.
					data.fill(0.0);
					let (lr, _) = cb.scratch.as_chunks::<2>();
					match channels {
						1 => {
							for (d, [l, r]) in data.iter_mut().zip(lr) {
								*d = (l + r) * 0.5;
							}
						}
						2 => data[..frames * 2].copy_from_slice(&cb.scratch),
						// Stereo goes to the first two channels, the rest stay silent.
						_ => {
							for (frame, pair) in data.chunks_exact_mut(channels).zip(lr) {
								frame[..2].copy_from_slice(pair);
							}
						}
					}
				},
				|err| eprintln!("audio output error: {err}"),
				None,
			)
			.map_err(|e| CoreError::Output(e.to_string()))?;
		stream
			.play()
			.map_err(|e| CoreError::Output(e.to_string()))?;
		Ok(Output {
			sample_rate,
			cmd_tx,
			rendered_rx,
			_stream: stream,
			_ui_thread: PhantomData,
		})
	}

	pub fn send(&self, cmd: MixerCmd) {
		let _ = self.cmd_tx.try_send(cmd);
	}

	pub fn rendered(&self) -> &Receiver<(u64, Instant)> {
		&self.rendered_rx
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	const RATE: u32 = 1000;

	fn const_ring(v: f32, frames: usize) -> Arc<PcmRing> {
		let ring = Arc::new(PcmRing::new(RATE, 3.0));
		ring.push(&vec![v; frames * 2]);
		ring
	}

	/// Cushion 0, so tests with tiny rings play at once; the cushion has its own tests.
	fn mixer() -> MixerCore {
		let mut m = MixerCore::new(RATE);
		m.cushion_ms = 0;
		m.hiss_level = 0.0;
		m
	}

	fn render_frames(mixer: &mut MixerCore, frames: usize) -> Vec<f32> {
		let mut out = vec![0.0; frames * 2];
		mixer.render(&mut out);
		out
	}

	#[test]
	fn render_reads_active_ring() {
		let mut mixer = mixer();
		mixer.handle(MixerCmd::SetActive {
			ring: Some(const_ring(0.25, 40)),
			token: 7,
		});
		// The first 25 ms fade in from silence.
		render_frames(&mut mixer, 25);
		assert_eq!(render_frames(&mut mixer, 4), [0.25; 8]);
	}

	#[test]
	fn start_from_silence_fades_in() {
		let mut mixer = mixer();
		mixer.handle(MixerCmd::SetActive {
			ring: Some(const_ring(1.0, 40)),
			token: 1,
		});
		let out = render_frames(&mut mixer, 25);
		assert_eq!(out[0], 0.0, "no jump from silence to full level");
		assert!(out.windows(2).all(|w| w[1] >= w[0]), "rises steadily");
	}

	#[test]
	fn fast_steps_keep_every_fade() {
		let mut mixer = mixer();
		mixer.handle(MixerCmd::SetActive {
			ring: Some(const_ring(1.0, 100)),
			token: 1,
		});
		render_frames(&mut mixer, 30);
		mixer.handle(MixerCmd::SetActive {
			ring: Some(const_ring(0.0, 100)),
			token: 2,
		});
		render_frames(&mut mixer, 10);
		// A second step 10 ms into the first fade: the first ring keeps fading, not cut off.
		mixer.handle(MixerCmd::SetActive {
			ring: Some(const_ring(0.0, 100)),
			token: 3,
		});
		let p = 10.0 / 25.0;
		let out = render_frames(&mut mixer, 1);
		assert!((out[0] - (p * FRAC_PI_2).cos()).abs() < 1e-3, "{}", out[0]);
	}

	#[test]
	fn ran_dry_waits_for_cushion() {
		let mut mixer = mixer();
		mixer.cushion_ms = 100;
		let ring = Arc::new(PcmRing::new(RATE, 3.0));
		mixer.handle(MixerCmd::SetActive {
			ring: Some(ring.clone()),
			token: 1,
		});
		ring.push(&vec![1.0; 150 * 2]);
		render_frames(&mut mixer, 200);
		// Ran dry; a trickle below the cushion stays queued instead of stuttering out.
		ring.push(&[1.0; 20 * 2]);
		assert_eq!(render_frames(&mut mixer, 10), [0.0; 20]);
		assert_eq!(ring.len_ms(), 20);
		ring.push(&vec![1.0; 100 * 2]);
		let out = render_frames(&mut mixer, 10);
		assert_eq!(out[0], 0.0, "resumes with a fade-in");
		assert!(out[19] > 0.0);
	}

	#[test]
	fn tune_fills_the_gap_with_hiss() {
		let mut mixer = MixerCore::new(RATE);
		mixer.handle(MixerCmd::SetActive {
			ring: Some(const_ring(0.5, 2000)),
			token: 1,
		});
		render_frames(&mut mixer, 100);
		mixer.handle(MixerCmd::Tune);
		// After the crossfade only hiss is left: never silence, never loud.
		let out = render_frames(&mut mixer, 200);
		let tail = &out[100..];
		assert!(
			tail.iter().any(|s| s.abs() > 0.001),
			"hiss between stations"
		);
		assert!(tail.iter().all(|s| s.abs() <= HISS_LEVEL), "soft");
		// The next station arrives: hiss eases out under it.
		mixer.handle(MixerCmd::SetActive {
			ring: Some(const_ring(-0.5, 2000)),
			token: 2,
		});
		render_frames(&mut mixer, 30);
		assert_eq!(
			render_frames(&mut mixer, 4),
			[-0.5; 8],
			"only the station once it plays"
		);
	}

	#[test]
	fn stop_is_silent() {
		let mut mixer = MixerCore::new(RATE);
		mixer.handle(MixerCmd::Tune);
		render_frames(&mut mixer, 100);
		mixer.handle(MixerCmd::SetActive {
			ring: None,
			token: 0,
		});
		let out = render_frames(&mut mixer, 100);
		assert!(
			out[60..].iter().all(|&s| s == 0.0),
			"hiss fades to silence on stop"
		);
	}

	#[test]
	fn start_waits_for_cushion() {
		let mut mixer = MixerCore::new(RATE);
		let ring = Arc::new(PcmRing::new(RATE, 3.0));
		mixer.handle(MixerCmd::SetActive {
			ring: Some(ring.clone()),
			token: 9,
		});
		// Half a second (500 frames at 1 kHz) is not enough to start on.
		ring.push(&vec![1.0; 500 * 2]);
		let mut out = vec![0.0; 20];
		assert_eq!(mixer.render(&mut out), None);
		assert_eq!(ring.len_ms(), 500, "nothing taken while buffering");
		ring.push(&vec![1.0; 500 * 2]);
		assert_eq!(
			mixer.render(&mut out),
			Some(9),
			"starts once CUSHION_MS is buffered"
		);
	}

	#[test]
	fn crossfade_spans_25_ms() {
		let mut mixer = mixer();
		mixer.handle(MixerCmd::SetActive {
			ring: Some(const_ring(1.0, 100)),
			token: 1,
		});
		render_frames(&mut mixer, 5);
		mixer.handle(MixerCmd::SetActive {
			ring: Some(const_ring(-1.0, 100)),
			token: 2,
		});
		// Equal-power: the old ring leaves as cos, the new one enters as sin.
		let out = render_frames(&mut mixer, 25);
		assert!((out[0] - 1.0).abs() < 1e-6);
		let p = 12.0 / 25.0;
		let mid = (p * FRAC_PI_2).cos() - (p * FRAC_PI_2).sin();
		assert!((out[24] - mid).abs() < 1e-3);
		assert!((out[25] - mid).abs() < 1e-3);
		// After exactly 25 ms of frames the crossfade is over: output is the new ring.
		assert_eq!(render_frames(&mut mixer, 2), [-1.0; 4]);
	}

	#[test]
	fn render_reports_token_on_first_real_sample() {
		let mut mixer = mixer();
		let ring = Arc::new(PcmRing::new(RATE, 3.0));
		mixer.handle(MixerCmd::SetActive {
			ring: Some(ring.clone()),
			token: 5,
		});
		let mut out = [0.0; 4];
		assert_eq!(mixer.render(&mut out), None);
		ring.push(&[1.0, 1.0]);
		assert_eq!(mixer.render(&mut out), Some(5));
		assert_eq!(mixer.render(&mut out), None);
	}

	#[test]
	fn underrun_outputs_silence() {
		let mut mixer = mixer();
		mixer.handle(MixerCmd::SetActive {
			ring: Some(const_ring(1.0, 2)),
			token: 1,
		});
		render_frames(&mut mixer, 2);
		let mut out = [1.0; 4];
		assert_eq!(mixer.render(&mut out), None);
		assert_eq!(out, [0.0; 4]);
	}

	#[test]
	fn mute_keeps_draining_ring() {
		let mut mixer = mixer();
		mixer.handle(MixerCmd::SetActive {
			ring: Some(const_ring(0.5, 2)),
			token: 1,
		});
		mixer.handle(MixerCmd::Mute(true));
		assert_eq!(render_frames(&mut mixer, 2), [0.0; 4]);
		// The muted render pulled the samples, so nothing is left once unmuted.
		mixer.handle(MixerCmd::Mute(false));
		assert_eq!(render_frames(&mut mixer, 2), [0.0; 4]);
	}

	#[test]
	fn volume_scales_output() {
		let mut mixer = mixer();
		mixer.handle(MixerCmd::SetActive {
			ring: Some(const_ring(1.0, 40)),
			token: 1,
		});
		render_frames(&mut mixer, 25);
		mixer.handle(MixerCmd::Volume(0.5));
		assert_eq!(render_frames(&mut mixer, 2), [0.5; 4]);
		mixer.handle(MixerCmd::Volume(2.0));
		assert_eq!(render_frames(&mut mixer, 2), [1.0; 4]);
	}

	#[test]
	fn same_ring_again_is_not_drained_twice() {
		let mut mixer = mixer();
		let ring = const_ring(1.0, 100);
		mixer.handle(MixerCmd::SetActive {
			ring: Some(ring.clone()),
			token: 1,
		});
		render_frames(&mut mixer, 10);
		mixer.handle(MixerCmd::SetActive {
			ring: Some(ring.clone()),
			token: 2,
		});
		render_frames(&mut mixer, 10);
		assert_eq!(ring.len_ms(), 80, "20 frames taken at 1000 Hz, not 30");
	}
}
