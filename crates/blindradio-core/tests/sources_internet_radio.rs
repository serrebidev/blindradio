mod common;

use blindradio_core::net::client;
use blindradio_core::sources::internet_radio::InternetRadio;
use blindradio_core::sources::{Node, SearchQuery};
use blindradio_core::station::Source;
use common::{Route, serve};

fn page(path: &str, body: &str) -> Route {
	Route::Static {
		path: path.into(),
		content_type: "text/html; charset=utf-8".into(),
		body: body.as_bytes().to_vec(),
	}
}

fn row(slug: &str, name: &str, stream: &str) -> String {
	format!(
		r#"<table><tr><td>
<a href="/servers/tools/playlistgenerator/?u={stream}&amp;t=.pls">PLS</a>
<a href="/servers/tools/playlistgenerator/?u={stream}&amp;t=.m3u">M3U</a>
</td><td><h4><a href="/station/{slug}/">{name}</a></h4>
Genres: <a href="/stations/jazz/">Jazz</a> <a href="/stations/soul/">Soul</a>
<a href="/servers/tools/playlistgenerator/?u={stream}&amp;t=.pls">Mobile</a>
</td></tr></table>"#
	)
}

#[tokio::test]
async fn featured_maps_entities_stream_and_stable_id_once() {
	let html = row("cafe", "Café &amp; Soul", "https://audio.test/live.pls");
	let server = serve(vec![page("/", &html)]);
	let directory = InternetRadio::new(server.base.clone());
	let roots = directory
		.children(&client(), "internet-radio")
		.await
		.unwrap();
	assert!(
		matches!(&roots[0], Node::Folder { id, name } if id == "internet-radio/featured" && name == "Featured Stations")
	);
	assert!(
		matches!(&roots[1], Node::Folder { id, name } if id == "internet-radio/genres" && name == "By Genre")
	);
	let nodes = directory
		.children(&client(), "internet-radio/featured")
		.await
		.unwrap();
	assert_eq!(nodes.len(), 1);
	let Node::Station(station) = &nodes[0] else {
		panic!("station")
	};
	assert_eq!(station.id, "internet-radio:cafe");
	assert_eq!(station.name, "Café & Soul");
	assert_eq!(station.url, "https://audio.test/live.pls");
	assert_eq!(station.info.as_deref(), Some("Jazz, Soul"));
	assert_eq!(station.source, Source::InternetRadio);
}

#[tokio::test]
async fn invalid_streams_and_blank_names_are_omitted() {
	let mut html = String::new();
	for stream in [
		"file:///tmp/a",
		"javascript:alert(1)",
		"https://u:p@audio.test/live",
		"not a URL",
		"http://",
		"https://@audio.test/live",
	] {
		html.push_str(&row("bad", "Bad", stream));
	}
	html.push_str(&row("blank", "  ", "https://audio.test/blank"));
	html.push_str(&row("good", "Good", "https://audio.test/live?x=1%26y%3D2"));
	let server = serve(vec![page("/", &html)]);
	let nodes = InternetRadio::new(server.base)
		.children(&client(), "internet-radio/featured")
		.await
		.unwrap();
	assert_eq!(nodes.len(), 1);
	assert!(
		matches!(&nodes[0], Node::Station(s) if s.name == "Good" && s.url == "https://audio.test/live?x=1&y=2")
	);
}

#[tokio::test]
async fn genre_ids_roundtrip_spaces_unicode_and_punctuation() {
	let html = r#"<h1>Genres</h1><a href="/stations/drum and bass/">Drum &amp; Bass</a>
<a href="/stations/世界@home/">世界@home</a><a href="/stations/drum and bass/">Duplicate</a>
<a href="https://other.test/stations/rock/">External</a>"#;
	let server = serve(vec![
		page("/stations/", html),
		page(
			"/stations/drum%20and%20bass/",
			&row("bass", "Bass", "http://audio.test/live"),
		),
		page(
			"/stations/%E4%B8%96%E7%95%8C@home/",
			&row("world", "World", "http://audio.test/world"),
		),
	]);
	let directory = InternetRadio::new(server.base.clone());
	let genres = directory
		.children(&client(), "internet-radio/genres")
		.await
		.unwrap();
	assert_eq!(genres.len(), 2);
	for node in genres {
		let Node::Folder { id, .. } = node else {
			panic!("genre")
		};
		assert_eq!(directory.children(&client(), &id).await.unwrap().len(), 1);
	}
	assert_eq!(server.hits("/stations/drum%20and%20bass/"), 1);
	assert_eq!(server.hits("/stations/%E4%B8%96%E7%95%8C@home/"), 1);
}

#[tokio::test]
async fn genre_pagination_follows_next_link_and_stops_at_last_page() {
	let first = format!(
		"{}<ul class='pagination'><li class='next'><a href='/stations/jazz/page2'>Next</a></li></ul>",
		row("one", "One", "https://audio.test/one")
	);
	let server = serve(vec![
		page("/stations/", "<a href='/stations/jazz/'>Jazz</a>"),
		page("/stations/jazz/", &first),
		page(
			"/stations/jazz/page2",
			&row("two", "Two", "https://audio.test/two"),
		),
	]);
	let directory = InternetRadio::new(server.base);
	let genres = directory
		.children(&client(), "internet-radio/genres")
		.await
		.unwrap();
	let Node::Folder { id, .. } = &genres[0] else {
		panic!("genre")
	};
	let nodes = directory.children(&client(), id).await.unwrap();
	assert_eq!(nodes.len(), 2);
	let Node::LoadMore { id } = &nodes[1] else {
		panic!("next page")
	};
	let next = directory.children(&client(), id).await.unwrap();
	assert!(matches!(&next[..], [Node::Station(s)] if s.name == "Two"));
}

#[tokio::test]
async fn external_or_unrelated_pagination_is_omitted() {
	for href in [
		"https://other.test/stations/jazz/page2",
		"//other.test/search/?radio=jazz",
		"/servers/",
		"http://user:pass@{host}/stations/jazz/page2",
	] {
		let html = format!(
			"{}<ul class='pagination'><li class='next'><a href='{href}'>Next</a></li></ul>",
			row("one", "One", "https://audio.test/one")
		);
		let server = serve(vec![page("/", &html)]);
		let nodes = InternetRadio::new(server.base)
			.children(&client(), "internet-radio/featured")
			.await
			.unwrap();
		assert_eq!(nodes.len(), 1, "{href}");
	}
}

#[tokio::test]
async fn search_follows_real_query_pagination_with_a_three_page_bound() {
	let html = format!(
		"{}<ul class='pagination'><li class='next'><a href='/search/?radio=Caf%C3%A9%20%26%20Soul&amp;page=/page2'>Next</a></li></ul>",
		row("one", "A provider genre match", "https://audio.test/one")
	);
	let server = serve(vec![page("/search/", &html)]);
	let results = InternetRadio::new(server.base.clone())
		.search(
			&client(),
			&SearchQuery {
				name: "Café & Soul".into(),
				..Default::default()
			},
		)
		.await
		.unwrap();
	assert_eq!(results.len(), 1);
	// A repeated next link is a cycle; stop without requesting it again.
	assert_eq!(server.hits("/search/"), 2);
	assert!(
		server
			.last_query("/search/")
			.unwrap()
			.contains("page=/page2")
	);
	let server = serve(vec![Route::Sequence {
		path: "/search/".into(),
		content_type: "text/html".into(),
		bodies: (2..=5)
			.map(|n| {
				format!(
					"{}<li class='next'><a href='/search/?radio=jazz&amp;page=/page{n}'>Next</a></li>",
					row(&format!("s{n}"), "Jazz", &format!("https://audio.test/{n}"))
				)
				.into_bytes()
			})
			.collect(),
	}]);
	let results = InternetRadio::new(server.base.clone())
		.search(
			&client(),
			&SearchQuery {
				name: "jazz".into(),
				..Default::default()
			},
		)
		.await
		.unwrap();
	assert_eq!(results.len(), 3);
	assert_eq!(server.hits("/search/"), 3);
}

#[tokio::test]
async fn malformed_pages_error_but_empty_results_and_empty_queries_are_safe() {
	let server = serve(vec![
		page("/", "not a directory"),
		page("/search/", "<h1>No stations found</h1>"),
	]);
	let directory = InternetRadio::new(server.base.clone());
	assert!(
		directory
			.children(&client(), "internet-radio/featured")
			.await
			.is_err()
	);
	assert!(
		directory
			.search(&client(), &SearchQuery::default())
			.await
			.unwrap()
			.is_empty()
	);
	assert_eq!(server.hits("/search/"), 0);
	assert!(
		directory
			.search(
				&client(),
				&SearchQuery {
					name: "missing".into(),
					..Default::default()
				}
			)
			.await
			.unwrap()
			.is_empty()
	);
}

#[tokio::test]
async fn missing_station_link_uses_stream_id_and_search_encodes_name() {
	let html = row("cafe", "Café &amp; Soul", "https://audio.test/live").replace(
		"<a href=\"/station/cafe/\">Café &amp; Soul</a>",
		"Café &amp; Soul",
	);
	let server = serve(vec![page("/search/", &html)]);
	let stations = InternetRadio::new(server.base.clone())
		.search(
			&client(),
			&SearchQuery {
				name: "Café & Soul".into(),
				..Default::default()
			},
		)
		.await
		.unwrap();
	assert_eq!(stations.len(), 1);
	assert_eq!(stations[0].id, "internet-radio:https://audio.test/live");
	assert_eq!(
		server.last_query("/search/").as_deref(),
		Some("radio=Caf%C3%A9+%26+Soul")
	);
}
