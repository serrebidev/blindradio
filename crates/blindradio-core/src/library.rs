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
	/// Named subfolders, each holding its own stations and folders.
	#[serde(default)]
	pub folders: Vec<Folder>,
}

/// A named folder inside a collection or the favorites: its own stations at
/// its root, plus nested folders. Order is the user's order everywhere.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Folder {
	pub name: String,
	pub stations: Vec<Station>,
	pub folders: Vec<Folder>,
}

impl Folder {
	pub fn new(name: &str) -> Folder {
		Folder {
			name: name.into(),
			stations: Vec::new(),
			folders: Vec::new(),
		}
	}

	/// The child folder with this name (any case), created empty if missing.
	pub fn child_mut(&mut self, name: &str) -> &mut Folder {
		let lower = name.to_lowercase();
		let i = match self.folders.iter().position(|f| f.name.to_lowercase() == lower) {
			Some(i) => i,
			None => {
				self.folders.push(Folder::new(name));
				self.folders.len() - 1
			}
		};
		&mut self.folders[i]
	}

	/// Every station below this folder, depth-first in folder order.
	pub fn all_stations(&self) -> Vec<Station> {
		let mut out = self.stations.clone();
		for f in &self.folders {
			out.extend(f.all_stations());
		}
		out
	}

	pub fn contains_id(&self, id: &str) -> bool {
		self.stations.iter().any(|s| s.id == id)
			|| self.folders.iter().any(|f| f.contains_id(id))
	}

	pub fn contains_url(&self, url: &str) -> bool {
		self.stations.iter().any(|s| s.url == url)
			|| self.folders.iter().any(|f| f.contains_url(url))
	}

	/// Removes the station with this id wherever it sits below here.
	pub fn remove_id(&mut self, id: &str) -> bool {
		let before = self.stations.len();
		self.stations.retain(|s| s.id != id);
		self.stations.len() != before || self.folders.iter_mut().any(|f| f.remove_id(id))
	}

	/// The folder at the end of `path` (indices into nested `folders`), if it exists.
	pub fn at_mut(&mut self, path: &[usize]) -> Option<&mut Folder> {
		match path.split_first() {
			None => Some(self),
			Some((&i, rest)) => self.folders.get_mut(i)?.at_mut(rest),
		}
	}
}

impl Collection {
	/// Root stations plus every folder's, depth-first in folder order.
	pub fn all_stations(&self) -> Vec<Station> {
		let mut out = self.stations.clone();
		for f in &self.folders {
			out.extend(f.all_stations());
		}
		out
	}

	pub fn contains_url(&self, url: &str) -> bool {
		self.stations.iter().any(|s| s.url == url)
			|| self.folders.iter().any(|f| f.contains_url(url))
	}

	/// The root-level folder with this name (any case), created empty if missing.
	pub fn child_mut(&mut self, name: &str) -> &mut Folder {
		let lower = name.to_lowercase();
		let i = match self
			.folders
			.iter()
			.position(|f| f.name.to_lowercase() == lower)
		{
			Some(i) => i,
			None => {
				self.folders.push(Folder::new(name));
				self.folders.len() - 1
			}
		};
		&mut self.folders[i]
	}

	/// Removes the station with this id from the root or any folder.
	pub fn remove_id(&mut self, id: &str) -> bool {
		let before = self.stations.len();
		self.stations.retain(|s| s.id != id);
		self.stations.len() != before || self.folders.iter_mut().any(|f| f.remove_id(id))
	}

	pub fn contains_id(&self, id: &str) -> bool {
		self.stations.iter().any(|s| s.id == id)
			|| self.folders.iter().any(|f| f.contains_id(id))
	}

	/// The folder at `path` (indices into nested folder lists), if it exists.
	pub fn folder_at_mut(&mut self, path: &[usize]) -> Option<&mut Folder> {
		folder_in(&mut self.folders, path)
	}

	/// The station list at `path`: the root list for an empty path, else the
	/// folder's own list.
	pub fn stations_at_mut(&mut self, path: &[usize]) -> Option<&mut Vec<Station>> {
		stations_in(&mut self.stations, &mut self.folders, path)
	}

	/// The folder at `path`, read-only.
	pub fn folder_at(&self, path: &[usize]) -> Option<&Folder> {
		folder_ref(&self.folders, path)
	}

	/// The station list at `path`, read-only.
	pub fn stations_at(&self, path: &[usize]) -> Option<&[Station]> {
		stations_ref(&self.stations, &self.folders, path)
	}

	/// The child-folder list at `parent` (empty is the root list).
	pub fn folder_list_at_mut(&mut self, parent: &[usize]) -> Option<&mut Vec<Folder>> {
		folder_list_in(&mut self.folders, parent)
	}

	/// Adds an empty folder under `parent` (empty is the root); its full path.
	pub fn add_folder(&mut self, parent: &[usize], name: &str) -> Option<Vec<usize>> {
		add_folder_in(&mut self.folders, parent, name)
	}

	/// Removes the folder at `path` and everything inside it.
	pub fn remove_folder(&mut self, path: &[usize]) -> bool {
		remove_folder_in(&mut self.folders, path)
	}

	pub fn rename_folder(&mut self, path: &[usize], name: &str) -> bool {
		match self.folder_at_mut(path) {
			Some(f) => {
				f.name = name.to_string();
				true
			}
			None => false,
		}
	}
}

/// The folder at `path` in a folder list: the first index picks from the
/// list, the rest descend.
pub fn folder_in<'a>(folders: &'a mut [Folder], path: &[usize]) -> Option<&'a mut Folder> {
	match path.split_first() {
		None => None,
		Some((&i, rest)) => folders.get_mut(i)?.at_mut(rest),
	}
}

/// The station list at `path` under a root list and its folders.
pub fn stations_in<'a>(
	root: &'a mut Vec<Station>,
	folders: &'a mut Vec<Folder>,
	path: &[usize],
) -> Option<&'a mut Vec<Station>> {
	if path.is_empty() {
		return Some(root);
	}
	Some(&mut folder_in(folders, path)?.stations)
}

/// Adds an empty folder under `parent` in a folder list; its full path.
pub fn add_folder_in(folders: &mut Vec<Folder>, parent: &[usize], name: &str) -> Option<Vec<usize>> {
	let list = match parent.split_first() {
		None => folders,
		Some(_) => &mut folder_in(folders, parent)?.folders,
	};
	list.push(Folder::new(name));
	let mut path = parent.to_vec();
	path.push(list.len() - 1);
	Some(path)
}

/// The folder at `path` in a folder list, read-only.
pub fn folder_ref<'a>(folders: &'a [Folder], path: &[usize]) -> Option<&'a Folder> {
	match path.split_first() {
		None => None,
		Some((&i, rest)) => {
			let f = folders.get(i)?;
			if rest.is_empty() {
				Some(f)
			} else {
				folder_ref(&f.folders, rest)
			}
		}
	}
}

/// The station list at `path` under a root list and its folders, read-only.
pub fn stations_ref<'a>(
	root: &'a [Station],
	folders: &'a [Folder],
	path: &[usize],
) -> Option<&'a [Station]> {
	if path.is_empty() {
		return Some(root);
	}
	Some(&folder_ref(folders, path)?.stations)
}

/// The folder list at `parent`: the list itself for an empty path, else the
/// folder's own child list.
pub fn folder_list_in<'a>(
	folders: &'a mut Vec<Folder>,
	parent: &[usize],
) -> Option<&'a mut Vec<Folder>> {
	if parent.is_empty() {
		return Some(folders);
	}
	Some(&mut folder_in(folders, parent)?.folders)
}

/// Removes the folder at `path` from a folder list, with everything inside.
pub fn remove_folder_in(folders: &mut Vec<Folder>, path: &[usize]) -> bool {
	let Some((&last, parent)) = path.split_last() else {
		return false;
	};
	let list = match parent.split_first() {
		None => folders,
		Some(_) => match folder_in(folders, parent) {
			Some(f) => &mut f.folders,
			None => return false,
		},
	};
	if last >= list.len() {
		return false;
	}
	list.remove(last);
	true
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Library {
	pub favorites: Vec<Station>,
	/// Favorites folders; `favorites` itself is the favorites root.
	#[serde(default)]
	pub fav_folders: Vec<Folder>,
	pub recent: Vec<Station>,
	pub collections: Vec<Collection>,
}

impl Default for Library {
	fn default() -> Self {
		let mut lib = Library {
			favorites: Vec::new(),
			fav_folders: Vec::new(),
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
	pub(crate) fn ensure_custom(&mut self) {
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
					folders: Vec::new(),
				},
			),
		}
	}

	pub fn collection_mut(&mut self, id: &str) -> Option<&mut Collection> {
		self.collections.iter_mut().find(|c| c.id == id)
	}

	pub fn is_favorite(&self, id: &str) -> bool {
		self.favorites.iter().any(|s| s.id == id)
			|| self.fav_folders.iter().any(|f| f.contains_id(id))
	}

	/// True when the library holds a copy of the station (so it can be edited).
	pub fn holds_station(&self, id: &str) -> bool {
		self.is_favorite(id) || self.collections.iter().any(|c| c.contains_id(id))
	}

	/// Renames and re-points every held copy of a station: collections and
	/// their folders, favorites and their folders. True when any copy changed.
	pub fn update_station(&mut self, id: &str, name: &str, url: &str) -> bool {
		fn walk(
			stations: &mut [Station],
			folders: &mut [Folder],
			id: &str,
			name: &str,
			url: &str,
		) -> bool {
			let mut hit = false;
			for s in stations.iter_mut().filter(|s| s.id == id) {
				s.name = name.to_string();
				s.url = url.to_string();
				hit = true;
			}
			for f in folders {
				hit |= walk(&mut f.stations, &mut f.folders, id, name, url);
			}
			hit
		}
		let mut hit = walk(&mut self.favorites, &mut self.fav_folders, id, name, url);
		for c in &mut self.collections {
			hit |= walk(&mut c.stations, &mut c.folders, id, name, url);
		}
		hit
	}

	/// Every favorite, root first then folders depth-first.
	pub fn all_favorites(&self) -> Vec<Station> {
		let mut out = self.favorites.clone();
		for f in &self.fav_folders {
			out.extend(f.all_stations());
		}
		out
	}

	/// True when the station is now a favorite (added at the end of the root).
	pub fn toggle_favorite(&mut self, s: &Station) -> bool {
		if self.is_favorite(&s.id) {
			self.remove_favorite(&s.id);
			false
		} else {
			self.favorites.push(s.clone());
			true
		}
	}

	/// Adds the station to the favorites folder at `path` (empty is the root).
	/// False when it is already a favorite anywhere.
	pub fn add_favorite_to(&mut self, path: &[usize], s: &Station) -> bool {
		if self.is_favorite(&s.id) {
			return false;
		}
		match path.split_first() {
			None => self.favorites.push(s.clone()),
			Some((&i, rest)) => {
				let Some(f) = self.fav_folders.get_mut(i) else {
					return false;
				};
				match f.at_mut(rest) {
					Some(f) => f.stations.push(s.clone()),
					None => return false,
				}
			}
		}
		true
	}

	pub fn remove_favorite(&mut self, id: &str) {
		self.favorites.retain(|s| s.id != id);
		for f in &mut self.fav_folders {
			f.remove_id(id);
		}
	}

	pub fn move_favorite(&mut self, index: usize, up: bool) -> usize {
		step(&mut self.favorites, index, up)
	}

	/// The favorites folder at `path`, if it exists.
	pub fn fav_folder_at_mut(&mut self, path: &[usize]) -> Option<&mut Folder> {
		folder_in(&mut self.fav_folders, path)
	}

	/// The favorites folder at `path`, read-only.
	pub fn fav_folder_at(&self, path: &[usize]) -> Option<&Folder> {
		folder_ref(&self.fav_folders, path)
	}

	/// The favorites station list at `path`, read-only.
	pub fn fav_stations_at(&self, path: &[usize]) -> Option<&[Station]> {
		stations_ref(&self.favorites, &self.fav_folders, path)
	}

	/// The favorites child-folder list at `parent` (empty is the root list).
	pub fn fav_folder_list_at_mut(&mut self, parent: &[usize]) -> Option<&mut Vec<Folder>> {
		folder_list_in(&mut self.fav_folders, parent)
	}

	/// The favorites station list at `path`: the root list for an empty path.
	pub fn fav_stations_at_mut(&mut self, path: &[usize]) -> Option<&mut Vec<Station>> {
		stations_in(&mut self.favorites, &mut self.fav_folders, path)
	}

	/// Adds an empty favorites folder under `parent`; its full path.
	pub fn add_fav_folder(&mut self, parent: &[usize], name: &str) -> Option<Vec<usize>> {
		add_folder_in(&mut self.fav_folders, parent, name)
	}

	/// Removes the favorites folder at `path` and everything inside it.
	pub fn remove_fav_folder(&mut self, path: &[usize]) -> bool {
		remove_folder_in(&mut self.fav_folders, path)
	}

	pub fn rename_fav_folder(&mut self, path: &[usize], name: &str) -> bool {
		match self.fav_folder_at_mut(path) {
			Some(f) => {
				f.name = name.to_string();
				true
			}
			None => false,
		}
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
			for f in &mut c.folders {
				f.remove_id(id);
			}
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
			folders: Vec::new(),
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

	/// Custom Stations changes only through `add_custom` and `delete_custom`: anything else
	/// would put a non-custom station there or drop one with no other copy.
	fn editable_mut(&mut self, id: &str) -> Option<&mut Collection> {
		if id == CUSTOM {
			return None;
		}
		self.collection_mut(id)
	}

	/// True when the station is now in the collection (added at the root). False if
	/// removed (from the root or any folder), no such collection, or Custom Stations.
	pub fn toggle_in_collection(&mut self, id: &str, s: &Station) -> bool {
		let Some(c) = self.editable_mut(id) else {
			return false;
		};
		if c.remove_id(&s.id) {
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
			c.remove_id(station_id);
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

	/// Count added; stations already in `to` stay as they are. Never into Custom Stations.
	/// Copies the source's stations flat into the target's root; folders are not copied.
	pub fn add_all_to_collection(&mut self, from: &str, to: &str) -> usize {
		let Some(src) = self.collections.iter().find(|c| c.id == from) else {
			return 0;
		};
		let src = src.all_stations();
		let Some(dst) = self.editable_mut(to) else {
			return 0;
		};
		let mut added = 0;
		for s in src {
			if !dst.all_stations().iter().any(|x| x.id == s.id) {
				dst.stations.push(s);
				added += 1;
			}
		}
		added
	}

	pub fn collections_containing(&self, station_id: &str) -> Vec<String> {
		self.collections
			.iter()
			.filter(|c| c.all_stations().iter().any(|s| s.id == station_id))
			.map(|c| c.id.clone())
			.collect()
	}
}
