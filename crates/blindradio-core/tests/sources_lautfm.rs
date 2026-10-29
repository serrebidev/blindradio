mod common;

use blindradio_core::net::client;
use blindradio_core::sources::lautfm::LautFm;
use blindradio_core::sources::{Node, SearchQuery, Sort, SortField};
use blindradio_core::station::Source;
use common::{Route, serve};

fn json_route(path: &str, body: &str) -> Route {
	Route::Static {
		path: path.into(),
		content_type: "application/json".into(),
		body: body.as_bytes().to_vec(),
	}
}

fn station(name: &str, slug: &str, genres: &str, active: bool) -> String {
	format!(
		r#"{{"name":"{slug}","display_name":"{name}","stream_url":"https://{slug}.stream.laut.fm/{slug}","location":"Berlin","genres":{genres},"active":{active}}}"#
	)
}

fn folder(id: &str, name: &str) -> Node {
	Node::Folder {
		id: id.into(),
		name: name.into(),
	}
}

#[tokio::test]
async fn root_names_by_genre_without_fetching() {
	let server = serve(vec![]);
	let laut = LautFm::new(&server.base);
	assert_eq!(
		laut.children(&client(), "lautfm", Sort::default())
			.await
			.unwrap(),
		vec![folder("lautfm/genres", "By Genre")]
	);
	assert_eq!(server.hits("/genres"), 0);
}

#[tokio::test]
async fn genres_are_folders_in_api_order() {
	let server = serve(vec![json_route(
		"/genres",
		r#"[{"name":"Pop","score":3234.0},{"name":"Drum & Bass","score":12.0}]"#,
	)]);
	let laut = LautFm::new(&server.base);
	assert_eq!(
		laut.children(&client(), "lautfm/genres", Sort::default())
			.await
			.unwrap(),
		vec![
			folder("lautfm/genre/Pop", "Pop"),
			folder("lautfm/genre/Drum%20%26%20Bass", "Drum & Bass"),
		]
	);
}

#[tokio::test]
async fn genre_page_encodes_the_genre_and_drops_dead_rows() {
	let server = serve(vec![json_route(
		"/stations/genre/Drum%20%26%20Bass",
		&format!(
			"[{},{},{}]",
			station("Alpha", "alpha", r#"["Rock"]"#, true),
			station("Dead", "dead", r#"["Rock"]"#, false),
			r#"{"name":"bad","display_name":"No Stream","stream_url":"ftp://x/y","active":true}"#
		),
	)]);
	let laut = LautFm::new(&server.base);
	let nodes = laut
		.children(&client(), "lautfm/genre/Drum%20%26%20Bass", Sort::default())
		.await
		.unwrap();
	assert_eq!(nodes.len(), 1);
	let Node::Station(s) = &nodes[0] else {
		panic!("{nodes:?}")
	};
	assert_eq!(s.id, "lautfm:alpha");
	assert_eq!(s.name, "Alpha");
	assert_eq!(s.url, "https://alpha.stream.laut.fm/alpha");
	assert_eq!(s.source, Source::LautFm);
	assert_eq!(s.info.as_deref(), Some("Berlin, Rock"));
}

#[tokio::test]
async fn genre_pages_keep_order_and_load_more_past_a_hundred() {
	let rows: Vec<_> = (0..101)
		.map(|i| {
			station(
				&format!("Station {i:03}"),
				&format!("s{i:03}"),
				r#"["Pop"]"#,
				true,
			)
		})
		.collect();
	let server = serve(vec![json_route(
		"/stations/genre/Pop",
		&format!("[{}]", rows.join(",")),
	)]);
	let laut = LautFm::new(&server.base);

	let first = laut
		.children(&client(), "lautfm/genre/Pop", Sort::default())
		.await
		.unwrap();
	assert_eq!(first.len(), 101, "100 stations and the Load More row");
	assert!(matches!(first.last(), Some(Node::LoadMore { .. })));

	let second = laut
		.children(&client(), "lautfm/genre/Pop@100", Sort::default())
		.await
		.unwrap();
	assert_eq!(second.len(), 1);
	assert!(matches!(&second[0], Node::Station(s) if s.name == "Station 100"));
}

#[tokio::test]
async fn sort_reorders_a_genre_and_reverse_flips_it() {
	let body = format!(
		"[{},{},{}]",
		station("Bravo", "bravo", r#"["Rock"]"#, true),
		station("alpha", "alpha", r#"["Jazz"]"#, true),
		station("Charlie", "charlie", r#"["Pop"]"#, true),
	);
	let server = serve(vec![json_route("/stations/genre/Pop", &body)]);
	let laut = LautFm::new(&server.base);

	async fn names(laut: &LautFm, sort: Sort) -> Vec<String> {
		laut.children(&client(), "lautfm/genre/Pop", sort)
			.await
			.unwrap()
			.into_iter()
			.filter_map(|n| match n {
				Node::Station(s) => Some(s.name),
				_ => None,
			})
			.collect()
	}

	assert_eq!(
		names(&laut, Sort::default()).await,
		["Bravo", "alpha", "Charlie"],
		"the provider's own order"
	);
	assert_eq!(
		names(
			&laut,
			Sort {
				field: SortField::Name,
				reverse: false
			}
		)
		.await,
		["alpha", "Bravo", "Charlie"]
	);
	assert_eq!(
		names(
			&laut,
			Sort {
				field: SortField::Name,
				reverse: true
			}
		)
		.await,
		["Charlie", "Bravo", "alpha"]
	);
	assert_eq!(
		names(
			&laut,
			Sort {
				field: SortField::Tags,
				reverse: false
			}
		)
		.await,
		["alpha", "Charlie", "Bravo"]
	);
}

#[tokio::test]
async fn search_encodes_the_name_and_flattens_groups() {
	let server = serve(vec![json_route(
		"/search/stations",
		&format!(
			r#"{{"total":2,"results":[{{"items":[{{"station":{}}}]}},{{"items":[{{"station":{}}}]}}]}}"#,
			station("Alpha", "alpha", r#"["Jazz"]"#, true),
			station("Bravo", "bravo", r#"["Jazz"]"#, true)
		),
	)]);
	let laut = LautFm::new(&server.base);
	let found = laut
		.search(
			&client(),
			&SearchQuery {
				name: "jazz & soul".into(),
				..Default::default()
			},
		)
		.await
		.unwrap();
	assert_eq!(
		found.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
		["Alpha", "Bravo"]
	);
	let sent = server.last_query("/search/stations").unwrap();
	assert!(sent.contains("query=jazz%20%26%20soul"), "{sent}");
}

#[tokio::test]
async fn an_empty_name_searches_nothing() {
	let server = serve(vec![]);
	let laut = LautFm::new(&server.base);
	let found = laut
		.search(
			&client(),
			&SearchQuery {
				name: "   ".into(),
				..Default::default()
			},
		)
		.await
		.unwrap();
	assert!(found.is_empty());
	assert_eq!(server.hits("/search/stations"), 0);
}

#[tokio::test]
async fn a_bad_reply_is_an_error_not_an_empty_list() {
	let server = serve(vec![Route::Static {
		path: "/genres".into(),
		content_type: "application/json".into(),
		body: b"not json".to_vec(),
	}]);
	let laut = LautFm::new(&server.base);
	assert!(
		laut.children(&client(), "lautfm/genres", Sort::default())
			.await
			.is_err()
	);
}
