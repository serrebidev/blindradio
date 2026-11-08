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
	/// A background recording's own stream failed for good: the
	/// recording ended and its file was saved.
	RecordingFailed {
		station_id: String,
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

	/// The taps currently registered for a station, so a caller can
	/// merge one field in instead of replacing the whole entry.
	fn taps(&self, _station_id: &str) -> Taps {
		Taps::default()
	}

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

	fn taps(&self, station_id: &str) -> Taps {
		crate::stream::taps_for(&self.0.taps, station_id)
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

/// A recording that outlived its station's turn at playback: the
/// station's stream, opened independently of the tuner's open list,
/// plus where its titles land — a background stream's titles drive
/// song splits and never reach the listener.
struct BgRecord {
	station_id: String,
	slot: Arc<Mutex<Option<Recorder>>>,
	titles: Receiver<(u64, StreamEvent)>,
	serial: u64,
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

/// The file suffix an `Original` recording takes: the decoded codec
/// when it is known (an AAC stream at an extensionless address is
/// still AAC), else the address's own suffix with any query stripped,
/// else MP3. Only self-synchronizing formats ever reach a passthrough
/// file (the rest record as WAV), so the codec list is short.
fn recording_source_ext(codec: Option<&str>, url: &str) -> String {
	match codec {
		Some("AAC") => return "aac".into(),
		Some("MP3") => return "mp3".into(),
		Some("MP2") => return "mp2".into(),
		_ => {}
	}
	let last = url
		.split(['?', '#'])
		.next()
		.unwrap_or(url)
		.rsplit('/')
		.next()
		.unwrap_or_default();
	last.rsplit('.')
		.next()
		.filter(|e| {
			matches!(
				e.to_ascii_lowercase().as_str(),
				"mp3" | "aac" | "mp2" | "ogg" | "flac" | "m4a"
			)
		})
		.unwrap_or("mp3")
		.to_string()
}

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
	/// The recording detached from playback by a station change: its
	/// stream stays open and keeps feeding the recorder while the
	/// listener hears another station. The stream is kept alive by
	/// the slot's task, not by an `Open` entry, so warm-close and
	/// focus moves never drop it.
	bg_record: Option<BgRecord>,
	/// The station a paused background recording belongs to, kept so
	/// resuming can reopen its stream after the pause closed it.
	detached_station: Option<Station>,
	record_detached_paused: bool,
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
			bg_record: None,
			detached_station: None,
			record_detached_paused: false,
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
		// Stopping playback leaves a background recording running:
		// it was detached from playback on purpose. Only an explicit
		// stop_recording (or shutdown, which drops the tuner) ends it.
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
		if let Some(station_id) = self.poll_bg() {
			self.emit(TunerEvent::RecordingFailed { station_id });
		}
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
			// A recording in progress does not end here: it detaches
			// onto its own stream and keeps recording in the
			// background while this station plays.
			self.detach_recording();
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
		// A background recording's slot is borrowed, not owned: the
		// playing station never replaces it (detach_taps preserved it
		// for exactly this), so a fresh attach reuses it.
		if self.record_slot.is_none() {
			self.record_slot = Some(Arc::new(Mutex::new(None)));
		}
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
			// A background recording of this station keeps its field
			// when playback taps are (re)registered for it.
			bg_record: self.bg_slot(),
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

	/// Moves a running recording off the playing station onto its own
	/// stream, so playback can change stations without ending it. The
	/// recorder keeps its slot — start/stop, pause and song splits keep
	/// working unchanged — and a fresh stream of the same station feeds
	/// it, opened outside the open list so nothing in playback
	/// management can close it. A paused recording needs no stream: it
	/// writes nothing, and its slot waits where it is.
	fn detach_recording(&mut self) {
		let Some(slot) = self.record_slot.clone() else {
			return;
		};
		if self.bg_record.is_some() || self.record_detached_paused {
			return;
		}
		if slot.lock().is_none() {
			return;
		}
		let Some(station) = self.playing.clone() else {
			return;
		};
		// Even a paused recording detaches: its station is remembered
		// so a later resume reopens the stream (see set_recording_paused).
		self.detached_station = Some(station.clone());
		if slot.lock().as_ref().is_some_and(|rec| rec.is_paused()) {
			self.record_detached_paused = true;
			return;
		}
		self.detach_recording_for(Some(station));
	}

	/// Opens the background stream for a detached recording of
	/// `station` and points the station's taps at the recorder's slot.
	fn detach_recording_for(&mut self, station: Option<Station>) {
		let Some(station) = station else {
			return;
		};
		let Some(slot) = self.record_slot.clone() else {
			return;
		};
		if self.bg_record.is_some() || slot.lock().is_none() {
			return;
		}
		self.detached_station = Some(station.clone());
		self.next_id += 1;
		let serial = self.next_id;
		let tx = self.stream_tx.clone();
		let (titles_tx, titles_rx) = crossbeam_channel::unbounded();
		let (_, guard) = self.factory.start(
			&station,
			Box::new(move |event| {
				// Title and Format go to the background drain, for
				// splits and the file's format name; a failure goes
				// there too, so the drain can end the recording with
				// its file saved. The shared channel copies are
				// ignored in stream_event (this serial is not open).
				if matches!(
					event,
					StreamEvent::Title(_) | StreamEvent::Format(_) | StreamEvent::Failed(_)
				) {
					let _ = titles_tx.send((serial, event.clone()));
				}
				let _ = tx.send((serial, event));
			}),
		);
		// Merge into any existing entry: the same station may be
		// playing, with playback taps of its own in this map.
		let mut entry = self.factory.taps(&station.id);
		entry.bg_record = Some(slot.clone());
		self.factory.set_taps(&station.id, entry);
		self.bg_record = Some(BgRecord {
			station_id: station.id.clone(),
			slot,
			titles: titles_rx,
			serial,
			_guard: guard,
		});
	}

	/// Drains a background recording's stream events: its titles split
	/// its files in song-split mode, its format report names an
	/// Original file correctly, and a failed background stream ends
	/// the recording with its file saved — the app hears about that
	/// one, through the returned station id.
	fn poll_bg(&mut self) -> Option<String> {
		let (station_id, events) = {
			let bg = self.bg_record.as_ref()?;
			let mut events = Vec::new();
			while let Ok((serial, event)) = bg.titles.try_recv() {
				if serial == bg.serial {
					events.push(event);
				}
			}
			(bg.station_id.clone(), events)
		};
		let mut failed = false;
		for event in events {
			match event {
				StreamEvent::Title(Some(song)) => {
					let _ = self.split_recording(&song);
				}
				StreamEvent::Title(None) => {}
				StreamEvent::Format(format) => {
					if matches!(format.codec.as_str(), "AAC" | "MP3" | "MP2")
						&& let Some(bg) = &self.bg_record
					{
						let mut guard = bg.slot.lock();
						if let Some(rec) = guard.as_mut() {
							let ext = recording_source_ext(Some(&format.codec), "");
							let _ = rec.align_source_ext(&ext);
						}
					}
				}
				StreamEvent::Failed(_) => failed = true,
				_ => {}
			}
		}
		if failed && self.bg_record.is_some() {
			let _ = self.stop_recording();
			return Some(station_id);
		}
		None
	}

	/// Unregisters the taps of the station they belonged to. A
	/// recording survives: a detached one keeps its own stream; one
	/// still on the playing station's taps is stopped only when those
	/// taps are torn down for good (stop, a new output rate, app
	/// shutdown) — station changes detach it first, in tune().
	fn detach_taps(&mut self) {
		self.record_buffered();
		// Stop the recorder only when the taps being torn down are
		// the recording's own AND no background stream feeds it. In
		// tune() the recorder is detached first, so by the time the
		// old station's taps come off, bg_record is set and this is
		// false; a plain stop()/set_rate teardown has no background
		// stream and stops it, as before.
		let own_taps = self.bg_record.is_none()
			&& !self.record_detached_paused
			&& match (&self.tap_station, self.recording_station_id()) {
				(Some(tap), Some(rec)) => tap == &rec,
				_ => true,
			};
		if own_taps
			&& let Some(slot) = &self.record_slot
			&& let Some(rec) = slot.lock().take()
		{
			let _ = rec.stop();
		}
		if let Some(id) = self.tap_station.take() {
			// A background recording of this same station keeps its
			// own field in the entry: strip only the playback fields.
			if self.bg_record.as_ref().is_some_and(|bg| bg.station_id == id) {
				let mut entry = self.factory.taps(&id);
				entry.shift = None;
				entry.capture = None;
				entry.record = None;
				entry.ring = None;
				entry.shift_status = None;
				self.factory.set_taps(&id, entry);
			} else {
				self.factory.clear_taps(&id);
			}
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
		if own_taps {
			self.record_slot = None;
			self.record_song = None;
			self.record_at = None;
			self.record_after = None;
		}
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

	/// True while the active recording is a detached background one.
	pub fn recording_is_background(&self) -> bool {
		(self.bg_record.is_some() || self.record_detached_paused) && self.is_recording()
	}

	/// The detached recording's stream slot, while one runs in the
	/// background.
	pub fn bg_slot(&self) -> Option<Arc<Mutex<Option<Recorder>>>> {
		self.bg_record.as_ref().map(|bg| bg.slot.clone())
	}

	/// The station the active recording belongs to: the detached one
	/// while it runs in the background, the playing one otherwise.
	pub fn recording_station(&self) -> Option<Station> {
		if !self.is_recording() {
			return None;
		}
		if self.bg_record.is_some() || self.record_detached_paused {
			return self.detached_station.clone();
		}
		self.playing.clone()
	}

	/// The station the active recording belongs to, playing or detached.
	pub fn recording_station_id(&self) -> Option<String> {
		if !self.is_recording() {
			return None;
		}
		if let Some(bg) = &self.bg_record {
			return Some(bg.station_id.clone());
		}
		if self.record_detached_paused {
			return self.detached_station.as_ref().map(|s| s.id.clone());
		}
		if self.tap_station.is_some() {
			return self.tap_station.clone();
		}
		self.playing.as_ref().map(|p| p.id.clone())
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
		self.recording_plan_for(self.playing.as_ref().map(|p| p.id.clone()))
	}

	/// The plan for a recording of `station_id` — the playing station
	/// for a fresh recording, the detached one for a background split
	/// or a scheduled start (where the station is given, not played).
	fn recording_plan_for(
		&self,
		station_id: Option<String>,
	) -> Option<(PathBuf, String, crate::settings::Settings)> {
		let station_id = station_id?;
		let station = if self.playing.as_ref().is_some_and(|p| p.id == station_id) {
			self.playing.clone()?
		} else {
			// A scheduled start names its station before any detach
			// exists: the caller sets it first (start_background_recording
			// does), so reaching here without it means no station.
			self.detached_station
				.clone()
				.filter(|s| s.id == station_id)?
		};
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
		let codec = self.open_format(&station.id).map(|f| f.codec.as_str());
		let ext = recording_source_ext(codec, &station.url);
		Some((dir, ext, settings))
	}

	/// Starts recording a station on its own background stream,
	/// leaving playback wherever it is. For scheduled recordings:
	/// they never needed the listener's station, only their own.
	/// The recorder is built exactly as a playing-station recording
	/// is, minus the playhead — a background recording is live-edge
	/// only, so Original takes the stream's bytes and song splits
	/// ride the stream's own titles.
	pub fn start_background_recording(&mut self, station: &Station) -> Result<PathBuf> {
		if self.is_recording() {
			return Err(CoreError::Output("Already recording".into()));
		}
		if self.settings.is_none() {
			return Err(CoreError::Output("Recording is unavailable".into()));
		}
		// The playing station's slot, borrowed for the recorder; the
		// detached stream is registered right after and feeds it.
		if self.record_slot.is_none() {
			self.record_slot = Some(Arc::new(Mutex::new(None)));
		}
		self.detached_station = Some(station.clone());
		let (dir, ext, settings) = self
			.recording_plan_for(Some(station.id.clone()))
			.ok_or_else(|| CoreError::Output("Recording is unavailable".into()))?;
		let rec = Recorder::start(
			&dir,
			&station.name,
			&settings.recording,
			self.factory.rate(),
			&ext,
		)?;
		let path = rec.path().to_path_buf();
		let slot = self.record_slot.clone().unwrap();
		*slot.lock() = Some(rec);
		self.record_song = None;
		self.detach_recording_for(Some(station.clone()));
		if self.bg_record.is_none() {
			// The stream could not open: leave nothing half-started.
			let _ = self.stop_recording();
			return Err(CoreError::Output("Recording is unavailable".into()));
		}
		Ok(path)
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
		let (dir, ext, settings) = self.recording_plan_for(self.recording_station_id())?;
		if !settings.recording.split_songs {
			return None;
		}
		let station = self.recording_station()?;
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

	/// Stops the recording, returning its file path. A background
	/// recording's stream closes with it.
	pub fn stop_recording(&mut self) -> Option<Result<PathBuf>> {
		self.record_buffered();
		self.record_song = None;
		self.record_at = None;
		if let Some(bg) = self.bg_record.take() {
			// Clear only the background field: the same station may
			// be playing, with playback taps of its own in the entry.
			let mut entry = self.factory.taps(&bg.station_id);
			entry.bg_record = None;
			if entry.record.is_none()
				&& entry.shift.is_none()
				&& entry.capture.is_none()
				&& entry.ring.is_none()
			{
				self.factory.clear_taps(&bg.station_id);
			} else {
				self.factory.set_taps(&bg.station_id, entry);
			}
			drop(bg);
		}
		self.detached_station = None;
		self.record_detached_paused = false;
		let slot = self.record_slot.clone()?;
		slot.lock().take().map(|rec| rec.stop())
	}

	/// Pauses or resumes the active recording; false when none is running.
	/// A paused recording drops the audio written to it, so the file
	/// holds only the spans the user let through. Pausing a background
	/// recording closes its stream — a paused recorder writes nothing,
	/// and an open connection left feeding nothing is waste; resuming
	/// reopens it (a short gap, as with any reconnect).
	pub fn set_recording_paused(&mut self, paused: bool) -> bool {
		self.record_buffered();
		let Some(slot) = self.record_slot.clone() else {
			return false;
		};
		let mut guard = slot.lock();
		let ok = match guard.as_mut() {
			Some(rec) => {
				rec.set_paused(paused);
				true
			}
			None => false,
		};
		drop(guard);
		if ok && paused && let Some(bg) = self.bg_record.take() {
			let mut entry = self.factory.taps(&bg.station_id);
			entry.bg_record = None;
			self.factory.set_taps(&bg.station_id, entry);
			self.record_detached_paused = true;
			drop(bg);
		}
		if ok && !paused && self.bg_record.is_none() && self.record_detached_paused {
			self.record_detached_paused = false;
			self.detach_recording_for(self.detached_station.clone());
		}
		ok
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
		// A background recording's stream is not in the open list; its
		// events were already routed to the background drain.
		if self.bg_record.as_ref().is_some_and(|bg| bg.serial == serial) {
			return;
		}
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
					// A recording started before this report (a scheduled
					// one) was named from the address: correct it now.
					if let Some(slot) = &self.record_slot
						&& matches!(format.codec.as_str(), "AAC" | "MP3" | "MP2")
						&& let Some(rec) = slot.lock().as_mut()
					{
						let ext = recording_source_ext(Some(&format.codec), "");
						let _ = rec.align_source_ext(&ext);
					}
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

	/// A factory that hands out inert streams and counts them, so a
	/// test can prove a background recording opened its own stream.
	struct CountingFactory {
		started: Arc<std::sync::atomic::AtomicUsize>,
		taps: Arc<Mutex<HashMap<String, Taps>>>,
	}
	impl StreamFactory for CountingFactory {
		fn start(
			&self,
			_: &Station,
			_: Box<dyn Fn(StreamEvent) + Send + Sync>,
		) -> (Arc<PcmRing>, Box<dyn Send>) {
			self.started.fetch_add(1, Ordering::SeqCst);
			(Arc::new(PcmRing::new(1000, 1.0)), Box::new(()))
		}
		fn set_taps(&mut self, station_id: &str, taps: Taps) {
			self.taps.lock().insert(station_id.to_string(), taps);
		}
		fn clear_taps(&mut self, station_id: &str) {
			self.taps.lock().remove(station_id);
		}
		fn taps(&self, station_id: &str) -> Taps {
			self.taps
				.lock()
				.get(station_id)
				.cloned()
				.unwrap_or_default()
		}
		fn rate(&self) -> u32 {
			1000
		}
	}

	fn test_station(id: &str) -> Station {
		Station {
			id: id.into(),
			name: format!("Station {id}"),
			url: format!("http://test/{id}.mp3"),
			source: crate::station::Source::Custom,
			info: None,
			details: Default::default(),
		}
	}

	fn bg_tuner(dir: &std::path::Path) -> (Tuner<CountingFactory, NoSink>, Arc<std::sync::atomic::AtomicUsize>, Arc<Mutex<HashMap<String, Taps>>>) {
		let mut settings = Settings::default();
		settings.recording.dir = Some(dir.to_string_lossy().into_owned());
		settings.recording.format = crate::settings::RecordFormat::Wav;
		let started = Arc::new(std::sync::atomic::AtomicUsize::new(0));
		let taps = Arc::new(Mutex::new(HashMap::new()));
		let factory = CountingFactory {
			started: started.clone(),
			taps: taps.clone(),
		};
		let (tx, _) = crossbeam_channel::unbounded();
		let mut tuner = Tuner::new(factory, NoSink, tx);
		tuner.set_settings(Arc::new(Mutex::new(settings)));
		(tuner, started, taps)
	}

	#[test]
	fn a_recording_detaches_and_keeps_recording_across_station_changes() {
		let dir = tempfile::tempdir().unwrap();
		let (mut tuner, started, taps) = bg_tuner(dir.path());
		let a = test_station("a");
		let b = test_station("b");
		tuner.playing = Some(a.clone());
		tuner.attach_taps(&a);
		tuner.start_recording(None).unwrap();
		// Feed the recorder the way the streams do: through the tap
		// map — the playback field before the change, the background
		// field after it.
		let feed = |taps: &Arc<Mutex<HashMap<String, Taps>>>| {
			let t = taps.lock().get("a").cloned().unwrap_or_default();
			let slot = t.record.as_ref().or(t.bg_record.as_ref()).unwrap();
			{
				let mut guard = slot.lock();
				let rec = guard.as_mut().unwrap();
				rec.write_pcm(&vec![0.25; 2000]).unwrap();
			}
		};
		feed(&taps);

		// Change stations: the recording detaches onto its own stream.
		tuner.tune(&b, Vec::new(), false, Instant::now());
		assert!(tuner.is_recording(), "still recording after the change");
		assert!(tuner.recording_is_background());
		assert_eq!(tuner.recording_station_id().as_deref(), Some("a"));
		assert_eq!(started.load(Ordering::SeqCst), 2, "playback's cold start plus the background stream");
		assert!(taps.lock().get("a").is_some_and(|t| t.bg_record.is_some()), "a's background taps feed the recorder");
		// The background stream feeds the same recorder through a's taps.
		{
			let t = taps.lock().get("a").cloned().unwrap_or_default();
			let slot = t.bg_record.clone().unwrap();
			assert!(
				std::sync::Arc::ptr_eq(&slot, tuner.bg_slot().as_ref().unwrap()),
				"the entry's background slot is the tuner's"
			);
			let mut guard = slot.lock();
			let rec = guard.as_mut().unwrap();
			rec.write_pcm(&vec![0.25; 2000]).unwrap();
		}


		// Another change keeps it; stopping ends it and closes the stream.
		let c = test_station("c");
		tuner.tune(&c, Vec::new(), false, Instant::now());
		assert!(tuner.is_recording());
		let path = tuner.stop_recording().unwrap().unwrap();
		assert!(!tuner.is_recording());
		assert!(taps.lock().get("a").is_none_or(|t| t.bg_record.is_none()), "background taps cleared on stop");
		let reader = hound::WavReader::open(&path).unwrap();
		// hound counts frames per channel: 2000 stereo values = 1000
		// frames per feed, two feeds = 2000 frames.
		assert_eq!(reader.duration(), 2000, "audio from before and after the change");
	}

	#[test]
	fn a_scheduled_recording_starts_in_the_background_without_playing() {
		let dir = tempfile::tempdir().unwrap();
		let (mut tuner, started, _taps) = bg_tuner(dir.path());
		let a = test_station("a");
		let b = test_station("b");
		tuner.playing = Some(b.clone());
		tuner.attach_taps(&b);
		let path = tuner.start_background_recording(&a).unwrap();
		assert!(tuner.is_recording());
		assert!(tuner.recording_is_background());
		assert_eq!(tuner.recording_station_id().as_deref(), Some("a"));
		assert_eq!(tuner.playing.as_ref().map(|p| p.id.as_str()), Some("b"), "playback untouched");
		assert_eq!(started.load(Ordering::SeqCst), 1);
		// Playback stopping does not end it.
		tuner.stop();
		assert!(tuner.is_recording(), "a scheduled recording survives playback stop");
		let done = tuner.stop_recording().unwrap().unwrap();
		assert_eq!(done, path);
	}

	#[test]
	fn a_paused_background_recording_resumes_on_a_fresh_stream() {
		let dir = tempfile::tempdir().unwrap();
		let (mut tuner, started, _taps) = bg_tuner(dir.path());
		let a = test_station("a");
		tuner.start_background_recording(&a).unwrap();
		assert_eq!(started.load(Ordering::SeqCst), 1);
		assert!(tuner.set_recording_paused(true));
		assert!(tuner.recording_is_background(), "paused still counts as background");
		assert!(tuner.set_recording_paused(false));
		assert_eq!(started.load(Ordering::SeqCst), 2, "resume reopened the stream");
		assert!(tuner.is_recording());
	}

	#[test]
	fn a_recording_is_named_for_the_stream_not_its_address() {
		// The reported case: an AAC stream at an address with no suffix.
		assert_eq!(recording_source_ext(Some("AAC"), "https://ice.example/ClassicFMHD?hdauth=x"), "aac");
		assert_eq!(recording_source_ext(Some("MP3"), "https://ice.example/live.aac"), "mp3");
		assert_eq!(recording_source_ext(Some("MP2"), "https://ice.example/live"), "mp2");
		// Not yet decoded: the address names it, query and all.
		assert_eq!(recording_source_ext(None, "https://ice.example/live.aac?token=1"), "aac");
		assert_eq!(recording_source_ext(None, "https://ice.example/ClassicFMHD?hdauth=x"), "mp3");
		assert_eq!(recording_source_ext(Some("Vorbis"), "https://ice.example/live.ogg"), "ogg");
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
