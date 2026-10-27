mod common;

use blindradio_core::net::{Opened, ResolveCache, client, open};
use common::{Route, serve};

fn stream(path: &str) -> Route {
	Route::Stream {
		path: path.into(),
		content_type: "audio/mpeg".into(),
		data: vec![0xAA; 1000],
		metaint: None,
		titles: vec![],
		burst: 4096,
		bytes_per_sec: 0,
		close_after: Some(8192),
	}
}

fn text(path: &str, ct: &str, body: &str) -> Route {
	Route::Static {
		path: path.into(),
		content_type: ct.into(),
		body: body.as_bytes().to_vec(),
	}
}

fn redirect(path: &str, to: &str) -> Route {
	Route::Redirect {
		path: path.into(),
		to: to.into(),
	}
}

#[tokio::test]
async fn open_direct_audio_keeps_response() {
	let s = serve(vec![stream("/live")]);
	let url = s.url("/live");
	let Opened::Audio {
		mut response,
		final_url,
	} = open(&client(), &url).await.unwrap()
	else {
		panic!("expected audio");
	};
	assert_eq!(final_url, url);
	assert!(!response.chunk().await.unwrap().unwrap().is_empty());
}

#[tokio::test]
async fn open_follows_pls_to_audio() {
	let s = serve(vec![
		text(
			"/s.pls",
			"audio/x-scpls",
			"[playlist]\nFile1={base}/live\nNumberOfEntries=1\n",
		),
		stream("/live"),
	]);
	let Opened::Audio { final_url, .. } = open(&client(), &s.url("/s.pls")).await.unwrap() else {
		panic!("expected audio");
	};
	assert_eq!(final_url, s.url("/live"));
}

#[tokio::test]
async fn open_follows_m3u_in_text_plain() {
	let s = serve(vec![
		text("/s.m3u", "text/plain", "#EXTM3U\n{base}/live\n"),
		stream("/live"),
	]);
	let Opened::Audio { final_url, .. } = open(&client(), &s.url("/s.m3u")).await.unwrap() else {
		panic!("expected audio");
	};
	assert_eq!(final_url, s.url("/live"));
}

#[tokio::test]
async fn open_follows_redirect_and_reports_final_url() {
	let s = serve(vec![redirect("/go", "{base}/live"), stream("/live")]);
	let Opened::Audio { final_url, .. } = open(&client(), &s.url("/go")).await.unwrap() else {
		panic!("expected audio");
	};
	assert_eq!(final_url, s.url("/live"));
}

#[tokio::test]
async fn open_detects_hls() {
	let s = serve(vec![text(
		"/h.m3u8",
		"application/vnd.apple.mpegurl",
		"#EXTM3U\n#EXT-X-VERSION:3\n#EXTINF:4,\nseg.ts\n",
	)]);
	let Opened::Hls {
		playlist_url,
		playlist,
	} = open(&client(), &s.url("/h.m3u8")).await.unwrap()
	else {
		panic!("expected hls");
	};
	assert_eq!(playlist_url, s.url("/h.m3u8"));
	assert!(playlist.contains("seg.ts"), "the body read is handed on");
}

#[tokio::test]
async fn open_skips_dead_first_entry_quickly() {
	// 10.255.255.1 is unroutable: connecting there hangs until the 4 s connect timeout.
	let s = serve(vec![
		text(
			"/s.pls",
			"audio/x-scpls",
			"[playlist]\nFile1=http://10.255.255.1/dead\nFile2={base}/live\n",
		),
		stream("/live"),
	]);
	let started = std::time::Instant::now();
	let Opened::Audio { final_url, .. } = open(&client(), &s.url("/s.pls")).await.unwrap() else {
		panic!("expected audio");
	};
	assert_eq!(final_url, s.url("/live"));
	assert!(
		started.elapsed() < std::time::Duration::from_secs(2),
		"{:?}",
		started.elapsed()
	);
}

#[tokio::test]
async fn open_rejects_html() {
	let s = serve(vec![text(
		"/p",
		"text/html",
		"<html><body>hi</body></html>",
	)]);
	let err = open(&client(), &s.url("/p")).await.err().unwrap();
	assert_eq!(err.to_string(), "Not a stream");
}

#[tokio::test]
async fn open_stops_at_depth_3() {
	let s = serve(vec![text(
		"/loop.m3u",
		"text/plain",
		"#EXTM3U\n{base}/loop.m3u\n",
	)]);
	let err = open(&client(), &s.url("/loop.m3u")).await.err().unwrap();
	assert_eq!(err.to_string(), "Not a stream");
	assert_eq!(
		s.hits("/loop.m3u"),
		3,
		"the station URL plus two nested levels"
	);
}

#[tokio::test]
async fn open_maps_http_status_and_refused() {
	let s = serve(vec![]);
	let err = open(&client(), &s.url("/missing")).await.err().unwrap();
	assert_eq!(err.to_string(), "HTTP 404");
	let err = open(&client(), "http://127.0.0.1:1/").await.err().unwrap();
	assert_eq!(err.to_string(), "Unreachable");
}

#[test]
fn resolve_cache_round_trip() {
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("sub").join("resolve.json");
	assert!(ResolveCache::load(&path).get("a").is_none());
	let mut c = ResolveCache::load(&path);
	c.put("http://a/x", "http://b/y");
	c.save().unwrap();
	assert_eq!(
		ResolveCache::load(&path).get("http://a/x").as_deref(),
		Some("http://b/y")
	);
}

#[tokio::test]
async fn test_server_icy_framing() {
	let s = serve(vec![Route::Stream {
		path: "/icy".into(),
		content_type: "audio/mpeg".into(),
		data: vec![b'a'; 100],
		metaint: Some(16),
		titles: vec![b"Hi".to_vec(), vec![b'C', b'a', b'f', 0xE9]],
		burst: 200,
		bytes_per_sec: 0,
		close_after: Some(48),
	}]);
	let r = client().get(s.url("/icy")).send().await.unwrap();
	assert_eq!(r.headers()["icy-metaint"], "16");
	let body = r.bytes().await.unwrap();
	// 16 audio, len byte 2, 32 meta bytes, 16 audio, len byte 2, 32 meta, 16 audio
	assert_eq!(body.len(), 16 + 33 + 16 + 33 + 16);
	assert_eq!(&body[..16], &[b'a'; 16]);
	assert_eq!(body[16], 2);
	assert_eq!(&body[17..34], b"StreamTitle='Hi';");
	assert!(body[34..49].iter().all(|&b| b == 0));
	assert_eq!(&body[49..65], &[b'a'; 16]);
	assert_eq!(body[65], 2);
	assert_eq!(&body[66..85], b"StreamTitle='Caf\xE9';");
}
