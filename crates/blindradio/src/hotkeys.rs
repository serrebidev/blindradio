//! Global hotkeys: chords that act while another window has focus.

use std::collections::BTreeMap;
use std::sync::mpsc::{Receiver, Sender, channel};

use blindradio_core::settings::Settings;
use blindradio_core::station::Station;
use wx_utils::shortcuts::KeyChord;

use crate::strings as s;

/// What a global hotkey does; `id` is the name settings.json stores.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum HotkeyAction {
	PlayPause,
	Stop,
	VolumeUp,
	VolumeDown,
	NextFavorite,
	PreviousFavorite,
	AnnounceNowPlaying,
	ToggleWindow,
	ToggleEq,
}

impl HotkeyAction {
	pub const ALL: [HotkeyAction; 9] = [
		HotkeyAction::PlayPause,
		HotkeyAction::Stop,
		HotkeyAction::VolumeUp,
		HotkeyAction::VolumeDown,
		HotkeyAction::NextFavorite,
		HotkeyAction::PreviousFavorite,
		HotkeyAction::AnnounceNowPlaying,
		HotkeyAction::ToggleWindow,
		HotkeyAction::ToggleEq,
	];

	pub fn id(self) -> &'static str {
		match self {
			HotkeyAction::PlayPause => "play_pause",
			HotkeyAction::Stop => "stop",
			HotkeyAction::VolumeUp => "volume_up",
			HotkeyAction::VolumeDown => "volume_down",
			HotkeyAction::NextFavorite => "next_favorite",
			HotkeyAction::PreviousFavorite => "previous_favorite",
			HotkeyAction::AnnounceNowPlaying => "announce_now_playing",
			HotkeyAction::ToggleWindow => "toggle_window",
			HotkeyAction::ToggleEq => "toggle_eq",
		}
	}

	pub fn label(self) -> &'static str {
		match self {
			HotkeyAction::PlayPause => s::HK_PLAY_PAUSE,
			HotkeyAction::Stop => s::HK_STOP,
			HotkeyAction::VolumeUp => s::HK_VOLUME_UP,
			HotkeyAction::VolumeDown => s::HK_VOLUME_DOWN,
			HotkeyAction::NextFavorite => s::HK_NEXT_FAVORITE,
			HotkeyAction::PreviousFavorite => s::HK_PREVIOUS_FAVORITE,
			HotkeyAction::AnnounceNowPlaying => s::HK_ANNOUNCE,
			HotkeyAction::ToggleWindow => s::HK_TOGGLE_WINDOW,
			HotkeyAction::ToggleEq => s::HK_TOGGLE_EQ,
		}
	}

	pub fn default_chord(self) -> &'static str {
		match self {
			HotkeyAction::PlayPause => "Ctrl+Alt+Shift+P",
			HotkeyAction::Stop => "Ctrl+Alt+Shift+S",
			HotkeyAction::VolumeUp => "Ctrl+Alt+Shift+PageUp",
			HotkeyAction::VolumeDown => "Ctrl+Alt+Shift+PageDown",
			HotkeyAction::NextFavorite => "Ctrl+Alt+Shift+Right",
			HotkeyAction::PreviousFavorite => "Ctrl+Alt+Shift+Left",
			HotkeyAction::AnnounceNowPlaying => "Ctrl+Alt+Shift+N",
			HotkeyAction::ToggleWindow => "Ctrl+Alt+Shift+Y",
			HotkeyAction::ToggleEq => "Ctrl+Alt+Shift+E",
		}
	}
}

/// The chord bound to `action`: the user's, else the default. `None` is cleared.
pub fn bound(settings: &Settings, action: HotkeyAction) -> Option<KeyChord> {
	match settings.hotkeys.get(action.id()) {
		Some(text) => KeyChord::parse(text),
		None => KeyChord::parse(action.default_chord()),
	}
}

/// Every action with its chord, `None` where the user cleared it.
pub fn effective(settings: &Settings) -> Vec<(HotkeyAction, Option<KeyChord>)> {
	HotkeyAction::ALL
		.iter()
		.map(|&action| (action, bound(settings, action)))
		.collect()
}

/// The one message after registering, if anything was refused.
pub fn report(taken: &[String], unavailable: bool) -> Option<String> {
	if taken.is_empty() {
		return None;
	}
	if unavailable {
		return Some(s::HOTKEYS_UNAVAILABLE.to_string());
	}
	Some(format!("{}{}", s::HOTKEY_TAKEN, taken.join(", ")))
}

/// The favorite `forward` steps from what plays, wrapping round. From nothing playing,
/// forward starts at the first and backward at the last.
pub fn next_favorite(favorites: &[Station], playing: Option<&str>, forward: bool) -> Option<usize> {
	let len = favorites.len();
	if len == 0 {
		return None;
	}
	let at = playing.and_then(|id| favorites.iter().position(|s| s.id == id));
	Some(match (at, forward) {
		(Some(i), true) => (i + 1) % len,
		(Some(i), false) => (i + len - 1) % len,
		(None, true) => 0,
		(None, false) => len - 1,
	})
}

/// What Announce Now Playing says. It takes no settings on purpose: it works with every
/// announcement turned off.
pub fn now_playing_text(song: Option<&str>, station: Option<&str>) -> String {
	match (song, station) {
		(Some(song), Some(station)) => format!("{song} - {station}"),
		(_, Some(station)) => station.to_string(),
		_ => s::STOPPED.to_string(),
	}
}

pub struct Hotkeys {
	/// Kept for as long as the chords are wanted: dropping it gives them back.
	_handle: Option<platform::Handle>,
	/// Chords another program already holds.
	pub taken: Vec<String>,
	/// Every chord was refused on Wayland, where the portal is missing.
	pub unavailable: bool,
	pub keys: Receiver<HotkeyAction>,
}

pub fn register(settings: &Settings) -> Hotkeys {
	let (tx, keys) = channel();
	let bindings: Vec<(KeyChord, HotkeyAction)> = effective(settings)
		.into_iter()
		.filter_map(|(action, chord)| chord.map(|chord| (chord, action)))
		.collect();
	let wanted = bindings.len();
	let (handle, failed) = platform::register(bindings, tx);
	let taken: Vec<String> = failed.iter().map(KeyChord::to_display_string).collect();
	let unavailable = is_wayland() && wanted > 0 && taken.len() == wanted;
	Hotkeys {
		_handle: handle,
		taken,
		unavailable,
		keys,
	}
}

/// Wayland refuses every chord without its portal; the platform says so the same way.
fn is_wayland() -> bool {
	#[cfg(target_os = "linux")]
	{
		std::env::var("XDG_SESSION_TYPE").is_ok_and(|t| t.eq_ignore_ascii_case("wayland"))
	}
	#[cfg(not(target_os = "linux"))]
	{
		false
	}
}

#[cfg(windows)]
mod platform {
	use super::{HotkeyAction, KeyChord, Sender};
	use wx_utils::global_hotkeys::GlobalHotkeys;

	pub type Handle = GlobalHotkeys;

	pub fn register(
		bindings: Vec<(KeyChord, HotkeyAction)>,
		tx: Sender<HotkeyAction>,
	) -> (Option<Handle>, Vec<KeyChord>) {
		// The callback runs on the hotkey thread: only the value crosses over.
		GlobalHotkeys::register(bindings, move |action| {
			let _ = tx.send(action);
		})
	}
}

#[cfg(not(windows))]
mod platform {
	use super::{HotkeyAction, KeyChord, Sender};
	use global_hotkey::hotkey::HotKey;
	use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};

	pub type Handle = GlobalHotKeyManager;

	pub fn register(
		bindings: Vec<(KeyChord, HotkeyAction)>,
		tx: Sender<HotkeyAction>,
	) -> (Option<Handle>, Vec<KeyChord>) {
		let Ok(manager) = GlobalHotKeyManager::new() else {
			return (None, bindings.into_iter().map(|(chord, _)| chord).collect());
		};
		let mut failed = Vec::new();
		let mut bound: Vec<(u32, HotkeyAction)> = Vec::new();
		for (chord, action) in bindings {
			match chord.to_shortcut_string().parse::<HotKey>() {
				Ok(hotkey) => match manager.register(hotkey) {
					Ok(()) => bound.push((hotkey.id(), action)),
					Err(_) => failed.push(chord),
				},
				Err(_) => failed.push(chord),
			}
		}
		GlobalHotKeyEvent::set_event_handler(Some(move |event: GlobalHotKeyEvent| {
			if event.state() == HotKeyState::Pressed
				&& let Some((_, action)) = bound.iter().find(|(id, _)| *id == event.id())
			{
				let _ = tx.send(*action);
			}
		}));
		(Some(manager), failed)
	}
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
				.hotkeys
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
	type Action = HotkeyAction;

	fn tabs(&self) -> Vec<String> {
		vec![s::TAB_GLOBAL.to_string()]
	}

	fn tab_scope(&self, _tab: usize) -> wx_utils::shortcuts::TabScope {
		wx_utils::shortcuts::TabScope::Global
	}

	fn actions(&self, _tab: usize) -> Vec<HotkeyAction> {
		HotkeyAction::ALL.to_vec()
	}

	fn action_name(&self, action: HotkeyAction) -> String {
		action.label().to_string()
	}

	fn chord(&self, _tab: usize, action: HotkeyAction) -> Option<KeyChord> {
		match self.bindings.get(action.id()) {
			Some(bound) => bound.clone(),
			None => KeyChord::parse(action.default_chord()),
		}
	}

	fn set_chord(&mut self, _tab: usize, action: HotkeyAction, chord: Option<KeyChord>) {
		self.bindings.insert(action.id().to_string(), chord);
	}

	fn reset_action(&mut self, _tab: usize, action: HotkeyAction) {
		self.bindings.remove(action.id());
	}

	fn reset_all(&mut self, _tab: usize) {
		self.bindings.clear();
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use blindradio_core::station::Source;

	fn station(id: &str) -> Station {
		Station {
			id: id.into(),
			name: id.into(),
			url: format!("http://{id}"),
			source: Source::RadioBrowser,
			info: None,
		}
	}

	fn chord(settings: &Settings, action: HotkeyAction) -> KeyChord {
		bound(settings, action).unwrap_or_else(|| panic!("{}", action.id()))
	}

	#[test]
	fn defaults_exact() {
		let settings = Settings::default();
		let expected = [
			(HotkeyAction::PlayPause, "P"),
			(HotkeyAction::Stop, "S"),
			(HotkeyAction::VolumeUp, "PageUp"),
			(HotkeyAction::VolumeDown, "PageDown"),
			(HotkeyAction::NextFavorite, "Right"),
			(HotkeyAction::PreviousFavorite, "Left"),
			(HotkeyAction::AnnounceNowPlaying, "N"),
			(HotkeyAction::ToggleWindow, "Y"),
			(HotkeyAction::ToggleEq, "E"),
		];
		for (action, key) in expected {
			let c = chord(&settings, action);
			assert!(c.key.eq_ignore_ascii_case(key), "{} {}", action.id(), c.key);
			assert!(c.ctrl && c.alt && c.shift, "{} modifiers", action.id());
			assert!(!c.win, "{} wins", action.id());
		}
		assert_eq!(effective(&settings).len(), HotkeyAction::ALL.len());
	}

	#[test]
	fn user_binding_overrides_default_and_clear_removes() {
		let mut settings = Settings::default();
		settings
			.hotkeys
			.insert(HotkeyAction::PlayPause.id().into(), "Ctrl+F9".into());
		let c = chord(&settings, HotkeyAction::PlayPause);
		assert!(c.key.eq_ignore_ascii_case("F9") && c.ctrl && !c.alt && !c.shift);
		// Another action keeps its default.
		assert!(
			chord(&settings, HotkeyAction::Stop)
				.key
				.eq_ignore_ascii_case("S")
		);
		// Empty means cleared, which is not the default.
		settings
			.hotkeys
			.insert(HotkeyAction::PlayPause.id().into(), String::new());
		assert_eq!(bound(&settings, HotkeyAction::PlayPause), None);
		// Resetting drops the override.
		settings.hotkeys.remove(HotkeyAction::PlayPause.id());
		assert!(
			chord(&settings, HotkeyAction::PlayPause)
				.key
				.eq_ignore_ascii_case("P")
		);
	}

	#[test]
	fn next_favorite_wraps() {
		let favorites = [station("a"), station("b"), station("c")];
		let at = |playing, forward| next_favorite(&favorites, playing, forward);
		assert_eq!(at(Some("b"), true), Some(2));
		assert_eq!(at(Some("c"), true), Some(0), "wraps forward");
		assert_eq!(at(Some("a"), false), Some(2), "wraps backward");
		assert_eq!(at(None, true), Some(0), "nothing playing: the first");
		assert_eq!(at(None, false), Some(2), "nothing playing: the last");
		assert_eq!(
			at(Some("gone"), true),
			Some(0),
			"a station not in favorites"
		);
		assert_eq!(next_favorite(&[], None, true), None);
	}

	#[test]
	fn announce_now_playing_ignores_setting() {
		// No settings argument at all: off or on, it says the same.
		assert_eq!(
			now_playing_text(Some("Song"), Some("Station")),
			"Song - Station"
		);
		assert_eq!(now_playing_text(None, Some("Station")), "Station");
		assert_eq!(now_playing_text(Some("Song"), None), "Stopped");
		assert_eq!(now_playing_text(None, None), "Stopped");
	}

	#[test]
	fn taken_hotkeys_reported_once() {
		assert_eq!(report(&[], false), None);
		let one = vec!["Ctrl+Alt+Shift+P".to_string()];
		assert_eq!(
			report(&one, false).as_deref(),
			Some("Hotkey taken: Ctrl+Alt+Shift+P")
		);
		let two = vec!["Ctrl+Alt+Shift+P".into(), "Ctrl+Alt+Shift+S".into()];
		assert_eq!(
			report(&two, false).as_deref(),
			Some("Hotkey taken: Ctrl+Alt+Shift+P, Ctrl+Alt+Shift+S"),
			"one message lists them all"
		);
	}

	#[test]
	fn wayland_without_portal_says_once() {
		// Every chord refused on Wayland: one message, not nine.
		let all: Vec<String> = HotkeyAction::ALL
			.iter()
			.map(|a| a.default_chord().to_string())
			.collect();
		assert_eq!(report(&all, true).as_deref(), Some("Hotkeys unavailable"));
		assert_eq!(report(&[], true), None, "nothing refused, nothing to say");
		// Some registered: those that did not are named.
		assert!(
			report(&all[..1], false)
				.unwrap()
				.starts_with("Hotkey taken: ")
		);
	}
}
