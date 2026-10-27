mod common;

use blindradio_core::net::client;
use blindradio_core::sources::{Endpoints, Filters, Node, SearchQuery, Sources};
use blindradio_core::station::{Source, Station};
use common::{Route, TestServer, serve};
use std::time::{Duration, Instant, UNIX_EPOCH};

fn json(path: &str, body: &str) -> Route {
	Route::Static {
		path: path.into(),
		content_type: "application/json".into(),
		body: body.as_bytes().to_vec(),
	}
}

const RB: &str = r#"[{"stationuuid":"u1","name":"Groove RB","url_resolved":"http://rb.test/1"}]"#;
const TI: &str =
	r#"{"body":[{"type":"audio","item":"station","text":"Groove TI","guide_id":"s1"}]}"#;
const IH_SEARCH: &str = r#"{"results":{"stations":[{"id":6655}]}}"#;
const IH_LIVE: &str = r#"{"total":1,"hits":[{"id":6655,"name":"Groove IH","streams":{"shoutcast_stream":"http://ih.test/1"}}]}"#;
const IH_HISTORY: &str = r#"{"data":[{"title":"Song","artist":"Band","startTime":1793122765}]}"#;
const SOMA: &str = r#"{"id":"groovesalad","songs":[{"title":"Tune","artist":"Soma","album":"A","date":"1793122407"},{"title":"  ","artist":"X","date":"1"},{"title":"Bare","artist":"","date":"x"}]}"#;

fn sources(mut routes: Vec<Route>) -> (TestServer, Sources) {
	routes.push(json("/json/stations/search", RB));
	routes.push(json("/api/v3/search/all", IH_SEARCH));
	routes.push(json("/api/v2/content/liveStations", IH_LIVE));
	let s = serve(routes);
	let e = Endpoints {
		radio_browser: s.base.clone(),
		tunein: s.base.clone(),
		iheart: s.base.clone(),
		somafm: s.base.clone(),
	};
	let src = Sources::new(client(), e);
	(s, src)
}

fn q(name: &str) -> SearchQuery {
	SearchQuery {
		name: name.into(),
		..Default::default()
	}
}

fn folder(id: &str, name: &str) -> Node {
	Node::Folder {
		id: id.into(),
		name: name.into(),
	}
}

#[test]
fn roots_in_order() {
	assert_eq!(
		Sources::roots(),
		vec![
			folder("rb", "Radio Browser"),
			folder("tunein", "TuneIn"),
			folder("iheart", "iHeartRadio"),
			folder("net", "Networks"),
		]
	);
}

#[tokio::test]
async fn children_dispatch_by_prefix() {
	let (_s, src) = sources(vec![
		json(
			"/Browse.ashx",
			r#"{"body":[{"type":"link","text":"Africa","guide_id":"r1"}]}"#,
		),
		json("/json/tags", r#"[{"name":"jazz","stationcount":5}]"#),
	]);
	assert_eq!(
		src.children("rb").await.unwrap(),
		vec![
			folder("rb/genres", "By Genre"),
			folder("rb/countries", "By Country")
		]
	);
	assert_eq!(
		src.children("tunein").await.unwrap(),
		vec![folder("tunein/r1", "Africa")]
	);
	assert_eq!(
		src.children("iheart").await.unwrap(),
		vec![
			folder("iheart/all", "All Stations"),
			folder("iheart/genres", "Genres")
		]
	);
	let nets = src.children("net").await.unwrap();
	assert_eq!(nets.first(), Some(&folder("net/bbc", "BBC")));
	assert!(matches!(
		src.children("net/somafm").await.unwrap()[0],
		Node::Station(_)
	));
	assert!(src.children("custom").await.unwrap().is_empty());
}

#[tokio::test]
async fn search_merges_in_directory_order() {
	let (_s, src) = sources(vec![json("/Search.ashx", TI)]);
	let found = src.search(&q("salad classic")).await;
	let labels: Vec<_> = found.iter().map(|s| s.source.label().to_owned()).collect();
	assert_eq!(labels, ["Radio Browser", "TuneIn", "iHeartRadio", "SomaFM"]);
	assert_eq!(found[0].name, "Groove RB");
	assert_eq!(found[3].name, "Groove Salad Classic");
}

#[tokio::test]
async fn search_dedupes_by_stream_url() {
	let rb = r#"[{"stationuuid":"u1","name":"RB copy","url_resolved":"{base}/Tune.ashx?id=s1&formats=mp3,aac,ogg,hls"}]"#;
	let (_s, src) = sources(vec![
		json("/json/stations/search", rb),
		json("/Search.ashx", TI),
	]);
	let found = src.search(&q("salad classic")).await;
	let names: Vec<_> = found.iter().map(|s| s.name.as_str()).collect();
	assert_eq!(names, ["RB copy", "Groove IH", "Groove Salad Classic"]);
}

#[tokio::test]
async fn search_survives_dead_directory() {
	let (_s, src) = sources(vec![Route::Silent {
		path: "/Search.ashx".into(),
	}]);
	let started = Instant::now();
	let found = src.search(&q("salad classic")).await;
	assert!(started.elapsed() < Duration::from_secs(9));
	let labels: Vec<_> = found.iter().map(|s| s.source.label().to_owned()).collect();
	assert_eq!(labels, ["Radio Browser", "iHeartRadio", "SomaFM"]);
}

#[tokio::test]
async fn filter_only_search_uses_radio_browser() {
	let (s, src) = sources(vec![json("/Search.ashx", TI)]);
	let query = SearchQuery {
		genre: Some("jazz".into()),
		country: Some("France".into()),
		..Default::default()
	};
	let found = src.search(&query).await;
	assert_eq!(found.len(), 1);
	assert_eq!(found[0].source, Source::RadioBrowser);
	assert_eq!(s.hits("/Search.ashx"), 0);
	assert_eq!(s.hits("/api/v3/search/all"), 0);
	let sent = s.last_query("/json/stations/search").unwrap();
	assert!(
		sent.contains("tag=jazz") && sent.contains("country=France"),
		"{sent}"
	);
}

#[tokio::test]
async fn filters_from_radio_browser() {
	let (_s, src) = sources(vec![
		json("/json/tags", r#"[{"name":"rock"},{"name":"Jazz"}]"#),
		json(
			"/json/countries",
			r#"[{"name":"France","iso_3166_1":"FR","stationcount":3}]"#,
		),
		json("/json/languages", r#"[{"name":"french"}]"#),
	]);
	assert_eq!(
		src.filters().await,
		Filters {
			genres: vec!["Jazz".into(), "rock".into()],
			countries: vec!["France".into()],
			languages: vec!["french".into()],
		}
	);
	// Nothing served: all three fail and come back empty.
	let (_s, src) = sources(vec![]);
	assert_eq!(src.filters().await, Filters::default());
}

fn station(id: &str, source: Source) -> Station {
	Station {
		id: id.into(),
		name: "S".into(),
		url: "http://x".into(),
		source,
		info: None,
	}
}

#[tokio::test]
async fn history_somafm_and_iheart() {
	let (_s, src) = sources(vec![
		json("/songs/groovesalad.json", SOMA),
		json("/api/v3/live-meta/stream/6655/trackHistory", IH_HISTORY),
	]);
	let soma = station("net:somafm:groovesalad", Source::Network("SomaFM".into()));
	let items = src.history(&soma).await.unwrap();
	assert_eq!(items.len(), 2);
	assert_eq!(items[0].title, "Soma - Tune");
	assert_eq!(
		items[0].played_at,
		Some(UNIX_EPOCH + Duration::from_secs(1793122407))
	);
	assert_eq!(items[1].title, "Bare");
	assert_eq!(items[1].played_at, None);

	let ih = station("iheart:6655", Source::IHeart);
	assert_eq!(src.history(&ih).await.unwrap()[0].title, "Band - Song");

	let rb = station("rb:u1", Source::RadioBrowser);
	assert_eq!(src.history(&rb).await, None);
	let bbc = station("net:bbc:radio-1", Source::Network("BBC".into()));
	assert_eq!(src.history(&bbc).await, None);
	// Unserved path: failure is None, not an error.
	let missing = station("net:somafm:beatblender", Source::Network("SomaFM".into()));
	assert_eq!(src.history(&missing).await, None);
}

#[tokio::test]
async fn history_found_by_url_after_dedupe() {
	let (_s, src) = sources(vec![
		json("/songs/groovesalad.json", SOMA),
		json("/api/v3/live-meta/stream/1469/trackHistory", IH_HISTORY),
	]);
	let mut rb = station("rb:u9", Source::RadioBrowser);
	rb.url = "https://stream.revma.ihrhls.com/zc1469/hls.m3u8".into();
	assert_eq!(src.history(&rb).await.unwrap()[0].title, "Band - Song");

	let mut copy = station("rb:u10", Source::RadioBrowser);
	copy.url = "https://ice1.somafm.com/groovesalad-128-mp3".into();
	assert_eq!(src.history(&copy).await.unwrap()[0].title, "Soma - Tune");
}
