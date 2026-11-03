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
	// Live start is the second-from-last segment.
	assert_eq!(hits, [0, 0, 0, 1, 1, 1]);
	assert!(frames >= 2 * 44100, "{frames} frames");
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
fn hls_master_follows_separate_audio_rendition() {
	let master = "#EXTM3U\n\
		#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"aac\",NAME=\"Main\",DEFAULT=YES,URI=\"live.m3u8\"\n\
		#EXT-X-STREAM-INF:BANDWIDTH=900000,CODECS=\"avc1.42e00a,mp4a.40.2\",AUDIO=\"aac\"\n\
		video.m3u8\n";
	let server = serve(routes(vec![
		file("/master.m3u8", M3U8, master.into()),
		file("/live.m3u8", M3U8, fixture("live.m3u8")),
	]));
	let (result, _, frames) = play(server.url("/master.m3u8"));
	result.unwrap();
	assert!(frames >= 3 * 44100, "{frames} frames");
	assert_eq!(server.hits("/video.m3u8"), 0);
}

#[test]
fn hls_gzipped_playlists_and_segments_decode() {
	// RTP's chunklists reach the player gzipped, wanted or not.
	let names = ["aac0.aac", "aac1.aac", "aac2.aac", "aac3.aac", "aac4.aac"];
	let server = serve(routes(vec![Route::Gzip {
		path: "/gz.m3u8".into(),
		content_type: M3U8.into(),
		body: media(&names, 0, true),
	}]));
	let (result, _, frames) = play(server.url("/gz.m3u8"));
	result.unwrap();
	assert!(frames >= 3 * 44100, "{frames} frames");
}

#[test]
fn hls_fragmented_mp4_decodes_through_the_pipe() {
	let mut routes = vec![
		file("/fmp4.m3u8", M3U8, fixture("fmp4.m3u8")),
		file("/init.mp4", "video/mp4", fixture("init.mp4")),
	];
	for i in 0..4 {
		let name = format!("f{i}.m4s");
		routes.push(file(&format!("/{name}"), "video/iso.segment", fixture(&name)));
	}
	let server = serve(routes);
	let (tx, reader) = byte_pipe();
	let task = RT.spawn(run_hls(
		client(),
		server.url("/fmp4.m3u8"),
		None,
		tx,
		Box::new(|_| {}),
	));
	let mut dec = Decoder::new(Box::new(reader), Some("mp4")).unwrap();
	let mut pcm = Vec::new();
	while dec.next_stereo(&mut pcm).unwrap() {}
	RT.block_on(task).unwrap().unwrap();
	assert_eq!(dec.codec(), "AAC");
	assert_eq!(dec.sample_rate(), 48_000);
	assert!(pcm.len() / 2 >= 2 * 48_000, "{} frames", pcm.len() / 2);
	assert_eq!(server.hits("/init.mp4"), 1, "the init segment goes once");
}

/// Seals `data` as HLS's AES-128 does: CBC with PKCS#7 padding.
fn seal(data: &[u8], key: &[u8; 16], iv: [u8; 16]) -> Vec<u8> {
	use aes::cipher::{BlockCipherEncrypt, KeyInit};
	let cipher = aes::Aes128::new(&(*key).into());
	let mut padded = data.to_vec();
	let pad = 16 - data.len() % 16;
	padded.resize(data.len() + pad, pad as u8);
	let mut out = Vec::with_capacity(padded.len());
	let mut previous = iv;
	for block in padded.as_chunks::<16>().0 {
		let xored: [u8; 16] = std::array::from_fn(|i| block[i] ^ previous[i]);
		let mut sealed = xored.into();
		cipher.encrypt_block(&mut sealed);
		previous.copy_from_slice(&sealed);
		out.extend_from_slice(&sealed);
	}
	out
}

#[test]
fn hls_aes128_segments_decode() {
	let key = [0x2Bu8; 16];
	// One key for the whole playlist, stated once: the first segment's IV by attribute, and a
	// key rotation that takes the sequence number as the IV.
	let stated = 0x1234_5678u128;
	let mut playlist =
		"#EXTM3U\n#EXT-X-TARGETDURATION:1\n#EXT-X-MEDIA-SEQUENCE:0\n".to_owned();
	playlist += &format!("#EXT-X-KEY:METHOD=AES-128,URI=\"key.bin\",IV=0x{stated:032X}\n");
	playlist += "#EXTINF:1.0,\nenc0.ts\n#EXTINF:1.0,\nenc1.ts\n";
	playlist += "#EXT-X-KEY:METHOD=AES-128,URI=\"key.bin\"\n#EXTINF:1.0,\nenc2.ts\n";
	playlist += "#EXT-X-ENDLIST\n";
	let sealed = [
		seal(&fixture("seg0.ts"), &key, stated.to_be_bytes()),
		seal(&fixture("seg1.ts"), &key, stated.to_be_bytes()),
		seal(&fixture("seg2.ts"), &key, 2u128.to_be_bytes()),
	];
	let mut routes = vec![
		file("/aes.m3u8", M3U8, playlist.into_bytes()),
		file("/key.bin", "application/octet-stream", key.to_vec()),
	];
	for (i, body) in sealed.into_iter().enumerate() {
		routes.push(file(&format!("/enc{i}.ts"), "video/mp2t", body));
	}
	let server = serve(routes);
	let (result, rate, frames) = play(server.url("/aes.m3u8"));
	result.unwrap();
	assert_eq!(rate, 44100);
	assert!(frames >= 2 * 44100, "{frames} frames");
	assert_eq!(server.hits("/key.bin"), 1, "the key is fetched once");
}

#[test]
fn hls_byte_ranges_fetch_only_their_part() {
	// The segments cut from one file, the second range continuing from the first.
	let (a, b) = (fixture("aac0.aac"), fixture("aac1.aac"));
	let playlist = format!(
		"#EXTM3U\n#EXT-X-TARGETDURATION:1\n#EXT-X-MEDIA-SEQUENCE:0\n\
		#EXT-X-BYTERANGE:{}@0\n#EXTINF:1.0,\nall.aac\n\
		#EXT-X-BYTERANGE:{}\n#EXTINF:1.0,\nall.aac\n#EXT-X-ENDLIST\n",
		a.len(),
		b.len()
	);
	let server = serve(vec![
		file("/ranged.m3u8", M3U8, playlist.into_bytes()),
		Route::Ranged {
			path: "/all.aac".into(),
			content_type: "audio/aac".into(),
			body: [a, b].concat(),
		},
	]);
	let (result, _, frames) = play(server.url("/ranged.m3u8"));
	result.unwrap();
	assert!(frames >= 44100, "{frames} frames");
	assert!(frames < 3 * 44100, "not the file twice: {frames} frames");
	assert_eq!(server.hits("/all.aac"), 2);
}

#[test]
fn hls_drm_is_refused() {
	let sample = "#EXTM3U\n#EXT-X-TARGETDURATION:1\n#EXT-X-KEY:METHOD=SAMPLE-AES,URI=\"key\"\n#EXTINF:1.0,\nseg0.ts\n";
	let fairplay = "#EXTM3U\n#EXT-X-TARGETDURATION:1\n#EXT-X-KEY:METHOD=AES-128,URI=\"skd://x\",KEYFORMAT=\"com.apple.streamingkeydelivery\"\n#EXTINF:1.0,\nseg0.ts\n";
	let server = serve(routes(vec![
		file("/sample.m3u8", M3U8, sample.into()),
		file("/fairplay.m3u8", M3U8, fairplay.into()),
	]));
	for path in ["/sample.m3u8", "/fairplay.m3u8"] {
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
	assert_eq!(server.hits("/seg0.ts"), 0);
}
