use blindradio_core::sources::networks::{children, networks, search};
use blindradio_core::sources::{Node, SearchQuery};
use blindradio_core::station::Source;
use std::collections::HashSet;

#[test]
fn networks_parse_and_order() {
	let names: Vec<_> = networks().iter().map(|n| n.name.as_str()).collect();
	assert_eq!(
		names,
		[
			"BBC",
			"NPR",
			"CBC",
			"ABC Australia",
			"Radio France",
			"Deutschlandfunk",
			"SomaFM",
			"Stream Madness"
		]
	);
	let mut ids = HashSet::new();
	for n in networks() {
		assert!(!n.stations.is_empty(), "{} has no stations", n.name);
		for node in children(&format!("net/{}", n.slug)) {
			let Node::Station(s) = node else {
				panic!("not a station in {}", n.name)
			};
			assert!(ids.insert(s.id.clone()), "duplicate id {}", s.id);
			assert!(
				s.url.starts_with("http://") || s.url.starts_with("https://"),
				"{}: {}",
				s.id,
				s.url
			);
		}
	}
}

#[test]
fn networks_children_and_ids() {
	let roots = children("net");
	assert_eq!(roots.len(), networks().len());
	assert_eq!(
		roots[0],
		Node::Folder {
			id: "net/bbc".into(),
			name: "BBC".into()
		}
	);
	let bbc = &networks()[0];
	let nodes = children("net/bbc");
	assert_eq!(nodes.len(), bbc.stations.len());
	let Node::Station(first) = &nodes[0] else {
		panic!("{:?}", nodes[0])
	};
	assert_eq!(first.id, format!("net:bbc:{}", bbc.stations[0].slug));
	assert_eq!(first.name, bbc.stations[0].name);
	assert_eq!(first.url, bbc.stations[0].url);
	assert_eq!(first.source, Source::Network("BBC".into()));
	assert_eq!(first.info, bbc.stations[0].info);
	assert!(children("net/nope").is_empty());
	assert!(children("rb").is_empty());
}

#[test]
fn networks_search_by_name() {
	let q = |name: &str| SearchQuery {
		name: name.into(),
		country: Some("Nowhere".into()),
		..Default::default()
	};
	let found = search(&q("rAdIo 4"));
	assert!(found.iter().any(|s| s.id == "net:bbc:radio-4"), "{found:?}");
	assert!(
		found
			.iter()
			.all(|s| s.name.to_lowercase().contains("radio 4"))
	);
	let soma = search(&q("groove salad"));
	assert!(
		soma.iter()
			.any(|s| s.source == Source::Network("SomaFM".into())),
		"{soma:?}"
	);
	assert!(search(&q("zzzz no such station")).is_empty());
	assert!(search(&q("  ")).is_empty());
}
