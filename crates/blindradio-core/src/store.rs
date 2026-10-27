use std::io::Write;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::Result;

pub const SETTINGS_FILE: &str = "settings.json";
pub const LIBRARY_FILE: &str = "library.json";
pub const RESOLVE_FILE: &str = "resolve.json";

/// Portable: a `data` folder or `portable.txt` beside the exe, never on macOS.
/// Installed: the per-user config folder.
pub fn resolve_data_dir(exe_dir: &Path, config_dir: Option<PathBuf>) -> PathBuf {
	let portable = !cfg!(target_os = "macos")
		&& (exe_dir.join("data").is_dir() || exe_dir.join("portable.txt").is_file());
	if portable {
		return exe_dir.join("data");
	}
	let name = if cfg!(target_os = "linux") {
		"blindradio"
	} else {
		"BlindRadio"
	};
	match config_dir {
		Some(dir) => dir.join(name),
		// No per-user folder to use: fall back to beside the exe.
		None => exe_dir.join("data"),
	}
}

fn exe_dir() -> PathBuf {
	std::env::current_exe()
		.ok()
		.and_then(|p| p.parent().map(Path::to_owned))
		.unwrap_or_else(|| PathBuf::from("."))
}

pub fn is_portable() -> bool {
	let exe = exe_dir();
	resolve_data_dir(&exe, dirs::config_dir()) == exe.join("data")
}

/// The data folder, created if missing.
pub fn data_dir() -> PathBuf {
	let dir = resolve_data_dir(&exe_dir(), dirs::config_dir());
	let _ = std::fs::create_dir_all(&dir);
	dir
}

/// No library saved yet: the starter stations are due.
pub fn first_run(dir: &Path) -> bool {
	!dir.join(LIBRARY_FILE).exists()
}

fn sibling(path: &Path, ext: &str) -> PathBuf {
	let mut name = path.file_name().unwrap_or_default().to_owned();
	name.push(ext);
	path.with_file_name(name)
}

/// Missing gives the default. Unreadable or invalid JSON is kept as `<file>.bad`.
pub fn load<T: DeserializeOwned + Default>(path: &Path) -> T {
	let bytes = match std::fs::read(path) {
		Ok(b) => b,
		Err(e) if e.kind() == std::io::ErrorKind::NotFound => return T::default(),
		Err(_) => {
			keep_bad(path);
			return T::default();
		}
	};
	serde_json::from_slice(&bytes).unwrap_or_else(|_| {
		keep_bad(path);
		T::default()
	})
}

fn keep_bad(path: &Path) {
	let bad = sibling(path, ".bad");
	let _ = std::fs::remove_file(&bad);
	let _ = std::fs::rename(path, bad);
}

/// Writes `<file>.tmp` completely, then renames it over the target.
pub fn save<T: Serialize>(path: &Path, value: &T) -> Result<()> {
	if let Some(dir) = path.parent() {
		std::fs::create_dir_all(dir)?;
	}
	let json = serde_json::to_vec_pretty(value).map_err(std::io::Error::from)?;
	let tmp = sibling(path, ".tmp");
	let mut file = std::fs::File::create(&tmp)?;
	file.write_all(&json)?;
	file.sync_all()?;
	drop(file);
	std::fs::rename(&tmp, path)?;
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::collections::BTreeMap;

	type Map = BTreeMap<String, u32>;

	#[test]
	fn portable_when_data_folder_or_marker() {
		if cfg!(target_os = "macos") {
			return;
		}
		let cfg = Some(PathBuf::from("cfg"));
		let a = tempfile::tempdir().unwrap();
		std::fs::create_dir(a.path().join("data")).unwrap();
		assert_eq!(
			resolve_data_dir(a.path(), cfg.clone()),
			a.path().join("data")
		);
		let b = tempfile::tempdir().unwrap();
		std::fs::write(b.path().join("portable.txt"), "").unwrap();
		assert_eq!(resolve_data_dir(b.path(), cfg), b.path().join("data"));
	}

	#[test]
	fn installed_uses_config_dir() {
		let exe = tempfile::tempdir().unwrap();
		let name = if cfg!(target_os = "linux") {
			"blindradio"
		} else {
			"BlindRadio"
		};
		assert_eq!(
			resolve_data_dir(exe.path(), Some(PathBuf::from("cfg"))),
			PathBuf::from("cfg").join(name)
		);
	}

	#[test]
	fn load_missing_is_default() {
		let d = tempfile::tempdir().unwrap();
		let m: Map = load(&d.path().join("x.json"));
		assert!(m.is_empty());
	}

	#[test]
	fn corrupt_file_kept_as_bad() {
		let d = tempfile::tempdir().unwrap();
		let p = d.path().join("x.json");
		std::fs::write(d.path().join("x.json.bad"), "old").unwrap();
		std::fs::write(&p, "{not json").unwrap();
		let m: Map = load(&p);
		assert!(m.is_empty());
		assert!(!p.exists());
		assert_eq!(
			std::fs::read_to_string(d.path().join("x.json.bad")).unwrap(),
			"{not json"
		);
	}

	#[test]
	fn save_is_atomic() {
		let d = tempfile::tempdir().unwrap();
		let p = d.path().join("x.json");
		let mut m = Map::new();
		m.insert("a".into(), 1);
		save(&p, &m).unwrap();
		// A crash mid-write leaves a partial .tmp; load ignores it.
		std::fs::write(d.path().join("x.json.tmp"), "{\"a\":").unwrap();
		assert_eq!(load::<Map>(&p), m);
		m.insert("b".into(), 2);
		save(&p, &m).unwrap();
		assert_eq!(load::<Map>(&p), m);
		assert!(!d.path().join("x.json.tmp").exists());
	}
}
