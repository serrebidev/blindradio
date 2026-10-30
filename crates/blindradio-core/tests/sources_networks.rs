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
			"RTÉ",
			"RNZ",
			"Deutsche Welle",
			"RFI",
			"ORF",
			"SRF",
			"RAI",
			"RTVE",
			"NPO",
			"NRK",
			"DR",
			"Sveriges Radio",
			"Yle",
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
			"Radio FG",
			"1.FM",
			"101.ru",
			"DFM",
			"Hit FM",
			"Radioparty.pl",
			"RauteMusik",
			"TechnoBase.FM",
			"ACB Media",
			"Mushroom FM",
			"Vision Australia Radio",
			"NFBRN",
			"NCBI Live",
			"WRBH",
			"AMI Audio",
			"RNIB Connect Radio",
			"Double Tap Live"
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
		("Radio Swiss Jazz", "net:radio-swiss:jazz"),
		("RTÉ Radio 1", "net:rte:radio-1"),
		("RNZ Concert", "net:rnz:concert"),
		("RFI Monde", "net:rfi:monde"),
		("Ö1", "net:orf:oe1"),
		("SRF 3", "net:srf:3"),
		("Rai Radio 3", "net:rai:radio-3"),
		("NPO 3FM", "net:npo:3fm"),
		("NRK P3", "net:nrk:p3"),
		("DR P1", "net:dr:p1"),
		("Sveriges Radio P1", "net:sveriges-radio:p1"),
		("YleX", "net:yle:ylex"),
		("KCRW News 24", "net:kcrw:news-24"),
		("WNYC FM", "net:wnyc:fm"),
		("Amsterdam Trance", "net:1fm:amsterdam-trance"),
		("Smooth Jazz", "net:101ru:ch-31"),
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
		("rte", 4),
		("rnz", 3),
		("deutsche-welle", 1),
		("rfi", 10),
		("orf", 14),
		("srf", 6),
		("rai", 6),
		("rtve", 2),
		("npo", 11),
		("nrk", 10),
		("dr", 7),
		("sveriges-radio", 5),
		("yle", 6),
		("kcrw", 3),
		("wnyc", 3),
		("mpr", 1),
		("1fm", 34),
		("101ru", 19),
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
