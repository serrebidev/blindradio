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
use crate::ring::{CUSHION_MS, PcmRing};
use crate::settings::EqSettings;
use crate::timeshift::{ShiftBuffer, ShiftStatus};

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
	Eq(EqSettings),
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
	/// Returns to the live edge.
	ShiftLive,
	/// The active station's recorder slot, in encoded-ring mode: while
	/// the listener is time-shifted the mixer taps what they hear into
	/// it, and the stream-side taps yield.
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

/// The most a stream that keeps running dry must refill before it may sound
/// again. A server delivering just under its bitrate (TopRadio sends ~2/3 of
/// its 192 kbps) stutters every few seconds against a 1 s cushion; a deep
/// refill turns that into long plays between clean gaps. Healthy streams
/// never run dry, so they never see this.
const REBUFFER_MAX_MS: u32 = 15_000;
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
	/// Recorder slot for the ring mode's as-heard tap; see RecordSlot.
	record_slot: Option<Arc<Mutex<Option<Recorder>>>>,
}

impl MixerCore {
	pub fn new(rate: u32) -> MixerCore {
		let mut m = MixerCore {
			fade_frames: 0,
			tail_frames: 0,
			cushion_ms: CUSHION_MS,
			base_cushion_ms: CUSHION_MS,
			strikes: 0,
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
				status.cursor.store(u64::MAX, Ordering::Relaxed);
				status.paused.store(false, Ordering::Relaxed);
				self.shift = Some((buffer, status));
				self.shift_cursor = None;
				self.shift_paused = false;
			}
			MixerCmd::ShiftPause(paused) => {
				if let Some((buffer, status)) = &self.shift {
					if paused && self.shift_cursor.is_none() {
						// Freeze at the live edge; the buffer keeps filling past it.
						self.shift_cursor = Some(buffer.live_edge());
					}
					self.shift_paused = paused;
					status.paused.store(paused, Ordering::Relaxed);
					if !paused && self.shift_cursor == Some(buffer.live_edge()) {
						self.shift_cursor = None;
						status.cursor.store(u64::MAX, Ordering::Relaxed);
					}
				}
			}
			MixerCmd::ShiftSeek(seconds) => {
				if let Some((buffer, status)) = &self.shift {
					let edge = buffer.live_edge();
					let base = self.shift_cursor.unwrap_or(edge) as i64;
					let target = (base + seconds as i64 * buffer.rate() as i64)
						.clamp(buffer.start() as i64, edge as i64) as u64;
					self.shift_cursor = if target >= edge { None } else { Some(target) };
					if self.shift_cursor.is_none() {
						// Live from here on: say so in the status at once.
						// Render only writes the status while shifted, so a
						// cursor cleared here would otherwise leave the last
						// shifted frame standing — the status line and the
						// announcements would keep quoting a stale "behind
						// live" while the ring plays live.
						status.cursor.store(u64::MAX, Ordering::Relaxed);
					}
				}
			}
			MixerCmd::ShiftLive => {
				self.shift_cursor = None;
				self.shift_paused = false;
				if let Some((_, status)) = &self.shift {
					status.cursor.store(u64::MAX, Ordering::Relaxed);
					status.paused.store(false, Ordering::Relaxed);
				}
			}
			MixerCmd::RecordSlot(slot) => self.record_slot = slot,
			MixerCmd::Volume(v) => self.volume = v.clamp(0.0, 1.0),
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
				let (buffer, _) = self.shift.as_ref().unwrap();
				let n = buffer.read_at(cursor, &mut self.a);
				got = n / 2;
				let next = cursor + got as u64;
				// Reaching the live edge hands playback back to the ring.
				self.shift_cursor = if next >= buffer.live_edge() { None } else { Some(next) };
			}
			if let Some((_, status)) = &self.shift {
				status
					.cursor
					.store(self.shift_cursor.unwrap_or(u64::MAX), Ordering::Relaxed);
			}
		}
		if !shifted && let Some(ring) = &self.active {
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
					// This stream has proven it cannot sustain the base
					// cushion: demand a deeper refill before it sounds again.
					self.strikes += 1;
					self.cushion_ms = self
						.base_cushion_ms
						.saturating_mul(REBUFFER_GROWTH.saturating_pow(self.strikes))
						.min(REBUFFER_MAX_MS);
				}
			}
		}
		// Ring mode records what the listener hears: while shifted, the
		// shifted audio (silence while paused, so the file's timeline
		// matches the listening) goes to the recorder; an Original
		// recorder ignores PCM, and the stream-side taps yield.
		if shifted
			&& let Some(slot) = &self.record_slot
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

		// Hiss covers every frame the active station does not, easing in and out over a crossfade.
		for f in 0..frames {
			let want = if self.hiss && !self.shift_paused && f >= got { 1.0 } else { 0.0 };
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

		self.eq.process(&mut self.a);
		let vol = if self.muted { 0.0 } else { self.volume };
		for (o, &s) in out.iter_mut().zip(&self.a) {
			*o = (s * vol).clamp(-1.0, 1.0);
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
			bass: 6,
			mid: 0,
			treble: 0,
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
			bass: 6,
			mid: 0,
			treble: 0,
		};
		m.handle(MixerCmd::Eq(eq.clone()));
		m.set_rate(2 * RATE);
		assert_eq!(m.fade_frames, 50, "25 ms at 2 kHz");
		assert_eq!(m.eq_settings, eq);
		assert!(m.active.as_ref().is_some_and(|a| Arc::ptr_eq(a, &ring)));
		assert_eq!(m.volume, 0.5);
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
			bass: 12,
			mid: 12,
			treble: 12,
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
	fn seek_past_live_returns_to_live() {
		let (mut m, _, status) = shift_setup();
		m.handle(MixerCmd::ShiftSeek(-5));
		m.handle(MixerCmd::ShiftSeek(30));
		assert_eq!(status.cursor.load(Ordering::Relaxed), u64::MAX, "back at live");
		m.handle(MixerCmd::ShiftLive);
		let out = render_frames(&mut m, 10);
		assert!(out.iter().all(|s| *s == 0.0), "no ring, no sound");
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
