//! In-app keyboard shortcuts: every key the main window and the
//! dialogs answer to, as data the user can edit (File > Keyboard
//! Shortcuts…). Until this existed the table was a hardcoded match
//! in action.rs and menu items without a key could never gain one;
//! now defaults live here, overrides live in Settings.keys, and the
//! menus print the effective chord. A binding is one chord for one
//! action: actions with several keys (Skip Back on Ctrl+Left and on
//! F6) have several bindings, each its own row.

use std::collections::BTreeMap;

use blindradio_core::settings::Settings;
use blindradio_core::sleep::SLEEP_MINUTES;
use wx_utils::shortcuts::KeyChord;

use crate::action::Action;
use crate::menus::plain;
use crate::strings as s;

/// Where a binding fires.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Scope {
	/// The main window and dialogs alike.
	Both,
	/// The main window only; dialogs keep the key for their controls.
	MainOnly,
	/// Dialogs only (Escape closes them; the main window ignores it).
	DialogOnly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum KeyAction {
	PlayPause,
	Play,
	Stop,
	PreviousStation,
	NextStation,
	Record,
	RecordPause,
	RecordSongs,
	SaveBuffer,
	ScheduledRecordings,
	SeekBack,
	SeekBackAlt,
	SeekForward,
	SeekForwardAlt,
	SeekStart,
	SeekStartAlt,
	GoLive,
	GoLiveAlt,
	GoLiveF8,
	Mute,
	VolumeUp,
	VolumeDown,
	ToggleFavorite,
	AddToCollection,
	SongHistory,
	Sleep15,
	Sleep30,
	Sleep45,
	Sleep60,
	Sleep90,
	Sleep120,
	SleepOff,
	Equalizer,
	FocusVolume,
	OpenSearch,
	OpenBrowse,
	OpenFavorites,
	OpenRecent,
	OpenCollections,
	NewCustomStation,
	CopyUrl,
	CopyStation,
	CutStation,
	PasteStation,
	EditStation,
	StationSettings,
	OpenUrl,
	GetStarter,
	PlayFavorite1,
	PlayFavorite2,
	PlayFavorite3,
	PlayFavorite4,
	PlayFavorite5,
	PlayFavorite6,
	PlayFavorite7,
	PlayFavorite8,
	PlayFavorite9,
	PlayFavorite10,
	Import,
	ImportAsCollection,
	FeatureSettings,
	GlobalHotkeys,
	KeyboardShortcuts,
	CheckForUpdates,
	Help,
	About,
	Exit,
	Close,
}

use KeyAction as K;

impl KeyAction {
	/// Editor order: Playback tab, Stations tab, App tab.
	pub const ALL: [KeyAction; 68] = [
		K::PlayPause,
		K::Play,
		K::Stop,
		K::PreviousStation,
		K::NextStation,
		K::Record,
		K::RecordPause,
		K::RecordSongs,
		K::SaveBuffer,
		K::ScheduledRecordings,
		K::SeekBack,
		K::SeekBackAlt,
		K::SeekForward,
		K::SeekForwardAlt,
		K::SeekStart,
		K::SeekStartAlt,
		K::GoLive,
		K::GoLiveAlt,
		K::GoLiveF8,
		K::Mute,
		K::VolumeUp,
		K::VolumeDown,
		K::ToggleFavorite,
		K::AddToCollection,
		K::SongHistory,
		K::Sleep15,
		K::Sleep30,
		K::Sleep45,
		K::Sleep60,
		K::Sleep90,
		K::Sleep120,
		K::SleepOff,
		K::Equalizer,
		K::FocusVolume,
		K::OpenSearch,
		K::OpenBrowse,
		K::OpenFavorites,
		K::OpenRecent,
		K::OpenCollections,
		K::NewCustomStation,
		K::CopyUrl,
		K::CopyStation,
		K::CutStation,
		K::PasteStation,
		K::EditStation,
		K::StationSettings,
		K::OpenUrl,
		K::GetStarter,
		K::PlayFavorite1,
		K::PlayFavorite2,
		K::PlayFavorite3,
		K::PlayFavorite4,
		K::PlayFavorite5,
		K::PlayFavorite6,
		K::PlayFavorite7,
		K::PlayFavorite8,
		K::PlayFavorite9,
		K::PlayFavorite10,
		K::Import,
		K::ImportAsCollection,
		K::FeatureSettings,
		K::GlobalHotkeys,
		K::KeyboardShortcuts,
		K::CheckForUpdates,
		K::Help,
		K::About,
		K::Exit,
		K::Close,
	];

	/// The editor tab: 0 Playback, 1 Stations, 2 App.
	pub fn tab(self) -> usize {
		match self {
			K::OpenSearch
			| K::OpenBrowse
			| K::OpenFavorites
			| K::OpenRecent
			| K::OpenCollections
			| K::NewCustomStation
			| K::CopyUrl
			| K::CopyStation
			| K::CutStation
			| K::PasteStation
			| K::EditStation
			| K::StationSettings
			| K::OpenUrl
			| K::GetStarter
			| K::PlayFavorite1
			| K::PlayFavorite2
			| K::PlayFavorite3
			| K::PlayFavorite4
			| K::PlayFavorite5
			| K::PlayFavorite6
			| K::PlayFavorite7
			| K::PlayFavorite8
			| K::PlayFavorite9
			| K::PlayFavorite10 => 1,
			K::Import
			| K::ImportAsCollection
			| K::FeatureSettings
			| K::GlobalHotkeys
			| K::KeyboardShortcuts
			| K::CheckForUpdates
			| K::Help
			| K::About
			| K::Exit
			| K::Close => 2,
			_ => 0,
		}
	}

	/// The name settings.json stores.
	pub fn id(self) -> &'static str {
		match self {
			K::PlayPause => "play_pause",
			K::Play => "play",
			K::Stop => "stop",
			K::PreviousStation => "previous_station",
			K::NextStation => "next_station",
			K::Record => "record",
			K::RecordPause => "record_pause",
			K::RecordSongs => "record_songs",
			K::SaveBuffer => "save_buffer",
			K::ScheduledRecordings => "scheduled_recordings",
			K::SeekBack => "seek_back",
			K::SeekBackAlt => "seek_back_alt",
			K::SeekForward => "seek_forward",
			K::SeekForwardAlt => "seek_forward_alt",
			K::SeekStart => "seek_start",
			K::SeekStartAlt => "seek_start_alt",
			K::GoLive => "go_live",
			K::GoLiveAlt => "go_live_alt",
			K::GoLiveF8 => "go_live_f8",
			K::Mute => "mute",
			K::VolumeUp => "volume_up",
			K::VolumeDown => "volume_down",
			K::ToggleFavorite => "toggle_favorite",
			K::AddToCollection => "add_to_collection",
			K::SongHistory => "song_history",
			K::Sleep15 => "sleep_15",
			K::Sleep30 => "sleep_30",
			K::Sleep45 => "sleep_45",
			K::Sleep60 => "sleep_60",
			K::Sleep90 => "sleep_90",
			K::Sleep120 => "sleep_120",
			K::SleepOff => "sleep_off",
			K::Equalizer => "equalizer",
			K::FocusVolume => "focus_volume",
			K::OpenSearch => "open_search",
			K::OpenBrowse => "open_browse",
			K::OpenFavorites => "open_favorites",
			K::OpenRecent => "open_recent",
			K::OpenCollections => "open_collections",
			K::NewCustomStation => "new_custom_station",
			K::CopyUrl => "copy_url",
			K::CopyStation => "copy_station",
			K::CutStation => "cut_station",
			K::PasteStation => "paste_station",
			K::EditStation => "edit_station",
			K::StationSettings => "station_settings",
			K::OpenUrl => "open_url",
			K::GetStarter => "get_starter",
			K::PlayFavorite1 => "play_favorite_1",
			K::PlayFavorite2 => "play_favorite_2",
			K::PlayFavorite3 => "play_favorite_3",
			K::PlayFavorite4 => "play_favorite_4",
			K::PlayFavorite5 => "play_favorite_5",
			K::PlayFavorite6 => "play_favorite_6",
			K::PlayFavorite7 => "play_favorite_7",
			K::PlayFavorite8 => "play_favorite_8",
			K::PlayFavorite9 => "play_favorite_9",
			K::PlayFavorite10 => "play_favorite_10",
			K::Import => "import",
			K::ImportAsCollection => "import_as_collection",
			K::FeatureSettings => "feature_settings",
			K::GlobalHotkeys => "global_hotkeys",
			K::KeyboardShortcuts => "keyboard_shortcuts",
			K::CheckForUpdates => "check_for_updates",
			K::Help => "help",
			K::About => "about",
			K::Exit => "exit",
			K::Close => "close",
		}
	}

	/// The row name in the editor.
	pub fn label(self) -> String {
		let alt = |base: &'static str| format!("{}{}", plain(base), s::KB_ALTERNATE);
		let sleep = |i: usize| {
			format!(
				"{}: {}",
				plain(s::SLEEP_TIMER),
				plain(s::SLEEP_ITEMS[i])
			)
		};
		let favorite = |n: usize| format!("{} {n}", s::KB_PLAY_FAVORITE);
		match self {
			K::PlayPause => plain(s::PLAY_PAUSE),
			K::Play => s::PLAY.to_string(),
			K::Stop => plain(s::STOP_MENU),
			K::PreviousStation => plain(s::PREVIOUS_STATION),
			K::NextStation => plain(s::NEXT_STATION),
			K::Record => plain(s::RECORD),
			K::RecordPause => plain(s::RECORD_PAUSE),
			K::RecordSongs => plain(s::RECORD_SONGS),
			K::SaveBuffer => plain(s::SAVE_BUFFER),
			K::ScheduledRecordings => plain(s::SCHEDULED_RECORDINGS),
			K::SeekBack => plain(s::SKIP_BACK),
			K::SeekBackAlt => alt(s::SKIP_BACK),
			K::SeekForward => plain(s::SKIP_FORWARD),
			K::SeekForwardAlt => alt(s::SKIP_FORWARD),
			K::SeekStart => plain(s::JUMP_START),
			K::SeekStartAlt => alt(s::JUMP_START),
			K::GoLive => plain(s::GO_LIVE),
			K::GoLiveAlt | K::GoLiveF8 => alt(s::GO_LIVE),
			K::Mute => plain(s::MUTE_MENU),
			K::VolumeUp => plain(s::VOLUME_UP),
			K::VolumeDown => plain(s::VOLUME_DOWN),
			K::ToggleFavorite => plain(s::ADD_TO_FAVORITES),
			K::AddToCollection => plain(s::ADD_TO_COLLECTION),
			K::SongHistory => plain(s::SONG_HISTORY),
			K::Sleep15 => sleep(0),
			K::Sleep30 => sleep(1),
			K::Sleep45 => sleep(2),
			K::Sleep60 => sleep(3),
			K::Sleep90 => sleep(4),
			K::Sleep120 => sleep(5),
			K::SleepOff => format!("{}: {}", plain(s::SLEEP_TIMER), plain(s::SLEEP_OFF)),
			K::Equalizer => plain(s::EQUALIZER),
			K::FocusVolume => s::KB_FOCUS_VOLUME.to_string(),
			K::OpenSearch => plain(s::SEARCH),
			K::OpenBrowse => plain(s::BROWSE),
			K::OpenFavorites => plain(s::FAVORITES),
			K::OpenRecent => plain(s::RECENT),
			K::OpenCollections => plain(s::COLLECTIONS),
			K::NewCustomStation => plain(s::ADD_CUSTOM),
			K::CopyUrl => plain(s::COPY_URL),
			K::CopyStation => plain(s::COPY_STATION_ITEM),
			K::CutStation => plain(s::CUT_STATION_ITEM),
			K::PasteStation => plain(s::PASTE_STATION_ITEM),
			K::EditStation => plain(s::EDIT_STATION_ITEM),
			K::StationSettings => plain(s::STATION_SETTINGS_ITEM),
			K::OpenUrl => plain(s::OPEN_URL),
			K::GetStarter => plain(s::GET_STARTER),
			K::PlayFavorite1 => favorite(1),
			K::PlayFavorite2 => favorite(2),
			K::PlayFavorite3 => favorite(3),
			K::PlayFavorite4 => favorite(4),
			K::PlayFavorite5 => favorite(5),
			K::PlayFavorite6 => favorite(6),
			K::PlayFavorite7 => favorite(7),
			K::PlayFavorite8 => favorite(8),
			K::PlayFavorite9 => favorite(9),
			K::PlayFavorite10 => favorite(10),
			K::Import => plain(s::IMPORT),
			K::ImportAsCollection => plain(s::IMPORT_AS_COLLECTION),
			K::FeatureSettings => plain(s::FEATURE_SETTINGS),
			K::GlobalHotkeys => plain(s::GLOBAL_HOTKEYS),
			K::KeyboardShortcuts => plain(s::KEYBOARD_SHORTCUTS),
			K::CheckForUpdates => plain(s::CHECK_UPDATES),
			K::Help => plain(s::USER_GUIDE),
			K::About => plain(s::ABOUT),
			K::Exit => plain(s::EXIT),
			K::Close => s::KB_CLOSE_DIALOG.to_string(),
		}
	}

	/// The chord a fresh install binds; `None` is deliberately unbound.
	pub fn default_chord(self) -> Option<&'static str> {
		Some(match self {
			K::PlayPause => "Ctrl+P",
			K::Stop => "Ctrl+S",
			K::PreviousStation => "Alt+Left",
			K::NextStation => "Alt+Right",
			K::Record => "Ctrl+R",
			K::RecordPause => "Ctrl+Shift+R",
			K::SeekBack => "Ctrl+Left",
			K::SeekBackAlt => "F6",
			K::SeekForward => "Ctrl+Right",
			K::SeekForwardAlt => "F7",
			K::SeekStart => "Ctrl+Shift+Left",
			K::SeekStartAlt => "F5",
			K::GoLive => "Ctrl+J",
			K::GoLiveAlt => "Ctrl+Shift+Right",
			K::GoLiveF8 => "F8",
			K::Mute => "Ctrl+M",
			K::VolumeUp => "Ctrl+Up",
			K::VolumeDown => "Ctrl+Down",
			K::ToggleFavorite => "Ctrl+D",
			K::AddToCollection => "Ctrl+Shift+L",
			K::SongHistory => "Ctrl+Y",
			K::Sleep15 => "Ctrl+1",
			K::Sleep30 => "Ctrl+2",
			K::Sleep45 => "Ctrl+3",
			K::Sleep60 => "Ctrl+4",
			K::Sleep90 => "Ctrl+5",
			K::Sleep120 => "Ctrl+6",
			K::SleepOff => "Ctrl+0",
			K::Equalizer => "Ctrl+E",
			K::FocusVolume => "Alt+V",
			K::OpenSearch => "Ctrl+F",
			K::OpenBrowse => "Ctrl+G",
			K::OpenFavorites => "Ctrl+B",
			K::OpenRecent => "Ctrl+H",
			K::OpenCollections => "Ctrl+L",
			K::NewCustomStation => "Ctrl+N",
			K::CopyUrl => "Ctrl+U",
			K::CopyStation => "Ctrl+C",
			K::CutStation => "Ctrl+X",
			K::PasteStation => "Ctrl+V",
			K::Import => "Ctrl+I",
			K::ImportAsCollection => "Ctrl+Shift+I",
			K::FeatureSettings => "Ctrl+T",
			K::Help => "F1",
			K::Close => "Escape",
			K::PlayFavorite1 => "Alt+1",
			K::PlayFavorite2 => "Alt+2",
			K::PlayFavorite3 => "Alt+3",
			K::PlayFavorite4 => "Alt+4",
			K::PlayFavorite5 => "Alt+5",
			K::PlayFavorite6 => "Alt+6",
			K::PlayFavorite7 => "Alt+7",
			K::PlayFavorite8 => "Alt+8",
			K::PlayFavorite9 => "Alt+9",
			K::PlayFavorite10 => "Alt+0",
			_ => return None,
		})
	}

	/// What pressing the key does.
	pub fn action(self) -> Action {
		match self {
			K::PlayPause => Action::PlayPause,
			K::Play => Action::Play,
			K::Stop => Action::Stop,
			K::PreviousStation => Action::PreviousStation,
			K::NextStation => Action::NextStation,
			K::Record => Action::Record,
			K::RecordPause => Action::RecordPause,
			K::RecordSongs => Action::RecordSongs,
			K::SaveBuffer => Action::SaveBuffer,
			K::ScheduledRecordings => Action::ScheduledRecordings,
			K::SeekBack | K::SeekBackAlt => Action::SeekBack,
			K::SeekForward | K::SeekForwardAlt => Action::SeekForward,
			K::SeekStart | K::SeekStartAlt => Action::SeekStart,
			K::GoLive | K::GoLiveAlt | K::GoLiveF8 => Action::GoLive,
			K::Mute => Action::Mute,
			K::VolumeUp => Action::VolumeUp,
			K::VolumeDown => Action::VolumeDown,
			K::ToggleFavorite => Action::ToggleFavorite,
			K::AddToCollection => Action::AddToCollection,
			K::SongHistory => Action::SongHistory,
			K::Sleep15 => Action::Sleep(SLEEP_MINUTES[0]),
			K::Sleep30 => Action::Sleep(SLEEP_MINUTES[1]),
			K::Sleep45 => Action::Sleep(SLEEP_MINUTES[2]),
			K::Sleep60 => Action::Sleep(SLEEP_MINUTES[3]),
			K::Sleep90 => Action::Sleep(SLEEP_MINUTES[4]),
			K::Sleep120 => Action::Sleep(SLEEP_MINUTES[5]),
			K::SleepOff => Action::SleepOff,
			K::Equalizer => Action::Equalizer,
			K::FocusVolume => Action::FocusVolume,
			K::OpenSearch => Action::OpenSearch,
			K::OpenBrowse => Action::OpenBrowse,
			K::OpenFavorites => Action::OpenFavorites,
			K::OpenRecent => Action::OpenRecent,
			K::OpenCollections => Action::OpenCollections,
			K::NewCustomStation => Action::NewCustomStation,
			K::CopyUrl => Action::CopyUrl,
			K::CopyStation => Action::CopyStation,
			K::CutStation => Action::CutStation,
			K::PasteStation => Action::PasteStation,
			K::EditStation => Action::EditStation,
			K::StationSettings => Action::StationSettings,
			K::OpenUrl => Action::OpenUrl,
			K::GetStarter => Action::GetStarter,
			K::PlayFavorite1 => Action::PlayFavorite(0),
			K::PlayFavorite2 => Action::PlayFavorite(1),
			K::PlayFavorite3 => Action::PlayFavorite(2),
			K::PlayFavorite4 => Action::PlayFavorite(3),
			K::PlayFavorite5 => Action::PlayFavorite(4),
			K::PlayFavorite6 => Action::PlayFavorite(5),
			K::PlayFavorite7 => Action::PlayFavorite(6),
			K::PlayFavorite8 => Action::PlayFavorite(7),
			K::PlayFavorite9 => Action::PlayFavorite(8),
			K::PlayFavorite10 => Action::PlayFavorite(9),
			K::Import => Action::Import,
			K::ImportAsCollection => Action::ImportAsCollection,
			K::FeatureSettings => Action::FeatureSettings,
			K::GlobalHotkeys => Action::GlobalHotkeys,
			K::KeyboardShortcuts => Action::KeyboardShortcuts,
			K::CheckForUpdates => Action::CheckForUpdates,
			K::Help => Action::Help,
			K::About => Action::About,
			K::Exit => Action::Exit,
			K::Close => Action::Close,
		}
	}

	pub fn scope(self) -> Scope {
		match self {
			K::PreviousStation
			| K::NextStation
			| K::AddToCollection
			| K::FocusVolume
			| K::Import
			| K::ImportAsCollection
			| K::PlayFavorite1
			| K::PlayFavorite2
			| K::PlayFavorite3
			| K::PlayFavorite4
			| K::PlayFavorite5
			| K::PlayFavorite6
			| K::PlayFavorite7
			| K::PlayFavorite8
			| K::PlayFavorite9
			| K::PlayFavorite10 => Scope::MainOnly,
			K::Close => Scope::DialogOnly,
			_ => Scope::Both,
		}
	}
}

/// The chord bound to `key`: the user's, else the default. `None` is cleared.
pub fn bound(settings: &Settings, key: KeyAction) -> Option<KeyChord> {
	match settings.keys.get(key.id()) {
		Some(text) => KeyChord::parse(text),
		None => key.default_chord().and_then(KeyChord::parse),
	}
}

/// The key event's action under the user's bindings: the replacement
/// for the old hardcoded app_key table, with the same scope rules.
pub fn lookup(
	settings: &Settings,
	code: i32,
	ctrl: bool,
	alt: bool,
	shift: bool,
	main_window: bool,
) -> Option<Action> {
	for key in KeyAction::ALL {
		match (key.scope(), main_window) {
			(Scope::MainOnly, false) | (Scope::DialogOnly, true) => continue,
			_ => {}
		}
		if bound(settings, key).is_some_and(|chord| chord.matches(code, ctrl, alt, shift)) {
			return Some(key.action());
		}
	}
	None
}

/// The binding whose chord a menu item prints: for actions with
/// several bindings, the one the menus have always shown (the F-row
/// transport keys, not the Ctrl+arrow twins).
pub fn menu_key(action: &Action) -> Option<KeyAction> {
	Some(match action {
		Action::PlayPause => K::PlayPause,
		Action::Stop => K::Stop,
		Action::PreviousStation => K::PreviousStation,
		Action::NextStation => K::NextStation,
		Action::Record => K::Record,
		Action::RecordPause => K::RecordPause,
		Action::RecordSongs => K::RecordSongs,
		Action::SaveBuffer => K::SaveBuffer,
		Action::ScheduledRecordings => K::ScheduledRecordings,
		Action::SeekBack => K::SeekBackAlt,
		Action::SeekForward => K::SeekForwardAlt,
		Action::SeekStart => K::SeekStartAlt,
		Action::GoLive => K::GoLiveF8,
		Action::Mute => K::Mute,
		Action::VolumeUp => K::VolumeUp,
		Action::VolumeDown => K::VolumeDown,
		Action::ToggleFavorite => K::ToggleFavorite,
		Action::AddToCollection => K::AddToCollection,
		Action::SongHistory => K::SongHistory,
		Action::Sleep(minutes) => match *minutes {
			15 => K::Sleep15,
			30 => K::Sleep30,
			45 => K::Sleep45,
			60 => K::Sleep60,
			90 => K::Sleep90,
			_ => K::Sleep120,
		},
		Action::SleepOff => K::SleepOff,
		Action::Equalizer => K::Equalizer,
		Action::OpenSearch => K::OpenSearch,
		Action::OpenBrowse => K::OpenBrowse,
		Action::OpenFavorites => K::OpenFavorites,
		Action::OpenRecent => K::OpenRecent,
		Action::OpenCollections => K::OpenCollections,
		Action::NewCustomStation => K::NewCustomStation,
		Action::CopyUrl => K::CopyUrl,
		Action::OpenUrl => K::OpenUrl,
		Action::GetStarter => K::GetStarter,
		Action::Import => K::Import,
		Action::ImportAsCollection => K::ImportAsCollection,
		Action::FeatureSettings => K::FeatureSettings,
		Action::GlobalHotkeys => K::GlobalHotkeys,
		Action::KeyboardShortcuts => K::KeyboardShortcuts,
		Action::CheckForUpdates => K::CheckForUpdates,
		Action::Help => K::Help,
		Action::About => K::About,
		Action::Exit => K::Exit,
		_ => return None,
	})
}

/// What a menu prints after the label for `action` right now.
pub fn menu_chord(settings: &Settings, action: &Action) -> String {
	menu_key(action)
		.and_then(|key| bound(settings, key))
		.map(|chord| chord.to_display_string())
		.unwrap_or_default()
}

/// The editor's copy of the bindings: what the user set, over the defaults.
#[derive(Clone, Default)]
pub struct Model {
	bindings: BTreeMap<String, Option<KeyChord>>,
}

impl Model {
	pub fn from_settings(settings: &Settings) -> Model {
		Model {
			bindings: settings
				.keys
				.iter()
				.map(|(id, text)| (id.clone(), KeyChord::parse(text)))
				.collect(),
		}
	}

	/// Back into settings: a cleared chord is stored empty, which parses to nothing.
	pub fn to_settings(&self) -> BTreeMap<String, String> {
		self.bindings
			.iter()
			.map(|(id, chord)| {
				let text = chord
					.as_ref()
					.map_or_else(String::new, KeyChord::to_shortcut_string);
				(id.clone(), text)
			})
			.collect()
	}
}

impl wx_utils::shortcuts::ShortcutModel for Model {
	type Action = KeyAction;

	fn tabs(&self) -> Vec<String> {
		vec![
			plain(s::MENU_PLAYBACK),
			plain(s::MENU_STATIONS),
			s::KB_TAB_APP.to_string(),
		]
	}

	fn tab_scope(&self, _tab: usize) -> wx_utils::shortcuts::TabScope {
		// One keymap, three category tabs: they share a mode, so a
		// chord taken under Playback conflicts under Stations too.
		wx_utils::shortcuts::TabScope::Mode(0)
	}

	fn actions(&self, tab: usize) -> Vec<KeyAction> {
		KeyAction::ALL
			.into_iter()
			.filter(|key| key.tab() == tab)
			.collect()
	}

	fn action_name(&self, action: KeyAction) -> String {
		action.label()
	}

	fn chord(&self, _tab: usize, action: KeyAction) -> Option<KeyChord> {
		match self.bindings.get(action.id()) {
			Some(bound) => bound.clone(),
			None => action.default_chord().and_then(KeyChord::parse),
		}
	}

	fn set_chord(&mut self, _tab: usize, action: KeyAction, chord: Option<KeyChord>) {
		self.bindings.insert(action.id().to_string(), chord);
	}

	fn reset_action(&mut self, _tab: usize, action: KeyAction) {
		self.bindings.remove(action.id());
	}

	fn reset_all(&mut self, _tab: usize) {
		self.bindings.clear();
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use wxdragon::prelude::{WXK_DOWN, WXK_ESCAPE, WXK_F5, WXK_LEFT, WXK_RIGHT, WXK_UP};

	fn key(c: char) -> i32 {
		c as i32
	}

	#[test]
	fn every_id_unique_and_tabbed() {
		let mut ids: Vec<&str> = KeyAction::ALL.iter().map(|k| k.id()).collect();
		ids.sort_unstable();
		ids.dedup();
		assert_eq!(ids.len(), KeyAction::ALL.len());
		for tab in 0..3 {
			assert!(
				KeyAction::ALL.iter().any(|k| k.tab() == tab),
				"tab {tab} empty"
			);
		}
	}

	#[test]
	fn defaults_match_the_old_table() {
		let settings = Settings::default();
		let look = |code, ctrl, alt, shift, main| lookup(&settings, code, ctrl, alt, shift, main);
		assert_eq!(look(key('P'), true, false, false, true), Some(Action::PlayPause));
		assert_eq!(look(key('R'), true, false, true, true), Some(Action::RecordPause));
		assert_eq!(look(WXK_F5, false, false, false, true), Some(Action::SeekStart));
		assert_eq!(look(WXK_LEFT, true, false, false, true), Some(Action::SeekBack));
		assert_eq!(
			look(WXK_LEFT, true, false, true, true),
			Some(Action::SeekStart)
		);
		assert_eq!(
			look(WXK_RIGHT, true, false, true, false),
			Some(Action::GoLive),
			"go live fires in dialogs too"
		);
		assert_eq!(look(WXK_UP, true, false, false, false), Some(Action::VolumeUp));
		assert_eq!(look(WXK_DOWN, true, false, false, true), Some(Action::VolumeDown));
		assert_eq!(look(key('1'), true, false, false, true), Some(Action::Sleep(15)));
		assert_eq!(look(key('0'), true, false, false, true), Some(Action::SleepOff));
		assert_eq!(
			look(WXK_LEFT, false, true, false, true),
			Some(Action::PreviousStation)
		);
		assert_eq!(look(WXK_LEFT, false, true, false, false), None, "main only");
		assert_eq!(look(key('3'), false, true, false, true), Some(Action::PlayFavorite(2)));
		assert_eq!(look(key('0'), false, true, false, true), Some(Action::PlayFavorite(9)));
		assert_eq!(look(key('V'), false, true, false, true), Some(Action::FocusVolume));
		assert_eq!(look(WXK_ESCAPE, false, false, false, false), Some(Action::Close));
		assert_eq!(look(WXK_ESCAPE, false, false, false, true), None, "main window");
		assert_eq!(look(key('I'), true, false, false, false), None, "import is main only");
		assert_eq!(look(key('P'), false, false, false, true), None, "plain letters type");
	}

	#[test]
	fn user_binding_moves_and_clears() {
		let mut settings = Settings::default();
		settings
			.keys
			.insert(K::SaveBuffer.id().into(), "Ctrl+W".into());
		assert_eq!(
			lookup(&settings, key('W'), true, false, false, true),
			Some(Action::SaveBuffer),
			"a keyless menu action gains a key"
		);
		settings.keys.insert(K::Record.id().into(), String::new());
		assert_eq!(
			lookup(&settings, key('R'), true, false, false, true),
			None,
			"cleared means unbound, not default"
		);
		settings
			.keys
			.insert(K::Record.id().into(), "Ctrl+F9".into());
		assert_eq!(menu_chord(&settings, &Action::Record), "Ctrl+F9");
		assert_eq!(menu_chord(&settings, &Action::SeekBack), "F6");
		settings.keys.remove(K::Record.id());
		assert_eq!(menu_chord(&settings, &Action::Record), "Ctrl+R");
	}

	#[test]
	fn model_round_trips() {
		let mut settings = Settings::default();
		settings
			.keys
			.insert(K::Play.id().into(), "Ctrl+Shift+P".into());
		let model = Model::from_settings(&settings);
		assert_eq!(
			wx_utils::shortcuts::ShortcutModel::chord(&model, 0, K::Play)
				.map(|c| c.to_shortcut_string()),
			Some("Ctrl+Shift+P".to_string())
		);
		assert_eq!(model.to_settings(), settings.keys);
	}
}
