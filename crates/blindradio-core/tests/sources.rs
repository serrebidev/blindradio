mod common;

use blindradio_core::library::Folder;
use blindradio_core::net::client;
use blindradio_core::sources::{Endpoints, Filters, Node, SearchQuery, Sort, Sources};
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

const RB: &str = r#"[{"stationuuid":"u1","name":"Salad Classic RB","url_resolved":"http://rb.test/1"}]"#;
const TI: &str =
	r#"{"body":[{"type":"audio","item":"station","text":"Salad Classic TI","guide_id":"s1"}]}"#;
const IH_SEARCH: &str = r#"{"results":{"stations":[{"id":6655}]}}"#;
const IH_LIVE: &str = r#"{"total":1,"hits":[{"id":6655,"name":"Salad Classic IH","streams":{"shoutcast_stream":"http://ih.test/1"}}]}"#;
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
		icecast: s.base.clone(),
		shoutcast: s.base.clone(),
		internet_radio: s.base.clone(),
		iprd: s.base.clone(),
		radionet: s.base.clone(),
		live365: s.base.clone(),
		orb: s.base.clone(),
		directory: s.base.clone(),
		recommended: s.base.clone(),
		moondust: s.base.clone(),
		triton: s.base.clone(),
		live365_api: s.base.clone(),
		laut_fm: s.base.clone(),
		bbc: s.base.clone(),
		kexp: s.base.clone(),
		radio_paradise: s.base.clone(),
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

#[tokio::test]
async fn tree_keeps_the_folder_structure() {
	let (_s, src) = sources(vec![
		json(
			"/contents",
			r#"[{"name":"Public Radio.m3u","type":"file"}]"#,
		),
		Route::Static {
			path: "/raw/Public%20Radio.m3u".into(),
			content_type: "text/plain".into(),
			body: "#EXTM3U\n#EXTINF:-1 group-title=\"NPR\",AAA\nhttp://pr.test/1\n#EXTINF:-1 group-title=\"NPR / Local\",BBB\nhttp://pr.test/2\n#EXTINF:-1,CCC\nhttp://pr.test/3\n"
				.as_bytes()
				.to_vec(),
		},
	]);
	let (stations, folders) = src.tree("brdir/Public%20Radio", Sort::default()).await.unwrap();
	let names: Vec<&str> = stations.iter().map(|s| s.name.as_str()).collect();
	assert_eq!(names, ["CCC"], "ungrouped stations stay at the top");
	assert_eq!(folders.len(), 1);
	assert_eq!(folders[0].name, "NPR");
	let names: Vec<&str> = folders[0].stations.iter().map(|s| s.name.as_str()).collect();
	assert_eq!(names, ["AAA"]);
	assert_eq!(folders[0].folders.len(), 1);
	assert_eq!(folders[0].folders[0].name, "Local");
	let names: Vec<&str> = folders[0].folders[0]
		.stations
		.iter()
		.map(|s| s.name.as_str())
		.collect();
	assert_eq!(names, ["BBB"]);
}

#[tokio::test]
async fn tree_up_to_stops_at_the_cap() {
	let (_s, src) = sources(vec![
		json(
			"/contents",
			r#"[{"name":"Public Radio.m3u","type":"file"}]"#,
		),
		Route::Static {
			path: "/raw/Public%20Radio.m3u".into(),
			content_type: "text/plain".into(),
			body: "#EXTM3U\n#EXTINF:-1 group-title=\"NPR\",AAA\nhttp://pr.test/1\n#EXTINF:-1 group-title=\"NPR\",BBB\nhttp://pr.test/2\n#EXTINF:-1,CCC\nhttp://pr.test/3\n"
				.as_bytes()
				.to_vec(),
		},
	]);
	let held = |(stations, folders): (Vec<Station>, Vec<Folder>)| {
		stations.len() + folders.iter().map(|f| f.all_stations().len()).sum::<usize>()
	};
	let id = "brdir/Public%20Radio";
	assert_eq!(held(src.tree_up_to(id, Sort::default(), 2).await.unwrap()), 2);
	assert_eq!(held(src.tree_up_to(id, Sort::default(), usize::MAX).await.unwrap()), 3);
}

#[tokio::test]
async fn recommended_reads_its_playlists_folder() {
	let (_s, src) = sources(vec![
		json(
			"/contents/playlists",
			r#"[{"name":"jazz-blues.m3u","type":"file"},{"name":"notes.txt","type":"file"}]"#,
		),
		Route::Static {
			path: "/contents/playlists/jazz-blues.m3u".into(),
			content_type: "text/plain".into(),
			body: "#EXTM3U\n#EXTINF:-1,Blue Note\nhttp://rec.test/1\n"
				.as_bytes()
				.to_vec(),
		},
	]);
	assert_eq!(
		src.children("recommended", Sort::default()).await.unwrap(),
		vec![folder("recommended/jazz-blues", "Jazz Blues")]
	);
	assert_eq!(
		src.children("recommended/jazz-blues", Sort::default())
			.await
			.unwrap(),
		vec![Node::Station(Station {
			id: "recommended:jazz-blues:0".into(),
			name: "Blue Note".into(),
			url: "http://rec.test/1".into(),
			source: Source::Recommended,
			info: None,
			details: Default::default(),
		})]
	);
}

#[tokio::test]
async fn directories_list_iprd_and_recommended_not_laut_fm() {
	let (_s, src) = sources(vec![]);
	let dirs = src.children("dir", Sort::default()).await.unwrap();
	assert!(dirs.contains(&folder("iprd", "IPRD")));
	assert!(dirs.contains(&folder("recommended", "Recommended Radio Streams")));
	assert!(!dirs.iter().any(|n| n.name() == "laut.fm"));
}

#[tokio::test]
async fn folders_turned_off_leave_browse_and_search() {
	let (_s, src) = sources(vec![]);
	let net = blindradio_core::sources::networks::networks()
		.iter()
		.find(|n| n.stations.len() > 1)
		.unwrap();
	let net_id = format!("net/{}", net.slug);
	let query = SearchQuery {
		name: net.stations[0].name.clone(),
		..Default::default()
	};
	let station_id = format!("net:{}:{}", net.slug, net.stations[0].slug);
	let found = |list: &[Station]| list.iter().any(|s| s.id == station_id);
	let listed = |list: &[Node], id: &str| {
		list.iter()
			.any(|n| matches!(n, Node::Folder { id: got, .. } if got == id))
	};
	assert!(found(&src.search(&query).await));

	src.set_disabled([String::from("tunein"), net_id.clone()].into());
	let sort = Sort::default();
	assert!(!listed(&src.children("dir", sort).await.unwrap(), "tunein"));
	assert!(listed(&src.children("dir", sort).await.unwrap(), "rb"));
	assert!(src.children("tunein", sort).await.unwrap().is_empty());
	assert!(!listed(&src.children("net", sort).await.unwrap(), &net_id));
	assert!(src.children(&net_id, sort).await.unwrap().is_empty());
	assert!(!found(&src.search(&query).await), "the network leaves search");

	src.set_disabled(Default::default());
	assert!(listed(&src.children("net", sort).await.unwrap(), &net_id));
	assert!(!src.children(&net_id, sort).await.unwrap().is_empty());
}

#[test]
fn a_saved_laut_fm_station_still_loads() {
	let s: Station =
		serde_json::from_str(r#"{"id":"lautfm:a","name":"A","url":"http://x","source":"LautFm"}"#)
			.unwrap();
	assert_eq!(s.source, Source::LautFm);
}

#[test]
fn roots_in_order() {
	assert_eq!(
		Sources::roots(),
		vec![folder("dir", "Directories"), folder("net", "Networks")]
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
		json(
			"/contents",
			r#"[{"name":"UK.m3u","type":"file"},{"name":"Public Radio.m3u","type":"file"},{"name":"README.md","type":"file"},{"name":"extra","type":"dir"}]"#,
		),
		Route::Static {
			path: "/raw/Public%20Radio.m3u".into(),
			content_type: "text/plain".into(),
			body: "#EXTM3U\n#EXTINF:-1 group-title=\"NPR\",WNYC\nhttp://pr.test/1\n"
				.as_bytes()
				.to_vec(),
		},
	]);
	assert_eq!(
		src.children("rb", Sort::default()).await.unwrap(),
		vec![
			folder("rb/genres", "By Genre"),
			folder("rb/countries", "By Country"),
			folder("rb/languages", "By Language"),
			folder("rb/states", "By State"),
			folder("rb/top", "Top Stations")
		]
	);
	assert_eq!(
		src.children("tunein/r0", Sort::default()).await.unwrap(),
		vec![folder("tunein/r1", "Africa")]
	);
	assert_eq!(
		src.children("iheart", Sort::default()).await.unwrap(),
		vec![
			folder("iheart/all", "All Stations"),
			folder("iheart/genres", "Genres"),
			folder("iheart/markets", "By City")
		]
	);
	assert_eq!(
		src.children("brdir", Sort::default()).await.unwrap(),
		vec![
			folder("brdir/Public%20Radio", "Public Radio"),
			folder("brdir/UK", "UK")
		]
	);
	assert_eq!(
		src.children("brdir/Public%20Radio", Sort::default())
			.await
			.unwrap(),
		vec![folder("brdir/Public%20Radio/NPR", "NPR")]
	);
	assert_eq!(
		src.children("brdir/Public%20Radio/NPR", Sort::default())
			.await
			.unwrap(),
		vec![Node::Station(Station {
			id: "brdir:Public Radio:0".into(),
			name: "WNYC".into(),
			url: "http://pr.test/1".into(),
			source: Source::Directory,
			info: Some("NPR".into()),
			details: Default::default(),
		})]
	);
	let nets = src.children("net", Sort::default()).await.unwrap();
	let names: Vec<&str> = nets.iter().map(|n| n.name()).collect();
	let mut sorted = names.clone();
	sorted.sort_by_key(|a| a.to_lowercase());
	assert_eq!(names, sorted, "networks in name order");
	assert!(nets.contains(&folder("net/bbc", "BBC")));
	assert!(matches!(
		src.children("net/somafm", Sort::default()).await.unwrap()[0],
		Node::Station(_)
	));
	assert!(
		src.children("custom", Sort::default())
			.await
			.unwrap()
			.is_empty()
	);
}

#[tokio::test]
async fn search_merges_in_directory_order() {
	let (_s, src) = sources(vec![json("/Search.ashx", TI)]);
	let found = src.search(&q("salad classic")).await;
	let labels: Vec<_> = found.iter().map(|s| s.source.label().to_owned()).collect();
	assert_eq!(labels, ["Radio Browser", "TuneIn", "iHeartRadio", "SomaFM"]);
	assert_eq!(found[0].name, "Salad Classic RB");
	assert_eq!(found[3].name, "Groove Salad Classic");
}

#[tokio::test]
async fn search_drops_what_directories_guessed() {
	let (_s, src) = sources(vec![
		json("/json/stations/search", "[]"),
		json(
			"/Search.ashx",
			r#"{"body":[
{"type":"audio","item":"station","text":"730 CKNW","guide_id":"s1"},
{"type":"audio","item":"station","text":"CerebroDigital","subtext":"cknw fans","guide_id":"s2"},
{"text":"Search results (not playable)","children":[
{"type":"audio","item":"station","text":"CKNW - Not Supported","key":"unavailable","guide_id":"s3"}]}]}"#,
		),
		json(
			"/api/v3/search/all",
			r#"{"results":{"stations":[{"id":1},{"id":2},{"id":3}]}}"#,
		),
		json(
			"/api/v2/content/liveStations",
			r#"{"total":3,"hits":[
{"id":1,"name":"KFI AM 640","callLetters":"KFI-AM","genres":[{"name":"News & Talk"}],"streams":{"shoutcast_stream":"http://ih.test/1"}},
{"id":2,"name":"Z100","callLetters":"CKNW-FM","genres":[{"name":"Top 40"}],"streams":{"shoutcast_stream":"http://ih.test/2"}},
{"id":3,"name":"Smooth","callLetters":"KSMO-FM","genres":[{"name":"CKNW"}],"streams":{"shoutcast_stream":"http://ih.test/3"}}]}"#,
		),
		json(
			"/stations/search",
			r#"{"totalCount":2,"playables":[
{"id":"a","name":"CKNW AM980","streams":[{"url":"http://rn.test/a","status":"VALID"}]},
{"id":"b","name":"CKNB 95","streams":[{"url":"http://rn.test/b","status":"VALID"}]}]}"#,
		),
		json(
			"/Search/UpdateSearch",
			r#"[{"ID":7,"Name":"BlacknWhita","Format":"audio/mpeg","Bitrate":128,"Genre":"Pop","Listeners":1}]"#,
		),
	]);
	let found = src.search(&q("cknw")).await;
	let names: Vec<_> = found.iter().map(|s| s.name.as_str()).collect();
	assert_eq!(names, ["730 CKNW", "CKNW", "Z100", "Smooth", "CKNW AM980"]);
	assert_eq!(found[2].details.call_sign.as_deref(), Some("CKNW-FM"));
	assert_eq!(found[2].details.genre.as_deref(), Some("Top 40"));
}

#[tokio::test]
async fn search_includes_new_directories_in_order() {
	let (_s, src) = sources(vec![
		json(
			"/json/stations/search",
			r#"[{"stationuuid":"u1","name":"New directory RB","url_resolved":"http://rb.test/1"}]"#,
		),
		json(
			"/Search.ashx",
			r#"{"body":[{"type":"audio","item":"station","text":"New directory TI","guide_id":"s1"}]}"#,
		),
		json(
			"/api/v2/content/liveStations",
			r#"{"total":1,"hits":[{"id":6655,"name":"New directory IH","streams":{"shoutcast_stream":"http://ih.test/1"}}]}"#,
		),
		Route::Static {
			path: "/yp.xml".into(),
			content_type: "application/xml".into(),
			body: br#"<directory><entry><server_name>New directory Icecast</server_name><server_type>audio/mpeg</server_type><listen_url>https://ice.test/live</listen_url><genre>jazz</genre></entry></directory>"#.to_vec(),
		},
		json("/Search/UpdateSearch", r#"[{"ID":42,"Name":"New directory SHOUTcast","Format":"audio/mpeg","Bitrate":128,"Genre":"jazz","Listeners":0}]"#),
		Route::Static {
			path: "/search/".into(),
			content_type: "text/html".into(),
			body: br#"<h1>Search results</h1><table><tr><td><a href="/servers/tools/playlistgenerator/?u=https%3A%2F%2Fir.test%2Flive&amp;t=.pls">PLS</a></td><td><h4><a href="/station/new-directory/">New directory Internet-Radio</a></h4><a href="/stations/jazz/">jazz</a></td></tr></table>"#.to_vec(),
		},
		json(
			"/site_data/metadata/catalog.json",
			r#"{"stations":[{"id":"nd","name":"New directory IPRD","country":"Chile","genres":["Pop"],"streams":[{"url":"https://iprd.test/nd"}]}]}"#,
		),
	]);
	let found = src.search(&q("New directory")).await;
	let labels: Vec<_> = found.iter().map(|s| s.source.label()).collect();
	assert_eq!(
		labels,
		[
			"Radio Browser",
			"TuneIn",
			"iHeartRadio",
			"Icecast",
			"SHOUTcast",
			"Internet-Radio",
			"IPRD"
		]
	);
}

#[tokio::test]
async fn search_dedupes_by_stream_url() {
	let rb = r#"[{"stationuuid":"u1","name":"Salad Classic copy","url_resolved":"{base}/Tune.ashx?id=s1&formats=mp3,aac,ogg,hls"}]"#;
	let (_s, src) = sources(vec![
		json("/json/stations/search", rb),
		json("/Search.ashx", TI),
	]);
	let found = src.search(&q("salad classic")).await;
	let names: Vec<_> = found.iter().map(|s| s.name.as_str()).collect();
	assert_eq!(names, ["Salad Classic copy", "Salad Classic IH", "Groove Salad Classic"]);
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
	assert_eq!(s.hits("/yp.xml"), 0);
	assert_eq!(s.hits("/Search/UpdateSearch"), 0);
	assert_eq!(s.hits("/search/"), 0);
	assert_eq!(s.hits("/site_data/metadata/catalog.json"), 0);
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
		details: Default::default(),
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

fn on(url: &str) -> Station {
	let mut st = station("rb:u1", Source::RadioBrowser);
	url.clone_into(&mut st.url);
	st
}

#[tokio::test]
async fn open_history_is_found_from_the_stream_address() {
	let triton = r#"<nowplaying-info-list><nowplaying-info type="track"><property name="cue_time_start"><![CDATA[2000000]]></property><property name="cue_title"><![CDATA[Song]]></property><property name="track_artist_name"><![CDATA[Band]]></property></nowplaying-info></nowplaying-info-list>"#;
	let (s, src) = sources(vec![
		json("/public/nowplaying", triton),
		json(
			"/station/a1",
			r#"{"current-track":{"title":"Now","artist":"A","start":"2026-11-04 08:05:16.45+00:00"},"last-played":[{"title":"Then","artist":"B","start":"2026-11-04 08:00:00+00:00"}]}"#,
		),
		json(
			"/station/eins/last_songs",
			r#"[{"title":"Lights","artist":{"name":"John"},"started_at":"2026-11-04 08:51:19 +0100"}]"#,
		),
		json(
			"/api/nowplaying/hits",
			r#"{"now_playing":{"played_at":300,"song":{"artist":"A","title":"Now","text":"A - Now"}},"song_history":[{"played_at":100,"song":{"artist":"","title":"","text":"Only Text"}}]}"#,
		),
		json(
			"/v2/services/bbc_6music/segments/latest",
			r#"{"data":[{"segment_type":"music","titles":{"primary":"Suede","secondary":"Trash"},"offset":{"label":"Now Playing"}},{"segment_type":"speech","titles":{"primary":"News","secondary":"Now"},"offset":{"label":"1 Minute Ago"}}]}"#,
		),
		json(
			"/v2/plays/",
			r#"{"results":[{"play_type":"airbreak","artist":null,"song":null,"airdate":"2026-11-04T00:10:00-08:00"},{"play_type":"trackplay","artist":"Puma Blue","song":"Croak Dream","airdate":"2026-11-04T00:07:32-08:00"}]}"#,
		),
		json(
			"/api/nowplaying_list_v2022",
			r#"{"song":[{"artist":"Lips","title":"Superman","play_time":1793779723550}]}"#,
		),
	]);
	let first = |items: Option<Vec<_>>| {
		let items: Vec<blindradio_core::sources::HistoryItem> = items.unwrap();
		items[0].title.clone()
	};
	let stw = "https://playerservices.streamtheworld.com/api/livestream-redirect/977_HITS.mp3";
	assert_eq!(first(src.history(&on(stw)).await), "Band - Song");
	assert!(s.last_query("/public/nowplaying").unwrap().contains("mountName=977_HITS"));

	let live = src.history(&on("https://streaming.live365.com/a1")).await.unwrap();
	assert_eq!(live.iter().map(|i| i.title.as_str()).collect::<Vec<_>>(), ["A - Now", "B - Then"]);
	assert_eq!(
		first(src.history(&on("https://eins.stream.laut.fm/eins")).await),
		"John - Lights"
	);
	let azura = src.history(&on(&format!("{}/listen/hits/radio.mp3", s.base))).await.unwrap();
	assert_eq!(azura[0].title, "A - Now");
	assert_eq!(azura[1].title, "Only Text");
	let bbc = src
		.history(&on("http://x.akamaized.net/live/ww/bbc_6music/bbc_6music.isml/a.m3u8"))
		.await
		.unwrap();
	assert_eq!(bbc.len(), 1, "speech is not a song");
	assert_eq!(bbc[0].title, "Suede - Trash");
	assert_eq!(
		first(src.history(&on("https://kexp-mp3-128.streamguys1.com/kexp128.mp3")).await),
		"Puma Blue - Croak Dream"
	);
	assert_eq!(
		first(src.history(&on("https://stream.radioparadise.com/aac-128")).await),
		"Lips - Superman"
	);
	assert!(s.last_query("/api/nowplaying_list_v2022").unwrap().contains("chan=0"));
}

const ORB_ROW: &str = r#"<ul><li class="stations__station"><ul class="stations__station__tags"><li><a>jazz</a></li></ul><ul class="stations__station__info"><li><a class="i-flag us" title="United States"></a></li></ul><button class="b-play station_play" stream="http://orb.test/salad" streamType="mp3" radioId="us.salad" radioName="Salad Classic ORB"></button></li></ul>"#;

fn html(path: &str, body: &str) -> Route {
	Route::Static {
		path: path.into(),
		content_type: "text/html".into(),
		body: body.as_bytes().to_vec(),
	}
}

#[tokio::test]
async fn orb_browse_and_search() {
	let list = format!(
		r#"{ORB_ROW}<dl class="pagination"><dd><a href="https://x/ca/?p=1">2</a></dd></dl>"#
	);
	let (s, src) = sources(vec![
		html("/genres/", r#"<div id="tags"><a href="/genre/jazz/">jazz</a></div>"#),
		html(
			"/Oceania/",
			r#"<ul class="countries__countries-list"><li><a href="/as/">American Samoa</a></li></ul>"#,
		),
		html("/ca/", &list),
		html("/genre/jazz/", ORB_ROW),
		html("/search", ORB_ROW),
	]);
	assert!(
		src.children("dir", Sort::default())
			.await
			.unwrap()
			.contains(&folder("orb", "Online Radio Box"))
	);
	let root = src.children("orb", Sort::default()).await.unwrap();
	assert_eq!(root[0], folder("orb/genres", "By Genre"));
	assert!(root.contains(&folder("orb/region/Oceania", "Oceania")));
	assert_eq!(
		src.children("orb/genres", Sort::default()).await.unwrap(),
		[folder("orb/genre/jazz", "jazz")]
	);
	assert_eq!(
		src.children("orb/region/Oceania", Sort::default()).await.unwrap(),
		[folder("orb/country/as", "American Samoa")]
	);
	let country = src.children("orb/country/ca", Sort::default()).await.unwrap();
	assert!(matches!(&country[0], Node::Station(st) if st.id == "orb:us.salad"));
	assert_eq!(
		country[1],
		Node::LoadMore {
			id: "orb/country/ca@1".into()
		}
	);
	let last = src.children("orb/country/ca@1", Sort::default()).await.unwrap();
	assert!(s.last_query("/ca/").unwrap().contains("p=1"));
	assert!(!last.iter().any(|n| matches!(n, Node::LoadMore { .. })), "{last:?}");
	assert_eq!(
		src.children("orb/genre/jazz", Sort::default()).await.unwrap().len(),
		1
	);
	let found = src.search(&q("salad classic")).await;
	assert!(found.iter().any(|st| st.source == Source::OnlineRadioBox), "{found:?}");
}

#[tokio::test]
async fn orb_carries_history_on_from_the_stations_own_list() {
	let page = r#"<div class="station-onair__localtime"><nobr>(<a>now in Here</a> 12:00)</nobr></div><table class="tablelist-schedule"><tr><td class="tablelist-schedule__time">Live</td><td class="track_history_item">Now - Live</td></tr></table>"#;
	let older = r#"<table class="tablelist-schedule"><tr><td class="tablelist-schedule__time">12:00</td><td class="track_history_item">Old - Song</td></tr></table>"#;
	let triton = format!(
		r#"<nowplaying-info-list><nowplaying-info type="track"><property name="cue_time_start"><![CDATA[{}000]]></property><property name="cue_title"><![CDATA[Song]]></property><property name="track_artist_name"><![CDATA[Band]]></property></nowplaying-info></nowplaying-info-list>"#,
		std::time::SystemTime::now()
			.duration_since(UNIX_EPOCH)
			.unwrap()
			.as_secs()
			- 60
	);
	let mut routes = vec![
		html("/search", ORB_ROW),
		json("/public/nowplaying", &triton),
		html("/us/salad/playlist/", page),
	];
	for back in 1..8 {
		routes.push(html(&format!("/us/salad/playlist/{back}"), older));
	}
	let (_s, src) = sources(routes);

	// Found by its stream, so the name does not have to match.
	let alone = src.history(&on("http://orb.test/salad")).await.unwrap();
	assert_eq!(alone.len(), 8);
	assert_eq!(alone[0].title, "Now - Live");
	assert_eq!(alone[1].title, "Old - Song");
	assert!(alone[1].played_at.is_some());

	// A station with no match gets nothing from it.
	assert_eq!(src.history(&on("http://elsewhere.test/x")).await, None);

	// By its own id, after a list of its own: only what is older than that list.
	let mut both = on("https://playerservices.streamtheworld.com/api/livestream-redirect/M.mp3");
	both.id = "orb:us.salad".into();
	let joined = src.history(&both).await.unwrap();
	assert_eq!(joined[0].title, "Band - Song");
	assert!(joined.iter().skip(1).all(|i| i.title == "Old - Song"), "{joined:?}");
	assert_eq!(joined.len(), 8, "the live row is newer than the list and drops out");
}

const RN_LIST: &str = r#"{"totalCount":2,"playables":[{"id":"one","name":"One FM","genres":["Pop"],"city":"Berlin","country":"DE","streams":[{"url":"http://rn.test/1","status":"VALID"}]},{"id":"two","name":"Two FM","genres":[],"city":"","country":"","streams":[{"url":"","status":"INVALID"}]}]}"#;

/// A Live365 page: its data is a flat list in `enqueue` calls, as on the real site.
fn live365_page(flat: &str) -> Route {
	let quoted = serde_json::to_string(&format!("{flat}\n")).unwrap();
	Route::Static {
		path: "unset".into(),
		content_type: "text/html".into(),
		body: format!("<script>x.enqueue({quoted});</script>").into_bytes(),
	}
}

fn at(path: &str, route: Route) -> Route {
	let Route::Static {
		content_type, body, ..
	} = route
	else {
		panic!("static route expected")
	};
	Route::Static {
		path: path.into(),
		content_type,
		body,
	}
}

#[tokio::test]
async fn live365_browse_and_search() {
	const GENRES: &str = r#"[{"_1":2},"loaderData",{"_3":4},"route",{"_5":6},"genres",[7],{"_8":9},"name","Urban and R&B"]"#;
	// One sub-genre, one station.
	const LIST: &str = r#"[{"_1":2},"loaderData",{"_3":4},"route",{"_5":6,"_7":8},"allStations",[9],"relatedGenres",[14],{"_10":11,"_12":13},"name","Salad Classic Live","mountId","a1",{"_10":15},"Acid Jazz"]"#;
	const FOUND: &str = r#"[{"_1":2},"loaderData",{"_3":4},"route",{"_5":6},"results",[7],{"_8":9,"_10":11},"name","Salad Classic Live","mountId","a1"]"#;
	let (s, src) = sources(vec![
		at("/listen/genres", live365_page(GENRES)),
		at("/listen/jazz-music-radio", live365_page(LIST)),
		at("/listen/search", live365_page(FOUND)),
	]);
	assert_eq!(
		src.children("live365", Sort::default()).await.unwrap(),
		[folder("live365/genre/urban-and-rnb-music-radio", "Urban and R&B")]
	);
	let kids = src
		.children("live365/genre/jazz-music-radio", Sort::default())
		.await
		.unwrap();
	assert_eq!(kids[0], folder("live365/sub/acid-jazz-music-radio", "Acid Jazz"));
	let Node::Station(st) = &kids[1] else {
		panic!("{kids:?}")
	};
	assert_eq!(st.id, "live365:a1");
	assert_eq!(st.url, "https://streaming.live365.com/a1");
	assert_eq!(st.source, Source::Live365);
	// A sub-genre does not list its sisters, or the tree would never end.
	let sub = src
		.children("live365/sub/jazz-music-radio", Sort::default())
		.await
		.unwrap();
	assert_eq!(sub.len(), 1, "{sub:?}");
	let found = src.search(&q("salad classic")).await;
	assert!(found.iter().any(|s| s.source == Source::Live365), "{found:?}");
	assert!(s.last_query("/listen/search").unwrap().contains("query=salad%20classic"));
}

#[tokio::test]
async fn radionet_tags_open_and_page() {
	let tags = r#"{"genres":[{"name":"Rock","slug":"rock"},{"name":"jazz","slug":"jazz"},{"name":"","slug":"x"}],"countries":[],"languages":[],"topics":[]}"#;
	let (s, src) = sources(vec![
		json("/stations/tags", tags),
		json("/stations/by-tag", RN_LIST),
	]);
	let kids = src
		.children("radionet/tags/genres", Sort::default())
		.await
		.unwrap();
	assert_eq!(
		kids,
		[
			folder("radionet/tag/genres/jazz", "jazz"),
			folder("radionet/tag/genres/rock", "Rock")
		]
	);
	assert!(
		src.children("radionet/tags/other", Sort::default())
			.await
			.unwrap()
			.is_empty()
	);
	// Two rows sent, one without a stream, 2 of 2: the list ends here.
	let page = src
		.children("radionet/tag/genres/jazz", Sort::default())
		.await
		.unwrap();
	assert_eq!(page.len(), 1, "{page:?}");
	assert_eq!(page[0].name(), "One FM");
	let q = s.last_query("/stations/by-tag").unwrap();
	assert!(q.contains("tagType=genres") && q.contains("slug=jazz"), "{q}");
	let more = r#"{"totalCount":5,"playables":[{"id":"one","name":"One FM","streams":[{"url":"http://rn.test/1","status":"VALID"}]}]}"#;
	let (s, src) = sources(vec![json("/stations/by-tag", more)]);
	let page = src
		.children("radionet/tag/genres/jazz@2", Sort::default())
		.await
		.unwrap();
	assert_eq!(
		page.last(),
		Some(&Node::LoadMore {
			id: "radionet/tag/genres/jazz@3".into()
		})
	);
	assert!(s.last_query("/stations/by-tag").unwrap().contains("offset=2"));
}

#[tokio::test]
async fn radionet_browse_and_search() {
	let (_s, src) = sources(vec![
		json("/stations/list-by-system-name", RN_LIST),
		json("/stations/search", RN_LIST),
	]);
	let kids = src.children("radionet", Sort::default()).await.unwrap();
	let names: Vec<&str> = kids.iter().map(|n| n.name()).collect();
	assert_eq!(
		names,
		[
			"Top Stations",
			"Local Stations",
			"By Genre",
			"By Country",
			"By Language",
			"By Topic"
		]
	);
	let top = src.children("radionet/top", Sort::default()).await.unwrap();
	assert_eq!(top.len(), 1, "the streamless entry is skipped");
	assert_eq!(top[0].name(), "One FM");
	let hits = src.search(&q("one")).await;
	assert!(
		hits.iter().any(|s| s.name == "One FM" && s.source == Source::RadioNet),
		"search reaches radio.net"
	);
}
