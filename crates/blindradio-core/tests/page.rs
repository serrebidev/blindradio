mod common;

use blindradio_core::net::client;
use blindradio_core::page::detect;
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

fn text(path: &str, content_type: &str, body: &str) -> Route {
	Route::Static {
		path: path.into(),
		content_type: content_type.into(),
		body: body.as_bytes().to_vec(),
	}
}

#[tokio::test]
async fn finds_the_streams_of_a_page_its_frame_and_its_script_and_only_those() {
	let page = r#"<html><head><title>Jazz Radio</title></head><body>
		<audio src="/live"></audio>
		<a href="/hi.mp3">High quality</a>
		<a href="/stream-info">About the stream</a>
		<a href="/gone.mp3">Dead link</a>
		<iframe src="/player"></iframe>
		<script src="/app.js"></script>
		</body></html>"#;
	let s = serve(vec![
		text("/page", "text/html", page),
		text("/player", "text/html", r#"<audio src="/framed.aac"></audio>"#),
		text("/app.js", "text/javascript", ""),
		text("/stream-info", "text/html", "<html>Our stream</html>"),
		stream("/live"),
		stream("/hi.mp3"),
		stream("/framed.aac"),
	]);
	let found = detect(&client(), &s.url("/page")).await.unwrap();
	let urls: Vec<&str> = found.iter().map(|st| st.url.as_str()).collect();
	assert_eq!(found.len(), 3, "{urls:?}");
	assert_eq!(found[0].url, s.url("/live"), "an audio element comes first");
	assert_eq!(
		found[0].name, "Jazz Radio - live",
		"the page's name, told apart from the frame's stream"
	);
	assert_eq!(found[0].info.as_deref(), Some("MP3"));
	assert_eq!(found[0].details.homepage.as_deref(), Some(s.url("/page").as_str()));
	assert!(urls.contains(&s.url("/framed.aac").as_str()), "{urls:?}");
	let linked = found.iter().find(|st| st.url == s.url("/hi.mp3")).unwrap();
	assert_eq!(linked.name, "High quality", "a link names its stream");
}

#[tokio::test]
async fn an_address_that_is_a_stream_is_its_own_answer() {
	let s = serve(vec![stream("/live")]);
	let found = detect(&client(), &s.url("/live")).await.unwrap();
	assert_eq!(found.len(), 1);
	assert_eq!(found[0].url, s.url("/live"));
}

#[tokio::test]
async fn a_page_naming_no_streams_finds_none_and_a_dead_page_says_so() {
	let s = serve(vec![text("/page", "text/html", "<html><title>No</title></html>")]);
	assert!(detect(&client(), &s.url("/page")).await.unwrap().is_empty());
	let dead = detect(&client(), &s.url("/nothing")).await.unwrap_err();
	assert_eq!(dead.to_string(), "HTTP 404");
}
