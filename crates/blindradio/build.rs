use std::path::{Path, PathBuf};

use shipfitter::docs::{self, Page};
use shipfitter::windows::{VersionInfo, embed_manifest};

fn commit_hash() -> String {
	std::process::Command::new("git")
		.args(["rev-parse", "--short", "HEAD"])
		.output()
		.ok()
		.filter(|out| out.status.success())
		.map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
		.unwrap_or_else(|| "unknown".to_string())
}

fn main() {
	println!("cargo:rerun-if-changed=build.rs");
	println!("cargo:rerun-if-changed=../../doc/readme.md");
	println!("cargo:rerun-if-changed=assets/blindradio.ico");

	let hash = commit_hash();
	println!("cargo:rustc-env=BLINDRADIO_COMMIT_HASH={hash}");

	// The updater's public key: the second line of the minisign public key beside the assets.
	println!("cargo:rerun-if-changed=assets/minisign.pub");
	let public =
		std::fs::read_to_string("assets/minisign.pub").expect("assets/minisign.pub is missing");
	let public = public.lines().nth(1).unwrap_or_default().trim();
	println!("cargo:rustc-env=BLINDRADIO_MINISIGN_PUBLIC_KEY={public}");

	// The user guide, converted once here and inflated at run time.
	let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
	let page = Page {
		title: "BlindRadio",
		toc_depth: 2,
		..Page::default()
	};
	if let Err(e) = docs::convert_deflated(
		Path::new("../../doc/readme.md"),
		&out.join("readme.html"),
		&page,
	) {
		panic!("the user guide does not convert: {e}");
	}

	if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
		if let Err(e) = embed_manifest("BlindRadio") {
			println!("cargo:warning=manifest not embedded: {e}");
		}
		let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
		let product_version = format!("{version} ({hash})");
		let info = VersionInfo {
			product_name: "BlindRadio",
			company: "serrebidev",
			copyright: "Copyright (c) 2026 serrebidev",
			original_filename: "blindradio.exe",
			product_version: Some(&product_version),
			icon: Some("assets/blindradio.ico"),
		};
		if let Err(e) = info.embed() {
			println!("cargo:warning=version resource not embedded: {e}");
		}
	}
}
