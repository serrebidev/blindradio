use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::eq::MAX_DB;
use crate::schedule::ScheduledRecording;
use crate::station::Station;
use crate::{Result, store};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EqPreset {
	#[default]
	Flat,
	BassBoost,
	TrebleBoost,
	BassAndTreble,
	Loudness,
	Voice,
	Warm,
	Clarity,
	LaptopSpeakers,
	Custom,
}

/// A preset the user saved by name in the equalizer dialog.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SavedEqPreset {
	pub name: String,
	pub bass: i8,
	pub mid: i8,
	pub treble: i8,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct EqSettings {
	pub enabled: bool,
	pub preset: EqPreset,
	pub bass: i8,
	pub mid: i8,
	pub treble: i8,
	/// The Custom preset's own bands. Named presets overwrite
	/// bass/mid/treble, so the hand-tuned set lives here and choosing
	/// Custom again restores it.
	pub custom_bass: i8,
	pub custom_mid: i8,
	pub custom_treble: i8,
	/// The saved preset the current bands came from, when one is in use.
	pub saved_name: Option<String>,
	/// Presets the user saved by name.
	pub saved: Vec<SavedEqPreset>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TimeshiftSettings {
	pub enabled: bool,
	/// How much audio to keep behind live, per station unless overridden.
	pub minutes: u32,
	/// Optional encoded ring buffer (the DVBViewer model): the stream's
	/// own compressed bytes are kept up to `max_mb`, the oldest dropped
	/// first, and time-shifting, pause/resume and recording-from-buffer
	/// work from the ring. Off by default; the decoded minutes buffer
	/// above is the standard path.
	pub encoded_ring: bool,
	/// Ring budget in megabytes, used only when `encoded_ring` is on.
	/// 0 means the default of 200 MB (about 3.5 hours at 128 kbps).
	pub max_mb: u32,
	/// Seconds one seek action moves.
	pub seek_step_seconds: u32,
}

impl Default for TimeshiftSettings {
	fn default() -> Self {
		TimeshiftSettings {
			enabled: true,
			minutes: 30,
			encoded_ring: false,
			max_mb: 0,
			seek_step_seconds: 15,
		}
	}
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RecordFormat {
	/// The stream's own encoded bytes, untouched.
	#[default]
	Original,
	Wav,
	Mp3,
	Flac,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RecordSettings {
	pub format: RecordFormat,
	/// Constant bitrate for MP3 recording when vbr is off.
	pub mp3_bitrate_kbps: u32,
	pub mp3_vbr: bool,
	/// None is the app's default recordings folder.
	pub dir: Option<String>,
	/// Record each song to its own file: a new stream title closes
	/// the current file and opens one named for the new song.
	pub split_songs: bool,
}

impl Default for RecordSettings {
	fn default() -> Self {
		RecordSettings {
			format: RecordFormat::Original,
			mp3_bitrate_kbps: 192,
			mp3_vbr: false,
			dir: None,
			split_songs: false,
		}
	}
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RecognitionProvider {
	/// Stream metadata only (ICY / ID3); no network calls.
	#[default]
	Metadata,
	/// AudD song recognition; needs audd_token.
	Audd,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RecognitionSettings {
	pub enabled: bool,
	pub provider: RecognitionProvider,
	pub audd_token: Option<String>,
	/// Skip recognition while the stream itself supplies titles.
	pub only_when_no_metadata: bool,
	/// Speak each newly recognized song.
	pub announce: bool,
}

impl Default for RecognitionSettings {
	fn default() -> Self {
		RecognitionSettings {
			enabled: false,
			provider: RecognitionProvider::Metadata,
			audd_token: None,
			only_when_no_metadata: true,
			announce: true,
		}
	}
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ScrobbleSettings {
	pub enabled: bool,
	pub listenbrainz_token: Option<String>,
	pub lastfm_api_key: Option<String>,
	pub lastfm_secret: Option<String>,
	pub lastfm_session_key: Option<String>,
	/// None is last.fm itself; set to a Libre.fm URL to scrobble there.
	pub lastfm_base: Option<String>,
}

/// Per-station overrides; a None field inherits the global setting.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StationSettings {
	pub timeshift_minutes: Option<u32>,
	pub recognition: Option<bool>,
	pub scrobble: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
	pub announce_titles: bool,
	pub announce_status: bool,
	/// Spoken confirmations of actions and shortcuts ("Recording
	/// started", "Copied", ...). Off silences them; failures still speak.
	pub announce_actions: bool,
	pub close_lists_when_playing: bool,
	pub play_last_at_startup: bool,
	/// Moving to a station in a list plays it (dial tuning). Off: only Enter plays.
	pub play_on_move: bool,
	/// The main window's tree opens every directory at startup and on
	/// rebuilds. Off: directories start collapsed (a folder the user
	/// opens or closes by hand keeps its state across rebuilds, as
	/// always). BlindRSS has the same choice.
	pub expand_dial_folders: bool,
	/// The FM sound: the soft hiss between stations while tuning,
	/// like a dial crossing the band. On unless turned off.
	pub fm_sound: bool,
	/// Closing the window hides it to the tray instead of quitting.
	pub close_to_tray: bool,
	/// Start with the main window maximized.
	pub maximize_on_start: bool,
	/// None is the default device.
	pub audio_device: Option<String>,
	pub volume: f32,
	pub muted: bool,
	pub eq: EqSettings,
	/// Action id to chord text; empty means defaults.
	pub hotkeys: BTreeMap<String, String>,
	/// In-app shortcut overrides, KeyAction id to chord text; a
	/// missing id is its default, an empty chord is unbound.
	pub keys: BTreeMap<String, String>,
	pub last_station: Option<Station>,
	/// Collection id the main window list shows; None is the SerrebiRadio collection.
	pub main_list: Option<String>,
	/// When the daily update check last ran, in seconds since the Unix epoch.
	pub last_update_check: Option<u64>,
	/// The daily update check runs automatically; off leaves only the
	/// manual Check for Updates in the Help menu.
	pub check_updates: bool,
	pub timeshift: TimeshiftSettings,
	pub recording: RecordSettings,
	pub recognition: RecognitionSettings,
	pub scrobble: ScrobbleSettings,
	/// Keyed by station id.
	pub station_overrides: BTreeMap<String, StationSettings>,
	/// Scheduled recordings, in the order the user made them.
	pub schedules: Vec<ScheduledRecording>,
	/// Folder order the user set in Browse for levels that cannot be
	/// reordered at the source: parent folder id ("" for the roots) to
	/// its folder ids in the user's order.
	pub browse_order: BTreeMap<String, Vec<String>>,
}

impl Default for Settings {
	fn default() -> Self {
		Settings {
			announce_titles: true,
			announce_status: true,
			announce_actions: true,
			close_lists_when_playing: false,
			play_last_at_startup: false,
			play_on_move: true,
			expand_dial_folders: true,
			fm_sound: true,
			close_to_tray: false,
			maximize_on_start: false,
			audio_device: None,
			volume: 0.8,
			muted: false,
			eq: EqSettings::default(),
			hotkeys: BTreeMap::new(),
			keys: BTreeMap::new(),
			last_station: None,
			main_list: None,
			last_update_check: None,
			check_updates: true,
			timeshift: TimeshiftSettings::default(),
			recording: RecordSettings::default(),
			recognition: RecognitionSettings::default(),
			scrobble: ScrobbleSettings::default(),
			station_overrides: BTreeMap::new(),
			schedules: Vec::new(),
			browse_order: BTreeMap::new(),
		}
	}
}

impl Settings {
	pub fn load(dir: &Path) -> Settings {
		let mut s: Settings = store::load(&dir.join(store::SETTINGS_FILE));
		s.volume = s.volume.clamp(0.0, 1.0);
		for band in [
			&mut s.eq.bass,
			&mut s.eq.mid,
			&mut s.eq.treble,
			&mut s.eq.custom_bass,
			&mut s.eq.custom_mid,
			&mut s.eq.custom_treble,
		] {
			*band = (*band).clamp(-MAX_DB, MAX_DB);
		}
		for p in &mut s.eq.saved {
			for band in [&mut p.bass, &mut p.mid, &mut p.treble] {
				*band = (*band).clamp(-MAX_DB, MAX_DB);
			}
		}
		s.timeshift.minutes = s.timeshift.minutes.clamp(1, 240);
		s.timeshift.max_mb = s.timeshift.max_mb.min(16_384);
		s.timeshift.seek_step_seconds = s.timeshift.seek_step_seconds.clamp(5, 60);
		s.recording.mp3_bitrate_kbps = s.recording.mp3_bitrate_kbps.clamp(32, 320);
		for sc in &mut s.schedules {
			sc.duration_minutes = sc.duration_minutes.clamp(1, 24 * 60);
		}
		for o in s.station_overrides.values_mut() {
			if let Some(m) = o.timeshift_minutes {
				o.timeshift_minutes = Some(m.clamp(1, 240));
			}
		}
		s
	}

	/// The encoded ring budget in bytes, when the ring is enabled:
	/// `max_mb`, or the 200 MB default when it is 0. Zero when the ring
	/// is off.
	pub fn timeshift_ring_bytes(&self) -> u64 {
		if !self.timeshift.encoded_ring {
			return 0;
		}
		let mb = if self.timeshift.max_mb == 0 {
			200
		} else {
			self.timeshift.max_mb
		};
		mb as u64 * 1024 * 1024
	}

	/// Buffered minutes for a station: its override, else the global value;
	/// zero when timeshift is off.
	pub fn timeshift_minutes_for(&self, station_id: &str) -> u32 {
		if !self.timeshift.enabled {
			return 0;
		}
		self.station_overrides
			.get(station_id)
			.and_then(|o| o.timeshift_minutes)
			.unwrap_or(self.timeshift.minutes)
	}

	pub fn recognition_enabled_for(&self, station_id: &str) -> bool {
		self.station_overrides
			.get(station_id)
			.and_then(|o| o.recognition)
			.unwrap_or(self.recognition.enabled)
	}

	pub fn scrobble_enabled_for(&self, station_id: &str) -> bool {
		self.scrobble.enabled
			&& self
				.station_overrides
				.get(station_id)
				.and_then(|o| o.scrobble)
				.unwrap_or(true)
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
		assert!(s.announce_titles && s.announce_status && s.announce_actions && s.check_updates);
		assert!(!s.close_lists_when_playing && !s.play_last_at_startup && !s.muted);
		assert!(s.play_on_move && !s.close_to_tray && !s.maximize_on_start);
		assert!(s.expand_dial_folders);
		assert!(s.fm_sound, "the FM sound is on by default");
		assert_eq!(s.audio_device, None);
		assert_eq!(s.volume, 0.8);
		assert_eq!(
			s.eq,
			EqSettings {
				enabled: false,
				preset: EqPreset::Flat,
				bass: 0,
				mid: 0,
				treble: 0,
				custom_bass: 0,
				custom_mid: 0,
				custom_treble: 0,
				saved_name: None,
				saved: Vec::new()
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
	fn feature_defaults_and_overrides() {
		let mut s = Settings::default();
		assert!(s.timeshift.enabled);
		assert_eq!((s.timeshift.minutes, s.timeshift.seek_step_seconds), (30, 15));
		assert_eq!(s.recording.format, RecordFormat::Original);
		assert!(!s.recognition.enabled && !s.scrobble.enabled);
		assert_eq!(s.timeshift_minutes_for("any"), 30);
		s.station_overrides.insert(
			"st:1".into(),
			StationSettings {
				timeshift_minutes: Some(5),
				recognition: Some(true),
				scrobble: None,
			},
		);
		assert_eq!(s.timeshift_minutes_for("st:1"), 5);
		assert!(s.recognition_enabled_for("st:1"));
		assert!(!s.recognition_enabled_for("st:2"));
		s.timeshift.enabled = false;
		assert_eq!(s.timeshift_minutes_for("st:1"), 0);
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
			custom_bass: 5,
			custom_mid: 5,
			custom_treble: 5,
			saved_name: Some("Mine".into()),
			saved: vec![crate::settings::SavedEqPreset {
				name: "Mine".into(),
				bass: 5,
				mid: 5,
				treble: 5,
			}],
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
