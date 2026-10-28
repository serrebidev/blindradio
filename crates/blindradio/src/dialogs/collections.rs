//! Collections (Ctrl+L): Custom Stations and every collection in one tree.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::path::Path;
use std::rc::Rc;

use blindradio_core::import;
use blindradio_core::library::{CUSTOM, Collection, Library};
use blindradio_core::station::Station;
use wx_utils::{confirm, prompt_text};
use wxdragon::event::{Event, EventType, WxEvtHandler};
use wxdragon::prelude::*;

use crate::action::Action;
use crate::app::App;
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

/// A row in the tree.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Row {
	Collection(usize),
	Station(usize, usize),
}

/// The collection of a focused row, and the station when one is focused.
struct At {
	id: String,
	name: String,
	station: Option<String>,
}

struct View {
	tree: TreeCtrl,
	root: TreeItemId,
	items: RefCell<Vec<(Row, TreeItemId)>>,
	/// Collections left open, by id: a rebuild keeps them open.
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

	fn quietly<R>(&self, f: impl FnOnce() -> R) -> R {
		let was = self.quiet.replace(true);
		let r = f();
		self.quiet.set(was);
		r
	}

	/// Rebuilds the tree from the library; `focus` is selected without tuning.
	fn refresh(&self, app: &Rc<RefCell<App>>, focus: Option<Row>) {
		self.quietly(|| {
			self.tree.delete_children(&self.root);
			self.items.borrow_mut().clear();
			for (ci, c) in ordered(&app.borrow().library).iter().enumerate() {
				let Some(item) = self.tree.append_item(&self.root, &c.name, None, None) else {
					continue;
				};
				self.tree.set_custom_data_direct(&item, Row::Collection(ci));
				self.items
					.borrow_mut()
					.push((Row::Collection(ci), item.clone()));
				for (si, st) in c.stations.iter().enumerate() {
					let Some(child) = self.tree.append_item(&item, &st.name, None, None) else {
						continue;
					};
					self.tree
						.set_custom_data_direct(&child, Row::Station(ci, si));
					self.items.borrow_mut().push((Row::Station(ci, si), child));
				}
				// A focused station must be shown: open its collection.
				if matches!(focus, Some(Row::Station(fci, _)) if fci == ci) {
					self.expanded.borrow_mut().insert(c.id.clone());
				}
				if self.expanded.borrow().contains(&c.id) {
					self.tree.expand(&item);
				}
				if focus == Some(Row::Collection(ci)) {
					self.tree.select_item(&item);
					self.tree.set_focused_item(&item);
				}
			}
			if let Some(f) = focus
				&& let Some(item) = self.item_of(f)
			{
				self.tree.select_item(&item);
				self.tree.set_focused_item(&item);
				self.tree.ensure_visible(&item);
			}
		});
		// What Ctrl+D and Enter act on next.
		let selected = focus.and_then(|f| self.station_at(app, f));
		app.borrow_mut().selected = selected;
	}

	fn station_at(&self, app: &Rc<RefCell<App>>, row: Row) -> Option<Station> {
		let lib = app.borrow();
		let Row::Station(ci, si) = row else {
			return None;
		};
		ordered(&lib.library).get(ci)?.stations.get(si).cloned()
	}

	fn at(&self, app: &Rc<RefCell<App>>, row: Row) -> Option<At> {
		let lib = app.borrow();
		let tree = ordered(&lib.library);
		let ci = match row {
			Row::Collection(ci) | Row::Station(ci, _) => ci,
		};
		let c = tree.get(ci)?;
		let station = match row {
			Row::Station(_, si) => c.stations.get(si).map(|x| x.id.clone()),
			Row::Collection(_) => None,
		};
		Some(At {
			id: c.id.clone(),
			name: c.name.clone(),
			station,
		})
	}

	/// Focus goes to the collection with this id.
	fn show_collection(&self, app: &Rc<RefCell<App>>, id: &str) {
		let ci = ordered(&app.borrow().library)
			.iter()
			.position(|c| c.id == id);
		self.refresh(app, ci.map(Row::Collection));
	}

	/// Focus goes to this station.
	fn show_station(&self, app: &Rc<RefCell<App>>, collection: &str, station: &str) {
		let row = {
			let lib = app.borrow();
			ordered(&lib.library)
				.iter()
				.position(|c| c.id == collection)
				.and_then(|ci| {
					ordered(&lib.library)[ci]
						.stations
						.iter()
						.position(|x| x.id == station)
						.map(|si| Row::Station(ci, si))
				})
		};
		self.refresh(app, row);
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

	fn rename(&self, app: &Rc<RefCell<App>>, collection: &At) {
		if let Some(text) = cannot_change(&collection.id) {
			return app.borrow_mut().say(text);
		}
		let Some(name) = prompt_text(&self.tree, s::COLLECTION_NAME, &plain(s::RENAME_ITEM)) else {
			return;
		};
		{
			let mut a = app.borrow_mut();
			if a.library.rename_collection(&collection.id, &name).is_ok() {
				a.save_library();
			}
		}
		self.show_collection(app, &collection.id);
	}

	fn delete(&self, app: &Rc<RefCell<App>>, collection: &At) {
		if let Some(text) = cannot_change(&collection.id) {
			return app.borrow_mut().say(text);
		}
		let question = format!("{}{}?", s::DELETE_PREFIX, collection.name);
		if !confirm(&self.tree, &question, &plain(s::DELETE_ITEM)) {
			return;
		}
		{
			let mut a = app.borrow_mut();
			if a.library.delete_collection(&collection.id).is_ok() {
				a.save_library();
				a.refresh_dial();
			}
		}
		self.refresh(app, Some(Row::Collection(0)));
	}

	fn remove_station(&self, app: &Rc<RefCell<App>>, collection: &At, station: &str) {
		{
			let mut a = app.borrow_mut();
			a.library.remove_from_collection(&collection.id, station);
			a.save_library();
			a.refresh_dial();
		}
		self.show_collection(app, &collection.id);
	}

	fn move_station(&self, app: &Rc<RefCell<App>>, collection: &At, station: &str, up: bool) {
		let index = {
			let lib = app.borrow();
			ordered(&lib.library)
				.iter()
				.find(|c| c.id == collection.id)
				.and_then(|c| c.stations.iter().position(|x| x.id == station))
		};
		let Some(index) = index else {
			return;
		};
		{
			let mut a = app.borrow_mut();
			a.library.move_in_collection(&collection.id, index, up);
			a.save_library();
		}
		self.show_station(app, &collection.id, station);
	}

	fn move_collection(&self, app: &Rc<RefCell<App>>, collection: &At, up: bool) {
		if let Some(text) = cannot_change(&collection.id) {
			return app.borrow_mut().say(text);
		}
		{
			let mut a = app.borrow_mut();
			a.library.move_collection(&collection.id, up);
			a.save_library();
		}
		self.show_collection(app, &collection.id);
	}

	fn export(&self, app: &Rc<RefCell<App>>, collection: &At) {
		let stations = {
			let lib = app.borrow();
			ordered(&lib.library)
				.iter()
				.find(|c| c.id == collection.id)
				.map(|c| c.stations.clone())
				.unwrap_or_default()
		};
		let file = FileDialog::builder(&self.tree)
			.with_message(s::EXPORT)
			.with_default_file(&format!("{}{}", collection.name, s::M3U_EXTENSION))
			.with_wildcard(s::PLAYLISTS_FILTER)
			.with_style(FileDialogStyle::Save | FileDialogStyle::OverwritePrompt)
			.build();
		if file.show_modal() != ID_OK {
			return;
		}
		let Some(path) = file.get_path() else {
			return;
		};
		match import::export_m3u(&stations, Path::new(&path)) {
			Ok(()) => app
				.borrow_mut()
				.say(&format!("{}{}", s::EXPORTED, stations.len())),
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
			Row::Station(..) => self.station_menu(app),
			Row::Collection(..) => self.collection_menu(app, row),
		}
	}

	fn station_menu(&self, app: &Rc<RefCell<App>>) {
		let station = self
			.selected_row()
			.and_then(|row| self.station_at(app, row));
		let Some(station) = station else {
			return;
		};
		let items = {
			let lib = app.borrow();
			match context_menu::items_for(&Target::Station(&station), &lib.library) {
				Ok(items) => items,
				Err(text) => {
					drop(lib);
					return app.borrow_mut().say(text);
				}
			}
		};
		if let Some(choice) = context_menu::popup(&self.tree, &items) {
			context_menu::apply(app, &self.tree, &station, choice);
		}
		// Adding to or removing from a collection changes this tree.
		let row = self.selected_row();
		self.refresh(app, row);
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
		match context_menu::popup(&self.tree, &items) {
			Some(Choice::Rename) => self.rename(app, &collection),
			Some(Choice::Delete) => self.delete(app, &collection),
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
		let to = match row {
			Row::Station(ci, si) => {
				let names: Vec<&str> = tree
					.get(ci)?
					.stations
					.iter()
					.map(|s| s.name.as_str())
					.collect();
				letter_jump(&names, Some(si), c).map(|si| Row::Station(ci, si))
			}
			Row::Collection(ci) => {
				let names: Vec<&str> = tree.iter().map(|s| s.name.as_str()).collect();
				letter_jump(&names, Some(ci), c).map(Row::Collection)
			}
		};
		to.filter(|&to| to != row)
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
	for b in [&new, &rename, &delete, &remove, &close] {
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
		expanded: RefCell::new(HashSet::new()),
		quiet: Cell::new(false),
	});
	view.refresh(app, Some(Row::Collection(0)));

	// Buttons act on the focused row's collection.
	for (button, act) in [
		(new, None),
		(rename, Some(0)),
		(delete, Some(1)),
		(remove, Some(2)),
		(up, Some(3)),
		(down, Some(4)),
		(export, Some(5)),
	] {
		let (v, a) = (view.clone(), app.clone());
		button.on_click(move |_| {
			if act.is_none() {
				return v.new_collection(&a);
			}
			let Some(row) = v.selected_row() else {
				return;
			};
			let Some(at) = v.at(&a, row) else {
				return;
			};
			match (act, &at) {
				(Some(0), _) => v.rename(&a, &at),
				(Some(1), _) => v.delete(&a, &at),
				(
					Some(2),
					At {
						station: Some(st), ..
					},
				) => v.remove_station(&a, &at, st),
				(
					Some(3) | Some(4),
					At {
						station: Some(st), ..
					},
				) => {
					v.move_station(&a, &at, st, act == Some(3));
				}
				(Some(3) | Some(4), _) => v.move_collection(&a, &at, act == Some(3)),
				(Some(5), _) => v.export(&a, &at),
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
			Row::Station(ci, si) => {
				let stations = {
					let lib = a.borrow();
					ordered(&lib.library).get(ci).map(|c| c.stations.clone())
				};
				if let Some(stations) = stations {
					a.borrow_mut().tune(&stations, si);
				}
			}
			Row::Collection(..) => a.borrow_mut().selected = None,
		}
	});

	let (v, a) = (view.clone(), app.clone());
	tree.on_item_expanding(move |e| {
		let id = e
			.get_item()
			.and_then(|item| v.row(&item))
			.and_then(|row| v.at(&a, row))
			.map(|at| at.id);
		if let Some(id) = id {
			v.expanded.borrow_mut().insert(id);
		}
	});
	let (v, a) = (view.clone(), app.clone());
	tree.on_item_collapsed(move |e| {
		let id = e
			.get_item()
			.and_then(|item| v.row(&item))
			.and_then(|row| v.at(&a, row))
			.map(|at| at.id);
		if let Some(id) = id {
			v.expanded.borrow_mut().remove(&id);
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
			Some(Row::Station(ci, si)) => {
				let station = {
					let lib = a.borrow();
					ordered(&lib.library)
						.get(ci)
						.and_then(|c| c.stations.get(si))
						.cloned()
				};
				let Some(st) = station else {
					return;
				};
				a.borrow_mut().selected = Some(st);
				dispatch(&a, Action::Play);
				if a.borrow().settings.close_lists_when_playing {
					dialog.end_modal(ID_OK);
				}
			}
			Some(Row::Collection(..)) => v.quietly(|| v.tree.toggle(&item)),
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
		let act = match (code, plain, alt) {
			(WXK_DELETE, true, _) => 0,
			(WXK_F2, true, _) => 1,
			(WXK_UP, _, true) => 2,
			(WXK_DOWN, _, true) => 3,
			_ => return e.skip(true),
		};
		e.skip(false);
		let Some(row) = v.selected_row() else {
			return;
		};
		let Some(at) = v.at(&a, row) else {
			return;
		};
		match (act, &at) {
			(
				0,
				At {
					station: Some(st), ..
				},
			) => v.remove_station(&a, &at, st),
			(0, _) => v.delete(&a, &at),
			(1, _) => v.rename(&a, &at),
			(
				2 | 3,
				At {
					station: Some(st), ..
				},
			) => v.move_station(&a, &at, st, act == 2),
			(2 | 3, _) => v.move_collection(&a, &at, act == 2),
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
