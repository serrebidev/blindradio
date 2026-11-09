//! Packaging for the cloud release (`release.yml`): `cargo xtask dist-windows`, `dist-macos`,
//! `dist-linux`, `sign`, `update-check`, `minisign-key` and `version`.

use std::path::{Path, PathBuf};
use std::process::Command;

use shipfitter::build::configure_file;
use shipfitter::docs::{self, Page};
use shipfitter::package::{self, Zip};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;

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
		#[cfg(target_os = "macos")]
		"dist-macos" => macos::dist(&root(), &dist_dir()),
		#[cfg(target_os = "linux")]
		"dist-linux" => linux::dist(&root(), &dist_dir()),
		"minisign-key" => minisign_key(&args[1..]),
		"sign" => sign_assets(&args[1..]),
		"update-check" => update_check(&args[1..]),
		"version" => match package::crate_version(&root(), PKG) {
			Ok(v) => {
				println!("{v}");
				Ok(())
			}
			Err(e) => Err(format!("version: {e}")),
		},
		_ => Err("usage: cargo xtask dist-windows | dist-macos | dist-linux | version | minisign-key <dir> | sign <file>… | update-check [version]".to_string()),
	};
	if let Err(e) = done {
		eprintln!("xtask: {e}");
		std::process::exit(1);
	}
}

/// Writes `<file>.minisig` beside `file`, the signature a ship-shape update verifies.
/// `MINISIGN_KEY` holds the key, `MINISIGN_PASSWORD` unlocks it if it is encrypted. Returns
/// false when there is no key, so a build without one still runs.
pub(crate) fn sign_file(path: &Path) -> Result<bool, String> {
	let Ok(key) = std::env::var("MINISIGN_KEY") else {
		return Ok(false);
	};
	let password = std::env::var("MINISIGN_PASSWORD").ok();
	let boxed =
		minisign::SecretKeyBox::from_string(&key).map_err(|e| format!("MINISIGN_KEY: {e}"))?;
	let key = match password {
		Some(password) => boxed.into_secret_key(Some(password)),
		None => boxed.into_unencrypted_secret_key(),
	}
	.map_err(|e| format!("MINISIGN_KEY: {e}"))?;
	let name = path
		.file_name()
		.ok_or_else(|| format!("{} has no file name", path.display()))?
		.to_string_lossy();
	let timestamp = std::time::SystemTime::now()
		.duration_since(std::time::UNIX_EPOCH)
		.map(|d| d.as_secs())
		.unwrap_or_default();
	let trusted = format!("timestamp:{timestamp}\tfile:{name}\thashed");
	let reader = std::io::BufReader::new(
		std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?,
	);
	let signature = minisign::sign(None, &key, reader, Some(&trusted), None)
		.map_err(|e| format!("signing {}: {e}", path.display()))?;
	let mut out = path.as_os_str().to_owned();
	out.push(".minisig");
	std::fs::write(&out, signature.into_string())
		.map_err(|e| format!("{}: {e}", Path::new(&out).display()))?;
	Ok(true)
}

/// `cargo xtask update-check [version]`: asks Forgejo what a copy running `version` would be
/// offered, then downloads that asset and verifies it, exactly as the app does.
fn update_check(args: &[String]) -> Result<(), String> {
	let current = match args.first() {
		Some(version) => version.trim_start_matches('v').to_string(),
		None => package::crate_version(&root(), PKG).map_err(|e| format!("version: {e}"))?,
	};
	let config = ship_shape::UpdaterConfig::new("serrebi/blindradio", APP, APP, public_key()?, &current)
		.with_api_base_url("https://git.serrebiradio.com/api/v1");
	let outcome = ship_shape::check_for_updates(&config, ship_shape::UpdateChannel::Stable)
		.map_err(|e| format!("check: {e}"))?;
	let update = match outcome {
		ship_shape::UpdateCheckOutcome::UpToDate(version) => {
			println!("up to date at {version}");
			return Ok(());
		}
		ship_shape::UpdateCheckOutcome::UpdateAvailable(update) => update,
	};
	println!(
		"{current} would be offered {} from {}",
		update.latest_version, update.download_url
	);
	let cancelled = std::sync::atomic::AtomicBool::new(false);
	let path = ship_shape::download_update_file(
		&config,
		&update.download_url,
		&update.signature_url,
		&cancelled,
		|_, _| {},
	)
	.map_err(|e| format!("download: {e}"))?;
	println!("verified and saved to {}", path.display());
	Ok(())
}

/// The updater's public key, from the file the app compiles in.
fn public_key() -> Result<String, String> {
	let path = root().join("crates/blindradio/assets/minisign.pub");
	let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
	text.lines()
		.nth(1)
		.map(|line| line.trim().to_string())
		.ok_or_else(|| format!("{} has no key line", path.display()))
}

/// `cargo xtask sign <file>…`: writes `<file>.minisig` beside each.
fn sign_assets(args: &[String]) -> Result<(), String> {
	if args.is_empty() {
		return Err("sign needs at least one file".to_string());
	}
	for file in args {
		if !sign_file(Path::new(file))? {
			return Err("MINISIGN_KEY is not set".to_string());
		}
		println!("xtask: signed {file}");
	}
	Ok(())
}

/// `cargo xtask minisign-key <dir>`: writes `minisign.key` and `minisign.pub`, the pair the
/// release signs with. Unencrypted: the secret key lives in `MINISIGN_KEY` and Vaultwarden, never
/// in git.
fn minisign_key(args: &[String]) -> Result<(), String> {
	let dir = args
		.first()
		.ok_or("minisign-key needs a folder, such as ~/.minisign")?;
	let pair = minisign::KeyPair::generate_unencrypted_keypair()
		.map_err(|e| format!("generating the key: {e}"))?;
	std::fs::create_dir_all(dir).map_err(|e| format!("{dir}: {e}"))?;
	let secret = pair
		.sk
		.to_box(None)
		.map_err(|e| format!("secret key: {e}"))?
		.into_string();
	let public = pair
		.pk
		.to_box()
		.map_err(|e| format!("public key: {e}"))?
		.into_string();
	std::fs::write(Path::new(dir).join("minisign.key"), &secret)
		.map_err(|e| format!("{dir}: {e}"))?;
	std::fs::write(Path::new(dir).join("minisign.pub"), &public)
		.map_err(|e| format!("{dir}: {e}"))?;
	// The second line is what the updater is given.
	println!("{public}");
	Ok(())
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
	// NVDA's controller client (LGPL 2.1, license beside it): speech
	// through the running NVDA, so the global hot keys are heard with
	// any app in front. Loaded at runtime; absent is fine.
	for f in ["nvdaControllerClient.dll", "nvdaControllerClient-LICENSE.txt"] {
		copy(
			&root.join("crates/blindradio/assets").join(f),
			&portable.join(f),
		)?;
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

	// Flat entries, no wrapping folder: the updater extracts the zip over
	// the folder the exe already lives in, so a wrapped `BlindRadio/`
	// folder would land beside the exe instead of over it, and the old
	// exe would relaunch — the update "downloads but never applies".
	let zip_path = dist.join(format!("{APP}.zip"));
	let _ = std::fs::remove_file(&zip_path);
	let mut zip = Zip::create(&zip_path).map_err(|e| format!("zip: {e}"))?;
	for name in [EXE, "LICENSE", "readme.html"]
		.into_iter()
		.chain(RUNTIME_DLLS)
		.chain(["nvdaControllerClient.dll", "nvdaControllerClient-LICENSE.txt"])
	{
		zip.file(&portable.join(name), name)
			.map_err(|e| format!("zip {name}: {e}"))?;
	}
	zip.dir(&portable.join("data"), "data")
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
	if sign_file(&zip_path)? {
		sign_file(&setup)?;
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

/// Signs with the owner's certificate; a missing one fails the build.
/// Cloud builds set BLINDRADIO_UNSIGNED: no owner certificate exists there,
/// so the exe and the installer ship without Authenticode. The minisign
/// signatures the updater verifies are unaffected.
fn sign(path: &Path) -> Result<(), String> {
	if std::env::var_os("BLINDRADIO_UNSIGNED").is_some() {
		println!(
			"xtask: BLINDRADIO_UNSIGNED set, not Authenticode-signing {}",
			path.display()
		);
		return Ok(());
	}
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

#[cfg(test)]
mod tests {
	/// A signature this command writes is one the updater accepts, and a changed file is refused.
	#[test]
	fn signature_verifies_with_the_public_key() {
		let dir = std::env::temp_dir().join(format!("blindradio-sign-{}", std::process::id()));
		let _ = std::fs::remove_dir_all(&dir);
		std::fs::create_dir_all(&dir).unwrap();
		let asset = dir.join("BlindRadio.zip");
		std::fs::write(&asset, b"release contents").unwrap();

		let pair = minisign::KeyPair::generate_unencrypted_keypair().unwrap();
		let secret = pair.sk.to_box(None).unwrap().into_string();
		let public = pair.pk.to_box().unwrap().into_string();
		// SAFETY: the only test in this binary.
		unsafe { std::env::set_var("MINISIGN_KEY", &secret) };
		assert!(super::sign_file(&asset).unwrap());

		let public =
			minisign_verify::PublicKey::from_base64(public.lines().nth(1).unwrap()).unwrap();
		let signature = minisign_verify::Signature::decode(
			&std::fs::read_to_string(dir.join("BlindRadio.zip.minisig")).unwrap(),
		)
		.unwrap();
		public
			.verify(&std::fs::read(&asset).unwrap(), &signature, false)
			.unwrap();
		assert!(
			public.verify(b"tampered", &signature, false).is_err(),
			"a changed download must be refused"
		);
		std::fs::remove_dir_all(&dir).unwrap();
	}
}
