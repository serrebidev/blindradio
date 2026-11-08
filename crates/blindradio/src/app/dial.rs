//! The main tree: rebuilding it, revealing rows, filtering, moving and deleting.

use super::*;

impl App {
	/// Refills the main window list; focus stays on the same station, else the same place.
	pub fn refresh_dial(&mut self) {
		let (stations, nodes) = dial_tree(
			&self.library,
			self.settings.main_list.as_deref(),
			&self.dial_filter,
		);
		let list = &self.win.stations;
		let current = list.current();
		// Past the end when the last row was the one deleted: the row before it.
		// The same copy of a station filed in several folders: its Nth row stays its Nth row.
		let old = list.stations();
		let focus = current
			.and_then(|i| {
				let id = &old.get(i)?.id;
				let nth = old[..i].iter().filter(|s| s.id == *id).count();
				let mut copies = stations.iter().enumerate().filter(|(_, s)| s.id == *id);
				let first = copies.clone().next();
				copies.nth(nth).or(first).map(|(at, _)| at)
			})
			.or(current)
			.unwrap_or(0)
			.min(stations.len().saturating_sub(1));
		list.set_expand_default(self.settings.expand_dial_folders);
		list.set_dial(&nodes, stations, focus);
	}

	/// Select quietly, so revealing playback cannot tune the station again.
	/// False when no row shows it: an opened URL, a search result, a favorite
	/// no list holds. The tree and its selection stay as they were.
	/// Behind a modal the hidden tree waits for the dialog to close: rebuilding
	/// it on every arrow key slows the dialog and takes its focus.
	pub(super) fn reveal_station(&mut self, station: &Station) -> bool {
		if self.dialog_open {
			self.reveal_on_close = true;
			return false;
		}
		let mut rebuild = false;
		let present = self
			.win
			.stations
			.stations()
			.iter()
			.any(|s| s.id == station.id);
		if !present {
			let Some(id) = self
				.library
				.collections_containing(&station.id)
				.first()
				.cloned()
			else {
				return false;
			};
			self.settings.main_list = Some(id);
			self.save_settings();
			rebuild = true;
		}
		if !self.dial_filter.is_empty()
			&& !station
				.name
				.to_lowercase()
				.contains(&self.dial_filter.to_lowercase())
		{
			self.dial_filter.clear();
			self.win.filter.change_value("");
			self.clear_message();
			rebuild = true;
		}
		if rebuild {
			self.refresh_dial();
		}
		let stations = self.win.stations.stations();
		// A row already showing this station stays: moving onto a copy filed in a
		// later folder must not throw focus back to the first copy.
		let on_it = self
			.win
			.stations
			.current()
			.and_then(|i| stations.get(i))
			.is_some_and(|s| s.id == station.id);
		if !on_it && let Some(i) = stations.iter().position(|s| s.id == station.id) {
			self.win.stations.select(i, false);
		}
		true
	}

	/// A modal closed: what it left playing gets its row, once, and is what
	/// Add to Favorites and the like act on until the user moves.
	pub fn reveal_after_dialog(&mut self) {
		if std::mem::take(&mut self.reveal_on_close)
			&& let Some(station) = self.tuner.playing().cloned()
		{
			self.reveal_station(&station);
			self.selected = Some(station);
		}
	}

	/// Edit Station: every held copy, Recently Played, and what later playback reuses.
	pub fn update_station(&mut self, id: &str, name: &str, url: &str, info: Option<&str>) -> bool {
		let copies = self.played.iter_mut().chain(self.settings.last_station.as_mut());
		for st in copies.filter(|st| st.id == id) {
			st.name = name.to_string();
			st.url = url.to_string();
			st.info = info.map(str::to_string);
		}
		self.library.update_station(id, name, url, info)
	}

	/// Return to the playing row without restarting it or changing playback history.
	pub fn move_to_playing_station(&mut self) {
		if self.dialog_open {
			return;
		}
		let Some(station) = self.tuner.playing().cloned() else {
			return;
		};
		if !self.reveal_station(&station) {
			return self.say(&s::playing_not_listed());
		}
		if !self.win.stations.list.has_focus() {
			self.win.stations.list.set_focus();
		}
		self.selected = Some(station);
	}

	/// The filter box changed: the tree narrows to matching stations, or
	/// comes back whole when the box empties.
	pub fn dial_filter_set(&mut self, text: String) {
		if text == self.dial_filter {
			return;
		}
		self.dial_filter = text;
		if self.dial_filter.is_empty() {
			self.win.message.set_label("");
			self.refresh_dial();
		} else {
			self.apply_dial_filter();
		}
	}

	pub(super) fn apply_dial_filter(&mut self) {
		let stations = dial_stations(&self.library, self.settings.main_list.as_deref());
		let q = self.dial_filter.to_lowercase();
		let hits: Vec<usize> = stations
			.iter()
			.enumerate()
			.filter(|(_, st)| st.name.to_lowercase().contains(&q))
			.map(|(i, _)| i)
			.collect();
		self.win.message.set_label(&if hits.is_empty() {
			format!("{}: {} - {}", s::filter(), self.dial_filter, s::no_matches())
		} else {
			format!(
				"{}: {} - {} {}",
				s::filter(),
				self.dial_filter,
				hits.len(),
				s::matches()
			)
		});
		self.refresh_dial();
		if let Some(&first) = hits.first() {
			self.win.stations.select(first, false);
			self.selected = stations.get(first).cloned();
		}
	}

	/// Focuses the main window list; false when it is empty. `play`: it plays now, the last
	/// station first with Play Last Station at Startup; silent otherwise.
	pub(super) fn land_dial(&mut self, play: bool) -> bool {
		let list = self.win.stations.clone();
		let stations = list.stations();
		if stations.is_empty() {
			return false;
		}
		list.list.set_focus();
		if !play {
			// The row focus landed on raised no selection event: Play/Pause
			// would answer "Nothing to play" with a station under focus.
			if self.selected.is_none() {
				self.selected = list.current().and_then(|i| stations.get(i).cloned());
			}
			return true;
		}
		match self
			.settings
			.play_last_at_startup
			.then(|| self.settings.last_station.clone())
			.flatten()
		{
			Some(last) => match stations.iter().position(|s| s.id == last.id) {
				Some(i) => {
					list.select(i, false);
					self.tune(&stations, i);
					// The startup station counts as played: Alt+Left from
					// the next station must be able to come back to it.
					// tune() only records it when Play on Move is on.
					self.note_played(&stations[i]);
				}
				None => self.play(last),
			},
			None => list.land(self),
		}
		true
	}

	/// Shift+Up or Shift+Down in the main tree: the focused row moves one step among its
	/// own kind on its level, in the library itself. Focus goes with it. Not while the
	/// Filter box flattens the tree.
	pub fn move_dial_row(&mut self, up: bool) {
		let Some(PasteDest::Collection(id, path)) = self.tree_home() else {
			return;
		};
		let list = self.win.stations.clone();
		if let Some(index) = list.selected_level_index() {
			let moved = self
				.library
				.collection_mut(&id)
				.and_then(|c| c.move_station(&path, index, up));
			if moved.is_none() {
				return;
			}
		} else {
			let Some(path) = self.library.move_at(&id, &path, up) else {
				return;
			};
			list.land_on_folder((id, path));
		}
		self.save_library();
		self.refresh_dial();
	}

	/// Delete in the main window list: out of the list its row stands in, not another
	/// copy filed elsewhere; the playing station plays on. With the Filter box up the
	/// rows show no folders, so it goes from the dial collection.
	pub fn delete_from_dial(&mut self, index: usize) {
		let Some(st) = self.win.stations.stations().get(index).cloned() else {
			return;
		};
		let was_focused = self.win.stations.current() == Some(index);
		let removed = match self.tree_home() {
			Some(PasteDest::Collection(id, path)) => self
				.library
				.collection_mut(&id)
				.and_then(|c| c.stations_at_mut(&path))
				.map(|list| list.retain(|x| x.id != st.id))
				.is_some(),
			_ => match dial_collection(&self.library, self.settings.main_list.as_deref()) {
				Some(c) => {
					let id = c.id.clone();
					self.library.remove_from_collection(&id, &st.id);
					true
				}
				None => false,
			},
		};
		if !removed {
			return;
		}
		self.save_library();
		self.refresh_dial();
		if was_focused {
			self.play_focused_replacement(&st.id);
		}
	}

	/// Play on Move: the station that took a deleted one's place has the
	/// focus now, so it plays, like moving onto it would. Nothing took the
	/// place of a row that stayed: another copy of `deleted` kept it listed.
	pub(super) fn play_focused_replacement(&mut self, deleted: &str) {
		if self.settings.play_on_move
			&& let Some(i) = self.win.stations.current()
		{
			let stations = self.win.stations.stations();
			if stations.get(i).is_some_and(|st| st.id != deleted) {
				self.tune(&stations, i);
			}
		}
	}
}
