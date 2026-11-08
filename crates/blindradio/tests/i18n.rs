//! The patois checks: translation calls stay single literals (a joined
//! literal never reaches the catalog whole), and no translation invents
//! a keyboard shortcut its source doesn't have.

use std::path::Path;

#[test]
fn sources_follow_patois_rules() {
	patois_build::lint::check_sources(&[Path::new("src")]).unwrap();
}

#[test]
fn catalogs_follow_patois_rules() {
	patois_build::lint::check_catalogs(Path::new("po")).unwrap();
}
