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
	let nodes = t.children(&client(), "tunein/r0").await.unwrap();
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
async fn tunein_describes_a_station_it_will_not_play() {
	// Shape of MS NOW's real reply: nulls and blanks are common.
	let (s, t) = tunein(
		"/Describe.ashx",
		r#"{"head":{"status":"200"},"body":[{"element":"station","guide_id":"s297990",
		"name":"MS NOW","slogan":"Breaking News","url":"","location":"US","language":"English",
		"genre_name":"Progressive Talk","region_id":null,"current_song":null,"is_available":false}]}"#,
	);
	let got = t.describe(&client(), "s297990").await.unwrap();
	assert_eq!(got.slogan.as_deref(), Some("Breaking News"));
	assert_eq!(got.location.as_deref(), Some("US"));
	assert_eq!(got.language.as_deref(), Some("English"));
	assert_eq!(got.genre.as_deref(), Some("Progressive Talk"));
	assert_eq!(got.homepage, None, "a blank is nothing");
	let q = s.last_query("/Describe.ashx").unwrap();
	assert!(q.contains("id=s297990") && q.contains("render=json"), "{q}");

	// A null field, and a station it does not know: nothing to add, not an error.
	let (_s, t) = tunein("/Describe.ashx", r#"{"body":[{"slogan":null,"url":"http://x"}]}"#);
	assert_eq!(t.describe(&client(), "s1").await.unwrap().homepage.as_deref(), Some("http://x"));
	let (_s, t) = tunein(
		"/Describe.ashx",
		r#"{"head":{"status":"400","fault":"Station not found"},"body":[]}"#,
	);
	assert!(t.describe(&client(), "s1").await.unwrap().is_empty());
}

#[tokio::test]
async fn tunein_root_needs_no_request() {
	let t = TuneIn::new("http://127.0.0.1:0");
	let nodes = t.children(&client(), "tunein").await.unwrap();
	let names: Vec<_> = nodes.iter().map(Node::name).collect();
	assert_eq!(
		names,
		["Local Radio", "Music", "Talk", "Sports", "By Location"]
	);
}

#[tokio::test]
async fn tunein_follows_categories_and_more_stations() {
	const GENRE: &str = r#"{"body":[
{"text":"Local Stations","key":"local","children":[
{"type":"audio","item":"station","text":"Here","guide_id":"s1"}]},
{"text":"Stations","key":"stations","children":[
{"type":"audio","item":"station","text":"Here","guide_id":"s1"},
{"type":"audio","item":"station","text":"There","guide_id":"s2"},
{"type":"link","text":"More Stations","key":"nextStations","URL":"http://opml.radiotime.com/Browse.ashx?offset=26&id=c57954&filter=s"}]},
{"text":"Shows","key":"shows","children":[
{"type":"link","text":"More Shows","key":"nextShows","URL":"http://opml.radiotime.com/Browse.ashx?offset=6&id=c57954&filter=p"},
{"type":"link","text":"Recent Episodes","URL":"http://opml.radiotime.com/Browse.ashx?id=c57954&filter=p:topic"}]},
{"text":"Explore","key":"related","children":[
{"type":"link","text":"Jazz","guide_id":"g22","URL":"http://opml.radiotime.com/Browse.ashx?id=g22"},
{"type":"link","text":"Elsewhere","URL":"http://opml.radiotime.com/Other.ashx?id=g22"}]}]}"#;
	let (s, t) = tunein("/Browse.ashx", GENRE);
	let nodes = t.children(&client(), "tunein/c=music").await.unwrap();
	let q = s.last_query("/Browse.ashx").unwrap();
	assert!(q.contains("c=music") && q.contains("render=json"), "{q}");
	assert_eq!(nodes.len(), 4, "{nodes:?}");
	assert_eq!(nodes[0].name(), "Here");
	assert_eq!(nodes[1].name(), "There");
	assert_eq!(
		nodes[2],
		Node::LoadMore {
			id: "tunein/offset=26&id=c57954&filter=s".into()
		}
	);
	assert_eq!(
		nodes[3],
		Node::Folder {
			id: "tunein/id=g22".into(),
			name: "Jazz".into()
		}
	);
	t.children(&client(), "tunein/offset=26&id=c57954&filter=s")
		.await
		.unwrap();
	let q = s.last_query("/Browse.ashx").unwrap();
	assert!(q.contains("offset=26") && q.contains("id=c57954"), "{q}");
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
async fn tunein_keeps_what_it_calls_unavailable_and_says_which_formats_we_play() {
	const SEARCHED: &str = r#"{"body":[{"text":"Search results (not playable)","children":[
{"type":"audio","item":"station","text":"MS NOW - Not Supported","subtext":"Not available in your country","key":"unavailable","guide_id":"s297990"},
{"type":"audio","item":"station","text":"Open - Not Supported Yet","subtext":"A slogan","guide_id":"s2"}]}]}"#;
	let (s, t) = tunein("/Search.ashx", SEARCHED);
	let q = SearchQuery {
		name: "ms now".into(),
		..Default::default()
	};
	let found = t.search(&client(), &q).await.unwrap();
	let sent = s.last_query("/Search.ashx").unwrap();
	assert!(sent.contains("formats=mp3,aac,ogg,hls"), "{sent}");
	assert_eq!(found[0].name, "MS NOW");
	assert_eq!(found[0].info.as_deref(), Some("Not available in your country"));
	assert_eq!(
		found[0].details.unavailable.as_deref(),
		Some("Not available in your country")
	);
	assert_eq!(found[1].name, "Open - Not Supported Yet");
	assert_eq!(found[1].details.unavailable, None, "a slogan is no reason");

	let (s, t) = tunein("/Browse.ashx", SEARCHED);
	t.children(&client(), "tunein/c=talk").await.unwrap();
	let sent = s.last_query("/Browse.ashx").unwrap();
	assert!(sent.contains("formats=mp3,aac,ogg,hls"), "{sent}");
	let next = "tunein/offset=26&id=c1&formats=mp3,aac,ogg,hls&filter=s";
	t.children(&client(), next).await.unwrap();
	let sent = s.last_query("/Browse.ashx").unwrap();
	assert_eq!(sent.matches("formats=").count(), 1, "{sent}");
}

#[tokio::test]
async fn tuning_a_refused_station_fails_instead_of_playing_a_refusal() {
	// What Tune.ashx really answers for a station blocked here.
	let s = serve(vec![Route::Static {
		path: "/Tune.ashx".into(),
		content_type: "audio/x-mpegurl".into(),
		body: b"#STATUS: 400\n".to_vec(),
	}]);
	assert!(blindradio_core::net::open(&client(), &s.url("/Tune.ashx")).await.is_err());
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
