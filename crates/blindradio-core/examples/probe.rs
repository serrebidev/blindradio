//! Playback health per station without a device: `cargo run -p blindradio-core --example probe --release`.
//! Drains each ring in 10 ms ticks like an audio callback, from the cold start, and reports
//! underruns (ticks that came up short after first audio) and frames lost to ring overflow.

use std::time::{Duration, Instant};

use blindradio_core::Engine;
use blindradio_core::station::Station;
use blindradio_core::stream::start;

const RATE: u32 = 48_000;
const TICK: Duration = Duration::from_millis(10);
const RUN: Duration = Duration::from_secs(20);

fn main() -> blindradio_core::Result<()> {
	let json = std::fs::read(concat!(
		env!("CARGO_MANIFEST_DIR"),
		"/examples/stations.json"
	))?;
	let list: Vec<Station> = serde_json::from_slice(&json).map_err(std::io::Error::from)?;
	let engine = Engine::new(
		&std::env::temp_dir().join("blindradio-probe/cache.json"),
		RATE,
	)?;
	let threads: Vec<_> = list
		.into_iter()
		.map(|station| {
			let ctx = engine.ctx.clone();
			std::thread::spawn(move || {
				let stream = start(&station, &ctx, Box::new(|_| {}));
				let frames = (RATE as usize * TICK.as_millis() as usize) / 1000;
				let mut buf = vec![0.0; frames * 2];
				let (mut started, mut short_ticks, mut missing, mut min_ms) =
					(None, 0, 0, u32::MAX);
				let began = Instant::now();
				let mut next = began;
				while began.elapsed() < RUN {
					next += TICK;
					std::thread::sleep(next.saturating_duration_since(Instant::now()));
					let level = stream.ring.len_ms();
					let got = stream.ring.pop_into(&mut buf) / 2;
					if started.is_none() && got > 0 {
						started = Some(began.elapsed());
					} else if started.is_some() {
						min_ms = min_ms.min(level);
						if got < frames {
							short_ticks += 1;
							missing += frames - got;
						}
					}
				}
				format!(
					"{}: first {:?}, short ticks {short_ticks}, missing {} ms, overflow {} ms, min level {} ms",
					station.name,
					started.map(|d| d.as_millis()),
					missing * 1000 / RATE as usize,
					stream.ring.dropped_frames() * 1000 / u64::from(RATE),
					if min_ms == u32::MAX { 0 } else { min_ms },
				)
			})
		})
		.collect();
	for t in threads {
		println!("{}", t.join().unwrap());
	}
	Ok(())
}
