mod common;

use blindradio_core::net::client;
use blindradio_core::sources::iprd::Iprd;
use blindradio_core::sources::{Node, SearchQuery, Sort, SortField};
use blindradio_core::station::Source;
use common::{Route, serve};

const PATH: &str = "/site_data/metadata/catalog.json";

fn catalog(body: &str) -> Route {
	Route::Static {
		path: PATH.into(),
		content_type: "application/json".into(),
		body: body.as_bytes().to_vec(),
	}
}

fn station(name: &str, country: &str, genres: &str, url: &str) -> String {
	format!(
		r#"{{"id":"x-{name}","name":"{name}","country":"{country}","language":["English"],"genres":{genres},"website":"https://{name}.test","streams":[{{"url":"{url}","format":"MP3","bitrate":128,"reliability":0.95}}],"tags":[],"source":"streams/x/x.m3u"}}"#
	)
}

fn stations(rows: &[String]) -> String {
	format!(r#"{{"version":"1.0","stations":[{}]}}"#, rows.join(","))
}

fn folder(id: &str, name: &str) -> Node {
	Node::Folder {
		id: id.into(),
		name: name.into(),
	}
}

fn names(nodes: &[Node]) -> Vec<&str> {
	nodes.iter().map(|n| n.name()).collect()
}

#[tokio::test]
async fn root_names_its_branches_without_fetching() {
	let server = serve(vec![]);
	let iprd = Iprd::new(&server.base);
	assert_eq!(
		iprd.children(&client(), "iprd", Sort::default())
			.await
			.unwrap(),
		vec![
			folder("iprd/countries", "By Country"),
			folder("iprd/genres", "By Genre")
		]
	);
	assert_eq!(server.hits(PATH), 0);
}

#[tokio::test]
async fn countries_and_genres_are_folders_and_one_download_serves_all() {
	let server = serve(vec![catalog(&stations(&[
		station("One", "New Zealand", r#"["Pop","News"]"#, "http://a.test/1"),
		station("Two", "Andorra", r#"["pop"]"#, "http://a.test/2"),
	]))]);
	let iprd = Iprd::new(&server.base);
	assert_eq!(
		iprd.children(&client(), "iprd/countries", Sort::default())
			.await
			.unwrap(),
		vec![
			folder("iprd/country/Andorra", "Andorra"),
			folder("iprd/country/New%20Zealand", "New Zealand"),
		]
	);
	assert_eq!(
		iprd.children(&client(), "iprd/genres", Sort::default())
			.await
			.unwrap(),
		vec![
			folder("iprd/genre/news", "News"),
			folder("iprd/genre/pop", "Pop"),
		],
		"genres differing only by case are one folder"
	);
	assert_eq!(server.hits(PATH), 1);
}

#[tokio::test]
async fn a_folder_lists_its_stations_and_drops_bad_rows() {
	let server = serve(vec![catalog(&stations(&[
		station("Bravo", "New Zealand", r#"["Pop"]"#, "http://a.test/b"),
		station("alpha", "New Zealand", r#"["Jazz"]"#, "http://a.test/a"),
		station("Elsewhere", "Andorra", r#"["Pop"]"#, "http://a.test/e"),
		station("Copy", "New Zealand", r#"["Pop"]"#, "http://a.test/a"),
		station("Ftp", "New Zealand", r#"["Pop"]"#, "ftp://a.test/f"),
		station(
			"Login",
			"New Zealand",
			r#"["Pop"]"#,
			"http://me:pw@a.test/l",
		),
	]))]);
	let iprd = Iprd::new(&server.base);
	let nodes = iprd
		.children(&client(), "iprd/country/New%20Zealand", Sort::default())
		.await
		.unwrap();
	assert_eq!(
		names(&nodes),
		["alpha", "Bravo"],
		"name order, one per stream"
	);
	let Node::Station(s) = &nodes[0] else {
		panic!("{nodes:?}")
	};
	assert_eq!(s.id, "iprd:x-alpha");
	assert_eq!(s.url, "http://a.test/a");
	assert_eq!(s.source, Source::Iprd);
	assert_eq!(s.info.as_deref(), Some("Jazz, New Zealand, MP3 128k"));
	assert_eq!(s.details.country.as_deref(), Some("New Zealand"));
	assert_eq!(s.details.language.as_deref(), Some("English"));
	assert_eq!(s.details.homepage.as_deref(), Some("https://alpha.test"));
	assert_eq!(s.details.bitrate_kbps, Some(128));

	let nodes = iprd
		.children(&client(), "iprd/genre/pop", Sort::default())
		.await
		.unwrap();
	assert_eq!(names(&nodes), ["Bravo", "Elsewhere"]);
}

#[tokio::test]
async fn an_unknown_format_and_no_bitrate_say_nothing() {
	let server = serve(vec![catalog(
		r#"{"stations":[{"id":"q","name":"Quiet","country":"Chile","genres":["Talk"],"streams":[{"url":"http://a.test/q","format":"Unknown","bitrate":0}]}]}"#,
	)]);
	let iprd = Iprd::new(&server.base);
	let nodes = iprd
		.children(&client(), "iprd/country/Chile", Sort::default())
		.await
		.unwrap();
	let Node::Station(s) = &nodes[0] else {
		panic!("{nodes:?}")
	};
	assert_eq!(s.info.as_deref(), Some("Talk, Chile"));
	assert_eq!(s.details.codec, None);
	assert_eq!(s.details.bitrate_kbps, None);
}

#[tokio::test]
async fn pages_load_more_past_a_hundred() {
	let rows: Vec<_> = (0..101)
		.map(|i| {
			station(
				&format!("Station {i:03}"),
				"Chile",
				r#"["Pop"]"#,
				&format!("http://a.test/{i}"),
			)
		})
		.collect();
	let server = serve(vec![catalog(&stations(&rows))]);
	let iprd = Iprd::new(&server.base);
	let first = iprd
		.children(&client(), "iprd/country/Chile", Sort::default())
		.await
		.unwrap();
	assert_eq!(first.len(), 101, "100 stations and the Load More row");
	assert_eq!(
		first.last(),
		Some(&Node::LoadMore {
			id: "iprd/country/Chile@100".into()
		})
	);
	let second = iprd
		.children(&client(), "iprd/country/Chile@100", Sort::default())
		.await
		.unwrap();
	assert_eq!(names(&second), ["Station 100"]);
}

#[tokio::test]
async fn a_sort_reorders_and_reverse_flips() {
	let server = serve(vec![catalog(&stations(&[
		station("Bravo", "Chile", r#"["Pop"]"#, "http://a.test/b"),
		station("alpha", "Chile", r#"["Jazz"]"#, "http://a.test/a"),
	]))]);
	let iprd = Iprd::new(&server.base);
	let nodes = iprd
		.children(
			&client(),
			"iprd/country/Chile",
			Sort {
				field: SortField::Name,
				reverse: true,
			},
		)
		.await
		.unwrap();
	assert_eq!(names(&nodes), ["Bravo", "alpha"]);
}

#[tokio::test]
async fn search_matches_the_name_and_caps_the_list() {
	let mut rows: Vec<_> = (0..150)
		.map(|i| {
			station(
				&format!("Jazz {i:03}"),
				"Chile",
				r#"["Jazz"]"#,
				&format!("http://a.test/{i}"),
			)
		})
		.collect();
	rows.push(station("Other", "Chile", r#"["Jazz"]"#, "http://a.test/o"));
	let server = serve(vec![catalog(&stations(&rows))]);
	let iprd = Iprd::new(&server.base);
	let query = |name: &str| SearchQuery {
		name: name.into(),
		..Default::default()
	};
	let found = iprd.search(&client(), &query(" jAzZ ")).await.unwrap();
	assert_eq!(found.len(), 100);
	assert!(found.iter().all(|s| s.name.starts_with("Jazz")));
	assert!(
		iprd.search(&client(), &query("  "))
			.await
			.unwrap()
			.is_empty()
	);
}

#[tokio::test]
async fn a_bad_reply_is_an_error_not_an_empty_list() {
	let server = serve(vec![catalog("not json")]);
	let iprd = Iprd::new(&server.base);
	assert!(
		iprd.children(&client(), "iprd/genres", Sort::default())
			.await
			.is_err()
	);
}
