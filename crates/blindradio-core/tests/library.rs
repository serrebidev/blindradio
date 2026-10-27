use blindradio_core::library::{CUSTOM, Library, LibraryError};
use blindradio_core::station::{Source, Station};

fn st(id: &str) -> Station {
	Station {
		id: id.into(),
		name: id.to_uppercase(),
		url: format!("http://x/{id}"),
		source: Source::RadioBrowser,
		info: None,
	}
}

fn ids(v: &[Station]) -> Vec<&str> {
	v.iter().map(|s| s.id.as_str()).collect()
}

#[test]
fn favorites_toggle_order_and_move() {
	let mut l = Library::default();
	assert!(l.toggle_favorite(&st("a")));
	assert!(l.toggle_favorite(&st("b")));
	assert!(l.toggle_favorite(&st("c")));
	assert!(l.is_favorite("b"));
	assert_eq!(ids(&l.favorites), ["a", "b", "c"]);
	assert_eq!(l.move_favorite(2, true), 1);
	assert_eq!(ids(&l.favorites), ["a", "c", "b"]);
	assert_eq!(l.move_favorite(0, true), 0);
	assert_eq!(l.move_favorite(2, false), 2);
	assert_eq!(ids(&l.favorites), ["a", "c", "b"]);
	assert!(!l.toggle_favorite(&st("c")));
	assert!(!l.is_favorite("c"));
	l.remove_favorite("a");
	assert_eq!(ids(&l.favorites), ["b"]);
}

#[test]
fn recent_front_dedupe_cap_50() {
	let mut l = Library::default();
	l.add_recent(&st("a"));
	l.add_recent(&st("b"));
	l.add_recent(&st("a"));
	assert_eq!(ids(&l.recent), ["a", "b"]);
	for i in 0..60 {
		l.add_recent(&st(&format!("s{i}")));
	}
	assert_eq!(l.recent.len(), 50);
	assert_eq!(l.recent[0].id, "s59");
	assert_eq!(l.recent[49].id, "s10");
}

#[test]
fn custom_stations_added_and_deleted() {
	let mut l = Library::default();
	let s = l.add_custom("Mine", "http://m");
	assert!(s.id.starts_with("custom:"));
	assert_eq!(s.source, Source::Custom);
	assert_eq!((s.name.as_str(), s.url.as_str()), ("Mine", "http://m"));
	let t = l.add_custom("Two", "http://t");
	assert_ne!(s.id, t.id);
	assert_eq!(
		ids(&l.collections[0].stations),
		[s.id.as_str(), t.id.as_str()]
	);
	// Ruling R1: copies elsewhere stay.
	l.toggle_favorite(&s);
	let col = l.create_collection("Jazz");
	l.toggle_in_collection(&col, &s);
	l.delete_custom(&s.id);
	assert_eq!(ids(&l.collections[0].stations), [t.id.as_str()]);
	assert!(l.is_favorite(&s.id));
	assert_eq!(l.collections_containing(&s.id), [col]);
}

#[test]
fn custom_collection_rules() {
	let mut l = Library::default();
	assert_eq!(l.collections[0].id, CUSTOM);
	assert_eq!(l.collections[0].name, "Custom Stations");
	assert_eq!(
		l.rename_collection(CUSTOM, "x"),
		Err(LibraryError::CustomFixed)
	);
	assert_eq!(l.delete_collection(CUSTOM), Err(LibraryError::CustomFixed));
	let a = l.create_collection("A");
	let b = l.create_collection("B");
	l.move_collection(CUSTOM, false);
	l.move_collection(&a, true);
	assert_eq!(l.collections[0].id, CUSTOM);
	assert_eq!(l.collections[1].id, a);
	l.move_collection(&b, true);
	let order: Vec<&str> = l.collections.iter().map(|c| c.id.as_str()).collect();
	assert_eq!(order, [CUSTOM, b.as_str(), a.as_str()]);
	l.move_collection(&b, false);
	assert_eq!(l.collections[2].id, b);
	// Removing from Custom Stations is delete_custom.
	let s = l.add_custom("Mine", "http://m");
	l.remove_from_collection(CUSTOM, &s.id);
	assert!(l.collections[0].stations.is_empty());
}

#[test]
fn collection_create_reuses_name_case_insensitive() {
	let mut l = Library::default();
	let a = l.create_collection("Jazz");
	assert!(a.starts_with("col:"));
	assert_eq!(l.create_collection("jAZZ"), a);
	assert_eq!(l.create_collection("custom stations"), CUSTOM);
	assert_eq!(l.collections.len(), 2);
	l.rename_collection(&a, "Blues").unwrap();
	assert_eq!(l.collections[1].name, "Blues");
}

#[test]
fn toggle_and_remove_in_collection() {
	let mut l = Library::default();
	let c = l.create_collection("C");
	assert!(l.toggle_in_collection(&c, &st("a")));
	assert!(l.toggle_in_collection(&c, &st("b")));
	assert!(l.toggle_in_collection(&c, &st("d")));
	assert_eq!(l.move_in_collection(&c, 2, true), 1);
	assert_eq!(l.move_in_collection(&c, 0, true), 0);
	assert_eq!(ids(&l.collections[1].stations), ["a", "d", "b"]);
	assert!(!l.toggle_in_collection(&c, &st("a")));
	l.remove_from_collection(&c, "d");
	assert_eq!(ids(&l.collections[1].stations), ["b"]);
}

#[test]
fn delete_collection_keeps_stations_elsewhere() {
	let mut l = Library::default();
	let c = l.create_collection("C");
	l.toggle_in_collection(&c, &st("a"));
	l.toggle_favorite(&st("a"));
	l.delete_collection(&c).unwrap();
	assert!(l.is_favorite("a"));
	assert_eq!(l.collections.len(), 1);
	assert_eq!(l.delete_collection(&c), Err(LibraryError::NoCollection));
}

#[test]
fn add_all_skips_existing_and_counts() {
	let mut l = Library::default();
	let a = l.create_collection("A");
	let b = l.create_collection("B");
	for id in ["1", "2", "3"] {
		l.toggle_in_collection(&a, &st(id));
	}
	l.toggle_in_collection(&b, &st("2"));
	assert_eq!(l.add_all_to_collection(&a, &b), 2);
	assert_eq!(ids(&l.collections[2].stations), ["2", "1", "3"]);
	assert_eq!(l.add_all_to_collection(&a, &b), 0);
}

#[test]
fn collections_containing_station() {
	let mut l = Library::default();
	let a = l.create_collection("A");
	let b = l.create_collection("B");
	l.toggle_in_collection(&a, &st("x"));
	l.toggle_in_collection(&b, &st("x"));
	l.toggle_in_collection(&b, &st("y"));
	assert_eq!(l.collections_containing("x"), [a, b.clone()]);
	assert_eq!(l.collections_containing("y"), [b]);
	assert!(l.collections_containing("z").is_empty());
}

#[test]
fn library_round_trip_and_custom_readded() {
	let d = tempfile::tempdir().unwrap();
	let mut l = Library::default();
	l.toggle_favorite(&st("a"));
	l.add_recent(&st("b"));
	l.add_custom("Mine", "http://m");
	let c = l.create_collection("C");
	l.toggle_in_collection(&c, &st("a"));
	l.save(d.path()).unwrap();
	assert_eq!(Library::load(d.path()), l);

	let d2 = tempfile::tempdir().unwrap();
	std::fs::write(
		d2.path().join("library.json"),
		r#"{"favorites":[],"recent":[],"collections":[{"id":"col:1","name":"Z","stations":[]}]}"#,
	)
	.unwrap();
	let l2 = Library::load(d2.path());
	assert_eq!(l2.collections[0].id, CUSTOM);
	assert_eq!(l2.collections[1].id, "col:1");

	let d3 = tempfile::tempdir().unwrap();
	assert_eq!(Library::load(d3.path()).collections.len(), 1);
}
