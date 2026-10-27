mod common;

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, mpsc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use blindradio_core::net::{ResolveCache, client};
use blindradio_core::ring::PcmRing;
use blindradio_core::station::{Source, Station};
use blindradio_core::stream::{EngineCtx, StationStream, StreamEvent, start};
use common::{Route, serve};

const OUT_RATE: u32 = 48000;

static RT: LazyLock<tokio::runtime::Runtime> = LazyLock::new(|| {
	tokio::runtime::Builder::new_multi_thread()
		.enable_all()
		.build()
		.unwrap()
});

fn ctx(retry_window: Duration) -> EngineCtx {
	// Never saved, so the path only has to not exist.
	let cache = ResolveCache::load(&std::env::temp_dir().join("blindradio-no-such-dir/cache.json"));
	EngineCtx {
		rt: RT.handle().clone(),
		client: client(),
		cache: Arc::new(parking_lot::Mutex::new(cache)),
		out_rate: OUT_RATE,
		retry_window,
	}
}

fn tone() -> Vec<u8> {
	std::fs::read(concat!(
		env!("CARGO_MANIFEST_DIR"),
		"/tests/fixtures/tone.mp3"
	))
	.unwrap()
}

/// tone.mp3 looped at 128 kbps after a one-second burst.
fn live(metaint: Option<usize>, titles: Vec<Vec<u8>>, close_after: Option<usize>) -> Route {
	Route::Stream {
		path: "/live".into(),
		content_type: "audio/mpeg".into(),
		data: tone(),
		metaint,
		titles,
		burst: 16_000,
		bytes_per_sec: 16_000,
		close_after,
	}
}

fn play(url: &str, ctx: &EngineCtx) -> (StationStream, mpsc::Receiver<StreamEvent>) {
	let (tx, rx) = mpsc::channel();
	let station = Station {
		id: "test".into(),
		name: "Test".into(),
		url: url.into(),
		source: Source::Custom,
		info: None,
	};
	let s = start(&station, ctx, Box::new(move |e| drop(tx.send(e))));
	(s, rx)
}

/// Collects events until `want` matches or `within` passes; returns everything seen.
fn events_until(
	rx: &mpsc::Receiver<StreamEvent>,
	within: Duration,
	want: impl Fn(&StreamEvent) -> bool,
) -> Vec<StreamEvent> {
	let end = Instant::now() + within;
	let mut seen = Vec::new();
	while let Some(left) = end.checked_duration_since(Instant::now()) {
		match rx.recv_timeout(left) {
			Ok(e) => {
				let done = want(&e);
				seen.push(e);
				if done {
					return seen;
				}
			}
			Err(_) => break,
		}
	}
	panic!("timed out after {within:?}; events: {seen:?}");
}

/// Plays the ring out in real time like the audio device would; returns the longest underrun.
fn consume(ring: Arc<PcmRing>, stop: Arc<AtomicBool>) -> JoinHandle<Duration> {
	std::thread::spawn(move || {
		let mut last = Instant::now();
		let mut dry_since: Option<Instant> = None;
		let mut longest = Duration::ZERO;
		let mut buf = Vec::new();
		while !stop.load(Ordering::Relaxed) {
			std::thread::sleep(Duration::from_millis(10));
			let now = Instant::now();
			let frames = ((now - last).as_secs_f64() * OUT_RATE as f64) as usize;
			last = now;
			buf.resize(frames * 2, 0.0);
			if ring.pop_into(&mut buf) < buf.len() {
				let since = *dry_since.get_or_insert(now);
				longest = longest.max(now - since);
			} else {
				dry_since = None;
			}
		}
		longest
	})
}

#[test]
fn stream_fills_ring_and_reports_first_audio() {
	let server = serve(vec![live(None, vec![], None)]);
	let (s, rx) = play(&server.url("/live"), &ctx(Duration::from_secs(60)));
	events_until(&rx, Duration::from_secs(1), |e| {
		*e == StreamEvent::FirstAudio
	});
	assert!(s.ring.len_ms() > 0);

	let ring = s.ring.clone();
	drop(s);
	std::thread::sleep(Duration::from_millis(300));
	ring.clear();
	std::thread::sleep(Duration::from_millis(500));
	assert_eq!(ring.len_ms(), 0, "decoding went on after drop");
}

#[test]
fn stream_reports_icy_titles() {
	let titles = vec![b"A - One".to_vec(), Vec::new()];
	let server = serve(vec![live(Some(8192), titles, None)]);
	let (_s, rx) = play(&server.url("/live"), &ctx(Duration::from_secs(60)));
	let got: Vec<_> = events_until(&rx, Duration::from_secs(3), |e| {
		*e == StreamEvent::Title(None)
	})
	.into_iter()
	.filter(|e| matches!(e, StreamEvent::Title(_)))
	.collect();
	assert_eq!(
		got,
		[
			StreamEvent::Title(Some("A - One".into())),
			StreamEvent::Title(None)
		]
	);
}

#[test]
fn icy_title_latin1_and_quotes() {
	let title = b"Caf\xe9 - It's".to_vec();
	let server = serve(vec![live(Some(8192), vec![title], None)]);
	let (_s, rx) = play(&server.url("/live"), &ctx(Duration::from_secs(60)));
	let seen = events_until(&rx, Duration::from_secs(3), |e| {
		matches!(e, StreamEvent::Title(_))
	});
	assert_eq!(
		seen.last(),
		Some(&StreamEvent::Title(Some("Café - It's".into())))
	);
}

#[test]
fn stream_fails_on_silent_server() {
	let server = serve(vec![Route::Silent {
		path: "/live".into(),
	}]);
	let (_s, rx) = play(&server.url("/live"), &ctx(Duration::from_secs(60)));
	let seen = events_until(&rx, Duration::from_secs(6), |e| {
		matches!(e, StreamEvent::Failed(_))
	});
	assert_eq!(seen, [StreamEvent::Failed("No data".into())]);
}

#[test]
fn stream_reconnects_silently() {
	let server = serve(vec![live(None, vec![], Some(64 * 1024))]);
	let (s, rx) = play(&server.url("/live"), &ctx(Duration::from_secs(60)));
	events_until(&rx, Duration::from_secs(1), |e| {
		*e == StreamEvent::FirstAudio
	});
	let stop = Arc::new(AtomicBool::new(false));
	let consumer = consume(s.ring.clone(), stop.clone());
	// Each connection lasts about 3.1 s, so this spans two reconnects.
	std::thread::sleep(Duration::from_secs(7));
	stop.store(true, Ordering::Relaxed);
	let longest_dry = consumer.join().unwrap();
	let events: Vec<_> = rx.try_iter().collect();
	assert!(events.is_empty(), "unexpected events: {events:?}");
	assert!(
		longest_dry <= Duration::from_millis(300),
		"ring dry for {longest_dry:?}"
	);
}

#[test]
fn stream_reports_lost_after_retries() {
	// One connection with about 2 s of audio, then the port closes for good.
	let listener = TcpListener::bind("127.0.0.1:0").unwrap();
	let url = format!("http://{}/live", listener.local_addr().unwrap());
	std::thread::spawn(move || {
		let (mut sock, _) = listener.accept().unwrap();
		let mut req = [0u8; 1024];
		let _ = sock.read(&mut req).unwrap();
		sock.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: audio/mpeg\r\nConnection: close\r\n\r\n")
			.unwrap();
		sock.write_all(&tone()[..16_000]).unwrap();
	});
	let (s, rx) = play(&url, &ctx(Duration::from_secs(3)));
	events_until(&rx, Duration::from_secs(1), |e| {
		*e == StreamEvent::FirstAudio
	});
	let stop = Arc::new(AtomicBool::new(false));
	let consumer = consume(s.ring.clone(), stop.clone());
	let seen = events_until(&rx, Duration::from_secs(15), |e| {
		matches!(e, StreamEvent::Failed(_))
	});
	stop.store(true, Ordering::Relaxed);
	consumer.join().unwrap();
	assert_eq!(
		seen,
		[
			StreamEvent::Reconnecting,
			StreamEvent::Failed("Lost".into())
		]
	);
}

enum Reply {
	/// 200 with these body bytes, then close.
	Audio(Vec<u8>),
	/// 200 with a body no decoder accepts, then close.
	Junk,
	Busy,
}

/// Local server whose reply to each connection is chosen by `script(index, since_start)`.
fn scripted(script: impl Fn(usize, Duration) -> Reply + Send + 'static) -> String {
	let listener = TcpListener::bind("127.0.0.1:0").unwrap();
	let url = format!("http://{}/live", listener.local_addr().unwrap());
	let began = Instant::now();
	std::thread::spawn(move || {
		for (i, sock) in listener.incoming().enumerate() {
			let Ok(mut sock) = sock else { continue };
			let reply = script(i, began.elapsed());
			std::thread::spawn(move || {
				let mut req = [0u8; 1024];
				let _ = sock.read(&mut req);
				let ok = "HTTP/1.1 200 OK\r\nContent-Type: audio/mpeg\r\nConnection: close\r\n\r\n";
				let _ = match reply {
					Reply::Audio(body) => sock
						.write_all(ok.as_bytes())
						.and_then(|()| sock.write_all(&body)),
					Reply::Junk => sock
						.write_all(ok.as_bytes())
						.and_then(|()| sock.write_all(&[0xAB; 2000])),
					Reply::Busy => sock.write_all(
						b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
					),
				};
			});
		}
	});
	url
}

#[test]
fn reconnected_fires_once_after_audio_returns() {
	// About 1 s of audio, refusals until the ring has run dry, then the station is back.
	let url = scripted(|i, since| match i {
		0 => Reply::Audio(tone()[..8_000].to_vec()),
		_ if since < Duration::from_millis(2500) => Reply::Busy,
		_ => Reply::Audio(tone()),
	});
	let (s, rx) = play(&url, &ctx(Duration::from_secs(60)));
	events_until(&rx, Duration::from_secs(1), |e| {
		*e == StreamEvent::FirstAudio
	});
	let stop = Arc::new(AtomicBool::new(false));
	let consumer = consume(s.ring.clone(), stop.clone());
	let mut seen = events_until(&rx, Duration::from_secs(10), |e| {
		*e == StreamEvent::Reconnected
	});
	assert!(s.ring.len_ms() > 0, "Reconnected before audio returned");
	std::thread::sleep(Duration::from_secs(1));
	stop.store(true, Ordering::Relaxed);
	consumer.join().unwrap();
	seen.extend(rx.try_iter());
	assert_eq!(seen, [StreamEvent::Reconnecting, StreamEvent::Reconnected]);
}

#[test]
fn reconnect_to_stalling_server_announces_once() {
	// After the first connection every reconnect is accepted but never yields audio.
	let url = scripted(|i, _| match i {
		0 => Reply::Audio(tone()[..8_000].to_vec()),
		_ => Reply::Junk,
	});
	let (s, rx) = play(&url, &ctx(Duration::from_secs(4)));
	events_until(&rx, Duration::from_secs(1), |e| {
		*e == StreamEvent::FirstAudio
	});
	let stop = Arc::new(AtomicBool::new(false));
	let consumer = consume(s.ring.clone(), stop.clone());
	let seen = events_until(&rx, Duration::from_secs(15), |e| {
		matches!(e, StreamEvent::Failed(_))
	});
	stop.store(true, Ordering::Relaxed);
	consumer.join().unwrap();
	assert_eq!(
		seen,
		[
			StreamEvent::Reconnecting,
			StreamEvent::Failed("Lost".into())
		]
	);
}

#[test]
fn stale_cached_url_falls_back_and_is_evicted() {
	let server = serve(vec![
		Route::Silent {
			path: "/old".into(),
		},
		live(None, vec![], None),
	]);
	let station_url = server.url("/live");
	let ctx = ctx(Duration::from_secs(60));
	ctx.cache.lock().put(&station_url, &server.url("/old"));
	let (_s, rx) = play(&station_url, &ctx);
	events_until(&rx, Duration::from_secs(5), |e| {
		*e == StreamEvent::FirstAudio
	});
	let cached = ctx.cache.lock().get(&station_url);
	assert!(
		cached.is_none() || cached.as_deref() == Some(station_url.as_str()),
		"cache still holds {cached:?}"
	);
}

#[test]
fn stream_plays_hls_with_extinf_titles() {
	let mut playlist = "#EXTM3U\n#EXT-X-TARGETDURATION:1\n#EXT-X-MEDIA-SEQUENCE:0\n".to_owned();
	let mut routes = Vec::new();
	for i in 0..5 {
		playlist += &format!("#EXTINF:1.0,A - One\nseg{i}.ts\n");
		let path = format!(
			"{}/tests/fixtures/hls/seg{i}.ts",
			env!("CARGO_MANIFEST_DIR")
		);
		routes.push(Route::Static {
			path: format!("/seg{i}.ts"),
			content_type: "video/mp2t".into(),
			body: std::fs::read(path).unwrap(),
		});
	}
	routes.push(Route::Static {
		path: "/live.m3u8".into(),
		content_type: "application/vnd.apple.mpegurl".into(),
		body: playlist.into_bytes(),
	});
	let server = serve(routes);
	let (_s, rx) = play(&server.url("/live.m3u8"), &ctx(Duration::from_secs(60)));
	let title = StreamEvent::Title(Some("A - One".into()));
	let mut seen = Vec::new();
	while !(seen.contains(&StreamEvent::FirstAudio) && seen.contains(&title)) {
		seen.extend(events_until(&rx, Duration::from_secs(3), |_| true));
	}
	assert_eq!(seen.iter().filter(|e| **e == title).count(), 1, "{seen:?}");
}

#[test]
fn stream_fails_on_endless_junk() {
	let server = serve(vec![Route::Stream {
		path: "/live".into(),
		content_type: "audio/mpeg".into(),
		data: b"not audio at all ".repeat(64),
		metaint: None,
		titles: vec![],
		burst: 16_000,
		bytes_per_sec: 16_000,
		close_after: None,
	}]);
	let (_s, rx) = play(&server.url("/live"), &ctx(Duration::from_secs(60)));
	let seen = events_until(&rx, Duration::from_secs(8), |e| {
		matches!(e, StreamEvent::Failed(_))
	});
	assert!(
		matches!(seen.as_slice(), [StreamEvent::Failed(r)] if r == "Not a stream"),
		"{seen:?}"
	);
}

#[test]
fn hls_vod_ends_once_without_refetching_playlist() {
	let mut playlist = "#EXTM3U\n#EXT-X-TARGETDURATION:1\n#EXT-X-MEDIA-SEQUENCE:0\n".to_owned();
	let mut routes = Vec::new();
	for i in 0..2 {
		playlist += &format!("#EXTINF:1.0,\nseg{i}.ts\n");
		let path = format!(
			"{}/tests/fixtures/hls/seg{i}.ts",
			env!("CARGO_MANIFEST_DIR")
		);
		routes.push(Route::Static {
			path: format!("/seg{i}.ts"),
			content_type: "video/mp2t".into(),
			body: std::fs::read(path).unwrap(),
		});
	}
	playlist += "#EXT-X-ENDLIST\n";
	routes.push(Route::Static {
		path: "/vod.m3u8".into(),
		content_type: "application/vnd.apple.mpegurl".into(),
		body: playlist.into_bytes(),
	});
	let server = serve(routes);
	let (_s, rx) = play(&server.url("/vod.m3u8"), &ctx(Duration::from_secs(60)));
	let seen = events_until(&rx, Duration::from_secs(6), |e| {
		matches!(e, StreamEvent::Failed(_))
	});
	assert_eq!(
		seen.last(),
		Some(&StreamEvent::Failed("Ended".into())),
		"{seen:?}"
	);
	assert!(
		rx.recv_timeout(Duration::from_secs(2)).is_err(),
		"an ended playlist is not replayed"
	);
	assert_eq!(server.hits("/vod.m3u8"), 1, "open() read the playlist once");
}
