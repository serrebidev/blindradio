//! Collections (Ctrl+L): Custom Stations and every collection in one tree,
//! folders included.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::path::Path;
use std::rc::Rc;

use blindradio_core::import;
use blindradio_core::library::{CUSTOM, Collection, Folder, Library};
use blindradio_core::station::Station;
use wx_utils::{confirm, prompt_text};
use wxdragon::event::{Event, EventType, WxEvtHandler};
use wxdragon::prelude::*;

use crate::action::Action;
use crate::app::{App, PasteDest};
use crate::bridge::WindowToken;
use crate::context_menu::{self, Choice, Item, Target};
use crate::main_window::dispatch;
use crate::menus::plain;
use crate::station_list::letter_jump;
use crate::strings as s;

/// Custom Stations first, then every other collection in its order.
pub fn ordered(lib: &Library) -> Vec<&Collection> {
	let custom = lib.collections.iter().find(|c| c.id == CUSTOM);
	custom
		.into_iter()
		.chain(lib.collections.iter().filter(|c| c.id != CUSTOM))
		.collect()
}

/// Custom Stations cannot be renamed or deleted.
pub fn cannot_change(id: &str) -> Option<&'static str> {
	(id == CUSTOM).then_some(s::CUSTOM_FIXED)
}

/// What Add All to Collection offers: every other collection.
pub fn others(lib: &Library, from: &str) -> Vec<(String, String)> {
	lib.collections
		.iter()
		.filter(|c| c.id != CUSTOM && c.id != from)
		.map(|c| (c.id.clone(), c.name.clone()))
		.collect()
}

/// A row in the tree. Folder and Station rows index into the view's
/// location table, rebuilt with the tree.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Row {
	Collection(usize),
	Folder(usize),
	Station(usize),
}

/// Where a folder or station row lives.
#[derive(Clone, PartialEq, Eq)]
struct Loc {
	/// Index into `ordered`.
	collection: usize,
	/// The folder's own path, or the station's containing folder path.
	path: Vec<usize>,
	/// The station's index in its folder's list; `None` on folder rows.
	station: Option<usize>,
}

/// The collection of a focused row, and the station when one is focused.
struct At {
	id: String,
	/// The focused row's own name.
	name: String,
	/// The owning collection's name, for prompts about the collection.
	collection_name: String,
	path: Vec<usize>,
	station: Option<String>,
}

/// What a rebuild leaves focused.
#[derive(Clone, PartialEq, Eq)]
enum Focus {
	Collection(String),
	Folder(String, Vec<usize>),
	Station(String, String),
}

/// Every folder of a collection with its path, parents first, names joined.
fn folder_list(c: &Collection) -> Vec<(Vec<usize>, String)> {
	fn walk(folders: &[Folder], base: &[usize], names: &str, out: &mut Vec<(Vec<usize>, String)>) {
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
	walk(&c.folders, &[], "", &mut out);
	out
}

struct View {
	tree: TreeCtrl,
	root: TreeItemId,
	items: RefCell<Vec<(Row, TreeItemId)>>,
	locs: RefCell<Vec<Loc>>,
	/// Collections and folders left open, by key: a rebuild keeps them open.
	expanded: RefCell<HashSet<String>>,
	/// Set while the program changes the tree: only the user's moves tune.
	quiet: Cell<bool>,
}

impl View {
	fn row(&self, item: &TreeItemId) -> Option<Row> {
		self.tree
			.get_custom_data_direct(item)?
			.downcast_ref::<Row>()
			.copied()
	}

	fn item_of(&self, row: Row) -> Option<TreeItemId> {
		self.items
			.borrow()
			.iter()
			.find(|(r, _)| *r == row)
			.map(|(_, item)| item.clone())
	}

	fn selected_row(&self) -> Option<Row> {
		self.tree.get_selection().and_then(|item| self.row(&item))
	}

	fn loc(&self, row: Row) -> Option<Loc> {
		match row {
			Row::Folder(i) | Row::Station(i) => self.locs.borrow().get(i).cloned(),
			Row::Collection(_) => None,
		}
	}

	fn row_for(&self, collection: usize, path: &[usize], station: Option<usize>) -> Option<Row> {
		self.locs
			.borrow()
			.iter()
			.position(|l| l.collection == collection && l.path == path && l.station == station)
			.map(|i| {
				if station.is_some() {
					Row::Station(i)
				} else {
					Row::Folder(i)
				}
			})
	}

	fn quietly<R>(&self, f: impl FnOnce() -> R) -> R {
		let was = self.quiet.replace(true);
		let r = f();
		self.quiet.set(was);
		r
	}

	fn folder_key(id: &str, path: &[usize]) -> String {
		let parts: Vec<String> = path.iter().map(|i| i.to_string()).collect();
		format!("{id}|{}", parts.join("/"))
	}

	/// Rebuilds the tree from the library; `focus` is selected without tuning.
	fn refresh(&self, app: &Rc<RefCell<App>>, focus: Option<Focus>) {
		let mut landed: Option<TreeItemId> = None;
		let mut landed_station: Option<Station> = None;
		self.quietly(|| {
			self.tree.delete_children(&self.root);
			self.items.borrow_mut().clear();
			self.locs.borrow_mut().clear();
			let lib = app.borrow();
			for (ci, c) in ordered(&lib.library).iter().enumerate() {
				let Some(item) = self.tree.append_item(&self.root, &c.name, None, None) else {
					continue;
				};
				self.tree
					.set_custom_data_direct(&item, Row::Collection(ci));
				self.items
					.borrow_mut()
					.push((Row::Collection(ci), item.clone()));
				if focus == Some(Focus::Collection(c.id.clone())) {
					landed = Some(item.clone());
				}
				self.level(
					&item,
					ci,
					c,
					&[],
					&c.stations,
					&c.folders,
					&focus,
					&mut landed,
					&mut landed_station,
				);
				if self.expanded.borrow().contains(&c.id) {
					self.tree.expand(&item);
				}
			}
			drop(lib);
			if let Some(item) = &landed {
				self.tree.select_item(item);
				self.tree.set_focused_item(item);
				self.tree.ensure_visible(item);
			}
		});
		// What Ctrl+D and Enter act on next.
		app.borrow_mut().selected = landed_station;
	}

	#[allow(clippy::too_many_arguments)]
	fn level(
		&self,
		parent: &TreeItemId,
		ci: usize,
		c: &Collection,
		path: &[usize],
		stations: &[Station],
		folders: &[Folder],
		focus: &Option<Focus>,
		landed: &mut Option<TreeItemId>,
		landed_station: &mut Option<Station>,
	) {
		let was_landed = landed.is_some();
		for (si, st) in stations.iter().enumerate() {
			let Some(child) = self.tree.append_item(parent, &st.name, None, None) else {
				continue;
			};
			let li = self.locs.borrow().len();
			self.locs.borrow_mut().push(Loc {
				collection: ci,
				path: path.to_vec(),
				station: Some(si),
			});
			self.tree
				.set_custom_data_direct(&child, Row::Station(li));
			self.items.borrow_mut().push((Row::Station(li), child.clone()));
			if *focus == Some(Focus::Station(c.id.clone(), st.id.clone())) {
				*landed = Some(child);
				*landed_station = Some(st.clone());
			}
		}
		for (fi, f) in folders.iter().enumerate() {
			let mut fpath = path.to_vec();
			fpath.push(fi);
			let Some(child) = self.tree.append_item(parent, &f.name, None, None) else {
				continue;
			};
			let li = self.locs.borrow().len();
			self.locs.borrow_mut().push(Loc {
				collection: ci,
				path: fpath.clone(),
				station: None,
			});
			self.tree.set_custom_data_direct(&child, Row::Folder(li));
			self.items.borrow_mut().push((Row::Folder(li), child.clone()));
			if *focus == Some(Focus::Folder(c.id.clone(), fpath.clone())) {
				*landed = Some(child.clone());
			}
			self.level(
				&child,
				ci,
				c,
				&fpath,
				&f.stations,
				&f.folders,
				focus,
				landed,
				landed_station,
			);
			if self
				.expanded
				.borrow()
				.contains(&Self::folder_key(&c.id, &fpath))
			{
				self.tree.expand(&child);
			}
		}
		// A focused station or folder must be shown: open its parents.
		if !was_landed && landed.is_some() {
			self.tree.expand(parent);
		}
	}

	fn station_at(&self, app: &Rc<RefCell<App>>, row: Row) -> Option<Station> {
		let loc = self.loc(row)?;
		let lib = app.borrow();
		let tree = ordered(&lib.library);
		let c = tree.get(loc.collection)?;
		c.stations_at(&loc.path)?.get(loc.station?).cloned()
	}

	/// The stations of the focused station's own level, with its index.
	fn level_of(&self, app: &Rc<RefCell<App>>, row: Row) -> Option<(Vec<Station>, usize)> {
		let loc = self.loc(row)?;
		let lib = app.borrow();
		let tree = ordered(&lib.library);
		let c = tree.get(loc.collection)?;
		let stations = c.stations_at(&loc.path)?.to_vec();
		Some((stations, loc.station?))
	}

	fn at(&self, app: &Rc<RefCell<App>>, row: Row) -> Option<At> {
		let lib = app.borrow();
		let tree = ordered(&lib.library);
		let (ci, path, station) = match row {
			Row::Collection(ci) => (ci, Vec::new(), None),
			Row::Folder(_) | Row::Station(_) => {
				let loc = self.loc(row)?;
				(loc.collection, loc.path, loc.station)
			}
		};
		let c = tree.get(ci)?;
		let (name, station) = match (row, station) {
			(Row::Folder(_), _) => (
				c.folder_at(&path).map(|f| f.name.clone())?,
				None,
			),
			(_, Some(si)) => {
				let st = c.stations_at(&path)?.get(si)?;
				(st.name.clone(), Some(st.id.clone()))
			}
			_ => (c.name.clone(), None),
		};
		Some(At {
			id: c.id.clone(),
			name,
			collection_name: c.name.clone(),
			path,
			station,
		})
	}

	/// The expansion key of a focused row, for the expand/collapse handlers.
	fn key_of(&self, app: &Rc<RefCell<App>>, row: Row) -> Option<String> {
		let at = self.at(app, row)?;
		match row {
			Row::Collection(_) => Some(at.id),
			Row::Folder(_) => Some(Self::folder_key(&at.id, &at.path)),
			Row::Station(_) => None,
		}
	}

	fn show_collection(&self, app: &Rc<RefCell<App>>, id: &str) {
		self.refresh(app, Some(Focus::Collection(id.to_string())));
	}

	fn show_station(&self, app: &Rc<RefCell<App>>, collection: &str, station: &str) {
		self.refresh(
			app,
			Some(Focus::Station(collection.to_string(), station.to_string())),
		);
	}

	fn new_collection(&self, app: &Rc<RefCell<App>>) {
		let Some(name) = prompt_text(&self.tree, s::COLLECTION_NAME, s::NEW_COLLECTION_TITLE)
		else {
			return;
		};
		let id = {
			let mut a = app.borrow_mut();
			let id = a.library.create_collection(&name);
			a.save_library();
			id
		};
		self.show_collection(app, &id);
	}

	fn new_folder(&self, app: &Rc<RefCell<App>>, at: &At) {
		let Some(name) = prompt_text(&self.tree, s::FOLDER_NAME, s::NEW_FOLDER_TITLE) else {
			return;
		};
		let parent = if at.station.is_some() {
			at.path.clone()
		} else {
			at.path.clone()
		};
		let made = {
			let mut a = app.borrow_mut();
			let made = a
				.library
				.collection_mut(&at.id)
				.and_then(|c| c.add_folder(&parent, &name));
			if made.is_some() {
				a.save_library();
			}
			made
		};
		if let Some(path) = made {
			self.refresh(app, Some(Focus::Folder(at.id.clone(), path)));
		}
	}

	fn rename(&self, app: &Rc<RefCell<App>>, row: Row, at: &At) {
		if matches!(row, Row::Folder(_)) {
			let Some(name) = prompt_text(&self.tree, s::FOLDER_NAME, &plain(s::RENAME_ITEM))
			else {
				return;
			};
			{
				let mut a = app.borrow_mut();
				if a
					.library
					.collection_mut(&at.id)
					.is_some_and(|c| c.rename_folder(&at.path, &name))
				{
					a.save_library();
				}
			}
			return self.refresh(app, Some(Focus::Folder(at.id.clone(), at.path.clone())));
		}
		if let Some(text) = cannot_change(&at.id) {
			return app.borrow_mut().say(text);
		}
		let Some(name) = prompt_text(&self.tree, s::COLLECTION_NAME, &plain(s::RENAME_ITEM)) else {
			return;
		};
		{
			let mut a = app.borrow_mut();
			if a.library.rename_collection(&at.id, &name).is_ok() {
				a.save_library();
			}
		}
		self.show_collection(app, &at.id);
	}

	fn delete(&self, app: &Rc<RefCell<App>>, row: Row, at: &At) {
		if matches!(row, Row::Folder(_)) {
			let question = format!("{}{}?", s::DELETE_PREFIX, at.name);
			if !confirm(&self.tree, &question, &plain(s::DELETE_ITEM)) {
				return;
			}
			let parent: Vec<usize> = at.path[..at.path.len().saturating_sub(1)].to_vec();
			{
				let mut a = app.borrow_mut();
				if a
					.library
					.collection_mut(&at.id)
					.is_some_and(|c| c.remove_folder(&at.path))
				{
					a.save_library();
					a.refresh_dial();
				}
			}
			return self.refresh(app, Some(Focus::Folder(at.id.clone(), parent)));
		}
		if let Some(text) = cannot_change(&at.id) {
			return app.borrow_mut().say(text);
		}
		let question = format!("{}{}?", s::DELETE_PREFIX, at.collection_name);
		if !confirm(&self.tree, &question, &plain(s::DELETE_ITEM)) {
			return;
		}
		{
			let mut a = app.borrow_mut();
			if a.library.delete_collection(&at.id).is_ok() {
				a.save_library();
				a.refresh_dial();
			}
		}
		self.refresh(app, None);
	}

	fn remove_station(&self, app: &Rc<RefCell<App>>, at: &At, station: &str) {
		{
			let mut a = app.borrow_mut();
			a.library.remove_from_collection(&at.id, station);
			a.save_library();
			a.refresh_dial();
		}
		self.show_collection(app, &at.id);
	}

	fn move_station(&self, app: &Rc<RefCell<App>>, row: Row, at: &At, up: bool) {
		let Some(loc) = self.loc(row) else {
			return;
		};
		let Some(si) = loc.station else {
			return;
		};
		let to = if up {
			si.saturating_sub(1)
		} else {
			si + 1
		};
		let station = at.station.clone();
		{
			let mut a = app.borrow_mut();
			if let Some(list) = a
				.library
				.collection_mut(&at.id)
				.and_then(|c| c.stations_at_mut(&at.path))
				&& to < list.len()
				&& to != si
			{
				list.swap(si, to);
				a.save_library();
			}
		}
		if let Some(station) = station {
			self.show_station(app, &at.id, &station);
		}
	}

	fn move_folder(&self, app: &Rc<RefCell<App>>, at: &At, up: bool) {
		let Some((&last, parent)) = at.path.split_last() else {
			return;
		};
		let parent = parent.to_vec();
		let to = if up {
			last.saturating_sub(1)
		} else {
			last + 1
		};
		let landed = {
			let mut a = app.borrow_mut();
			match a
				.library
				.collection_mut(&at.id)
				.and_then(|c| c.folder_list_at_mut(&parent))
			{
				Some(list) if to < list.len() && to != last => {
					list.swap(last, to);
					a.save_library();
					to
				}
				_ => last,
			}
		};
		let mut path = parent;
		path.push(landed);
		self.refresh(app, Some(Focus::Folder(at.id.clone(), path)));
	}

	fn move_collection(&self, app: &Rc<RefCell<App>>, at: &At, up: bool) {
		if let Some(text) = cannot_change(&at.id) {
			return app.borrow_mut().say(text);
		}
		{
			let mut a = app.borrow_mut();
			a.library.move_collection(&at.id, up);
			a.save_library();
		}
		self.show_collection(app, &at.id);
	}

	fn move_station_to(&self, app: &Rc<RefCell<App>>, at: &At, station: &str, to_path: &[usize]) {
		{
			let mut a = app.borrow_mut();
			if let Some(c) = a.library.collection_mut(&at.id) {
				let moved = c
					.stations_at_mut(&at.path)
					.and_then(|list| {
						list.iter()
							.position(|x| x.id == station)
							.map(|i| list.remove(i))
					});
				if let Some(st) = moved {
					if let Some(target) = c.stations_at_mut(to_path)
						&& !target.iter().any(|x| x.id == st.id)
					{
						target.push(st);
					}
					a.save_library();
					a.refresh_dial();
				}
			}
		}
		self.show_station(app, &at.id, station);
	}

	fn export(&self, app: &Rc<RefCell<App>>, row: Row, at: &At) {
		let (stations, folders) = {
			let lib = app.borrow();
			let Some(c) = lib.library.collections.iter().find(|c| c.id == at.id) else {
				return;
			};
			match row {
				Row::Folder(_) => match c.folder_at(&at.path) {
					Some(f) => (f.stations.clone(), f.folders.clone()),
					None => return,
				},
				_ => (c.stations.clone(), c.folders.clone()),
			}
		};
		let file_name = match row {
			Row::Folder(_) => &at.name,
			_ => &at.collection_name,
		};
		let file = FileDialog::builder(&self.tree)
			.with_message(s::EXPORT)
			.with_default_file(&format!("{}{}", file_name, s::M3U_EXTENSION))
			.with_wildcard(s::PLAYLISTS_FILTER)
			.with_style(FileDialogStyle::Save | FileDialogStyle::OverwritePrompt)
			.build();
		if file.show_modal() != ID_OK {
			return;
		}
		let Some(path) = file.get_path() else {
			return;
		};
		let count = stations.len()
			+ folders
				.iter()
				.map(|f| f.all_stations().len())
				.sum::<usize>();
		match import::export_m3u_tree(&stations, &folders, Path::new(&path)) {
			Ok(()) => app
				.borrow_mut()
				.say(&format!("{}{}", s::EXPORTED, count)),
			Err(e) => {
				eprintln!("export: {e}");
				app.borrow_mut().say(s::NOT_SAVED);
			}
		}
	}

	fn menu(&self, app: &Rc<RefCell<App>>) {
		let Some(row) = self.selected_row() else {
			return;
		};
		match row {
			Row::Station(_) => self.station_menu(app),
			Row::Folder(_) => self.folder_menu(app, row),
			Row::Collection(_) => self.collection_menu(app, row),
		}
	}

	fn station_menu(&self, app: &Rc<RefCell<App>>) {
		let Some(row) = self.selected_row() else {
			return;
		};
		let Some(station) = self.station_at(app, row) else {
			return;
		};
		let Some(at) = self.at(app, row) else {
			return;
		};
		let mut items = {
			let lib = app.borrow();
			match context_menu::items_for(
				&Target::Station(&station),
				&lib.library,
				lib.station_clip.is_some(),
			) {
				Ok(items) => items,
				Err(text) => {
					drop(lib);
					return app.borrow_mut().say(text);
				}
			}
		};
		// Moving between this collection's folders, root included.
		{
			let lib = app.borrow();
			if let Some(c) = lib.library.collections.iter().find(|c| c.id == at.id)
				&& !c.folders.is_empty()
			{
				let mut targets: Vec<(Vec<usize>, String)> =
					vec![(Vec::new(), c.name.clone())];
				targets.extend(folder_list(c));
				targets.retain(|(path, _)| *path != at.path);
				items.push(Item::MoveToFolder(targets));
			}
		}
		match context_menu::popup(&self.tree, &items) {
			Some(Choice::MoveToFolder(path)) => {
				self.move_station_to(app, &at, &station.id, &path);
			}
			Some(choice) => {
				context_menu::apply(app, &self.tree, &station, choice);
				// Adding to or removing from a collection changes this tree.
				self.refresh(app, Some(Focus::Station(at.id.clone(), station.id.clone())));
			}
			None => {}
		}
	}

	fn folder_menu(&self, app: &Rc<RefCell<App>>, row: Row) {
		let Some(at) = self.at(app, row) else {
			return;
		};
		let mut items = vec![Item::Rename, Item::Delete, Item::Export];
		if app.borrow().station_clip.is_some() {
			items.push(Item::PasteStation);
		}
		match context_menu::popup(&self.tree, &items) {
			Some(Choice::Rename) => self.rename(app, row, &at),
			Some(Choice::Delete) => self.delete(app, row, &at),
			Some(Choice::Export) => self.export(app, row, &at),
			Some(Choice::PasteStation) => {
				app.borrow_mut()
					.paste_into(PasteDest::Collection(at.id.clone(), at.path.clone()));
				self.refresh(app, None);
			}
			_ => {}
		}
	}

	fn collection_menu(&self, app: &Rc<RefCell<App>>, row: Row) {
		let Some(collection) = self.at(app, row) else {
			return;
		};
		let mut items = Vec::new();
		if cannot_change(&collection.id).is_none() {
			items.push(Item::Rename);
			items.push(Item::Delete);
		}
		let to = others(&app.borrow().library, &collection.id);
		if !to.is_empty() {
			items.push(Item::AddAllTo(to));
		}
		items.push(Item::Export);
		if app.borrow().station_clip.is_some() {
			items.push(Item::PasteStation);
		}
		match context_menu::popup(&self.tree, &items) {
			Some(Choice::Rename) => self.rename(app, row, &collection),
			Some(Choice::Delete) => self.delete(app, row, &collection),
			Some(Choice::Export) => self.export(app, row, &collection),
			Some(Choice::PasteStation) => {
				app.borrow_mut()
					.paste_into(PasteDest::Collection(collection.id.clone(), Vec::new()));
				self.refresh(app, None);
			}
			Some(Choice::AddAllTo(id)) => {
				app.borrow_mut().add_all(&collection.id, &id);
				self.show_collection(app, &collection.id);
			}
			_ => {}
		}
	}

	/// A letter: the next row on the focused level starting with it.
	fn jump(&self, app: &Rc<RefCell<App>>, row: Row, c: char) -> Option<Row> {
		let lib = app.borrow();
		let tree = ordered(&lib.library);
		match row {
			Row::Station(_) => {
				let loc = self.loc(row)?;
				let col = tree.get(loc.collection)?;
				let names: Vec<&str> = col
					.stations_at(&loc.path)?
					.iter()
					.map(|s| s.name.as_str())
					.collect();
				letter_jump(&names, loc.station, c).and_then(|si| {
					self.row_for(loc.collection, &loc.path, Some(si))
				})
			}
			Row::Folder(_) => {
				let loc = self.loc(row)?;
				let col = tree.get(loc.collection)?;
				let parent = &loc.path[..loc.path.len() - 1];
				let siblings: &[Folder] = if parent.is_empty() {
					&col.folders
				} else {
					&col.folder_at(parent)?.folders
				};
				let names: Vec<&str> = siblings.iter().map(|f| f.name.as_str()).collect();
				letter_jump(&names, Some(*loc.path.last()?), c).and_then(|fi| {
					let mut path = parent.to_vec();
					path.push(fi);
					self.row_for(loc.collection, &path, None)
				})
			}
			Row::Collection(ci) => {
				let names: Vec<&str> = tree.iter().map(|s| s.name.as_str()).collect();
				letter_jump(&names, Some(ci), c).map(Row::Collection)
			}
		}
	}
}

pub fn show(app: &Rc<RefCell<App>>) -> Option<Action> {
	let frame = app.borrow().win.frame;
	let dialog = Dialog::builder(&frame, s::COLLECTIONS_TITLE).build();
	// The label first: wxMSW names the tree after it.
	let label = StaticText::builder(&dialog)
		.with_label(s::COLLECTIONS_LIST)
		.build();
	let tree_ctrl = TreeCtrl::builder(&dialog)
		.with_style(
			TreeCtrlStyle::HasButtons
				| TreeCtrlStyle::LinesAtRoot
				| TreeCtrlStyle::HideRoot
				| TreeCtrlStyle::Single,
		)
		.build();
	let new = Button::builder(&dialog)
		.with_label(s::NEW_COLLECTION_BUTTON)
		.build();
	let new_folder = Button::builder(&dialog).with_label(s::NEW_FOLDER).build();
	let rename = Button::builder(&dialog).with_label(s::RENAME_ITEM).build();
	let delete = Button::builder(&dialog).with_label(s::DELETE_ITEM).build();
	let remove = Button::builder(&dialog)
		.with_label(s::REMOVE_STATION)
		.build();
	let up = Button::builder(&dialog).with_label(s::MOVE_UP).build();
	let down = Button::builder(&dialog).with_label(s::MOVE_DOWN).build();
	let export = Button::builder(&dialog).with_label(s::EXPORT).build();
	let close = Button::builder(&dialog)
		.with_id(ID_CANCEL)
		.with_label(s::CLOSE)
		.build();
	let edit = BoxSizer::builder(Orientation::Horizontal).build();
	for b in [&new, &new_folder, &rename, &delete, &remove, &close] {
		edit.add(b, 0, SizerFlag::All, 5);
	}
	let order = BoxSizer::builder(Orientation::Horizontal).build();
	for b in [&up, &down, &export] {
		order.add(b, 0, SizerFlag::All, 5);
	}
	let sizer = BoxSizer::builder(Orientation::Vertical).build();
	sizer.add(
		&label,
		0,
		SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
		5,
	);
	sizer.add(&tree_ctrl, 1, SizerFlag::Expand | SizerFlag::All, 5);
	sizer.add_sizer(&edit, 0, SizerFlag::All, 0);
	sizer.add_sizer(&order, 0, SizerFlag::All, 0);
	dialog.set_sizer(sizer, true);
	dialog.set_size(Size::new(640, 560));

	let Some(root) = tree_ctrl.add_root("", None, None) else {
		dialog.destroy();
		return None;
	};
	let view = Rc::new(View {
		tree: tree_ctrl,
		root,
		items: RefCell::new(Vec::new()),
		locs: RefCell::new(Vec::new()),
		expanded: RefCell::new(HashSet::new()),
		quiet: Cell::new(false),
	});
	let first = ordered(&app.borrow().library)
		.first()
		.map(|c| Focus::Collection(c.id.clone()));
	view.refresh(app, first);

	// Buttons act on the focused row.
	{
		let (v, a) = (view.clone(), app.clone());
		new.on_click(move |_| v.new_collection(&a));
	}
	{
		let (v, a) = (view.clone(), app.clone());
		new_folder.on_click(move |_| {
			if let Some(row) = v.selected_row()
				&& let Some(at) = v.at(&a, row)
			{
				// On a folder row the new folder goes inside it; on a
				// station row it joins the station's folder; on a
				// collection row it lands at the root.
				v.new_folder(&a, &at);
			}
		});
	}
	for (button, act) in [
		(rename, 0),
		(delete, 1),
		(remove, 2),
		(up, 3),
		(down, 4),
		(export, 5),
	] {
		let (v, a) = (view.clone(), app.clone());
		button.on_click(move |_| {
			let Some(row) = v.selected_row() else {
				return;
			};
			let Some(at) = v.at(&a, row) else {
				return;
			};
			match (act, row, &at) {
				(0, _, _) => v.rename(&a, row, &at),
				(1, _, _) => v.delete(&a, row, &at),
				(
					2,
					_,
					At {
						station: Some(st), ..
					},
				) => v.remove_station(&a, &at, st),
				(3 | 4, Row::Station(_), _) => v.move_station(&a, row, &at, act == 3),
				(3 | 4, Row::Folder(_), _) => v.move_folder(&a, &at, act == 3),
				(3 | 4, _, _) => v.move_collection(&a, &at, act == 3),
				(5, _, _) => v.export(&a, row, &at),
				_ => {}
			}
		});
	}

	let token = WindowToken::new();
	bind(&view, app, dialog, &token);

	view.tree.set_focus();
	crate::dialogs::run(app, dialog, Action::OpenCollections, view.tree, &token)
}

fn bind(view: &Rc<View>, app: &Rc<RefCell<App>>, dialog: Dialog, _token: &WindowToken) {
	let tree = view.tree;

	let (v, a) = (view.clone(), app.clone());
	tree.on_selection_changed(move |e| {
		if v.quiet.get() {
			return;
		}
		let Some(row) = e.get_item().and_then(|item| v.row(&item)) else {
			return;
		};
		match row {
			Row::Station(_) => {
				if let Some((stations, si)) = v.level_of(&a, row) {
					a.borrow_mut().focus_station(&stations, si);
				}
			}
			Row::Folder(_) | Row::Collection(_) => a.borrow_mut().selected = None,
		}
	});

	let (v, a) = (view.clone(), app.clone());
	tree.on_item_expanding(move |e| {
		if let Some(key) = e
			.get_item()
			.and_then(|item| v.row(&item))
			.and_then(|row| v.key_of(&a, row))
		{
			v.expanded.borrow_mut().insert(key);
		}
	});
	let (v, a) = (view.clone(), app.clone());
	tree.on_item_collapsed(move |e| {
		if let Some(key) = e
			.get_item()
			.and_then(|item| v.row(&item))
			.and_then(|row| v.key_of(&a, row))
		{
			v.expanded.borrow_mut().remove(&key);
		}
	});

	let (v, a) = (view.clone(), app.clone());
	tree.on_item_activated(move |e| {
		// Handled: a double click must not toggle the folder as well.
		e.event.skip(false);
		let Some(item) = e.get_item() else {
			return;
		};
		match v.row(&item) {
			Some(Row::Station(_)) => {
				let station = v
					.selected_row()
					.and_then(|row| v.station_at(&a, row));
				let Some(st) = station else {
					return;
				};
				a.borrow_mut().selected = Some(st);
				dispatch(&a, Action::Play);
				if a.borrow().settings.close_lists_when_playing {
					dialog.end_modal(ID_OK);
				}
			}
			Some(Row::Folder(_)) | Some(Row::Collection(_)) => {
				v.quietly(|| v.tree.toggle(&item))
			}
			None => {}
		}
	});

	let (v, a) = (view.clone(), app.clone());
	tree.bind_internal(EventType::CHAR, move |e: Event| {
		let c = e
			.get_unicode_key()
			.and_then(|k| char::from_u32(k as u32))
			.filter(|c| !c.is_control() && *c != ' ');
		match c.filter(|_| !e.control_down() && !e.alt_down()) {
			// Consumed: the native type-ahead would jump as well.
			Some(c) => {
				e.skip(false);
				let to = v.selected_row().and_then(|row| v.jump(&a, row, c));
				if let Some(to) = to
					&& let Some(item) = v.item_of(to)
				{
					v.tree.select_item(&item);
					v.tree.set_focused_item(&item);
					v.tree.ensure_visible(&item);
				}
			}
			None => e.skip(true),
		}
	});

	let (v, a) = (view.clone(), app.clone());
	tree.bind_internal(EventType::KEY_DOWN, move |e: Event| {
		let Some(code) = e.get_key_code() else {
			return e.skip(true);
		};
		let plain = !(e.control_down() || e.alt_down() || e.shift_down());
		let alt = e.alt_down() && !e.control_down() && !e.shift_down();
		// Shift+Up / Shift+Down moves folders too (decided below: folders only).
		let shift = e.shift_down() && !e.control_down() && !e.alt_down();
		let act = match (code, plain, alt, shift) {
			(WXK_DELETE, true, _, _) => 0,
			(WXK_F2, true, _, _) => 1,
			(WXK_UP, _, true, _) => 2,
			(WXK_DOWN, _, true, _) => 3,
			(WXK_UP, _, false, true) => 2,
			(WXK_DOWN, _, false, true) => 3,
			_ => return e.skip(true),
		};
		e.skip(false);
		let Some(row) = v.selected_row() else {
			return;
		};
		let Some(at) = v.at(&a, row) else {
			return;
		};
		if shift && !matches!(row, Row::Folder(_)) {
			return;
		}
		match (act, row, &at) {
			(
				0,
				_,
				At {
					station: Some(st), ..
				},
			) => v.remove_station(&a, &at, st),
			(0, Row::Folder(_), _) => v.delete(&a, row, &at),
			(0, _, _) => v.delete(&a, row, &at),
			(1, _, _) => v.rename(&a, row, &at),
			(2 | 3, Row::Station(_), _) => v.move_station(&a, row, &at, act == 2),
			(2 | 3, Row::Folder(_), _) => v.move_folder(&a, &at, act == 2),
			(2 | 3, _, _) => v.move_collection(&a, &at, act == 2),
			_ => {}
		}
	});

	// The Applications key, Shift+F10 and a right click.
	let (v, a) = (view.clone(), app.clone());
	tree.bind_internal(EventType::CONTEXT_MENU, move |e: Event| {
		e.skip(false);
		v.menu(&a);
	});
}

/// Import Playlist and Import Playlist as Collection: the file, then the report.
pub fn import(app: &Rc<RefCell<App>>, as_collection: bool) {
	let parent = app.borrow().win.frame;
	let file = FileDialog::builder(&parent)
		.with_message(s::IMPORT_TITLE)
		.with_wildcard(s::PLAYLISTS_FILTER)
		.with_style(FileDialogStyle::Open | FileDialogStyle::FileMustExist)
		.build();
	if file.show_modal() != ID_OK {
		return;
	}
	let Some(path) = file.get_path() else {
		return;
	};
	let path = Path::new(&path);
	let target = match as_collection {
		true => import::ImportTarget::Collection(
			path.file_stem()
				.and_then(|n| n.to_str())
				.unwrap_or_default()
				.to_string(),
		),
		false => import::ImportTarget::Custom,
	};
	let said = {
		let mut a = app.borrow_mut();
		match import::import_playlist(&mut a.library, path, target) {
			Ok(report) => {
				a.save_library();
				a.refresh_dial();
				report.message()
			}
			Err(e) => {
				eprintln!("import: {e}");
				s::NOT_SAVED.to_string()
			}
		}
	};
	app.borrow_mut().say(&said);
}

#[cfg(test)]
mod tests {
	use super::*;

	fn station(id: &str) -> Station {
		Station {
			id: id.into(),
			name: id.into(),
			url: format!("http://{id}"),
			source: blindradio_core::station::Source::Custom,
			info: None,
		}
	}

	#[test]
	fn collections_tree_order() {
		let mut lib = Library::default();
		let jazz = lib.create_collection("Jazz");
		let news = lib.create_collection("News");
		lib.toggle_in_collection(&news, &station("a"));
		let names: Vec<&str> = ordered(&lib).iter().map(|c| c.name.as_str()).collect();
		assert_eq!(names, ["Custom Stations", "Jazz", "News"]);
		assert!(ordered(&lib)[0].id == CUSTOM, "Custom Stations stays first");
		assert_eq!(
			ordered(&lib)[2].stations.len(),
			1,
			"stations stay with theirs"
		);
		assert_eq!(jazz, lib.collections[1].id);
	}

	#[test]
	fn custom_cannot_change_message() {
		assert_eq!(
			cannot_change(CUSTOM),
			Some("Custom Stations cannot be changed")
		);
		assert_eq!(cannot_change("col:1"), None);
		let mut lib = Library::default();
		let jazz = lib.create_collection("Jazz");
		assert_eq!(cannot_change(&jazz), None);
	}

	#[test]
	fn add_all_offers_other_collections_only() {
		let mut lib = Library::default();
		let jazz = lib.create_collection("Jazz");
		let news = lib.create_collection("News");
		assert_eq!(
			others(&lib, &jazz),
			vec![(news, "News".to_string())],
			"not itself, not Custom Stations"
		);
		assert_eq!(others(&lib, CUSTOM).len(), 2);
	}
}
