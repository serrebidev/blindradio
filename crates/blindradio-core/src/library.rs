use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::station::{Source, Station};
use crate::{Result, store};

/// Id of the Custom Stations collection.
pub const CUSTOM: &str = "custom";
const CUSTOM_NAME: &str = "Custom Stations";
const RECENT_CAP: usize = 50;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum LibraryError {
	#[error("Custom Stations is fixed")]
	CustomFixed,
	#[error("No such collection")]
	NoCollection,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Collection {
	pub id: String,
	pub name: String,
	pub stations: Vec<Station>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Library {
	pub favorites: Vec<Station>,
	pub recent: Vec<Station>,
	pub collections: Vec<Collection>,
}

impl Default for Library {
	fn default() -> Self {
		let mut lib = Library {
			favorites: Vec::new(),
			recent: Vec::new(),
			collections: Vec::new(),
		};
		lib.ensure_custom();
		lib
	}
}

/// Swaps the item at `index` one step; returns its new index (unchanged at the ends).
fn step<T>(list: &mut [T], index: usize, up: bool) -> usize {
	if index >= list.len() {
		return index;
	}
	let to = if up {
		index.checked_sub(1)
	} else {
		Some(index + 1).filter(|&i| i < list.len())
	};
	match to {
		Some(to) => {
			list.swap(index, to);
			to
		}
		None => index,
	}
}

impl Library {
	pub fn load(dir: &Path) -> Library {
		let mut lib: Library = store::load(&dir.join(store::LIBRARY_FILE));
		lib.ensure_custom();
		lib
	}

	pub fn save(&self, dir: &Path) -> Result<()> {
		store::save(&dir.join(store::LIBRARY_FILE), self)
	}

	/// Custom Stations is always present and first.
	fn ensure_custom(&mut self) {
		match self.collections.iter().position(|c| c.id == CUSTOM) {
			Some(0) => {}
			Some(i) => {
				let c = self.collections.remove(i);
				self.collections.insert(0, c);
			}
			None => self.collections.insert(
				0,
				Collection {
					id: CUSTOM.into(),
					name: CUSTOM_NAME.into(),
					stations: Vec::new(),
				},
			),
		}
	}

	fn collection_mut(&mut self, id: &str) -> Option<&mut Collection> {
		self.collections.iter_mut().find(|c| c.id == id)
	}

	pub fn is_favorite(&self, id: &str) -> bool {
		self.favorites.iter().any(|s| s.id == id)
	}

	/// True when the station is now a favorite (added at the end).
	pub fn toggle_favorite(&mut self, s: &Station) -> bool {
		if self.is_favorite(&s.id) {
			self.remove_favorite(&s.id);
			false
		} else {
			self.favorites.push(s.clone());
			true
		}
	}

	pub fn remove_favorite(&mut self, id: &str) {
		self.favorites.retain(|s| s.id != id);
	}

	pub fn move_favorite(&mut self, index: usize, up: bool) -> usize {
		step(&mut self.favorites, index, up)
	}

	pub fn add_recent(&mut self, s: &Station) {
		self.recent.retain(|r| r.id != s.id);
		self.recent.insert(0, s.clone());
		self.recent.truncate(RECENT_CAP);
	}

	pub fn add_custom(&mut self, name: &str, url: &str) -> Station {
		let s = Station {
			id: format!("custom:{}", uuid::Uuid::new_v4()),
			name: name.into(),
			url: url.into(),
			source: Source::Custom,
			info: None,
		};
		self.ensure_custom();
		self.collections[0].stations.push(s.clone());
		s
	}

	/// Copies in favorites or other collections stay.
	pub fn delete_custom(&mut self, id: &str) {
		if let Some(c) = self.collection_mut(CUSTOM) {
			c.stations.retain(|s| s.id != id);
		}
	}

	/// A name that exists already (case-insensitive) returns that collection's id.
	pub fn create_collection(&mut self, name: &str) -> String {
		let lower = name.to_lowercase();
		if let Some(c) = self
			.collections
			.iter()
			.find(|c| c.name.to_lowercase() == lower)
		{
			return c.id.clone();
		}
		let id = format!("col:{}", uuid::Uuid::new_v4());
		self.collections.push(Collection {
			id: id.clone(),
			name: name.into(),
			stations: Vec::new(),
		});
		id
	}

	pub fn rename_collection(
		&mut self,
		id: &str,
		name: &str,
	) -> std::result::Result<(), LibraryError> {
		if id == CUSTOM {
			return Err(LibraryError::CustomFixed);
		}
		self.collection_mut(id)
			.ok_or(LibraryError::NoCollection)?
			.name = name.into();
		Ok(())
	}

	pub fn delete_collection(&mut self, id: &str) -> std::result::Result<(), LibraryError> {
		if id == CUSTOM {
			return Err(LibraryError::CustomFixed);
		}
		let before = self.collections.len();
		self.collections.retain(|c| c.id != id);
		if self.collections.len() == before {
			return Err(LibraryError::NoCollection);
		}
		Ok(())
	}

	/// True when the station is now in the collection. False if removed or no such collection.
	pub fn toggle_in_collection(&mut self, id: &str, s: &Station) -> bool {
		let Some(c) = self.collection_mut(id) else {
			return false;
		};
		if let Some(i) = c.stations.iter().position(|x| x.id == s.id) {
			c.stations.remove(i);
			false
		} else {
			c.stations.push(s.clone());
			true
		}
	}

	pub fn remove_from_collection(&mut self, id: &str, station_id: &str) {
		if id == CUSTOM {
			return self.delete_custom(station_id);
		}
		if let Some(c) = self.collection_mut(id) {
			c.stations.retain(|s| s.id != station_id);
		}
	}

	pub fn move_in_collection(&mut self, id: &str, index: usize, up: bool) -> usize {
		match self.collection_mut(id) {
			Some(c) => step(&mut c.stations, index, up),
			None => index,
		}
	}

	/// Custom Stations stays first: it never moves and nothing moves above it.
	pub fn move_collection(&mut self, id: &str, up: bool) {
		if id == CUSTOM {
			return;
		}
		if let Some(i) = self.collections.iter().position(|c| c.id == id) {
			step(&mut self.collections[1..], i - 1, up);
		}
	}

	/// Count added; stations already in `to` stay as they are.
	pub fn add_all_to_collection(&mut self, from: &str, to: &str) -> usize {
		let Some(src) = self.collections.iter().find(|c| c.id == from) else {
			return 0;
		};
		let src = src.stations.clone();
		let Some(dst) = self.collection_mut(to) else {
			return 0;
		};
		let mut added = 0;
		for s in src {
			if !dst.stations.iter().any(|x| x.id == s.id) {
				dst.stations.push(s);
				added += 1;
			}
		}
		added
	}

	pub fn collections_containing(&self, station_id: &str) -> Vec<String> {
		self.collections
			.iter()
			.filter(|c| c.stations.iter().any(|s| s.id == station_id))
			.map(|c| c.id.clone())
			.collect()
	}
}
