use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::eq::MAX_DB;
use crate::station::Station;
use crate::{Result, store};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EqPreset {
	#[default]
	Flat,
	BassBoost,
	TrebleBoost,
	Loudness,
	Voice,
	Warm,
	Custom,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct EqSettings {
	pub enabled: bool,
	pub preset: EqPreset,
	pub bass: i8,
	pub mid: i8,
	pub treble: i8,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
	pub announce_titles: bool,
	pub announce_status: bool,
	pub close_lists_when_playing: bool,
	pub play_last_at_startup: bool,
	/// None is the default device.
	pub audio_device: Option<String>,
	pub volume: f32,
	pub muted: bool,
	pub eq: EqSettings,
	/// Action id to chord text; empty means defaults.
	pub hotkeys: BTreeMap<String, String>,
	pub last_station: Option<Station>,
	/// Collection id the main window list shows; None is the SerrebiRadio collection.
	pub main_list: Option<String>,
	/// When the daily update check last ran, in seconds since the Unix epoch.
	pub last_update_check: Option<u64>,
}

impl Default for Settings {
	fn default() -> Self {
		Settings {
			announce_titles: true,
			announce_status: true,
			close_lists_when_playing: false,
			play_last_at_startup: false,
			audio_device: None,
			volume: 0.8,
			muted: false,
			eq: EqSettings::default(),
			hotkeys: BTreeMap::new(),
			last_station: None,
			main_list: None,
			last_update_check: None,
		}
	}
}

impl Settings {
	pub fn load(dir: &Path) -> Settings {
		let mut s: Settings = store::load(&dir.join(store::SETTINGS_FILE));
		s.volume = s.volume.clamp(0.0, 1.0);
		for band in [&mut s.eq.bass, &mut s.eq.mid, &mut s.eq.treble] {
			*band = (*band).clamp(-MAX_DB, MAX_DB);
		}
		s
	}

	pub fn save(&self, dir: &Path) -> Result<()> {
		store::save(&dir.join(store::SETTINGS_FILE), self)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn defaults_match_spec() {
		let s = Settings::default();
		assert!(s.announce_titles && s.announce_status);
		assert!(!s.close_lists_when_playing && !s.play_last_at_startup && !s.muted);
		assert_eq!(s.audio_device, None);
		assert_eq!(s.volume, 0.8);
		assert_eq!(
			s.eq,
			EqSettings {
				enabled: false,
				preset: EqPreset::Flat,
				bass: 0,
				mid: 0,
				treble: 0
			}
		);
		assert!(s.hotkeys.is_empty());
		assert_eq!(s.last_station, None);
		assert_eq!(s.main_list, None);
		assert_eq!(s.last_update_check, None);
	}

	#[test]
	fn unknown_and_missing_fields_tolerated() {
		let d = tempfile::tempdir().unwrap();
		std::fs::write(
			d.path().join("settings.json"),
			r#"{"volume":2.0,"future":1}"#,
		)
		.unwrap();
		let s = Settings::load(d.path());
		assert_eq!(
			s,
			Settings {
				volume: 1.0,
				..Settings::default()
			}
		);
		assert!(!d.path().join("settings.json.bad").exists());
	}

	#[test]
	fn eq_bands_clamped_on_load() {
		let d = tempfile::tempdir().unwrap();
		std::fs::write(
			d.path().join("settings.json"),
			r#"{"eq":{"bass":20,"mid":-30,"treble":5}}"#,
		)
		.unwrap();
		let eq = Settings::load(d.path()).eq;
		assert_eq!((eq.bass, eq.mid, eq.treble), (12, -12, 5));
	}

	#[test]
	fn round_trip() {
		let d = tempfile::tempdir().unwrap();
		let mut s = Settings {
			audio_device: Some("Speakers".into()),
			volume: 0.25,
			muted: true,
			..Settings::default()
		};
		s.eq = EqSettings {
			enabled: true,
			preset: EqPreset::BassBoost,
			bass: 3,
			mid: -2,
			treble: 1,
		};
		s.hotkeys.insert("play".into(), "Ctrl+P".into());
		s.main_list = Some("col:1".into());
		s.save(d.path()).unwrap();
		assert!(
			std::fs::read_to_string(d.path().join("settings.json"))
				.unwrap()
				.contains("bass-boost")
		);
		assert_eq!(Settings::load(d.path()), s);
	}
}
