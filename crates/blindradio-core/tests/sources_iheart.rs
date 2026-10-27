mod common;

use blindradio_core::net::client;
use blindradio_core::sources::iheart::IHeart;
use blindradio_core::sources::{Node, SearchQuery};
use blindradio_core::station::Source;
use common::{Route, TestServer, serve};
use std::time::{Duration, UNIX_EPOCH};

const GENRES: &str = include_str!("fixtures/sources/iheart_genres.json");
const PAGE: &str = include_str!("fixtures/sources/iheart_live_page.json");
const LAST: &str = include_str!("fixtures/sources/iheart_live_last.json");
const SEARCH: &str = include_str!("fixtures/sources/iheart_search.json");
const LOOKUP: &str = include_str!("fixtures/sources/iheart_lookup.json");
const HISTORY: &str = include_str!("fixtures/sources/iheart_history.json");

fn json(path: &str, body: &str) -> Route {
	Route::Static {
		path: path.into(),
		content_type: "application/json".into(),
		body: body.as_bytes().to_vec(),
	}
}

fn iheart(routes: Vec<Route>) -> (TestServer, IHeart) {
	let s = serve(routes);
	let i = IHeart::new(s.base.clone());
	(s, i)
}

const LIVE: &str = "/api/v2/content/liveStations";

fn folder(id: &str, name: &str) -> Node {
	Node::Folder {
		id: id.into(),
		name: name.into(),
	}
}

#[tokio::test]
async fn iheart_root() {
	let (_s, i) = iheart(vec![]);
	let nodes = i.children(&client(), "iheart").await.unwrap();
	assert_eq!(
		nodes,
		vec![
			folder("iheart/all", "All Stations"),
			folder("iheart/genres", "Genres")
		]
	);
}

#[tokio::test]
async fn iheart_genres_sorted() {
	let (s, i) = iheart(vec![json("/api/v2/content/genre", GENRES)]);
	let nodes = i.children(&client(), "iheart/genres").await.unwrap();
	assert_eq!(nodes.len(), 18);
	assert_eq!(nodes[0], folder("iheart/genre/16", "Top 40 & Pop"));
	assert!(
		s.last_query("/api/v2/content/genre")
			.unwrap()
			.contains("limit=100")
	);

	let unsorted =
		r#"{"total":2,"hits":[{"id":2,"name":"B","sort":9},{"id":1,"name":"A","sort":3}]}"#;
	let (_s, i) = iheart(vec![json("/api/v2/content/genre", unsorted)]);
	let nodes = i.children(&client(), "iheart/genres").await.unwrap();
	assert_eq!(
		nodes,
		vec![folder("iheart/genre/1", "A"), folder("iheart/genre/2", "B")]
	);
}

#[tokio::test]
async fn iheart_pages_until_total() {
	let (s, i) = iheart(vec![json(LIVE, PAGE)]);
	let nodes = i.children(&client(), "iheart/all").await.unwrap();
	// 86 of the 100 have a stream, plus Load More.
	assert_eq!(nodes.len(), 87);
	assert_eq!(
		nodes[86],
		Node::LoadMore {
			id: "iheart/all@100".into()
		}
	);
	let Node::Station(st) = &nodes[0] else {
		panic!("station expected");
	};
	assert_eq!(st.id, "iheart:1477");
	assert_eq!(st.name, "106.7 Lite FM");
	assert_eq!(st.source, Source::IHeart);
	assert_eq!(st.info.as_deref(), Some("106.7 FM, New York"));
	let q = s.last_query(LIVE).unwrap();
	assert!(
		q.contains("countryCode=US") && q.contains("limit=100") && q.contains("offset=0"),
		"{q}"
	);
	assert!(!q.contains("genreId"), "{q}");

	// 3600 + 100 < 3647 is false for the page at 3600: last page, no Load More.
	let (s, i) = iheart(vec![json(LIVE, LAST)]);
	let nodes = i.children(&client(), "iheart/all@3600").await.unwrap();
	assert!(
		nodes.iter().all(|n| matches!(n, Node::Station(_))),
		"{nodes:?}"
	);
	assert!(s.last_query(LIVE).unwrap().contains("offset=3600"));

	let (s, i) = iheart(vec![json(LIVE, PAGE)]);
	let nodes = i.children(&client(), "iheart/genre/13@200").await.unwrap();
	assert_eq!(
		nodes.last(),
		Some(&Node::LoadMore {
			id: "iheart/genre/13@300".into()
		})
	);
	let q = s.last_query(LIVE).unwrap();
	assert!(q.contains("genreId=13") && q.contains("offset=200"), "{q}");
}

#[tokio::test]
async fn iheart_prefers_icy_stream() {
	let body = r#"{"total":4,"hits":[
{"id":1,"name":"All","streams":{"hls_stream":"http://h","shoutcast_stream":"http://s","secure_hls_stream":"https://sh","secure_shoutcast_stream":"https://ss"}},
{"id":2,"name":"Hls","streams":{"hls_stream":"http://h","secure_hls_stream":"https://sh"}},
{"id":3,"name":"None","streams":{}},
{"id":4,"name":"Bare"}]}"#;
	let (_s, i) = iheart(vec![json(LIVE, body)]);
	let nodes = i.children(&client(), "iheart/all").await.unwrap();
	let urls: Vec<_> = nodes
		.iter()
		.map(|n| match n {
			Node::Station(s) => (s.id.as_str(), s.url.as_str(), s.info.clone()),
			n => panic!("{n:?}"),
		})
		.collect();
	assert_eq!(
		urls,
		vec![
			("iheart:1", "https://ss", None),
			("iheart:2", "https://sh", None)
		]
	);
}

#[tokio::test]
async fn iheart_search_looks_up_streams() {
	let (s, i) = iheart(vec![json("/api/v3/search/all", SEARCH), json(LIVE, LOOKUP)]);
	let q = SearchQuery {
		name: "jazz fm".into(),
		genre: Some("blues".into()),
		..Default::default()
	};
	let found = i.search(&client(), &q).await.unwrap();
	let sent = s.last_query("/api/v3/search/all").unwrap();
	assert!(
		sent.contains("keywords=jazz%20fm") && sent.contains("maxRows=50"),
		"{sent}"
	);
	assert!(
		sent.contains("station=true") && sent.contains("podcast=false"),
		"{sent}"
	);
	assert!(!sent.contains("blues"), "{sent}");
	let lookup = s.last_query(LIVE).unwrap();
	assert!(lookup.contains("id=4242,7079,9918"), "{lookup}");
	assert_eq!(s.hits("/api/v3/search/all") + s.hits(LIVE), 2);

	// Search order, not lookup order; stations the lookup lacks are dropped.
	let ids: Vec<_> = found.iter().map(|s| s.id.as_str()).collect();
	assert_eq!(found.len(), 45);
	let order = ["4242", "7079", "9918", "7224"];
	let positions: Vec<_> = order
		.iter()
		.filter_map(|o| ids.iter().position(|i| *i == format!("iheart:{o}")))
		.collect();
	assert!(positions.windows(2).all(|w| w[0] < w[1]), "{positions:?}");
	assert!(
		found
			.iter()
			.all(|s| s.source == Source::IHeart && !s.url.is_empty())
	);
}

#[tokio::test]
async fn iheart_search_empty_skips_lookup() {
	let (s, i) = iheart(vec![json(
		"/api/v3/search/all",
		r#"{"results":{"stations":[]}}"#,
	)]);
	let q = SearchQuery {
		name: "zzz".into(),
		..Default::default()
	};
	assert!(i.search(&client(), &q).await.unwrap().is_empty());
	assert_eq!(s.hits(LIVE), 0);
}

#[tokio::test]
async fn iheart_history_titles_and_times() {
	let path = "/api/v3/live-meta/stream/1477/trackHistory";
	let (s, i) = iheart(vec![json(path, HISTORY)]);
	let items = i.history(&client(), "iheart:1477").await.unwrap();
	assert!(s.last_query(path).unwrap().contains("limit=20"));
	assert_eq!(
		items[0].title,
		"Backstreet Boys - Everybody (Backstreet's Back)"
	);
	assert_eq!(
		items[0].played_at,
		Some(UNIX_EPOCH + Duration::from_secs(1793122765))
	);

	let odd = r#"{"data":[{"title":"Solo","startTime":5},{"artist":"No title"},{"title":"T","artist":"A"}]}"#;
	let (_s, i) = iheart(vec![json(path, odd)]);
	let items = i.history(&client(), "1477").await.unwrap();
	assert_eq!(items.len(), 2);
	assert_eq!(items[0].title, "Solo");
	assert_eq!(items[1].title, "A - T");
	assert_eq!(items[1].played_at, None);
}
