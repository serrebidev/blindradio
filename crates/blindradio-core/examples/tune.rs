//! Latency harness: `cargo run -p blindradio-core --example tune --release`.
//! Cold-plays each station, walks the dial 0 to N and back every 3 s, then steps fast (100 ms).

use std::collections::HashMap;
use std::time::{Duration, Instant};

use blindradio_core::Engine;
use blindradio_core::output::Output;
use blindradio_core::station::Station;
use blindradio_core::tuner::{LiveFactory, Tuner, TunerEvent};
use crossbeam_channel::Receiver;

const DWELL: Duration = Duration::from_secs(3);
const FAST_STEP: Duration = Duration::from_millis(100);

type LiveTuner = Tuner<LiveFactory, Output>;

struct Harness {
	tuner: LiveTuner,
	rendered: Receiver<(u64, Instant)>,
	events: Receiver<TunerEvent>,
	names: HashMap<String, String>,
	warm: Vec<u32>,
	cold: Vec<u32>,
}

impl Harness {
	/// Polls the tuner for `d`, printing latency and failures as they come.
	fn run_for(&mut self, d: Duration) {
		let end = Instant::now() + d;
		while Instant::now() < end {
			let rendered: Vec<_> = self.rendered.try_iter().collect();
			self.tuner.poll(Instant::now(), &rendered);
			for event in self.events.try_iter() {
				match event {
					TunerEvent::Latency {
						station_id,
						ms,
						warm,
					} => {
						println!("{} warm={warm} {ms} ms", self.names[&station_id]);
						if warm { &mut self.warm } else { &mut self.cold }.push(ms);
					}
					TunerEvent::Failed { name, reason, .. } => {
						println!("Failed: {name} ({reason})")
					}
					_ => {}
				}
			}
			std::thread::sleep(Duration::from_millis(5));
		}
	}
}

fn median(v: &mut [u32]) -> Option<u32> {
	v.sort_unstable();
	let mid = v.len() / 2;
	match v.len() {
		0 => None,
		n if n % 2 == 1 => Some(v[mid]),
		_ => Some((v[mid - 1] + v[mid]) / 2),
	}
}

fn main() -> blindradio_core::Result<()> {
	let json = std::fs::read(concat!(
		env!("CARGO_MANIFEST_DIR"),
		"/examples/stations.json"
	))?;
	let list: Vec<Station> = serde_json::from_slice(&json).map_err(std::io::Error::from)?;
	if list.is_empty() {
		return Err(std::io::Error::other("stations.json is empty").into());
	}
	let output = Output::start_default()?;
	let engine = Engine::new(
		&std::env::temp_dir().join("blindradio-tune/cache.json"),
		output.sample_rate,
	)?;
	let rendered = output.rendered().clone();
	let (tx, events) = crossbeam_channel::unbounded();
	let mut h = Harness {
		tuner: Tuner::new(LiveFactory(engine.ctx.clone()), output, tx),
		rendered,
		events,
		names: list
			.iter()
			.map(|s| (s.id.clone(), s.name.clone()))
			.collect(),
		warm: Vec::new(),
		cold: Vec::new(),
	};

	println!("-- cold");
	for station in &list {
		h.tuner.stop();
		h.tuner.play(station, Instant::now());
		h.run_for(DWELL);
	}
	h.tuner.stop();

	println!("-- walk");
	let n = list.len();
	for i in (0..n).chain((0..n - 1).rev()) {
		h.tuner.focus(&list, i, Instant::now());
		h.run_for(DWELL);
	}

	println!("-- fast");
	for i in 1..n {
		h.tuner.focus(&list, i, Instant::now());
		h.run_for(FAST_STEP);
	}
	h.run_for(DWELL);

	let fmt = |m: Option<u32>| m.map_or("none".to_owned(), |m| format!("{m} ms"));
	println!(
		"median warm {} ({} swaps)",
		fmt(median(&mut h.warm)),
		h.warm.len()
	);
	println!(
		"median cold {} ({} starts)",
		fmt(median(&mut h.cold)),
		h.cold.len()
	);
	Ok(())
}
