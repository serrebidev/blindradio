use blindradio_core::library::{CUSTOM, Home, Library, LibraryError};
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
fn custom_stations_change_only_through_add_and_delete() {
	let mut l = Library::default();
	let s = l.add_custom("Mine", "http://m");
	assert!(!l.toggle_in_collection(CUSTOM, &st("a")));
	assert!(!l.toggle_in_collection(CUSTOM, &s));
	let a = l.create_collection("A");
	l.toggle_in_collection(&a, &st("b"));
	assert_eq!(l.add_all_to_collection(&a, CUSTOM), 0);
	assert_eq!(ids(&l.collections[0].stations), [s.id.as_str()]);
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

#[test]
fn favorites_folders_hold_stations() {
	use blindradio_core::library::Folder;
	let mut l = Library::default();
	l.fav_folders.push(Folder::new("News"));
	l.fav_folders.push(Folder::new("Music"));
	// Add into the Music folder (index 1) and the root.
	assert!(l.add_favorite_to(&[1], &st("song")));
	assert!(l.add_favorite_to(&[], &st("rooted")));
	// Already a favorite anywhere: a second add fails wherever it points.
	assert!(!l.add_favorite_to(&[0], &st("song")));
	assert!(!l.add_favorite_to(&[], &st("song")));
	assert!(l.is_favorite("song"));
	assert_eq!(
		l.all_favorites().iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
		["rooted", "song"]
	);
	// Toggle removes from inside a folder too.
	assert!(!l.toggle_favorite(&st("song")));
	assert!(!l.is_favorite("song"));
	assert_eq!(l.all_favorites().len(), 1);
	// Nested folders work through paths.
	l.fav_folders[0].folders.push(Folder::new("World"));
	assert!(l.add_favorite_to(&[0, 0], &st("world")));
	assert!(l.is_favorite("world"));
	l.remove_favorite("world");
	assert!(!l.is_favorite("world"));
}

#[test]
fn collection_folders_and_removal() {
	let mut l = Library::default();
	let id = l.create_collection("C");
	let a = l.add_custom("A", "http://a");
	l.toggle_in_collection(&id, &a);
	{
		let c = l.collections.iter_mut().find(|c| c.id == id).unwrap();
		let st = c.stations.remove(0);
		c.child_mut("Sub").stations.push(st);
	}
	let cid = l
		.collections
		.iter()
		.find(|c| c.id == id)
		.unwrap()
		.all_stations()[0]
		.id
		.clone();
	// It lives in Custom Stations (add_custom) and in C's folder.
	assert_eq!(
		l.collections_containing(&cid),
		[blindradio_core::library::CUSTOM.to_string(), id.clone()]
	);
	// Toggling a station that lives in a folder removes it.
	let st = l
		.collections
		.iter()
		.find(|c| c.id == id)
		.unwrap()
		.all_stations()[0]
		.clone();
	assert!(!l.toggle_in_collection(&id, &st));
	let c = l.collections.iter().find(|c| c.id == id).unwrap();
	assert!(c.all_stations().is_empty());
}

#[test]
fn folder_paths_add_rename_remove() {
	let mut lib = Library::default();
	let id = lib.create_collection("Mix");
	let c = lib.collections.iter_mut().find(|c| c.id == id).unwrap();
	let news = c.add_folder(&[], "News").unwrap();
	assert_eq!(news, vec![0]);
	let world = c.add_folder(&news, "World").unwrap();
	assert_eq!(world, vec![0, 0]);
	c.stations_at_mut(&world).unwrap().push(st("w1"));
	assert!(c.rename_folder(&news, "News 2"));
	assert_eq!(c.folder_at_mut(&world).unwrap().name, "World");
	assert_eq!(c.all_stations().len(), 1);
	assert!(c.remove_folder(&news), "removes the subtree");
	assert!(c.all_stations().is_empty());
	assert!(!c.remove_folder(&[9]));

	// Favorites share the same path rules, rooted at the favorites list.
	let music = lib.add_fav_folder(&[], "Music").unwrap();
	lib.fav_stations_at_mut(&music)
		.unwrap()
		.push(st("f1"));
	lib.fav_stations_at_mut(&[]).unwrap().push(st("f0"));
	assert_eq!(lib.all_favorites().len(), 2);
	assert!(lib.rename_fav_folder(&music, "Tunes"));
	assert!(lib.remove_fav_folder(&music));
	assert_eq!(lib.all_favorites().len(), 1, "the root station stays");
}

#[test]
fn station_home_finds_collections_folders_and_favorites() {
	let mut lib = Library::default();
	let jazz = lib.create_collection("Jazz");
	let deep = {
		let c = lib.collection_mut(&jazz).unwrap();
		let folder = c.add_folder(&[], "Deep").unwrap();
		c.stations_at_mut(&folder).unwrap().push(st("in-folder"));
		c.stations_at_mut(&[]).unwrap().push(st("at-root"));
		folder
	};
	assert_eq!(
		lib.station_home("in-folder"),
		Some(Home::Collection {
			id: jazz.clone(),
			path: deep
		})
	);
	assert_eq!(
		lib.station_home("at-root"),
		Some(Home::Collection {
			id: jazz,
			path: Vec::new()
		})
	);
	let music = lib.add_fav_folder(&[], "Music").unwrap();
	lib.fav_stations_at_mut(&music).unwrap().push(st("fav"));
	assert_eq!(
		lib.station_home("fav"),
		Some(Home::Favorite { path: music })
	);
	lib.toggle_favorite(&st("fav-root"));
	assert_eq!(
		lib.station_home("fav-root"),
		Some(Home::Favorite { path: Vec::new() })
	);
	assert_eq!(lib.station_home("nowhere"), None);
}
