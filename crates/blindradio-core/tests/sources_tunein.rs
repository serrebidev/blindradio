mod common;

use blindradio_core::net::client;
use blindradio_core::sources::tunein::TuneIn;
use blindradio_core::sources::{Node, SearchQuery};
use blindradio_core::station::Source;
use common::{Route, TestServer, serve};

const ROOT: &str = include_str!("fixtures/sources/tunein_root.json");
const CONTINENT: &str = include_str!("fixtures/sources/tunein_continent.json");
const CITY: &str = include_str!("fixtures/sources/tunein_country.json");
const SEARCH: &str = include_str!("fixtures/sources/tunein_search.json");
const MIXED: &str = r#"{"head":{"status":"200"},"body":[
{"element":"outline","type":"link","text":"Shows","guide_id":"c100000"},
{"element":"outline","type":"link","text":"No id"},
{"element":"outline","type":"audio","item":"show","text":"A show","guide_id":"p1"},
{"element":"outline","type":"audio","item":"station","text":"Odd","guide_id":"t5"},
{"element":"outline","type":"audio","item":"station","text":"No id"},
{"element":"outline","type":"text","text":"Note"},
{"element":"outline","text":"Group","key":"shows","children":[
{"element":"outline","type":"audio","item":"station","text":"Nested","guide_id":"s7","subtext":"Hi"}]}]}"#;

fn tunein(path: &str, body: &str) -> (TestServer, TuneIn) {
	let s = serve(vec![Route::Static {
		path: path.into(),
		content_type: "application/json".into(),
		body: body.as_bytes().to_vec(),
	}]);
	let t = TuneIn::new(s.base.clone());
	(s, t)
}

#[tokio::test]
async fn tunein_location_folders() {
	let (s, t) = tunein("/Browse.ashx", ROOT);
	let nodes = t.children(&client(), "tunein").await.unwrap();
	assert_eq!(
		nodes[0],
		Node::Folder {
			id: "tunein/r101215".into(),
			name: "Africa".into()
		}
	);
	assert!(s.last_query("/Browse.ashx").unwrap().contains("id=r0"));

	let (s, t) = tunein("/Browse.ashx", CONTINENT);
	let nodes = t.children(&client(), "tunein/r101215").await.unwrap();
	assert!(matches!(&nodes[0], Node::Folder { id, .. } if id.starts_with("tunein/r")));
	let q = s.last_query("/Browse.ashx").unwrap();
	assert!(q.contains("id=r101215") && q.contains("render=json"), "{q}");
}

#[tokio::test]
async fn tunein_city_stations_flattened() {
	let (_s, t) = tunein("/Browse.ashx", CITY);
	let nodes = t.children(&client(), "tunein/r100289").await.unwrap();
	let stations = nodes
		.iter()
		.filter(|n| matches!(n, Node::Station(_)))
		.count();
	assert_eq!(stations, 9);
	let Node::Station(st) = &nodes[0] else {
		panic!("station expected");
	};
	assert_eq!(st.id, "tunein:s361052");
	assert_eq!(st.source, Source::TuneIn);
	assert_eq!(
		st.info.as_deref(),
		Some("Radio El Bahdja est la radio préférée des Algériens")
	);
}

#[tokio::test]
async fn tunein_station_url_uses_tune() {
	let (s, t) = tunein("/Browse.ashx", CITY);
	let nodes = t.children(&client(), "tunein/r100289").await.unwrap();
	let Node::Station(st) = &nodes[0] else {
		panic!("station expected");
	};
	assert_eq!(
		st.url,
		format!("{}/Tune.ashx?id=s361052&formats=mp3,aac,ogg,hls", s.base)
	);
}

#[tokio::test]
async fn tunein_skips_non_stations() {
	let (_s, t) = tunein("/Browse.ashx", MIXED);
	let nodes = t.children(&client(), "tunein/r1").await.unwrap();
	assert_eq!(nodes.len(), 1, "{nodes:?}");
	assert!(matches!(&nodes[0], Node::Station(s) if s.name == "Nested" && s.id == "tunein:s7"));
	let (_s, t) = tunein("/Search.ashx", SEARCH);
	let q = SearchQuery {
		name: "jazz".into(),
		..Default::default()
	};
	let found = t.search(&client(), &q).await.unwrap();
	assert_eq!(found.len(), 84);
	assert!(found.iter().all(|s| s.id.starts_with("tunein:s")));
}

#[tokio::test]
async fn tunein_search_ignores_filters() {
	let (s, t) = tunein("/Search.ashx", SEARCH);
	let q = SearchQuery {
		name: "jazz fm".into(),
		genre: Some("blues".into()),
		country: Some("France".into()),
		language: Some("french".into()),
		sort: blindradio_core::sources::Sort::default(),
	};
	t.search(&client(), &q).await.unwrap();
	let sent = s.last_query("/Search.ashx").unwrap();
	assert!(sent.contains("query=jazz%20fm"), "{sent}");
	assert!(
		sent.contains("types=station") && sent.contains("render=json"),
		"{sent}"
	);
	assert!(
		!sent.contains("blues") && !sent.contains("France") && !sent.contains("french"),
		"{sent}"
	);
}
