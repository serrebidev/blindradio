mod common;

use blindradio_core::net::client;
use blindradio_core::sources::icecast::Icecast;
use blindradio_core::sources::{Node, SearchQuery};
use blindradio_core::station::Source;
use common::{Route, serve};

fn entry(name: &str, url: &str, genre: &str, kind: &str) -> String {
	format!(
		"<entry><server_name>{name}</server_name><listen_url>{url}</listen_url><genre>{genre}</genre><server_type>{kind}</server_type><bitrate>128</bitrate></entry>"
	)
}

fn route(body: String) -> Route {
	Route::Static {
		path: "/yp.xml".into(),
		content_type: "text/xml".into(),
		body: body.into_bytes(),
	}
}

fn folder(id: &str, name: &str) -> Node {
	Node::Folder {
		id: id.into(),
		name: name.into(),
	}
}

#[tokio::test]
async fn root_does_not_fetch() {
	let server = serve(vec![]);
	let ice = Icecast::new(&server.base);
	assert_eq!(
		ice.children(&client(), "icecast").await.unwrap(),
		vec![
			folder("icecast/all", "All Stations"),
			folder("icecast/genres", "By Genre")
		]
	);
	assert_eq!(server.hits("/yp.xml"), 0);
}

#[tokio::test]
async fn validates_decodes_sorts_and_deduplicates() {
	let mut body = String::from("<directory>");
	body += &entry(" Zulu ", "https://example.org/z", "Rock", "audio/mpeg");
	body += &entry(
		"A &amp; B &#x266b;",
		"https://example.org/a?a=1&amp;b=2",
		"Jazz &amp; Soul",
		"application/ogg",
	);
	body += &entry("Duplicate", "https://example.org/z", "Rock", "audio/aac");
	for (name, url, kind) in [
		(" ", "http://example.org/blank", "audio/mpeg"),
		("Bad", "not a url", "audio/mpeg"),
		("File", "file:///music.mp3", "audio/mpeg"),
		(
			"Credentials",
			"https://user:secret@example.org/a",
			"audio/mpeg",
		),
		("Video", "https://example.org/video", "video/webm"),
	] {
		body += &entry(name, url, "Rock", kind);
	}
	body += "</directory>";
	let server = serve(vec![route(body)]);
	let ice = Icecast::new(&server.base);
	let nodes = ice.children(&client(), "icecast/all").await.unwrap();
	assert_eq!(nodes.len(), 2);
	let Node::Station(station) = &nodes[0] else {
		panic!("station expected")
	};
	assert_eq!(station.name, "A & B ♫");
	assert_eq!(station.url, "https://example.org/a?a=1&b=2");
	assert_eq!(station.source, Source::Icecast);
	assert!(station.id.starts_with("icecast:"));
	assert_eq!(
		ice.children(&client(), "icecast/genres")
			.await
			.unwrap()
			.len(),
		2
	);
	assert_eq!(server.hits("/yp.xml"), 1);
}

#[tokio::test]
async fn genres_roundtrip_and_search_shares_cache() {
	let body = format!(
		"<directory>{}{}</directory>",
		entry(
			"Café Night",
			"http://example.org/a",
			"R&amp;B @ Café/夜",
			"audio/aacp"
		),
		entry("News", "http://example.org/b", "News", "audio/ogg")
	);
	let server = serve(vec![route(body)]);
	let ice = Icecast::new(&server.base);
	let genres = ice.children(&client(), "icecast/genres").await.unwrap();
	let Node::Folder { id, name } = &genres[1] else {
		panic!("folder expected")
	};
	assert_eq!(name, "R&B @ Café/夜");
	assert!(!id.contains('@'));
	assert!(id.contains("%40"));
	assert_eq!(ice.children(&client(), id).await.unwrap().len(), 1);
	let result = ice
		.search(
			&client(),
			&SearchQuery {
				name: "CAFÉ".into(),
				genre: Some("r&b".into()),
				..Default::default()
			},
		)
		.await
		.unwrap();
	assert_eq!(result.len(), 1);
	assert_eq!(
		ice.search(
			&client(),
			&SearchQuery {
				name: "night".into(),
				genre: Some("news".into()),
				..Default::default()
			}
		)
		.await
		.unwrap()
		.len(),
		0
	);
	assert_eq!(server.hits("/yp.xml"), 1);
}

#[tokio::test]
async fn pagination_has_no_gaps_and_concurrent_calls_share_fetch() {
	let body = format!(
		"<directory>{}</directory>",
		(0..201)
			.rev()
			.map(|i| entry(
				&format!("Station {i:03}"),
				&format!("http://example.org/{i}"),
				"Jazz",
				"audio/mpeg"
			))
			.collect::<String>()
	);
	let server = serve(vec![route(body)]);
	let ice = Icecast::new(&server.base);
	let client = client();
	let query = SearchQuery::default();
	let (first, genres, search) = tokio::join!(
		ice.children(&client, "icecast/all"),
		ice.children(&client, "icecast/genres"),
		ice.search(&client, &query)
	);
	let first = first.unwrap();
	assert_eq!(first.len(), 101);
	assert_eq!(
		first[100],
		Node::LoadMore {
			id: "icecast/all@100".into()
		}
	);
	assert_eq!(genres.unwrap().len(), 1);
	assert_eq!(search.unwrap().len(), 201);
	let second = ice.children(&client, "icecast/all@100").await.unwrap();
	assert_eq!(second.len(), 101);
	assert_eq!(
		second[100],
		Node::LoadMore {
			id: "icecast/all@200".into()
		}
	);
	let last = ice.children(&client, "icecast/all@200").await.unwrap();
	assert_eq!(last.len(), 1);
	assert!(
		ice.children(&client, "icecast/all@999999")
			.await
			.unwrap()
			.is_empty()
	);
	assert_eq!(server.hits("/yp.xml"), 1);
}

#[tokio::test]
async fn malformed_feed_is_retried() {
	let server = serve(vec![Route::Sequence {
		path: "/yp.xml".into(),
		content_type: "text/xml".into(),
		bodies: vec![
			b"<directory><entry></directory>".to_vec(),
			format!(
				"<directory>{}</directory>",
				entry("Recovered", "http://example.org/a", "", "audio/mp3")
			)
			.into_bytes(),
		],
	}]);
	let ice = Icecast::new(&server.base);
	assert!(ice.children(&client(), "icecast/all").await.is_err());
	assert_eq!(
		ice.children(&client(), "icecast/all").await.unwrap().len(),
		1
	);
	assert_eq!(server.hits("/yp.xml"), 2);
}

#[tokio::test]
async fn oversized_stream_is_rejected_without_content_length() {
	let server = serve(vec![Route::Stream {
		path: "/yp.xml".into(),
		content_type: "text/xml".into(),
		data: vec![b'x'; 1024],
		metaint: None,
		titles: vec![],
		burst: 17 * 1024 * 1024,
		bytes_per_sec: 0,
		close_after: Some(17 * 1024 * 1024),
	}]);
	let ice = Icecast::new(&server.base);
	assert_eq!(
		ice.children(&client(), "icecast/all")
			.await
			.unwrap_err()
			.to_string(),
		"Directory too large"
	);
}
