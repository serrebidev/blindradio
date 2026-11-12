use std::f32::consts::FRAC_PI_2;
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, ErrorKind, SampleFormat, SupportedBufferSize, SupportedStreamConfig};
use crossbeam_channel::{Receiver, Sender, unbounded};
use parking_lot::Mutex;

use crate::Result;
use crate::eq::Eq;
use crate::record::Recorder;
use crate::error::CoreError;
use crate::ring::PcmRing;
use crate::settings::EqSettings;
use crate::timeshift::{ShiftBuffer, ShiftStatus};

/// Warm-swap crossfade length.
const CROSSFADE_MS: u64 = 25;
/// Ease-out at an underrun, so running dry does not click.
const TAIL_MS: u64 = 5;
/// Target stream buffer when the device range allows it.
const BUFFER_MS: u32 = 10;
/// Peak of the hiss between stations: soft, like a dial between stations. Fixed, so not
/// scaled by the volume, which jumps to each station's remembered level as it is tuned.
/// Mute still silences it. Same level the hiss had at the default volume, 0.8.
// ponytail: fixed level and tone; the on/off setting listeners asked for exists
// (MixerCmd::Hiss, Settings' FM sound), louder/quieter would ride the same path.
const HISS_LEVEL: f32 = 0.064;
/// One-pole lowpass on the hiss: dulls white noise toward a radio's softer static.
const HISS_TONE: f32 = 0.3;

pub enum MixerCmd {
	/// The Buffer settings changed.
	Buffers {
		start_ms: u32,
		refill_limit_ms: u32,
	},
	SetActive {
		ring: Option<Arc<PcmRing>>,
		token: u64,
	},
	Volume(f32),
	Mute(bool),
	Eq(EqSettings),
	/// The FM sound: whether the between-stations hiss sounds at all.
	Hiss(bool),
	/// Leave the active station for the space between stations: it fades out into soft hiss,
	/// which lasts until a `SetActive` ring has audio, so tuning never has dead air.
	/// `SetActive { ring: None }` (stop) is silence.
	Tune,
	/// Gives the active station a time-shift buffer. Playback stays live until
	/// a pause or a seek back moves the cursor into the buffer.
	ShiftAttach {
		buffer: Arc<ShiftBuffer>,
		status: Arc<ShiftStatus>,
	},
	/// Holds playback where it is; the stream keeps filling the buffer.
	ShiftPause(bool),
	/// Moves the cursor by signed seconds from where it is (or from live).
	ShiftSeek(i32),
	/// Moves the cursor to an absolute frame (a segment's start).
	ShiftSeekTo(u64),
	/// Returns to the live edge.
	ShiftLive,
	/// The playing station's recorder slot: the mixer writes what the
	/// listener hears into it, live or time-shifted, so a recording
	/// starts where the listener is. The decoder runs ahead by the ring.
	RecordSlot(Option<Arc<Mutex<Option<Recorder>>>>),
}

/// Where the playing ring stands.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Flow {
	/// Newly active or ran dry: waits for `cushion_ms` of audio, so playback never starts on
	/// a few hundred ms only to stop again, and one clean gap replaces a stutter.
	Buffering,
	Playing,
}

/// Each dry-out in a row multiplies the required refill by this.
const REBUFFER_GROWTH: u32 = 3;
/// Seconds of uninterrupted play that earn the base cushion back.
const HEALTHY_SECONDS: u64 = 60;

pub struct MixerCore {
	fade_frames: usize,
	tail_frames: usize,
	cushion_ms: u32,
	/// The refill a fresh station needs; `cushion_ms` grows from here.
	base_cushion_ms: u32,
	/// Dry-outs in a row on the active ring; grows `cushion_ms`.
	strikes: u32,
	/// The most a stream that keeps running dry must refill before it may sound
	/// again. A server that dips below its bitrate for seconds at a time (TopRadio
	/// falls up to ~10 s behind, then catches up) stutters against a 1 s cushion; a
	/// deep refill turns that into long plays between clean gaps. Healthy streams
	/// never run dry, so they never see this. The ring's measured dip
	/// (`PcmRing::dip_ms`) sets the refill at once, before the strikes grow it.
	refill_limit_ms: u32,
	/// Output rate, for the healthy-play reset window.
	rate: u32,
	active: Option<Arc<PcmRing>>,
	flow: Flow,
	/// Frames the active ring has sounded since it (re)started; drives its fade-in.
	sounded: usize,
	/// Rings fading out, each with frames faded so far. A fast step can leave several.
	fading: Vec<(Arc<PcmRing>, usize)>,
	pending: Option<u64>,
	volume: f32,
	muted: bool,
	eq: Eq,
	/// Kept to retune the EQ when the device rate changes.
	eq_settings: EqSettings,
	/// Between stations or waiting on the active one: hiss fills the gap.
	hiss: bool,
	/// The FM sound setting: off, the gap between stations is silent.
	hiss_enabled: bool,
	hiss_level: f32,
	hiss_gain: f32,
	hiss_step: f32,
	noise: u32,
	noise_lp: f32,
	a: Vec<f32>,
	b: Vec<f32>,
	/// Time-shift state for the active station; None plays the ring live.
	shift: Option<(Arc<ShiftBuffer>, Arc<ShiftStatus>)>,
	/// Absolute frame the shifted playback reads next; None is live.
	shift_cursor: Option<u64>,
	shift_paused: bool,
	/// Recorder slot for the as-heard tap; see RecordSlot.
	record_slot: Option<Arc<Mutex<Option<Recorder>>>>,
}

impl MixerCore {
	pub fn new(rate: u32) -> MixerCore {
		let mut m = MixerCore {
			fade_frames: 0,
			tail_frames: 0,
			cushion_ms: crate::ring::start_ms(),
			base_cushion_ms: crate::ring::start_ms(),
			strikes: 0,
			refill_limit_ms: crate::ring::refill_limit_ms(),
			rate,
			active: None,
			flow: Flow::Buffering,
			sounded: 0,
			fading: Vec::new(),
			pending: None,
			volume: 1.0,
			muted: false,
			eq: Eq::new(rate),
			eq_settings: EqSettings::default(),
			hiss: false,
			hiss_enabled: true,
			hiss_level: HISS_LEVEL,
			hiss_gain: 0.0,
			hiss_step: 0.0,
			noise: 0x9E37_79B9,
			noise_lp: 0.0,
			a: Vec::new(),
			b: Vec::new(),
			shift: None,
			shift_cursor: None,
			shift_paused: false,
			record_slot: None,
		};
		m.set_rate(rate);
		m
	}

	/// Retimes fades and retunes the EQ; rings and settings stay.
	fn set_rate(&mut self, rate: u32) {
		self.rate = rate;
		self.fade_frames = (rate as u64 * CROSSFADE_MS / 1000) as usize;
		self.tail_frames = (rate as u64 * TAIL_MS / 1000) as usize;
		self.hiss_step = 1.0 / self.fade_frames.max(1) as f32;
		self.eq = Eq::new(rate);
		self.eq.set(&self.eq_settings);
	}

	pub fn handle(&mut self, cmd: MixerCmd) {
		match cmd {
			MixerCmd::Hiss(on) => self.hiss_enabled = on,
			MixerCmd::Buffers {
				start_ms,
				refill_limit_ms,
			} => {
				self.base_cushion_ms = start_ms;
				self.refill_limit_ms = refill_limit_ms;
				// A stream partway through a deeper refill keeps it; the rest change now.
				if self.strikes == 0 {
					self.cushion_ms = start_ms;
				}
			}
			MixerCmd::Tune => {
				self.handle(MixerCmd::SetActive {
					ring: None,
					token: 0,
				});
				self.hiss = true;
			}
			MixerCmd::SetActive { ring, token } => {
				// A new station starts live and unshifted; its ShiftAttach follows.
				if let Some((_, status)) = &self.shift {
					status.attached.store(false, Ordering::Relaxed);
					status.cursor.store(u64::MAX, Ordering::Relaxed);
					status.paused.store(false, Ordering::Relaxed);
				}
				self.shift = None;
				self.shift_cursor = None;
				self.shift_paused = false;
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
					// A new station starts at the base cushion: its network
					// has not earned a deeper refill yet.
					self.cushion_ms = self.base_cushion_ms;
					self.strikes = 0;
				}
				self.pending = Some(token);
			}
			MixerCmd::ShiftAttach { buffer, status } => {
				status.attached.store(true, Ordering::Relaxed);
				status.paused.store(false, Ordering::Relaxed);
				status.jump(u64::MAX);
				self.shift = Some((buffer, status));
				self.shift_cursor = None;
				self.shift_paused = false;
			}
			MixerCmd::ShiftPause(paused) => {
				if let Some((buffer, status)) = &self.shift {
					if paused && self.shift_cursor.is_none() {
						// Freeze where live playback stands; the buffer keeps filling past it.
						self.shift_cursor = Some(status.live_frame(buffer));
					}
					self.shift_paused = paused;
					status.paused.store(paused, Ordering::Relaxed);
					if !paused && self.shift_cursor == Some(status.live_frame(buffer)) {
						self.shift_cursor = None;
						status.cursor.store(u64::MAX, Ordering::Relaxed);
					}
				}
			}
			MixerCmd::ShiftSeek(seconds) => {
				if let Some((buffer, status)) = &self.shift {
					// From where playback stands, not the newest audio: a connect burst
					// puts that far ahead, and a step back from it would land forward.
					let live = status.live_frame(buffer);
					let start = buffer.start();
					let base = self.shift_cursor.unwrap_or(live) as i64;
					let target = (base + seconds as i64 * buffer.rate() as i64)
						.clamp(start as i64, live.max(start) as i64) as u64;
					self.shift_cursor = if target >= live { None } else { Some(target) };
					// Say where the playhead went at once, live or not, as one
					// jump. Render only writes the status while shifted, so a
					// cursor cleared here would otherwise leave the last
					// shifted frame standing — the status line and the
					// announcements would keep quoting a stale "behind
					// live" while the ring plays live. And an Original
					// recording reading the jump before the next render
					// would anchor at the old spot.
					status.jump(self.shift_cursor.unwrap_or(u64::MAX));
				}
			}
			MixerCmd::ShiftSeekTo(frame) => {
				if let Some((buffer, status)) = &self.shift {
					let live = status.live_frame(buffer);
					let start = buffer.start();
					let target = frame.clamp(start, live.max(start));
					self.shift_cursor = (target < live).then_some(target);
					status
						.cursor
						.store(self.shift_cursor.unwrap_or(u64::MAX), Ordering::Relaxed);
				}
			}
			MixerCmd::ShiftLive => {
				self.shift_cursor = None;
				self.shift_paused = false;
				if let Some((_, status)) = &self.shift {
					status.paused.store(false, Ordering::Relaxed);
					status.jump(u64::MAX);
				}
			}
			MixerCmd::RecordSlot(slot) => self.record_slot = slot,
			MixerCmd::Volume(v) => self.volume = v.clamp(0.0, crate::settings::MAX_VOLUME),
			MixerCmd::Mute(muted) => self.muted = muted,
			MixerCmd::Eq(s) => {
				self.eq.set(&s);
				self.eq_settings = s;
			}
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
		if let Some((_, status)) = &self.shift {
			let backlog = self.active.as_ref().map_or(0, |ring| ring.frames());
			status.backlog.store(backlog, Ordering::Relaxed);
		}
		// Time-shifted playback reads the buffer at the cursor instead of the
		// ring; the ring is drained and dropped so its producer never blocks.
		let shifted = self.shift.is_some() && (self.shift_paused || self.shift_cursor.is_some());
		if shifted {
			if let Some(ring) = &self.active {
				self.b.clear();
				self.b.resize(out.len(), 0.0);
				ring.pop_into(&mut self.b);
			}
			if !self.shift_paused {
				let cursor = self.shift_cursor.unwrap_or(0);
				let (buffer, status) = self.shift.as_ref().unwrap();
				let n = buffer.read_at(cursor, &mut self.a);
				got = n / 2;
				let next = cursor + got as u64;
				// Reaching live hands playback back to the ring, which plays on from this frame.
				self.shift_cursor = if next >= status.live_frame(buffer) { None } else { Some(next) };
			}
			if let Some((_, status)) = &self.shift {
				status
					.cursor
					.store(self.shift_cursor.unwrap_or(u64::MAX), Ordering::Relaxed);
			}
		}
		if !shifted && let Some(ring) = &self.active {
			let level = ring.len_ms();
			// A server that has dipped below real time gets that much in hand before it
			// sounds: one clean gap, not a stutter while the strikes catch up.
			let limit = self.refill_limit_ms.max(self.base_cushion_ms);
			let cushion = self.cushion_ms.max(ring.dip_ms().min(limit));
			if self.flow == Flow::Buffering && level > 0 && level >= cushion {
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
					// This stream has proven it cannot sustain the base
					// cushion: demand a deeper refill before it sounds again.
					self.strikes += 1;
					self.cushion_ms = self
						.base_cushion_ms
						.saturating_mul(REBUFFER_GROWTH.saturating_pow(self.strikes))
						.min(self.refill_limit_ms.max(self.base_cushion_ms));
				}
			}
		}
		// Record what the listener hears: the ring's frames live, the
		// shifted audio while shifted (silence while paused, so the
		// file's timeline matches the listening). An Original recorder
		// ignores PCM. Raw station audio: before fades, EQ and volume.
		if let Some(slot) = &self.record_slot
			&& let Some(rec) = slot.lock().as_mut()
		{
			let frames = if self.shift_paused { out.len() / 2 } else { got };
			let _ = rec.write_pcm(&self.a[..frames * 2]);
		}
		// Equal-power: the active ring enters as sin, from silence or across a crossfade.
		for f in 0..got {
			let p = (self.sounded + f).min(self.fade_frames) as f32 / len;
			let g = (p * FRAC_PI_2).sin();
			self.a[2 * f] *= g;
			self.a[2 * f + 1] *= g;
		}
		self.sounded += got;
		// A long clean run earns the base cushion back.
		if self.flow == Flow::Playing
			&& self.cushion_ms > self.base_cushion_ms
			&& self.sounded as u64 >= self.rate as u64 * HEALTHY_SECONDS
		{
			self.cushion_ms = self.base_cushion_ms;
			self.strikes = 0;
		}

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

		// Before the hiss: the Loudness leveler would ride it up to near full scale.
		self.eq.process(&mut self.a);

		// Station audio takes the volume, the hiss does not.
		let vol = if self.muted { 0.0 } else { self.volume };
		for s in &mut self.a {
			*s *= vol;
		}
		let hiss_mix = if self.muted { 0.0 } else { 1.0 };

		// Hiss covers every frame the active station does not, easing in and out over a crossfade.
		for f in 0..frames {
			let want = if self.hiss && self.hiss_enabled && !self.shift_paused && f >= got {
				1.0
			} else {
				0.0
			};
			self.hiss_gain += (want - self.hiss_gain).clamp(-self.hiss_step, self.hiss_step);
			if self.hiss_gain > 0.0 {
				// xorshift32 white noise, lowpassed.
				self.noise ^= self.noise << 13;
				self.noise ^= self.noise >> 17;
				self.noise ^= self.noise << 5;
				let white = self.noise as f32 / u32::MAX as f32 * 2.0 - 1.0;
				self.noise_lp += (white - self.noise_lp) * HISS_TONE;
				let s = self.noise_lp * self.hiss_level * self.hiss_gain * hiss_mix;
				self.a[2 * f] += s;
				self.a[2 * f + 1] += s;
			}
		}

		for (o, &s) in out.iter_mut().zip(&self.a) {
			*o = s.clamp(-1.0, 1.0);
		}

		if got > 0 { self.pending.take() } else { None }
	}
}

struct Callback {
	core: MixerCore,
	scratch: Vec<f32>,
}

/// Announced by `poll_devices`.
#[derive(Debug, PartialEq, Eq)]
pub enum DeviceEvent {
	/// The chosen device went away; playing through the default.
	Lost,
	/// The chosen device is back and playing.
	Back(String),
}

/// How long a chosen device that failed to open is left alone, unless it is unplugged and back sooner.
const HOLD: Duration = Duration::from_secs(30);

/// Which device plays and when to move; pure, so `Output::poll_devices` is testable without hardware.
#[derive(Debug, Default)]
pub struct Devices {
	pub chosen: Option<String>,
	/// The chosen name while the stream is on it, `None` while on the default.
	pub active: Option<String>,
	/// When the chosen device failed to open, and whether it has left the list since.
	/// Held, it is not tried again, so a broken device does not flap Lost and Back.
	held: Option<(Instant, bool)>,
}

impl Devices {
	pub fn new(chosen: Option<&str>, active: Option<&str>) -> Devices {
		Devices {
			chosen: chosen.map(str::to_string),
			active: active.map(str::to_string),
			held: None,
		}
	}

	/// One poll. `open(Some(name))` moves the stream to that device and `open(None)` to the
	/// default; each is true when the stream is now there.
	pub fn poll(
		&mut self,
		errored: bool,
		available: &[String],
		now: Instant,
		mut open: impl FnMut(Option<&str>) -> bool,
	) -> Option<DeviceEvent> {
		let Some(chosen) = self.chosen.clone() else {
			// The default itself died (unplugged with nothing chosen): reopen whatever is default now.
			if errored {
				open(None);
			}
			return None;
		};
		let present = available.contains(&chosen);
		if let Some((since, gone)) = &mut self.held {
			*gone |= !present;
			if (*gone && present) || now.duration_since(*since) >= HOLD {
				self.held = None;
			}
		}
		if self.active.as_ref() == Some(&chosen) {
			if present && !errored {
				return None;
			}
			// Errored but still listed: a silent reopen in place usually brings it back.
			if present && open(Some(&chosen)) {
				return None;
			}
			if present {
				self.held = Some((now, false));
			}
			if !open(None) {
				return None;
			}
			self.active = None;
			return Some(DeviceEvent::Lost);
		}
		if present && self.held.is_none() {
			if open(Some(&chosen)) {
				self.active = Some(chosen.clone());
				return Some(DeviceEvent::Back(chosen));
			}
			self.held = Some((now, false));
		}
		if errored {
			open(None);
		}
		None
	}
}

fn device_name(device: &cpal::Device) -> Option<String> {
	device.description().ok().map(|d| d.name().to_string())
}

/// Names of every output device of the default host, default first.
pub fn output_devices() -> Vec<String> {
	let host = cpal::default_host();
	let mut names: Vec<String> = host
		.default_output_device()
		.and_then(|d| device_name(&d))
		.into_iter()
		.collect();
	for d in host.output_devices().into_iter().flatten() {
		if let Some(n) = device_name(&d)
			&& !names.contains(&n)
		{
			names.push(n);
		}
	}
	names
}

/// The named device, or the default when the name is not found; the name comes back only when found.
fn find_device(name: Option<&str>) -> Result<(cpal::Device, Option<String>)> {
	let host = cpal::default_host();
	if let Some(name) = name
		&& let Some(d) = host
			.output_devices()
			.into_iter()
			.flatten()
			.find(|d| device_name(d).as_deref() == Some(name))
	{
		return Ok((d, Some(name.to_string())));
	}
	let d = host
		.default_output_device()
		.ok_or_else(|| CoreError::Output("no default output device".into()))?;
	Ok((d, None))
}

fn out_err(e: impl std::fmt::Display) -> CoreError {
	CoreError::Output(e.to_string())
}

/// f32 at `rate` when the device supports it, otherwise the device default.
fn pick_config(device: &cpal::Device, rate: Option<u32>) -> Result<SupportedStreamConfig> {
	let default = device.default_output_config().map_err(out_err)?;
	let f32_ok = default.sample_format() == SampleFormat::F32;
	if let Some(rate) = rate
		&& !(f32_ok && default.sample_rate() == rate)
		&& let Some(c) = device
			.supported_output_configs()
			.into_iter()
			.flatten()
			.filter(|c| c.sample_format() == SampleFormat::F32)
			.filter_map(|c| c.try_with_sample_rate(rate))
			.max_by_key(|c| c.channels() == default.channels())
	{
		return Ok(c);
	}
	if !f32_ok {
		return Err(CoreError::Output(format!(
			"unsupported sample format {:?}",
			default.sample_format()
		)));
	}
	Ok(default)
}

/// Built paused; every stream shares the one mixer, so a new device carries on where the last left off.
fn build_stream(
	device: &cpal::Device,
	supported: SupportedStreamConfig,
	state: &Arc<Mutex<Callback>>,
	cmd_rx: &Receiver<MixerCmd>,
	rendered_tx: &Sender<(u64, Instant)>,
	errored: &Arc<AtomicBool>,
) -> Result<cpal::Stream> {
	let mut config = supported.config();
	let frames = supported.sample_rate() * BUFFER_MS / 1000;
	if let SupportedBufferSize::Range { min, max } = supported.buffer_size()
		&& frames >= *min
		&& frames <= *max
	{
		config.buffer_size = BufferSize::Fixed(frames);
	}
	let channels = config.channels as usize;
	let (cb_state, cmd_rx, rendered_tx, errored) = (
		state.clone(),
		cmd_rx.clone(),
		rendered_tx.clone(),
		errored.clone(),
	);
	device
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
			// Only a flag: `poll_devices` decides on the UI thread. Glitches and reroutes keep playing.
			move |err| {
				if !matches!(
					err.kind(),
					ErrorKind::Xrun | ErrorKind::DeviceChanged | ErrorKind::RealtimeDenied
				) {
					errored.store(true, Ordering::Relaxed);
				}
			},
			None,
		)
		.map_err(out_err)
}

/// Device output stream; built, driven and dropped on the UI thread.
pub struct Output {
	pub sample_rate: u32,
	cmd_tx: Sender<MixerCmd>,
	rendered_rx: Receiver<(u64, Instant)>,
	// Kept so a rebuilt stream reads the same commands and reports to the same receiver.
	cmd_rx: Receiver<MixerCmd>,
	rendered_tx: Sender<(u64, Instant)>,
	state: Arc<Mutex<Callback>>,
	devices: Devices,
	/// Set by the current stream's error callback.
	errored: Arc<AtomicBool>,
	rate_changed: Option<u32>,
	// Dropping the stream stops playback.
	stream: cpal::Stream,
	// Not Send: Output stays on the thread that built it, so mixer commands come from one thread in order.
	_ui_thread: PhantomData<Rc<()>>,
}

impl Output {
	pub fn start_default() -> Result<Output> {
		Output::start(None)
	}

	/// A `device` not found, or found but failing to open, plays through the default and stays
	/// the choice; `poll_devices` moves to it once it opens.
	pub fn start(device: Option<&str>) -> Result<Output> {
		let (cmd_tx, cmd_rx) = unbounded();
		let (rendered_tx, rendered_rx) = unbounded();
		let launch = |name: Option<&str>| -> Result<_> {
			let (dev, active) = find_device(name)?;
			let config = pick_config(&dev, None)?;
			let sample_rate = config.sample_rate();
			let state = Arc::new(Mutex::new(Callback {
				core: MixerCore::new(sample_rate),
				scratch: Vec::new(),
			}));
			let errored = Arc::new(AtomicBool::new(false));
			let stream = build_stream(&dev, config, &state, &cmd_rx, &rendered_tx, &errored)?;
			stream.play().map_err(out_err)?;
			Ok((stream, state, sample_rate, active, errored))
		};
		let (stream, state, sample_rate, active, errored) = match launch(device) {
			Err(_) if device.is_some() => launch(None)?,
			r => r?,
		};
		Ok(Output {
			sample_rate,
			cmd_tx,
			rendered_rx,
			cmd_rx,
			rendered_tx,
			state,
			devices: Devices::new(device, active.as_deref()),
			errored,
			rate_changed: None,
			stream,
			_ui_thread: PhantomData,
		})
	}

	/// Moves playback to `device` (`None` = default) without a restart: same mixer, rings and settings.
	pub fn switch(&mut self, device: Option<&str>) -> Result<()> {
		let active = self.reopen(device)?;
		self.devices = Devices::new(device, active.as_deref());
		Ok(())
	}

	/// The device name when that device was found, `None` when on the default.
	fn reopen(&mut self, device: Option<&str>) -> Result<Option<String>> {
		let (dev, active) = find_device(device)?;
		let config = pick_config(&dev, Some(self.sample_rate))?;
		let rate = config.sample_rate();
		let errored = Arc::new(AtomicBool::new(false));
		let stream = build_stream(
			&dev,
			config,
			&self.state,
			&self.cmd_rx,
			&self.rendered_tx,
			&errored,
		)?;
		// Old one paused first, so two streams never pull from the mixer at once.
		let _ = self.stream.pause();
		let old = self.sample_rate;
		// Retimed before the new stream's first callback, so it never renders at the old rate.
		if rate != old {
			self.state.lock().core.set_rate(rate);
		}
		if let Err(e) = stream.play() {
			if rate != old {
				self.state.lock().core.set_rate(old);
			}
			let _ = self.stream.play();
			return Err(out_err(e));
		}
		if rate != old {
			self.sample_rate = rate;
			self.rate_changed = Some(rate);
		}
		self.stream = stream;
		self.errored = errored;
		Ok(active)
	}

	/// The new rate, once, after a switch to a device that could not keep the old one.
	pub fn rate_changed(&mut self) -> Option<u32> {
		self.rate_changed.take()
	}

	pub fn chosen(&self) -> Option<&str> {
		self.devices.chosen.as_deref()
	}

	/// Call about every 2 s with `output_devices`. Rules in `Devices::poll`.
	pub fn poll_devices(&mut self, list: &dyn Fn() -> Vec<String>) -> Option<DeviceEvent> {
		let errored = self.errored.load(Ordering::Relaxed);
		let mut devices = std::mem::take(&mut self.devices);
		let event = devices.poll(errored, &list(), Instant::now(), |name| {
			self.reopen(name)
				.is_ok_and(|active| name.is_none() || active.as_deref() == name)
		});
		self.devices = devices;
		event
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
		m.base_cushion_ms = 0;
		m.hiss_level = 0.0;
		m
	}

	fn render_frames(mixer: &mut MixerCore, frames: usize) -> Vec<f32> {
		let mut out = vec![0.0; frames * 2];
		mixer.render(&mut out);
		out
	}

	#[test]
	fn live_recording_takes_what_is_heard_not_the_rings_backlog() {
		// A connect burst sits in the ring ahead of the listener. The
		// recording holds only the frames played, never the backlog.
		let dir = tempfile::tempdir().unwrap();
		let settings = crate::settings::RecordSettings {
			format: crate::settings::RecordFormat::Wav,
			..Default::default()
		};
		let rec = Recorder::start(dir.path(), "Test", &settings, RATE, "mp3").unwrap();
		let slot = Arc::new(Mutex::new(Some(rec)));
		let mut mixer = mixer();
		mixer.handle(MixerCmd::SetActive {
			ring: Some(const_ring(0.25, 2000)),
			token: 1,
		});
		mixer.handle(MixerCmd::RecordSlot(Some(slot.clone())));
		render_frames(&mut mixer, 300);
		let path = slot.lock().take().unwrap().stop().unwrap();
		let reader = hound::WavReader::open(&path).unwrap();
		assert_eq!(reader.duration(), 300, "played frames only");
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
	fn eq_command_reaches_the_rendered_audio() {
		// A 100 Hz sine sits in the low shelf's range: Bass Boost at
		// +6 dB must come out measurably louder than the same ring
		// rendered with the EQ off. Guards the whole path: command
		// handling, Eq state, and the render tail.
		let frames = 3000;
		let ring = Arc::new(PcmRing::new(RATE, 3.0));
		let sine: Vec<f32> = (0..frames)
			.flat_map(|i| {
				let v = (2.0 * std::f32::consts::PI * 100.0 * i as f32 / RATE as f32).sin() * 0.25;
				[v, v]
			})
			.collect();
		ring.push(&sine);
		let mut mixer = mixer();
		mixer.handle(MixerCmd::SetActive {
			ring: Some(ring),
			token: 1,
		});
		let rms = |out: &[f32]| {
			(out.iter().map(|v| v * v).sum::<f32>() / out.len() as f32).sqrt()
		};
		// Past the fade-in, with the EQ still off.
		render_frames(&mut mixer, 100);
		let off = rms(&render_frames(&mut mixer, 200));
		mixer.handle(MixerCmd::Eq(EqSettings {
			enabled: true,
			preset: crate::settings::EqPreset::BassBoost,
			bands: crate::settings::EqPreset::BassBoost.bands().unwrap(),
			..EqSettings::default()
		}));
		// Past the EQ's own 25 ms wet fade.
		render_frames(&mut mixer, 100);
		let on = rms(&render_frames(&mut mixer, 200));
		assert!(
			on > off * 1.3,
			"Bass Boost must raise a 100 Hz sine: off {off}, on {on}"
		);
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
		mixer.base_cushion_ms = 100;
		let ring = Arc::new(PcmRing::new(RATE, 3.0));
		mixer.handle(MixerCmd::SetActive {
			ring: Some(ring.clone()),
			token: 1,
		});
		ring.push(&vec![1.0; 150 * 2]);
		render_frames(&mut mixer, 200);
		// Ran dry; a trickle below the grown cushion (three times the base)
		// stays queued instead of stuttering out.
		ring.push(&[1.0; 20 * 2]);
		assert_eq!(render_frames(&mut mixer, 10), [0.0; 20]);
		assert_eq!(ring.len_ms(), 20);
		ring.push(&vec![1.0; 100 * 2]);
		assert_eq!(render_frames(&mut mixer, 10), [0.0; 20]);
		assert_eq!(ring.len_ms(), 120);
		ring.push(&vec![1.0; 200 * 2]);
		let out = render_frames(&mut mixer, 10);
		assert_eq!(out[0], 0.0, "resumes with a fade-in");
		assert!(out[19] > 0.0);
	}

	#[test]
	fn a_server_that_dipped_refills_past_its_dip() {
		let mut mixer = mixer();
		mixer.cushion_ms = 100;
		mixer.base_cushion_ms = 100;
		let ring = Arc::new(PcmRing::new(RATE, 3.0));
		// 400 ms ahead, then nothing for 400 ms: the server fell 400 ms behind its lead.
		ring.push(&vec![1.0; 400 * 2]);
		std::thread::sleep(std::time::Duration::from_millis(400));
		ring.push(&[1.0; 2]);
		ring.clear();
		mixer.handle(MixerCmd::SetActive {
			ring: Some(ring.clone()),
			token: 1,
		});
		ring.push(&vec![1.0; 200 * 2]);
		assert_eq!(
			render_frames(&mut mixer, 10),
			[0.0; 20],
			"twice the cushion, still short of the dip"
		);
		ring.push(&vec![1.0; 400 * 2]);
		assert!(
			render_frames(&mut mixer, 10)[19] > 0.0,
			"plays once the dip is covered"
		);
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
	fn fm_sound_off_tunes_in_silence() {
		let mut mixer = MixerCore::new(RATE);
		mixer.handle(MixerCmd::SetActive {
			ring: Some(const_ring(0.5, 2000)),
			token: 1,
		});
		render_frames(&mut mixer, 100);
		mixer.handle(MixerCmd::Hiss(false));
		mixer.handle(MixerCmd::Tune);
		// Past the old station's crossfade, the gap the hiss filled
		// is silent (bar the EQ's dying ring, far under audibility),
		// and the station still lands.
		let out = render_frames(&mut mixer, 200);
		assert!(
			out[100..].iter().all(|s| s.abs() < 0.001),
			"no hiss when the FM sound is off"
		);
		mixer.handle(MixerCmd::SetActive {
			ring: Some(const_ring(-0.5, 2000)),
			token: 2,
		});
		render_frames(&mut mixer, 30);
		assert_eq!(render_frames(&mut mixer, 4), [-0.5; 8]);
		// Back on, the hiss returns.
		mixer.handle(MixerCmd::Hiss(true));
		mixer.handle(MixerCmd::Tune);
		let out = render_frames(&mut mixer, 200);
		assert!(out[100..].iter().any(|s| s.abs() > 0.001), "hiss back on");
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
	fn buffer_settings_change_the_start_and_the_refill_limit() {
		let mut mixer = MixerCore::new(RATE);
		mixer.handle(MixerCmd::Buffers {
			start_ms: 200,
			refill_limit_ms: 400,
		});
		let ring = Arc::new(PcmRing::new(RATE, 20.0));
		mixer.handle(MixerCmd::SetActive {
			ring: Some(ring.clone()),
			token: 9,
		});
		let mut out = [0.0; 40];
		// 100 ms is not 200.
		ring.push(&vec![1.0; 100 * 2]);
		assert_eq!(mixer.render(&mut out), None, "below the start buffer");
		assert_eq!(ring.len_ms(), 100, "nothing taken while buffering");
		ring.push(&vec![1.0; 100 * 2]);
		assert_eq!(mixer.render(&mut out), Some(9), "at the start buffer");
		// Drained dry, a refill would want 600 ms; the limit holds it to 400.
		let mut drain = [0.0; 2000];
		mixer.render(&mut drain);
		assert_eq!(ring.len_ms(), 0);
		ring.push(&vec![1.0; 390 * 2]);
		mixer.render(&mut out);
		assert_eq!(ring.len_ms(), 390, "below the limit");
		ring.push(&[1.0; 10 * 2]);
		mixer.render(&mut out);
		assert!(ring.len_ms() < 400, "at the limit, it sounds");
	}

	#[test]
	fn rebuffer_demands_a_deeper_refill() {
		let mut mixer = MixerCore::new(RATE);
		let ring = Arc::new(PcmRing::new(RATE, 20.0));
		mixer.handle(MixerCmd::SetActive {
			ring: Some(ring.clone()),
			token: 9,
		});
		let mut out = vec![0.0; 40];
		// Cold start still needs only the base cushion.
		ring.push(&vec![1.0; 1000 * 2]);
		assert_eq!(mixer.render(&mut out), Some(9));
		// Drain it dry.
		let mut big = vec![0.0; 4000 * 2];
		mixer.render(&mut big);
		assert_eq!(ring.len_ms(), 0);
		// One base cushion is no longer enough to sound again.
		ring.push(&vec![1.0; 1000 * 2]);
		assert_eq!(mixer.render(&mut out), None);
		assert_eq!(ring.len_ms(), 1000, "nothing taken while rebuffering");
		ring.push(&vec![1.0; 2000 * 2]);
		mixer.render(&mut out);
		assert!(
			ring.len_ms() < 3000,
			"resumes once three seconds are buffered"
		);
	}

	#[test]
	fn rebuffer_refill_caps_at_fifteen_seconds() {
		let mut mixer = MixerCore::new(RATE);
		let ring = Arc::new(PcmRing::new(RATE, 20.0));
		mixer.handle(MixerCmd::SetActive {
			ring: Some(ring.clone()),
			token: 9,
		});
		let mut big = vec![0.0; 20000 * 2];
		// Three dry-outs in a row; each render both resumes and runs dry.
		for refill in [1000usize, 3000, 9000] {
			ring.push(&vec![1.0; refill * 2]);
			mixer.render(&mut big);
			assert_eq!(ring.len_ms(), 0);
		}
		let mut out = vec![0.0; 40];
		ring.push(&vec![1.0; 14000 * 2]);
		mixer.render(&mut out);
		assert_eq!(
			ring.len_ms(),
			14000,
			"fourteen seconds is not enough after three dry-outs"
		);
		ring.push(&vec![1.0; 1000 * 2]);
		mixer.render(&mut out);
		assert!(
			ring.len_ms() < 15000,
			"fifteen seconds resumes, so the cap held"
		);
	}

	#[test]
	fn clean_minute_earns_the_base_cushion_back() {
		let mut mixer = MixerCore::new(RATE);
		let ring = Arc::new(PcmRing::new(RATE, 20.0));
		mixer.handle(MixerCmd::SetActive {
			ring: Some(ring.clone()),
			token: 9,
		});
		let mut big = vec![0.0; 4000 * 2];
		ring.push(&vec![1.0; 1000 * 2]);
		mixer.render(&mut big);
		// Resume at the grown cushion, then play a clean minute in chunks.
		ring.push(&vec![1.0; 3000 * 2]);
		let mut chunk = vec![0.0; 1000 * 2];
		mixer.render(&mut chunk);
		for _ in 0..65 {
			ring.push(&vec![1.0; 1000 * 2]);
			mixer.render(&mut chunk);
		}
		// The next dry-out starts over at one strike, not two.
		mixer.render(&mut big);
		assert_eq!(ring.len_ms(), 0);
		let mut out = vec![0.0; 40];
		ring.push(&vec![1.0; 1000 * 2]);
		mixer.render(&mut out);
		assert_eq!(ring.len_ms(), 1000, "one second is not enough");
		ring.push(&vec![1.0; 2000 * 2]);
		mixer.render(&mut out);
		assert!(
			ring.len_ms() < 3000,
			"three seconds resumes: the count reset"
		);
	}

	#[test]
	fn new_station_starts_at_the_base_cushion() {
		let mut mixer = MixerCore::new(RATE);
		let ring = Arc::new(PcmRing::new(RATE, 20.0));
		mixer.handle(MixerCmd::SetActive {
			ring: Some(ring.clone()),
			token: 9,
		});
		let mut big = vec![0.0; 4000 * 2];
		ring.push(&vec![1.0; 1000 * 2]);
		mixer.render(&mut big);
		assert_eq!(ring.len_ms(), 0, "drained dry: the refill demand grew");
		let other = Arc::new(PcmRing::new(RATE, 20.0));
		mixer.handle(MixerCmd::SetActive {
			ring: Some(other.clone()),
			token: 10,
		});
		other.push(&vec![1.0; 1000 * 2]);
		let mut out = vec![0.0; 40];
		mixer.render(&mut out);
		assert!(
			other.len_ms() < 1000,
			"the new station starts on the base cushion"
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

	#[test]
	fn rate_change_keeps_mixer_state() {
		let mut m = mixer();
		let ring = const_ring(1.0, 100);
		m.handle(MixerCmd::SetActive {
			ring: Some(ring.clone()),
			token: 1,
		});
		m.handle(MixerCmd::Volume(0.5));
		let eq = EqSettings {
			enabled: true,
			preset: crate::settings::EqPreset::Custom,
			bands: [6, 0, 0, 0, 0, 0, 0, 0, 0, 0],
			..EqSettings::default()
		};
		m.handle(MixerCmd::Eq(eq.clone()));
		m.set_rate(2 * RATE);
		assert_eq!(m.fade_frames, 50, "25 ms at 2 kHz");
		assert_eq!(m.eq_settings, eq);
		assert!(m.active.as_ref().is_some_and(|a| Arc::ptr_eq(a, &ring)));
		assert_eq!(m.volume, 0.5);
	}

	#[test]
	fn hiss_ignores_the_volume_until_muted() {
		let peak_at = |volume: f32, muted: bool| {
			let mut m = MixerCore::new(1000);
			m.handle(MixerCmd::Volume(volume));
			m.handle(MixerCmd::Mute(muted));
			m.handle(MixerCmd::Tune);
			let out = render_frames(&mut m, 500);
			out.iter().fold(0.0f32, |p, s| p.max(s.abs()))
		};
		let loud = peak_at(1.0, false);
		assert!(loud > 0.0, "hiss sounds");
		assert_eq!(peak_at(0.1, false), loud, "same level whatever the next station's volume");
		assert_eq!(peak_at(0.0, true), 0.0, "mute silences it");
	}

	#[test]
	fn hiss_stays_soft_under_loudness() {
		let rate = 48_000;
		let mut m = MixerCore::new(rate);
		m.handle(MixerCmd::Eq(EqSettings {
			enabled: true,
			preset: crate::settings::EqPreset::Loudness,
			bands: crate::settings::EqPreset::Loudness.bands().unwrap(),
			..EqSettings::default()
		}));
		m.handle(MixerCmd::Tune);
		let mut out = vec![0.0; 9600];
		let mut peak = 0.0f32;
		// The leveler would have 3 s to ride it up.
		for _ in 0..30 {
			m.render(&mut out);
			peak = peak.max(out.iter().fold(0.0, |p, s| p.max(s.abs())));
		}
		assert!(peak > 0.0, "hiss sounds");
		assert!(peak <= HISS_LEVEL, "hiss not amplified: {peak}");
	}

	#[test]
	fn eq_never_clips() {
		let rate = 48_000;
		let ring = Arc::new(PcmRing::new(rate, 3.0));
		let sine: Vec<f32> = (0..rate as usize)
			.flat_map(|i| {
				let v = (2.0 * std::f32::consts::PI * 100.0 * i as f32 / rate as f32).sin();
				[v, v]
			})
			.collect();
		ring.push(&sine);
		let mut m = MixerCore::new(rate);
		m.cushion_ms = 0;
		m.handle(MixerCmd::Eq(EqSettings {
			enabled: true,
			preset: crate::settings::EqPreset::Custom,
			bands: [12; 10],
			..EqSettings::default()
		}));
		m.handle(MixerCmd::SetActive {
			ring: Some(ring),
			token: 1,
		});
		let mut out = vec![0.0; 9600];
		let mut peak = 0.0f32;
		for _ in 0..8 {
			m.render(&mut out);
			peak = peak.max(out.iter().fold(0.0, |p, s| p.max(s.abs())));
		}
		assert!(peak > 0.9, "boosted signal reaches the limit: {peak}");
		assert!(peak <= 1.0);
	}

	fn shift_setup() -> (MixerCore, Arc<ShiftBuffer>, Arc<ShiftStatus>) {
		let buffer = Arc::new(ShiftBuffer::new(1000, 1));
		buffer.push(&vec![0.5; 20_000]); // 10 s of constant tone
		let status = Arc::new(ShiftStatus::new());
		let mut m = MixerCore::new(1000);
		m.handle(MixerCmd::SetActive { ring: None, token: 1 });
		m.handle(MixerCmd::ShiftAttach {
			buffer: buffer.clone(),
			status: status.clone(),
		});
		(m, buffer, status)
	}

	#[test]
	fn seek_back_plays_the_buffer() {
		let (mut m, buffer, status) = shift_setup();
		m.handle(MixerCmd::ShiftSeek(-5));
		let out = render_frames(&mut m, 100);
		assert!(
			(out[2 * 50] - 0.5).abs() < 0.01,
			"shifted audio sounds: {}",
			out[2 * 50]
		);
		let behind = status.behind_seconds(&buffer);
		assert!((4.8..5.0).contains(&behind), "behind: {behind}");
	}

	#[test]
	fn pause_freezes_and_resume_continues() {
		let (mut m, buffer, status) = shift_setup();
		m.handle(MixerCmd::ShiftPause(true));
		let out = render_frames(&mut m, 100);
		assert!(out.iter().all(|s| *s == 0.0), "pause is silent");
		buffer.push(&vec![0.5; 4_000]); // 2 more seconds arrive while paused
		assert_eq!(status.behind_seconds(&buffer), 2.0);
		m.handle(MixerCmd::ShiftPause(false));
		let out = render_frames(&mut m, 100);
		assert!((out[2 * 50] - 0.5).abs() < 0.01, "resume plays from the freeze point");
	}

	#[test]
	fn seek_to_lands_on_the_frame_and_clamps_to_the_buffer() {
		let (mut m, buffer, status) = shift_setup();
		m.handle(MixerCmd::ShiftSeekTo(4_000));
		assert_eq!(status.cursor.load(Ordering::Relaxed), 4_000);
		render_frames(&mut m, 100);
		assert_eq!(status.cursor.load(Ordering::Relaxed), 4_100, "plays on from there");
		m.handle(MixerCmd::ShiftPause(true));
		m.handle(MixerCmd::ShiftSeekTo(2_000));
		assert_eq!(status.cursor.load(Ordering::Relaxed), 2_000, "paused jumps count");
		m.handle(MixerCmd::ShiftPause(false));
		buffer.push(&vec![0.5; 2 * 60_000]); // trims the first 10 s
		m.handle(MixerCmd::ShiftSeekTo(0));
		assert_eq!(status.cursor.load(Ordering::Relaxed), buffer.start());
		m.handle(MixerCmd::ShiftSeekTo(u64::MAX - 1));
		assert_eq!(status.cursor.load(Ordering::Relaxed), u64::MAX, "past the edge is live");
	}

	#[test]
	fn seek_past_live_returns_to_live() {
		let (mut m, _, status) = shift_setup();
		m.handle(MixerCmd::ShiftSeek(-5));
		m.handle(MixerCmd::ShiftSeek(30));
		assert_eq!(status.cursor.load(Ordering::Relaxed), u64::MAX, "back at live");
		m.handle(MixerCmd::ShiftLive);
		let out = render_frames(&mut m, 10);
		assert!(out.iter().all(|s| *s == 0.0), "no ring, no sound");
	}

	/// A server bursts 10 s on connect, and 2 s of it have played.
	fn burst_setup() -> (MixerCore, Arc<ShiftBuffer>, Arc<ShiftStatus>, Arc<PcmRing>) {
		let buffer = Arc::new(ShiftBuffer::new(1000, 1));
		let status = Arc::new(ShiftStatus::new());
		let ring = Arc::new(PcmRing::new(1000, 60.0));
		let mut m = MixerCore::new(1000);
		m.handle(MixerCmd::SetActive { ring: Some(ring.clone()), token: 1 });
		m.handle(MixerCmd::ShiftAttach {
			buffer: buffer.clone(),
			status: status.clone(),
		});
		ring.push(&vec![0.5; 20_000]);
		buffer.push(&vec![0.5; 20_000]);
		render_frames(&mut m, 2_000);
		render_frames(&mut m, 1); // the status catches up with the last pass
		(m, buffer, status, ring)
	}

	#[test]
	fn a_connect_burst_is_not_behind_live() {
		let (mut m, buffer, status, _ring) = burst_setup();
		assert_eq!(status.behind_seconds(&buffer), 0.0, "playing live");
		let depth = status.depth_seconds(&buffer);
		assert!((1.9..2.1).contains(&depth), "only what has played: {depth}");
		// The jump to the start lands 2 s back, not 10.
		m.handle(MixerCmd::ShiftSeek(i32::MIN));
		render_frames(&mut m, 1);
		let behind = status.behind_seconds(&buffer);
		assert!((1.9..2.1).contains(&behind), "behind: {behind}");
	}

	#[test]
	fn a_step_back_never_lands_ahead_of_playback() {
		let (mut m, _buffer, status, _ring) = burst_setup();
		m.handle(MixerCmd::ShiftSeek(-1));
		let at = status.cursor.load(Ordering::Relaxed);
		render_frames(&mut m, 1);
		let at = if at == u64::MAX { status.cursor.load(Ordering::Relaxed) } else { at };
		assert!((900..=1_100).contains(&at), "a second before frame 2000: {at}");
	}

	#[test]
	fn forward_to_live_goes_on_from_the_ring_without_running_dry() {
		let (mut m, buffer, status, ring) = burst_setup();
		m.handle(MixerCmd::ShiftSeek(-1));
		render_frames(&mut m, 100);
		let held = ring.frames();
		assert!(held > 7_000, "the ring plays on underneath: {held}");
		m.handle(MixerCmd::ShiftSeek(5));
		assert_eq!(status.cursor.load(Ordering::Relaxed), u64::MAX, "live");
		assert_eq!(status.behind_seconds(&buffer), 0.0);
		render_frames(&mut m, 100);
		assert_eq!(m.strikes, 0, "no dry-out");
	}

	#[test]
	fn pausing_at_live_resumes_where_it_stopped() {
		let (mut m, buffer, status, ring) = burst_setup();
		m.handle(MixerCmd::ShiftPause(true));
		render_frames(&mut m, 500);
		render_frames(&mut m, 1); // the status catches up with the last pass
		let at = status.cursor.load(Ordering::Relaxed);
		assert!((1_900..=2_100).contains(&at), "frozen at playback, not the newest audio: {at}");
		let behind = status.behind_seconds(&buffer);
		assert!((0.4..0.7).contains(&behind), "the pause counts: {behind}");
		assert!(ring.frames() > 7_000);
	}
}

#[cfg(test)]
mod dynamic_seek {
	use super::*;

	#[test]
	fn forward_seeks_reach_live_while_buffer_fills() {
		// rate 1000 => 1 frame = 1 ms; buffer holds 120 s.
		let buffer = Arc::new(ShiftBuffer::new(1000, 2));
		let status = Arc::new(ShiftStatus::new());
		let mut m = MixerCore::new(1000);
		m.handle(MixerCmd::SetActive { ring: None, token: 1 });
		m.handle(MixerCmd::ShiftAttach { buffer: buffer.clone(), status: status.clone() });
		let tone = vec![0.5f32; 200]; // 100 frames = 0.1 s per push
		// Fill 60 s while rendering live.
		for _ in 0..600 {
			buffer.push(&tone);
			let mut out = vec![0.0f32; 200];
			m.render(&mut out);
		}
		// Seek back 30 s.
		m.handle(MixerCmd::ShiftSeek(-30));
		{
			let mut out = vec![0.0f32; 200];
			m.render(&mut out);
		}
		let b0 = status.behind_seconds(&buffer);
		println!("behind after -30: {b0}");
		assert!((29.0..31.0).contains(&b0), "behind after seek back: {b0}");
		// Now: keep producing + rendering 0.1 s per step, seek +15 twice.
		for press in 1..=2 {
			m.handle(MixerCmd::ShiftSeek(15));
			for _ in 0..5 {
				buffer.push(&tone);
				let mut out = vec![0.0f32; 200];
				m.render(&mut out);
			}
			let b = status.behind_seconds(&buffer);
			println!("behind after press {press}: {b}");
		}
		let b = status.behind_seconds(&buffer);
		assert!(b < 2.0, "should be at/near live after two +15 seeks from 30 behind, got {b}");
	}
}
