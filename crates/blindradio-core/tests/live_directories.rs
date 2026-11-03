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

/// Different stations under a folder, following its Load More rows `pages` times.
async fn walk(sources: &Sources, id: &str, pages: usize) -> usize {
	let (mut next, mut ids) = (Some(id.to_owned()), std::collections::HashSet::new());
	for _ in 0..pages {
		let Some(id) = next.take() else { break };
		for node in sources.children(&id, Sort::default()).await.unwrap() {
			match node {
				Node::Station(s) => {
					ids.insert(s.id);
				}
				Node::LoadMore { id } => next = Some(id),
				Node::Folder { .. } => {}
			}
		}
	}
	ids.len()
}

#[tokio::test]
#[ignore = "network"]
async fn tunein_live_categories_and_more_stations() {
	let sources = Sources::new(client(), Endpoints::default());
	for id in ["tunein/c=music", "tunein/c=talk", "tunein/c=sports", "tunein/r0"] {
		let kids = sources.children(id, Sort::default()).await.unwrap();
		assert!(kids.iter().any(|n| matches!(n, Node::Folder { .. })), "{id}: {kids:?}");
	}
	let first = walk(&sources, "tunein/id=g22", 1).await;
	let more = walk(&sources, "tunein/id=g22", 3).await;
	println!("TuneIn World Music: {first} then {more} after two Load More");
	assert!(more > first, "{first} {more}");
}

#[tokio::test]
#[ignore = "network"]
async fn radionet_live_tags() {
	let sources = Sources::new(client(), Endpoints::default());
	for kind in ["genres", "countries", "languages", "topics"] {
		let kids = sources
			.children(&format!("radionet/tags/{kind}"), Sort::default())
			.await
			.unwrap();
		assert!(kids.len() > 10, "{kind}: {}", kids.len());
		let Node::Folder { id, .. } = &kids[kids.len() / 2] else {
			panic!("{kids:?}")
		};
		assert!(walk(&sources, id, 1).await > 0, "{id}");
	}
	let first = walk(&sources, "radionet/tag/genres/jazz", 1).await;
	let more = walk(&sources, "radionet/tag/genres/jazz", 3).await;
	println!("radio.net Jazz: {first} then {more} after two Load More");
	assert!(more > first, "{first} {more}");
}

#[tokio::test]
#[ignore = "network"]
async fn iheart_live_cities() {
	let sources = Sources::new(client(), Endpoints::default());
	let cities = sources.children("iheart/markets", Sort::default()).await.unwrap();
	assert!(cities.len() > 100, "{}", cities.len());
	let Node::Folder { id, name } = &cities[cities.len() / 2] else {
		panic!("{cities:?}")
	};
	let count = walk(&sources, id, 1).await;
	println!("iHeart {name}: {count} stations, {} cities", cities.len());
	assert!(count > 0, "{name}");
}

#[tokio::test]
#[ignore = "network"]
async fn live365_live_browse_and_search() {
	check("live365", "live365/genre/jazz-music-radio", "jazz", Source::Live365).await;
	let sources = Sources::new(client(), Endpoints::default());
	let genres = sources.children("live365", Sort::default()).await.unwrap();
	assert_eq!(genres.len(), 22, "{genres:?}");
	let jazz = sources
		.children("live365/genre/jazz-music-radio", Sort::default())
		.await
		.unwrap();
	assert!(jazz.iter().any(|n| matches!(n, Node::Folder { .. })), "no sub-genres");
	assert!(matches!(jazz.last(), Some(Node::LoadMore { .. })), "no Load More");
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
