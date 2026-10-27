//! Playback health per station without a device: `cargo run -p blindradio-core --example probe --release [warm_secs]`.
//! Drains each ring in 10 ms ticks like an audio callback and reports underruns (ticks that
//! came up short after first audio). With `warm_secs`, each station first sits warm (open,
//! not played) that long, then is swapped to like the tuner does (trim to 250 ms).

use std::time::{Duration, Instant};

use blindradio_core::Engine;
use blindradio_core::station::Station;
use blindradio_core::stream::start;

const RATE: u32 = 48_000;
const TICK: Duration = Duration::from_millis(10);
const RUN: Duration = Duration::from_secs(20);
const SWAP_MS: u32 = 250;

fn main() -> blindradio_core::Result<()> {
	let warm = std::env::args()
		.nth(1)
		.and_then(|s| s.parse().ok())
		.map(Duration::from_secs_f32);
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
				if let Some(warm) = warm {
					std::thread::sleep(warm);
					stream.ring.trim_to_ms(SWAP_MS);
				}
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
					"{}: first {:?}, short ticks {short_ticks}, missing {} ms, min level {} ms",
					station.name,
					started.map(|d| d.as_millis()),
					missing * 1000 / RATE as usize,
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
