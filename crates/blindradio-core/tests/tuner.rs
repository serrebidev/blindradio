use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use blindradio_core::output::MixerCmd;
use blindradio_core::ring::PcmRing;
use blindradio_core::station::Station;
use blindradio_core::stream::StreamEvent;
use blindradio_core::tuner::{Settle, Sink, StreamFactory, Tuner, TunerEvent, wanted};
use crossbeam_channel::Receiver;
use parking_lot::Mutex;

const RATE: u32 = 48_000;

fn ms(n: u64) -> Duration {
	Duration::from_millis(n)
}

fn id(i: usize) -> String {
	format!("s{i}")
}

fn stations(n: usize) -> Vec<Station> {
	(0..n)
		.map(|i| Station {
			id: id(i),
			name: format!("Station {i}"),
			url: format!("http://h{i}.example/listen"),
		})
		.collect()
}

struct Guard {
	shared: Arc<FakeShared>,
}

impl Drop for Guard {
	fn drop(&mut self) {
		self.shared.active.fetch_sub(1, Ordering::SeqCst);
	}
}

type Callback = Box<dyn Fn(StreamEvent) + Send + Sync>;

#[derive(Default)]
struct FakeShared {
	starts: Mutex<Vec<String>>,
	rings: Mutex<HashMap<String, Arc<PcmRing>>>,
	callbacks: Mutex<HashMap<String, Vec<Callback>>>,
	dns: Mutex<Vec<Vec<String>>>,
	active: AtomicUsize,
	max_active: AtomicUsize,
}

#[derive(Clone, Default)]
struct FakeFactory {
	shared: Arc<FakeShared>,
}

impl FakeFactory {
	fn starts(&self) -> Vec<String> {
		self.shared.starts.lock().clone()
	}

	fn started(&self, i: usize) -> bool {
		self.shared.starts.lock().iter().any(|s| s == &id(i))
	}

	fn start_count(&self, i: usize) -> usize {
		self.shared
			.starts
			.lock()
			.iter()
			.filter(|s| **s == id(i))
			.count()
	}

	fn active(&self) -> usize {
		self.shared.active.load(Ordering::SeqCst)
	}

	fn max_active(&self) -> usize {
		self.shared.max_active.load(Ordering::SeqCst)
	}

	fn ring(&self, i: usize) -> Arc<PcmRing> {
		self.shared.rings.lock()[&id(i)].clone()
	}

	/// Delivers an event as the stream task would, possibly after the stream was dropped.
	fn emit(&self, i: usize, event: StreamEvent) {
		let callbacks = self.shared.callbacks.lock();
		let cb = callbacks
			.get(&id(i))
			.and_then(|v| v.last())
			.unwrap_or_else(|| panic!("no stream started for {}", id(i)));
		cb(event);
	}

	/// Delivers an event through the first stream ever started for station `i`.
	fn emit_first(&self, i: usize, event: StreamEvent) {
		let callbacks = self.shared.callbacks.lock();
		callbacks[&id(i)][0](event);
	}

	fn dns_calls(&self) -> Vec<Vec<String>> {
		self.shared.dns.lock().clone()
	}
}

impl StreamFactory for FakeFactory {
	fn start(
		&self,
		station: &Station,
		on_event: Box<dyn Fn(StreamEvent) + Send + Sync>,
	) -> (Arc<PcmRing>, Box<dyn Send>) {
		let now = self.shared.active.fetch_add(1, Ordering::SeqCst) + 1;
		self.shared.max_active.fetch_max(now, Ordering::SeqCst);
		self.shared.starts.lock().push(station.id.clone());
		self.shared
			.callbacks
			.lock()
			.entry(station.id.clone())
			.or_default()
			.push(on_event);
		let ring = Arc::new(PcmRing::new(RATE, 3.0));
		self.shared
			.rings
			.lock()
			.insert(station.id.clone(), ring.clone());
		(
			ring,
			Box::new(Guard {
				shared: self.shared.clone(),
			}),
		)
	}

	fn warm_dns(&self, stations: &[Station]) {
		self.shared
			.dns
			.lock()
			.push(stations.iter().map(|s| s.id.clone()).collect());
	}
}

#[derive(Clone)]
enum RecCmd {
	SetActive {
		ring: Option<Arc<PcmRing>>,
		token: u64,
	},
	Volume(f32),
	Mute(bool),
	Tune,
}

impl std::fmt::Debug for RecCmd {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			RecCmd::SetActive { ring, token } => f
				.debug_struct("SetActive")
				.field("ring", &ring.as_ref().map(|_| "PcmRing"))
				.field("token", token)
				.finish(),
			RecCmd::Volume(v) => f.debug_tuple("Volume").field(v).finish(),
			RecCmd::Mute(m) => f.debug_tuple("Mute").field(m).finish(),
			RecCmd::Tune => f.write_str("Tune"),
		}
	}
}

#[derive(Clone, Default)]
struct RecSink {
	cmds: Arc<Mutex<Vec<RecCmd>>>,
}

impl Sink for RecSink {
	fn send(&self, cmd: MixerCmd) {
		let rec = match cmd {
			MixerCmd::SetActive { ring, token } => RecCmd::SetActive { ring, token },
			MixerCmd::Volume(v) => RecCmd::Volume(v),
			MixerCmd::Mute(m) => RecCmd::Mute(m),
			MixerCmd::Tune => RecCmd::Tune,
		};
		self.cmds.lock().push(rec);
	}
}

impl RecSink {
	fn cmds(&self) -> Vec<RecCmd> {
		self.cmds.lock().clone()
	}

	/// Ring and token of the most recent `SetActive` carrying a ring.
	fn last_active(&self) -> (Arc<PcmRing>, u64) {
		self.cmds
			.lock()
			.iter()
			.rev()
			.find_map(|c| match c {
				RecCmd::SetActive {
					ring: Some(r),
					token,
				} => Some((r.clone(), *token)),
				_ => None,
			})
			.expect("no station was activated")
	}
}

struct Fixture {
	tuner: Tuner<FakeFactory, RecSink>,
	fake: FakeFactory,
	sink: RecSink,
	events: Receiver<TunerEvent>,
	t0: Instant,
}

fn fixture() -> Fixture {
	let fake = FakeFactory::default();
	let sink = RecSink::default();
	let (tx, events) = crossbeam_channel::unbounded();
	let tuner = Tuner::new(fake.clone(), sink.clone(), tx);
	Fixture {
		tuner,
		fake,
		sink,
		events,
		t0: Instant::now(),
	}
}

impl Fixture {
	fn drain_events(&self) -> Vec<TunerEvent> {
		self.events.try_iter().collect()
	}
}

#[test]
fn wanted_edges() {
	assert_eq!(wanted(1, 0), (0, vec![]));
	assert_eq!(wanted(5, 0), (0, vec![1]));
	assert_eq!(wanted(5, 4), (4, vec![3]));
	assert_eq!(wanted(5, 2), (2, vec![1, 3]));
	assert_eq!(wanted(200, 100), (100, vec![99, 101]));
}

#[test]
fn focus_plays_target_and_warms_neighbours() {
	let mut f = fixture();
	let list = stations(11);
	f.tuner.focus(&list, 4, f.t0);
	let mut starts = f.fake.starts();
	starts.sort();
	assert_eq!(starts, ["s3", "s4", "s5"]);
	let (ring, _) = f.sink.last_active();
	assert!(
		Arc::ptr_eq(&ring, &f.fake.ring(4)),
		"active ring is station 4's"
	);
	// Every focus warms DNS for the stations within 5 of the index.
	assert_eq!(
		f.fake.dns_calls(),
		vec![(0..=9).map(id).collect::<Vec<_>>()]
	);
}

#[test]
fn moving_to_warm_neighbour_swaps_without_new_connect_for_it() {
	let mut f = fixture();
	let list = stations(11);
	f.tuner.focus(&list, 4, f.t0);
	f.fake.ring(5).push(&vec![0.5; RATE as usize * 4]);
	f.tuner.focus(&list, 5, f.t0 + ms(1000));
	assert_eq!(f.fake.start_count(5), 1, "no new connect for warm 5");
	assert!(f.fake.started(6), "new neighbour 6 warms");
	assert_eq!(f.fake.active(), 3, "4, 5, 6 stay open; 3 dropped");
	assert_eq!(
		f.fake.ring(5).len_ms(),
		1000,
		"warm swap trims to the 1 s cushion"
	);
	let (ring, _) = f.sink.last_active();
	assert!(
		Arc::ptr_eq(&ring, &f.fake.ring(5)),
		"active ring is station 5's"
	);
	// An event from the dropped station 3 arrives late and is ignored.
	f.drain_events();
	f.fake.emit(3, StreamEvent::Title(Some("late".into())));
	f.tuner.poll(f.t0 + ms(1010), &[]);
	assert!(
		f.drain_events().is_empty(),
		"late events from dropped streams are ignored"
	);
}

#[test]
fn cold_focus_leaves_old_station_for_hiss_first() {
	let mut f = fixture();
	let list = stations(11);
	f.tuner.focus(&list, 0, f.t0);
	let before = f.sink.cmds().len();
	f.tuner.focus(&list, 9, f.t0 + ms(1000));
	let cmds = f.sink.cmds();
	match &cmds[before] {
		RecCmd::Tune => {}
		other => panic!("first command after a cold focus: {other:?}"),
	}
}

#[test]
fn tuner_key_repeat_ends_on_last_station() {
	let mut f = fixture();
	let list = stations(200);
	for i in 0..200 {
		f.tuner.focus(&list, i, f.t0 + ms(i as u64 * 33));
		assert!(
			f.fake.max_active() <= 3,
			"open streams after focus {i}: {}",
			f.fake.max_active()
		);
		if i == 1 {
			let (ring, _) = f.sink.last_active();
			assert!(
				Arc::ptr_eq(&ring, &f.fake.ring(1)),
				"warm swaps are never deferred"
			);
		}
	}
	assert!(
		!f.fake.started(199),
		"the last station is not started while keys repeat"
	);
	f.tuner.poll(f.t0 + ms(199 * 33 + 150), &[]);
	assert!(f.fake.max_active() <= 3);
	let (ring, _) = f.sink.last_active();
	assert!(
		Arc::ptr_eq(&ring, &f.fake.ring(199)),
		"active ring is station 199's"
	);
	assert_eq!(f.tuner.playing().map(|s| s.id.as_str()), Some("s199"));
}

#[test]
fn stop_then_focus_plays_again() {
	let mut f = fixture();
	let list = stations(11);
	f.tuner.focus(&list, 4, f.t0);
	assert_eq!(f.fake.active(), 3);
	f.tuner.stop();
	assert_eq!(f.fake.active(), 0, "stop closes every stream");
	assert!(f.tuner.playing().is_none());
	match f.sink.cmds().last() {
		Some(RecCmd::SetActive { ring: None, .. }) => {}
		other => panic!("stop silences the mixer: {other:?}"),
	}
	f.tuner.focus(&list, 4, f.t0 + ms(2000));
	assert_eq!(f.fake.start_count(4), 2, "focus after stop starts again");
	let (ring, _) = f.sink.last_active();
	assert!(Arc::ptr_eq(&ring, &f.fake.ring(4)));
	assert_eq!(f.tuner.playing().map(|s| s.id.as_str()), Some("s4"));
	// Same for play.
	f.tuner.stop();
	f.tuner.play(&list[7], f.t0 + ms(4000));
	assert_eq!(f.fake.active(), 1, "play has no neighbours");
	let (ring, _) = f.sink.last_active();
	assert!(Arc::ptr_eq(&ring, &f.fake.ring(7)));
	assert_eq!(f.tuner.playing().map(|s| s.id.as_str()), Some("s7"));
}

#[test]
fn warm_streams_close_after_25_s_idle() {
	let mut f = fixture();
	let list = stations(7);
	f.tuner.focus(&list, 4, f.t0);
	f.tuner.focus(&list, 5, f.t0 + ms(10_000));
	assert_eq!(f.fake.active(), 3);
	// 24 s since the last focus: nothing closes.
	f.tuner.poll(f.t0 + ms(34_000), &[]);
	assert_eq!(f.fake.active(), 3, "the focus 10 s in reset the idle clock");
	// 26 s since the last focus: warm neighbours close, the playing one stays.
	f.tuner.poll(f.t0 + ms(36_000), &[]);
	assert_eq!(f.fake.active(), 1, "only the playing station stays open");
}

#[test]
fn latency_event_marks_warm_swaps() {
	let mut f = fixture();
	let list = stations(7);
	f.tuner.focus(&list, 4, f.t0);
	let (_, cold_token) = f.sink.last_active();
	f.tuner.poll(f.t0 + ms(50), &[(cold_token, f.t0 + ms(400))]);
	let got = f.drain_events();
	assert!(
		got.contains(&TunerEvent::Latency {
			station_id: "s4".into(),
			ms: 400,
			warm: false,
		}),
		"{got:?}"
	);
	assert!(got.contains(&TunerEvent::Playing {
		station_id: "s4".into()
	}));
	f.tuner.focus(&list, 5, f.t0 + ms(1000));
	let (_, warm_token) = f.sink.last_active();
	f.tuner
		.poll(f.t0 + ms(1010), &[(warm_token, f.t0 + ms(1030))]);
	let got = f.drain_events();
	assert!(
		got.contains(&TunerEvent::Latency {
			station_id: "s5".into(),
			ms: 30,
			warm: true,
		}),
		"{got:?}"
	);
	assert!(got.contains(&TunerEvent::Playing {
		station_id: "s5".into()
	}));
}

#[test]
fn settle_holds_title_until_rested() {
	let t0 = Instant::now();
	let mut s = Settle::new();
	s.focused(t0);
	assert_eq!(s.title(Some("a".into()), t0 + ms(500)), None);
	assert_eq!(s.tick(t0 + ms(1000)), None);
	assert_eq!(s.tick(t0 + ms(1500)), Some("a".to_owned()));
	assert_eq!(
		s.title(Some("b".into()), t0 + ms(3000)),
		Some("b".to_owned())
	);
	s.focused(t0 + ms(4000));
	assert_eq!(s.title(Some("c".into()), t0 + ms(4100)), None);
	assert_eq!(s.tick(t0 + ms(5499)), None);
	assert_eq!(s.tick(t0 + ms(5500)), Some("c".to_owned()));
	assert_eq!(
		s.title(None, t0 + ms(6000)),
		None,
		"empty titles never announce"
	);
}

#[test]
fn events_from_warm_stations_not_forwarded() {
	let mut f = fixture();
	let list = stations(7);
	f.tuner.focus(&list, 4, f.t0);
	f.drain_events();
	f.tuner.poll(f.t0 + ms(10), &[]);
	f.fake.emit(5, StreamEvent::Reconnecting);
	f.fake.emit(5, StreamEvent::Title(Some("W".into())));
	f.fake.emit(3, StreamEvent::Reconnecting);
	f.fake.emit(3, StreamEvent::Reconnected);
	f.fake.emit(3, StreamEvent::Failed("x".into()));
	f.tuner.poll(f.t0 + ms(20), &[]);
	assert!(
		f.drain_events().is_empty(),
		"warm stations never forward events, but their titles are kept"
	);
	assert_eq!(f.fake.active(), 2, "the failed warm stream is closed");
	// The playing station's events pass through.
	f.fake.emit(4, StreamEvent::Title(Some("P".into())));
	f.tuner.poll(f.t0 + ms(30), &[]);
	let got = f.drain_events();
	assert!(
		got.contains(&TunerEvent::Title {
			station_id: "s4".into(),
			title: Some("P".into()),
		}),
		"{got:?}"
	);
	// Swapping to the warm neighbour emits its kept title at once.
	f.tuner.focus(&list, 5, f.t0 + ms(1000));
	let got = f.drain_events();
	assert!(
		got.contains(&TunerEvent::Title {
			station_id: "s5".into(),
			title: Some("W".into()),
		}),
		"{got:?}"
	);
	// Station 5 said Reconnecting while warm; the swap passes it on.
	assert!(
		got.contains(&TunerEvent::Reconnecting {
			station_id: "s5".into()
		}),
		"{got:?}"
	);
}

#[test]
fn failed_uses_station_name() {
	let mut f = fixture();
	let list = stations(7);
	f.tuner.focus(&list, 4, f.t0);
	f.fake.emit(4, StreamEvent::Failed("No data".into()));
	f.tuner.poll(f.t0 + ms(10), &[]);
	let got = f.drain_events();
	assert!(
		got.contains(&TunerEvent::Failed {
			station_id: "s4".into(),
			name: "Station 4".into(),
			reason: "No data".into(),
		}),
		"{got:?}"
	);
}

#[test]
fn title_announced_after_focus_rests() {
	let mut f = fixture();
	let list = stations(7);
	f.tuner.focus(&list, 4, f.t0);
	f.fake.emit(4, StreamEvent::Title(Some("Song A".into())));
	f.tuner.poll(f.t0 + ms(10), &[]);
	let got = f.drain_events();
	assert!(got.contains(&TunerEvent::Title {
		station_id: "s4".into(),
		title: Some("Song A".into()),
	}));
	assert!(!got.iter().any(|e| matches!(e, TunerEvent::Announce(_))));
	f.tuner.poll(f.t0 + ms(1500), &[]);
	let got = f.drain_events();
	assert!(
		got.contains(&TunerEvent::Announce("Song A".into())),
		"{got:?}"
	);
	// Once rested, each later title announces at once.
	f.fake.emit(4, StreamEvent::Title(Some("Song B".into())));
	f.tuner.poll(f.t0 + ms(1600), &[]);
	let got = f.drain_events();
	assert!(
		got.contains(&TunerEvent::Announce("Song B".into())),
		"{got:?}"
	);
}

#[test]
fn late_events_from_a_reopened_station_are_ignored() {
	let mut f = fixture();
	let list = stations(11);
	f.tuner.focus(&list, 4, f.t0);
	f.tuner.focus(&list, 9, f.t0 + ms(1000));
	f.tuner.focus(&list, 4, f.t0 + ms(2000));
	assert_eq!(f.fake.start_count(4), 2, "station 4 reopened");
	f.drain_events();
	// The first, dropped stream of station 4 speaks after the second one opened.
	f.fake.emit_first(4, StreamEvent::Title(Some("old".into())));
	f.tuner.poll(f.t0 + ms(2010), &[]);
	assert!(
		f.drain_events().is_empty(),
		"matched by stream, not by station"
	);
}

#[test]
fn deferred_cold_start_latency_counts_from_focus() {
	let mut f = fixture();
	let list = stations(11);
	f.tuner.focus(&list, 0, f.t0);
	f.tuner.focus(&list, 5, f.t0 + ms(50));
	assert!(!f.fake.started(5), "deferred under key repeat");
	f.tuner.poll(f.t0 + ms(200), &[]);
	let (ring, token) = f.sink.last_active();
	assert!(Arc::ptr_eq(&ring, &f.fake.ring(5)));
	f.tuner.poll(f.t0 + ms(210), &[(token, f.t0 + ms(450))]);
	let got = f.drain_events();
	assert!(
		got.contains(&TunerEvent::Latency {
			station_id: "s5".into(),
			ms: 400,
			warm: false,
		}),
		"{got:?}"
	);
}
