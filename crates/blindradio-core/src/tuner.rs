use std::collections::HashMap;
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant, SystemTime};

use crossbeam_channel::{Receiver, Sender};
use parking_lot::Mutex;

use std::path::PathBuf;

use crate::net;
use crate::output::{MixerCmd, Output};
use crate::recognize::{SongId, identify_audd, wav_bytes};
use crate::record::Recorder;
use crate::ring::PcmRing;
use crate::ringfeed::{RingFeed, WINDOW_SECONDS};
use crate::scrobble;
use crate::segments::{self, Audio, Jump, Marks, Segment, SegmentWriter, Which};
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
		at: SystemTime,
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

struct PendingTitle {
	station_id: String,
	title: Option<String>,
	received_ms: u64,
	received_at: Instant,
	mark_ms: u64,
	start_ms: Option<u64>,
	due_at: Instant,
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
	/// The encoded ring replay feed, when the playing station's
	/// time-shift uses the optional encoded ring.
	ring_feed: Option<RingFeed>,
	/// Session time the listener paused at, in ring mode: resuming
	/// after the window slid past it re-primes the window there.
	ring_paused_at: Option<u64>,
	/// The ring and its byte tap, kept across a mode switch so a chained
	/// stream that turns replayable finds the history it already has.
	encoded_ring: Option<Arc<crate::encoded_ring::EncodedRing>>,
	ring_tap: Option<Arc<crate::stream::RingTap>>,
	capture: Option<Arc<ShiftBuffer>>,
	record_slot: Option<Arc<Mutex<Option<Recorder>>>>,
	/// Song-split recording: the song the current file is recording,
	/// so a title event for the same song does not split it again.
	record_song: Option<String>,
	/// Last completed encoded chunk and playhead time written to Original.
	record_after: Option<u64>,
	record_at: Option<u64>,
	/// Where each title landed in the buffer: the segment bookmarks.
	marks: Marks,
	pending_titles: Vec<PendingTitle>,
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
			ring_feed: None,
			ring_paused_at: None,
			encoded_ring: None,
			ring_tap: None,
			capture: None,
			record_slot: None,
			record_song: None,
			record_after: None,
			record_at: None,
			marks: Marks::default(),
			pending_titles: Vec::new(),
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
		let snapshot = settings.lock().clone();
		for pending in &mut self.pending_titles {
			let offset = snapshot
				.metadata_offset_for(&pending.station_id)
				.unwrap_or(0);
			let offset_ms = i64::from(offset) * 1000;
			pending.mark_ms = if offset_ms >= 0 {
				pending.received_ms.saturating_add(offset_ms as u64)
			} else {
				pending.received_ms.saturating_sub(offset_ms.unsigned_abs())
			};
			pending.due_at = pending.received_at + Duration::from_secs(offset.max(0) as u64);
		}
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
			// Warming one would run yt-dlp for a station nobody asked to hear.
			.filter(|s| !crate::live::is_page(&s.url))
			.collect();
		self.tune(target, warm, repeat, now);
	}

	pub fn play(&mut self, station: &Station, now: Instant) {
		// A focus right after this is key repeat relative to the play, not to an older focus.
		self.last_focus = Some(now);
		// An explicit play always connects afresh: dropping the
		// station's open stream makes tune() take the cold path, so
		// Enter on the station already playing restarts it — the way
		// to hear an edited URL without switching away and back, and
		// tune() builds it a new time-shift buffer. Focus moves keep the
		// warm path and never restart a sounding station. A stream's own
		// reconnects never come here, so they keep the buffer.
		self.open.retain(|o| o.station.id != station.id);
		self.tune(station, Vec::new(), false, now);
	}

	pub fn stop(&mut self) {
		self.detach_taps();
		self.silence();
		self.open.clear();
		self.pending_titles.clear();
		self.playing = None;
		self.deferred = None;
		self.pending = None;
	}

	pub fn poll(&mut self, now: Instant, rendered: &[(u64, Instant)]) {
		self.record_buffered();
		while let Ok((serial, event)) = self.stream_rx.try_recv() {
			self.stream_event(serial, event, now);
		}
		self.flush_pending_titles(now);

		while let Ok((station_id, song)) = self.recog_rx.try_recv() {
			self.recog_in_flight = false;
			if let Some(song) = song
				&& self.playing.as_ref().is_some_and(|p| p.id == station_id)
			{
				let display = song.display();
				if self.song.as_ref().is_none_or(|(t, _)| *t != display) {
					self.mark_title(Some(&display));
					if let Some(open) = self.open.iter_mut().find(|o| o.station.id == station_id) {
						open.title = Some(display.clone());
					}
					self.emit(TunerEvent::Title {
						station_id: station_id.clone(),
						title: Some(display.clone()),
						at: SystemTime::now(),
					});
					self.song = Some((display.clone(), now));
					let announce = self
						.settings
						.as_ref()
						.is_some_and(|s| s.lock().recognition.announce);
					if announce && let Some(title) = self.settle.title(Some(display), now) {
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
		self.open_format(&self.playing.as_ref()?.id)
	}

	/// The output reopened at a new rate: everything open closes, so the restart after it
	/// decodes to that rate.
	pub fn set_rate(&mut self, rate: u32) {
		self.factory.set_rate(rate);
		self.stop();
	}

	fn tune(&mut self, target: &Station, warm: Vec<Station>, repeat: bool, now: Instant) {
		if self.playing.as_ref().is_some_and(|p| p.id != target.id) {
			self.pending_titles.clear();
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
					// The mixer's start cushion, so a ready neighbour swaps at once and
					// one still filling waits like a cold start.
					ring.trim_to_ms(crate::ring::start_ms());
					self.activate(target, ring, now, true);
					self.receive_title(&target.id, title.clone(), now);
					if reconnecting {
						self.emit(TunerEvent::Reconnecting {
							station_id: target.id.clone(),
						});
					}
					// Just focused, so this only holds the title for Settle.
					self.settle.title(title, now);
				}
				None => {
					// Starting the playing station over means a new buffer. A recording
					// still running keeps its taps: Enter has ended it by now, so this is
					// a lost connection or a new output rate, and detaching would end it
					// with no word to the user.
					if self.tap_station.as_deref() == Some(target.id.as_str()) && !self.is_recording() {
						self.detach_taps();
					}
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
			// Only a running recording gets here. The mixer dropped its buffer
			// on this start and must be given it back.
			if let Some((buffer, status)) = self.shift.clone() {
				self.ring_paused_at = None;
				self.sink.send(MixerCmd::ShiftAttach { buffer, status });
			}
			return;
		}
		self.detach_taps();
		let Some(settings) = &self.settings else {
			return;
		};
		let settings = settings.lock().clone();
		let rate = self.factory.rate();
		let minutes = settings.timeshift_minutes_for(&station.id);
		let ring_bytes = settings.timeshift_ring_bytes();
		let slot = Arc::new(Mutex::new(None));
		self.record_slot = Some(slot);
		if settings.recognition_enabled_for(&station.id) {
			let capture = Arc::new(ShiftBuffer::with_seconds(rate, 90));
			self.capture = Some(capture);
		}
		if minutes > 0 && ring_bytes > 0 {
			// The byte pump fills the ring from the first chunk; which
			// buffer plays waits for the format event when the stream
			// is cold, since only self-synchronizing codecs can be
			// replayed from the middle of the ring.
			let ring = Arc::new(crate::encoded_ring::EncodedRing::new(ring_bytes as usize));
			let tap = Arc::new(crate::stream::RingTap::new(ring.clone()));
			self.encoded_ring = Some(ring.clone());
			self.ring_tap = Some(tap.clone());
			let replayable = self
				.open_format(&station.id)
				.is_none_or(|format| format.replayable);
			if replayable {
				self.attach_ring_shift(rate, ring, tap);
			} else {
				self.attach_decoded_shift(rate, minutes);
			}
		} else if minutes > 0 {
			self.attach_decoded_shift(rate, minutes);
		}
		self.tap_station = Some(station.id.clone());
		let taps = self.current_taps();
		self.factory.set_taps(&station.id, taps);
	}

	/// A station's stream format once its decoder has reported it.
	fn open_format(&self, station_id: &str) -> Option<&crate::decode::StreamFormat> {
		self.open
			.iter()
			.find(|o| o.station.id == station_id)
			.and_then(|o| o.format.as_ref())
	}

	/// Plays the encoded ring's decoded window: the byte pump and replay
	/// feed keep the deep history, and the mixer plays a small slice of it.
	fn attach_ring_shift(
		&mut self,
		rate: u32,
		ring: Arc<crate::encoded_ring::EncodedRing>,
		tap: Arc<crate::stream::RingTap>,
	) {
		let status = Arc::new(ShiftStatus::new());
		let feed = RingFeed::new(ring, tap, status.clone(), rate);
		let buffer = feed.window();
		self.sink.send(MixerCmd::ShiftAttach {
			buffer: buffer.clone(),
			status: status.clone(),
		});
		self.sink
			.send(MixerCmd::RecordSlot(self.record_slot.clone()));
		self.shift = Some((buffer, status));
		self.ring_feed = Some(feed);
		self.ring_paused_at = None;
	}

	/// Plays the standard decoded-minutes buffer.
	fn attach_decoded_shift(&mut self, rate: u32, minutes: u32) {
		if let Some(feed) = self.ring_feed.take() {
			feed.stop();
		}
		// The mixer taps the shifted frames in this mode too, so it
		// holds the record slot just like the ring attach does.
		self.sink
			.send(MixerCmd::RecordSlot(self.record_slot.clone()));
		self.ring_paused_at = None;
		if minutes == 0 {
			return;
		}
		let buffer = Arc::new(ShiftBuffer::new(rate, minutes));
		let status = Arc::new(ShiftStatus::new());
		self.sink.send(MixerCmd::ShiftAttach {
			buffer: buffer.clone(),
			status: status.clone(),
		});
		self.shift = Some((buffer, status));
	}

	/// The taps to register from the tuner's current pieces. In ring mode
	/// the decoded shift tap stays off and the mixer's shifted frames go to
	/// the recorder through its slot; in decoded mode the stream taps them.
	fn current_taps(&self) -> Taps {
		let ring = self.ring_feed.is_some();
		Taps {
			shift: if ring {
				None
			} else {
				self.shift.as_ref().map(|(buffer, _)| buffer.clone())
			},
			capture: self.capture.clone(),
			record: self.record_slot.clone(),
			ring: self.ring_tap.clone(),
			// Both modes: the stream's live taps must yield while the
			// listener is time-shifted, because the mixer is tapping
			// the playhead audio into the recorder instead. Without
			// this the decoded mode recorded the live edge no matter
			// where the listener sat in the buffer.
			shift_status: self.shift.as_ref().map(|(_, status)| status.clone()),
		}
	}

	/// A format report can change the time-shift mode mid-stream: a cold
	/// stream starts before its codec is known, and a chained stream can
	/// switch codecs. Recording and recognition taps stay attached.
	fn sync_shift_mode(&mut self) {
		if self.ring_tap.is_none() {
			return;
		}
		let Some(station) = self.playing.clone() else {
			return;
		};
		if self.tap_station.as_deref() != Some(station.id.as_str()) {
			return;
		}
		let replayable = match self.open_format(&station.id) {
			Some(format) => format.replayable,
			None => return,
		};
		if replayable == self.ring_feed.is_some() {
			return;
		}
		if replayable {
			let (Some(ring), Some(tap)) = (self.encoded_ring.clone(), self.ring_tap.clone()) else {
				return;
			};
			self.attach_ring_shift(self.factory.rate(), ring, tap);
		} else {
			let Some(settings) = &self.settings else {
				return;
			};
			let minutes = settings.lock().timeshift_minutes_for(&station.id);
			self.attach_decoded_shift(self.factory.rate(), minutes);
		}
		self.factory.set_taps(&station.id, self.current_taps());
		// The two modes keep different timelines: old marks mean nothing here.
		self.marks = Marks::default();
		self.pending_titles.clear();
		let title = self.open.iter().find(|o| o.station.id == station.id);
		let title = title.and_then(|o| o.title.clone());
		self.mark_title(title.as_deref());
	}

	/// Stops any recording and unregisters the taps of the station they
	/// belonged to.
	fn detach_taps(&mut self) {
		self.record_buffered();
		if let Some(slot) = &self.record_slot
			&& let Some(rec) = slot.lock().take()
		{
			let _ = rec.stop();
		}
		if let Some(id) = self.tap_station.take() {
			self.factory.clear_taps(&id);
		}
		if let Some(feed) = self.ring_feed.take() {
			feed.stop();
		}
		self.sink.send(MixerCmd::RecordSlot(None));
		self.ring_paused_at = None;
		self.encoded_ring = None;
		self.ring_tap = None;
		self.shift = None;
		self.capture = None;
		self.record_slot = None;
		self.record_song = None;
		self.record_at = None;
		self.record_after = None;
		self.marks = Marks::default();
		self.pending_titles.clear();
		self.song = None;
	}

	/// The oldest time the buffer holds, the live edge and the playhead,
	/// on the timeline the marks use. `None` with no time-shift buffer.
	fn timeline(&self) -> Option<(u64, u64, u64)> {
		let (buffer, status) = self.shift.as_ref()?;
		if let Some(feed) = &self.ring_feed {
			return Some((feed.ring_start_ms(), feed.now_ms(), feed.cursor_time()));
		}
		let ms = |frame: u64| frame * 1000 / u64::from(buffer.rate());
		let now = ms(buffer.live_edge());
		let cursor = status.cursor.load(std::sync::atomic::Ordering::Relaxed);
		let playhead = ms(if cursor == u64::MAX { status.live_frame(buffer) } else { cursor });
		Some((ms(buffer.start()), now, playhead))
	}

	/// Where live playback stands on the marks' timeline. Session time in
	/// ring mode already is that: a burst arrives in a moment and plays out.
	pub fn live_ms(&self) -> Option<u64> {
		let (buffer, status) = self.shift.as_ref()?;
		if let Some(feed) = &self.ring_feed {
			return Some(feed.now_ms());
		}
		Some(status.live_frame(buffer) * 1000 / u64::from(buffer.rate()))
	}

	/// How far behind live the oldest audio held is, in seconds. Ring mode
	/// asks the ring: its deep history reaches past the decoded window.
	pub fn buffer_depth_seconds(&self) -> f32 {
		let Some((buffer, status)) = &self.shift else {
			return 0.0;
		};
		match &self.ring_feed {
			Some(feed) => feed.now_ms().saturating_sub(feed.ring_start_ms()) as f32 / 1000.0,
			None => status.depth_seconds(buffer),
		}
	}

	/// A title landed at the live edge: it starts a segment there.
	fn mark_title(&mut self, title: Option<&str>) {
		if let (Some(title), Some((start, now, _))) = (title, self.timeline()) {
			self.marks.add(title, now, start);
		}
	}

	fn receive_title(&mut self, station_id: &str, title: Option<String>, now: Instant) {
		let offset = self
			.settings
			.as_ref()
			.and_then(|s| s.lock().metadata_offset_for(station_id))
			.unwrap_or(0);
		let offset_ms = i64::from(offset) * 1000;
		let timeline = self.timeline();
		let live_ms = timeline.map_or(0, |(_, live, _)| live);
		let mark_ms = if offset_ms >= 0 {
			live_ms.saturating_add(offset_ms as u64)
		} else {
			live_ms.saturating_sub(offset_ms.unsigned_abs())
		};
		if offset > 0 && (timeline.is_some_and(|(_, _, playhead)| mark_ms > playhead) || timeline.is_none()) {
			self.pending_titles.push(PendingTitle {
				station_id: station_id.to_owned(),
				title,
				received_ms: live_ms,
				received_at: now,
				mark_ms,
				start_ms: timeline.map(|(start, _, _)| start),
				due_at: now + Duration::from_secs(offset as u64),
			});
		} else {
			let late = if timeline.is_none() && offset < 0 {
				Duration::from_secs(offset.unsigned_abs() as u64)
			} else {
				Duration::ZERO
			};
			self.apply_title(
				station_id,
				title,
				now,
				timeline.map(|(start, _, playhead)| (start, live_ms, mark_ms, playhead)),
				late,
			);
		}
	}

	fn flush_pending_titles(&mut self, now: Instant) {
		let timeline = self.timeline();
		self.pending_titles.sort_by_key(|p| p.due_at);
		while self.pending_titles.first().is_some_and(|p| {
			now >= p.due_at && timeline.is_none_or(|(_, _, playhead)| p.mark_ms <= playhead)
		}) {
			let pending = self.pending_titles.remove(0);
			if self.playing.as_ref().is_some_and(|p| p.id == pending.station_id) {
				let (coords, late) = match timeline {
					Some((_, _, playhead)) => (
						pending.start_ms.map(|start| (start, pending.received_ms, pending.mark_ms, playhead)),
						Duration::ZERO,
					),
					None => (None, now.duration_since(pending.due_at)),
				};
				self.apply_title(
					&pending.station_id,
					pending.title,
					now,
					coords,
					late,
				);
			}
		}
	}

	fn apply_title(
		&mut self,
		station_id: &str,
		title: Option<String>,
		now: Instant,
		timeline: Option<(u64, u64, u64, u64)>,
		fallback_late: Duration,
	) {
		let late = timeline.map_or(fallback_late, |(_, _, mark_ms, playhead_ms)| Duration::from_millis(playhead_ms.saturating_sub(mark_ms)));
		let (event_at, event_instant) = if let Some((start_ms, received_ms, mark_ms, _)) = timeline {
			self.marks.add_adjusted(
				title.as_deref().unwrap_or_default(),
				received_ms,
				mark_ms,
				start_ms,
			);
			(
				SystemTime::now().checked_sub(late).unwrap_or(SystemTime::now()),
				now.checked_sub(late).unwrap_or(now),
			)
		} else {
			(
				SystemTime::now().checked_sub(fallback_late).unwrap_or(SystemTime::now()),
				now.checked_sub(fallback_late).unwrap_or(now),
			)
		};
		if self.song.as_ref().map(|(t, _)| t) != title.as_ref() {
			if let Some(prev) = self.song.take() {
				self.scrobble_previous(&prev, event_instant);
			}
			self.song = title.clone().map(|t| (t, event_instant));
		}
		self.emit(TunerEvent::Title {
			station_id: station_id.to_owned(),
			title: title.clone(),
			at: event_at,
		});
		if let Some(title) = self.settle.title(title, now) {
			self.emit(TunerEvent::Announce(title));
		}
	}

	/// The title of the audio at the playhead.
	pub fn heard_title(&self) -> Option<String> {
		let (_, _, playhead) = self.timeline()?;
		self.marks.title_at(playhead).map(str::to_string)
	}

	/// Every segment the buffer still holds, oldest first.
	pub fn segments(&self) -> Vec<Segment> {
		self.timeline()
			.map_or_else(Vec::new, |(start, now, _)| self.marks.segments(start, now))
	}

	/// The segment at, before or after the playhead.
	pub fn segment(&self, which: Which) -> Option<Segment> {
		let (_, _, playhead) = self.timeline()?;
		segments::pick(&self.segments(), playhead, which).cloned()
	}

	/// Moves playback to the previous or next segment and returns where it
	/// landed. `Live` is left for the caller: going live says so itself.
	pub fn segment_jump(&mut self, forward: bool) -> Jump {
		let Some((_, _, playhead)) = self.timeline() else {
			return Jump::Nowhere;
		};
		let shifted = self
			.shift
			.as_ref()
			.is_some_and(|(_, st)| st.cursor.load(std::sync::atomic::Ordering::Relaxed) != u64::MAX);
		let jump = segments::jump(&self.segments(), playhead, forward, shifted);
		if let Jump::To(segment) = &jump {
			self.seek_to_ms(segment.start_ms);
		}
		jump
	}

	fn seek_to_ms(&mut self, ms: u64) {
		self.record_buffered();
		self.record_at = None;
		if let Some(feed) = &self.ring_feed {
			self.ring_paused_at = None;
			let buffer = feed.prime(ms);
			self.sink.send(MixerCmd::ShiftAttach {
				buffer: buffer.clone(),
				status: feed.status(),
			});
			self.sink
				.send(MixerCmd::ShiftSeek(-(WINDOW_SECONDS as i32)));
			self.shift = Some((buffer, feed.status()));
			return;
		}
		if let Some((buffer, status)) = &self.shift {
			// Rounded up: a frame short would still be the segment before,
			// and a paused playhead would stay there.
			let frame = (ms * u64::from(buffer.rate())).div_ceil(1000);
			// Read back before the mixer's next pass: the next key and the
			// status line must already see the new position.
			status.cursor.store(frame, std::sync::atomic::Ordering::Relaxed);
			self.sink.send(MixerCmd::ShiftSeekTo(frame));
		}
	}

	/// What writes segments to files, with the settings as they are now.
	pub fn segment_writer(&self) -> Option<SegmentWriter> {
		let station = self.playing.as_ref()?;
		let (_, now_ms, _) = self.timeline()?;
		let (dir, ext, settings) = self.recording_plan()?;
		let audio = match (&self.ring_feed, &self.encoded_ring, &self.shift) {
			(Some(_), Some(ring), _) => Audio::Ring(ring.clone()),
			(_, _, Some((buffer, _))) => Audio::Decoded(buffer.clone()),
			_ => return None,
		};
		Some(SegmentWriter {
			audio,
			dir,
			station: station.name.clone(),
			settings: settings.recording,
			rate: self.factory.rate(),
			ext,
			now_ms,
			now_epoch: std::time::SystemTime::now()
				.duration_since(std::time::UNIX_EPOCH)
				.map_or(0, |d| d.as_secs()),
		})
	}

	/// The playing station's time-shift buffer and its live status.
	pub fn shift(&self) -> Option<(Arc<ShiftBuffer>, Arc<ShiftStatus>)> {
		self.shift.clone()
	}

	pub fn shift_pause(&mut self, paused: bool) {
		if let Some(feed) = &self.ring_feed {
			if paused {
				self.ring_paused_at = Some(feed.cursor_time());
				self.sink.send(MixerCmd::ShiftPause(true));
			} else if let Some(at) = self.ring_paused_at.take()
				&& at < feed.window_start_ms()
			{
				// The window slid past the pause point while paused;
				// re-prime there. Attaching also unpauses in the mixer,
				// and the seek places the cursor at the window start.
				let buffer = feed.prime(at);
				self.sink.send(MixerCmd::ShiftAttach {
					buffer: buffer.clone(),
					status: feed.status(),
				});
				self.sink
					.send(MixerCmd::ShiftSeek(-(WINDOW_SECONDS as i32)));
				self.shift = Some((buffer, feed.status()));
			} else {
				self.sink.send(MixerCmd::ShiftPause(false));
			}
			return;
		}
		self.sink.send(MixerCmd::ShiftPause(paused));
	}

	pub fn shift_seek(&mut self, seconds: i32) {
		self.record_buffered();
		self.record_at = None;
		if let Some(feed) = &self.ring_feed {
			let target = (feed.cursor_time() as i64 + seconds as i64 * 1000)
				.max(feed.ring_start_ms() as i64) as u64;
			if seconds > 0 && target >= feed.now_ms() {
				// At or past live: go live for real instead of clamping
				// at the window's edge, which can trail the live edge.
				self.shift_live();
			} else if (seconds < 0 && target < feed.window_start_ms())
				|| (seconds > 0 && target > feed.window_end_ms())
			{
				// Outside the window: re-prime at the target instead of
				// letting the mixer clamp at the window's edge.
				let buffer = feed.prime(target);
				self.sink.send(MixerCmd::ShiftAttach {
					buffer: buffer.clone(),
					status: feed.status(),
				});
				self.sink
					.send(MixerCmd::ShiftSeek(-(WINDOW_SECONDS as i32)));
				self.shift = Some((buffer, feed.status()));
			} else {
				self.sink.send(MixerCmd::ShiftSeek(seconds));
			}
			return;
		}
		self.sink.send(MixerCmd::ShiftSeek(seconds));
	}

	/// Jumps to the oldest audio the buffer still holds.
	pub fn shift_start(&mut self) {
		self.record_buffered();
		self.record_at = None;
		if let Some(feed) = &self.ring_feed {
			self.ring_paused_at = None;
			let buffer = feed.prime(feed.ring_start_ms());
			self.sink.send(MixerCmd::ShiftAttach {
				buffer: buffer.clone(),
				status: feed.status(),
			});
			self.sink
				.send(MixerCmd::ShiftSeek(-(WINDOW_SECONDS as i32)));
			self.shift = Some((buffer, feed.status()));
			return;
		}
		// The mixer clamps at the buffer's start.
		self.sink.send(MixerCmd::ShiftSeek(i32::MIN));
	}

	pub fn shift_live(&mut self) {
		self.record_buffered();
		self.record_at = None;
		if let Some(feed) = &self.ring_feed {
			self.ring_paused_at = None;
			let buffer = feed.prime_live();
			self.sink.send(MixerCmd::ShiftAttach {
				buffer: buffer.clone(),
				status: feed.status(),
			});
			self.shift = Some((buffer, feed.status()));
			return;
		}
		self.sink.send(MixerCmd::ShiftLive);
	}

	pub fn is_recording(&self) -> bool {
		self.record_slot
			.as_ref()
			.is_some_and(|slot| slot.lock().is_some())
	}

	/// Original recordings follow the playhead in both buffer modes.
	/// Polling here keeps recording independent of replay decoder read-ahead.
	fn record_buffered(&mut self) {
		let Some((buffer, status)) = &self.shift else {
			return;
		};
		let cursor = status.cursor.load(std::sync::atomic::Ordering::Relaxed);
		if cursor == u64::MAX {
			self.record_at = None;
			return;
		}
		let Some(slot) = &self.record_slot else {
			return;
		};
		let mut guard = slot.lock();
		let Some(rec) = guard.as_mut().filter(|rec| rec.is_original()) else {
			return;
		};
		let (ring, time) = if let Some(feed) = &self.ring_feed {
			let Some(ring) = &self.encoded_ring else {
				return;
			};
			(ring.as_ref(), feed.cursor_time())
		} else {
			(&buffer.encoded, cursor * 1000 / buffer.rate() as u64)
		};
		if self.record_at.is_none_or(|last| time < last) {
			self.record_after = ring.seq_at(time).and_then(|seq| seq.checked_sub(1));
		}
		self.record_at = Some(time);
		if status.paused.load(std::sync::atomic::Ordering::Relaxed) {
			return;
		}
		// Whole finished chunks only: never copy future audio at the live edge.
		let (bytes, last) = ring.chunks_ended_after_seq(self.record_after, time);
		if !bytes.is_empty() {
			let _ = rec.write_bytes(&bytes);
			self.record_after = last;
		}
	}

	/// Where a recording goes, its source extension, and the settings
	/// snapshot: everything starting or splitting one needs.
	fn recording_plan(&self) -> Option<(PathBuf, String, crate::settings::Settings)> {
		let station = self.playing.as_ref()?;
		let mut settings = self.settings.as_ref()?.lock().clone();
		// Containers needing opening headers cannot form valid buffered or
		// song-split source files; keep every part of this recording as WAV.
		if settings.recording.format == crate::settings::RecordFormat::Original
			&& self
				.open_format(&station.id)
				.is_some_and(|format| !format.replayable)
		{
			settings.recording.format = crate::settings::RecordFormat::Wav;
		}
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
			.filter(|e| {
				matches!(
					e.to_ascii_lowercase().as_str(),
					"mp3" | "aac" | "ogg" | "flac" | "m4a"
				)
			})
			.unwrap_or("mp3")
			.to_string();
		Some((dir, ext, settings))
	}

	/// Starts recording the playing station; returns the file path.
	/// `song` is what the listener is hearing right now — the playhead's
	/// song in the buffer, the live one at the edge — and in song-split
	/// mode it names the first file. `None` records under the station's
	/// own name, as a station that names no songs does.
	pub fn start_recording(&mut self, song: Option<&str>) -> Result<PathBuf> {
		let station = self
			.playing
			.clone()
			.ok_or_else(|| CoreError::Output("Nothing is playing".into()))?;
		let slot = self
			.record_slot
			.clone()
			.ok_or_else(|| CoreError::Output("Recording is unavailable".into()))?;
		let (dir, ext, settings) = self
			.recording_plan()
			.ok_or_else(|| CoreError::Output("Recording is unavailable".into()))?;
		let song = song.map(str::to_string);
		let (rec, record_song) = if settings.recording.split_songs {
			match &song {
				Some(song) => (
					Recorder::start_song(
						&dir,
						&station.name,
						song,
						&settings.recording,
						self.factory.rate(),
						&ext,
					)?,
					Some(song.clone()),
				),
				None => (
					Recorder::start(
						&dir,
						&station.name,
						&settings.recording,
						self.factory.rate(),
						&ext,
					)?,
					None,
				),
			}
		} else {
			(
				Recorder::start(
					&dir,
					&station.name,
					&settings.recording,
					self.factory.rate(),
					&ext,
				)?,
				None,
			)
		};
		let path = rec.path().to_path_buf();
		*slot.lock() = Some(rec);
		self.record_song = record_song;
		self.record_at = None;
		self.record_buffered();
		Ok(path)
	}

	/// Song-split recording: a new song closes the current file and
	/// opens one named for it. Returns the finished file's result, or
	/// None when there is nothing to split (not recording, the mode
	/// is off, or the song is the one already recording).
	pub fn split_recording(&mut self, song: &str) -> Option<Result<PathBuf>> {
		if song.is_empty() || !self.is_recording() {
			return None;
		}
		if self.record_song.as_deref() == Some(song) {
			return None;
		}
		self.record_buffered();
		let (dir, ext, settings) = self.recording_plan()?;
		if !settings.recording.split_songs {
			return None;
		}
		let station = self.playing.clone()?;
		let slot = self.record_slot.clone()?;
		let paused = slot.lock().as_ref().is_some_and(|rec| rec.is_paused());
		let old = slot.lock().take()?;
		self.record_song = Some(song.to_string());
		let done = old.stop();
		if done.is_err() {
			self.record_song = None;
			return Some(done);
		}
		match Recorder::start_song(
			&dir,
			&station.name,
			song,
			&settings.recording,
			self.factory.rate(),
			&ext,
		) {
			Ok(mut rec) => {
				rec.set_paused(paused);
				*slot.lock() = Some(rec);
				Some(done)
			}
			Err(e) => {
				self.record_song = None;
				Some(Err(e))
			}
		}
	}

	/// Stops the recording, returning its file path.
	pub fn stop_recording(&mut self) -> Option<Result<PathBuf>> {
		self.record_buffered();
		self.record_song = None;
		self.record_at = None;
		let slot = self.record_slot.clone()?;
		slot.lock().take().map(|rec| rec.stop())
	}

	/// Pauses or resumes the active recording; false when none is running.
	/// A paused recording drops the audio written to it, so the file
	/// holds only the spans the user let through.
	pub fn set_recording_paused(&mut self, paused: bool) -> bool {
		self.record_buffered();
		let Some(slot) = &self.record_slot else {
			return false;
		};
		let mut guard = slot.lock();
		match guard.as_mut() {
			Some(rec) => {
				rec.set_paused(paused);
				true
			}
			None => false,
		}
	}

	pub fn recording_paused(&self) -> bool {
		self.record_slot
			.as_ref()
			.is_some_and(|slot| slot.lock().as_ref().is_some_and(|rec| rec.is_paused()))
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
		let Some(token) = settings
			.recognition
			.audd_token
			.clone()
			.filter(|t| !t.is_empty())
		else {
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
					self.receive_title(&station_id, title, now);
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
					self.emit(TunerEvent::Format { station_id, format });
					self.sync_shift_mode();
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

#[cfg(test)]
mod recording_tests {
	use super::*;
	use std::sync::atomic::Ordering;

	struct NoStreams;
	impl StreamFactory for NoStreams {
		fn start(
			&self,
			_: &Station,
			_: Box<dyn Fn(StreamEvent) + Send + Sync>,
		) -> (Arc<PcmRing>, Box<dyn Send>) {
			unreachable!()
		}
		fn rate(&self) -> u32 {
			1000
		}
	}
	struct NoSink;
	impl Sink for NoSink {
		fn send(&self, _: MixerCmd) {}
	}

	#[test]
	fn titles_become_segments_the_playhead_can_jump_and_write() {
		let dir = tempfile::tempdir().unwrap();
		let mut settings = Settings::default();
		settings.recording.dir = Some(dir.path().to_string_lossy().into_owned());
		settings.recording.format = crate::settings::RecordFormat::Wav;
		let station = Station {
			id: "test".into(),
			name: "Test".into(),
			url: "http://test/stream.mp3".into(),
			source: crate::station::Source::Custom,
			info: None,
			details: Default::default(),
		};
		let (tx, _) = crossbeam_channel::unbounded();
		let mut tuner = Tuner::new(NoStreams, NoSink, tx);
		tuner.set_settings(Arc::new(Mutex::new(settings)));
		tuner.playing = Some(station.clone());
		tuner.attach_taps(&station);
		let (buffer, status) = tuner.shift().unwrap();
		tuner.mark_title(Some("One"));
		buffer.push(&vec![0.0; 2 * 20_000]);
		tuner.mark_title(Some("Two"));
		buffer.push(&vec![0.0; 2 * 20_000]);
		tuner.mark_title(Some("Three"));
		buffer.push(&vec![0.0; 2 * 10_000]);

		let titles = |t: &Tuner<NoStreams, NoSink>| {
			t.segments().into_iter().map(|s| s.title).collect::<Vec<_>>()
		};
		assert_eq!(titles(&tuner), ["One", "Two", "Three"]);
		assert_eq!(tuner.heard_title().as_deref(), Some("Three"), "live");
		assert_eq!(tuner.segment(Which::Next), None, "nothing after live");

		// Well into the live segment, Previous restarts it; again, it steps.
		assert!(matches!(tuner.segment_jump(false), Jump::To(s) if s.title == "Three"));
		assert_eq!(status.cursor.load(Ordering::Relaxed), 40_000);
		assert!(matches!(tuner.segment_jump(false), Jump::To(s) if s.title == "Two"));
		assert_eq!(status.cursor.load(Ordering::Relaxed), 20_000);
		assert_eq!(tuner.heard_title().as_deref(), Some("Two"));
		assert_eq!(tuner.segment(Which::Previous).unwrap().title, "One");
		assert_eq!(tuner.segment(Which::Next).unwrap().title, "Three");
		assert!(matches!(tuner.segment_jump(true), Jump::To(s) if s.title == "Three"));
		assert_eq!(tuner.segment_jump(true), Jump::Live, "shifted past the last");

		let writer = tuner.segment_writer().unwrap();
		let paths = writer.write(&tuner.segments()).unwrap();
		assert_eq!(paths.len(), 3);
		let frames: Vec<u32> = paths
			.iter()
			.map(|p| hound::WavReader::open(p).unwrap().duration())
			.collect();
		assert_eq!(frames, [20_000, 20_000, 10_000]);

		tuner.detach_taps();
		assert!(tuner.segments().is_empty(), "a new station starts clean");
	}

	#[test]
	fn original_recording_paces_chunks_and_split_pause_and_seek_follow_the_playhead() {
		for encoded_mode in [false, true] {
			let dir = tempfile::tempdir().unwrap();
			let mut settings = Settings::default();
			settings.recording.dir = Some(dir.path().to_string_lossy().into_owned());
			settings.recording.split_songs = true;
			let station = Station {
				id: "test".into(),
				name: "Test".into(),
				url: "http://test/stream.mp3".into(),
				source: crate::station::Source::Custom,
				info: None,
				details: Default::default(),
			};
			let (tx, _) = crossbeam_channel::unbounded();
			let mut tuner = Tuner::new(NoStreams, NoSink, tx);
			tuner.set_settings(Arc::new(Mutex::new(settings)));
			tuner.playing = Some(station.clone());
			tuner.attach_taps(&station);
			let (buffer, status) = tuner.shift().unwrap();
			for byte in 0..6 {
				buffer.push_encoded(&[byte]);
				buffer.push(&vec![0.0; 2000]);
			}
			if encoded_mode {
				let ring = Arc::new(crate::encoded_ring::EncodedRing::new(100));
				for byte in 0..6 {
					ring.push(byte as u64 * 1000, &[byte]);
				}
				let tap = Arc::new(crate::stream::RingTap::new(ring.clone()));
				let feed = RingFeed::new(ring.clone(), tap, status.clone(), 1000);
				feed.stop(); // Recording must progress even when replay decoding stops.
				tuner.encoded_ring = Some(ring);
				tuner.ring_feed = Some(feed);
			}
			status.cursor.store(0, Ordering::Relaxed);
			tuner.start_recording(Some("First")).unwrap();
			status.cursor.store(1000, Ordering::Relaxed);
			let first = tuner.split_recording("Second").unwrap().unwrap();
			assert_eq!(std::fs::read(first).unwrap(), [0]);
			status.cursor.store(2500, Ordering::Relaxed);
			tuner.set_recording_paused(true);
			status.cursor.store(3500, Ordering::Relaxed);
			tuner.set_recording_paused(false);
			status.cursor.store(4000, Ordering::Relaxed);
			let second = tuner.stop_recording().unwrap().unwrap();
			assert_eq!(std::fs::read(second).unwrap(), [1, 3]);
			// A new recording and backwards seek restart from their own cursor.
			status.cursor.store(1000, Ordering::Relaxed);
			tuner.start_recording(None).unwrap();
			status.cursor.store(2000, Ordering::Relaxed);
			tuner.poll(Instant::now(), &[]);
			status.cursor.store(0, Ordering::Relaxed);
			tuner.poll(Instant::now(), &[]);
			status.cursor.store(1000, Ordering::Relaxed);
			let third = tuner.stop_recording().unwrap().unwrap();
			assert_eq!(std::fs::read(third).unwrap(), [1, 0]);
		}
	}
}
