//! The Applications key menu: about the item focused, or `Nothing to do here`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use blindradio_core::library::{CUSTOM, Library};
use blindradio_core::station::Station;
use wx_utils::prompt_text;
use wxdragon::prelude::*;

use crate::action::Action;
use crate::app::App;
use crate::main_window::{dispatch, escape};
use crate::menus::favorite_label;
use crate::strings as s;

/// What the menu is about.
pub enum Target<'a> {
	Station(&'a Station),
	/// `custom` is Custom Stations, which cannot be renamed or deleted.
	Collection {
		custom: bool,
	},
	/// A category, Load More, an empty list.
	Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Item {
	Play,
	/// True when the station is a favorite already: the item removes it.
	Favorite(bool),
	CopyUrl,
	CopyStation,
	CutStation,
	PasteStation,
	EditStation,
	StationSettings,
	/// Every collection but Custom Stations: id, name, holds the station.
	AddToCollection(Vec<(String, String, bool)>),
	/// The same collections alone, as top-level check items (Add to Collection).
	Collections(Vec<(String, String, bool)>),
	NewCollection,
	/// Every other collection, id and name (Add All to Collection).
	AddAllTo(Vec<(String, String)>),
	/// Every favorites folder: its path and its display name.
	AddToFavoriteFolder(Vec<(Vec<usize>, String)>),
	/// Every folder of one collection: its path and its display name.
	MoveToFolder(Vec<(Vec<usize>, String)>),
	DeleteCustom,
	Rename,
	Delete,
}

/// What the user chose.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Choice {
	Play,
	Favorite,
	CopyUrl,
	CopyStation,
	CutStation,
	PasteStation,
	EditStation,
	StationSettings,
	/// Into or out of the collection with this id.
	Toggle(String),
	NewCollection,
	/// Every station of the source collection into the collection with this id.
	AddAllTo(String),
	/// Into the favorites folder at this path.
	AddToFavoriteFolder(Vec<usize>),
	/// Into the folder at this path of the station's own collection.
	MoveToFolder(Vec<usize>),
	DeleteCustom,
	Rename,
	Delete,
}

/// The menu's items, or the message said instead of a menu.
/// `has_clip` is a station sitting on the clipboard: Paste joins the
/// menu of a held station (it lands in that station's folder).
pub fn items_for(
	target: &Target,
	lib: &Library,
	has_clip: bool,
) -> Result<Vec<Item>, &'static str> {
	match target {
		Target::Station(st) => {
			let mut items = vec![
				Item::Play,
				Item::Favorite(lib.is_favorite(&st.id)),
				Item::CopyUrl,
				Item::CopyStation,
				Item::StationSettings,
				Item::AddToCollection(
					lib.collections
						.iter()
						.filter(|c| c.id != CUSTOM)
						.map(|c| (c.id.clone(), c.name.clone(), c.contains_id(&st.id)))
						.collect(),
				),
			];
			// Only a held station can be edited: a directory or network
			// station exists nowhere in the library to save the change to.
			if lib.holds_station(&st.id) {
				items.insert(3, Item::EditStation);
				// Cut and Paste need a home to take from / land beside.
				items.insert(5, Item::CutStation);
				if has_clip {
					items.insert(6, Item::PasteStation);
				}
			}
			if !lib.fav_folders.is_empty() {
				items.push(Item::AddToFavoriteFolder(fav_folder_list(lib)));
			}
			// Imported stations get `custom:` ids too, so only Custom Stations membership counts.
			let custom = lib
				.collections
				.iter()
				.find(|c| c.id == CUSTOM)
				.is_some_and(|c| c.contains_id(&st.id));
			if custom {
				items.push(Item::DeleteCustom);
			}
			Ok(items)
		}
		Target::Collection { custom: false } => Ok(vec![Item::Rename, Item::Delete]),
		// Add All to Collection joins with the Collections window (Task 6).
		Target::Collection { custom: true } | Target::Other => Err(s::NOTHING_TO_DO),
	}
}

/// Every favorites folder with its path, parents first, names joined.
pub fn fav_folder_list(lib: &Library) -> Vec<(Vec<usize>, String)> {
	fn walk(
		folders: &[blindradio_core::library::Folder],
		base: &[usize],
		names: &str,
		out: &mut Vec<(Vec<usize>, String)>,
	) {
		for (i, f) in folders.iter().enumerate() {
			let mut path = base.to_vec();
			path.push(i);
			let label = if names.is_empty() {
				f.name.clone()
			} else {
				format!("{names} / {}", f.name)
			};
			out.push((path.clone(), label.clone()));
			walk(&f.folders, &path, &label, out);
		}
	}
	let mut out = Vec::new();
	walk(&lib.fav_folders, &[], "", &mut out);
	out
}

const FIRST_ID: Id = ID_HIGHEST + 5000;

/// Pops the menu up on `window` and waits; `None` when it was dismissed.
pub fn popup(window: &dyn WxWidget, items: &[Item]) -> Option<Choice> {
	let mut choices = Vec::new();
	let mut add = |c: Choice| {
		choices.push(c);
		FIRST_ID + choices.len() as Id - 1
	};
	let chosen = Rc::new(Cell::new(None));
	let catch = |menu: &Menu| {
		let chosen = chosen.clone();
		menu.on_selected(move |e| {
			e.event.skip(false);
			chosen.set(Some(e.get_id()));
		});
	};
	let mut menu = Menu::builder().build();
	catch(&menu);
	for item in items {
		match item {
			Item::Play => {
				menu.append(add(Choice::Play), s::PLAY_ITEM, "", ItemKind::Normal);
			}
			Item::Favorite(is) => {
				menu.append(
					add(Choice::Favorite),
					favorite_label(*is),
					"",
					ItemKind::Normal,
				);
			}
			Item::CopyUrl => {
				menu.append(add(Choice::CopyUrl), s::COPY_URL, "", ItemKind::Normal);
			}
			Item::CopyStation => {
				menu.append(
					add(Choice::CopyStation),
					s::COPY_STATION_ITEM,
					"",
					ItemKind::Normal,
				);
			}
			Item::CutStation => {
				menu.append(
					add(Choice::CutStation),
					s::CUT_STATION_ITEM,
					"",
					ItemKind::Normal,
				);
			}
			Item::PasteStation => {
				menu.append(
					add(Choice::PasteStation),
					s::PASTE_STATION_ITEM,
					"",
					ItemKind::Normal,
				);
			}
			Item::EditStation => {
				menu.append(
					add(Choice::EditStation),
					s::EDIT_STATION_ITEM,
					"",
					ItemKind::Normal,
				);
			}
			Item::StationSettings => {
				menu.append(
					add(Choice::StationSettings),
					s::STATION_SETTINGS_ITEM,
					"",
					ItemKind::Normal,
				);
			}
			Item::AddToCollection(collections) => {
				let sub = Menu::builder().build();
				// wx sends a submenu's choice to the submenu first.
				catch(&sub);
				for (id, name, holds) in collections {
					let item = sub.append(
						add(Choice::Toggle(id.clone())),
						&escape(name),
						"",
						ItemKind::Check,
					);
					if let Some(item) = item {
						item.check(*holds);
					}
				}
				if !collections.is_empty() {
					sub.append_separator();
				}
				sub.append(
					add(Choice::NewCollection),
					s::NEW_COLLECTION,
					"",
					ItemKind::Normal,
				);
				menu.append_submenu(sub, s::ADD_TO_COLLECTION_SUB, "");
			}
			Item::Collections(collections) => {
				for (id, name, holds) in collections {
					let item = menu.append(
						add(Choice::Toggle(id.clone())),
						&escape(name),
						"",
						ItemKind::Check,
					);
					if let Some(item) = item {
						item.check(*holds);
					}
				}
				if !collections.is_empty() {
					menu.append_separator();
				}
			}
			Item::NewCollection => {
				menu.append(
					add(Choice::NewCollection),
					s::NEW_COLLECTION,
					"",
					ItemKind::Normal,
				);
			}
			Item::AddAllTo(collections) => {
				let sub = Menu::builder().build();
				catch(&sub);
				for (id, name) in collections {
					sub.append(
						add(Choice::AddAllTo(id.clone())),
						&escape(name),
						"",
						ItemKind::Normal,
					);
				}
				menu.append_submenu(sub, s::ADD_ALL_SUB, "");
			}
			Item::AddToFavoriteFolder(folders) => {
				let sub = Menu::builder().build();
				catch(&sub);
				for (path, name) in folders {
					sub.append(
						add(Choice::AddToFavoriteFolder(path.clone())),
						&escape(name),
						"",
						ItemKind::Normal,
					);
				}
				menu.append_submenu(sub, s::ADD_TO_FAV_FOLDER, "");
			}
			Item::MoveToFolder(folders) => {
				let sub = Menu::builder().build();
				catch(&sub);
				for (path, name) in folders {
					sub.append(
						add(Choice::MoveToFolder(path.clone())),
						&escape(name),
						"",
						ItemKind::Normal,
					);
				}
				menu.append_submenu(sub, s::MOVE_TO_FOLDER, "");
			}
			Item::DeleteCustom => {
				menu.append(
					add(Choice::DeleteCustom),
					s::DELETE_CUSTOM,
					"",
					ItemKind::Normal,
				);
			}
			Item::Rename => {
				menu.append(add(Choice::Rename), s::RENAME_ITEM, "", ItemKind::Normal);
			}
			Item::Delete => {
				menu.append(add(Choice::Delete), s::DELETE_ITEM, "", ItemKind::Normal);
			}
		}
	}
	open_on_first_item(window);
	// wxMSW dispatches the choice before `popup_menu` returns.
	window.popup_menu(&mut menu, None);
	let i = usize::try_from(chosen.get()?.checked_sub(FIRST_ID)?).ok()?;
	choices.get(i).cloned()
}

/// A Win32 popup menu opens with nothing highlighted. A Down key queued before its loop starts
/// puts it on the first item, like the menu bar; the menu loop takes it, the list never sees it.
#[cfg(windows)]
fn open_on_first_item(window: &dyn WxWidget) {
	const WM_KEYDOWN: u32 = 0x0100;
	const VK_DOWN: usize = 0x28;
	#[link(name = "user32")]
	unsafe extern "system" {
		fn PostMessageW(hwnd: *mut std::ffi::c_void, msg: u32, wparam: usize, lparam: isize)
		-> i32;
	}
	// SAFETY: posting to a live window of this thread; a stale handle only fails the call.
	unsafe {
		PostMessageW(window.get_handle(), WM_KEYDOWN, VK_DOWN, 0);
	}
}

#[cfg(not(windows))]
fn open_on_first_item(_: &dyn WxWidget) {}

/// Runs a station menu choice.
pub fn apply(app: &Rc<RefCell<App>>, parent: &dyn WxWidget, station: &Station, choice: Choice) {
	let action = match choice {
		Choice::Play => Action::Play,
		Choice::Favorite => Action::ToggleFavorite,
		Choice::CopyUrl => Action::CopyUrl,
		Choice::CopyStation => return app.borrow_mut().copy_station(station, false),
		Choice::CutStation => return app.borrow_mut().copy_station(station, true),
		Choice::PasteStation => return app.borrow_mut().paste_near(station),
		Choice::EditStation => Action::EditStation,
		Choice::StationSettings => Action::StationSettings,
		Choice::Toggle(id) => return app.borrow_mut().toggle_in_collection(&id, station),
		Choice::NewCollection => {
			// Modal: outside the borrow.
			if let Some(name) = prompt_text(parent, s::COLLECTION_NAME, s::NEW_COLLECTION_TITLE) {
				app.borrow_mut().add_to_new_collection(&name, station);
			}
			return;
		}
		Choice::DeleteCustom => {
			// Out of Custom Stations wherever it was chosen: copies in lists and collections stay.
			app.borrow_mut().delete_custom(&station.id);
			return;
		}
		Choice::AddToFavoriteFolder(path) => {
			return app.borrow_mut().add_favorite_to(&path, station);
		}
		// Collection owners handle their own.
		Choice::Rename | Choice::Delete | Choice::AddAllTo(_) | Choice::MoveToFolder(_) => return,
	};
	app.borrow_mut().selected = Some(station.clone());
	dispatch(app, action);
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

	#[test]
	fn context_menu_items_for() {
		let mut lib = Library::default();
		let jazz = lib.create_collection("Jazz");
		let news = lib.create_collection("News");
		let rb = station("rb:1");
		lib.toggle_in_collection(&news, &rb);
		lib.toggle_favorite(&rb);
		let mine = lib.add_custom("Mine", "http://mine");

		assert_eq!(
			items_for(&Target::Station(&rb), &lib, false),
			Ok(vec![
				Item::Play,
				Item::Favorite(true),
				Item::CopyUrl,
				Item::EditStation,
				Item::CopyStation,
				Item::CutStation,
				Item::StationSettings,
				Item::AddToCollection(vec![
					(jazz.clone(), "Jazz".into(), false),
					(news.clone(), "News".into(), true),
				]),
			])
		);
		// A station on the clipboard adds Paste beside a held station.
		assert_eq!(
			items_for(&Target::Station(&rb), &lib, true),
			Ok(vec![
				Item::Play,
				Item::Favorite(true),
				Item::CopyUrl,
				Item::EditStation,
				Item::CopyStation,
				Item::CutStation,
				Item::PasteStation,
				Item::StationSettings,
				Item::AddToCollection(vec![
					(jazz.clone(), "Jazz".into(), false),
					(news.clone(), "News".into(), true),
				]),
			])
		);
		// Imported stations carry `custom:` ids but are not in Custom Stations.
		let imported = Station {
			id: "custom:starter".into(),
			..station("Serrebi One")
		};
		assert_eq!(
			items_for(&Target::Station(&imported), &lib, true),
			Ok(vec![
				Item::Play,
				Item::Favorite(false),
				Item::CopyUrl,
				Item::CopyStation,
				Item::StationSettings,
				Item::AddToCollection(vec![
					(jazz.clone(), "Jazz".into(), false),
					(news.clone(), "News".into(), false),
				]),
			])
		);
		// Custom Stations is not togglable; own stations can be deleted.
		assert_eq!(
			items_for(&Target::Station(&mine), &lib, false),
			Ok(vec![
				Item::Play,
				Item::Favorite(false),
				Item::CopyUrl,
				Item::EditStation,
				Item::CopyStation,
				Item::CutStation,
				Item::StationSettings,
				Item::AddToCollection(vec![
					(jazz, "Jazz".into(), false),
					(news, "News".into(), false),
				]),
				Item::DeleteCustom,
			])
		);
		assert_eq!(
			items_for(&Target::Collection { custom: false }, &lib, false),
			Ok(vec![Item::Rename, Item::Delete])
		);
		assert_eq!(
			items_for(&Target::Collection { custom: true }, &lib, false),
			Err("Nothing to do here")
		);
		assert_eq!(
			items_for(&Target::Other, &lib, false),
			Err("Nothing to do here")
		);
	}
}
