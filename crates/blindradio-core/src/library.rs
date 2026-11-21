use std::collections::{HashMap, HashSet};
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
	/// The Browse folder this list was filed from, when a directory's folder
	/// was pasted as a collection of its own.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub origin: Option<String>,
	/// Show the playlist as station rows in its saved source order.
	#[serde(default)]
	pub flat_view: Option<bool>,
	/// Station IDs in import order, independent of their folder positions.
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub playlist_order: Vec<String>,
}

/// The same stream, whichever source named it: ids follow the source
/// (`rb:…`, `tunein:…`, `net:…`), so two listings of one stream carry
/// two ids, and only the URL says they are one station. An empty URL
/// never matches by URL: it says nothing about the stream.
/// ponytail: exact URL match — the same stream under http vs https, or
/// behind a playlist address, counts as another station; address
/// normalization is the upgrade path.
fn same_stream(a: &Station, b: &Station) -> bool {
	(a.id == b.id && !positional(a)) || (!a.url.is_empty() && a.url == b.url)
}

/// Stations saved from playlist-file directories by older versions carry their row
/// (`brdir:UK:3`): an edit upstream shifts the rows, so there an id names no stream.
fn positional(s: &Station) -> bool {
	matches!(s.source, Source::Directory | Source::Recommended)
}

/// A named folder inside a collection or the favorites: its own stations at
/// its root, plus nested folders. Order is the user's order everywhere.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Folder {
	pub name: String,
	pub stations: Vec<Station>,
	pub folders: Vec<Folder>,
	/// The Browse folder this one was filed from (its browse id), so
	/// Update from Directory can read it again. None for a folder made by hand.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub origin: Option<String>,
}

/// What `Folder::refresh` changed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Refreshed {
	pub added: usize,
	/// Stations whose stream address the directory changed.
	pub updated: usize,
}

impl Refreshed {
	pub fn any(&self) -> bool {
		self.added + self.updated > 0
	}

	pub fn plus(self, o: Refreshed) -> Refreshed {
		Refreshed {
			added: self.added + o.added,
			updated: self.updated + o.updated,
		}
	}
}

/// A directory's stations, indexed: a whole directory runs to thousands of
/// rows, and an update lands on the UI thread.
struct Listing<'a> {
	theirs: &'a [Station],
	urls: HashMap<&'a str, Vec<usize>>,
	ids: HashMap<&'a str, usize>,
	names: HashMap<String, Vec<usize>>,
}

impl<'a> Listing<'a> {
	fn new(theirs: &'a [Station]) -> Listing<'a> {
		let mut listing = Listing {
			theirs,
			urls: HashMap::new(),
			ids: HashMap::new(),
			names: HashMap::new(),
		};
		for (i, t) in theirs.iter().enumerate() {
			if !t.url.is_empty() {
				listing.urls.entry(&t.url).or_default().push(i);
			}
			listing.ids.entry(&t.id).or_insert(i);
			listing.names.entry(t.name.to_lowercase()).or_default().push(i);
		}
		listing
	}

	/// The listed station `mine` is: by address, else by id, else (row-numbered
	/// ids) by name. An address another saved station already has is no new
	/// address for this one.
	fn counterpart(&self, mine: &Station, held: &HashSet<String>) -> Option<usize> {
		if let Some(all) = self.urls.get(mine.url.as_str()) {
			return all.first().copied();
		}
		let free = |i: &usize| !held.contains(&self.theirs[*i].url);
		if positional(mine) {
			self.names
				.get(&mine.name.to_lowercase())?
				.iter()
				.copied()
				.find(|i| self.theirs[*i].source == mine.source && free(i))
		} else {
			self.ids.get(mine.id.as_str()).copied().filter(free)
		}
	}
}

impl Folder {
	pub fn new(name: &str) -> Folder {
		Folder {
			name: name.into(),
			..Folder::default()
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

	/// Takes in `other`'s stations and folders: a folder of the same name (any case)
	/// merges, a station already on its level stays once. How many stations arrived.
	pub fn merge(&mut self, other: Folder) -> usize {
		let mut added = 0;
		if other.origin.is_some() {
			self.origin = other.origin;
		}
		for s in other.stations {
			if !self.stations.iter().any(|x| same_stream(x, &s)) {
				self.stations.push(s);
				added += 1;
			}
		}
		for f in other.folders {
			added += self.child_mut(&f.name).merge(f);
		}
		added
	}

	pub fn contains_id(&self, id: &str) -> bool {
		self.stations.iter().any(|s| s.id == id)
			|| self.folders.iter().any(|f| f.contains_id(id))
	}

	pub fn contains_url(&self, url: &str) -> bool {
		self.stations.iter().any(|s| s.url == url)
			|| self.folders.iter().any(|f| f.contains_url(url))
	}

	/// This station, or another source's listing of the same stream,
	/// anywhere below here.
	pub fn contains_station(&self, s: &Station) -> bool {
		self.stations.iter().any(|x| same_stream(x, s))
			|| self.folders.iter().any(|f| f.contains_station(s))
	}

	/// Removes the station with this id wherever it sits below here.
	pub fn remove_id(&mut self, id: &str) -> bool {
		let before = self.stations.len();
		self.stations.retain(|s| s.id != id);
		let mut hit = self.stations.len() != before;
		for f in &mut self.folders {
			hit |= f.remove_id(id);
		}
		hit
	}

	/// The folder at the end of `path` (indices into nested `folders`), if it exists.
	pub fn at_mut(&mut self, path: &[usize]) -> Option<&mut Folder> {
		match path.split_first() {
			None => Some(self),
			Some((&i, rest)) => self.folders.get_mut(i)?.at_mut(rest),
		}
	}

	/// Brings this folder level with `upstream`, the directory's folder as it
	/// stands now: stations it lists that are missing arrive, in the same
	/// subfolders, and a station whose address it changed follows. Nothing
	/// leaves, and names and ids stay as the library has them.
	/// ponytail: a station the user deleted comes back, nothing records the
	/// deletion; keep a list of dismissed addresses if that annoys anyone.
	pub fn refresh(&mut self, upstream: &Folder) -> Refreshed {
		let theirs = upstream.all_stations();
		let listing = Listing::new(&theirs);
		let mut held: HashSet<String> = self.all_stations().into_iter().map(|s| s.url).collect();
		let mut found = vec![false; theirs.len()];
		let mut out = Refreshed::default();
		self.reconcile(&listing, &mut found, &mut held, &mut out);
		let taken: HashSet<String> = self.all_stations().into_iter().map(|s| s.id).collect();
		out.added = self.fill(upstream, &found, &mut 0, &taken);
		self.link_folders(upstream);
		out
	}

	fn reconcile(
		&mut self,
		listing: &Listing,
		found: &mut [bool],
		held: &mut HashSet<String>,
		out: &mut Refreshed,
	) {
		for mine in &mut self.stations {
			let Some(i) = listing.counterpart(mine, held) else {
				continue;
			};
			let theirs = &listing.theirs[i];
			found[i] = true;
			// One stream can sit in two playlists: every listing of it is held.
			for &j in listing.urls.get(theirs.url.as_str()).into_iter().flatten() {
				found[j] = true;
			}
			if mine.url != theirs.url {
				mine.url.clone_from(&theirs.url);
				held.insert(theirs.url.clone());
				out.updated += 1;
			}
		}
		for f in &mut self.folders {
			f.reconcile(listing, found, held, out);
		}
	}

	/// Subfolders learn where they came from, so each can update alone.
	fn link_folders(&mut self, upstream: &Folder) {
		for theirs in &upstream.folders {
			let lower = theirs.name.to_lowercase();
			if let Some(mine) = self.folders.iter_mut().find(|f| f.name.to_lowercase() == lower) {
				if mine.origin.is_none() {
					mine.origin.clone_from(&theirs.origin);
				}
				mine.link_folders(theirs);
			}
		}
	}

	/// Files what `found` marks missing, walking `upstream` in the order
	/// `all_stations` lists it. A row-numbered id another station here already
	/// wears is made unique, so one id never names two stations.
	fn fill(
		&mut self,
		upstream: &Folder,
		found: &[bool],
		next: &mut usize,
		taken: &HashSet<String>,
	) -> usize {
		let mut added = 0;
		for s in &upstream.stations {
			if !found[*next] {
				let mut s = s.clone();
				if positional(&s) && taken.contains(&s.id) {
					s.id = format!("{}:{}", s.id, uuid::Uuid::new_v4().simple());
				}
				self.stations.push(s);
				added += 1;
			}
			*next += 1;
		}
		for f in &upstream.folders {
			let n = f.all_stations().len();
			if found[*next..*next + n].iter().all(|&x| x) {
				*next += n;
			} else {
				added += self.child_mut(&f.name).fill(f, found, next, taken);
			}
		}
		added
	}

	/// The Browse folder this one was filed from, when it says so itself or its
	/// stations do: a folder of the BlindRadio Directory or Recommended Radio
	/// Streams filed before folders remembered their origin has stations
	/// numbered `brdir:<playlist>:<row>`, and wears the playlist's name.
	/// Stations the user added from elsewhere say nothing either way.
	pub fn inferred_origin(&self) -> Option<String> {
		if self.origin.is_some() {
			return self.origin.clone();
		}
		let mut file: Option<(String, String)> = None;
		for s in self.all_stations() {
			let Some((root, rest)) = s.id.split_once(':') else {
				continue;
			};
			if root != "brdir" && root != "recommended" {
				continue;
			}
			let Some((stem, _row)) = rest.split_once(':') else {
				continue;
			};
			match &file {
				None => file = Some((root.into(), stem.into())),
				Some((r, f)) if r == root && f == stem => {}
				Some(_) => return None,
			}
		}
		let (root, stem) = file?;
		let plain = |s: &str| s.replace('-', " ").trim().to_lowercase();
		(plain(&self.name) == plain(&stem))
			.then(|| format!("{root}/{}", crate::sources::directory::enc(&stem)))
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

	/// Grouped station indices in playlist order. Stations added by hand follow the imported rows.
	pub fn flat_indices(&self, stations: &[Station]) -> Vec<usize> {
		let positions: HashMap<_, _> = self.playlist_order.iter().enumerate()
			.map(|(index, id)| (id.as_str(), index))
			.collect();
		let mut indices: Vec<_> = (0..stations.len()).collect();
		indices.sort_by_key(|&index| positions.get(stations[index].id.as_str()).copied().unwrap_or(usize::MAX));
		indices
	}

	/// The real folder and local index for a station in depth-first library order.
	pub fn station_location(&self, index: usize) -> Option<(Vec<usize>, usize)> {
		fn locate(
			mut index: usize, stations: &[Station], folders: &[Folder], path: &mut Vec<usize>,
		) -> std::result::Result<usize, usize> {
			if index < stations.len() {
				return Ok(index);
			}
			index -= stations.len();
			for (i, folder) in folders.iter().enumerate() {
				path.push(i);
				match locate(index, &folder.stations, &folder.folders, path) {
					Ok(local) => return Ok(local),
					Err(left) => index = left,
				}
				path.pop();
			}
			Err(index)
		}
		let mut path = Vec::new();
		let local = locate(index, &self.stations, &self.folders, &mut path).ok()?;
		Some((path, local))
	}

	pub fn contains_url(&self, url: &str) -> bool {
		self.stations.iter().any(|s| s.url == url)
			|| self.folders.iter().any(|f| f.contains_url(url))
	}

	/// This station, or another source's listing of the same stream,
	/// at the root or in any folder.
	pub fn contains_station(&self, s: &Station) -> bool {
		self.stations.iter().any(|x| same_stream(x, s))
			|| self.folders.iter().any(|f| f.contains_station(s))
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
		let mut hit = self.stations.len() != before;
		for f in &mut self.folders {
			hit |= f.remove_id(id);
		}
		hit
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

	/// Moves the folder at `path` one step among its siblings; its new path.
	pub fn move_folder(&mut self, path: &[usize], up: bool) -> Option<Vec<usize>> {
		move_folder_in(&mut self.folders, path, up)
	}

	/// Moves the station at `index` in the station list at `path` one
	/// step among its fellow stations; its new index. None at an edge.
	pub fn move_station(&mut self, path: &[usize], index: usize, up: bool) -> Option<usize> {
		move_station_in(&mut self.stations, &mut self.folders, path, index, up)
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
	folders: &'a mut [Folder],
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

/// Moves the folder at `path` one step among its siblings; its new path,
/// or None when it is already at that end (or the path is wrong).
pub fn move_folder_in(folders: &mut Vec<Folder>, path: &[usize], up: bool) -> Option<Vec<usize>> {
	let (&last, parent) = path.split_last()?;
	let list = folder_list_in(folders, parent)?;
	let next = if up {
		last.checked_sub(1)?
	} else {
		last + 1
	};
	if next >= list.len() {
		return None;
	}
	list.swap(last, next);
	let mut new_path = parent.to_vec();
	new_path.push(next);
	Some(new_path)
}

/// Moves the station at `index` in the station list at `path` (under a
/// root list and its folders) one step; its new index. None when the
/// index or the step lands outside the list.
pub fn move_station_in(
	root: &mut Vec<Station>,
	folders: &mut [Folder],
	path: &[usize],
	index: usize,
	up: bool,
) -> Option<usize> {
	let list = stations_in(root, folders, path)?;
	if index >= list.len() {
		return None;
	}
	let next = if up { index.checked_sub(1)? } else { index + 1 };
	if next >= list.len() {
		return None;
	}
	list.swap(index, next);
	Some(next)
}

/// Every folder under `folders` with its path, parents first, names joined
/// with " / " (the pickers' display form).
pub fn folder_paths(folders: &[Folder]) -> Vec<(Vec<usize>, String)> {
	fn walk(
		folders: &[Folder],
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
	walk(folders, &[], "", &mut out);
	out
}

/// A place a station can be filed from a picker: the favorites (root or a
/// folder) or a collection (root or a folder).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StationDest {
	Favorites(Vec<usize>),
	Collection { id: String, path: Vec<usize> },
}

/// Where a pasted folder lands: inside a list (its folders), or as a
/// collection of its own, after the collection `after` (else last).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FolderTarget {
	Into(StationDest),
	NewCollection { after: Option<String> },
}

/// Why a folder paste did nothing.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum FolderPasteError {
	#[error("Nowhere to paste here")]
	Gone,
	#[error("A folder cannot go inside itself")]
	IntoItself,
	#[error("It is already here")]
	AlreadyThere,
	#[error("The cut folder changed. Cut it again")]
	Changed,
	#[error("Custom Stations is fixed")]
	CustomFixed,
}

/// Both places sit in the same folder tree: the favorites, or one collection.
fn same_tree(a: &StationDest, b: &StationDest) -> bool {
	match (a, b) {
		(StationDest::Favorites(_), StationDest::Favorites(_)) => true,
		(StationDest::Collection { id: a, .. }, StationDest::Collection { id: b, .. }) => a == b,
		_ => false,
	}
}

fn dest_path(d: &StationDest) -> &[usize] {
	match d {
		StationDest::Favorites(path) | StationDest::Collection { path, .. } => path,
	}
}

/// `path` once the folder at `removed` has left the same tree: a later
/// sibling on the way down slides up one.
pub fn path_after_removal(path: &[usize], removed: &[usize]) -> Vec<usize> {
	let mut path = path.to_vec();
	if let Some((&last, parent)) = removed.split_last()
		&& path.len() > parent.len()
		&& path[..parent.len()] == *parent
		&& path[parent.len()] > last
	{
		path[parent.len()] -= 1;
	}
	path
}

/// Where a held station lives (see `Library::station_home`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Home {
	Collection { id: String, path: Vec<usize> },
	Favorite { path: Vec<usize> },
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
		// Deleted by the user, it stays gone until a custom station is added again.
		if lib.has_custom() {
			lib.ensure_custom();
		}
		lib
	}

	pub fn save(&self, dir: &Path) -> Result<()> {
		store::save(&dir.join(store::LIBRARY_FILE), self)
	}

	/// Whether Custom Stations is there: the user may delete it.
	pub fn has_custom(&self) -> bool {
		self.collections.iter().any(|c| c.id == CUSTOM)
	}

	/// Where other collections start: after Custom Stations, which stays first.
	fn first_movable(&self) -> usize {
		usize::from(self.collections.first().is_some_and(|c| c.id == CUSTOM))
	}

	/// Custom Stations present and first: made again when something goes into it.
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
					flat_view: None,
					playlist_order: Vec::new(),
					name: CUSTOM_NAME.into(),
					stations: Vec::new(),
					folders: Vec::new(),
					origin: None,
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

	/// This station is a favorite, or another source's listing of the
	/// same stream is: favorites hold a stream once, however it was found.
	pub fn is_favorite_station(&self, s: &Station) -> bool {
		self.favorites.iter().any(|x| same_stream(x, s))
			|| self.fav_folders.iter().any(|f| f.contains_station(s))
	}

	/// True when the library holds a copy of the station (so it can be edited).
	pub fn holds_station(&self, id: &str) -> bool {
		self.is_favorite(id) || self.collections.iter().any(|c| c.contains_id(id))
	}

	/// Renames and re-points every held copy of a station: collections and
	/// their folders, favorites and their folders. True when any copy changed.
	/// Where a held station lives: the first collection (and folder path
	/// within it) holding it, or the favorites folder holding it. The
	/// clipboard's cut and paste resolve their endpoints through this.
	pub fn station_home(&self, id: &str) -> Option<Home> {
		fn find(
			stations: &[Station],
			folders: &[Folder],
			path: &[usize],
			id: &str,
		) -> Option<Vec<usize>> {
			if stations.iter().any(|s| s.id == id) {
				return Some(path.to_vec());
			}
			for (i, f) in folders.iter().enumerate() {
				let mut p = path.to_vec();
				p.push(i);
				if let Some(found) = find(&f.stations, &f.folders, &p, id) {
					return Some(found);
				}
			}
			None
		}
		for c in &self.collections {
			if let Some(path) = find(&c.stations, &c.folders, &[], id) {
				return Some(Home::Collection {
					id: c.id.clone(),
					path,
				});
			}
		}
		if let Some(path) = find(&self.favorites, &self.fav_folders, &[], id) {
			return Some(Home::Favorite { path });
		}
		None
	}

	pub fn update_station(&mut self, id: &str, name: &str, url: &str, info: Option<&str>) -> bool {
		fn walk(
			stations: &mut [Station],
			folders: &mut [Folder],
			id: &str,
			name: &str,
			url: &str,
			info: Option<&str>,
		) -> bool {
			let mut hit = false;
			for s in stations.iter_mut().filter(|s| s.id == id) {
				s.name = name.to_string();
				s.url = url.to_string();
				s.info = info.map(str::to_string);
				hit = true;
			}
			for f in folders {
				hit |= walk(&mut f.stations, &mut f.folders, id, name, url, info);
			}
			hit
		}
		let mut hit = walk(
			&mut self.favorites,
			&mut self.fav_folders,
			id,
			name,
			url,
			info,
		);
		hit |= walk(&mut self.recent, &mut [], id, name, url, info);
		for c in &mut self.collections {
			hit |= walk(&mut c.stations, &mut c.folders, id, name, url, info);
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
		if self.is_favorite_station(s) {
			// Out by the held copies' own ids: the station being
			// toggled may be another source's listing of the stream.
			let held: Vec<String> = self
				.all_favorites()
				.iter()
				.filter(|x| same_stream(x, s))
				.map(|x| x.id.clone())
				.collect();
			for id in held {
				self.remove_favorite(&id);
			}
			false
		} else {
			self.favorites.push(s.clone());
			true
		}
	}

	/// Adds the station to the favorites folder at `path` (empty is the root).
	/// False when its stream is already a favorite anywhere.
	pub fn add_favorite_to(&mut self, path: &[usize], s: &Station) -> bool {
		if self.is_favorite_station(s) {
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

	/// Moves the favorites folder at `path` one step among its siblings.
	pub fn move_fav_folder(&mut self, path: &[usize], up: bool) -> Option<Vec<usize>> {
		move_folder_in(&mut self.fav_folders, path, up)
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
			details: Default::default(),
		};
		self.ensure_custom();
		self.collections[0].stations.push(s.clone());
		s
	}

	/// A new custom station filed straight into `dest` (the Add Custom
	/// Station dialog's picker). Falls back to Custom Stations when the
	/// destination no longer exists.
	pub fn add_custom_to(&mut self, name: &str, url: &str, dest: &StationDest) -> Station {
		let s = Station {
			id: format!("custom:{}", uuid::Uuid::new_v4()),
			name: name.into(),
			url: url.into(),
			source: Source::Custom,
			info: None,
			details: Default::default(),
		};
		let filed = match dest {
			StationDest::Favorites(path) => match self.fav_stations_at_mut(path) {
				Some(list) => {
					list.push(s.clone());
					true
				}
				None => false,
			},
			StationDest::Collection { id, path } => {
				if id == CUSTOM {
					self.ensure_custom();
				}
				match self
					.collection_mut(id)
					.and_then(|c| c.stations_at_mut(path))
				{
					Some(list) => {
						list.push(s.clone());
						true
					}
					None => false,
				}
			}
		};
		if !filed {
			self.ensure_custom();
			self.collections[0].stations.push(s.clone());
		}
		s
	}

	/// Every destination the Add Custom Station picker offers, in order:
	/// Custom Stations, each collection and its folders, Favorites and its
	/// folders. The first entry is where new stations went before the picker.
	pub fn destinations(&self) -> Vec<(String, StationDest)> {
		let mut out = vec![(
			"Custom Stations".to_string(),
			StationDest::Collection {
				id: CUSTOM.into(),
				path: Vec::new(),
			},
		)];
		for c in &self.collections {
			if c.id != CUSTOM {
				out.push((
					c.name.clone(),
					StationDest::Collection {
						id: c.id.clone(),
						path: Vec::new(),
					},
				));
			}
			for (path, label) in folder_paths(&c.folders) {
				out.push((
					format!("{} / {}", c.name, label),
					StationDest::Collection {
						id: c.id.clone(),
						path,
					},
				));
			}
		}
		out.push(("Favorites".to_string(), StationDest::Favorites(Vec::new())));
		for (path, label) in folder_paths(&self.fav_folders) {
			out.push((
				format!("Favorites / {label}"),
				StationDest::Favorites(path),
			));
		}
		out
	}

	/// Files a held station into one folder of a collection. False when the
	/// collection already holds the station anywhere, or the folder is gone.
	pub fn add_to_collection_folder(
		&mut self,
		id: &str,
		path: &[usize],
		s: &Station,
	) -> bool {
		let Some(c) = self.collection_mut(id) else {
			return false;
		};
		if c.contains_station(s) {
			return false;
		}
		match c.stations_at_mut(path) {
			Some(list) => {
				list.push(s.clone());
				true
			}
			None => false,
		}
	}

	/// Files a whole folder inside `dest`, merged into a folder of the same name there.
	/// How many stations arrived; None when `dest` is gone.
	pub fn add_folder_to(&mut self, dest: &StationDest, folder: Folder) -> Option<usize> {
		let list = match dest {
			StationDest::Favorites(path) => self.fav_folder_list_at_mut(path)?,
			StationDest::Collection { id, path } => {
				self.collection_mut(id)?.folder_list_at_mut(path)?
			}
		};
		let lower = folder.name.to_lowercase();
		let at = match list.iter().position(|f| f.name.to_lowercase() == lower) {
			Some(i) => i,
			None => {
				list.push(Folder::new(&folder.name));
				list.len() - 1
			}
		};
		Some(list[at].merge(folder))
	}

	/// Every folder of every collection, Custom Stations included, for
	/// the station menu: collection id, folder path, "Collection /
	/// Folder" label. Custom Stations was skipped once, which locked
	/// its folders — the imported playlists live there as folders — out
	/// of the Add to Collection Folder menu entirely (Brandon's
	/// report); the Add Custom Station picker never skipped them.
	pub fn collection_folder_list(&self) -> Vec<(String, Vec<usize>, String)> {
		let mut out = Vec::new();
		for c in &self.collections {
			for (path, label) in folder_paths(&c.folders) {
				out.push((c.id.clone(), path, format!("{} / {}", c.name, label)));
			}
		}
		out
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
			flat_view: None,
			playlist_order: Vec::new(),
			name: name.into(),
			stations: Vec::new(),
			folders: Vec::new(),
			origin: None,
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

	/// Custom Stations too: it comes back when a custom station is added.
	pub fn delete_collection(&mut self, id: &str) -> std::result::Result<(), LibraryError> {
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
		// Out by the held copy's own id: the station being toggled may
		// be another source's listing of the same stream.
		let held = c
			.all_stations()
			.iter()
			.find(|x| same_stream(x, s))
			.map(|x| x.id.clone());
		match held {
			Some(held) => {
				c.remove_id(&held);
				false
			}
			None => {
				c.stations.push(s.clone());
				true
			}
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
	/// True when the collection moved.
	pub fn move_collection(&mut self, id: &str, up: bool) -> bool {
		if id == CUSTOM {
			return false;
		}
		let first = self.first_movable();
		match self.collections.iter().position(|c| c.id == id) {
			Some(i) => step(&mut self.collections[first..], i - first, up) != i - first,
			None => false,
		}
	}

	/// Renames the folder at `path` of a collection, or the collection itself
	/// for an empty path. False for Custom Stations itself, or a wrong place.
	pub fn rename_at(&mut self, id: &str, path: &[usize], name: &str) -> bool {
		if path.is_empty() {
			return self.rename_collection(id, name).is_ok();
		}
		self.collection_mut(id)
			.is_some_and(|c| c.rename_folder(path, name))
	}

	/// Removes the folder at `path` of a collection, or the collection itself
	/// for an empty path. False for a wrong place.
	pub fn remove_at(&mut self, id: &str, path: &[usize]) -> bool {
		if path.is_empty() {
			return self.delete_collection(id).is_ok();
		}
		self.collection_mut(id)
			.is_some_and(|c| c.remove_folder(path))
	}

	/// Moves the folder at `path` of a collection one step among its siblings,
	/// or the collection itself for an empty path; its new path. None at an edge.
	pub fn move_at(&mut self, id: &str, path: &[usize], up: bool) -> Option<Vec<usize>> {
		if path.is_empty() {
			return self.move_collection(id, up).then(Vec::new);
		}
		self.collection_mut(id)?.move_folder(path, up)
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
			if !dst.all_stations().iter().any(|x| same_stream(x, &s)) {
				dst.stations.push(s);
				added += 1;
			}
		}
		added
	}

	/// The folder at `at` as it stands, a whole collection for an empty
	/// collection path. None for the favorites root, or a wrong place.
	pub fn folder_snapshot(&self, at: &StationDest) -> Option<Folder> {
		match at {
			StationDest::Favorites(path) => self.fav_folder_at(path).cloned(),
			StationDest::Collection { id, path } => {
				let c = self.collections.iter().find(|c| c.id == *id)?;
				if path.is_empty() {
					return Some(Folder {
						name: c.name.clone(),
						stations: c.stations.clone(),
						folders: c.folders.clone(),
						origin: c.origin.clone(),
					});
				}
				c.folder_at(path).cloned()
			}
		}
	}

	/// Where the folder or collection at `at` was filed from, for Update from
	/// Directory.
	pub fn origin_at(&self, at: &StationDest) -> Option<String> {
		let folder = self.folder_snapshot(at)?;
		// A collection that only holds folders is not the directory's: they are.
		let whole = matches!(at, StationDest::Collection { path, .. } if path.is_empty());
		if whole && folder.origin.is_none() && folder.stations.is_empty() {
			return None;
		}
		folder.inferred_origin()
	}

	/// Every folder that can be updated from its directory, outermost only:
	/// an updated folder brings the folders inside it along.
	pub fn linked_folders(&self) -> Vec<(StationDest, String)> {
		fn walk(
			folders: &[Folder],
			base: &[usize],
			dest: &dyn Fn(Vec<usize>) -> StationDest,
			out: &mut Vec<(StationDest, String)>,
		) {
			for (i, f) in folders.iter().enumerate() {
				let mut path = base.to_vec();
				path.push(i);
				match f.inferred_origin() {
					Some(origin) => out.push((dest(path), origin)),
					None => walk(&f.folders, &path, dest, out),
				}
			}
		}
		let mut out = Vec::new();
		walk(&self.fav_folders, &[], &StationDest::Favorites, &mut out);
		for c in &self.collections {
			let id = c.id.clone();
			let whole = StationDest::Collection {
				id: id.clone(),
				path: Vec::new(),
			};
			if let Some(origin) = self.origin_at(&whole) {
				out.push((whole, origin));
				continue;
			}
			walk(
				&c.folders,
				&[],
				&move |path| StationDest::Collection {
					id: id.clone(),
					path,
				},
				&mut out,
			);
		}
		out
	}

	/// Brings the folder at `at` level with `upstream`, read from `origin`,
	/// and keeps `origin` on it from now on. The read takes a while and folders
	/// move meanwhile, so a place that no longer holds `origin` is looked up
	/// again: the stations never land in a neighbor. None when the folder is gone.
	pub fn refresh_folder(
		&mut self,
		at: &StationDest,
		origin: &str,
		upstream: &Folder,
	) -> Option<Refreshed> {
		let at = if self.origin_at(at).as_deref() == Some(origin) {
			at.clone()
		} else {
			self.linked_folders().into_iter().find(|(_, o)| o == origin)?.0
		};
		let folder = match &at {
			StationDest::Favorites(path) => self.fav_folder_at_mut(path)?,
			StationDest::Collection { id, path } if path.is_empty() => {
				let c = self.collection_mut(id)?;
				let mut whole = Folder {
					name: c.name.clone(),
					stations: std::mem::take(&mut c.stations),
					folders: std::mem::take(&mut c.folders),
					origin: Some(origin.to_owned()),
				};
				let got = whole.refresh(upstream);
				c.stations = whole.stations;
				c.folders = whole.folders;
				c.origin = whole.origin;
				return Some(got);
			}
			StationDest::Collection { id, path } => {
				self.collection_mut(id)?.folder_at_mut(path)?
			}
		};
		folder.origin = Some(origin.to_owned());
		Some(folder.refresh(upstream))
	}

	/// Whether the list at `d` exists: a collection or favorites root, or a folder.
	fn list_exists(&self, d: &StationDest) -> bool {
		match d {
			StationDest::Favorites(path) => self.fav_stations_at(path).is_some(),
			StationDest::Collection { id, path } => self
				.collections
				.iter()
				.find(|c| c.id == *id)
				.is_some_and(|c| c.stations_at(path).is_some()),
		}
	}

	/// Files `folder` as a collection of its own, placed after the
	/// collection `after` (else last). A collection of the same name
	/// (any case) takes it in instead. The collection's id.
	pub fn add_folder_as_collection(&mut self, folder: Folder, after: Option<&str>) -> String {
		let lower = folder.name.to_lowercase();
		if let Some(c) = self
			.collections
			.iter_mut()
			.find(|c| c.name.to_lowercase() == lower)
		{
			let mut whole = Folder {
				name: c.name.clone(),
				stations: std::mem::take(&mut c.stations),
				folders: std::mem::take(&mut c.folders),
				origin: c.origin.take(),
			};
			whole.merge(folder);
			c.stations = whole.stations;
			c.folders = whole.folders;
			c.origin = whole.origin;
			return c.id.clone();
		}
		let id = format!("col:{}", uuid::Uuid::new_v4());
		let at = after
			.and_then(|a| self.collections.iter().position(|c| c.id == a))
			.map_or(self.collections.len(), |i| i + 1)
			// Custom Stations stays first.
			.max(self.first_movable());
		self.collections.insert(
			at.min(self.collections.len()),
			Collection {
				id: id.clone(),
				flat_view: None,
				playlist_order: Vec::new(),
				name: folder.name,
				stations: folder.stations,
				folders: folder.folders,
				origin: folder.origin,
			},
		);
		id
	}

	/// Pastes the folder copied or cut from `from` at `to`, subfolders and
	/// stations with it. A copy files the copy taken (`folder`). A cut moves
	/// the folder: it must still stand at `from` exactly as it was taken,
	/// since its place is all there is to take it out by. A whole
	/// collection cut into a list becomes a folder there, and a folder
	/// pasted as a collection leaves its old list. Folders of the same name
	/// merge, as filing a folder always does. Where the folder now is.
	pub fn paste_folder(
		&mut self,
		folder: Folder,
		from: &StationDest,
		cut: bool,
		to: &FolderTarget,
	) -> std::result::Result<StationDest, FolderPasteError> {
		use FolderPasteError as E;
		let from_path = dest_path(from);
		match to {
			// A collection is a collection already.
			FolderTarget::NewCollection { .. } if from_path.is_empty() => {
				return Err(E::AlreadyThere);
			}
			FolderTarget::Into(d) if same_tree(from, d) => {
				let at = dest_path(d);
				if from_path.split_last().map(|(_, parent)| parent) == Some(at) {
					return Err(E::AlreadyThere);
				}
				if cut && at.starts_with(from_path) {
					return Err(E::IntoItself);
				}
			}
			_ => {}
		}
		if let FolderTarget::Into(d) = to
			&& !self.list_exists(d)
		{
			return Err(E::Gone);
		}
		let mut to = to.clone();
		if cut {
			if matches!(from, StationDest::Collection { id, path } if id == CUSTOM && path.is_empty()) {
				return Err(E::CustomFixed);
			}
			if self.folder_snapshot(from).as_ref() != Some(&folder) {
				return Err(E::Changed);
			}
			let removed = match from {
				StationDest::Favorites(path) => self.remove_fav_folder(path),
				StationDest::Collection { id, path } => self.remove_at(id, path),
			};
			if !removed {
				return Err(E::Gone);
			}
			if let FolderTarget::Into(d) = &mut to
				&& same_tree(from, d)
			{
				let moved = path_after_removal(dest_path(d), from_path);
				match d {
					StationDest::Favorites(path) | StationDest::Collection { path, .. } => {
						*path = moved;
					}
				}
			}
		}
		match to {
			FolderTarget::Into(d) => {
				let lower = folder.name.to_lowercase();
				self.add_folder_to(&d, folder);
				// Where it landed: the folder of its name on that level.
				let list = match &d {
					StationDest::Favorites(path) => self.fav_folder_list_at_mut(path),
					StationDest::Collection { id, path } => self
						.collection_mut(id)
						.and_then(|c| c.folder_list_at_mut(path)),
				};
				let at = list
					.and_then(|l| l.iter().position(|f| f.name.to_lowercase() == lower))
					.unwrap_or(0);
				Ok(match d {
					StationDest::Favorites(mut path) => {
						path.push(at);
						StationDest::Favorites(path)
					}
					StationDest::Collection { id, mut path } => {
						path.push(at);
						StationDest::Collection { id, path }
					}
				})
			}
			FolderTarget::NewCollection { after } => Ok(StationDest::Collection {
				id: self.add_folder_as_collection(folder, after.as_deref()),
				path: Vec::new(),
			}),
		}
	}

	pub fn collections_containing(&self, station_id: &str) -> Vec<String> {
		self.collections
			.iter()
			.filter(|c| c.all_stations().iter().any(|s| s.id == station_id))
			.map(|c| c.id.clone())
			.collect()
	}
}
