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
use crate::ring::PcmRing;

/// Warm-swap crossfade length.
const CROSSFADE_MS: u64 = 25;
/// Target stream buffer when the device range allows it.
const BUFFER_MS: u32 = 10;

pub enum MixerCmd {
	SetActive {
		ring: Option<Arc<PcmRing>>,
		token: u64,
	},
	Volume(f32),
	Mute(bool),
}

pub struct MixerCore {
	fade_frames: usize,
	active: Option<Arc<PcmRing>>,
	fading: Option<(Arc<PcmRing>, usize)>,
	pending: Option<u64>,
	volume: f32,
	muted: bool,
	a: Vec<f32>,
	b: Vec<f32>,
}

impl MixerCore {
	pub fn new(rate: u32) -> MixerCore {
		MixerCore {
			fade_frames: (rate as u64 * CROSSFADE_MS / 1000) as usize,
			active: None,
			fading: None,
			pending: None,
			volume: 1.0,
			muted: false,
			a: Vec::new(),
			b: Vec::new(),
		}
	}

	pub fn handle(&mut self, cmd: MixerCmd) {
		match cmd {
			MixerCmd::SetActive { ring, token } => {
				if let Some(old) = self.active.take() {
					self.fading = Some((old, 0));
				}
				self.active = ring;
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

		self.a.clear();
		self.a.resize(out.len(), 0.0);
		let got = match &self.active {
			Some(ring) => ring.pop_into(&mut self.a),
			None => 0,
		};

		let mut fade = self.fading.take();
		if let Some((ring, pos)) = fade.as_mut() {
			self.b.clear();
			self.b.resize(out.len(), 0.0);
			ring.pop_into(&mut self.b);
			let len = self.fade_frames.max(1) as f32;
			for f in 0..frames {
				// Equal-power crossfade: old ring leaves as cos, new one enters as sin.
				let p = (*pos).min(self.fade_frames) as f32 / len;
				let (g_out, g_in) = ((p * FRAC_PI_2).cos(), (p * FRAC_PI_2).sin());
				self.a[2 * f] = self.a[2 * f] * g_in + self.b[2 * f] * g_out;
				self.a[2 * f + 1] = self.a[2 * f + 1] * g_in + self.b[2 * f + 1] * g_out;
				*pos += 1;
			}
			if *pos >= self.fade_frames {
				fade = None;
			}
		}
		self.fading = fade;

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
	// The cpal stream is Send; the mixer is not, so swaps can't race the callback from another thread.
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
					if channels == 2 {
						data.copy_from_slice(&cb.scratch);
					} else {
						// Stereo goes to the first two channels, the rest stay silent.
						for f in 0..frames {
							data[f * channels] = cb.scratch[2 * f];
							if channels > 1 {
								data[f * channels + 1] = cb.scratch[2 * f + 1];
							}
							for c in 2..channels {
								data[f * channels + c] = 0.0;
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

	fn render_frames(mixer: &mut MixerCore, frames: usize) -> Vec<f32> {
		let mut out = vec![0.0; frames * 2];
		mixer.render(&mut out);
		out
	}

	#[test]
	fn render_reads_active_ring() {
		let mut mixer = MixerCore::new(RATE);
		mixer.handle(MixerCmd::SetActive {
			ring: Some(const_ring(0.25, 10)),
			token: 7,
		});
		assert_eq!(render_frames(&mut mixer, 4), [0.25; 8]);
	}

	#[test]
	fn crossfade_spans_25_ms() {
		let mut mixer = MixerCore::new(RATE);
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
		let mut mixer = MixerCore::new(RATE);
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
		let mut mixer = MixerCore::new(RATE);
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
		let mut mixer = MixerCore::new(RATE);
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
		let mut mixer = MixerCore::new(RATE);
		mixer.handle(MixerCmd::SetActive {
			ring: Some(const_ring(1.0, 4)),
			token: 1,
		});
		mixer.handle(MixerCmd::Volume(0.5));
		assert_eq!(render_frames(&mut mixer, 2), [0.5; 4]);
		mixer.handle(MixerCmd::Volume(2.0));
		assert_eq!(render_frames(&mut mixer, 2), [1.0; 4]);
	}
}
