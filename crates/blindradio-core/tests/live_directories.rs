//! Optional live checks for the additional directories; fixtures cover normal CI.
use blindradio_core::net::client;
use blindradio_core::sources::{Endpoints, Node, SearchQuery, Sort, Sources};
use blindradio_core::station::Source;

async fn check(root: &str, branch: &str, name: &str, source: Source) {
	let sources = Sources::new(client(), Endpoints::default());
	let folders = sources.children(root, Sort::default()).await.unwrap();
	assert!(!folders.is_empty());
	let stations = sources.children(branch, Sort::default()).await.unwrap();
	let count = stations
		.iter()
		.filter(|n| matches!(n, Node::Station(_)))
		.count();
	assert!(count > 0, "{root} returned no stations");
	let query = SearchQuery {
		name: name.into(),
		..Default::default()
	};
	let found = sources.search(&query).await;
	let matches = found.iter().filter(|s| s.source == source).count();
	assert!(matches > 0, "{root} returned no search results for {name}");
	println!("{root}: {count} browse stations, {matches} search results");
}

#[tokio::test]
#[ignore = "network"]
async fn icecast_live_browse_and_search() {
	check("icecast", "icecast/all", "jazz", Source::Icecast).await;
}

#[tokio::test]
#[ignore = "network"]
async fn shoutcast_live_browse_and_search() {
	check("shoutcast", "shoutcast/top", "jazz", Source::Shoutcast).await;
}

#[tokio::test]
#[ignore = "network"]
async fn internet_radio_live_browse_and_search() {
	check(
		"internet-radio",
		"internet-radio/featured",
		"paradise",
		Source::InternetRadio,
	)
	.await;
}

#[tokio::test]
#[ignore = "network"]
async fn iprd_live_browse_and_search() {
	check("iprd", "iprd/genre/jazz", "jazz", Source::Iprd).await;
	let sources = Sources::new(client(), Endpoints::default());
	let countries = sources.children("iprd/countries", Sort::default()).await.unwrap();
	assert!(countries.len() > 100, "{}", countries.len());
	let genres = sources.children("iprd/genres", Sort::default()).await.unwrap();
	assert_eq!(genres.len(), 100);
}

#[tokio::test]
#[ignore = "network"]
async fn recommended_live_browse() {
	let sources = Sources::new(client(), Endpoints::default());
	let folders = sources.children("recommended", Sort::default()).await.unwrap();
	assert!(folders.iter().any(|n| n.name() == "Jazz Blues"), "{folders:?}");
	let stations = sources
		.children("recommended/jazz-blues", Sort::default())
		.await
		.unwrap();
	assert!(
		stations.iter().any(|n| matches!(n, Node::Station(s) if s.source == Source::Recommended)),
		"{stations:?}"
	);
}
