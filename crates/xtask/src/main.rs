//! Packaging and release plumbing: `cargo xtask dist-windows`, `cargo xtask version`.

use std::path::{Path, PathBuf};
use std::process::Command;

use shipfitter::build::configure_file;
use shipfitter::docs::{self, Page};
use shipfitter::package::{self, Zip};
use shipfitter::sign::Minisign;

const APP: &str = "BlindRadio";
/// The crate the release builds.
const PKG: &str = "blindradio";
const EXE: &str = "blindradio.exe";
/// Linked dynamically: they ship beside the exe so a clean Windows 11 runs it.
const RUNTIME_DLLS: [&str; 3] = ["msvcp140.dll", "vcruntime140.dll", "vcruntime140_1.dll"];

fn root() -> PathBuf {
	Path::new(env!("CARGO_MANIFEST_DIR"))
		.join("../..")
		.canonicalize()
		.expect("the workspace root")
}

fn dist_dir() -> PathBuf {
	root().join("target").join("dist")
}

fn main() {
	let args: Vec<String> = std::env::args().skip(1).collect();
	let command = args.first().map_or("help", String::as_str);
	let done = match command {
		"dist-windows" => dist_windows(),
		"version" => match package::crate_version(&root(), PKG) {
			Ok(v) => {
				println!("{v}");
				Ok(())
			}
			Err(e) => Err(format!("version: {e}")),
		},
		_ => Err("usage: cargo xtask dist-windows | version".to_string()),
	};
	if let Err(e) = done {
		eprintln!("xtask: {e}");
		std::process::exit(1);
	}
}

fn dist_windows() -> Result<(), String> {
	let root = root();
	let dist = dist_dir();
	std::fs::create_dir_all(&dist).map_err(|e| format!("{}: {e}", dist.display()))?;
	package::cargo_build_release(&root, &[PKG]).map_err(|e| format!("building: {e}"))?;

	let exe = root.join("target/release").join(EXE);
	sign(&exe)?;

	// The portable folder: the exe, the runtime, the guide and an empty data folder.
	let portable = dist.join(APP);
	if portable.exists() {
		std::fs::remove_dir_all(&portable).map_err(|e| format!("{}: {e}", portable.display()))?;
	}
	std::fs::create_dir_all(portable.join("data")).map_err(|e| format!("data folder: {e}"))?;
	copy(&exe, &portable.join(EXE))?;
	copy(&root.join("LICENSE"), &portable.join("LICENSE"))?;
	let redist = redist_dir()?;
	for dll in RUNTIME_DLLS {
		copy(&redist.join(dll), &portable.join(dll))?;
	}
	docs::convert(
		&root.join("doc/readme.md"),
		&portable.join("readme.html"),
		&Page {
			title: APP,
			toc_depth: 2,
			..Page::default()
		},
	)
	.map_err(|e| format!("guide: {e}"))?;

	let zip_path = dist.join(format!("{APP}.zip"));
	let _ = std::fs::remove_file(&zip_path);
	let mut zip = Zip::create(&zip_path).map_err(|e| format!("zip: {e}"))?;
	for name in [EXE, "LICENSE", "readme.html"]
		.into_iter()
		.chain(RUNTIME_DLLS)
	{
		zip.file(&portable.join(name), &format!("{APP}/{name}"))
			.map_err(|e| format!("zip {name}: {e}"))?;
	}
	zip.dir(&portable.join("data"), &format!("{APP}/data"))
		.map_err(|e| format!("zip data: {e}"))?;
	zip.finish().map_err(|e| format!("zip: {e}"))?;

	// The installer, built from the same folder. Inno rejects the `\\?\` path prefix.
	let iss = dist.join("blindradio.iss");
	let app_dir = portable.to_string_lossy();
	let app_dir = app_dir.strip_prefix(r"\\?\").unwrap_or(&app_dir);
	configure_file(
		&root.join("installer/blindradio.iss.in"),
		&iss,
		&[("APP_DIR", app_dir)],
	)
	.map_err(|e| format!("installer script: {e}"))?;
	if !package::inno_setup(&iss).map_err(|e| format!("installer: {e}"))? {
		return Err("Inno Setup is missing".to_string());
	}
	let setup = dist.join(format!("{APP}_setup.exe"));
	sign(&setup)?;

	// Signatures last: both files are final.
	if let Some(key) = Minisign::from_env().map_err(|e| format!("minisign: {e}"))? {
		key.sign(&zip_path)
			.map_err(|e| format!("signing zip: {e}"))?;
		key.sign(&setup)
			.map_err(|e| format!("signing setup: {e}"))?;
	} else {
		println!("xtask: no MINISIGN_KEY, so nothing is signed with minisign");
	}
	println!("xtask: {} and {}", zip_path.display(), setup.display());
	Ok(())
}

fn copy(from: &Path, to: &Path) -> Result<(), String> {
	std::fs::copy(from, to)
		.map(|_| ())
		.map_err(|e| format!("{} -> {}: {e}", from.display(), to.display()))
}

/// The VC++ runtime the exe links against, inside the newest Build Tools redist.
fn redist_dir() -> Result<PathBuf, String> {
	if let Some(dir) = std::env::var_os("VC_REDIST_DIR") {
		return Ok(PathBuf::from(dir));
	}
	let base =
		Path::new("C:/Program Files (x86)/Microsoft Visual Studio/2022/BuildTools/VC/Redist/MSVC");
	let mut found: Vec<PathBuf> = Vec::new();
	if let Ok(entries) = std::fs::read_dir(base) {
		for entry in entries.flatten() {
			// Version folders are numbered; `v143` is a link to the newest of them.
			let name = entry.file_name().to_string_lossy().to_string();
			if !name.starts_with(|c: char| c.is_ascii_digit()) {
				continue;
			}
			let crt = entry.path().join("x64").join("Microsoft.VC143.CRT");
			if crt.join("msvcp140.dll").is_file() {
				found.push(crt);
			}
		}
	}
	found
		.into_iter()
		.max()
		.ok_or_else(|| "the VC++ runtime was not found; set VC_REDIST_DIR".to_string())
}

/// Signs with the owner's certificate; a missing one fails the release.
fn sign(path: &Path) -> Result<(), String> {
	let signtool = std::env::var_os("SIGNTOOL_PATH")
		.map(PathBuf::from)
		.map(Ok)
		.unwrap_or_else(find_signtool)?;
	let status = Command::new(&signtool)
		.args([
			"sign",
			"/fd",
			"SHA256",
			"/tr",
			"http://timestamp.digicert.com",
			"/td",
			"SHA256",
			"/a",
		])
		.arg(path)
		.status()
		.map_err(|e| format!("{}: {e}", signtool.display()))?;
	if status.success() {
		Ok(())
	} else {
		Err(format!("signtool failed on {}", path.display()))
	}
}

fn find_signtool() -> Result<PathBuf, String> {
	let base = Path::new("C:/Program Files (x86)/Windows Kits/10/bin");
	let newest = std::fs::read_dir(base)
		.map_err(|e| format!("{}: {e}", base.display()))?
		.filter_map(|entry| entry.ok().map(|e| e.path()))
		.filter(|path| path.join("x64/signtool.exe").is_file())
		.max()
		.ok_or("signtool.exe not found; set SIGNTOOL_PATH")?;
	Ok(newest.join("x64").join("signtool.exe"))
}
