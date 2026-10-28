mod common;

use blindradio_core::net::client;
use blindradio_core::sources::radio_browser::RadioBrowser;
use blindradio_core::sources::{Node, SearchQuery, Sort, SortField};
use blindradio_core::station::Source;
use common::{Route, TestServer, serve};

const TAGS: &str = include_str!("fixtures/sources/rb_tags.json");
const COUNTRIES: &str = include_str!("fixtures/sources/rb_countries.json");
const STATES_US: &str = include_str!("fixtures/sources/rb_states_us.json");
const PAGE: &str = include_str!("fixtures/sources/rb_stations_page.json");
const SHORT: &str = include_str!("fixtures/sources/rb_stations_short.json");
const SEARCH: &str = include_str!("fixtures/sources/rb_search.json");

fn json(path: &str, body: &str) -> Route {
	Route::Static {
		path: path.into(),
		content_type: "application/json".into(),
		body: body.as_bytes().to_vec(),
	}
}

fn rb(routes: Vec<Route>) -> (TestServer, RadioBrowser) {
	let s = serve(routes);
	let rb = RadioBrowser::new(s.base.clone());
	(s, rb)
}

fn names(nodes: &[Node]) -> Vec<&str> {
	nodes
		.iter()
		.map(|n| match n {
			Node::Folder { name, .. } => name.as_str(),
			Node::Station(s) => s.name.as_str(),
			Node::LoadMore { .. } => "<more>",
		})
		.collect()
}

#[tokio::test]
async fn rb_root_has_genre_and_country() {
	let (_s, rb) = rb(vec![]);
	let nodes = rb.children(&client(), "rb", Sort::default()).await.unwrap();
	assert_eq!(names(&nodes), ["By Genre", "By Country"]);
}

#[tokio::test]
async fn rb_genres_list_folders() {
	let (_s, rb) = rb(vec![json("/json/tags", TAGS)]);
	let nodes = rb
		.children(&client(), "rb/genres", Sort::default())
		.await
		.unwrap();
	assert_eq!(nodes.len(), 200);
	assert!(matches!(&nodes[0], Node::Folder { id, .. } if id.starts_with("rb/genre/")));
}

#[tokio::test]
async fn rb_big_country_splits_by_state() {
	let (_s, rb) = rb(vec![
		json("/json/countries", COUNTRIES),
		json(
			"/json/states/The%20United%20States%20Of%20America/",
			STATES_US,
		),
	]);
	let nodes = rb
		.children(&client(), "rb/country/US", Sort::default())
		.await
		.unwrap();
	assert_eq!(names(&nodes)[0], "All stations");
	assert!(matches!(&nodes[0], Node::Folder { id, .. } if id == "rb/country/US/all"));
	assert!(names(&nodes).contains(&"California"));
	assert!(nodes.len() > 2);
}

#[tokio::test]
async fn rb_small_country_lists_stations() {
	let (_s, rb) = rb(vec![
		json("/json/countries", COUNTRIES),
		json("/json/stations/bycountrycodeexact/LI", SHORT),
	]);
	let nodes = rb
		.children(&client(), "rb/country/LI", Sort::default())
		.await
		.unwrap();
	assert_eq!(nodes.len(), 3);
	assert!(nodes.iter().all(|n| matches!(n, Node::Station(_))));
}

#[tokio::test]
async fn rb_full_page_ends_with_load_more() {
	let (s, rb) = rb(vec![json("/json/stations/bytagexact/jazz", PAGE)]);
	let nodes = rb
		.children(&client(), "rb/genre/jazz", Sort::default())
		.await
		.unwrap();
	assert_eq!(nodes.len(), 101);
	assert_eq!(
		nodes.last(),
		Some(&Node::LoadMore {
			id: "rb/genre/jazz@100".into()
		})
	);
	let q = s.last_query("/json/stations/bytagexact/jazz").unwrap();
	assert!(q.contains("offset=0") && q.contains("limit=100"), "{q}");
	let more = rb
		.children(&client(), "rb/genre/jazz@100", Sort::default())
		.await
		.unwrap();
	assert!(matches!(more.last(), Some(Node::LoadMore { id }) if id == "rb/genre/jazz@200"));
	let q = s.last_query("/json/stations/bytagexact/jazz").unwrap();
	assert!(q.contains("offset=100"), "{q}");
}

#[tokio::test]
async fn rb_load_more_ends() {
	let (_s, rb) = rb(vec![json("/json/stations/bytagexact/jazz", "[]")]);
	let nodes = rb
		.children(&client(), "rb/genre/jazz@200", Sort::default())
		.await
		.unwrap();
	assert!(nodes.is_empty());
}

#[tokio::test]
async fn rb_state_keeps_exact_matches_only() {
	let (s, rb) = rb(vec![json("/json/stations/search", PAGE)]);
	let nodes = rb
		.children(&client(), "rb/country/US/state/New%20York", Sort::default())
		.await
		.unwrap();
	// The fixture's first station is in "New York NY": a substring hit, not an exact one.
	assert!(
		nodes
			.iter()
			.all(|n| !matches!(n, Node::Station(s) if s.name == "Classic Vinyl HD"))
	);
	// A full raw page still offers more even when filtering emptied it, after the page cap.
	assert_eq!(
		nodes.last(),
		Some(&Node::LoadMore {
			id: "rb/country/US/state/New%20York@500".into()
		})
	);
	let q = s.last_query("/json/stations/search").unwrap();
	assert!(
		q.contains("state=New%20York") && q.contains("countrycode=US"),
		"{q}"
	);
}

#[tokio::test]
async fn rb_station_names_cleaned() {
	let (_s, rb) = rb(vec![json("/json/stations/bytagexact/x", SHORT)]);
	let nodes = rb
		.children(&client(), "rb/genre/x", Sort::default())
		.await
		.unwrap();
	assert_eq!(names(&nodes), ["A & B", "Spaced", "stream.example.org"]);
}

#[tokio::test]
async fn rb_station_info_and_id() {
	let (_s, rb) = rb(vec![json("/json/stations/bytagexact/x", SHORT)]);
	let nodes = rb
		.children(&client(), "rb/genre/x", Sort::default())
		.await
		.unwrap();
	let (Node::Station(a), Node::Station(b)) = (&nodes[0], &nodes[1]) else {
		panic!("stations expected");
	};
	assert!(a.id.starts_with("rb:") && a.source == Source::RadioBrowser);
	assert_eq!(a.info, None);
	assert_eq!(b.info.as_deref(), Some("Jazz, France, AAC 128k"));
}

#[tokio::test]
async fn rb_search_sends_only_given_filters() {
	let (s, rb) = rb(vec![json("/json/stations/search", SEARCH)]);
	let q = SearchQuery {
		name: "jazz fm".into(),
		genre: Some("smooth jazz".into()),
		country: None,
		language: Some(String::new()),
		sort: Sort::default(),
	};
	let found = rb.search(&client(), &q).await.unwrap();
	assert_eq!(found.len(), 100);
	let sent = s.last_query("/json/stations/search").unwrap();
	assert!(sent.contains("name=jazz%20fm"), "{sent}");
	assert!(sent.contains("tag=smooth%20jazz"), "{sent}");
	assert!(!sent.contains("country="), "{sent}");
	assert!(!sent.contains("language="), "{sent}");
}

#[tokio::test]
async fn rb_filter_lists_sorted_by_name() {
	let (_s, rb) = rb(vec![
		json("/json/tags", TAGS),
		json("/json/countries", COUNTRIES),
		json(
			"/json/languages",
			r#"[{"name":"french"},{"name":"Arabic"},{"name":""}]"#,
		),
	]);
	let c = client();
	let genres = rb.genres(&c).await.unwrap();
	assert_eq!(genres.len(), 200);
	assert!(
		genres
			.windows(2)
			.all(|w| w[0].to_lowercase() <= w[1].to_lowercase())
	);
	assert!(!rb.countries(&c).await.unwrap().is_empty());
	assert_eq!(rb.languages(&c).await.unwrap(), ["Arabic", "french"]);
}

fn exact_state_page() -> String {
	r#"[{"stationuuid":"ex","name":"Exact","url_resolved":"http://x/1","state":"New York"},
	{"stationuuid":"","name":"No id","url_resolved":"http://x/2","state":"New York"},
	{"stationuuid":"nu","name":"No url","state":"New York"}]"#
		.into()
}

#[tokio::test]
async fn rb_state_keeps_exact_name_and_skips_unusable() {
	let (_s, rb) = rb(vec![json("/json/stations/search", &exact_state_page())]);
	let nodes = rb
		.children(&client(), "rb/country/US/state/New%20York", Sort::default())
		.await
		.unwrap();
	assert_eq!(names(&nodes), ["Exact"]);
}

#[tokio::test]
async fn rb_state_skips_empty_full_pages() {
	let seq = Route::Sequence {
		path: "/json/stations/search".into(),
		content_type: "application/json".into(),
		bodies: vec![
			PAGE.as_bytes().to_vec(),
			PAGE.as_bytes().to_vec(),
			exact_state_page().into_bytes(),
		],
	};
	let (s, rb) = rb(vec![seq]);
	let nodes = rb
		.children(&client(), "rb/country/US/state/New%20York", Sort::default())
		.await
		.unwrap();
	assert_eq!(names(&nodes), ["Exact"]);
	assert_eq!(s.hits("/json/stations/search"), 3);
	assert!(
		s.last_query("/json/stations/search")
			.unwrap()
			.contains("offset=200")
	);
}

#[tokio::test]
async fn rb_state_gives_up_after_five_pages() {
	let (s, rb) = rb(vec![json("/json/stations/search", PAGE)]);
	let nodes = rb
		.children(&client(), "rb/country/US/state/New%20York", Sort::default())
		.await
		.unwrap();
	assert_eq!(s.hits("/json/stations/search"), 5);
	assert_eq!(
		nodes.last(),
		Some(&Node::LoadMore {
			id: "rb/country/US/state/New%20York@500".into()
		})
	);
}

#[tokio::test]
async fn rb_browse_uses_chosen_sort() {
	let (s, rb) = rb(vec![json("/json/stations/bytagexact/jazz", PAGE)]);
	let sort = Sort {
		field: SortField::Bitrate,
		reverse: true,
	};
	rb.children(&client(), "rb/genre/jazz", sort).await.unwrap();
	let q = s.last_query("/json/stations/bytagexact/jazz").unwrap();
	assert!(q.contains("order=bitrate&reverse=true"), "{q}");
	assert!(q.contains("hidebroken=true"), "{q}");
}

#[tokio::test]
async fn rb_browse_api_default_sort_sends_no_order() {
	let (s, rb) = rb(vec![json("/json/stations/bytagexact/jazz", PAGE)]);
	let sort = Sort {
		field: SortField::Default,
		reverse: false,
	};
	rb.children(&client(), "rb/genre/jazz", sort).await.unwrap();
	let q = s.last_query("/json/stations/bytagexact/jazz").unwrap();
	assert!(!q.contains("order="), "{q}");
}

#[tokio::test]
async fn rb_search_uses_chosen_sort() {
	let (s, rb) = rb(vec![json("/json/stations/search", SEARCH)]);
	let q = SearchQuery {
		name: "jazz".into(),
		sort: Sort {
			field: SortField::Name,
			reverse: false,
		},
		..Default::default()
	};
	rb.search(&client(), &q).await.unwrap();
	let sent = s.last_query("/json/stations/search").unwrap();
	assert!(sent.contains("order=name&reverse=false"), "{sent}");
}
