use std::collections::HashMap;
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender};
use parking_lot::Mutex;

use std::path::PathBuf;

use crate::net;
use crate::output::{MixerCmd, Output};
use crate::recognize::{SongId, identify_audd, wav_bytes};
use crate::record::Recorder;
use crate::ring::{CUSHION_MS, PcmRing};
use crate::scrobble;
use crate::settings::{RecognitionProvider, Settings};
use crate::station::Station;
use crate::stream::{EngineCtx, StreamEvent, Taps};
use crate::timeshift::{ShiftBuffer, ShiftStatus};
use crate::{CoreError, Result};

/// Focus calls closer than this are key repeat: new connections wait until the key rests.
const REPEAT: Duration = Duration::from_millis(100);
/// Focus rest before a held title is announced.
const SETTLE: Duration = Duration::from_millis(1500);
/// Warm neighbours close after this long without a focus.
const WARM_IDLE: Duration = Duration::from_secs(25);
/// A warm swap plays this far behind the ring's newest sample: the mixer's start cushion, so a
/// ready neighbour swaps at once and one still filling waits like a cold start.
const SWAP_MS: u32 = CUSHION_MS;
/// DNS is warmed for the stations this close to the focused one.
const DNS_SPAN: usize = 5;
/// A host is warmed again after this, since the OS resolver cache expires.
const DNS_REWARM: Duration = Duration::from_secs(300);

/// The station to play and its warm neighbours (index-1 and index+1 when they exist).
pub fn wanted(len: usize, index: usize) -> (usize, Vec<usize>) {
	let mut warm = Vec::new();
	if index > 0 {
		warm.push(index - 1);
	}
	if index + 1 < len {
		warm.push(index + 1);
	}
	(index, warm)
}

/// Title announcement rule: hold titles while focus moves, release once it rested.
pub struct Settle {
	focused_at: Option<Instant>,
	held: Option<String>,
}

impl Settle {
	pub fn new() -> Settle {
		Settle {
			focused_at: None,
			held: None,
		}
	}

	pub fn focused(&mut self, now: Instant) {
		self.focused_at = Some(now);
		self.held = None;
	}

	fn rested(&mut self, now: Instant) -> bool {
		match self.focused_at {
			Some(at) if now.duration_since(at) < SETTLE => false,
			_ => {
				self.focused_at = None;
				true
			}
		}
	}

	pub fn title(&mut self, title: Option<String>, now: Instant) -> Option<String> {
		if self.rested(now) {
			return title;
		}
		self.held = title;
		None
	}

	pub fn tick(&mut self, now: Instant) -> Option<String> {
		if self.focused_at.is_some() && self.rested(now) {
			return self.held.take();
		}
		None
	}
}

impl Default for Settle {
	fn default() -> Settle {
		Settle::new()
	}
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TunerEvent {
	Playing {
		station_id: String,
	},
	Title {
		station_id: String,
		title: Option<String>,
	},
	/// The playing stream's format became known (codec, bitrate, rate).
	Format {
		station_id: String,
		format: crate::decode::StreamFormat,
	},
	Announce(String),
	Reconnecting {
		station_id: String,
	},
	Reconnected {
		station_id: String,
	},
	Failed {
		station_id: String,
		name: String,
		reason: String,
	},
	Latency {
		station_id: String,
		ms: u32,
		warm: bool,
	},
}

pub trait StreamFactory {
	/// The returned `Box<dyn Send>` is the stream's drop guard.
	fn start(
		&self,
		station: &Station,
		on_event: Box<dyn Fn(StreamEvent) + Send + Sync>,
	) -> (Arc<PcmRing>, Box<dyn Send>);

	fn warm_dns(&self, _stations: &[Station]) {}

	/// The output reopened at a new rate: streams started from now decode to it.
	fn set_rate(&mut self, _rate: u32) {}

	/// Registers the audio taps for a station (time-shift, capture, recording).
	fn set_taps(&mut self, _station_id: &str, _taps: Taps) {}

	fn clear_taps(&mut self, _station_id: &str) {}

	/// The rate streams decode to.
	fn rate(&self) -> u32 {
		48_000
	}

	/// A runtime for background work (recognition, scrobbling).
	fn rt(&self) -> Option<tokio::runtime::Handle> {
		None
	}
}

/// When each hostname was last warmed; warming is only an OS resolver hint.
static WARMED_HOSTS: LazyLock<Mutex<HashMap<String, Instant>>> =
	LazyLock::new(|| Mutex::new(HashMap::new()));

pub struct LiveFactory(pub EngineCtx);

impl StreamFactory for LiveFactory {
	fn start(
		&self,
		station: &Station,
		on_event: Box<dyn Fn(StreamEvent) + Send + Sync>,
	) -> (Arc<PcmRing>, Box<dyn Send>) {
		let stream = crate::stream::start(station, &self.0, on_event);
		(stream.ring.clone(), Box::new(stream))
	}

	fn warm_dns(&self, stations: &[Station]) {
		let mut hosts = Vec::new();
		{
			let now = Instant::now();
			let mut warmed = WARMED_HOSTS.lock();
			for station in stations {
				if let Ok(url) = reqwest::Url::parse(&station.url)
					&& let Some(host) = url.host_str()
					&& warmed
						.get(host)
						.is_none_or(|at| now.duration_since(*at) >= DNS_REWARM)
				{
					warmed.insert(host.to_owned(), now);
					hosts.push(host.to_owned());
				}
			}
		}
		if !hosts.is_empty() {
			drop(self.0.rt.spawn(net::warm_dns(hosts)));
		}
	}

	fn set_rate(&mut self, rate: u32) {
		self.0.out_rate = rate;
	}

	fn set_taps(&mut self, station_id: &str, taps: Taps) {
		self.0.taps.lock().insert(station_id.to_string(), taps);
	}

	fn clear_taps(&mut self, station_id: &str) {
		self.0.taps.lock().remove(station_id);
	}

	fn rate(&self) -> u32 {
		self.0.out_rate
	}

	fn rt(&self) -> Option<tokio::runtime::Handle> {
		Some(self.0.rt.clone())
	}
}

pub trait Sink {
	fn send(&self, cmd: MixerCmd);
}

impl Sink for Output {
	fn send(&self, cmd: MixerCmd) {
		Output::send(self, cmd)
	}
}

struct Open {
	station: Station,
	/// Tags this stream's events, so late events from a dropped stream are ignored.
	serial: u64,
	ring: Arc<PcmRing>,
	title: Option<String>,
	format: Option<crate::decode::StreamFormat>,
	/// Said `Reconnecting` and has not said `Reconnected` since.
	reconnecting: bool,
	_guard: Box<dyn Send>,
}

/// Connections held back while keys repeat.
struct Deferred {
	/// Focus instant of the playing station when it still needs a cold start.
	cold: Option<Instant>,
	warm: Vec<Station>,
}

/// The last `SetActive` waiting for its first rendered audio.
struct Pending {
	token: u64,
	station_id: String,
	since: Instant,
	warm: bool,
}

/// How long a song must play before it is scrobbled.
const SCROBBLE_MIN: Duration = Duration::from_secs(60);
/// Recognition attempts on one station are at most this frequent.
const RECOG_GAP: Duration = Duration::from_secs(45);
/// Seconds of captured audio sent to the recognition service.
const RECOG_SAMPLE_SECONDS: f32 = 12.0;

pub struct Tuner<F: StreamFactory, S: Sink> {
	factory: F,
	sink: S,
	events: Sender<TunerEvent>,
	stream_tx: Sender<(u64, StreamEvent)>,
	stream_rx: Receiver<(u64, StreamEvent)>,
	open: Vec<Open>,
	playing: Option<Station>,
	deferred: Option<Deferred>,
	pending: Option<Pending>,
	last_focus: Option<Instant>,
	settle: Settle,
	/// Source of stream serials and mixer tokens; 0 is never issued.
	next_id: u64,
	settings: Option<Arc<Mutex<Settings>>>,
	/// Station the taps below belong to; they reset when it changes.
	tap_station: Option<String>,
	shift: Option<(Arc<ShiftBuffer>, Arc<ShiftStatus>)>,
	capture: Option<Arc<ShiftBuffer>>,
	record_slot: Option<Arc<Mutex<Option<Recorder>>>>,
	/// The playing station's current song title and when it started.
	song: Option<(String, Instant)>,
	last_recog: Option<Instant>,
	recog_in_flight: bool,
	recog_tx: Sender<(String, Option<SongId>)>,
	recog_rx: Receiver<(String, Option<SongId>)>,
}

impl<F: StreamFactory, S: Sink> Tuner<F, S> {
	pub fn new(factory: F, sink: S, events: Sender<TunerEvent>) -> Tuner<F, S> {
		let (stream_tx, stream_rx) = crossbeam_channel::unbounded();
		let (recog_tx, recog_rx) = crossbeam_channel::unbounded();
		Tuner {
			factory,
			sink,
			events,
			stream_tx,
			stream_rx,
			open: Vec::new(),
			playing: None,
			deferred: None,
			pending: None,
			last_focus: None,
			settle: Settle::new(),
			next_id: 0,
			settings: None,
			tap_station: None,
			shift: None,
			capture: None,
			record_slot: None,
			song: None,
			last_recog: None,
			recog_in_flight: false,
			recog_tx,
			recog_rx,
		}
	}

	/// Gives the tuner the live settings, enabling time-shift buffers,
	/// recording, recognition and scrobbling for the playing station.
	pub fn set_settings(&mut self, settings: Arc<Mutex<Settings>>) {
		self.settings = Some(settings);
	}

	pub fn focus(&mut self, stations: &[Station], index: usize, now: Instant) {
		let Some(target) = stations.get(index) else {
			return;
		};
		let repeat = self
			.last_focus
			.is_some_and(|t| now.duration_since(t) < REPEAT);
		self.last_focus = Some(now);
		// Under key repeat even lookups wait: each one is a blocking thread.
		if !repeat {
			let lo = index.saturating_sub(DNS_SPAN);
			let hi = (index + DNS_SPAN + 1).min(stations.len());
			self.factory.warm_dns(&stations[lo..hi]);
		}
		let warm = wanted(stations.len(), index)
			.1
			.into_iter()
			.map(|i| stations[i].clone())
			.collect();
		self.tune(target, warm, repeat, now);
	}

	pub fn play(&mut self, station: &Station, now: Instant) {
		// A focus right after this is key repeat relative to the play, not to an older focus.
		self.last_focus = Some(now);
		self.tune(station, Vec::new(), false, now);
	}

	pub fn stop(&mut self) {
		self.detach_taps();
		self.silence();
		self.open.clear();
		self.playing = None;
		self.deferred = None;
		self.pending = None;
	}

	pub fn poll(&mut self, now: Instant, rendered: &[(u64, Instant)]) {
		while let Ok((serial, event)) = self.stream_rx.try_recv() {
			self.stream_event(serial, event, now);
		}

		while let Ok((station_id, song)) = self.recog_rx.try_recv() {
			self.recog_in_flight = false;
			if let Some(song) = song
				&& self.playing.as_ref().is_some_and(|p| p.id == station_id)
			{
				let display = song.display();
				if self.song.as_ref().is_none_or(|(t, _)| *t != display) {
					if let Some(open) = self.open.iter_mut().find(|o| o.station.id == station_id) {
						open.title = Some(display.clone());
					}
					self.emit(TunerEvent::Title {
						station_id: station_id.clone(),
						title: Some(display.clone()),
					});
					self.song = Some((display.clone(), now));
					let announce = self.settings.as_ref().is_some_and(|s| s.lock().recognition.announce);
					if announce
						&& let Some(title) = self.settle.title(Some(display), now)
					{
						self.emit(TunerEvent::Announce(title));
					}
				}
			}
		}
		self.maybe_recognize(now);

		for &(token, at) in rendered {
			if let Some(p) = self.pending.take_if(|p| p.token == token) {
				let ms = u32::try_from(at.saturating_duration_since(p.since).as_millis())
					.unwrap_or(u32::MAX);
				self.emit(TunerEvent::Latency {
					station_id: p.station_id.clone(),
					ms,
					warm: p.warm,
				});
				self.emit(TunerEvent::Playing {
					station_id: p.station_id,
				});
			}
		}

		if self
			.last_focus
			.is_some_and(|t| now.duration_since(t) >= REPEAT)
			&& let Some(d) = self.deferred.take()
		{
			self.connect(d.cold, &d.warm);
		}

		if self
			.last_focus
			.is_some_and(|t| now.duration_since(t) >= WARM_IDLE)
		{
			self.close_warm();
		}

		if let Some(title) = self.settle.tick(now) {
			self.emit(TunerEvent::Announce(title));
		}
	}

	/// The station list closed: its warm neighbours close, the playing station plays on.
	pub fn list_closed(&mut self) {
		self.close_warm();
		if let Some(d) = &mut self.deferred {
			d.warm.clear();
		}
	}

	pub fn playing(&self) -> Option<&Station> {
		self.playing.as_ref()
	}

	/// The playing stream's format, once its decoder has reported it.
	pub fn format(&self) -> Option<&crate::decode::StreamFormat> {
		let playing = self.playing.as_ref()?;
		self.open
			.iter()
			.find(|o| o.station.id == playing.id)
			.and_then(|o| o.format.as_ref())
	}

	/// The output reopened at a new rate: everything open closes, so the restart after it
	/// decodes to that rate.
	pub fn set_rate(&mut self, rate: u32) {
		self.factory.set_rate(rate);
		self.stop();
	}

	fn tune(&mut self, target: &Station, warm: Vec<Station>, repeat: bool, now: Instant) {
		if self.playing.as_ref().is_some_and(|p| p.id != target.id) {
			// The old station's recording ends and its taps come off.
			self.detach_taps();
		}
		self.settle.focused(now);
		self.deferred = None;
		let mut cold = None;
		let already =
			self.playing.as_ref().is_some_and(|p| p.id == target.id) && self.is_open(&target.id);
		if !already {
			match self.open.iter().find(|o| o.station.id == target.id) {
				Some(o) => {
					let (ring, title, reconnecting) =
						(o.ring.clone(), o.title.clone(), o.reconnecting);
					ring.trim_to_ms(SWAP_MS);
					self.activate(target, ring, now, true);
					self.emit(TunerEvent::Title {
						station_id: target.id.clone(),
						title: title.clone(),
					});
					if reconnecting {
						self.emit(TunerEvent::Reconnecting {
							station_id: target.id.clone(),
						});
					}
					// Just focused, so this only holds the title for Settle.
					self.settle.title(title, now);
				}
				None => {
					// Silence first: the old station must never keep playing under a new focus.
					// The old station fades into hiss until this one comes in: no dead air, never the old station.
					self.sink.send(MixerCmd::Tune);
					self.pending = None;
					cold = Some(now);
				}
			}
		}
		self.playing = Some(target.clone());
		self.open
			.retain(|o| o.station.id == target.id || warm.iter().any(|w| w.id == o.station.id));
		if repeat {
			self.deferred = Some(Deferred { cold, warm });
		} else {
			self.connect(cold, &warm);
		}
	}

	/// Opens the playing station when it needs a cold start, then the missing neighbours.
	fn connect(&mut self, cold: Option<Instant>, warm: &[Station]) {
		if let Some(since) = cold
			&& let Some(target) = self.playing.clone()
			&& !self.is_open(&target.id)
		{
			let ring = self.start(&target);
			self.activate(&target, ring, since, false);
		}
		for station in warm {
			if !self.is_open(&station.id) {
				self.start(station);
			}
		}
	}

	fn start(&mut self, station: &Station) -> Arc<PcmRing> {
		self.next_id += 1;
		let serial = self.next_id;
		let tx = self.stream_tx.clone();
		let (ring, guard) = self.factory.start(
			station,
			Box::new(move |event| {
				let _ = tx.send((serial, event));
			}),
		);
		self.open.push(Open {
			station: station.clone(),
			serial,
			ring: ring.clone(),
			title: None,
			format: None,
			reconnecting: false,
			_guard: guard,
		});
		ring
	}

	fn activate(&mut self, station: &Station, ring: Arc<PcmRing>, since: Instant, warm: bool) {
		self.next_id += 1;
		let token = self.next_id;
		self.sink.send(MixerCmd::SetActive {
			ring: Some(ring),
			token,
		});
		self.pending = Some(Pending {
			token,
			station_id: station.id.clone(),
			since,
			warm,
		});
		self.attach_taps(station);
	}

	/// Builds the playing station's taps from the settings: a time-shift
	/// buffer when it has minutes, a capture buffer when recognition is on,
	/// and a fresh recorder slot. Re-activating the same station keeps them.
	fn attach_taps(&mut self, station: &Station) {
		if self.tap_station.as_deref() == Some(station.id.as_str()) {
			return;
		}
		self.detach_taps();
		let Some(settings) = &self.settings else {
			return;
		};
		let settings = settings.lock().clone();
		let rate = self.factory.rate();
		let mut taps = Taps::default();
		let minutes = settings.timeshift_minutes_for(&station.id);
		if minutes > 0 {
			let buffer = Arc::new(ShiftBuffer::new(rate, minutes));
			let status = Arc::new(ShiftStatus::new());
			self.sink.send(MixerCmd::ShiftAttach {
				buffer: buffer.clone(),
				status: status.clone(),
			});
			taps.shift = Some(buffer.clone());
			self.shift = Some((buffer, status));
		}
		if settings.recognition_enabled_for(&station.id) {
			let capture = Arc::new(ShiftBuffer::with_seconds(rate, 90));
			taps.capture = Some(capture.clone());
			self.capture = Some(capture);
		}
		let slot = Arc::new(Mutex::new(None));
		taps.record = Some(slot.clone());
		self.record_slot = Some(slot);
		self.tap_station = Some(station.id.clone());
		self.factory.set_taps(&station.id, taps);
	}

	/// Stops any recording and unregisters the taps of the station they
	/// belonged to.
	fn detach_taps(&mut self) {
		if let Some(slot) = &self.record_slot
			&& let Some(rec) = slot.lock().take()
		{
			let _ = rec.stop();
		}
		if let Some(id) = self.tap_station.take() {
			self.factory.clear_taps(&id);
		}
		self.shift = None;
		self.capture = None;
		self.record_slot = None;
		self.song = None;
	}

	/// The playing station's time-shift buffer and its live status.
	pub fn shift(&self) -> Option<(Arc<ShiftBuffer>, Arc<ShiftStatus>)> {
		self.shift.clone()
	}

	pub fn shift_pause(&mut self, paused: bool) {
		self.sink.send(MixerCmd::ShiftPause(paused));
	}

	pub fn shift_seek(&mut self, seconds: i32) {
		self.sink.send(MixerCmd::ShiftSeek(seconds));
	}

	pub fn shift_live(&mut self) {
		self.sink.send(MixerCmd::ShiftLive);
	}

	pub fn is_recording(&self) -> bool {
		self.record_slot
			.as_ref()
			.is_some_and(|slot| slot.lock().is_some())
	}

	/// Starts recording the playing station; returns the file path.
	pub fn start_recording(&mut self) -> Result<PathBuf> {
		let station = self
			.playing
			.clone()
			.ok_or_else(|| CoreError::Output("Nothing is playing".into()))?;
		let slot = self
			.record_slot
			.clone()
			.ok_or_else(|| CoreError::Output("Recording is unavailable".into()))?;
		let settings = self
			.settings
			.as_ref()
			.ok_or_else(|| CoreError::Output("Recording is unavailable".into()))?;
		let settings = settings.lock().clone();
		let dir = settings
			.recording
			.dir
			.as_deref()
			.map(PathBuf::from)
			.unwrap_or_else(crate::record::default_dir);
		let ext = station
			.url
			.rsplit('.')
			.next()
			.filter(|e| matches!(e.to_ascii_lowercase().as_str(), "mp3" | "aac" | "ogg" | "flac" | "m4a"))
			.unwrap_or("mp3");
		let rec = Recorder::start(&dir, &station.name, &settings.recording, self.factory.rate(), ext)?;
		let path = rec.path().to_path_buf();
		*slot.lock() = Some(rec);
		Ok(path)
	}

	/// Stops the recording, returning its file path.
	pub fn stop_recording(&mut self) -> Option<Result<PathBuf>> {
		let slot = self.record_slot.clone()?;
		slot.lock().take().map(|rec| rec.stop())
	}

	/// Starts a recognition attempt when the settings ask for one: the
	/// station is enabled, AudD is the provider, the capture buffer holds a
	/// full sample, and the stream is not already telling us the song (or is
	/// not asked to be believed).
	fn maybe_recognize(&mut self, now: Instant) {
		if self.recog_in_flight
			|| self
				.last_recog
				.is_some_and(|t| now.duration_since(t) < RECOG_GAP)
		{
			return;
		}
		let (Some(settings), Some(station), Some(capture)) =
			(&self.settings, &self.playing, &self.capture)
		else {
			return;
		};
		let settings = settings.lock().clone();
		if !settings.recognition_enabled_for(&station.id)
			|| settings.recognition.provider != RecognitionProvider::Audd
		{
			return;
		}
		let Some(token) = settings.recognition.audd_token.clone().filter(|t| !t.is_empty()) else {
			return;
		};
		if settings.recognition.only_when_no_metadata && self.song.is_some() {
			return;
		}
		if capture.buffered_seconds() < RECOG_SAMPLE_SECONDS {
			return;
		}
		let Some(rt) = self.factory.rt() else {
			return;
		};
		let Ok(wav) = wav_bytes(&capture.tail_f32(RECOG_SAMPLE_SECONDS), capture.rate()) else {
			return;
		};
		self.last_recog = Some(now);
		self.recog_in_flight = true;
		let tx = self.recog_tx.clone();
		let station_id = station.id.clone();
		rt.spawn(async move {
			let song = identify_audd(&token, wav).await.ok().flatten();
			let _ = tx.send((station_id, song));
		});
	}

	/// Scrobbles the song that just ended, when it played long enough and
	/// the station allows it.
	fn scrobble_previous(&mut self, previous: &(String, Instant), now: Instant) {
		if now.duration_since(previous.1) < SCROBBLE_MIN {
			return;
		}
		let (Some(settings), Some(station), Some(rt)) =
			(&self.settings, &self.playing, self.factory.rt())
		else {
			return;
		};
		let settings = settings.lock().clone();
		if !settings.scrobble_enabled_for(&station.id) {
			return;
		}
		let (artist, track) = match previous.0.split_once(" - ") {
			Some((a, t)) => (a.trim().to_string(), t.trim().to_string()),
			None => (String::new(), previous.0.clone()),
		};
		if track.is_empty() {
			return;
		}
		let started = std::time::SystemTime::now()
			.duration_since(std::time::UNIX_EPOCH)
			.map(|d| d.as_secs())
			.unwrap_or(0)
			.saturating_sub(now.duration_since(previous.1).as_secs());
		rt.spawn(async move {
			if let Some(token) = &settings.scrobble.listenbrainz_token
				&& !token.is_empty()
			{
				let _ = scrobble::listenbrainz(token, &artist, &track, None, started).await;
			}
			if let (Some(key), Some(secret), Some(session)) = (
				&settings.scrobble.lastfm_api_key,
				&settings.scrobble.lastfm_secret,
				&settings.scrobble.lastfm_session_key,
			) && !key.is_empty()
			{
				let _ = scrobble::lastfm(
					settings.scrobble.lastfm_base.as_deref(),
					key,
					secret,
					session,
					&artist,
					&track,
					started,
				)
				.await;
			}
		});
	}

	fn stream_event(&mut self, serial: u64, event: StreamEvent, now: Instant) {
		let Some(i) = self.open.iter().position(|o| o.serial == serial) else {
			return;
		};
		let station_id = self.open[i].station.id.clone();
		let playing = self.playing.as_ref().is_some_and(|p| p.id == station_id);
		match event {
			StreamEvent::Title(title) => {
				self.open[i].title = title.clone();
				if playing {
					if self.song.as_ref().map(|(t, _)| t) != title.as_ref() {
						if let Some(prev) = self.song.take() {
							self.scrobble_previous(&prev, now);
						}
						self.song = title.clone().map(|t| (t, now));
					}
					self.emit(TunerEvent::Title {
						station_id,
						title: title.clone(),
					});
					if let Some(title) = self.settle.title(title, now) {
						self.emit(TunerEvent::Announce(title));
					}
				}
			}
			StreamEvent::Failed(reason) => {
				// A failed stream is dead: drop it so the next focus on it reconnects.
				let open = self.open.remove(i);
				if playing {
					self.emit(TunerEvent::Failed {
						station_id,
						name: open.station.name,
						reason,
					});
				}
			}
			// A warm stream's outage is kept, so a swap onto it announces it before `Reconnected`.
			StreamEvent::Reconnecting => {
				self.open[i].reconnecting = true;
				if playing {
					self.emit(TunerEvent::Reconnecting { station_id });
				}
			}
			StreamEvent::Reconnected => {
				self.open[i].reconnecting = false;
				if playing {
					self.emit(TunerEvent::Reconnected { station_id });
				}
			}
			StreamEvent::Format(format) => {
				self.open[i].format = Some(format.clone());
				if playing {
					self.emit(TunerEvent::Format {
						station_id,
						format,
					});
				}
			}
			StreamEvent::FirstAudio => {}
		}
	}

	/// Token 0 is never issued, so silence never reports latency.
	fn silence(&mut self) {
		self.sink.send(MixerCmd::SetActive {
			ring: None,
			token: 0,
		});
		self.pending = None;
	}

	fn close_warm(&mut self) {
		let playing = self.playing.as_ref().map(|p| p.id.as_str());
		self.open.retain(|o| Some(o.station.id.as_str()) == playing);
	}

	fn is_open(&self, id: &str) -> bool {
		self.open.iter().any(|o| o.station.id == id)
	}

	fn emit(&self, event: TunerEvent) {
		let _ = self.events.send(event);
	}
}
