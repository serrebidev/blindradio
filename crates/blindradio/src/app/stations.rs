//! Station clipboard and filing: copy, cut, paste, favorites, collections.

use super::*;

impl App {
	/// Add to Collection: says which way it went.
	pub fn toggle_in_collection(&mut self, id: &str, station: &Station) {
		let Some(name) = self
			.library
			.collections
			.iter()
			.find(|c| c.id == id)
			.map(|c| c.name.clone())
		else {
			return;
		};
		let added = self.library.toggle_in_collection(id, station);
		let prefix = if added { s::ADDED_TO } else { s::REMOVED_FROM };
		self.say(&format!("{prefix}{name}"));
		self.save_library();
		self.refresh_dial();
	}

	/// The list the main tree's focused row stands in. None behind a dialog,
	/// and while the Filter box flattens the tree: its rows show no folders.
	pub(super) fn tree_home(&self) -> Option<PasteDest> {
		if self.dialog_open || !self.dial_filter.is_empty() {
			return None;
		}
		let (id, path) = self
			.win
			.stations
			.selected_home(&self.library, self.settings.main_list.as_deref())?;
		Some(PasteDest::Collection(id, path))
	}

	/// The station Ctrl+C and Ctrl+X act on. `selected` outlives a move onto a main tree
	/// folder, so there the row itself decides; the dialogs empty `selected` on their folders.
	pub(super) fn focused_station(&self) -> Option<Station> {
		if self.dialog_open {
			return self.selected.clone();
		}
		let rows = self.win.stations.stations();
		self.win.stations.current().and_then(|i| rows.get(i).cloned())
	}

	/// Copy or cut a station onto the clipboard. A cut station stays
	/// where it is until Paste takes it out of its home.
	pub fn copy_station(&mut self, station: &Station, cut: bool) {
		// The focused row is the copy meant, when it shows this station: a cut
		// takes that copy out, not the first one filed.
		let rows = self.win.stations.stations();
		let on_row = self
			.win
			.stations
			.current()
			.and_then(|i| rows.get(i))
			.is_some_and(|s| s.id == station.id);
		let focused = self
			.paste_home
			.clone()
			.or_else(|| self.tree_home().filter(|_| on_row));
		let (from_collection, from_favorites, from_path) =
			match PasteDest::for_station(&self.library, &station.id, focused.as_ref()) {
				Some(PasteDest::Collection(id, path)) => (Some(id), false, path),
				Some(PasteDest::Favorites(path)) => (None, true, path),
				None => (None, false, Vec::new()),
			};
		self.station_clip = Some(Clip {
			station: station.clone(),
			cut,
			from_collection,
			from_favorites,
			from_path,
		});
		let prefix = if cut { s::CUT_SAID } else { s::COPIED_STATION };
		self.say(&format!("{prefix}{}", station.name));
	}

	/// Put the clipboard station at `dest`. A cut station is taken out
	/// of its home first, so pasting it into another folder of its own
	/// collection moves it there.
	pub fn paste_into(&mut self, dest: PasteDest) {
		let Some(clip) = self.station_clip.clone() else {
			return self.say(s::NOTHING_TO_PASTE);
		};
		let st = &clip.station;
		let (held, dest_name) = match &dest {
			PasteDest::Collection(id, path) => {
				let Some(c) = self.library.collections.iter().find(|c| &c.id == id) else {
					return self.say(s::NOWHERE_TO_PASTE);
				};
				// The destination list itself must exist before
				// anything is taken out of anywhere for a cut.
				let Some(list) = c.stations_at(path) else {
					return self.say(s::NOWHERE_TO_PASTE);
				};
				(
					list.iter().any(|x| x.id == st.id),
					// The list itself, not the collection around it: a playlist
					// imported into Custom Stations is a folder there.
					c.folder_at(path)
						.map_or_else(|| c.name.clone(), |f| f.name.clone()),
				)
			}
			PasteDest::Favorites(path) => {
				let Some(list) = self.library.fav_stations_at(path) else {
					return self.say(s::NOWHERE_TO_PASTE);
				};
				(
					list.iter().any(|x| x.id == st.id),
					s::FAVORITES_LIST.to_string(),
				)
			}
		};
		// A cut pasted back into the very list it was taken from has
		// nowhere to go; that is the one "already in" a cut keeps.
		let same_home = clip.cut
			&& match &dest {
				PasteDest::Collection(id, path) => {
					clip.from_collection.as_deref() == Some(id.as_str())
						&& clip.from_path == *path
				}
				PasteDest::Favorites(path) => clip.from_favorites && clip.from_path == *path,
			};
		if held && (!clip.cut || same_home) {
			return self.say(&format!("{}{}{}", st.name, s::IS_ALREADY_IN, dest_name));
		}
		if clip.cut {
			// Out of the list it was taken from, specifically: the
			// same station may be filed in other lists too, and
			// those copies are not the cut's to take.
			if let Some(from) = &clip.from_collection
				&& let Some(list) = self
					.library
					.collection_mut(from)
					.and_then(|c| c.stations_at_mut(&clip.from_path))
			{
				list.retain(|x| x.id != st.id);
			}
			if clip.from_favorites
				&& let Some(list) = self.library.fav_stations_at_mut(&clip.from_path)
			{
				list.retain(|x| x.id != st.id);
			}
			self.station_clip = None;
		}
		// A cut whose station already lives at the destination (filed
		// there separately) is done once the source copy is out: the
		// move has landed, and pushing would duplicate it.
		let landed = held
			|| match &dest {
				PasteDest::Collection(id, path) => self
					.library
					.collection_mut(id)
					.and_then(|c| c.stations_at_mut(path))
					.map(|v| v.push(st.clone()))
					.is_some(),
				PasteDest::Favorites(path) => self
					.library
					.fav_stations_at_mut(path)
					.map(|v| v.push(st.clone()))
					.is_some(),
			};
		if !landed {
			return self.say(s::NOWHERE_TO_PASTE);
		}
		self.say(&format!(
			"{}{}{}{}",
			s::PASTED,
			st.name,
			s::INTO,
			dest_name
		));
		self.save_library();
		self.refresh_dial();
	}

	/// Paste beside `station`: into the folder that holds it, or the
	/// dial's collection when it is held nowhere.
	pub fn paste_near(&mut self, station: &Station) {
		let focused = self.paste_home.clone().or_else(|| self.tree_home());
		let dest =
			match PasteDest::for_station(&self.library, &station.id, focused.as_ref()) {
				Some(dest) => dest,
				None => match dial_collection(&self.library, self.settings.main_list.as_deref()) {
					Some(c) => PasteDest::Collection(c.id.clone(), Vec::new()),
					None => return self.say(s::NOWHERE_TO_PASTE),
				},
			};
		self.paste_into(dest);
	}

	/// New Collection: an existing name is that collection; the station is added, never removed.
	/// Focus landed on a station in a list or tree: it plays when Play On
	/// Move is on, and is only the selection when it is off.
	pub fn focus_station(&mut self, stations: &[Station], index: usize) {
		if self.settings.play_on_move {
			self.tune(stations, index);
		} else if let Some(station) = stations.get(index) {
			self.selected = Some(station.clone());
		}
	}

	/// Into a favorites folder; says so when it lands.
	pub fn add_favorite_to(&mut self, path: &[usize], station: &Station) {
		if self.library.add_favorite_to(path, station) {
			self.say(s::ADDED_TO_FAVORITES);
			self.save_library();
		}
	}

	/// Browse and the station menu: file a station into one folder of a
	/// collection (the root-only Add to Collection could not reach them).
	pub fn add_to_collection_folder(&mut self, id: &str, path: &[usize], station: &Station) {
		let Some(name) = self
			.library
			.collections
			.iter()
			.find(|c| c.id == id)
			.map(|c| c.name.clone())
		else {
			return;
		};
		if self.library.add_to_collection_folder(id, path, station) {
			self.say(&format!("{}{name}", s::ADDED_TO));
			self.save_library();
			self.refresh_dial();
		}
	}

	pub fn add_to_new_collection(&mut self, name: &str, station: &Station) {
		let id = self.library.create_collection(name);
		if !self
			.library
			.collections_containing(&station.id)
			.contains(&id)
		{
			self.library.toggle_in_collection(&id, station);
		}
		self.say(&format!("{}{name}", s::ADDED_TO));
		self.save_library();
		self.refresh_dial();
	}

	/// The context menu's Delete Custom Station where the list has no delete of its own.
	pub fn delete_custom(&mut self, id: &str) {
		// Behind a modal the dialog tunes its own replacement.
		let list = &self.win.stations;
		let was_focused = !self.dialog_open
			&& list
				.current()
				.and_then(|i| list.stations().get(i).map(|s| s.id == id))
				.unwrap_or(false);
		self.library.delete_custom(id);
		self.save_library();
		self.refresh_dial();
		if was_focused {
			self.play_focused_replacement(id);
		}
	}

	/// Out of Favorites; the playing station plays on.
	/// Add All to Collection: says how many arrived.
	pub fn add_all(&mut self, from: &str, to: &str) {
		let n = self.library.add_all_to_collection(from, to);
		let name = self
			.library
			.collections
			.iter()
			.find(|c| c.id == to)
			.map(|c| c.name.clone())
			.unwrap_or_default();
		self.save_library();
		self.say(&format!("{}{n}{}{name}", s::ADDED, s::TO));
	}

	/// Copy Stream URL: the focused station's URL, read back from the clipboard.
	pub(super) fn copy_url(&mut self) {
		match self.station_here() {
			Some(st) => {
				let ok = clipboard::copy(&st.url);
				self.say(clipboard::message(ok));
			}
			None => self.say(s::NOTHING_TO_COPY),
		}
	}
}
