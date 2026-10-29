//! Favorites (Ctrl+B): a tree of the favorites root and its folders —
//! remove, reorder, move between folders, and export.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::path::Path;
use std::rc::Rc;

use blindradio_core::import;
use blindradio_core::library::Folder;
use blindradio_core::station::Station;
use wx_utils::{confirm, prompt_text};
use wxdragon::event::{Event, EventType, WxEvtHandler};
use wxdragon::prelude::*;

use crate::action::Action;
use crate::app::App;
use crate::bridge::WindowToken;
use crate::context_menu::{self, Choice, Item};
use crate::main_window::dispatch;
use crate::menus::plain;
use crate::station_list::letter_jump;
use crate::strings as s;

/// Where focus lands after removing `index` from a list that had `len` items.
pub fn focus_after_remove(len: usize, index: usize) -> Option<usize> {
	(len > 1).then(|| index.min(len - 2))
}

/// A row in the tree; both kinds index into the view's location table.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Row {
	Folder(usize),
	Station(usize),
}

/// Where a row lives: a folder path (empty is the favorites root) and, for
/// stations, the index in that folder's list.
#[derive(Clone, PartialEq, Eq)]
struct Loc {
	path: Vec<usize>,
	station: Option<usize>,
}

/// What a rebuild leaves focused.
#[derive(Clone, PartialEq, Eq)]
enum Focus {
	Folder(Vec<usize>),
	Station(String),
}

struct View {
	tree: TreeCtrl,
	root: TreeItemId,
	items: RefCell<Vec<(Row, TreeItemId)>>,
	locs: RefCell<Vec<Loc>>,
	expanded: RefCell<HashSet<String>>,
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
		}
	}

	fn row_for(&self, path: &[usize], station: Option<usize>) -> Option<Row> {
		self.locs
			.borrow()
			.iter()
			.position(|l| l.path == path && l.station == station)
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

	fn key(path: &[usize]) -> String {
		path.iter()
			.map(|i| i.to_string())
			.collect::<Vec<_>>()
			.join("/")
	}

	fn refresh(&self, app: &Rc<RefCell<App>>, focus: Option<Focus>) {
		let mut landed: Option<TreeItemId> = None;
		let mut landed_station: Option<Station> = None;
		self.quietly(|| {
			self.tree.delete_children(&self.root);
			self.items.borrow_mut().clear();
			self.locs.borrow_mut().clear();
			let lib = app.borrow();
			self.level(
				&self.root.clone(),
				&[],
				&lib.library.favorites,
				&lib.library.fav_folders,
				&focus,
				&mut landed,
				&mut landed_station,
			);
			drop(lib);
			if let Some(item) = &landed {
				self.tree.select_item(item);
				self.tree.set_focused_item(item);
				self.tree.ensure_visible(item);
			}
		});
		app.borrow_mut().selected = landed_station;
	}

	#[allow(clippy::too_many_arguments)]
	fn level(
		&self,
		parent: &TreeItemId,
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
				path: path.to_vec(),
				station: Some(si),
			});
			self.tree
				.set_custom_data_direct(&child, Row::Station(li));
			self.items.borrow_mut().push((Row::Station(li), child.clone()));
			if *focus == Some(Focus::Station(st.id.clone())) {
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
				path: fpath.clone(),
				station: None,
			});
			self.tree.set_custom_data_direct(&child, Row::Folder(li));
			self.items.borrow_mut().push((Row::Folder(li), child.clone()));
			if *focus == Some(Focus::Folder(fpath.clone())) {
				*landed = Some(child.clone());
			}
			self.level(
				&child,
				&fpath,
				&f.stations,
				&f.folders,
				focus,
				landed,
				landed_station,
			);
			if self.expanded.borrow().contains(&Self::key(&fpath)) {
				self.tree.expand(&child);
			}
		}
		if !was_landed && landed.is_some() {
			self.tree.expand(parent);
		}
	}

	fn station_at(&self, app: &Rc<RefCell<App>>, row: Row) -> Option<Station> {
		let loc = self.loc(row)?;
		let lib = app.borrow();
		lib.library
			.fav_stations_at(&loc.path)?
			.get(loc.station?)
			.cloned()
	}

	fn level_of(&self, app: &Rc<RefCell<App>>, row: Row) -> Option<(Vec<Station>, usize)> {
		let loc = self.loc(row)?;
		let lib = app.borrow();
		let stations = lib.library.fav_stations_at(&loc.path)?.to_vec();
		Some((stations, loc.station?))
	}

	/// Opening the window: the first favorite is the landing station.
	fn land(&self, app: &Rc<RefCell<App>>) {
		let first = self
			.locs
			.borrow()
			.iter()
			.position(|l| l.station.is_some())
			.map(Row::Station);
		if let Some(row) = first
			&& let Some((stations, si)) = self.level_of(app, row)
		{
			app.borrow_mut().focus_station(&stations, si);
		}
	}

	fn new_folder(&self, app: &Rc<RefCell<App>>, parent: &[usize]) {
		let Some(name) = prompt_text(&self.tree, s::FOLDER_NAME, s::NEW_FOLDER_TITLE) else {
			return;
		};
		let made = {
			let mut a = app.borrow_mut();
			let made = a.library.add_fav_folder(parent, &name);
			if made.is_some() {
				a.save_library();
			}
			made
		};
		if let Some(path) = made {
			self.refresh(app, Some(Focus::Folder(path)));
		}
	}

	fn rename_folder(&self, app: &Rc<RefCell<App>>, path: &[usize]) {
		let Some(name) = prompt_text(&self.tree, s::FOLDER_NAME, &plain(s::RENAME_ITEM)) else {
			return;
		};
		{
			let mut a = app.borrow_mut();
			if a.library.rename_fav_folder(path, &name) {
				a.save_library();
			}
		}
		self.refresh(app, Some(Focus::Folder(path.to_vec())));
	}

	fn remove(&self, app: &Rc<RefCell<App>>, row: Row) {
		let Some(loc) = self.loc(row) else {
			return;
		};
		match (row, loc.station) {
			(Row::Station(_), Some(_)) => {
				let Some(st) = self.station_at(app, row) else {
					return;
				};
				let remaining: Vec<Station> = self
					.level_of(app, row)
					.map(|(stations, _)| {
						stations.into_iter().filter(|x| x.id != st.id).collect()
					})
					.unwrap_or_default();
				let focus = focus_after_remove(remaining.len() + 1, loc.station.unwrap_or(0));
				let next = focus
					.and_then(|i| remaining.get(i))
					.map(|s| s.id.clone());
				{
					let mut a = app.borrow_mut();
					a.library.remove_favorite(&st.id);
					a.save_library();
					a.say(s::REMOVED_FROM_FAVORITES);
				}
				self.refresh(app, next.map(Focus::Station));
			}
			_ => {
				let name = {
					let lib = app.borrow();
					lib.library
						.fav_folder_at(&loc.path)
						.map(|f| f.name.clone())
				};
				let Some(name) = name else {
					return;
				};
				let question = format!("{}{}?", s::DELETE_PREFIX, name);
				if !confirm(&self.tree, &question, &plain(s::DELETE_ITEM)) {
					return;
				}
				{
					let mut a = app.borrow_mut();
					if a.library.remove_fav_folder(&loc.path) {
						a.save_library();
					}
				}
				let parent: Vec<usize> = loc.path[..loc.path.len().saturating_sub(1)].to_vec();
				self.refresh(
					app,
					(!parent.is_empty()).then_some(Focus::Folder(parent)),
				);
			}
		}
	}

	fn move_row(&self, app: &Rc<RefCell<App>>, row: Row, up: bool) {
		let Some(loc) = self.loc(row) else {
			return;
		};
		match (row, loc.station) {
			(Row::Station(_), Some(si)) => {
				let to = if up { si.saturating_sub(1) } else { si + 1 };
				let station = self.station_at(app, row);
				{
					let mut a = app.borrow_mut();
					if let Some(list) = a.library.fav_stations_at_mut(&loc.path)
						&& to < list.len()
						&& to != si
					{
						list.swap(si, to);
						a.save_library();
					}
				}
				self.refresh(app, station.map(|st| Focus::Station(st.id)));
			}
			_ => {
				let Some((&last, parent)) = loc.path.split_last() else {
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
					match a.library.fav_folder_list_at_mut(&parent) {
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
				self.refresh(app, Some(Focus::Folder(path)));
			}
		}
	}

	fn move_station_to(&self, app: &Rc<RefCell<App>>, station: &Station, to_path: &[usize]) {
		{
			let mut a = app.borrow_mut();
			a.library.remove_favorite(&station.id);
			if let Some(list) = a.library.fav_stations_at_mut(to_path)
				&& !list.iter().any(|x| x.id == station.id)
			{
				list.push(station.clone());
			}
			a.save_library();
		}
		self.refresh(app, Some(Focus::Station(station.id.clone())));
	}

	fn export(&self, app: &Rc<RefCell<App>>) {
		let (stations, folders, count) = {
			let lib = app.borrow();
			(
				lib.library.favorites.clone(),
				lib.library.fav_folders.clone(),
				lib.library.all_favorites().len(),
			)
		};
		let file = FileDialog::builder(&self.tree)
			.with_message(s::EXPORT)
			.with_default_file(s::FAVORITES_FILE)
			.with_wildcard(s::PLAYLISTS_FILTER)
			.with_style(FileDialogStyle::Save | FileDialogStyle::OverwritePrompt)
			.build();
		if file.show_modal() != ID_OK {
			return;
		}
		let Some(path) = file.get_path() else {
			return;
		};
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
			Row::Station(_) => {
				let Some(station) = self.station_at(app, row) else {
					return;
				};
				let Some(loc) = self.loc(row) else {
					return;
				};
				let mut items = vec![Item::Play];
				{
					let lib = app.borrow();
					if !lib.library.fav_folders.is_empty() {
						let mut targets: Vec<(Vec<usize>, String)> =
							vec![(Vec::new(), s::FAVORITES_LIST.to_string())];
						targets.extend(context_menu::fav_folder_list(&lib.library));
						targets.retain(|(path, _)| *path != loc.path);
						items.push(Item::MoveToFolder(targets));
					}
				}
				items.push(Item::Delete);
				match context_menu::popup(&self.tree, &items) {
					Some(Choice::Play) => {
						app.borrow_mut().selected = Some(station);
						dispatch(app, Action::Play);
					}
					Some(Choice::MoveToFolder(path)) => {
						self.move_station_to(app, &station, &path)
					}
					Some(Choice::Delete) => self.remove(app, row),
					_ => {}
				}
			}
			Row::Folder(_) => {
				let Some(loc) = self.loc(row) else {
					return;
				};
				let items = vec![Item::Rename, Item::Delete];
				match context_menu::popup(&self.tree, &items) {
					Some(Choice::Rename) => self.rename_folder(app, &loc.path),
					Some(Choice::Delete) => self.remove(app, row),
					_ => {}
				}
			}
		}
	}

	/// A letter: the next row on the focused level starting with it.
	fn jump(&self, app: &Rc<RefCell<App>>, row: Row, c: char) -> Option<Row> {
		let loc = self.loc(row)?;
		let lib = app.borrow();
		match (row, loc.station) {
			(Row::Station(_), Some(si)) => {
				let names: Vec<&str> = lib
					.library
					.fav_stations_at(&loc.path)?
					.iter()
					.map(|s| s.name.as_str())
					.collect();
				letter_jump(&names, Some(si), c)
					.and_then(|to| self.row_for(&loc.path, Some(to)))
			}
			_ => {
				let parent = &loc.path[..loc.path.len() - 1];
				let siblings: &[Folder] = if parent.is_empty() {
					&lib.library.fav_folders
				} else {
					&lib.library.fav_folder_at(parent)?.folders
				};
				let names: Vec<&str> = siblings.iter().map(|f| f.name.as_str()).collect();
				letter_jump(&names, Some(*loc.path.last()?), c).and_then(|fi| {
					let mut path = parent.to_vec();
					path.push(fi);
					self.row_for(&path, None)
				})
			}
		}
	}
}

pub fn show(app: &Rc<RefCell<App>>) -> Option<Action> {
	let frame = app.borrow().win.frame;
	let dialog = Dialog::builder(&frame, s::FAVORITES_TITLE).build();
	// The label first: wxMSW names the tree after it.
	let label = StaticText::builder(&dialog)
		.with_label(s::FAVORITES_LIST)
		.build();
	let tree_ctrl = TreeCtrl::builder(&dialog)
		.with_style(
			TreeCtrlStyle::HasButtons
				| TreeCtrlStyle::LinesAtRoot
				| TreeCtrlStyle::HideRoot
				| TreeCtrlStyle::Single,
		)
		.build();
	let new_folder = Button::builder(&dialog).with_label(s::NEW_FOLDER).build();
	let remove = Button::builder(&dialog).with_label(s::REMOVE).build();
	let up = Button::builder(&dialog).with_label(s::MOVE_UP).build();
	let down = Button::builder(&dialog).with_label(s::MOVE_DOWN).build();
	let export = Button::builder(&dialog).with_label(s::EXPORT).build();
	let close = Button::builder(&dialog)
		.with_id(ID_CANCEL)
		.with_label(s::CLOSE)
		.build();
	let buttons = BoxSizer::builder(Orientation::Horizontal).build();
	for b in [&new_folder, &remove, &up, &down, &export, &close] {
		buttons.add(b, 0, SizerFlag::All, 5);
	}
	let sizer = BoxSizer::builder(Orientation::Vertical).build();
	sizer.add(
		&label,
		0,
		SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
		5,
	);
	sizer.add(&tree_ctrl, 1, SizerFlag::Expand | SizerFlag::All, 5);
	sizer.add_sizer(&buttons, 0, SizerFlag::All, 0);
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
	let first = app
		.borrow()
		.library
		.all_favorites()
		.first()
		.map(|st| Focus::Station(st.id.clone()));
	view.refresh(app, first);
	view.land(app);

	{
		let (v, a) = (view.clone(), app.clone());
		new_folder.on_click(move |_| {
			// On a folder row the new folder goes inside it; on a station
			// row it joins the station's folder; with nothing focused it
			// lands at the root.
			let parent = v
				.selected_row()
				.and_then(|row| v.loc(row))
				.map(|loc| loc.path)
				.unwrap_or_default();
			v.new_folder(&a, &parent);
		});
	}
	{
		let (v, a) = (view.clone(), app.clone());
		remove.on_click(move |_| {
			if let Some(row) = v.selected_row() {
				v.remove(&a, row);
			}
		});
	}
	for (button, up) in [(up, true), (down, false)] {
		let (v, a) = (view.clone(), app.clone());
		button.on_click(move |_| {
			if let Some(row) = v.selected_row() {
				v.move_row(&a, row, up);
			}
		});
	}
	{
		let (v, a) = (view.clone(), app.clone());
		export.on_click(move |_| v.export(&a));
	}

	let token = WindowToken::new();
	bind(&view, app, dialog, &token);

	view.tree.set_focus();
	crate::dialogs::run(app, dialog, Action::OpenFavorites, view.tree, &token)
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
			Row::Folder(_) => a.borrow_mut().selected = None,
		}
	});

	let v = view.clone();
	tree.on_item_expanding(move |e| {
		if let Some(loc) = e
			.get_item()
			.and_then(|item| v.row(&item))
			.and_then(|row| v.loc(row))
		{
			v.expanded.borrow_mut().insert(View::key(&loc.path));
		}
	});
	let v = view.clone();
	tree.on_item_collapsed(move |e| {
		if let Some(loc) = e
			.get_item()
			.and_then(|item| v.row(&item))
			.and_then(|row| v.loc(row))
		{
			v.expanded.borrow_mut().remove(&View::key(&loc.path));
		}
	});

	let (v, a) = (view.clone(), app.clone());
	tree.on_item_activated(move |e| {
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
			Some(Row::Folder(_)) => v.quietly(|| v.tree.toggle(&item)),
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
		match act {
			0 => v.remove(&a, row),
			1 => {
				if let Row::Folder(_) = row
					&& let Some(loc) = v.loc(row)
				{
					v.rename_folder(&a, &loc.path);
				}
			}
			2 | 3 => v.move_row(&a, row, act == 2),
			_ => {}
		}
	});

	let (v, a) = (view.clone(), app.clone());
	tree.bind_internal(EventType::CONTEXT_MENU, move |e: Event| {
		e.skip(false);
		v.menu(&a);
	});
}

#[cfg(test)]
mod tests {
	use super::focus_after_remove;

	#[test]
	fn favorites_delete_keeps_focus_and_audio() {
		// len before the removal; the station itself is never tuned by this.
		assert_eq!(focus_after_remove(3, 1), Some(1), "stays in place");
		assert_eq!(
			focus_after_remove(3, 2),
			Some(1),
			"the last one: the one before"
		);
		assert_eq!(focus_after_remove(2, 0), Some(0));
		assert_eq!(focus_after_remove(1, 0), None, "nothing left");
	}
}
