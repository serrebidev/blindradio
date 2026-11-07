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
			"RTÉ",
			"RNZ",
			"KCRW",
			"WNYC",
			"Minnesota Public Radio",
			"SomaFM",
			"Stream Madness",
			"Radio Paradise",
			"NTS",
			"WFMU",
			"181.fm",
			".977",
			"Big R Radio",
			"KEXP",
			"WFUV",
			"Capital",
			"Heart",
			"Smooth",
			"Classic FM",
			"Gold",
			"LBC",
			"Radio X",
			"Power Hitz",
			"WHFF",
			"Nightride FM",
			"FRISKY",
			"XLtrax",
			"1.FM",
			"ACB Media",
			"Mushroom FM",
			"Vision Australia Radio",
			"NFBRN",
			"WRBH",
			"AMI Audio",
			"RNIB Connect Radio",
			"Double Tap Live",
			"The Global Voice",
			"Radio Udaan",
			"Treehouse Radio",
			"Dodge Radio",
			"2020 The Beacon",
			"Virgin Radio UK",
			"talkSPORT",
			"Times Radio",
			"Rainwave",
			"Radio Caroline",
			"Party Vibe",
			"Linn Radio",
			"K-LOVE",
			"Air1",
			"Pacifica",
			"Triple M",
			"ARN Australia",
			"NZME",
			"Rova",
			"SABC",
			"American Family Radio",
			"Moody Radio",
			"DKFM",
			"Soho Radio",
			"Rinse FM",
			"Worldwide FM",
			"dublab",
			"Amazing Radio",
			"Boom Radio",
			"8Radio",
			"Bloomberg Radio",
			"C-SPAN Radio",
			"Fox News Radio",
			"ESPN Radio",
			"Al Jazeera English",
			"NHK World Radio"
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
				"sanctuary",
			],
		),
		(
			"nts",
			vec![
				"nts-1",
				"nts-2",
				"poolside",
				"slow-focus",
				"low-key",
				"memory-lane",
				"4-to-the-floor",
				"island-time",
				"the-tube",
				"sheet-music",
				"feelings",
				"expansions",
				"rap-house",
				"labyrinth",
				"sweat",
				"otaku",
				"the-pit",
				"field-recordings",
			],
		),
		(
			"wfmu",
			vec![
				"wfmu",
				"rock-n-soul",
				"give-the-drummer",
				"sheenas-jungle-room",
				"ubuweb-radio",
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
		let mut expected: Vec<_> = expected
			.iter()
			.map(|station| format!("net:{slug}:{station}"))
			.collect();
		// Membership is the contract here; children come back in name order.
		let mut ids_sorted = ids.clone();
		ids_sorted.sort();
		expected.sort();
		assert_eq!(ids_sorted, expected);
		let names: Vec<String> = nodes
			.iter()
			.map(|node| node.name().to_lowercase())
			.collect();
		let mut names_sorted = names.clone();
		names_sorted.sort();
		assert_eq!(names, names_sorted, "stations in name order");
	}
	for (name, id) in [
		("Main Mix", "net:radio-paradise:main-mix"),
		("NTS 2", "net:nts:nts-2"),
		("Give the Drummer", "net:wfmu:give-the-drummer"),
		("Classic Hits 181", "net:181fm:greatoldies"),
		(".977 Jamz", "net:977:jamz"),
		("Grunge FM", "net:bigr:grunge-fm"),
		("RTÉ Radio 1", "net:rte:radio-1"),
		("RNZ Concert", "net:rnz:concert"),
		("KCRW News 24", "net:kcrw:news-24"),
		("WNYC FM", "net:wnyc:fm"),
		("Amsterdam Trance", "net:1fm:amsterdam-trance"),
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
		("181fm", 83),
		("977", 9),
		("bigr", 51),
		("kexp", 1),
		("wfuv", 1),
		("capital", 10),
		("heart", 10),
		("smooth", 8),
		("classic-fm", 3),
		("gold", 1),
		("lbc", 2),
		("radio-x", 5),
		("powerhitz", 15),
		("whff", 19),
		("nightride-fm", 7),
		("frisky", 4),
		("xltrax", 1),
		("rte", 5),
		("rnz", 4),
		("kcrw", 3),
		("wnyc", 3),
		("mpr", 16),
		("1fm", 73),
		("virgin-radio-uk", 3),
		("talksport", 2),
		("times-radio", 1),
		("rainwave", 4),
		("radio-caroline", 3),
		("party-vibe", 5),
		("linn-radio", 1),
		("k-love", 9),
		("air1", 2),
		("pacifica", 7),
		("triple-m", 39),
		("arn-australia", 12),
		("nzme", 53),
		("rova", 11),
		("sabc", 5),
		("american-family-radio", 2),
		("moody-radio", 3),
		("dkfm", 3),
		("soho-radio", 1),
		("rinse-fm", 1),
		("worldwide-fm", 1),
		("dublab", 1),
		("amazing-radio", 1),
		("boom-radio", 1),
		("8radio", 1),
		("bloomberg-radio", 1),
		("c-span-radio", 1),
		("fox-news-radio", 1),
		("espn-radio", 1),
		("al-jazeera-english", 1),
		("nhk-world-radio", 1),
		("vision-australia-radio", 9),
		("the-global-voice", 2),
		("radio-udaan", 2),
		("dodge-radio", 2),
		("2020-the-beacon", 1),
	] {
		let nodes = children(&format!("net/{slug}"));
		assert_eq!(nodes.len(), count, "{slug}");
		for node in nodes {
			let Node::Station(station) = node else {
				panic!("not a station in {slug}")
			};
			assert!(
				station.url.starts_with("http://") || station.url.starts_with("https://"),
				"{}: {}",
				station.id,
				station.url
			);
			assert!(
				station.info.as_deref().is_some_and(|info| !info.is_empty()),
				"{} has no info",
				station.id
			);
		}
	}
}

#[test]
fn named_single_station_services_are_listed() {
	let roots = children("net");
	for slug in [
		"kexp",
		"wfuv",
		"gold",
		"xltrax",
		"times-radio",
		"linn-radio",
		"treehouse-radio",
		"2020-the-beacon",
	] {
		assert!(
			roots
				.iter()
				.any(|n| matches!(n, Node::Folder { id, .. } if *id == format!("net/{slug}"))),
			"{slug} is hidden"
		);
	}
	let found = search(&SearchQuery {
		name: "KEXP".into(),
		..Default::default()
	});
	assert!(found.iter().any(|s| s.id == "net:kexp:kexp"), "{found:?}");
}

#[test]
fn networks_children_and_ids() {
	let roots = children("net");
	// Single-station networks are not listed unless flagged as named services.
	assert_eq!(
		roots.len(),
		networks()
			.iter()
			.filter(|n| n.stations.len() > 1 || n.listed)
			.count()
	);
	// Roots come back in name order, BBC among them.
	let root_names: Vec<String> = roots.iter().map(|n| n.name().to_lowercase()).collect();
	let mut root_names_sorted = root_names.clone();
	root_names_sorted.sort();
	assert_eq!(root_names, root_names_sorted, "networks in name order");
	assert!(roots.contains(&Node::Folder {
		id: "net/bbc".into(),
		name: "BBC".into()
	}));
	let bbc = &networks()[0];
	let nodes = children("net/bbc");
	assert_eq!(nodes.len(), bbc.stations.len());
	let Node::Station(first) = &nodes[0] else {
		panic!("{:?}", nodes[0])
	};
	let first_by_name = bbc
		.stations
		.iter()
		.min_by_key(|st| st.name.to_lowercase())
		.expect("bbc has stations");
	assert_eq!(first.id, format!("net:bbc:{}", first_by_name.slug));
	assert_eq!(first.name, first_by_name.name);
	assert_eq!(first.url, first_by_name.url);
	assert_eq!(first.source, Source::Network("BBC".into()));
	assert_eq!(first.info, first_by_name.info);
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
