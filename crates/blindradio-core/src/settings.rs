use std::collections::{BTreeMap, BTreeSet};
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
	Classical,
	Club,
	Dance,
	FullBass,
	FullBassTreble,
	FullTreble,
	LargeHall,
	Live,
	Party,
	Pop,
	Reggae,
	Rock,
	Ska,
	Soft,
	SoftRock,
	Techno,
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
	#[serde(default)]
	pub bands: [i8; 10],
	#[serde(default)]
	pub preamp_db: i8,
	#[serde(default)]
	pub preamp_enabled: bool,
	// Read old three-band presets and migrate them in Settings::load.
	#[serde(default, skip_serializing)]
	pub bass: i8,
	#[serde(default, skip_serializing)]
	pub mid: i8,
	#[serde(default, skip_serializing)]
	pub treble: i8,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct EqSettings {
	pub enabled: bool,
	pub preset: EqPreset,
	#[serde(default)]
	pub bands: [i8; 10],
	#[serde(default)]
	pub custom_bands: [i8; 10],
	#[serde(default)]
	pub preamp_db: i8,
	#[serde(default)]
	pub preamp_enabled: bool,
	#[serde(default)]
	pub custom_preamp_db: i8,
	#[serde(default)]
	pub custom_preamp_enabled: bool,
	// Read old three-band settings and migrate them in Settings::load.
	#[serde(default, skip_serializing)]
	pub bass: i8,
	#[serde(default, skip_serializing)]
	pub mid: i8,
	#[serde(default, skip_serializing)]
	pub treble: i8,
	/// The Custom preset's own bands. Named presets overwrite
	/// bass/mid/treble, so the hand-tuned set lives here and choosing
	/// Custom again restores it.
	#[serde(default, skip_serializing)]
	pub custom_bass: i8,
	#[serde(default, skip_serializing)]
	pub custom_mid: i8,
	#[serde(default, skip_serializing)]
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

/// How much audio to wait for: more rides out a bad connection, less is closer to live.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct BufferSettings {
	/// Audio held before a station starts, in milliseconds.
	pub start_ms: u32,
	/// The most a stream that keeps dropping out must refill before it sounds again, in seconds.
	pub refill_limit_seconds: u32,
	/// Segments behind the newest that a live HLS stream starts.
	pub hls_segments_behind: u32,
}

impl BufferSettings {
	pub const START_MS: (u32, u32) = (0, 30_000);
	pub const REFILL_SECONDS: (u32, u32) = (1, 120);
	pub const HLS_SEGMENTS: (u32, u32) = (1, 10);

	/// Every value inside what playback can honour.
	pub fn clamped(self) -> Self {
		BufferSettings {
			start_ms: self.start_ms.clamp(Self::START_MS.0, Self::START_MS.1),
			refill_limit_seconds: self
				.refill_limit_seconds
				.clamp(Self::REFILL_SECONDS.0, Self::REFILL_SECONDS.1),
			hls_segments_behind: self
				.hls_segments_behind
				.clamp(Self::HLS_SEGMENTS.0, Self::HLS_SEGMENTS.1),
		}
	}

	/// Puts these values in force for new streams.
	pub fn apply(self) {
		crate::ring::set_buffers(
			self.start_ms,
			self.refill_limit_seconds * 1000,
			self.hls_segments_behind,
		);
	}
}

impl Default for BufferSettings {
	fn default() -> Self {
		BufferSettings {
			start_ms: crate::ring::CUSHION_MS,
			refill_limit_seconds: crate::ring::REFILL_LIMIT_MS / 1000,
			hls_segments_behind: crate::ring::HLS_BEHIND,
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
	#[serde(serialize_with = "crate::secret::seal", deserialize_with = "crate::secret::open")]
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
	#[serde(serialize_with = "crate::secret::seal", deserialize_with = "crate::secret::open")]
	pub listenbrainz_token: Option<String>,
	#[serde(serialize_with = "crate::secret::seal", deserialize_with = "crate::secret::open")]
	pub lastfm_api_key: Option<String>,
	#[serde(serialize_with = "crate::secret::seal", deserialize_with = "crate::secret::open")]
	pub lastfm_secret: Option<String>,
	#[serde(serialize_with = "crate::secret::seal", deserialize_with = "crate::secret::open")]
	pub lastfm_session_key: Option<String>,
	/// None is last.fm itself; set to a Libre.fm URL to scrobble there.
	pub lastfm_base: Option<String>,
}

/// A station's own equalizer, chosen in Edit Station.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StationEq {
	Off,
	Preset(EqPreset),
	/// A preset the user saved, by name.
	Saved(String),
}

/// Per-station overrides; a None field inherits the global setting.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StationSettings {
	pub timeshift_minutes: Option<u32>,
	pub recognition: Option<bool>,
	pub scrobble: Option<bool>,
	/// The volume last set while this station played; applied again
	/// whenever the station starts.
	pub volume: Option<f32>,
	pub eq: Option<StationEq>,
	/// Metadata timing correction in seconds; None trusts the stream.
	pub metadata_offset_seconds: Option<i8>,
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
	/// Tell Radio Browser when one of its stations has played a while.
	pub report_plays: bool,
	pub timeshift: TimeshiftSettings,
	pub buffers: BufferSettings,
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
	/// Directories and networks turned off, by browse id such as `tunein` or `net/bbc`.
	/// Stored as what is off, so anything added later starts on.
	pub disabled_sources: BTreeSet<String>,
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
			report_plays: true,
			timeshift: TimeshiftSettings::default(),
			buffers: BufferSettings::default(),
			recording: RecordSettings::default(),
			recognition: RecognitionSettings::default(),
			scrobble: ScrobbleSettings::default(),
			station_overrides: BTreeMap::new(),
			schedules: Vec::new(),
			browse_order: BTreeMap::new(),
			disabled_sources: BTreeSet::new(),
		}
	}
}

impl Settings {
	pub fn load(dir: &Path) -> Settings {
		let mut s: Settings = store::load(&dir.join(store::SETTINGS_FILE));
		s.volume = s.volume.clamp(0.0, 1.0);
		let old_bands = |bass, mid, treble| [bass, bass, bass, mid, mid, mid, treble, treble, treble, treble];
		if s.eq.bands.iter().all(|&b| b == 0) {
			s.eq.bands = old_bands(s.eq.bass, s.eq.mid, s.eq.treble);
		}
		if s.eq.custom_bands.iter().all(|&b| b == 0) {
			s.eq.custom_bands = old_bands(s.eq.custom_bass, s.eq.custom_mid, s.eq.custom_treble);
		}
		for p in &mut s.eq.saved {
			if p.bands.iter().all(|&b| b == 0) {
				p.bands = old_bands(p.bass, p.mid, p.treble);
			}
		}
		for band in [
			&mut s.eq.preamp_db,
			&mut s.eq.custom_preamp_db,
		] {
			*band = (*band).clamp(-crate::eq::MAX_PREAMP_DB, crate::eq::MAX_PREAMP_DB);
		}
		for band in s.eq.bands.iter_mut().chain(s.eq.custom_bands.iter_mut()) {
			*band = (*band).clamp(-MAX_DB, MAX_DB);
		}
		s.eq.bass = 0;
		s.eq.mid = 0;
		s.eq.treble = 0;
		s.eq.custom_bass = 0;
		s.eq.custom_mid = 0;
		s.eq.custom_treble = 0;
		for p in &mut s.eq.saved {
			for band in p.bands.iter_mut() {
				*band = (*band).clamp(-MAX_DB, MAX_DB);
			}
			p.bass = 0;
			p.mid = 0;
			p.treble = 0;
		}
		s.buffers = s.buffers.clamped();
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
			if let Some(v) = o.volume {
				o.volume = Some(v.clamp(0.0, 1.0));
			}
			if let Some(offset) = o.metadata_offset_seconds {
				o.metadata_offset_seconds = Some(offset.clamp(-30, 30));
			}
		}
		s
	}

	/// Metadata timing correction for a station; None means use the stream's timing.
	pub fn metadata_offset_for(&self, station_id: &str) -> Option<i8> {
		self.station_overrides
			.get(station_id)
			.and_then(|o| o.metadata_offset_seconds)
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

	/// Volume for a station: its remembered level, else the global one.
	pub fn volume_for(&self, station_id: &str) -> f32 {
		self.station_overrides
			.get(station_id)
			.and_then(|o| o.volume)
			.unwrap_or(self.volume)
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
		assert_eq!(s.eq, EqSettings::default());
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
		assert_eq!(eq.bands, [16, 16, 16, -16, -16, -16, 5, 5, 5, 5]);
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
				volume: Some(0.4),
				eq: Some(StationEq::Saved("Quiet".into())),
				metadata_offset_seconds: Some(18),
			},
		);
		let json = serde_json::to_string(&s).unwrap();
		let back: Settings = serde_json::from_str(&json).unwrap();
		assert_eq!(back.station_overrides["st:1"].eq, s.station_overrides["st:1"].eq);
		assert_eq!(back.station_overrides["st:1"].metadata_offset_seconds, Some(18));
		let old: StationSettings = serde_json::from_str(r#"{"volume":0.5}"#).unwrap();
		assert_eq!(old.eq, None, "a settings file from before it means global");
		assert_eq!(s.volume_for("st:1"), 0.4);
		assert_eq!(s.metadata_offset_for("st:1"), Some(18));
		assert_eq!(s.metadata_offset_for("st:2"), None);
		assert_eq!(s.volume_for("st:2"), s.volume);
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
			bands: [3, 3, 3, -2, -2, -2, 1, 1, 1, 1],
			custom_bands: [5; 10],
			saved_name: Some("Mine".into()),
			saved: vec![crate::settings::SavedEqPreset {
				name: "Mine".into(),
				bands: [5; 10],
				preamp_db: 3,
				preamp_enabled: true,
				bass: 0,
				mid: 0,
				treble: 0,
			}],
			preamp_db: 2,
			preamp_enabled: true,
			custom_preamp_db: 4,
			custom_preamp_enabled: false,
			bass: 0,
			mid: 0,
			treble: 0,
			custom_bass: 0,
			custom_mid: 0,
			custom_treble: 0,
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
