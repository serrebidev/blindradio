mod common;

use std::path::{Path, PathBuf};

use blindradio_core::import::{
	ImportReport, ImportTarget, STARTER_COLLECTION, STARTER_PLAYLIST_URL, export_m3u,
	export_m3u_tree, export_playlist_dir, fetch_playlist, import_playlist, import_playlist_dir,
	import_playlist_text, import_starter, playlist_dir_files,
};
use blindradio_core::playlist::PlaylistKind;
use blindradio_core::library::{CUSTOM, Library};
use blindradio_core::net::client;
use blindradio_core::store;
use common::{Route, serve};

fn collection(name: &str) -> ImportTarget {
	ImportTarget::Collection(name.into())
}

fn fixture(name: &str) -> PathBuf {
	Path::new(env!("CARGO_MANIFEST_DIR"))
		.join("tests/fixtures/import")
		.join(name)
}

fn custom(lib: &Library) -> Vec<blindradio_core::station::Station> {
	lib.collections[0].all_stations()
}

#[test]
fn import_custom_reports_counts() {
	let mut lib = Library::default();
	let r = import_playlist(&mut lib, &fixture("mixed.m3u"), ImportTarget::Custom).unwrap();
	assert_eq!(r.message(), "Added 2. Had 0. Skipped 1.");
	// A file import keeps its name: a "mixed" folder inside Custom Stations.
	assert!(lib.collections[0].stations.is_empty());
	assert_eq!(lib.collections[0].folders.len(), 1);
	assert_eq!(lib.collections[0].folders[0].name, "mixed");
	let c = custom(&lib);
	assert_eq!(c.len(), 2);
	assert_eq!(
		(c[0].name.as_str(), c[0].url.as_str()),
		("Jazz FM", "http://a.example/jazz")
	);
	let r = import_playlist(&mut lib, &fixture("mixed.m3u"), ImportTarget::Custom).unwrap();
	assert_eq!(r.message(), "Added 0. Had 2. Skipped 1.");
	assert_eq!(custom(&lib).len(), 2);
}

#[test]
fn import_as_collection_named_after_file() {
	let mut lib = Library::default();
	let r = import_playlist(&mut lib, &fixture("mixed.m3u"), collection("mixed")).unwrap();
	assert_eq!(r.message(), "Added 2. Had 0. Skipped 1.");
	assert!(custom(&lib).is_empty());
	let c = lib.collections.iter().find(|c| c.name == "mixed").unwrap();
	assert_eq!(c.stations.len(), 2);
	assert!(c.stations.iter().all(|s| s.id.starts_with("custom:")));
}

#[test]
fn import_into_existing_collection_counts_had() {
	let mut lib = Library::default();
	let id = lib.create_collection("MIXED");
	let s = lib.add_custom("Mine", "http://a.example/jazz");
	lib.toggle_in_collection(&id, &s);
	let r = import_playlist(&mut lib, &fixture("mixed.m3u"), collection("mixed")).unwrap();
	// (the Custom copy of Jazz FM sits in its own file folder, not the root list)
	assert_eq!(r.message(), "Added 1. Had 1. Skipped 1.");
	assert_eq!(lib.collections.iter().filter(|c| c.id != CUSTOM).count(), 1);
	let c = lib.collections.iter().find(|c| c.id == id).unwrap();
	assert_eq!(c.stations.len(), 2);
	// The pre-existing Custom Stations entry is untouched and nothing new went there.
	assert_eq!(custom(&lib).len(), 1);
}

#[test]
fn import_counts_duplicates_and_encodings() {
	let mut lib = Library::default();
	let r = import_playlist(&mut lib, &fixture("dup.m3u"), ImportTarget::Custom).unwrap();
	assert_eq!((r.added, r.had, r.skipped), (1, 1, 0));
	import_playlist(&mut lib, &fixture("bom.m3u8"), ImportTarget::Custom).unwrap();
	import_playlist(&mut lib, &fixture("latin1.pls"), ImportTarget::Custom).unwrap();
	let all = custom(&lib);
	let names: Vec<_> = all.iter().map(|s| s.name.as_str()).collect();
	assert_eq!(names, ["One", "Caf\u{e9} Radio", "Radio \u{c9}t\u{e9}"]);
}

#[test]
fn nameless_entry_named_after_url() {
	let mut lib = Library::default();
	import_playlist(&mut lib, &fixture("nameless.m3u"), ImportTarget::Custom).unwrap();
	assert_eq!(custom(&lib)[0].name, "http://f.example/nameless");
}

#[test]
fn export_then_import_round_trip() {
	let mut lib = Library::default();
	lib.add_custom("Jazz FM", "http://a.example/jazz");
	lib.add_custom("Caf\u{e9} Radio", "https://b.example/cafe");
	let dir = tempfile::tempdir().unwrap();
	let p = dir.path().join("out.m3u");
	export_m3u(&custom(&lib), &p).unwrap();
	assert_eq!(
		std::fs::read_to_string(&p).unwrap(),
		"#EXTM3U\n#EXTINF:-1,Jazz FM\nhttp://a.example/jazz\n#EXTINF:-1,Caf\u{e9} Radio\nhttps://b.example/cafe\n"
	);
	let mut other = Library::default();
	let r = import_playlist(&mut other, &p, ImportTarget::Custom).unwrap();
	assert_eq!((r.added, r.had, r.skipped), (2, 0, 0));
	let la = custom(&lib);
	let lb = custom(&other);
	let a: Vec<_> = la.iter().map(|s| (&s.name, &s.url)).collect();
	let b: Vec<_> = lb.iter().map(|s| (&s.name, &s.url)).collect();
	assert_eq!(a, b);
}

#[test]
fn import_groups_become_folders_with_carry_forward() {
	// Brandon's Public Radio shape: group-title only on the first entry of a block;
	// the block runs until the next tag. Entries before any tag stay at the root.
	let body = "#EXTM3U\n\
		#EXTINF:-1,Loose One\nhttp://x.example/loose\n\
		#EXTINF:-1 group-title=\"AZPM\",AZPM\nhttp://x.example/azpm\n\
		#EXTINF:-1,AZPM Classical\nhttp://x.example/azpm-classical\n\
		#EXTINF:-1,AZPM Jazz\nhttp://x.example/azpm-jazz\n\
		#EXTINF:-1 group-title=\"KCRW\",KCRW\nhttp://x.example/kcrw\n\
		#EXTINF:-1,News 24\nhttp://x.example/news24\n";
	let mut lib = Library::default();
	let r = import_playlist_text(
		&mut lib,
		body,
		Some(PlaylistKind::M3u),
		collection("Public Radio"),
	);
	assert_eq!((r.added, r.had, r.skipped), (6, 0, 0));
	let c = lib
		.collections
		.iter()
		.find(|c| c.name == "Public Radio")
		.unwrap();
	assert_eq!(
		c.stations.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
		["Loose One"]
	);
	assert_eq!(c.folders.len(), 2);
	assert_eq!(c.folders[0].name, "AZPM");
	assert_eq!(
		c.folders[0]
			.stations
			.iter()
			.map(|s| s.name.as_str())
			.collect::<Vec<_>>(),
		["AZPM", "AZPM Classical", "AZPM Jazz"]
	);
	assert_eq!(c.folders[1].name, "KCRW");
	assert_eq!(
		c.folders[1]
			.stations
			.iter()
			.map(|s| s.name.as_str())
			.collect::<Vec<_>>(),
		["KCRW", "News 24"]
	);
	// Re-import: everything counts as had, folders are not duplicated.
	let r = import_playlist_text(
		&mut lib,
		body,
		Some(PlaylistKind::M3u),
		collection("Public Radio"),
	);
	assert_eq!((r.added, r.had), (0, 6));
	let c = lib
		.collections
		.iter()
		.find(|c| c.name == "Public Radio")
		.unwrap();
	assert_eq!(c.folders.len(), 2);
}

#[test]
fn custom_folder_import_keeps_playlist_name_and_groups() {
	let body = "#EXTM3U\n\
		#EXTINF:-1 group-title=\"News\",World News\nhttp://x.example/news\n\
		#EXTINF:-1,Sport\nhttp://x.example/sport\n";
	let mut lib = Library::default();
	let r = import_playlist_text(
		&mut lib,
		body,
		Some(PlaylistKind::M3u),
		ImportTarget::CustomFolder("My List".into()),
	);
	assert_eq!(r.added, 2);
	let c = &lib.collections[0];
	assert!(c.stations.is_empty());
	assert_eq!(c.folders.len(), 1);
	let wrap = &c.folders[0];
	assert_eq!(wrap.name, "My List");
	assert_eq!(wrap.folders.len(), 1);
	assert_eq!(wrap.folders[0].name, "News");
	// "Sport" has no tag of its own: it runs on in the News block.
	assert_eq!(wrap.folders[0].stations.len(), 2);
}

#[test]
fn export_tree_round_trip_rebuilds_folders() {
	let mut lib = Library::default();
	let id = lib.create_collection("Mix");
	let a = lib.add_custom("A", "http://x.example/a");
	let b = lib.add_custom("B", "http://x.example/b");
	lib.toggle_in_collection(&id, &a);
	lib.toggle_in_collection(&id, &b);
	{
		let c = lib.collections.iter_mut().find(|c| c.id == id).unwrap();
		let st = c.stations.remove(1);
		c.child_mut("News").stations.push(st);
	}
	let dir = tempfile::tempdir().unwrap();
	let p = dir.path().join("tree.m3u");
	{
		let c = lib.collections.iter().find(|c| c.id == id).unwrap();
		export_m3u_tree(&c.stations, &c.folders, &p).unwrap();
	}
	let mut other = Library::default();
	let r = import_playlist(&mut other, &p, collection("Copy")).unwrap();
	assert_eq!(r.added, 2);
	let c = other.collections.iter().find(|c| c.name == "Copy").unwrap();
	assert_eq!(c.stations.len(), 1);
	assert_eq!(c.folders.len(), 1);
	assert_eq!(c.folders[0].stations[0].name, "B");
}

#[test]
fn export_tree_round_trip_rebuilds_nested_folders() {
	let mut lib = Library::default();
	let id = lib.create_collection("Mix");
	let a = lib.add_custom("A", "http://x.example/a");
	let b = lib.add_custom("B", "http://x.example/b");
	lib.toggle_in_collection(&id, &a);
	lib.toggle_in_collection(&id, &b);
	{
		let c = lib.collections.iter_mut().find(|c| c.id == id).unwrap();
		let st = c.stations.remove(1);
		// News inside Public: two levels down.
		c.child_mut("Public")
			.child_mut("News")
			.stations
			.push(st);
		// A slash without spaces is a name, not a path.
		let st = c.stations.remove(0);
		c.child_mut("Top 40/Pop").stations.push(st);
	}
	let dir = tempfile::tempdir().unwrap();
	let p = dir.path().join("tree.m3u");
	{
		let c = lib.collections.iter().find(|c| c.id == id).unwrap();
		export_m3u_tree(&c.stations, &c.folders, &p).unwrap();
	}
	let body = std::fs::read_to_string(&p).unwrap();
	assert!(body.contains("group-title=\"Public / News\""), "{body}");
	assert!(body.contains("group-title=\"Top 40/Pop\""), "{body}");
	let mut other = Library::default();
	let r = import_playlist(&mut other, &p, collection("Copy")).unwrap();
	assert_eq!(r.added, 2);
	let c = other.collections.iter().find(|c| c.name == "Copy").unwrap();
	assert_eq!(c.folders.len(), 2);
	let public = c.folders.iter().find(|f| f.name == "Public").unwrap();
	assert_eq!(public.folders.len(), 1);
	assert_eq!(public.folders[0].name, "News");
	assert_eq!(public.folders[0].stations[0].name, "B");
	let pop = c.folders.iter().find(|f| f.name == "Top 40/Pop").unwrap();
	assert_eq!(pop.stations[0].name, "A");
}

#[test]
fn directory_imports_each_file_as_a_folder_and_exports_back() {
	let dir = tempfile::tempdir().unwrap();
	let top = dir.path().join("Radio");
	std::fs::create_dir_all(top.join("World")).unwrap();
	std::fs::write(
		top.join("Jazz.m3u"),
		"#EXTM3U\n#EXTINF:-1,A\nhttp://x.example/a\n#EXTINF:-1 group-title=\"Smooth\",B\nhttp://x.example/b\n",
	)
	.unwrap();
	std::fs::write(
		top.join("World/France.pls"),
		"[playlist]\nFile1=http://x.example/c\nTitle1=C\nFile2=http://x.example/a\nNumberOfEntries=2\n",
	)
	.unwrap();
	std::fs::write(top.join("notes.txt"), "http://x.example/z\n").unwrap();

	let mut lib = Library::default();
	let r = import_playlist_dir(&mut lib, &top, true).unwrap();
	// A sits in both files: it lands once.
	assert_eq!(r.message(), "Added 3. Had 1. Skipped 0.");
	let c = lib.collections.iter().find(|c| c.name == "Radio").unwrap();
	assert!(c.stations.is_empty());
	let names: Vec<&str> = c.folders.iter().map(|f| f.name.as_str()).collect();
	assert_eq!(names, ["Jazz", "World"]);
	assert_eq!(c.folders[0].stations[0].name, "A");
	assert_eq!(c.folders[0].folders[0].name, "Smooth");
	assert_eq!(c.folders[1].folders[0].name, "France");
	assert_eq!(c.folders[1].folders[0].stations[0].name, "C");

	// Into Custom Stations: one folder named after the directory holds it all.
	let mut lib2 = Library::default();
	import_playlist_dir(&mut lib2, &top, false).unwrap();
	assert_eq!(lib2.collections[0].folders[0].name, "Radio");
	assert_eq!(lib2.collections[0].folders[0].folders.len(), 2);

	// Export: one file per top folder, into a new directory, never over one.
	let out = tempfile::tempdir().unwrap();
	let (made, count) = export_playlist_dir(out.path(), "Radio", &c.stations, &c.folders).unwrap();
	assert_eq!(
		(made.file_name().unwrap().to_str(), count),
		(Some("Radio"), 3)
	);
	let (again, _) = export_playlist_dir(out.path(), "Radio", &c.stations, &c.folders).unwrap();
	assert_eq!(again.file_name().unwrap().to_str(), Some("Radio (2)"));
	let mut back = Library::default();
	let r = import_playlist_dir(&mut back, &made, true).unwrap();
	assert_eq!(r.added, 3);
	let b = back.collections.iter().find(|c| c.name == "Radio").unwrap();
	assert_eq!(b.folders[1].name, "World");
	assert_eq!(b.folders[1].folders[0].name, "France");

	let empty = tempfile::tempdir().unwrap();
	assert!(import_playlist_dir(&mut back, empty.path(), true).is_err());
}

#[test]
fn directory_file_names_safe_and_unique() {
	let f = |name: &str| blindradio_core::library::Folder {
		name: name.into(),
		stations: Vec::new(),
		folders: Vec::new(),
		origin: None,
	};
	let files = playlist_dir_files("x", &[], &[f("Top 40/Pop"), f("jazz"), f("Jazz"), f(" .")]);
	let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
	assert_eq!(
		names,
		["Top 40_Pop.m3u", "jazz.m3u", "Jazz (2).m3u", "Playlist.m3u"]
	);
}

#[test]
fn report_message_exact() {
	let r = ImportReport {
		added: 12,
		had: 3,
		skipped: 1,
	};
	assert_eq!(r.message(), "Added 12. Had 3. Skipped 1.");
}

#[test]
fn not_a_playlist_errors() {
	let dir = tempfile::tempdir().unwrap();
	let p = dir.path().join("x.txt");
	std::fs::write(&p, "<html></html>").unwrap();
	let mut lib = Library::default();
	assert!(import_playlist(&mut lib, &p, ImportTarget::Custom).is_err());
}

fn starter_body() -> String {
	std::fs::read_to_string(fixture("radios.m3u")).unwrap()
}

#[test]
fn starter_constants() {
	assert_eq!(STARTER_PLAYLIST_URL, "https://serrebiradio.com/radios.m3u");
	assert_eq!(STARTER_COLLECTION, "SerrebiRadio");
}

#[test]
fn starter_imports_all_into_serrebiradio_collection() {
	let mut lib = Library::default();
	let r = import_starter(&mut lib, &starter_body());
	assert_eq!(r.message(), "Added 175. Had 0. Skipped 0.");
	assert!(custom(&lib).is_empty());
	let c = lib
		.collections
		.iter()
		.find(|c| c.name == "SerrebiRadio")
		.unwrap();
	// Every starter entry carries a group: groups became folders holding all 175.
	assert_eq!(c.all_stations().len(), 175);
	assert!(c.stations.is_empty());
	assert_eq!(c.folders[0].name, "electronic");
	let first = &c.folders[0].stations[0];
	assert_eq!(first.name, "SerrebiRadio");
	assert_eq!(first.url, "http://radio.serrebiradio.com:8000/SerrebiRadio");
	assert_eq!(first.info.as_deref(), Some("electronic"));
}

#[test]
fn starter_reimport_counts_had() {
	let mut lib = Library::default();
	let body = starter_body();
	import_starter(&mut lib, &body);
	let r = import_starter(&mut lib, &body);
	assert_eq!(r.message(), "Added 0. Had 175. Skipped 0.");
	assert_eq!(lib.collections.len(), 2, "Custom Stations and SerrebiRadio");
}

#[tokio::test]
async fn fetch_starter_against_test_server() {
	let s = serve(vec![Route::Static {
		path: "/radios.m3u".into(),
		content_type: "application/octet-stream".into(),
		body: starter_body().into_bytes(),
	}]);
	let body = fetch_playlist(&client(), &s.url("/radios.m3u"))
		.await
		.unwrap();
	assert_eq!(body, starter_body());
	let e = fetch_playlist(&client(), &s.url("/nope"))
		.await
		.unwrap_err();
	assert_eq!(e.to_string(), "HTTP 404");
}

#[test]
fn first_run_true_until_library_saved() {
	let d = tempfile::tempdir().unwrap();
	assert!(store::first_run(d.path()));
	Library::default().save(d.path()).unwrap();
	assert!(!store::first_run(d.path()));
}
