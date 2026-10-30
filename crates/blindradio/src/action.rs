//! Everything a menu item, button or key can do. Menus, buttons and keys only produce these.

use blindradio_core::sleep::SLEEP_MINUTES;
use wxdragon::prelude::{
	WXK_DOWN, WXK_ESCAPE, WXK_F1, WXK_F5, WXK_F6, WXK_F7, WXK_F8, WXK_LEFT, WXK_RIGHT, WXK_UP,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
	/// The station focused in the open list, else the playing one.
	Play,
	PlayPause,
	Stop,
	/// Toggle recording of the playing station.
	Record,
	/// Pause or resume the active recording.
	RecordPause,
	/// Save the time-shift buffer to a WAV file in the recordings folder.
	SaveBuffer,
	/// The scheduled recordings list dialog.
	ScheduledRecordings,
	/// One step back in the time-shift buffer.
	SeekBack,
	/// One step forward, never past live.
	SeekForward,
	/// Jump to the oldest audio the buffer still holds.
	SeekStart,
	/// Jump to the live edge.
	GoLive,
	/// Back through the stations played this session (Alt+Left).
	PreviousStation,
	/// Forward through them again (Alt+Right).
	NextStation,
	Mute,
	VolumeUp,
	VolumeDown,
	/// 0 to 100.
	SetVolume(u8),
	ToggleFavorite,
	OpenSearch,
	OpenBrowse,
	OpenFavorites,
	OpenRecent,
	OpenCollections,
	NewCustomStation,
	CopyUrl,
	SongHistory,
	AddToCollection,
	Import,
	ImportAsCollection,
	/// Minutes.
	Sleep(u32),
	SleepOff,
	Equalizer,
	/// 0-based: Alt+1 is 0, Alt+0 is 9.
	PlayFavorite(usize),
	/// `None` is the default device.
	SetDevice(Option<String>),
	/// File > Global Hotkeys…: the chord editor.
	GlobalHotkeys,
	/// Playback feature settings dialog.
	FeatureSettings,
	/// Per-station overrides dialog (context menu).
	StationSettings,
	EditStation,
	OpenUrl,
	CopyStation,
	CutStation,
	PasteStation,
	/// Help > Check for Updates.
	CheckForUpdates,
	/// Help > Install NVDA Add-on: opens the shipped add-on file.
	InstallNvdaAddon,
	Help,
	About,
	/// Fetch the SerrebiRadio starter list again.
	GetStarter,
	Exit,
	/// Alt+V: focus the volume slider.
	FocusVolume,
	/// Escape: close the open dialog.
	Close,
}

/// The app-wide key table. `main_window` is false inside dialogs, where the main-window-only keys
/// fall through to the focused control.
pub fn app_key(code: i32, ctrl: bool, alt: bool, shift: bool, main_window: bool) -> Option<Action> {
	let letter = u8::try_from(code).ok().map(char::from);
	let digit = letter.and_then(|c| c.to_digit(10));
	match (ctrl, alt, shift) {
		(true, false, false) => {
			if let Some(d) = digit {
				return Some(match d {
					0 => Action::SleepOff,
					1..=6 => Action::Sleep(SLEEP_MINUTES[d as usize - 1]),
					_ => return None,
				});
			}
			match code {
				WXK_UP => return Some(Action::VolumeUp),
				WXK_DOWN => return Some(Action::VolumeDown),
				WXK_LEFT => return Some(Action::SeekBack),
				WXK_RIGHT => return Some(Action::SeekForward),
				_ => {}
			}
			Some(match letter? {
				'P' => Action::PlayPause,
				'S' => Action::Stop,
				'R' => Action::Record,
				'J' => Action::GoLive,
				'M' => Action::Mute,
				'D' => Action::ToggleFavorite,
				'F' => Action::OpenSearch,
				'G' => Action::OpenBrowse,
				'B' => Action::OpenFavorites,
				'H' => Action::OpenRecent,
				'N' => Action::NewCustomStation,
				'U' => Action::CopyUrl,
				'C' => Action::CopyStation,
				'X' => Action::CutStation,
				'V' => Action::PasteStation,
				'Y' => Action::SongHistory,
				'L' => Action::OpenCollections,
				'E' => Action::Equalizer,
				'T' => Action::FeatureSettings,
				'I' if main_window => Action::Import,
				_ => return None,
			})
		}
		(true, false, true) => {
			match code {
				WXK_LEFT => return Some(Action::SeekStart),
				WXK_RIGHT => return Some(Action::GoLive),
				_ => {}
			}
			// Like Ctrl+R, pausing a recording works from dialogs too.
			if letter == Some('R') {
				return Some(Action::RecordPause);
			}
			if !main_window {
				return None;
			}
			Some(match letter? {
				'I' => Action::ImportAsCollection,
				'L' => Action::AddToCollection,
				_ => return None,
			})
		}
		(false, true, false) if main_window => match code {
			// Back and forward through the stations played this session.
			WXK_LEFT => Some(Action::PreviousStation),
			WXK_RIGHT => Some(Action::NextStation),
			_ => match (digit, letter) {
				(Some(0), _) => Some(Action::PlayFavorite(9)),
				(Some(d), _) => Some(Action::PlayFavorite(d as usize - 1)),
				(_, Some('V')) => Some(Action::FocusVolume),
				_ => None,
			},
		},
		(false, false, false) => match code {
			WXK_F1 => Some(Action::Help),
			// The transport row: buffer start, skip back, skip forward, live.
			WXK_F5 => Some(Action::SeekStart),
			WXK_F6 => Some(Action::SeekBack),
			WXK_F7 => Some(Action::SeekForward),
			WXK_F8 => Some(Action::GoLive),
			// The main window has nothing to close.
			WXK_ESCAPE if !main_window => Some(Action::Close),
			_ => None,
		},
		_ => None,
	}
}

/// The text-editing chords: copy, cut and paste inside a text field. The
/// app key table maps the same chords to station clipboard actions, so a
/// char hook must pass these to the control whenever a text field has
/// focus, or pasting a URL into a dialog answers "Nothing to paste" and
/// the text never lands (the Edit Station bug, twice shipped).
pub fn is_text_edit_chord(code: i32, ctrl: bool, alt: bool, shift: bool) -> bool {
	if !(ctrl && !alt && !shift) {
		return false;
	}
	let letter = u8::try_from(code).ok().map(char::from);
	matches!(letter, Some('C' | 'X' | 'V'))
}

#[cfg(test)]
mod tests {
	use super::*;
	fn key(c: char) -> i32 {
		c as i32
	}

	#[test]
	fn text_edit_chords_are_the_clipboard_trio() {
		for c in ['C', 'X', 'V'] {
			assert!(is_text_edit_chord(key(c), true, false, false), "{c}");
		}
		// Not editing chords: the app actions keep them.
		assert!(!is_text_edit_chord(key('U'), true, false, false), "copy URL");
		assert!(!is_text_edit_chord(key('V'), true, true, false), "alt held");
		assert!(!is_text_edit_chord(key('V'), true, false, true), "shift held");
		assert!(!is_text_edit_chord(key('V'), false, false, false), "plain V");
		assert!(!is_text_edit_chord(WXK_LEFT, true, false, false));
	}

	#[test]
	fn alt_arrows_walk_played_stations() {
		assert_eq!(
			app_key(WXK_LEFT, false, true, false, true),
			Some(Action::PreviousStation)
		);
		assert_eq!(
			app_key(WXK_RIGHT, false, true, false, true),
			Some(Action::NextStation)
		);
		assert_eq!(
			app_key(WXK_LEFT, false, true, false, false),
			None,
			"main window only"
		);
	}

	#[test]
	fn app_key_table() {
		let everywhere = [
			('P', Action::PlayPause),
			('S', Action::Stop),
			('R', Action::Record),
			('J', Action::GoLive),
			('M', Action::Mute),
			('D', Action::ToggleFavorite),
			('F', Action::OpenSearch),
			('G', Action::OpenBrowse),
			('B', Action::OpenFavorites),
			('H', Action::OpenRecent),
			('N', Action::NewCustomStation),
			('U', Action::CopyUrl),
			('C', Action::CopyStation),
			('X', Action::CutStation),
			('V', Action::PasteStation),
			('Y', Action::SongHistory),
			('L', Action::OpenCollections),
			('E', Action::Equalizer),
			('T', Action::FeatureSettings),
			('1', Action::Sleep(15)),
			('2', Action::Sleep(30)),
			('3', Action::Sleep(45)),
			('4', Action::Sleep(60)),
			('5', Action::Sleep(90)),
			('6', Action::Sleep(120)),
			('0', Action::SleepOff),
		];
		for main in [true, false] {
			for (c, a) in &everywhere {
				assert_eq!(
					app_key(key(*c), true, false, false, main),
					Some(a.clone()),
					"Ctrl+{c}"
				);
			}
			assert_eq!(
				app_key(WXK_UP, true, false, false, main),
				Some(Action::VolumeUp)
			);
			assert_eq!(
				app_key(WXK_DOWN, true, false, false, main),
				Some(Action::VolumeDown)
			);
			assert_eq!(
				app_key(WXK_LEFT, true, false, false, main),
				Some(Action::SeekBack)
			);
			assert_eq!(
				app_key(WXK_RIGHT, true, false, false, main),
				Some(Action::SeekForward)
			);
			assert_eq!(
				app_key(WXK_LEFT, true, false, true, main),
				Some(Action::SeekStart)
			);
			assert_eq!(
				app_key(WXK_RIGHT, true, false, true, main),
				Some(Action::GoLive)
			);
			assert_eq!(
				app_key(key('R'), true, false, true, main),
				Some(Action::RecordPause)
			);
			assert_eq!(
				app_key(WXK_F1, false, false, false, main),
				Some(Action::Help)
			);
			assert_eq!(
				app_key(WXK_F5, false, false, false, main),
				Some(Action::SeekStart)
			);
			assert_eq!(
				app_key(WXK_F6, false, false, false, main),
				Some(Action::SeekBack)
			);
			assert_eq!(
				app_key(WXK_F7, false, false, false, main),
				Some(Action::SeekForward)
			);
			assert_eq!(
				app_key(WXK_F8, false, false, false, main),
				Some(Action::GoLive)
			);
		}
		assert_eq!(
			app_key(WXK_ESCAPE, false, false, false, false),
			Some(Action::Close)
		);
		assert_eq!(app_key(WXK_ESCAPE, false, false, false, true), None);

		// Main window only.
		assert_eq!(
			app_key(key('I'), true, false, false, true),
			Some(Action::Import)
		);
		assert_eq!(
			app_key(key('I'), true, false, true, true),
			Some(Action::ImportAsCollection)
		);
		assert_eq!(
			app_key(key('L'), true, false, true, true),
			Some(Action::AddToCollection)
		);
		assert_eq!(
			app_key(key('V'), false, true, false, true),
			Some(Action::FocusVolume)
		);
		for d in 1..=9 {
			let c = char::from_digit(d, 10).unwrap();
			assert_eq!(
				app_key(key(c), false, true, false, true),
				Some(Action::PlayFavorite(d as usize - 1)),
				"Alt+{c}"
			);
		}
		assert_eq!(
			app_key(key('0'), false, true, false, true),
			Some(Action::PlayFavorite(9))
		);
		assert_eq!(
			app_key(key('I'), true, false, false, false),
			None,
			"Ctrl+I in a dialog"
		);
		assert_eq!(app_key(key('I'), true, false, true, false), None);
		assert_eq!(app_key(key('L'), true, false, true, false), None);
		assert_eq!(app_key(key('1'), false, true, false, false), None);
		assert_eq!(app_key(key('V'), false, true, false, false), None);

		// Plain letters type; other chords are not ours.
		for main in [true, false] {
			for c in ('A'..='Z').chain('0'..='9') {
				assert_eq!(app_key(key(c), false, false, false, main), None, "{c}");
				assert_eq!(app_key(key(c), false, false, true, main), None, "Shift+{c}");
			}
			assert_eq!(app_key(key('7'), true, false, false, main), None);
			assert_eq!(
				app_key(key('P'), true, true, false, main),
				None,
				"Ctrl+Alt is AltGr"
			);
			assert_eq!(app_key(key('P'), true, false, true, main), None);
			assert_eq!(app_key(WXK_UP, false, false, false, main), None);
		}
	}
}
