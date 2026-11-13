//! The Linux AppImage and tarball: built inside the Ubuntu container.

use std::path::{Path, PathBuf};

use shipfitter::package::{AppImage, TarGz};

/// Runs the app with the bundled image libraries first. GTK and glibc come from the host.
const APP_RUN: &str = r#"#!/bin/sh
HERE="$(dirname "$(readlink -f "${0}")")"
LD_LIBRARY_PATH="${HERE}/usr/bin${LD_LIBRARY_PATH:+:${LD_LIBRARY_PATH}}"
export LD_LIBRARY_PATH
exec "${HERE}/usr/bin/blindradio" "$@"
"#;

pub fn dist(root: &Path, dist: &Path) -> Result<(), String> {
	let exe = root.join("target/release/blindradio");
	if !exe.is_file() {
		return Err(format!("{} is missing: build first", exe.display()));
	}
	std::fs::create_dir_all(dist).map_err(|e| format!("{}: {e}", dist.display()))?;
	let icon = root.join("crates/blindradio/assets/blindradio.png");
	let desktop = root.join("installer/blindradio.desktop");
	// Ubuntu 22.04's TIFF and its codec dependencies are not present on every GTK host.
	let libraries = [
		"libtiff.so.5",
		"libjpeg.so.8",
		"libwebp.so.7",
		"libzstd.so.1",
		"liblzma.so.5",
		"libjbig.so.0",
		"libdeflate.so.0",
		"libz.so.1",
	]
	.map(library)
	.into_iter()
	.collect::<Result<Vec<_>, _>>()?;
	let mut binaries = vec![exe.as_path()];
	binaries.extend(libraries.iter().map(PathBuf::as_path));
	let appimage = dist.join("BlindRadio.AppImage");
	let made = AppImage {
		binaries: &binaries,
		desktop_file: &desktop,
		icon: &icon,
		app_run: Some(APP_RUN),
	}
	.build(&appimage)
	.map_err(|e| format!("AppImage: {e}"))?;
	if !made {
		eprintln!("xtask: appimagetool was not available; the tarball is still built");
	}
	// Flat entries, like the Windows zip: the updater extracts the
	// tarball over the folder the binary already lives in.
	let tarball = dist.join("BlindRadio.tar.gz");
	let mut tar = TarGz::create(&tarball).map_err(|e| format!("tar: {e}"))?;
	tar.file(&exe, "blindradio")
		.map_err(|e| format!("tar: {e}"))?;
	tar.file(&root.join("LICENSE"), "LICENSE")
		.map_err(|e| format!("tar: {e}"))?;
	tar.file(&root.join("doc/readme.md"), "readme.md")
		.map_err(|e| format!("tar: {e}"))?;
	tar.dir(&root.join("doc"), "doc")
		.map_err(|e| format!("tar: {e}"))?;
	tar.finish().map_err(|e| format!("tar: {e}"))?;
	println!("xtask: {} and {}", appimage.display(), tarball.display());
	Ok(())
}

/// A shared library the build machine has, looked for where a distribution keeps them.
fn library(name: &str) -> Result<PathBuf, String> {
	for dir in [
		"/usr/lib/x86_64-linux-gnu",
		"/usr/lib64",
		"/usr/lib",
		"/lib/x86_64-linux-gnu",
	] {
		let path = Path::new(dir).join(name);
		if path.is_file() {
			return Ok(path);
		}
	}
	Err(format!(
		"{name} was not found; the AppImage needs it bundled"
	))
}
