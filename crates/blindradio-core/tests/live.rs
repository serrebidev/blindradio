//! Real stations over the network: `cargo test -p blindradio-core --test live -- --ignored`.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use blindradio_core::Engine;
use blindradio_core::decode::Decoder;
use blindradio_core::net::{Opened, open};
use blindradio_core::station::Station;
use blindradio_core::stream::{StationStream, StreamEvent, start};

const FIRST_AUDIO: Duration = Duration::from_secs(3);

fn engine() -> Engine {
	// Never saved, so the path only has to not exist.
	Engine::new(
		&std::env::temp_dir().join("blindradio-no-such-dir/cache.json"),
		48_000,
	)
	.unwrap()
}

fn dev_stations() -> Vec<Station> {
	let json = std::fs::read(concat!(
		env!("CARGO_MANIFEST_DIR"),
		"/examples/stations.json"
	))
	.unwrap();
	serde_json::from_slice(&json).unwrap()
}

/// Starts `station` and waits for its first audio; `Err` holds the reason it did not come.
fn first_audio(engine: &Engine, station: &Station) -> Result<(StationStream, Duration), String> {
	let (tx, rx) = mpsc::channel();
	let started = Instant::now();
	let stream = start(
		station,
		&engine.ctx,
		Box::new(move |event| {
			let _ = tx.send(event);
		}),
	);
	loop {
		let left = FIRST_AUDIO.saturating_sub(started.elapsed());
		match rx.recv_timeout(left) {
			Ok(StreamEvent::FirstAudio) => return Ok((stream, started.elapsed())),
			Ok(StreamEvent::Failed(reason)) => return Err(reason),
			Ok(_) => {}
			Err(_) => return Err("no audio in 3 s".into()),
		}
	}
}

#[test]
#[ignore = "network"]
fn each_dev_station_produces_audio() {
	let engine = engine();
	let mut failed = Vec::new();
	for station in dev_stations() {
		match first_audio(&engine, &station) {
			Ok((_, took)) => println!("{} {} ms", station.name, took.as_millis()),
			Err(reason) => failed.push(format!("{}: {reason}", station.name)),
		}
	}
	assert!(failed.is_empty(), "{failed:#?}");
}

#[test]
#[ignore = "network"]
fn he_aac_station_decodes() {
	let engine = engine();
	let station = dev_stations()
		.into_iter()
		.find(|s| s.id == "foxnews-heaac")
		.unwrap();
	first_audio(&engine, &station).unwrap();
	// SBR doubles the 22.05 kHz core rate; a decoder that ignored it would report 22050.
	let bytes = engine.rt.block_on(async {
		let Opened::Audio { mut response, .. } =
			open(&engine.ctx.client, &station.url).await.unwrap()
		else {
			panic!("expected audio");
		};
		let mut bytes = Vec::new();
		while bytes.len() < 64 * 1024 {
			bytes.extend_from_slice(&response.chunk().await.unwrap().unwrap());
		}
		bytes
	});
	let decoder = Decoder::new(Box::new(std::io::Cursor::new(bytes)), Some("aac")).unwrap();
	assert_eq!(decoder.sample_rate(), 44_100);
}
