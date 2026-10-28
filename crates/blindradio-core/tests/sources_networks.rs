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
			"Stream Madness",
			"Radio Paradise",
			"NTS",
			"WFMU",
			"181.fm",
			".977",
			"Big R Radio",
			"Radio Swiss",
			"KEXP",
			"WFUV",
			"Sunshine Live",
			"Radio 538",
			"SLAM!",
			"Capital",
			"Heart",
			"Smooth",
			"Classic FM",
			"Gold",
			"LBC",
			"Radio X",
			"Power Hit Radio Estonia",
			"Power Hit Radio Lithuania",
			"Dance Wave!",
			"Power Hitz",
			"WHFF",
			"Radio Record",
			"PulsRadio",
			"Planet Radio",
			"Nightride FM",
			"FRISKY",
			"XLtrax",
			"Radio FG"
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
fn curated_network_channels_and_search() {
	for (slug, expected) in [
		(
			"radio-paradise",
			vec![
				"main-mix",
				"mellow-mix",
				"rock-mix",
				"global-mix",
				"beyond",
				"serenity",
				"radio-2050",
				"kfat",
			],
		),
		("nts", vec!["nts-1", "nts-2"]),
		(
			"wfmu",
			vec![
				"wfmu",
				"rock-n-soul",
				"give-the-drummer",
				"sheenas-jungle-room",
			],
		),
	] {
		let nodes = children(&format!("net/{slug}"));
		let ids: Vec<_> = nodes
			.iter()
			.map(|node| {
				let Node::Station(station) = node else {
					panic!("not a station")
				};
				assert!(station.url.starts_with("https://"), "{}", station.url);
				station.id.clone()
			})
			.collect();
		let expected: Vec<_> = expected
			.iter()
			.map(|station| format!("net:{slug}:{station}"))
			.collect();
		assert_eq!(ids, expected);
	}
	for (name, id) in [
		("Main Mix", "net:radio-paradise:main-mix"),
		("NTS 2", "net:nts:nts-2"),
		("Give the Drummer", "net:wfmu:give-the-drummer"),
		("Classic Hits 181", "net:181fm:greatoldies"),
		(".977 Jamz", "net:977:jamz"),
		("Grunge FM", "net:bigr:grunge-fm"),
		("Radio Swiss Jazz", "net:radio-swiss:jazz"),
		("KEXP", "net:kexp:kexp"),
		("WFUV", "net:wfuv:wfuv"),
	] {
		let found = search(&SearchQuery {
			name: name.into(),
			..Default::default()
		});
		assert!(found.iter().any(|station| station.id == id), "{found:?}");
	}
}

#[test]
fn new_networks_are_complete() {
	for (slug, count) in [
		("181fm", 77),
		("977", 9),
		("bigr", 51),
		("radio-swiss", 3),
		("kexp", 1),
		("wfuv", 1),
		("sunshine-live", 7),
		("538", 1),
		("slam", 1),
		("capital", 4),
		("heart", 5),
		("smooth", 3),
		("classic-fm", 1),
		("gold", 1),
		("lbc", 2),
		("radio-x", 1),
		("power-hit-estonia", 1),
		("power-hit-lithuania", 2),
		("dance-wave", 1),
		("powerhitz", 15),
		("whff", 18),
		("radio-record", 92),
		("pulsradio", 8),
		("planet-radio", 33),
		("nightride-fm", 7),
		("frisky", 1),
		("xltrax", 1),
		("radio-fg", 1),
	] {
		let nodes = children(&format!("net/{slug}"));
		assert_eq!(nodes.len(), count, "{slug}");
		for node in nodes {
			let Node::Station(station) = node else {
				panic!("not a station in {slug}")
			};
			assert!(station.url.starts_with("https://"), "{}", station.url);
			assert!(
				station.info.as_deref().is_some_and(|info| !info.is_empty()),
				"{} has no info",
				station.id
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
