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
	// Saved on drop; a scratch path keeps the app's real cache out of it.
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

/// WNYC bursts about 15 s on connect. Five seconds in, the jump to the buffer's start is five
/// seconds back: the burst is not behind live. Fails if the server stops bursting.
#[test]
#[ignore = "network"]
fn a_connect_burst_is_not_behind_live() {
	use blindradio_core::output::{MixerCmd, MixerCore};
	use blindradio_core::stream::Taps;
	use blindradio_core::timeshift::{ShiftBuffer, ShiftStatus};
	use std::sync::Arc;

	const RATE: u32 = 48_000;
	let engine = engine();
	let mut station = dev_stations().remove(0);
	station.id = "wnyc-burst".into();
	station.url = "http://fm939.wnyc.org/wnycfm".into();
	let buffer = Arc::new(ShiftBuffer::new(RATE, 1));
	let status = Arc::new(ShiftStatus::new());
	engine.ctx.taps.lock().insert(
		station.id.clone(),
		Taps {
			shift: Some(buffer.clone()),
			..Taps::default()
		},
	);
	let (stream, _) = first_audio(&engine, &station).unwrap();
	let mut mixer = MixerCore::new(RATE);
	mixer.handle(MixerCmd::SetActive {
		ring: Some(stream.ring.clone()),
		token: 1,
	});
	mixer.handle(MixerCmd::ShiftAttach {
		buffer: buffer.clone(),
		status: status.clone(),
	});
	// Real time, in 10 ms passes like the audio callback.
	let mut out = vec![0.0; RATE as usize / 100 * 2];
	let mut tick = |mixer: &mut MixerCore, passes: u32| {
		for _ in 0..passes {
			mixer.render(&mut out);
			std::thread::sleep(Duration::from_millis(10));
		}
	};
	let heard_from = Instant::now();
	while stream.ring.len_ms() == 0 && heard_from.elapsed() < FIRST_AUDIO {
		tick(&mut mixer, 1);
	}
	let played_from = Instant::now();
	tick(&mut mixer, 500);
	let played = played_from.elapsed().as_secs_f32();
	let received = buffer.buffered_seconds();
	let depth = status.depth_seconds(&buffer);
	println!("played {played:.1} s, received {received:.1} s, depth {depth:.1} s");
	assert!(received > played + 5.0, "the server no longer bursts: {received}");
	assert!((depth - played).abs() < 1.5, "depth {depth}, played {played}");
	mixer.handle(MixerCmd::ShiftSeek(i32::MIN));
	tick(&mut mixer, 5);
	let behind = status.behind_seconds(&buffer);
	println!("after the jump to the start: {behind:.1} s behind live");
	assert!((behind - played).abs() < 1.5, "behind {behind}, played {played}");
}

#[test]
#[ignore = "network"]
fn gzipped_hls_station_plays() {
	// RTP's CDN gzips this chunklist under any Accept-Encoding; it failed at once before gzip.
	let engine = engine();
	let station = dev_stations()
		.into_iter()
		.find(|s| s.id == "rtp-antena1-hls")
		.unwrap();
	first_audio(&engine, &station).unwrap();
}

#[test]
#[ignore = "network"]
fn hls_station_with_no_declared_bitrate_gets_a_measured_one() {
	let engine = engine();
	let station = dev_stations()
		.into_iter()
		.find(|s| s.id == "rtp-antena1-hls")
		.unwrap();
	let (tx, rx) = mpsc::channel();
	let _stream = start(
		&station,
		&engine.ctx,
		Box::new(move |event| {
			let _ = tx.send(event);
		}),
	);
	loop {
		match rx.recv_timeout(Duration::from_secs(20)).expect("a bitrate within 20 s") {
			StreamEvent::Format(format) if format.bitrate_kbps.is_some() => {
				println!("{}", format.describe());
				assert!((16..=512).contains(&format.bitrate_kbps.unwrap()));
				return;
			}
			StreamEvent::Failed(reason) => panic!("{reason}"),
			_ => {}
		}
	}
}

#[tokio::test]
#[ignore = "network"]
async fn detects_the_streams_of_real_pages() {
	let client = blindradio_core::net::client();
	let page = "https://somafm.com/groovesalad/";
	let found = blindradio_core::page::detect(&client, page).await.unwrap();
	for st in &found {
		println!("{} | {:?} | {}", st.name, st.info, st.url);
	}
	assert!(!found.is_empty(), "{page}");
}

#[test]
#[ignore = "network"]
fn opus_station_says_what_is_playing() {
	let engine = engine();
	let station = dev_stations()
		.into_iter()
		.find(|s| s.id == "serrebiradio-hd-opus")
		.unwrap();
	let (tx, rx) = mpsc::channel();
	let started = Instant::now();
	let _stream = start(
		&station,
		&engine.ctx,
		Box::new(move |event| {
			let _ = tx.send(event);
		}),
	);
	loop {
		let left = Duration::from_secs(10).saturating_sub(started.elapsed());
		match rx.recv_timeout(left).expect("a title within 10 s") {
			StreamEvent::Title(Some(title)) => {
				println!("{title} after {} ms", started.elapsed().as_millis());
				assert!(title.contains(" - "), "artist and title: {title}");
				return;
			}
			StreamEvent::Failed(reason) => panic!("{reason}"),
			_ => {}
		}
	}
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

#[test]
#[ignore = "network"]
fn tunein_station_plays() {
	use blindradio_core::sources::Node;
	use blindradio_core::sources::tunein::TuneIn;
	let engine = engine();
	let t = TuneIn::new("https://opml.radiotime.com");
	// Drill from the location root, first folder at each level, until stations show up.
	let station = engine.rt.block_on(async {
		let mut id = "tunein".to_owned();
		for _ in 0..6 {
			let nodes = t.children(&engine.ctx.client, &id).await.unwrap();
			if let Some(Node::Station(s)) = nodes.iter().find(|n| matches!(n, Node::Station(_))) {
				return s.clone();
			}
			id = nodes
				.into_iter()
				.find_map(|n| match n {
					Node::Folder { id, .. } => Some(id),
					_ => None,
				})
				.expect("a folder or station");
		}
		panic!("no station found");
	});
	first_audio(&engine, &station).unwrap();
}

#[tokio::test]
#[ignore = "network"]
async fn live_search_jazz() {
	use blindradio_core::sources::{Endpoints, SearchQuery, Sources};
	use std::collections::HashSet;
	let sources = Sources::new(blindradio_core::net::client(), Endpoints::default());
	let found = sources
		.search(&SearchQuery {
			name: "jazz".into(),
			..Default::default()
		})
		.await;
	let dirs: HashSet<_> = found.iter().map(|s| s.source.label().to_owned()).collect();
	assert!(found.len() >= 20, "{} results", found.len());
	assert!(dirs.len() >= 3, "{dirs:?}");
}

#[tokio::test]
#[ignore = "network"]
async fn live_browse_each_root() {
	use blindradio_core::sources::{Endpoints, Node, Sources};
	let sources = Sources::new(blindradio_core::net::client(), Endpoints::default());
	for root in Sources::roots() {
		let Node::Folder { id, name } = root else {
			panic!("root is not a folder")
		};
		let kids = sources
			.children(&id, blindradio_core::sources::Sort::default())
			.await
			.unwrap_or_else(|e| panic!("{name}: {e}"));
		assert!(!kids.is_empty(), "{name} is empty");
	}
}
