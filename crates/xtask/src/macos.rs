//! The macOS app bundle and disk image: built on a macOS runner by the workflow.

use std::path::Path;

use shipfitter::macos::{MacApp, dmg, sign};

pub fn dist(root: &Path, dist: &Path) -> Result<(), String> {
	shipfitter::package::cargo_build_release(root, &["blindradio"])
		.map_err(|e| format!("building: {e}"))?;
	let exe = root.join("target/release/blindradio");
	if !exe.is_file() {
		return Err(format!("{} is missing: build first", exe.display()));
	}
	let version = shipfitter::package::crate_version(root, "blindradio")
		.map_err(|e| format!("version: {e}"))?;
	std::fs::create_dir_all(dist).map_err(|e| format!("{}: {e}", dist.display()))?;
	let app = MacApp {
		name: "BlindRadio",
		identifier: "com.serrebidev.blindradio",
		executable: "blindradio",
		version: &version,
		icon: None,
		extra: "",
	};
	let bundle = app
		.bundle(dist, &exe, &[], &[])
		.map_err(|e| format!("bundling: {e}"))?;
	// Ad-hoc: no Developer ID here, and the app is distributed unsigned.
	if !sign(&bundle, &[]).map_err(|e| format!("signing: {e}"))? {
		eprintln!("xtask: ad-hoc signing did not run");
	}
	dmg(&bundle, &dist.join("BlindRadio.dmg")).map_err(|e| format!("dmg: {e}"))?;
	println!("xtask: {}", dist.join("BlindRadio.dmg").display());
	Ok(())
}
