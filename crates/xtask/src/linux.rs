//! The Linux AppImage and tarball: built inside the Ubuntu container.

use std::path::Path;

use shipfitter::package::{AppImage, TarGz};

pub fn dist(root: &Path, dist: &Path) -> Result<(), String> {
	let exe = root.join("target/release/blindradio");
	if !exe.is_file() {
		return Err(format!("{} is missing: build first", exe.display()));
	}
	std::fs::create_dir_all(dist).map_err(|e| format!("{}: {e}", dist.display()))?;
	let icon = root.join("crates/blindradio/assets/blindradio.png");
	let desktop = root.join("installer/blindradio.desktop");
	let appimage = dist.join("BlindRadio.AppImage");
	let made = AppImage {
		binaries: &[&exe],
		desktop_file: &desktop,
		icon: &icon,
		app_run: None,
	}
	.build(&appimage)
	.map_err(|e| format!("AppImage: {e}"))?;
	if !made {
		eprintln!("xtask: appimagetool was not available; the tarball is still built");
	}
	let tarball = dist.join("BlindRadio.tar.gz");
	let mut tar = TarGz::create(&tarball).map_err(|e| format!("tar: {e}"))?;
	tar.file(&exe, "BlindRadio/blindradio")
		.map_err(|e| format!("tar: {e}"))?;
	tar.file(&root.join("LICENSE"), "BlindRadio/LICENSE")
		.map_err(|e| format!("tar: {e}"))?;
	tar.file(&root.join("doc/readme.md"), "BlindRadio/readme.md")
		.map_err(|e| format!("tar: {e}"))?;
	tar.dir(&root.join("doc"), "BlindRadio/doc")
		.map_err(|e| format!("tar: {e}"))?;
	tar.finish().map_err(|e| format!("tar: {e}"))?;
	println!("xtask: {} and {}", appimage.display(), tarball.display());
	Ok(())
}
