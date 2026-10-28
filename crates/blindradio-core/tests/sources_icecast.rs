mod common;

use blindradio_core::net::client;
use blindradio_core::sources::icecast::Icecast;
use blindradio_core::sources::{Node, SearchQuery, Sort, SortField};
use blindradio_core::station::Source;
use common::{Route, serve};

fn entry(name: &str, url: &str, genre: &str, kind: &str) -> String {
	entry_rate(name, url, genre, kind, 128)
}

fn entry_rate(name: &str, url: &str, genre: &str, kind: &str, bitrate: u32) -> String {
	format!(
		"<entry><server_name>{name}</server_name><listen_url>{url}</listen_url><genre>{genre}</genre><server_type>{kind}</server_type><bitrate>{bitrate}</bitrate></entry>"
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
async fn sorts_by_every_field_it_publishes() {
	let body = format!(
		"<directory>{}{}{}</directory>",
		entry_rate("Alpha", "https://example.org/a", "Rock", "audio/aac", 64),
		entry_rate("Beta", "https://example.org/b", "Jazz", "audio/mpeg", 192),
		entry_rate("gamma", "https://example.org/c", "Rock", "audio/mpeg", 128),
	);
	let server = serve(vec![route(body)]);
	let ice = Icecast::new(&server.base);

	async fn names(ice: &Icecast, sort: Sort) -> Vec<String> {
		ice.children(&client(), "icecast/all", sort)
			.await
			.unwrap()
			.into_iter()
			.filter_map(|node| match node {
				Node::Station(s) => Some(s.name),
				_ => None,
			})
			.collect()
	}
	let sort = |field, reverse| Sort { field, reverse };

	assert_eq!(
		names(&ice, sort(SortField::Name, false)).await,
		["Alpha", "Beta", "gamma"]
	);
	assert_eq!(
		names(&ice, sort(SortField::Name, true)).await,
		["gamma", "Beta", "Alpha"]
	);
	assert_eq!(
		names(&ice, sort(SortField::Bitrate, false)).await,
		["Alpha", "gamma", "Beta"]
	);
	assert_eq!(
		names(&ice, sort(SortField::Bitrate, true)).await,
		["Beta", "gamma", "Alpha"]
	);
	assert_eq!(
		names(&ice, sort(SortField::Codec, false)).await,
		["Alpha", "Beta", "gamma"]
	);
	assert_eq!(
		names(&ice, sort(SortField::Tags, false)).await,
		["Beta", "Alpha", "gamma"]
	);
	assert_eq!(
		names(&ice, sort(SortField::Url, true)).await,
		["gamma", "Beta", "Alpha"]
	);
	// The default is alphabetical, as this directory has always listed.
	assert_eq!(
		names(&ice, sort(SortField::Default, false)).await,
		["Alpha", "Beta", "gamma"]
	);
	// A field Icecast does not publish leaves the default order alone.
	assert_eq!(
		names(&ice, sort(SortField::Country, true)).await,
		["Alpha", "Beta", "gamma"]
	);
	assert_eq!(
		names(&ice, sort(SortField::ClickCount, true)).await,
		["Alpha", "Beta", "gamma"]
	);
	let shuffled = names(&ice, sort(SortField::Random, false)).await;
	assert_eq!(shuffled.len(), 3);
	assert!(
		shuffled
			.iter()
			.all(|n| ["Alpha", "Beta", "gamma"].contains(&n.as_str()))
	);
}

#[tokio::test]
async fn search_uses_the_chosen_sort() {
	let body = format!(
		"<directory>{}{}</directory>",
		entry_rate("Jazz One", "https://example.org/1", "Jazz", "audio/aac", 64),
		entry_rate(
			"Jazz Two",
			"https://example.org/2",
			"Jazz",
			"audio/mpeg",
			192
		),
	);
	let server = serve(vec![route(body)]);
	let ice = Icecast::new(&server.base);
	let query = |sort| SearchQuery {
		name: "jazz".into(),
		sort,
		..Default::default()
	};
	let by_codec = ice
		.search(
			&client(),
			&query(Sort {
				field: SortField::Codec,
				reverse: true,
			}),
		)
		.await
		.unwrap();
	assert_eq!(
		by_codec.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
		["Jazz Two", "Jazz One"]
	);
	let by_bitrate = ice
		.search(
			&client(),
			&query(Sort {
				field: SortField::Bitrate,
				reverse: false,
			}),
		)
		.await
		.unwrap();
	assert_eq!(
		by_bitrate
			.iter()
			.map(|s| s.name.as_str())
			.collect::<Vec<_>>(),
		["Jazz One", "Jazz Two"]
	);
}

#[tokio::test]
async fn root_does_not_fetch() {
	let server = serve(vec![]);
	let ice = Icecast::new(&server.base);
	assert_eq!(
		ice.children(&client(), "icecast", Sort::default())
			.await
			.unwrap(),
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
	let nodes = ice
		.children(&client(), "icecast/all", Sort::default())
		.await
		.unwrap();
	assert_eq!(nodes.len(), 2);
	let Node::Station(station) = &nodes[0] else {
		panic!("station expected")
	};
	assert_eq!(station.name, "A & B ♫");
	assert_eq!(station.url, "https://example.org/a?a=1&b=2");
	assert_eq!(station.source, Source::Icecast);
	assert!(station.id.starts_with("icecast:"));
	assert_eq!(
		ice.children(&client(), "icecast/genres", Sort::default())
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
	let genres = ice
		.children(&client(), "icecast/genres", Sort::default())
		.await
		.unwrap();
	let Node::Folder { id, name } = &genres[1] else {
		panic!("folder expected")
	};
	assert_eq!(name, "R&B @ Café/夜");
	assert!(!id.contains('@'));
	assert!(id.contains("%40"));
	assert_eq!(
		ice.children(&client(), id, Sort::default())
			.await
			.unwrap()
			.len(),
		1
	);
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
		ice.children(&client, "icecast/all", Sort::default()),
		ice.children(&client, "icecast/genres", Sort::default()),
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
	let second = ice
		.children(&client, "icecast/all@100", Sort::default())
		.await
		.unwrap();
	assert_eq!(second.len(), 101);
	assert_eq!(
		second[100],
		Node::LoadMore {
			id: "icecast/all@200".into()
		}
	);
	let last = ice
		.children(&client, "icecast/all@200", Sort::default())
		.await
		.unwrap();
	assert_eq!(last.len(), 1);
	assert!(
		ice.children(&client, "icecast/all@999999", Sort::default())
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
	assert!(
		ice.children(&client(), "icecast/all", Sort::default())
			.await
			.is_err()
	);
	assert_eq!(
		ice.children(&client(), "icecast/all", Sort::default())
			.await
			.unwrap()
			.len(),
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
		ice.children(&client(), "icecast/all", Sort::default())
			.await
			.unwrap_err()
			.to_string(),
		"Directory too large"
	);
}
