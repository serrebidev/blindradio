mod common;

use std::path::{Path, PathBuf};

use blindradio_core::import::{
	ImportReport, ImportTarget, STARTER_COLLECTION, STARTER_PLAYLIST_URL, export_m3u,
	fetch_playlist, import_playlist, import_starter,
};
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

fn custom(lib: &Library) -> &[blindradio_core::station::Station] {
	&lib.collections[0].stations
}

#[test]
fn import_custom_reports_counts() {
	let mut lib = Library::default();
	let r = import_playlist(&mut lib, &fixture("mixed.m3u"), ImportTarget::Custom).unwrap();
	assert_eq!(r.message(), "Added 2. Had 0. Skipped 1.");
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
	let names: Vec<_> = custom(&lib).iter().map(|s| s.name.as_str()).collect();
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
	export_m3u(custom(&lib), &p).unwrap();
	assert_eq!(
		std::fs::read_to_string(&p).unwrap(),
		"#EXTM3U\n#EXTINF:-1,Jazz FM\nhttp://a.example/jazz\n#EXTINF:-1,Caf\u{e9} Radio\nhttps://b.example/cafe\n"
	);
	let mut other = Library::default();
	let r = import_playlist(&mut other, &p, ImportTarget::Custom).unwrap();
	assert_eq!((r.added, r.had, r.skipped), (2, 0, 0));
	let a: Vec<_> = custom(&lib).iter().map(|s| (&s.name, &s.url)).collect();
	let b: Vec<_> = custom(&other).iter().map(|s| (&s.name, &s.url)).collect();
	assert_eq!(a, b);
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
	assert_eq!(c.stations.len(), 175);
	let first = &c.stations[0];
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
