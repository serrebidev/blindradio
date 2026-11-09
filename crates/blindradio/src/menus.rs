//! The menu bar as a data table, and the wx menus built from it.

use blindradio_core::settings::Settings;
use blindradio_core::sleep::SLEEP_MINUTES;
use wx_utils::menu_label;
use wxdragon::prelude::*;

use crate::action::Action;
use crate::strings as s;

pub enum Entry {
	/// Label, shortcut (`""` for none), action. The shortcut is the
	/// action's effective chord, so a remapped key shows here.
	Item(String, String, Action),
	/// The Audio Device submenu: `Default`, then every output device.
	Devices,
	Sub(String, Vec<Entry>),
	Separator,
}

pub fn menus(settings: &Settings) -> Vec<(String, Vec<Entry>)> {
	use Action as A;
	use Entry::{Item, Separator, Sub};
	let keys = |action: &A| crate::keys::menu_chord(settings, action);
	let mut sleep: Vec<Entry> = s::sleep_items()
		.iter()
		.zip(SLEEP_MINUTES)
		.map(|(label, minutes)| {
			let action = A::Sleep(minutes);
			Item(label.clone(), keys(&action), action)
		})
		.collect();
	sleep.push(Item(s::sleep_off(), keys(&A::SleepOff), A::SleepOff));
	vec![
		(
			s::menu_file(),
			vec![
				Item(s::open_url(), keys(&A::OpenUrl), A::OpenUrl),
				Item(s::detect_streams(), keys(&A::DetectStreams), A::DetectStreams),
				Item(s::import(), keys(&A::Import), A::Import),
				Item(
					s::import_as_collection(),
					keys(&A::ImportAsCollection),
					A::ImportAsCollection,
				),
				Separator,
				Item(
					s::export_current_list(),
					keys(&A::ExportCurrentList),
					A::ExportCurrentList,
				),
				Item(
					s::export_all_playlists(),
					keys(&A::ExportAllPlaylists),
					A::ExportAllPlaylists,
				),
				Item(
					s::export_favorites(),
					keys(&A::ExportFavorites),
					A::ExportFavorites,
				),
				Separator,
				Item(
					s::feature_settings(),
					keys(&A::FeatureSettings),
					A::FeatureSettings,
				),
				Item(s::global_hotkeys(), keys(&A::GlobalHotkeys), A::GlobalHotkeys),
				Item(
					s::keyboard_shortcuts(),
					keys(&A::KeyboardShortcuts),
					A::KeyboardShortcuts,
				),
				Separator,
				Item(s::exit(), keys(&A::Exit), A::Exit),
			],
		),
		(
			s::menu_stations(),
			vec![
				Item(s::search(), keys(&A::OpenSearch), A::OpenSearch),
				Item(s::browse(), keys(&A::OpenBrowse), A::OpenBrowse),
				Item(s::favorites(), keys(&A::OpenFavorites), A::OpenFavorites),
				Item(s::recent(), keys(&A::OpenRecent), A::OpenRecent),
				Item(s::collections(), keys(&A::OpenCollections), A::OpenCollections),
				Separator,
				Item(s::previous_station(), keys(&A::PreviousStation), A::PreviousStation),
				Item(s::next_station(), keys(&A::NextStation), A::NextStation),
				Item(
					s::move_to_playing_station(),
					keys(&A::MoveToPlayingStation),
					A::MoveToPlayingStation,
				),
				Separator,
				Item(s::add_custom(), keys(&A::NewCustomStation), A::NewCustomStation),
				Item(s::copy_url(), keys(&A::CopyUrl), A::CopyUrl),
				Item(favorite_label(false), keys(&A::ToggleFavorite), A::ToggleFavorite),
				Item(s::add_to_collection(), keys(&A::AddToCollection), A::AddToCollection),
				Separator,
				Item(s::get_starter(), keys(&A::GetStarter), A::GetStarter),
			],
		),
		(
			s::menu_playback(),
			vec![
				Item(s::play_pause(), keys(&A::PlayPause), A::PlayPause),
				Item(s::stop_menu(), keys(&A::Stop), A::Stop),
				Separator,
				Item(s::skip_back(), keys(&A::SeekBack), A::SeekBack),
				Item(s::skip_forward(), keys(&A::SeekForward), A::SeekForward),
				Item(s::jump_start(), keys(&A::SeekStart), A::SeekStart),
				Item(s::go_live(), keys(&A::GoLive), A::GoLive),
				Item(s::previous_segment(), keys(&A::PreviousSegment), A::PreviousSegment),
				Item(s::next_segment(), keys(&A::NextSegment), A::NextSegment),
				Separator,
				Item(s::mute_menu(), keys(&A::Mute), A::Mute),
				Item(s::volume_up(), keys(&A::VolumeUp), A::VolumeUp),
				Item(s::volume_down(), keys(&A::VolumeDown), A::VolumeDown),
				Item(s::volume_up_5(), keys(&A::VolumeUp5), A::VolumeUp5),
				Item(s::volume_down_5(), keys(&A::VolumeDown5), A::VolumeDown5),
				Separator,
				Item(s::song_history(), keys(&A::SongHistory), A::SongHistory),
				Sub(s::sleep_timer(), sleep),
				Item(s::equalizer(), keys(&A::Equalizer), A::Equalizer),
				Entry::Devices,
			],
		),
		(
			s::menu_recording(),
			vec![
				Item(s::record(), keys(&A::Record), A::Record),
				Item(s::record_pause(), keys(&A::RecordPause), A::RecordPause),
				Item(s::record_songs(), keys(&A::RecordSongs), A::RecordSongs),
				Separator,
				Item(s::save_buffer(), keys(&A::SaveBuffer), A::SaveBuffer),
				Item(
					s::write_current_segment(),
					keys(&A::WriteCurrentSegment),
					A::WriteCurrentSegment,
				),
				Item(
					s::write_previous_segment(),
					keys(&A::WritePreviousSegment),
					A::WritePreviousSegment,
				),
				Item(s::write_next_segment(), keys(&A::WriteNextSegment), A::WriteNextSegment),
				Item(s::write_all_segments(), keys(&A::WriteAllSegments), A::WriteAllSegments),
				Separator,
				Item(s::scheduled_recordings(), keys(&A::ScheduledRecordings), A::ScheduledRecordings),
				Item(s::open_record_folder(), keys(&A::OpenRecordFolder), A::OpenRecordFolder),
			],
		),
		(
			s::menu_help(),
			vec![
				Item(s::user_guide(), keys(&A::Help), A::Help),
				Item(s::view_changelog(), keys(&A::ViewChangelog), A::ViewChangelog),
				Item(s::github_page(), keys(&A::GitHubPage), A::GitHubPage),
				Separator,
				Item(s::check_updates(), keys(&A::CheckForUpdates), A::CheckForUpdates),
				Item(s::about(), keys(&A::About), A::About),
			],
		),
	]
}

/// The menu item's label; the main window button drops the `&` (`plain`).
pub fn favorite_label(is_favorite: bool) -> String {
	if is_favorite {
		s::remove_from_favorites()
	} else {
		s::add_to_favorites()
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
	/// Record, checked while a recording is in progress.
	pub record: Id,
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
		record: wxdragon::id::ID_NONE,
	};
	let mut bar = MenuBar::builder();
	for (title, entries) in menus(settings) {
		bar = bar.append(fill(&mut ids, &entries, settings, devices), &title);
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
				// The song-split recording mode is a check: its state
				// is the setting, read at build like the devices are.
				if *action == Action::RecordSongs {
					let item = menu.append(
						id,
						&menu_label(label, keys),
						"",
						ItemKind::Check,
					);
					if let Some(item) = item
						&& settings.recording.split_songs
					{
						item.check(true);
					}
				} else if *action == Action::Record {
					// App::refresh keeps the check on what the tuner is doing.
					ids.record = id;
					menu.append(id, &menu_label(label, keys), "", ItemKind::Check);
				} else {
					menu.append(id, &menu_label(label, keys), "", ItemKind::Normal);
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
				sub.append(default, &s::default_device(), "", ItemKind::Radio);
				for name in devices {
					let id = ids.add(Action::SetDevice(Some(name.clone())));
					let item = sub.append(id, &name.replace('&', "&&"), "", ItemKind::Radio);
					if let Some(item) = item
						&& active == Some(name)
					{
						item.check(true);
					}
				}
				menu.append_submenu(sub, &s::audio_device(), "");
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

#[cfg(test)]
mod tests {
	use super::*;
	use crate::action::app_key;
	use wxdragon::prelude::{
		WXK_DOWN, WXK_F1, WXK_F5, WXK_F6, WXK_F7, WXK_F8, WXK_LEFT, WXK_RIGHT, WXK_UP,
	};

	/// Every item with its shortcut, `&` dropped, submenus flattened.
	fn items() -> Vec<(String, String, Action)> {
		fn walk(entries: &[Entry], out: &mut Vec<(String, String, Action)>) {
			for e in entries {
				match e {
					Entry::Item(label, keys, action) => {
						out.push((label.replace('&', ""), keys.clone(), action.clone()));
					}
					Entry::Sub(_, sub) => walk(sub, out),
					_ => {}
				}
			}
		}
		let mut out = Vec::new();
		for (_, entries) in menus(&Settings::default()) {
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
			"Left" => WXK_LEFT,
			"Right" => WXK_RIGHT,
			"F1" => WXK_F1,
			"F5" => WXK_F5,
			"F6" => WXK_F6,
			"F7" => WXK_F7,
			"F8" => WXK_F8,
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
			("Open URL…", ""),
			("Import Playlist…", "Ctrl+I"),
			("Import Playlist as Collection…", "Ctrl+Shift+I"),
			("Copy Stream URL", "Ctrl+U"),
			("Get SerrebiRadio Stations", ""),
			("Play/Pause", "Ctrl+P"),
			("Stop", "Ctrl+S"),
			("Previous Station", "Alt+Left"),
			("Next Station", "Alt+Right"),
			("Move to Playing Station", ""),
			("Record", "Ctrl+R"),
			("Pause Recording", "Ctrl+Shift+R"),
			("Record Each Song to Its Own File", ""),
			("Skip Back", "F6"),
			("Skip Forward", "F7"),
			("Jump to Start of Buffer", "F5"),
			("Go to Live", "F8"),
			("Previous Segment", "Shift+F6"),
			("Next Segment", "Shift+F7"),
			("Write Current Segment", "Ctrl+F8"),
			("Write Previous Segment", "Ctrl+F6"),
			("Write Next Segment", "Ctrl+F7"),
			("Write All Segments", "Ctrl+F5"),
			("Mute", "Ctrl+M"),
			("Volume Up", "Ctrl+Up"),
			("Volume Down", "Ctrl+Down"),
			("Volume Up 5%", "Ctrl+Shift+Up"),
			("Volume Down 5%", "Ctrl+Shift+Down"),
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
			("Settings…", "Ctrl+T"),
			("Keyboard Shortcuts…", ""),
			("User Guide", "F1"),
			("View Changelog", ""),
			("Go to BlindRadio GitHub Page", ""),
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
				app_key(&Settings::default(), code, ctrl, alt, shift, true),
				Some(action.clone()),
				"{label} {keys}"
			);
		}
		let titles: Vec<_> = menus(&Settings::default()).into_iter().map(|(t, _)| t).collect();
		assert_eq!(
			titles,
			["&File", "&Stations", "&Playback", "&Recording", "&Help"].map(String::from)
		);
	}

	#[test]
	fn favorite_label_flips() {
		assert_eq!(favorite_label(false), "&Add to Favorites");
		assert_eq!(favorite_label(true), "Re&move from Favorites");
		assert_eq!(plain(&favorite_label(false)), "Add to Favorites");
		assert_eq!(plain(&favorite_label(true)), "Remove from Favorites");
	}

	/// One menu's entries, by title.
	fn menu(title: String) -> Vec<Entry> {
		menus(&Settings::default())
			.into_iter()
			.find(|(t, _)| *t == title)
			.map(|(_, entries)| entries)
			.unwrap_or_else(|| panic!("{title} missing"))
	}

	/// The items' labels without their Alt letters.
	fn labels(entries: &[Entry]) -> Vec<String> {
		entries
			.iter()
			.filter_map(|e| match e {
				Entry::Item(l, ..) | Entry::Sub(l, _) => Some(plain(l)),
				Entry::Devices => Some(plain(&crate::strings::audio_device())),
				Entry::Separator => None,
			})
			.collect()
	}

	#[test]
	fn recording_and_station_items_live_where_they_belong() {
		let recording = labels(&menu(s::menu_recording()));
		for want in [
			"Record",
			"Pause Recording",
			"Record Each Song to Its Own File",
			"Write Buffer…",
			"Scheduled Recordings…",
		] {
			assert!(recording.iter().any(|l| l == want), "{want}");
		}
		let stations = labels(&menu(s::menu_stations()));
		for want in [
			"Previous Station",
			"Next Station",
			"Move to Playing Station",
			"Add to Favorites",
			"Add to Collection…",
		] {
			assert!(stations.iter().any(|l| l == want), "{want}");
		}
		let playback = labels(&menu(s::menu_playback()));
		for gone in [
			"Record",
			"Pause Recording",
			"Record Each Song to Its Own File",
			"Write Buffer…",
			"Scheduled Recordings…",
			"Previous Station",
			"Next Station",
			"Move to Playing Station",
			"Add to Favorites",
			"Add to Collection…",
		] {
			assert!(
				!playback.iter().any(|l| l == gone),
				"{gone} moved out of Playback"
			);
		}
	}

	#[test]
	fn file_exports_route_to_their_scopes() {
		let entries = menu(s::menu_file());
		for action in [
			Action::ExportCurrentList,
			Action::ExportAllPlaylists,
			Action::ExportFavorites,
		] {
			assert!(
				entries
					.iter()
					.any(|entry| matches!(entry, Entry::Item(_, _, bound) if *bound == action))
			);
		}
	}

	#[test]
	fn file_menu_has_no_checks() {
		let file = menu(s::menu_file());
		// The audio device submenu lives in Playback now, not File.
		assert!(!file.iter().any(|e| matches!(e, Entry::Devices)));
		let playback = menu(s::menu_playback());
		assert!(playback.iter().any(|e| matches!(e, Entry::Devices)));
		let labels: Vec<String> = file
			.iter()
			.filter_map(|e| match e {
				Entry::Item(l, ..) => Some(plain(l)),
				_ => None,
			})
			.collect();
		// Settings lives in File now.
		assert!(labels.iter().any(|l| l == "Settings…"));
		for gone in [
			"Announce Song Titles",
			"Announce Connection Status",
			"Close Lists When Playing",
			"Play Last Station at Startup",
		] {
			assert!(!labels.iter().any(|l| l == gone), "{gone} moved to Settings");
		}
	}

	#[test]
	fn alt_letters_unique_per_menu() {
		fn letters(entries: &[Entry]) {
			let mut seen = Vec::new();
			for e in entries {
				let label = match e {
					Entry::Item(l, ..) | Entry::Sub(l, _) => l.clone(),
					Entry::Devices => crate::strings::audio_device(),
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
		for (_, entries) in menus(&Settings::default()) {
			letters(&entries);
		}
	}
}
