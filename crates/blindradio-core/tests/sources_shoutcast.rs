mod common;

use blindradio_core::sources::shoutcast::Shoutcast;
use blindradio_core::sources::{Node, SearchQuery, Sort, SortField};
use blindradio_core::station::Source;
use common::{Route, serve};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn response(path: &str, body: &str) -> Route {
	Route::Static {
		path: path.into(),
		content_type: "application/json".into(),
		body: body.as_bytes().to_vec(),
	}
}

fn station(id: u64) -> serde_json::Value {
	serde_json::json!({"ID":id,"Name":format!("Station {id}"),"Format":"audio/mpeg","Bitrate":128,"Genre":"Jazz","Listeners":42})
}

async fn post_server(
	path: &'static str,
	body: String,
) -> (Shoutcast, tokio::task::JoinHandle<String>) {
	let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
	let source = Shoutcast::new(format!("http://{}", listener.local_addr().unwrap()));
	let task = tokio::spawn(async move {
		let (mut socket, _) = listener.accept().await.unwrap();
		let mut request = Vec::new();
		let mut buffer = [0; 4096];
		let header_end = loop {
			let read = socket.read(&mut buffer).await.unwrap();
			assert_ne!(read, 0);
			request.extend_from_slice(&buffer[..read]);
			if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
				break end + 4;
			}
		};
		let headers = String::from_utf8_lossy(&request[..header_end]);
		assert!(
			headers.starts_with(&format!("POST {path} HTTP/1.1\r\n")),
			"{headers}"
		);
		assert!(
			headers
				.to_lowercase()
				.contains("content-type: application/x-www-form-urlencoded")
		);
		let length: usize = headers
			.lines()
			.filter_map(|line| line.split_once(':'))
			.find(|(key, _)| key.eq_ignore_ascii_case("content-length"))
			.map_or(0, |(_, value)| value.trim().parse().unwrap());
		while request.len() < header_end + length {
			let read = socket.read(&mut buffer).await.unwrap();
			assert_ne!(read, 0);
			request.extend_from_slice(&buffer[..read]);
		}
		let reply = format!(
			"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
			body.len()
		);
		socket.write_all(reply.as_bytes()).await.unwrap();
		String::from_utf8(request[header_end..header_end + length].to_vec()).unwrap()
	});
	(source, task)
}

#[tokio::test]
async fn sorts_by_every_field_it_publishes() {
	let body = serde_json::json!([
		{"ID":1,"Name":"Alpha","Format":"audio/aac","Bitrate":64,"Genre":"Rock","Listeners":5},
		{"ID":2,"Name":"Beta","Format":"audio/mpeg","Bitrate":192,"Genre":"Jazz","Listeners":900},
		{"ID":3,"Name":"gamma","Format":"audio/mpeg","Bitrate":128,"Genre":"Rock","Listeners":50},
	])
	.to_string();
	let server = serve(vec![response("/Home/Top", &body)]);
	let source = Shoutcast::new(&server.base);

	async fn names(source: &Shoutcast, sort: Sort) -> Vec<String> {
		source
			.children(&reqwest::Client::new(), "shoutcast/top", sort)
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
		names(&source, sort(SortField::Name, false)).await,
		["Alpha", "Beta", "gamma"]
	);
	assert_eq!(
		names(&source, sort(SortField::Bitrate, true)).await,
		["Beta", "gamma", "Alpha"]
	);
	assert_eq!(
		names(&source, sort(SortField::Codec, false)).await,
		["Alpha", "Beta", "gamma"]
	);
	assert_eq!(
		names(&source, sort(SortField::Tags, false)).await,
		["Beta", "Alpha", "gamma"]
	);
	// Listeners stand in for the play counts Radio Browser sorts by.
	assert_eq!(
		names(&source, sort(SortField::ClickCount, true)).await,
		["Beta", "gamma", "Alpha"]
	);
	assert_eq!(
		names(&source, sort(SortField::Votes, false)).await,
		["Alpha", "gamma", "Beta"]
	);
	// The provider's own order.
	assert_eq!(
		names(&source, sort(SortField::Default, false)).await,
		["Alpha", "Beta", "gamma"]
	);
	// A field SHOUTcast does not publish keeps the provider's order.
	assert_eq!(
		names(&source, sort(SortField::Country, true)).await,
		["Alpha", "Beta", "gamma"]
	);
}

#[tokio::test]
async fn roots_and_top_station_mapping() {
	let (source, request) =
		post_server("/Home/Top", serde_json::json!([station(7)]).to_string()).await;
	let client = reqwest::Client::new();
	assert_eq!(
		source
			.children(&client, "shoutcast", Sort::default())
			.await
			.unwrap(),
		vec![
			Node::Folder {
				id: "shoutcast/top".into(),
				name: "Top Stations".into()
			},
			Node::Folder {
				id: "shoutcast/genres".into(),
				name: "By Genre".into()
			},
		]
	);
	let nodes = source
		.children(&client, "shoutcast/top", Sort::default())
		.await
		.unwrap();
	assert_eq!(request.await.unwrap(), "");
	let Node::Station(station) = &nodes[0] else {
		panic!("station expected")
	};
	assert_eq!(station.id, "shoutcast:7");
	assert_eq!(station.name, "Station 7");
	assert_eq!(station.source, Source::Shoutcast);
	assert!(station.url.ends_with("/sbin/tunein-station.pls?id=7"));
	assert_eq!(
		station.info.as_deref(),
		Some("Jazz, MP3 128k, 42 listeners")
	);
}

#[tokio::test]
async fn genres_use_safe_same_site_links_and_form_encoding() {
	let server = serve(vec![response(
		"/",
		r#"<a href="/Genre?name=R%26B+%40+%E6%97%A5%E6%9C%AC">R&amp;B</a>
<a href="/Genre?name=R%26B+%40+%E6%97%A5%E6%9C%AC">duplicate</a>
<a href="https://evil.test/Genre?name=Bad">Bad</a><a href="/Genre?name=">empty</a>"#,
	)]);
	let client = reqwest::Client::new();
	let nodes = Shoutcast::new(server.base)
		.children(&client, "shoutcast/genres", Sort::default())
		.await
		.unwrap();
	assert_eq!(nodes.len(), 1);
	let Node::Folder { id, name } = &nodes[0] else {
		panic!("genre expected")
	};
	assert_eq!(name, "R&B @ 日本");
	assert!(!id.contains('@'));
	let (source, request) = post_server("/Home/BrowseByGenre", "[]".into()).await;
	assert!(
		source
			.children(&client, id, Sort::default())
			.await
			.unwrap()
			.is_empty()
	);
	assert_eq!(
		request.await.unwrap(),
		"genrename=R%26B+%40+%E6%97%A5%E6%9C%AC"
	);
}

#[tokio::test]
async fn pages_preserve_order_and_do_not_drop_boundary_entries() {
	let body = serde_json::to_string(&(1..=101).map(station).collect::<Vec<_>>()).unwrap();
	let server = serve(vec![response("/Home/Top", &body)]);
	let source = Shoutcast::new(server.base);
	let client = reqwest::Client::new();
	let nodes = source
		.children(&client, "shoutcast/top", Sort::default())
		.await
		.unwrap();
	assert_eq!(nodes.len(), 101);
	assert_eq!(
		nodes[100],
		Node::LoadMore {
			id: "shoutcast/top@100".into()
		}
	);
	let page = source
		.children(&client, "shoutcast/top@100", Sort::default())
		.await
		.unwrap();
	assert_eq!(page.len(), 1);
	assert!(matches!(&page[0], Node::Station(s) if s.id == "shoutcast:101"));
	assert!(
		source
			.children(
				&client,
				"shoutcast/top@18446744073709551615",
				Sort::default()
			)
			.await
			.unwrap()
			.is_empty()
	);
}

#[tokio::test]
async fn search_posts_name_and_ignores_filters() {
	let (source, request) = post_server(
		"/Search/UpdateSearch",
		serde_json::json!([station(8)]).to_string(),
	)
	.await;
	let query = SearchQuery {
		name: " jazz & 日本 ".into(),
		country: Some("US".into()),
		..Default::default()
	};
	let stations = source
		.search(&reqwest::Client::new(), &query)
		.await
		.unwrap();
	assert_eq!(stations.len(), 1);
	assert_eq!(request.await.unwrap(), "query=jazz+%26+%E6%97%A5%E6%9C%AC");
}

#[tokio::test]
async fn malformed_replies_fail_and_invalid_stations_are_omitted() {
	for body in [
		"not json",
		"{}",
		r#"[{"ID":"bad","Name":"Wrong"}]"#,
		r#"[{"ID":1}]"#,
	] {
		let server = serve(vec![response("/Home/Top", body)]);
		assert!(
			Shoutcast::new(server.base)
				.children(&reqwest::Client::new(), "shoutcast/top", Sort::default())
				.await
				.is_err(),
			"{body}"
		);
	}
	let mut empty = station(2);
	empty["Name"] = " ".into();
	let mut invalid_format = station(3);
	invalid_format["Format"] = "text/html".into();
	let mut aac = station(4);
	aac["Format"] = "audio/aacp".into();
	let body = serde_json::json!([station(0), empty, invalid_format, aac]).to_string();
	let server = serve(vec![response("/Home/Top", &body)]);
	let nodes = Shoutcast::new(server.base)
		.children(&reqwest::Client::new(), "shoutcast/top", Sort::default())
		.await
		.unwrap();
	assert_eq!(nodes.len(), 1);
	assert!(
		matches!(&nodes[0], Node::Station(s) if s.id == "shoutcast:4" && s.info.as_ref().unwrap().contains("AAC 128k"))
	);
}
