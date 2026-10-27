use std::collections::HashSet;
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender};
use parking_lot::Mutex;

use crate::net;
use crate::output::{MixerCmd, Output};
use crate::ring::PcmRing;
use crate::station::Station;
use crate::stream::{EngineCtx, StreamEvent};

/// Focus calls closer than this are key repeat: new connections wait until the key rests.
const REPEAT: Duration = Duration::from_millis(100);
/// Focus rest before a held title is announced.
const SETTLE: Duration = Duration::from_millis(1500);
/// Warm neighbours close after this long without a focus.
const WARM_IDLE: Duration = Duration::from_secs(25);
/// A warm swap plays this far behind the ring's newest sample.
const SWAP_MS: u32 = 250;
/// DNS is warmed for the stations this close to the focused one.
const DNS_SPAN: usize = 5;

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
}

/// Hostnames already warmed this process; warming is only an OS resolver hint.
static WARMED_HOSTS: LazyLock<Mutex<HashSet<String>>> =
	LazyLock::new(|| Mutex::new(HashSet::new()));

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
			let mut warmed = WARMED_HOSTS.lock();
			for station in stations {
				if let Ok(url) = reqwest::Url::parse(&station.url)
					&& let Some(host) = url.host_str()
					&& warmed.insert(host.to_owned())
				{
					hosts.push(host.to_owned());
				}
			}
		}
		if !hosts.is_empty() {
			drop(self.0.rt.spawn(net::warm_dns(hosts)));
		}
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
}

impl<F: StreamFactory, S: Sink> Tuner<F, S> {
	pub fn new(factory: F, sink: S, events: Sender<TunerEvent>) -> Tuner<F, S> {
		let (stream_tx, stream_rx) = crossbeam_channel::unbounded();
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
		}
	}

	pub fn focus(&mut self, stations: &[Station], index: usize, now: Instant) {
		let Some(target) = stations.get(index) else {
			return;
		};
		let repeat = self
			.last_focus
			.is_some_and(|t| now.duration_since(t) < REPEAT);
		self.last_focus = Some(now);
		let lo = index.saturating_sub(DNS_SPAN);
		let hi = (index + DNS_SPAN + 1).min(stations.len());
		self.factory.warm_dns(&stations[lo..hi]);
		let warm = wanted(stations.len(), index)
			.1
			.into_iter()
			.map(|i| stations[i].clone())
			.collect();
		self.tune(target, warm, repeat, now);
	}

	pub fn play(&mut self, station: &Station, now: Instant) {
		self.tune(station, Vec::new(), false, now);
	}

	pub fn stop(&mut self) {
		self.sink.send(MixerCmd::SetActive {
			ring: None,
			token: 0,
		});
		self.open.clear();
		self.playing = None;
		self.deferred = None;
		self.pending = None;
	}

	pub fn poll(&mut self, now: Instant, rendered: &[(u64, Instant)]) {
		while let Ok((serial, event)) = self.stream_rx.try_recv() {
			self.stream_event(serial, event, now);
		}

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
			let playing = self.playing.as_ref().map(|p| p.id.as_str());
			self.open.retain(|o| Some(o.station.id.as_str()) == playing);
		}

		if let Some(title) = self.settle.tick(now) {
			self.emit(TunerEvent::Announce(title));
		}
	}

	pub fn playing(&self) -> Option<&Station> {
		self.playing.as_ref()
	}

	fn tune(&mut self, target: &Station, warm: Vec<Station>, repeat: bool, now: Instant) {
		self.settle.focused(now);
		self.deferred = None;
		let mut cold = None;
		let already =
			self.playing.as_ref().is_some_and(|p| p.id == target.id) && self.is_open(&target.id);
		if !already {
			match self.open.iter().find(|o| o.station.id == target.id) {
				Some(o) => {
					let (ring, title) = (o.ring.clone(), o.title.clone());
					ring.trim_to_ms(SWAP_MS);
					self.activate(target, ring, now, true);
					self.emit(TunerEvent::Title {
						station_id: target.id.clone(),
						title: title.clone(),
					});
					// Just focused, so this only holds the title for Settle.
					self.settle.title(title, now);
				}
				None => {
					// Silence first: the old station must never keep playing under a new focus.
					self.sink.send(MixerCmd::SetActive {
						ring: None,
						token: 0,
					});
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
			StreamEvent::Reconnecting if playing => {
				self.emit(TunerEvent::Reconnecting { station_id })
			}
			StreamEvent::Reconnected if playing => {
				self.emit(TunerEvent::Reconnected { station_id })
			}
			_ => {}
		}
	}

	fn is_open(&self, id: &str) -> bool {
		self.open.iter().any(|o| o.station.id == id)
	}

	fn emit(&self, event: TunerEvent) {
		let _ = self.events.send(event);
	}
}
