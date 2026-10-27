//! Playback health per station without a device: `cargo run -p blindradio-core --example probe --release [warm_secs]`.
//! Runs each station through the real mixer in 10 ms ticks like the audio callback, and
//! reports when sound starts and how many ticks went silent after that (gaps). With
//! `warm_secs`, each station first sits warm (open, not played) that long, then is swapped to
//! like the tuner does (trimmed to the cushion).

use std::sync::Arc;
use std::time::{Duration, Instant};

use blindradio_core::Engine;
use blindradio_core::output::{MixerCmd, MixerCore};
use blindradio_core::ring::CUSHION_MS;
use blindradio_core::station::Station;
use blindradio_core::stream::start;

const RATE: u32 = 48_000;
const TICK: Duration = Duration::from_millis(10);
const RUN: Duration = Duration::from_secs(20);

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
				let began = Instant::now();
				let stream = start(&station, &ctx, Box::new(|_| {}));
				if let Some(warm) = warm {
					std::thread::sleep(warm);
					stream.ring.trim_to_ms(CUSHION_MS);
				}
				let mut mixer = MixerCore::new(RATE);
				let swapped = Instant::now();
				mixer.handle(MixerCmd::SetActive {
					ring: Some(Arc::clone(&stream.ring)),
					token: 1,
				});
				let mut out = vec![0.0; (RATE as usize * TICK.as_millis() as usize) / 1000 * 2];
				let (mut sound, mut gaps) = (None, 0);
				let mut next = swapped;
				while swapped.elapsed() < RUN {
					next += TICK;
					std::thread::sleep(next.saturating_duration_since(Instant::now()));
					let level = stream.ring.len_ms();
					if mixer.render(&mut out).is_some() {
						sound = Some(swapped.elapsed());
					} else if sound.is_some() && level == 0 {
						// Ran dry: a real gap, not silence in the programme.
						gaps += 1;
					}
				}
				let from = if warm.is_some() { "swap" } else { "start" };
				format!(
					"{}: sound {:?} ms after {from} ({} ms after connect), dry ticks {gaps}",
					station.name,
					sound.map(|d| d.as_millis()),
					sound.map_or(0, |d| (swapped - began + d).as_millis()),
				)
			})
		})
		.collect();
	for t in threads {
		println!("{}", t.join().unwrap());
	}
	Ok(())
}
