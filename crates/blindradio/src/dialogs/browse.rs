//! Browse (Ctrl+G): the directories, the collections and Custom Stations in one tree.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use blindradio_core::library::{CUSTOM, Folder, Library, StationDest};
use blindradio_core::sources::{Node, Sort, SortField, Sources};
use blindradio_core::station::Station;
use wx_utils::{confirm, prompt_text};
use wxdragon::event::{Event, EventType, WxEvtHandler};
use wxdragon::prelude::*;

use crate::action::Action;
use crate::app::App;
use crate::bridge::WindowToken;
use crate::context_menu::{self, Choice, Target};
use crate::main_window::dispatch;
use crate::menus::plain;
use crate::station_list::letter_jump;
use crate::strings as s;

mod view;

pub use view::show;

/// Key of the Collections root.
const COLLECTIONS: &str = "collections";

/// A local folder's key is its collection id plus one "f<index>"
/// segment per level down. Splits it back into the collection id and
/// the folder index path the library's move takes. None for a
/// collection row itself (no segments) or a malformed key.
fn local_path(key: &str) -> Option<(String, Vec<usize>)> {
	let (collection, rest) = key.split_once('/')?;
	let mut path = Vec::new();
	for seg in rest.split('/') {
		path.push(seg.strip_prefix('f')?.parse().ok()?);
	}
	Some((collection.to_owned(), path))
}

/// What Browse keeps between openings, for the session: where focus was, and every directory
/// level loaded, so reopening is instant and needs no loader to move focus.
#[derive(Default)]
pub struct Memory {
	path: Vec<String>,
	cache: HashMap<String, Vec<Node>>,
	sort: Sort,
}

impl Memory {
	/// Drops what Browse loaded and where it stood, so folders turned off leave with them.
	pub fn forget(&mut self) {
		self.path.clear();
		self.cache.clear();
	}
}

enum Children {
	Unloaded,
	/// The placeholder child.
	Loading(usize),
	Loaded(Vec<usize>),
}

#[expect(clippy::large_enum_variant)]
enum Kind {
	Folder {
		id: String,
		name: String,
		children: Children,
		expanded: bool,
	},
	Station(Station),
	LoadMore {
		id: String,
		loading: bool,
	},
	/// The placeholder child of a folder whose children are on their way.
	Loading,
}

impl Kind {
	fn from_node(node: Node) -> Kind {
		match node {
			Node::Folder { id, name } => Kind::Folder {
				id,
				name,
				children: Children::Unloaded,
				expanded: false,
			},
			Node::Station(st) => Kind::Station(st),
			Node::LoadMore { id } => Kind::LoadMore { id, loading: false },
		}
	}

	/// A library folder: its children are known.
	fn local(id: &str, name: &str) -> Kind {
		Kind::Folder {
			id: id.into(),
			name: name.into(),
			children: Children::Loaded(Vec::new()),
			expanded: false,
		}
	}

	fn key(&self) -> Option<&str> {
		match self {
			Kind::Folder { id, .. } | Kind::LoadMore { id, .. } => Some(id),
			Kind::Station(st) => Some(&st.id),
			Kind::Loading => None,
		}
	}
}

struct Item {
	parent: Option<usize>,
	kind: Kind,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Delete {
	Remove { collection: String, station: String },
	/// A folder inside one of the local roots, or with an empty path a
	/// whole collection: it goes, with everything in it (the view
	/// confirms first).
	RemoveFolder { collection: String, path: Vec<usize> },
	NotOwn,
}

/// The tree without wx. Items are indices that stay valid while the window is open.
pub struct BrowseModel {
	items: Vec<Item>,
	roots: Vec<usize>,
	focus: Option<usize>,
	cache: HashMap<String, Vec<Node>>,
	/// Branches whose failure was said since their last fetch.
	failed: HashSet<usize>,
	collections: usize,
	custom: usize,
	/// The user's folder order for remote levels and the roots, from
	/// Settings: parent folder id ("" for the roots) to folder ids.
	order: HashMap<String, Vec<String>>,
}

impl BrowseModel {
	pub fn new(memory: Memory, lib: &Library) -> BrowseModel {
		let mut m = BrowseModel {
			items: Vec::new(),
			roots: Vec::new(),
			focus: None,
			cache: memory.cache,
			failed: HashSet::new(),
			collections: 0,
			custom: 0,
			order: HashMap::new(),
		};
		for node in Sources::roots() {
			let i = m.add(None, Kind::from_node(node));
			m.roots.push(i);
		}
		m.collections = m.add(None, Kind::local(COLLECTIONS, &s::collections_folder()));
		let custom = lib
			.collections
			.iter()
			.find(|c| c.id == CUSTOM)
			.map(|c| c.name.clone())
			.unwrap_or_default();
		m.custom = m.add(None, Kind::local(CUSTOM, &custom));
		m.roots.extend([m.collections, m.custom]);
		m.refresh_local(lib);
		m.restore(&memory.path);
		m
	}

	/// The settings key a level's order is stored under: the parent
	/// folder's id, "" for the roots.
	fn order_key(&self, parent: Option<usize>) -> String {
		parent
			.and_then(|p| self.key(p))
			.unwrap_or_default()
			.to_owned()
	}

	/// Seeds the user's saved folder order (Settings.browse_order) and
	/// puts the roots in it. Remote levels pick it up as they load.
	pub fn set_order(&mut self, order: HashMap<String, Vec<String>>) {
		self.order = order;
		let mut roots = self.roots.clone();
		self.apply_order(None, &mut roots);
		self.roots = roots;
	}

	/// Puts a level's folders in the user's saved order: folders named
	/// in the list take the folder slots in that order, folders it does
	/// not name keep their source order after them, and every other row
	/// keeps its slot.
	fn apply_order(&self, parent: Option<usize>, kids: &mut [usize]) {
		let key = self.order_key(parent);
		let Some(want) = self.order.get(&key) else {
			return;
		};
		if want.is_empty() {
			return;
		}
		let slots: Vec<usize> = kids
			.iter()
			.enumerate()
			.filter(|(_, i)| self.is_folder(**i))
			.map(|(n, _)| n)
			.collect();
		let mut folders: Vec<usize> = slots.iter().map(|&n| kids[n]).collect();
		folders.sort_by_key(|&i| {
			let k = self.key(i);
			want.iter()
				.position(|w| Some(w.as_str()) == k)
				.unwrap_or(usize::MAX)
		});
		for (n, f) in slots.into_iter().zip(folders) {
			kids[n] = f;
		}
	}

	/// Moves folder `i` one step among the folder rows on its level;
	/// other rows keep their slots. Records the level's new order and
	/// returns it with its settings key so the caller can persist it.
	/// None when there is no folder sibling that way. Local folders do
	/// not come here: the library itself is reordered for those.
	pub fn move_folder(&mut self, i: usize, up: bool) -> Option<(String, Vec<String>)> {
		if !self.is_folder(i) {
			return None;
		}
		let parent = self.items[i].parent;
		let mut level = self.children(parent).to_vec();
		let folders: Vec<usize> = level
			.iter()
			.copied()
			.filter(|&x| self.is_folder(x))
			.collect();
		let at = folders.iter().position(|&x| x == i)?;
		let next = if up { at.checked_sub(1)? } else { at + 1 };
		let other = *folders.get(next)?;
		let a = level.iter().position(|&x| x == i)?;
		let b = level.iter().position(|&x| x == other)?;
		level.swap(a, b);
		match parent {
			None => self.roots = level.clone(),
			Some(p) => self.set_children(p, level.clone()),
		}
		let order: Vec<String> = level
			.iter()
			.copied()
			.filter(|&x| self.is_folder(x))
			.filter_map(|x| self.key(x).map(str::to_owned))
			.collect();
		let key = self.order_key(parent);
		self.order.insert(key.clone(), order.clone());
		Some((key, order))
	}

	/// Under Collections or Custom Stations: the library owns the order.
	pub fn is_local(&self, i: usize) -> bool {
		let mut at = i;
		while let Some(p) = self.items[at].parent {
			at = p;
		}
		at == self.collections || at == self.custom
	}

	/// Taken when the window closes.
	pub fn memory(&mut self, sort: Sort) -> Memory {
		Memory {
			path: self.path(),
			cache: std::mem::take(&mut self.cache),
			sort,
		}
	}

	/// The sort changed: Radio Browser levels (the only sorted ones) are dropped, cache
	/// first, so the next open fetches them in the new order. Focus inside one moves to
	/// the Radio Browser folder.
	pub fn resort(&mut self) {
		self.cache.retain(|id, _| !id.starts_with("rb"));
		let Some(rb) = self
			.items
			.iter()
			.position(|item| item.kind.key() == Some("rb"))
		else {
			return;
		};
		let mut under = vec![false; self.items.len()];
		for (i, item) in self.items.iter().enumerate() {
			under[i] = i == rb || item.parent.is_some_and(|p| under[p]);
		}
		for (i, item) in self.items.iter_mut().enumerate() {
			if under[i]
				&& let Kind::Folder {
					children, expanded, ..
				} = &mut item.kind
			{
				*children = Children::Unloaded;
				*expanded = false;
			}
		}
		if self.focus.is_some_and(|f| f != rb && under[f]) {
			self.focus = Some(rb);
		}
	}

	fn add(&mut self, parent: Option<usize>, kind: Kind) -> usize {
		self.items.push(Item { parent, kind });
		self.items.len() - 1
	}

	/// The roots when `parent` is `None`.
	pub fn children(&self, parent: Option<usize>) -> &[usize] {
		let Some(p) = parent else {
			return &self.roots;
		};
		match &self.items[p].kind {
			Kind::Folder {
				children: Children::Loaded(v),
				..
			} => v,
			Kind::Folder {
				children: Children::Loading(p),
				..
			} => std::slice::from_ref(p),
			_ => &[],
		}
	}

	fn set_children(&mut self, folder: usize, kids: Vec<usize>) {
		if let Kind::Folder { children, .. } = &mut self.items[folder].kind {
			*children = Children::Loaded(kids);
		}
	}

	pub fn parent(&self, i: usize) -> Option<usize> {
		self.items[i].parent
	}

	fn level(&self, i: usize) -> &[usize] {
		self.children(self.items[i].parent)
	}

	pub fn label(&self, i: usize) -> String {
		match &self.items[i].kind {
			Kind::Folder { name, .. } => name.clone(),
			Kind::Station(st) => st.name.clone(),
			Kind::LoadMore { loading: false, .. } => s::load_more(),
			Kind::LoadMore { loading: true, .. } | Kind::Loading => s::loading(),
		}
	}

	/// The tree text for a row. A local collection's or folder's
	/// name carries how many stations live under it ("English 24",
	/// "Misc 15"): the library's levels are fully built, so the
	/// count is exact. Remote rows keep the source's name alone —
	/// their count is unknowable until every level under them has
	/// loaded. The Collections header itself is a grouping, not a
	/// playlist, and stays bare.
	pub fn display_label(&self, i: usize) -> String {
		if i != self.collections && self.is_folder(i) && self.is_local(i) {
			format!("{} {}", self.label(i), self.stations_under(i).len())
		} else {
			self.label(i).to_owned()
		}
	}

	pub fn key(&self, i: usize) -> Option<&str> {
		self.items[i].kind.key()
	}

	pub fn station(&self, i: usize) -> Option<&Station> {
		match &self.items[i].kind {
			Kind::Station(st) => Some(st),
			_ => None,
		}
	}

	pub fn is_folder(&self, i: usize) -> bool {
		matches!(self.items[i].kind, Kind::Folder { .. })
	}

	pub fn is_load_more(&self, i: usize) -> bool {
		matches!(self.items[i].kind, Kind::LoadMore { .. })
	}

	/// Shows an expand button: not loaded yet, or loaded with children.
	pub fn has_children(&self, i: usize) -> bool {
		match &self.items[i].kind {
			Kind::Folder {
				children: Children::Loaded(v),
				..
			} => !v.is_empty(),
			Kind::Folder { .. } => true,
			_ => false,
		}
	}

	pub fn expanded(&self, i: usize) -> bool {
		matches!(self.items[i].kind, Kind::Folder { expanded: true, .. })
	}

	pub fn focus(&self) -> Option<usize> {
		self.focus
	}

	pub fn set_focus(&mut self, i: usize) {
		self.focus = Some(i);
	}

	/// The branch an item belongs to: the highest item below a tree root, or the
	/// root itself. Failures are announced per branch (Radio Browser, a network).
	fn root_of(&self, mut i: usize) -> usize {
		while let Some(p) = self.items[i].parent {
			if self.roots.contains(&p) {
				break;
			}
			i = p;
		}
		i
	}

	/// The folder is opened. Returns the id to fetch when its children are not known yet.
	pub fn open(&mut self, i: usize) -> Option<String> {
		let Kind::Folder {
			id,
			children,
			expanded,
			..
		} = &mut self.items[i].kind
		else {
			return None;
		};
		*expanded = true;
		if !matches!(children, Children::Unloaded) {
			return None;
		}
		let id = id.clone();
		if let Some(nodes) = self.cache.get(&id).cloned() {
			let kids = self.instantiate(i, nodes);
			self.set_children(i, kids);
			return None;
		}
		let p = self.add(Some(i), Kind::Loading);
		if let Kind::Folder { children, .. } = &mut self.items[i].kind {
			*children = Children::Loading(p);
		}
		let root = self.root_of(i);
		self.failed.remove(&root);
		Some(id)
	}

	pub fn close(&mut self, i: usize) {
		if let Kind::Folder { expanded, .. } = &mut self.items[i].kind {
			*expanded = false;
		}
	}

	/// Enter on Load More. Returns the id to fetch unless that page is on its way already.
	pub fn more(&mut self, i: usize) -> Option<String> {
		let Kind::LoadMore { id, loading } = &mut self.items[i].kind else {
			return None;
		};
		if *loading {
			return None;
		}
		*loading = true;
		let id = id.clone();
		let root = self.root_of(i);
		self.failed.remove(&root);
		Some(id)
	}

	/// A fetch for folder or Load More `i` ended; `None` is a failure. Focus stays where it is:
	/// the item waiting in its place (placeholder or Load More) becomes the first new one.
	/// Returns what to say.
	pub fn loaded(&mut self, i: usize, nodes: Option<Vec<Node>>) -> Option<String> {
		let (id, placeholder) = match &self.items[i].kind {
			Kind::Folder {
				id,
				children: Children::Loading(p),
				..
			} => (id.clone(), Some(*p)),
			Kind::LoadMore { id, loading: true } => (id.clone(), None),
			_ => return None,
		};
		let Some(nodes) = nodes else {
			return self.failed_load(i, placeholder);
		};
		self.cache.insert(id, nodes.clone());
		match placeholder {
			Some(p) => {
				let kids = self.instantiate(i, nodes);
				let kids = self.reuse(p, kids);
				if kids.is_empty() {
					self.close(i);
					if self.focus == Some(p) {
						self.focus = Some(i);
					}
				}
				self.set_children(i, kids);
			}
			None => {
				let parent = self.items[i].parent?;
				let kids = self.instantiate(parent, nodes);
				let kids = self.reuse(i, kids);
				let mut siblings = self.children(Some(parent)).to_vec();
				let at = siblings.iter().position(|&x| x == i)?;
				if kids.is_empty() && self.focus == Some(i) {
					self.focus = Some(at.checked_sub(1).map_or(parent, |k| siblings[k]));
				}
				siblings.splice(at..=at, kids);
				self.set_children(parent, siblings);
			}
		}
		None
	}

	/// The folder is left closed so Right fetches again; Load More stays for Enter.
	fn failed_load(&mut self, i: usize, placeholder: Option<usize>) -> Option<String> {
		match &mut self.items[i].kind {
			Kind::Folder {
				children, expanded, ..
			} => {
				*children = Children::Unloaded;
				*expanded = false;
				if placeholder.is_some() && self.focus == placeholder {
					self.focus = Some(i);
				}
			}
			Kind::LoadMore { loading, .. } => *loading = false,
			_ => {}
		}
		let root = self.root_of(i);
		self.failed
			.insert(root)
			.then(|| format!("{}{}", s::failed(), self.label(root)))
	}

	/// `p` takes the place of the first new item, so focus on it lands there.
	fn reuse(&mut self, p: usize, mut kids: Vec<usize>) -> Vec<usize> {
		if let Some(first) = kids.first_mut() {
			self.items[p].kind = std::mem::replace(&mut self.items[*first].kind, Kind::Loading);
			*first = p;
		}
		kids
	}

	/// New items; a Load More whose page is cached is replaced by that page. A directory's pages
	/// can overlap (TuneIn's do): a station already in the list is not added again.
	fn instantiate(&mut self, parent: usize, nodes: Vec<Node>) -> Vec<usize> {
		let mut stations: HashSet<String> = self
			.children(Some(parent))
			.iter()
			.filter_map(|&i| self.station(i).map(|st| st.id.clone()))
			.collect();
		self.instantiate_page(parent, nodes, &mut stations)
	}

	fn instantiate_page(
		&mut self,
		parent: usize,
		nodes: Vec<Node>,
		stations: &mut HashSet<String>,
	) -> Vec<usize> {
		let mut out = Vec::new();
		for node in nodes {
			match node {
				Node::LoadMore { id } if self.cache.contains_key(&id) => {
					let page = self.cache[&id].clone();
					out.extend(self.instantiate_page(parent, page, stations));
				}
				Node::Station(st) if !stations.insert(st.id.clone()) => {}
				node => out.push(self.add(Some(parent), Kind::from_node(node))),
			}
		}
		self.apply_order(Some(parent), &mut out);
		out
	}

	fn path(&self) -> Vec<String> {
		let mut path = Vec::new();
		let mut at = self.focus;
		while let Some(i) = at {
			if let Some(key) = self.key(i) {
				path.push(key.to_owned());
			}
			at = self.items[i].parent;
		}
		path.reverse();
		path
	}

	/// Reopens the remembered path from what is known; stops at the nearest item that is left.
	fn restore(&mut self, path: &[String]) {
		let mut level = self.roots.clone();
		for (n, key) in path.iter().enumerate() {
			let Some(i) = level
				.iter()
				.copied()
				.find(|&i| self.key(i) == Some(key.as_str()))
			else {
				break;
			};
			self.focus = Some(i);
			if n + 1 == path.len() || !self.open_known(i) {
				break;
			}
			level = self.children(Some(i)).to_vec();
		}
		if self.focus.is_none() {
			self.focus = self.roots.first().copied();
		}
	}

	/// Opens a folder whose children are known without a fetch.
	fn open_known(&mut self, i: usize) -> bool {
		let known = match &self.items[i].kind {
			Kind::Folder {
				children: Children::Loaded(_),
				..
			} => true,
			Kind::Folder {
				id,
				children: Children::Unloaded,
				..
			} => self.cache.contains_key(id),
			_ => false,
		};
		if known {
			self.open(i);
		}
		known
	}

	/// Collections and Custom Stations follow the library; items still there keep their place.
	pub fn refresh_local(&mut self, lib: &Library) {
		let chain = self.chain();
		let old = self.children(Some(self.collections)).to_vec();
		let mut kids = Vec::new();
		for c in lib.collections.iter().filter(|c| c.id != CUSTOM) {
			let i = self.keep(self.collections, &old, Kind::local(&c.id, &c.name));
			self.set_contents(i, &c.stations, &c.folders, &c.id);
			kids.push(i);
		}
		self.set_children(self.collections, kids);
		if let Some(c) = lib.collections.iter().find(|c| c.id == CUSTOM) {
			self.set_contents(self.custom, &c.stations, &c.folders, &c.id);
		}
		self.refocus(&chain);
	}

	/// A collection's own stations, then its folders, each folder recursed.
	/// Folder keys carry the path under the collection id, so a rebuilt
	/// level finds its old nodes again.
	fn set_contents(
		&mut self,
		folder: usize,
		stations: &[Station],
		folders: &[blindradio_core::library::Folder],
		prefix: &str,
	) {
		let old = self.children(Some(folder)).to_vec();
		let mut kids: Vec<usize> = stations
			.iter()
			.map(|st| self.keep(folder, &old, Kind::Station(st.clone())))
			.collect();
		for (fi, f) in folders.iter().enumerate() {
			let key = format!("{prefix}/f{fi}");
			let i = self.keep(folder, &old, Kind::local(&key, &f.name));
			self.set_contents(i, &f.stations, &f.folders, &key);
			kids.push(i);
		}
		self.set_children(folder, kids);
	}

	/// The old child with the same key, updated, else a new one.
	fn keep(&mut self, parent: usize, old: &[usize], kind: Kind) -> usize {
		let found = old
			.iter()
			.copied()
			.find(|&i| self.items[i].kind.key() == kind.key());
		let Some(i) = found else {
			return self.add(Some(parent), kind);
		};
		match (&mut self.items[i].kind, kind) {
			(Kind::Folder { name, .. }, Kind::Folder { name: new, .. }) => *name = new,
			(k, new) => *k = new,
		}
		i
	}

	/// Focus and its ancestors, each with its index on its level.
	fn chain(&self) -> Vec<(usize, usize)> {
		let mut chain = Vec::new();
		let mut at = self.focus;
		while let Some(i) = at {
			let pos = self.level(i).iter().position(|&x| x == i).unwrap_or(0);
			chain.push((i, pos));
			at = self.items[i].parent;
		}
		chain
	}

	/// Focus that went with its item moves to what took its place, else the one before, else
	/// the nearest ancestor left.
	fn refocus(&mut self, chain: &[(usize, usize)]) {
		for (k, &(i, _)) in chain.iter().enumerate() {
			if !self.attached(i) {
				continue;
			}
			if k > 0 {
				let level = self.children(Some(i));
				let pos = chain[k - 1].1;
				self.focus = Some(level.get(pos).or(level.last()).copied().unwrap_or(i));
			}
			return;
		}
	}

	fn attached(&self, mut i: usize) -> bool {
		loop {
			match self.items[i].parent {
				None => return self.roots.contains(&i),
				Some(p) if self.children(Some(p)).contains(&i) => i = p,
				Some(_) => return false,
			}
		}
	}

	pub fn first_on_level(&self, i: usize) -> usize {
		self.level(i).first().copied().unwrap_or(i)
	}

	pub fn last_on_level(&self, i: usize) -> usize {
		self.level(i).last().copied().unwrap_or(i)
	}

	pub fn top(&self) -> usize {
		self.roots[0]
	}

	/// The last item showing.
	pub fn bottom(&self) -> usize {
		let mut i = self.roots[self.roots.len() - 1];
		while self.expanded(i)
			&& let Some(&last) = self.children(Some(i)).last()
		{
			i = last;
		}
		i
	}

	/// A letter: the next item on the focused level starting with it.
	pub fn jump(&self, c: char) -> Option<usize> {
		let f = self.focus?;
		let level = self.level(f);
		let names: Vec<String> = level.iter().map(|&i| self.label(i)).collect();
		let names: Vec<&str> = names.iter().map(String::as_str).collect();
		let from = level.iter().position(|&i| i == f);
		letter_jump(&names, from, c)
			.map(|k| level[k])
			.filter(|&to| to != f)
	}

	/// The stations on the level of station `i`, and its index among them: the dial.
	pub fn dial(&self, i: usize) -> Option<(Vec<Station>, usize)> {
		self.station(i)?;
		let mut at = 0;
		let mut stations = Vec::new();
		for &x in self.level(i) {
			if let Some(st) = self.station(x) {
				if x == i {
					at = stations.len();
				}
				stations.push(st.clone());
			}
		}
		Some((stations, at))
	}

	/// Every station at or below row `i`, in list order: what an
	/// export of a category, network or section header contains.
	/// Only rows the model has loaded can appear.
	pub fn stations_under(&self, i: usize) -> Vec<Station> {
		let mut out = Vec::new();
		let mut stack = vec![i];
		while let Some(x) = stack.pop() {
			if let Some(st) = self.station(x) {
				out.push(st.clone());
			}
			// Push reversed so the walk stays in list order.
			for &c in self.children(Some(x)).iter().rev() {
				stack.push(c);
			}
		}
		out
	}

	/// The local folder row `i` names, as (collection id, folder path):
	/// a folder inside Collections or Custom Stations, at any depth.
	/// A collection row itself is not one, and neither is a remote
	/// directory's folder.
	pub fn local_folder(&self, i: usize) -> Option<(String, Vec<usize>)> {
		if self.station(i).is_none() && self.is_folder(i) && self.is_local(i) {
			self.key(i).and_then(local_path)
		} else {
			None
		}
	}

	/// A local station's place in the library: its collection, the
	/// path of the folder holding it (empty at the collection's own
	/// level), and its index among the stations there. None for
	/// remote stations, whose order is the source's.
	pub fn local_station(&self, i: usize) -> Option<(String, Vec<usize>, usize)> {
		if self.station(i).is_none() || !self.is_local(i) {
			return None;
		}
		let parent = self.items[i].parent?;
		let index = self
			.children(Some(parent))
			.iter()
			.filter(|&&x| self.station(x).is_some())
			.position(|&x| x == i)?;
		if parent == self.custom || self.items[parent].parent == Some(self.collections) {
			Some((self.key(parent)?.to_owned(), Vec::new(), index))
		} else {
			let (collection, path) = self.local_folder(parent)?;
			Some((collection, path, index))
		}
	}

	/// Where Paste puts a station copied with this row in mind: a
	/// local station's own folder, a local folder itself, or a
	/// collection's own level (its row's key is the collection id).
	/// None for remote rows and for the Collections header itself.
	pub fn paste_home(&self, i: usize) -> Option<(String, Vec<usize>)> {
		if let Some((collection, path, _)) = self.local_station(i) {
			return Some((collection, path));
		}
		if let Some(home) = self.local_folder(i) {
			return Some(home);
		}
		if i == self.collections || !self.is_local(i) {
			return None;
		}
		self.key(i).map(|key| (key.to_owned(), Vec::new()))
	}

	/// The row of a local folder, or of the collection itself for an
	/// empty path, found by the key its level builder gives it.
	pub fn local_row(&self, collection: &str, path: &[usize]) -> Option<usize> {
		let mut key = collection.to_owned();
		for i in path {
			key += &format!("/f{i}");
		}
		self.items
			.iter()
			.enumerate()
			.find(|(i, _)| self.is_local(*i) && self.key(*i) == Some(key.as_str()))
			.map(|(i, _)| i)
	}

	pub fn delete_target(&self, i: usize) -> Delete {
		// A folder inside a local root is the user's own too, empty or
		// not — it was NotOwn before, so an imported playlist's folders
		// could never be removed here (Brandon's report).
		if let Some((collection, path)) = self.local_folder(i) {
			return Delete::RemoveFolder { collection, path };
		}
		// So is a collection: the Delete key answered "only your own
		// stations" where its context menu's Delete worked.
		if let Some((collection, _)) = self.collection(i) {
			return Delete::RemoveFolder {
				collection,
				path: Vec::new(),
			};
		}
		// A station in a folder belongs to the collection above the folder.
		let Some(st) = self.station(i) else {
			return Delete::NotOwn;
		};
		let mut at = self.items[i].parent;
		while let Some(p) = at {
			if p == self.custom || self.items[p].parent == Some(self.collections) {
				return Delete::Remove {
					collection: self.key(p).unwrap_or_default().to_owned(),
					station: st.id.clone(),
				};
			}
			at = self.items[p].parent;
		}
		Delete::NotOwn
	}

	/// What the context menu is about.
	pub fn target(&self, i: usize) -> Target<'_> {
		match self.station(i) {
			Some(st) => Target::Station(st),
			None if i == self.custom => Target::Collection { custom: true },
			None if self.items[i].parent == Some(self.collections) => {
				Target::Collection { custom: false }
			}
			None => Target::Other,
		}
	}

	/// Id and name of collection folder `i`.
	pub fn collection(&self, i: usize) -> Option<(String, String)> {
		(self.items[i].parent == Some(self.collections)).then(|| {
			(
				self.key(i).unwrap_or_default().to_owned(),
				self.label(i).to_owned(),
			)
		})
	}

	pub fn local_roots(&self) -> [usize; 2] {
		[self.collections, self.custom]
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use blindradio_core::station::Source;

	fn st(id: &str) -> Station {
		Station {
			id: id.into(),
			name: id.into(),
			url: format!("http://{id}"),
			source: Source::RadioBrowser,
			info: None,
			details: Default::default(),
		}
	}

	fn folder(id: &str) -> Node {
		Node::Folder {
			id: id.into(),
			name: id.into(),
		}
	}

	fn station(id: &str) -> Node {
		Node::Station(st(id))
	}

	fn more(id: &str) -> Node {
		Node::LoadMore { id: id.into() }
	}

	/// The child of `parent` (a root when `None`) with this key.
	fn find(m: &BrowseModel, parent: Option<usize>, key: &str) -> usize {
		*m.children(parent)
			.iter()
			.find(|&&i| m.key(i) == Some(key))
			.unwrap_or_else(|| panic!("{key}"))
	}

	fn labels(m: &BrowseModel, parent: usize) -> Vec<&str> {
		m.children(Some(parent))
			.iter()
			.map(|&i| m.label(i))
			.collect()
	}

	/// Opens `id` under `parent` and answers its load with `nodes`.
	fn load(m: &mut BrowseModel, parent: Option<usize>, id: &str, nodes: Vec<Node>) -> usize {
		let i = find(m, parent, id);
		assert_eq!(m.open(i).as_deref(), Some(id), "fetches {id}");
		assert_eq!(m.loaded(i, Some(nodes)), None);
		i
	}

	/// Opens the Directories root with its seven directories, as the app does.
	fn open_dir(m: &mut BrowseModel) -> usize {
		let dirs = [
			("rb", "Radio Browser"),
			("tunein", "TuneIn"),
			("iheart", "iHeartRadio"),
			("icecast", "Icecast"),
			("shoutcast", "SHOUTcast"),
			("internet-radio", "Internet-Radio"),
			("iprd", "IPRD"),
		];
		load(
			m,
			None,
			"dir",
			dirs
				.iter()
				.map(|(id, name)| Node::Folder {
					id: id.to_string(),
					name: name.to_string(),
				})
				.collect(),
		)
	}

	#[test]
	fn browse_roots() {
		let m = BrowseModel::new(Memory::default(), &Library::default());
		let roots: Vec<&str> = m.children(None).iter().map(|&i| m.label(i)).collect();
		assert_eq!(
			roots,
			["Directories", "Networks", "Collections", "Custom Stations"]
		);
		assert_eq!(m.focus(), Some(m.children(None)[0]), "opens on the top");
	}

	#[test]
	fn home_end_stay_on_level() {
		let mut lib = Library::default();
		lib.add_custom("Mine", "http://mine");
		let mut m = BrowseModel::new(Memory::default(), &lib);
		let dir = open_dir(&mut m);
		let rb = load(
			&mut m,
			Some(dir),
			"rb",
			vec![folder("rb/genre"), folder("rb/country")],
		);
		let genre = load(
			&mut m,
			Some(rb),
			"rb/genre",
			vec![station("a"), station("b"), more("rb/genre@100")],
		);
		let b = find(&m, Some(genre), "b");
		assert_eq!(m.first_on_level(b), find(&m, Some(genre), "a"));
		assert_eq!(
			m.label(m.last_on_level(b)),
			"Load More",
			"End lands on Load More rather than leaving the branch"
		);
		assert_eq!(m.first_on_level(genre), genre);
		assert_eq!(m.last_on_level(genre), find(&m, Some(rb), "rb/country"));
		let custom = find(&m, None, CUSTOM);
		assert_eq!(m.last_on_level(rb), find(&m, Some(dir), "iprd"));
		assert_eq!(m.top(), dir, "Ctrl+Home");
		assert_eq!(m.bottom(), custom, "Ctrl+End: Custom Stations is closed");
		assert_eq!(m.open(custom), None, "local: nothing to fetch");
		assert_eq!(
			m.label(m.bottom()),
			"Mine",
			"Ctrl+End: the last visible item"
		);
	}

	#[test]
	fn load_more_focus_first_new() {
		let mut m = BrowseModel::new(Memory::default(), &Library::default());
		let dir = open_dir(&mut m);
		let rb = load(&mut m, Some(dir), "rb", vec![folder("rb/genre")]);
		let genre = load(
			&mut m,
			Some(rb),
			"rb/genre",
			vec![station("a"), station("b"), more("rb/genre@100")],
		);
		let more = find(&m, Some(genre), "rb/genre@100");
		m.set_focus(more);
		assert_eq!(m.more(more).as_deref(), Some("rb/genre@100"));
		assert_eq!(m.label(more), "Loading");
		assert_eq!(m.more(more), None, "one fetch at a time");
		assert_eq!(
			m.loaded(
				more,
				Some(vec![
					station("c"),
					station("d"),
					Node::LoadMore {
						id: "rb/genre@200".into()
					}
				])
			),
			None
		);
		assert_eq!(labels(&m, genre), ["a", "b", "c", "d", "Load More"]);
		let focus = m.focus().unwrap();
		assert_eq!(
			m.station(focus).map(|s| s.id.as_str()),
			Some("c"),
			"focus is on the first new station"
		);
	}

	#[test]
	fn overlapping_pages_list_a_station_once() {
		let mut m = BrowseModel::new(Memory::default(), &Library::default());
		let dir = open_dir(&mut m);
		let rb = load(&mut m, Some(dir), "rb", vec![folder("rb/genre")]);
		let genre = load(
			&mut m,
			Some(rb),
			"rb/genre",
			vec![station("a"), station("b"), more("rb/genre@100")],
		);
		let next = find(&m, Some(genre), "rb/genre@100");
		m.more(next);
		m.loaded(next, Some(vec![station("b"), station("c"), station("c")]));
		assert_eq!(labels(&m, genre), ["a", "b", "c"]);
	}

	#[test]
	fn browse_load_does_not_move_focus() {
		let mut m = BrowseModel::new(Memory::default(), &Library::default());
		let dir = open_dir(&mut m);
		let rb = load(
			&mut m,
			Some(dir),
			"rb",
			vec![folder("rb/genre"), folder("rb/country")],
		);
		let genre = find(&m, Some(rb), "rb/genre");
		let country = find(&m, Some(rb), "rb/country");
		assert!(m.open(genre).is_some());
		assert!(m.open(country).is_some());
		assert_eq!(labels(&m, genre), ["Loading"]);
		// Down went past the branch while it loaded.
		m.set_focus(country);
		m.loaded(genre, Some(vec![station("a"), station("b")]));
		assert_eq!(m.focus(), Some(country), "focus unchanged");
		assert_eq!(labels(&m, genre), ["a", "b"]);
		// Focus waits on the placeholder: it stays on that item, which is now the first station.
		let placeholder = m.children(Some(country))[0];
		m.set_focus(placeholder);
		m.loaded(country, Some(vec![station("x"), station("y")]));
		assert_eq!(m.focus(), Some(placeholder));
		assert_eq!(m.station(placeholder).map(|s| s.id.as_str()), Some("x"));
		assert_eq!(labels(&m, country), ["x", "y"]);
	}

	#[test]
	fn remember_path_falls_back_to_ancestor() {
		let mut lib = Library::default();
		let jazz = lib.create_collection("Jazz");
		lib.toggle_in_collection(&jazz, &st("x"));
		lib.toggle_in_collection(&jazz, &st("y"));

		// TuneIn ids are flat: the path, not the id, says where a station is.
		let mut m = BrowseModel::new(Memory::default(), &lib);
		let dir = open_dir(&mut m);
		let tunein = load(&mut m, Some(dir), "tunein", vec![folder("tunein/r101215")]);
		let africa = load(
			&mut m,
			Some(tunein),
			"tunein/r101215",
			vec![folder("tunein/r100289")],
		);
		let kenya = load(
			&mut m,
			Some(africa),
			"tunein/r100289",
			vec![station("s1"), more("tunein/r100289@100")],
		);
		let page = find(&m, Some(kenya), "tunein/r100289@100");
		m.more(page);
		m.loaded(page, Some(vec![station("s2")]));
		m.set_focus(find(&m, Some(kenya), "s2"));
		let mut m = BrowseModel::new(m.memory(Sort::default()), &lib);
		let f = m.focus().unwrap();
		assert_eq!(
			m.station(f).map(|s| s.id.as_str()),
			Some("s2"),
			"back on the station, its Load More page included"
		);
		let dir = find(&m, None, "dir");
		let tunein = find(&m, Some(dir), "tunein");
		let africa = find(&m, Some(tunein), "tunein/r101215");
		let kenya = find(&m, Some(africa), "tunein/r100289");
		assert!(m.expanded(tunein) && m.expanded(africa) && m.expanded(kenya));

		// The station left its collection: the collection.
		let collections = find(&m, None, "collections");
		assert_eq!(m.open(collections), None);
		let jazz_folder = find(&m, Some(collections), &jazz);
		m.open(jazz_folder);
		m.set_focus(find(&m, Some(jazz_folder), "y"));
		let memory = m.memory(Sort::default());
		lib.remove_from_collection(&jazz, "y");
		let mut m = BrowseModel::new(memory, &lib);
		let collections = find(&m, None, "collections");
		assert_eq!(m.focus(), Some(find(&m, Some(collections), &jazz)));
		assert!(m.expanded(collections));

		// Closed while a folder was loading: that folder. The Directories
		// listing is in the carried cache, so opening it fetches nothing.
		let dir = find(&m, None, "dir");
		assert_eq!(m.open(dir), None, "directories come from the cache");
		let rb = load(&mut m, Some(dir), "rb", vec![folder("rb/genre")]);
		let genre = find(&m, Some(rb), "rb/genre");
		m.open(genre);
		m.set_focus(m.children(Some(genre))[0]);
		let m = BrowseModel::new(m.memory(Sort::default()), &lib);
		let dir = find(&m, None, "dir");
		let rb = find(&m, Some(dir), "rb");
		assert_eq!(m.focus(), Some(find(&m, Some(rb), "rb/genre")));
	}

	#[test]
	fn browse_failure_message() {
		let mut m = BrowseModel::new(Memory::default(), &Library::default());
		let dir = open_dir(&mut m);
		let rb = find(&m, Some(dir), "rb");
		m.open(rb);
		m.set_focus(m.children(Some(rb))[0]);
		assert_eq!(m.loaded(rb, None).as_deref(), Some("Failed: Radio Browser"));
		assert!(!m.expanded(rb), "left collapsed");
		assert!(m.has_children(rb));
		assert_eq!(m.focus(), Some(rb), "off the placeholder that went");
		assert_eq!(m.open(rb).as_deref(), Some("rb"), "Right retries");

		m.loaded(rb, Some(vec![folder("rb/genre"), folder("rb/country")]));
		let genre = find(&m, Some(rb), "rb/genre");
		let country = find(&m, Some(rb), "rb/country");
		m.open(genre);
		m.open(country);
		assert_eq!(
			m.loaded(genre, None).as_deref(),
			Some("Failed: Radio Browser")
		);
		assert_eq!(m.loaded(country, None), None, "said once");
		m.open(genre);
		assert_eq!(
			m.loaded(genre, None).as_deref(),
			Some("Failed: Radio Browser"),
			"a retry that fails says it again"
		);

		m.open(genre);
		m.loaded(genre, Some(vec![station("a"), more("rb/genre@100")]));
		let page = find(&m, Some(genre), "rb/genre@100");
		m.more(page);
		assert_eq!(
			m.loaded(page, None).as_deref(),
			Some("Failed: Radio Browser")
		);
		assert_eq!(m.label(page), "Load More", "Enter tries again");
		assert_eq!(m.more(page).as_deref(), Some("rb/genre@100"));
	}

	#[test]
	fn browse_resort_drops_radio_browser_levels() {
		let mut m = BrowseModel::new(Memory::default(), &Library::default());
		let dir = open_dir(&mut m);
		let rb = load(&mut m, Some(dir), "rb", vec![folder("rb/genre")]);
		let genre = load(&mut m, Some(rb), "rb/genre", vec![station("a")]);
		m.set_focus(find(&m, Some(genre), "a"));
		m.resort();
		assert_eq!(m.focus(), Some(rb), "focus out of the dropped level");
		assert_eq!(m.open(rb).as_deref(), Some("rb"), "root fetches again");
		m.loaded(rb, Some(vec![folder("rb/genre")]));
		let genre = find(&m, Some(rb), "rb/genre");
		assert_eq!(
			m.open(genre).as_deref(),
			Some("rb/genre"),
			"cache dropped too"
		);
		// Local levels are not the directory's to drop.
		let collections = find(&m, None, "collections");
		assert_eq!(m.open(collections), None);
	}

	#[test]
	fn browse_delete_rules() {
		let mut lib = Library::default();
		let mine = lib.add_custom("Mine", "http://mine");
		let jazz = lib.create_collection("Jazz");
		lib.toggle_in_collection(&jazz, &mine);
		let mut m = BrowseModel::new(Memory::default(), &lib);
		let custom = find(&m, None, CUSTOM);
		let collections = find(&m, None, "collections");
		let jazz_folder = find(&m, Some(collections), &jazz);
		assert_eq!(
			m.delete_target(find(&m, Some(custom), &mine.id)),
			Delete::Remove {
				collection: CUSTOM.into(),
				station: mine.id.clone()
			}
		);
		assert_eq!(
			m.delete_target(find(&m, Some(jazz_folder), &mine.id)),
			Delete::Remove {
				collection: jazz.clone(),
				station: mine.id.clone()
			},
			"in a collection: out of it"
		);
		let dir = open_dir(&mut m);
		let rb = load(&mut m, Some(dir), "rb", vec![station("a")]);
		assert_eq!(m.delete_target(find(&m, Some(rb), "a")), Delete::NotOwn);
		assert_eq!(
			m.delete_target(jazz_folder),
			Delete::RemoveFolder {
				collection: jazz.clone(),
				path: Vec::new()
			},
			"the Delete key on a collection, like its menu's Delete"
		);
		assert_eq!(m.delete_target(custom), Delete::NotOwn, "Custom Stations is fixed");
		assert_eq!(m.delete_target(collections), Delete::NotOwn, "the header");
	}

	#[test]
	fn browse_collections_follow_the_library_order() {
		let mut lib = Library::default();
		let a = lib.create_collection("A");
		let b = lib.create_collection("B");
		let mut m = BrowseModel::new(Memory::default(), &lib);
		let collections = find(&m, None, "collections");
		let row_b = find(&m, Some(collections), &b);
		m.set_focus(row_b);
		// What Shift+Up on a collection row works from: its own id.
		let (id, path) = m.paste_home(row_b).unwrap();
		assert_eq!(lib.move_at(&id, &path, true), Some(Vec::new()));
		m.refresh_local(&lib);
		assert_eq!(labels(&m, collections), ["B", "A"]);
		assert_eq!(m.focus(), Some(row_b), "focus goes with the collection");
		assert_eq!(find(&m, Some(collections), &a), m.children(Some(collections))[1]);
		assert_eq!(lib.move_at(&id, &path, true), None, "first under Collections");
	}

	#[test]
	fn browse_delete_folder_rules() {
		// Folders inside the local roots are the user's own, empty or
		// not; a remote directory's folders never are.
		let mut lib = Library::default();
		let jazz = lib.create_collection("Jazz");
		lib.collection_mut(&jazz)
			.unwrap()
			.add_folder(&[], "Sub");
		lib.add_custom("Mine", "http://mine");
		lib.collection_mut(CUSTOM)
			.unwrap()
			.add_folder(&[], "Imported");
		let m = BrowseModel::new(Memory::default(), &lib);
		let collections = find(&m, None, "collections");
		let jazz_folder = find(&m, Some(collections), &jazz);
		assert_eq!(
			m.delete_target(find(&m, Some(jazz_folder), &format!("{jazz}/f0"))),
			Delete::RemoveFolder {
				collection: jazz.clone(),
				path: vec![0]
			},
			"a folder inside a collection"
		);
		let custom = find(&m, None, CUSTOM);
		assert_eq!(
			m.delete_target(find(&m, Some(custom), "custom/f0")),
			Delete::RemoveFolder {
				collection: CUSTOM.into(),
				path: vec![0]
			},
			"an imported playlist's folder in Custom Stations"
		);
	}

	#[test]
	fn browse_local_folder_resolves_at_any_depth() {
		// Rename needs the same resolution Delete uses: the folder's
		// collection and index path, nested folders included — and
		// nothing for the collection row itself.
		let mut lib = Library::default();
		let jazz = lib.create_collection("Jazz");
		lib.collection_mut(&jazz)
			.unwrap()
			.add_folder(&[], "Sub");
		lib.collection_mut(&jazz)
			.unwrap()
			.add_folder(&[0], "Inner");
		let m = BrowseModel::new(Memory::default(), &lib);
		let collections = find(&m, None, "collections");
		let jazz_row = find(&m, Some(collections), &jazz);
		assert_eq!(m.local_folder(jazz_row), None, "the collection itself");
		let sub = find(&m, Some(jazz_row), &format!("{jazz}/f0"));
		assert_eq!(
			m.local_folder(sub),
			Some((jazz.clone(), vec![0])),
			"a playlist's subfolder"
		);
		let inner = find(&m, Some(sub), &format!("{jazz}/f0/f0"));
		assert_eq!(
			m.local_folder(inner),
			Some((jazz.clone(), vec![0, 0])),
			"a subfolder inside the subfolder"
		);
	}

	#[test]
	fn browse_move_folder_remote() {
		let mut m = BrowseModel::new(Memory::default(), &Library::default());
		let dir = open_dir(&mut m);
		let rb = load(
			&mut m,
			Some(dir),
			"rb",
			vec![folder("rb/genre"), folder("rb/country"), folder("rb/language")],
		);
		let language = find(&m, Some(rb), "rb/language");
		// The first folder on a level has nowhere up.
		assert_eq!(m.move_folder(rb, true), None, "rb is first under Directories");
		let genre = find(&m, Some(rb), "rb/genre");
		assert_eq!(m.move_folder(genre, true), None, "already first");
		let (key, order) = m.move_folder(language, true).expect("moves up");
		assert_eq!(key, "rb");
		assert_eq!(order, ["rb/genre", "rb/language", "rb/country"]);
		assert_eq!(labels(&m, rb), ["rb/genre", "rb/language", "rb/country"]);
		let (key, order) = m.move_folder(language, true).expect("moves up again");
		assert_eq!(key, "rb");
		assert_eq!(order, ["rb/language", "rb/genre", "rb/country"]);
		assert_eq!(m.move_folder(language, true), None, "now first");
		let (_, order) = m.move_folder(language, false).expect("moves down");
		assert_eq!(order, ["rb/genre", "rb/language", "rb/country"]);

		// The saved order survives a fresh model over the same cache:
		// the level is rebuilt from the source order, then re-sorted.
		let mut m2 = BrowseModel::new(m.memory(Sort::default()), &Library::default());
		m2.set_order([("rb".to_string(), order)].into_iter().collect());
		let dir = find(&m2, None, "dir");
		assert_eq!(m2.open(dir), None, "directories come from the cache");
		let rb = find(&m2, Some(dir), "rb");
		assert_eq!(m2.open(rb), None, "rb comes from the cache");
		assert_eq!(labels(&m2, rb), ["rb/genre", "rb/language", "rb/country"]);
	}

	#[test]
	fn browse_move_folder_local_keys() {
		assert_eq!(
			local_path("custom/f0"),
			Some((CUSTOM.to_string(), vec![0]))
		);
		assert_eq!(
			local_path("col:9/f1/f2"),
			Some(("col:9".to_string(), vec![1, 2]))
		);
		assert_eq!(local_path("col:9"), None, "a collection row itself");
		assert_eq!(local_path("custom/x0"), None);
		let mut lib = Library::default();
		let mine = lib.add_custom("Mine", "http://mine");
		let m = BrowseModel::new(Memory::default(), &lib);
		let custom = find(&m, None, CUSTOM);
		assert!(m.is_local(custom));
		assert!(m.is_local(find(&m, Some(custom), &mine.id)));
		let collections = find(&m, None, "collections");
		assert!(m.is_local(collections));
		assert!(!m.is_local(find(&m, None, "dir")));
	}

	#[test]
	fn browse_local_station_places() {
		let mut lib = Library::default();
		let p = lib.create_collection("List");
		{
			let c = lib.collection_mut(&p).unwrap();
			for id in ["one", "two"] {
				c.stations_at_mut(&[]).unwrap().push(st(id));
			}
			let sub = c.add_folder(&[], "Sub").unwrap();
			for id in ["x", "y"] {
				c.stations_at_mut(&sub).unwrap().push(st(id));
			}
		}
		let m = BrowseModel::new(Memory::default(), &lib);
		let collections = find(&m, None, "collections");
		let list = find(&m, Some(collections), &p);
		// A station at the collection's own level: empty path, its
		// index among the stations there (folders do not count).
		let two = find(&m, Some(list), "two");
		assert_eq!(m.local_station(two), Some((p.clone(), Vec::new(), 1)));
		// A station inside a subfolder: the folder's path and index.
		let sub = find(&m, Some(list), &format!("{p}/f0"));
		let y = find(&m, Some(sub), "y");
		assert_eq!(m.local_station(y), Some((p.clone(), vec![0], 1)));
		assert_eq!(m.local_row(&p, &[0]), Some(sub));
		assert_eq!(m.local_row(&p, &[]), Some(list));
		// A folder is not a station, and remote rows have no place.
		assert_eq!(m.local_station(sub), None);
	}

	#[test]
	fn browse_display_labels_count_local_stations() {
		let mut lib = Library::default();
		let p = lib.create_collection("English");
		{
			let c = lib.collection_mut(&p).unwrap();
			c.stations_at_mut(&[]).unwrap().push(st("root-one"));
			let sub = c.add_folder(&[], "Misc").unwrap();
			for id in ["x", "y"] {
				c.stations_at_mut(&sub).unwrap().push(st(id));
			}
		}
		let m = BrowseModel::new(Memory::default(), &lib);
		let collections = find(&m, None, "collections");
		let english = find(&m, Some(collections), &p);
		// The collection counts everything under it, subfolders
		// included; the folder counts its own.
		assert_eq!(m.display_label(english), "English 3");
		let misc = find(&m, Some(english), &format!("{p}/f0"));
		assert_eq!(m.display_label(misc), "Misc 2");
		// Stations and the Collections header itself stay bare.
		let x = find(&m, Some(misc), "x");
		assert_eq!(m.display_label(x), "x");
		assert_eq!(m.display_label(collections), m.label(collections));
	}

	#[test]
	fn browse_paste_homes_follow_the_row() {
		let mut lib = Library::default();
		let p = lib.create_collection("List");
		{
			let c = lib.collection_mut(&p).unwrap();
			c.stations_at_mut(&[]).unwrap().push(st("root-one"));
			let sub = c.add_folder(&[], "Sub").unwrap();
			c.stations_at_mut(&sub).unwrap().push(st("deep"));
		}
		let m = BrowseModel::new(Memory::default(), &lib);
		let collections = find(&m, None, "collections");
		let list = find(&m, Some(collections), &p);
		let sub = find(&m, Some(list), &format!("{p}/f0"));
		// A playlist row pastes at its own level; a folder row into
		// itself; a station into the folder holding it.
		assert_eq!(m.paste_home(list), Some((p.clone(), Vec::new())));
		assert_eq!(m.paste_home(sub), Some((p.clone(), vec![0])));
		let deep = find(&m, Some(sub), "deep");
		assert_eq!(m.paste_home(deep), Some((p.clone(), vec![0])));
		let root_one = find(&m, Some(list), "root-one");
		assert_eq!(m.paste_home(root_one), Some((p.clone(), Vec::new())));
		// The Custom Stations root is a level of its own; the
		// Collections header is not a paste target at all.
		let custom = find(&m, None, CUSTOM);
		assert_eq!(
			m.paste_home(custom),
			Some((CUSTOM.to_string(), Vec::new()))
		);
		assert_eq!(m.paste_home(collections), None);
	}
}
