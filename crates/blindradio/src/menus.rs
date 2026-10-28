//! The menu bar as a data table, and the wx menus built from it.

use blindradio_core::settings::Settings;
use blindradio_core::sleep::SLEEP_MINUTES;
use wx_utils::menu_label;
use wxdragon::prelude::*;

use crate::action::{Action, SettingId};
use crate::strings as s;

pub const FAVORITE_KEYS: &str = "Ctrl+D";

pub enum Entry {
	/// Label, shortcut (`""` for none), action.
	Item(&'static str, &'static str, Action),
	Check(&'static str, SettingId),
	/// The Audio Device submenu: `Default`, then every output device.
	Devices,
	Sub(&'static str, Vec<Entry>),
	Separator,
}

/// Global Hotkeys… (plan 5) and Check for Updates (plan 6) join when they work.
pub fn menus() -> Vec<(&'static str, Vec<Entry>)> {
	use Action as A;
	use Entry::{Check, Item, Separator, Sub};
	let mut sleep: Vec<Entry> = s::SLEEP_ITEMS
		.iter()
		.zip(SLEEP_MINUTES)
		.zip(["Ctrl+1", "Ctrl+2", "Ctrl+3", "Ctrl+4", "Ctrl+5", "Ctrl+6"])
		.map(|((label, minutes), keys)| Item(label, keys, A::Sleep(minutes)))
		.collect();
	sleep.push(Item(s::SLEEP_OFF, "Ctrl+0", A::SleepOff));
	vec![
		(
			s::MENU_FILE,
			vec![
				Check(s::ANNOUNCE_TITLES, SettingId::AnnounceTitles),
				Check(s::ANNOUNCE_STATUS, SettingId::AnnounceStatus),
				Check(s::CLOSE_LISTS, SettingId::CloseListsWhenPlaying),
				Check(s::PLAY_LAST, SettingId::PlayLastAtStartup),
				Separator,
				Entry::Devices,
				Separator,
				Item(s::EXIT, "", A::Exit),
			],
		),
		(
			s::MENU_STATIONS,
			vec![
				Item(s::SEARCH, "Ctrl+F", A::OpenSearch),
				Item(s::BROWSE, "Ctrl+G", A::OpenBrowse),
				Item(s::FAVORITES, "Ctrl+B", A::OpenFavorites),
				Item(s::RECENT, "Ctrl+H", A::OpenRecent),
				Item(s::COLLECTIONS, "Ctrl+L", A::OpenCollections),
				Separator,
				Item(s::ADD_CUSTOM, "Ctrl+N", A::NewCustomStation),
				Item(s::IMPORT, "Ctrl+I", A::Import),
				Item(
					s::IMPORT_AS_COLLECTION,
					"Ctrl+Shift+I",
					A::ImportAsCollection,
				),
				Item(s::COPY_URL, "Ctrl+U", A::CopyUrl),
				Separator,
				Item(s::GET_STARTER, "", A::GetStarter),
			],
		),
		(
			s::MENU_PLAYBACK,
			vec![
				Item(s::PLAY_PAUSE, "Ctrl+P", A::PlayPause),
				Item(s::STOP_MENU, "Ctrl+S", A::Stop),
				Item(s::MUTE_MENU, "Ctrl+M", A::Mute),
				Item(s::VOLUME_UP, "Ctrl+Up", A::VolumeUp),
				Item(s::VOLUME_DOWN, "Ctrl+Down", A::VolumeDown),
				Separator,
				Item(favorite_label(false), FAVORITE_KEYS, A::ToggleFavorite),
				Item(s::ADD_TO_COLLECTION, "Ctrl+Shift+L", A::AddToCollection),
				Item(s::SONG_HISTORY, "Ctrl+Y", A::SongHistory),
				Separator,
				Sub(s::SLEEP_TIMER, sleep),
				Item(s::EQUALIZER, "Ctrl+E", A::Equalizer),
			],
		),
		(
			s::MENU_HELP,
			vec![
				Item(s::USER_GUIDE, "F1", A::Help),
				Item(s::ABOUT, "", A::About),
			],
		),
	]
}

/// The menu item's label; the main window button drops the `&` (`plain`).
pub fn favorite_label(is_favorite: bool) -> &'static str {
	if is_favorite {
		s::REMOVE_FROM_FAVORITES
	} else {
		s::ADD_TO_FAVORITES
	}
}

/// Without its Alt letter.
pub fn plain(label: &str) -> String {
	label.replace('&', "")
}

/// What each menu id does.
pub struct MenuIds {
	actions: Vec<Action>,
	/// Add to / Remove from Favorites, relabelled as the station changes.
	pub favorite: Id,
}

const FIRST_ID: Id = ID_HIGHEST + 1;

impl MenuIds {
	pub fn action(&self, id: Id) -> Option<Action> {
		let i = usize::try_from(id.checked_sub(FIRST_ID)?).ok()?;
		self.actions.get(i).cloned()
	}

	fn add(&mut self, a: Action) -> Id {
		self.actions.push(a);
		FIRST_ID + self.actions.len() as Id - 1
	}
}

/// Sets the frame's menu bar. `devices` is `output_devices()`.
pub fn build(frame: &Frame, settings: &Settings, devices: &[String]) -> MenuIds {
	let mut ids = MenuIds {
		actions: Vec::new(),
		favorite: wxdragon::id::ID_NONE,
	};
	let mut bar = MenuBar::builder();
	for (title, entries) in menus() {
		bar = bar.append(fill(&mut ids, &entries, settings, devices), title);
	}
	frame.set_menu_bar(bar.build());
	ids
}

fn fill(ids: &mut MenuIds, entries: &[Entry], settings: &Settings, devices: &[String]) -> Menu {
	let menu = Menu::builder().build();
	for e in entries {
		match e {
			Entry::Item(label, keys, action) => {
				let id = ids.add(action.clone());
				if *action == Action::ToggleFavorite {
					ids.favorite = id;
				}
				menu.append(id, &menu_label(label, keys), "", ItemKind::Normal);
			}
			Entry::Check(label, setting) => {
				let id = ids.add(Action::ToggleSetting(*setting));
				if let Some(item) = menu.append(id, label, "", ItemKind::Check) {
					item.check(is_on(settings, *setting));
				}
			}
			Entry::Devices => {
				let sub = Menu::builder().build();
				// A chosen device that is gone plays through the default.
				let active = settings
					.audio_device
					.as_ref()
					.filter(|d| devices.contains(d));
				let default = ids.add(Action::SetDevice(None));
				sub.append(default, s::DEFAULT_DEVICE, "", ItemKind::Radio);
				for name in devices {
					let id = ids.add(Action::SetDevice(Some(name.clone())));
					let item = sub.append(id, &name.replace('&', "&&"), "", ItemKind::Radio);
					if let Some(item) = item
						&& active == Some(name)
					{
						item.check(true);
					}
				}
				menu.append_submenu(sub, s::AUDIO_DEVICE, "");
			}
			Entry::Sub(label, sub) => {
				let sub = fill(ids, sub, settings, devices);
				menu.append_submenu(sub, label, "");
			}
			Entry::Separator => menu.append_separator(),
		}
	}
	menu
}

fn is_on(settings: &Settings, id: SettingId) -> bool {
	match id {
		SettingId::AnnounceTitles => settings.announce_titles,
		SettingId::AnnounceStatus => settings.announce_status,
		SettingId::CloseListsWhenPlaying => settings.close_lists_when_playing,
		SettingId::PlayLastAtStartup => settings.play_last_at_startup,
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::action::app_key;
	use wxdragon::prelude::{WXK_DOWN, WXK_F1, WXK_UP};

	/// Every item with its shortcut, `&` dropped, submenus flattened.
	fn items() -> Vec<(String, &'static str, Action)> {
		fn walk(entries: &[Entry], out: &mut Vec<(String, &'static str, Action)>) {
			for e in entries {
				match e {
					Entry::Item(label, keys, action) => {
						out.push((label.replace('&', ""), keys, action.clone()));
					}
					Entry::Sub(_, sub) => walk(sub, out),
					_ => {}
				}
			}
		}
		let mut out = Vec::new();
		for (_, entries) in menus() {
			walk(&entries, &mut out);
		}
		out
	}

	/// `Ctrl+Shift+I` to what `app_key` gets for it.
	fn parse(keys: &str) -> (i32, bool, bool, bool) {
		let mut parts: Vec<&str> = keys.split('+').collect();
		let key = parts.pop().unwrap();
		let code = match key {
			"Up" => WXK_UP,
			"Down" => WXK_DOWN,
			"F1" => WXK_F1,
			k if k.len() == 1 => k.chars().next().unwrap() as i32,
			k => panic!("unknown key {k}"),
		};
		(
			code,
			parts.contains(&"Ctrl"),
			parts.contains(&"Alt"),
			parts.contains(&"Shift"),
		)
	}

	#[test]
	fn menus_have_every_shortcut() {
		let expected = [
			("Search…", "Ctrl+F"),
			("Browse…", "Ctrl+G"),
			("Favorites…", "Ctrl+B"),
			("Recently Played…", "Ctrl+H"),
			("Collections…", "Ctrl+L"),
			("Add Custom Station…", "Ctrl+N"),
			("Import Playlist…", "Ctrl+I"),
			("Import Playlist as Collection…", "Ctrl+Shift+I"),
			("Copy Stream URL", "Ctrl+U"),
			("Get SerrebiRadio Stations", ""),
			("Play/Pause", "Ctrl+P"),
			("Stop", "Ctrl+S"),
			("Mute", "Ctrl+M"),
			("Volume Up", "Ctrl+Up"),
			("Volume Down", "Ctrl+Down"),
			("Add to Favorites", "Ctrl+D"),
			("Add to Collection…", "Ctrl+Shift+L"),
			("Song History…", "Ctrl+Y"),
			("15 Minutes", "Ctrl+1"),
			("30 Minutes", "Ctrl+2"),
			("45 Minutes", "Ctrl+3"),
			("1 Hour", "Ctrl+4"),
			("90 Minutes", "Ctrl+5"),
			("2 Hours", "Ctrl+6"),
			("Off", "Ctrl+0"),
			("Equalizer…", "Ctrl+E"),
			("User Guide", "F1"),
			("About BlindRadio", ""),
			("Exit", ""),
		];
		let items = items();
		for (label, keys) in expected {
			assert!(
				items.iter().any(|(l, k, _)| l == label && *k == keys),
				"{label} {keys}"
			);
		}
		// The shown shortcut is the key that really does it.
		for (label, keys, action) in &items {
			if keys.is_empty() {
				continue;
			}
			let (code, ctrl, alt, shift) = parse(keys);
			assert_eq!(
				app_key(code, ctrl, alt, shift, true),
				Some(action.clone()),
				"{label} {keys}"
			);
		}
		let titles: Vec<_> = menus().into_iter().map(|(t, _)| t).collect();
		assert_eq!(titles, ["&File", "&Stations", "&Playback", "&Help"]);
	}

	#[test]
	fn favorite_label_flips() {
		assert_eq!(favorite_label(false), "Add to &Favorites");
		assert_eq!(favorite_label(true), "Remove from &Favorites");
		assert_eq!(plain(favorite_label(false)), "Add to Favorites");
		assert_eq!(plain(favorite_label(true)), "Remove from Favorites");
	}

	#[test]
	fn file_menu_settings() {
		let (_, file) = &menus()[0];
		let checks: Vec<_> = file
			.iter()
			.filter_map(|e| match e {
				Entry::Check(label, id) => Some((label.replace('&', ""), *id)),
				_ => None,
			})
			.collect();
		assert_eq!(
			checks,
			[
				(
					"Announce Song Titles".to_string(),
					SettingId::AnnounceTitles
				),
				(
					"Announce Connection Status".to_string(),
					SettingId::AnnounceStatus
				),
				(
					"Close Lists When Playing".to_string(),
					SettingId::CloseListsWhenPlaying
				),
				(
					"Play Last Station at Startup".to_string(),
					SettingId::PlayLastAtStartup
				),
			]
		);
		assert!(file.iter().any(|e| matches!(e, Entry::Devices)));
	}

	#[test]
	fn alt_letters_unique_per_menu() {
		fn letters(entries: &[Entry]) {
			let mut seen = Vec::new();
			for e in entries {
				let label = match e {
					Entry::Item(l, ..) | Entry::Check(l, _) | Entry::Sub(l, _) => *l,
					Entry::Devices => crate::strings::AUDIO_DEVICE,
					Entry::Separator => continue,
				};
				let letter = label
					.split_once('&')
					.and_then(|(_, rest)| rest.chars().next())
					.unwrap_or_else(|| panic!("{label} has no Alt letter"))
					.to_ascii_lowercase();
				assert!(!seen.contains(&letter), "{label}: {letter} taken");
				seen.push(letter);
				if let Entry::Sub(_, sub) = e {
					letters(sub);
				}
			}
		}
		for (_, entries) in menus() {
			letters(&entries);
		}
	}
}
