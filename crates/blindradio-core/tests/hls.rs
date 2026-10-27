mod common;

use std::sync::LazyLock;

use blindradio_core::decode::{Decoder, byte_pipe};
use blindradio_core::hls::run_hls;
use blindradio_core::net::client;
use blindradio_core::{CoreError, Result};
use common::{Route, serve};

const M3U8: &str = "application/vnd.apple.mpegurl";

static RT: LazyLock<tokio::runtime::Runtime> = LazyLock::new(|| {
	tokio::runtime::Builder::new_multi_thread()
		.enable_all()
		.build()
		.unwrap()
});

fn fixture(name: &str) -> Vec<u8> {
	std::fs::read(format!(
		"{}/tests/fixtures/hls/{name}",
		env!("CARGO_MANIFEST_DIR")
	))
	.unwrap()
}

fn file(path: &str, content_type: &str, body: Vec<u8>) -> Route {
	Route::Static {
		path: path.into(),
		content_type: content_type.into(),
		body,
	}
}

/// Every fixture segment at `/<name>`, plus `extra` routes.
fn routes(extra: Vec<Route>) -> Vec<Route> {
	let mut routes = extra;
	for i in 0..5 {
		routes.push(file(
			&format!("/seg{i}.ts"),
			"video/mp2t",
			fixture(&format!("seg{i}.ts")),
		));
		routes.push(file(
			&format!("/aac{i}.aac"),
			"audio/aac",
			fixture(&format!("aac{i}.aac")),
		));
	}
	routes
}

/// Media playlist of 1 s segments, the first numbered `first`.
fn media(names: &[&str], first: u64, ended: bool) -> Vec<u8> {
	let mut s = format!("#EXTM3U\n#EXT-X-TARGETDURATION:1\n#EXT-X-MEDIA-SEQUENCE:{first}\n");
	for name in names {
		s += &format!("#EXTINF:1.0,\n{name}\n");
	}
	if ended {
		s += "#EXT-X-ENDLIST\n";
	}
	s.into_bytes()
}

/// Runs `run_hls` while decoding the pipe; returns its result, the sample rate and stereo frames.
fn play(url: String) -> (Result<()>, u32, usize) {
	let (tx, reader) = byte_pipe();
	let task = RT.spawn(run_hls(client(), url, None, tx, Box::new(|_| {})));
	let mut dec = Decoder::new(Box::new(reader), Some("aac")).unwrap();
	let mut pcm = Vec::new();
	while dec.next_stereo(&mut pcm).unwrap() {}
	(RT.block_on(task).unwrap(), dec.sample_rate(), pcm.len() / 2)
}

#[test]
fn hls_ts_segments_decode() {
	let server = serve(routes(vec![file("/live.m3u8", M3U8, fixture("live.m3u8"))]));
	let (result, rate, frames) = play(server.url("/live.m3u8"));
	result.unwrap();
	assert_eq!(rate, 44100);
	assert!(frames >= 3 * 44100, "{frames} frames");
}

#[test]
fn hls_master_picks_audio_only_variant() {
	let server = serve(routes(vec![
		file("/master.m3u8", M3U8, fixture("master.m3u8")),
		file("/live.m3u8", M3U8, fixture("live.m3u8")),
	]));
	let (result, _, frames) = play(server.url("/master.m3u8"));
	result.unwrap();
	assert!(frames >= 3 * 44100, "{frames} frames");
	assert_eq!(server.hits("/video.m3u8"), 0);
	assert_eq!(server.hits("/live.m3u8"), 1);
}

#[test]
fn hls_live_reload_appends_new_segments() {
	let first = media(
		&["seg0.ts", "seg1.ts", "seg2.ts", "seg3.ts", "seg4.ts"],
		0,
		false,
	);
	let second = media(
		&["seg1.ts", "seg2.ts", "seg3.ts", "seg4.ts", "seg5.ts"],
		1,
		false,
	);
	let server = serve(routes(vec![
		Route::Sequence {
			path: "/live.m3u8".into(),
			content_type: M3U8.into(),
			bodies: vec![first, second],
		},
		file("/seg5.ts", "video/mp2t", fixture("seg0.ts")),
	]));
	// No segment follows seg5, so the live stream stalls and ends with an error.
	let (result, _, frames) = play(server.url("/live.m3u8"));
	assert!(result.is_err());
	assert!(server.hits("/live.m3u8") >= 2);
	let hits: Vec<_> = (0..6)
		.map(|i| server.hits(&format!("/seg{i}.ts")))
		.collect();
	// Live start is the third-from-last segment.
	assert_eq!(hits, [0, 0, 1, 1, 1, 1]);
	assert!(frames >= 3 * 44100, "{frames} frames");
}

#[test]
fn hls_raw_aac_segments_decode() {
	let names = ["aac0.aac", "aac1.aac", "aac2.aac", "aac3.aac", "aac4.aac"];
	let server = serve(routes(vec![file(
		"/aac.m3u8",
		M3U8,
		media(&names, 0, true),
	)]));
	let (result, rate, frames) = play(server.url("/aac.m3u8"));
	result.unwrap();
	assert_eq!(rate, 44100);
	assert!(frames >= 3 * 44100, "{frames} frames");
}

#[test]
fn hls_fmp4_is_unsupported() {
	let fmp4 =
		"#EXTM3U\n#EXT-X-TARGETDURATION:1\n#EXT-X-MAP:URI=\"init.mp4\"\n#EXTINF:1.0,\nseg0.m4s\n";
	let aes = "#EXTM3U\n#EXT-X-TARGETDURATION:1\n#EXT-X-KEY:METHOD=AES-128,URI=\"key\"\n#EXTINF:1.0,\nseg0.ts\n";
	let server = serve(routes(vec![
		file("/fmp4.m3u8", M3U8, fmp4.into()),
		file("/aes.m3u8", M3U8, aes.into()),
	]));
	for path in ["/fmp4.m3u8", "/aes.m3u8"] {
		let (tx, _rx) = crossbeam_channel::unbounded();
		let result = RT.block_on(run_hls(
			client(),
			server.url(path),
			None,
			tx,
			Box::new(|_| {}),
		));
		match result {
			Err(CoreError::Decode(e)) => assert_eq!(e, "Unsupported format", "{path}"),
			other => panic!("{path}: {other:?}"),
		}
	}
	assert_eq!(server.hits("/seg0.m4s") + server.hits("/seg0.ts"), 0);
}
