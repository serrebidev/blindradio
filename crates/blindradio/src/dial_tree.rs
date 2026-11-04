//! The main window's station tree: the startup list with its folders, where
//! the flat list used to drop them. Rows carry the station's index into the
//! flat dial order, so tuning, Play On Move and the menus behave exactly as
//! they did over the list. Letters typed in the tree jump to stations by
//! their first letters (the tree's own navigation); filtering lives in the
//! main window's separate filter box.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;

use blindradio_core::library::{CUSTOM, Folder, Library, LibraryError};
use blindradio_core::station::Station;
use wx_utils::{confirm, prompt_text};
use wxdragon::event::{Event, EventType, WxEvtHandler};
use wxdragon::prelude::*;

use crate::action::Action;
use crate::app::{App, dial_collection};
use crate::context_menu::{self, Target};
use crate::main_window::dispatch;
use crate::menus::plain;
use crate::strings as s;

/// A folder row's place in the library: its collection and index path. An
/// empty path is the collection itself, at the top of the All Lists view.
pub type FolderHome = (String, Vec<usize>);

/// What a tree row is. A station row carries its index into the flat dial
/// order. A folder row carries its name path, for keeping it collapsed or
/// expanded across rebuilds, and its place: names can repeat on a level, and
/// Delete must take the row the user is on.
#[derive(Clone, PartialEq, Eq)]
enum Row {
	Folder { key: String, home: FolderHome },
	Station(usize),
}

/// The tree structure the app hands over: folders of folders, with station
/// rows as flat-order indices.
pub enum DialNode {
	Folder {
		name: String,
		home: FolderHome,
		kids: Vec<DialNode>,
	},
	Station(usize),
}

/// What the main window does on Delete in the tree.
#[derive(Default)]
pub struct DialHooks {
	/// The focused station's flat index.
	pub on_delete: Option<Rc<dyn Fn(usize)>>,
}

#[derive(Clone)]
pub struct DialTree {
	pub label: StaticText,
	pub list: TreeCtrl,
	root: TreeItemId,
	stations: Rc<RefCell<Rc<[Station]>>>,
	/// Station rows in tree order: item and flat index.
	rows: Rc<RefCell<Vec<(TreeItemId, usize)>>>,
	/// Folder rows and their places, so a rebuild can find the focused one again.
	folders: Rc<RefCell<Vec<(TreeItemId, FolderHome)>>>,
	/// Where the next rebuild puts focus, for a change that moves it off the
	/// focused row's place. Set before the rebuild, so focus lands once.
	landing: Rc<RefCell<Option<Row>>>,
	/// Folder path keys whose open state differs from the default:
	/// collapsed ones while the default is expanded, opened ones
	/// while it is collapsed. Toggling a folder by hand records the
	/// exception, so its state sticks across rebuilds either way.
	overrides: Rc<RefCell<HashSet<String>>>,
	/// The default open state for folders: Settings' "Expand all
	/// directories" — on, every folder opens; off, they start shut.
	expand_default: Rc<Cell<bool>>,
	/// Set while the program changes the selection: only the user's moves tune.
	quiet: Rc<Cell<bool>>,
}

impl DialTree {
	/// The label first: wxMSW names the tree after it.
	pub fn new(parent: &impl WxWidget, label: &str) -> DialTree {
		let label = StaticText::builder(parent).with_label(label).build();
		let list = TreeCtrl::builder(parent)
			.with_style(
				TreeCtrlStyle::HasButtons
					| TreeCtrlStyle::LinesAtRoot
					| TreeCtrlStyle::HideRoot
					| TreeCtrlStyle::Single,
			)
			.build();
		let root = list.add_root("", None, None).expect("tree root");
		DialTree {
			label,
			list,
			root,
			stations: Rc::new(RefCell::new(Rc::from([]))),
			rows: Rc::new(RefCell::new(Vec::new())),
			folders: Rc::new(RefCell::new(Vec::new())),
			landing: Rc::new(RefCell::new(None)),
			overrides: Rc::new(RefCell::new(HashSet::new())),
			expand_default: Rc::new(Cell::new(true)),
			quiet: Rc::new(Cell::new(false)),
		}
	}

	pub fn stations(&self) -> Rc<[Station]> {
		self.stations.borrow().clone()
	}

	fn row(&self, item: &TreeItemId) -> Option<Row> {
		self.list
			.get_custom_data_direct(item)?
			.downcast_ref::<Row>()
			.cloned()
	}

	/// The flat index of the selected station row; a folder selects nothing.
	pub fn current(&self) -> Option<usize> {
		let item = self.list.get_selection()?;
		match self.row(&item) {
			Some(Row::Station(i)) => Some(i),
			_ => None,
		}
	}

	/// The selected row itself, folder or station: the Applications
	/// key acts on the row, not only on stations.
	fn selected_row(&self) -> Option<Row> {
		let item = self.list.get_selection()?;
		self.row(&item)
	}

	/// Whether a folder or collection row is selected.
	pub fn on_folder(&self) -> bool {
		matches!(self.selected_row(), Some(Row::Folder { .. }))
	}

	/// The list the focused row stands in, as a collection and index path: a
	/// folder row's own, a station row's parent folder, else the dial
	/// collection's root. The same station can be filed in several lists, and
	/// only the row knows which copy is focused. None when the tree holds
	/// no structure to read: nothing selected, or a root station of the
	/// merged All Lists view.
	pub fn selected_home(
		&self,
		lib: &Library,
		main_list: Option<&str>,
	) -> Option<(String, Vec<usize>)> {
		let item = self.list.get_selection()?;
		let row = self.row(&item)?;
		let parent = || self.row(&self.list.get_item_parent(&item)?);
		match if matches!(row, Row::Folder { .. }) {
			Some(row)
		} else {
			parent()
		} {
			Some(Row::Folder { home, .. }) => Some(home),
			_ => dial_collection(lib, main_list).map(|c| (c.id.clone(), Vec::new())),
		}
	}

	/// The selected station's index among the stations on its level, which
	/// lead the level in flat order. None on a folder row.
	pub fn selected_level_index(&self) -> Option<usize> {
		let item = self.list.get_selection()?;
		let Row::Station(i) = self.row(&item)? else {
			return None;
		};
		let parent = self.list.get_item_parent(&item)?;
		let (first, _) = self.list.get_first_child(&parent)?;
		let Row::Station(start) = self.row(&first)? else {
			return None;
		};
		Some(i - start)
	}

	/// The next rebuild focuses the folder at this place.
	pub fn land_on_folder(&self, home: FolderHome) {
		*self.landing.borrow_mut() = Some(Row::Folder {
			key: String::new(),
			home,
		});
	}

	fn item_of(&self, index: usize) -> Option<TreeItemId> {
		self.rows
			.borrow()
			.iter()
			.find(|(_, i)| *i == index)
			.map(|(item, _)| item.clone())
	}

	fn item_of_row(&self, row: &Row) -> Option<TreeItemId> {
		match row {
			Row::Station(i) => self.item_of(*i),
			Row::Folder { home, .. } => self
				.folders
				.borrow()
				.iter()
				.find(|(_, h)| h == home)
				.map(|(item, _)| item.clone()),
		}
	}

	/// Rebuilds the tree from `nodes`; never tunes. Focus goes where a change
	/// asked it to, else a focused folder row's place stays focused, else
	/// focus lands on the station with flat index `focus`.
	pub fn set_dial(&self, nodes: &[DialNode], stations: Vec<Station>, focus: usize) {
		let land = self.landing.borrow_mut().take().or_else(|| {
			self.selected_row()
				.filter(|row| matches!(row, Row::Folder { .. }))
		});
		self.quiet.set(true);
		// wx deletes rows one by one and the native tree moves the selection onto the
		// next each time: a repaint and a focus event per row, which floods NVDA.
		self.list.freeze();
		self.list.unselect_all();
		self.list.delete_children(&self.root);
		self.rows.borrow_mut().clear();
		self.folders.borrow_mut().clear();
		*self.stations.borrow_mut() = stations.into();
		let stations = self.stations();
		for node in nodes {
			self.add_node(&self.root, node, &stations, "");
		}
		// The focus select is the program's too: the guard stays up
		// until after it. It used to drop before the select, whose
		// selection event then ran the handler mid-rebuild — on GTK
		// the event is synchronous, the handler borrowed the app
		// while the caller held it, and the startup selection
		// panicked (caught by the toolkit, the event lost).
		let landed = land.and_then(|row| self.item_of_row(&row));
		if let Some(item) = landed.or_else(|| self.item_of(focus)) {
			self.list.select_item(&item);
			self.list.set_focused_item(&item);
			self.list.ensure_visible(&item);
		}
		self.list.thaw();
		self.quiet.set(false);
	}

	fn add_node(&self, parent: &TreeItemId, node: &DialNode, stations: &[Station], path: &str) {
		match node {
			DialNode::Station(i) => {
				let Some(st) = stations.get(*i) else { return };
				// An imported playlist files a station under its group,
				// and the group name rides along as the info. Read right
				// after that folder's own name it says the same thing
				// twice, so the suffix goes when it only repeats the
				// folder (or the station itself).
				let folder = path.rsplit('/').next().filter(|_| !path.is_empty());
				let text = match st.info.as_deref().filter(|t| !t.is_empty()) {
					Some(info)
						if !folder.is_some_and(|p| info.eq_ignore_ascii_case(p))
							&& !info.eq_ignore_ascii_case(&st.name) =>
					{
						format!("{} - {}", st.name, info)
					}
					_ => st.name.clone(),
				};
				let Some(item) = self.list.append_item(parent, &text, None, None) else {
					return;
				};
				self.list.set_custom_data_direct(&item, Row::Station(*i));
				self.rows.borrow_mut().push((item, *i));
			}
			DialNode::Folder { name, home, kids } => {
				let key = if path.is_empty() {
					name.clone()
				} else {
					format!("{path}/{name}")
				};
				// The name carries how many stations live under it,
				// as in Browse ("English 24").
				let text = format!("{name} {}", count_nodes(kids));
				let Some(item) = self.list.append_item(parent, &text, None, None) else {
					return;
				};
				self.list.set_custom_data_direct(
					&item,
					Row::Folder {
						key: key.clone(),
						home: home.clone(),
					},
				);
				self.folders.borrow_mut().push((item.clone(), home.clone()));
				for kid in kids {
					self.add_node(&item, kid, stations, &key);
				}
				let open = self.expand_default.get()
					!= self.overrides.borrow().contains(&key);
				if open {
					self.list.expand(&item);
				}
			}
		}
	}

	/// The Settings default for folder state, applied on the next
	/// rebuild. Flipping it forgets the per-folder exceptions: they
	/// were recorded against the old default and invert under the
	/// new one.
	pub fn set_expand_default(&self, expand: bool) {
		if self.expand_default.get() != expand {
			self.expand_default.set(expand);
			self.overrides.borrow_mut().clear();
		}
	}

	/// A folder opened or closed by hand: record it as an exception
	/// to the default exactly when it differs from the default.
	fn note_state(&self, key: String, open: bool) {
		if open == self.expand_default.get() {
			self.overrides.borrow_mut().remove(&key);
		} else {
			self.overrides.borrow_mut().insert(key);
		}
	}

	/// Selects and focuses the station with flat index `index`; `tune`
	/// makes it a user move.
	pub fn select(&self, index: usize, tune: bool) {
		let Some(item) = self.item_of(index) else {
			return;
		};
		self.quiet.set(!tune);
		self.list.select_item(&item);
		self.list.set_focused_item(&item);
		self.list.ensure_visible(&item);
		self.quiet.set(false);
	}

	/// Opening the window or a search landing on it: the focused station
	/// plays at once, unless Play On Move is off — then it is only the
	/// selection.
	pub fn land(&self, app: &mut App) {
		if let Some(i) = self.current() {
			if app.settings.play_on_move {
				app.tune(&self.stations(), i);
			} else if let Some(station) = self.stations().get(i) {
				app.selected = Some(station.clone());
			}
		}
	}

	/// Where focus goes when the focused folder at `home` goes: the next
	/// folder takes its place, else the row before it, else its parent.
	fn row_after_delete(&self, home: &FolderHome) -> Option<Row> {
		let item = self.list.get_selection()?;
		if let Some(next) = self.list.get_next_sibling(&item) {
			// A folder after it slides into its index; a collection keeps its id.
			return if home.1.is_empty() {
				self.row(&next)
			} else {
				Some(Row::Folder {
					key: String::new(),
					home: home.clone(),
				})
			};
		}
		let near = self
			.list
			.get_prev_sibling(&item)
			.or_else(|| self.list.get_item_parent(&item))?;
		self.row(&near)
	}

	/// Rename on a folder row, or on a collection row in the All Lists view.
	fn rename_folder(&self, app: &Rc<RefCell<App>>, home: &FolderHome) {
		let label = if home.1.is_empty() {
			s::COLLECTION_NAME
		} else {
			s::FOLDER_NAME
		};
		let Some(name) = modal(app, || {
			prompt_text(&self.list, label, &plain(s::RENAME_ITEM))
		}) else {
			return;
		};
		let mut a = app.borrow_mut();
		if a.library.rename_at(&home.0, &home.1, &name) {
			a.save_library();
			a.refresh_dial();
		}
	}

	/// Delete on a folder row, or on a collection row in the All Lists view:
	/// the row goes with everything in it, after asking and naming how much.
	fn delete_folder(&self, app: &Rc<RefCell<App>>, home: &FolderHome) {
		if is_custom_root(home) {
			let text = LibraryError::CustomFixed.to_string();
			return app.borrow_mut().say(&text);
		}
		let Some((name, stations, folders)) = folder_export(&app.borrow().library, home) else {
			return;
		};
		let count = stations.len()
			+ folders
				.iter()
				.map(|f| f.all_stations().len())
				.sum::<usize>();
		let question = context_menu::delete_question(&name, count);
		if !modal(app, || {
			confirm(&self.list, &question, &plain(s::DELETE_ITEM))
		}) {
			return;
		}
		let land = self.row_after_delete(home);
		let mut a = app.borrow_mut();
		if a.library.remove_at(&home.0, &home.1) {
			a.save_library();
			*self.landing.borrow_mut() = land;
			a.refresh_dial();
			// Focus lands nearby without tuning, as it does in Browse.
			a.selected = self.current().and_then(|i| self.stations().get(i).cloned());
		}
	}

	/// Delete on a station row asks first, naming it, as a folder's does.
	fn ask_delete_station(&self, app: &Rc<RefCell<App>>, index: usize) -> bool {
		let Some(station) = self.stations().get(index).cloned() else {
			return false;
		};
		let question = context_menu::delete_question(&station.name, 0);
		modal(app, || {
			confirm(&self.list, &question, &plain(s::DELETE_ITEM))
		})
	}

	/// Handlers run with the app not borrowed; programmatic changes made
	/// while it is borrowed return before borrowing it (`quiet`).
	pub fn bind(&self, app: &Rc<RefCell<App>>, hooks: DialHooks) {
		let (a, me) = (app.clone(), self.clone());
		self.list.on_selection_changed(move |_| {
			if me.quiet.get() {
				return;
			}
			let Some(i) = me.current() else { return };
			let stations = me.stations();
			let mut app = a.borrow_mut();
			if app.settings.play_on_move {
				app.tune(&stations, i);
			} else if let Some(station) = stations.get(i) {
				app.selected = Some(station.clone());
			}
		});

		let (a, me) = (app.clone(), self.clone());
		self.list.on_item_activated(move |_| {
			let Some(st) = me
				.current()
				.and_then(|i| me.stations().get(i).cloned())
			else {
				return;
			};
			a.borrow_mut().selected = Some(st);
			dispatch(&a, Action::Play);
		});

		let (a, me) = (app.clone(), self.clone());
		let on_delete = hooks.on_delete.clone();
		self.list
			.bind_internal(EventType::KEY_DOWN, move |e: Event| {
				let Some(code) = e.get_key_code() else {
					return e.skip(true);
				};
				let shifted = !e.control_down() && (e.alt_down() || e.shift_down());
				let plain = !(e.control_down() || e.alt_down() || e.shift_down());
				match code {
					// As in Browse: Alt+Up and Alt+Down work too.
					WXK_UP | WXK_DOWN if shifted => {
						e.skip(false);
						a.borrow_mut().move_dial_row(code == WXK_UP);
					}
					WXK_DELETE if plain => match (me.selected_row(), &on_delete) {
						(Some(Row::Folder { home, .. }), _) => {
							e.skip(false);
							me.delete_folder(&a, &home);
						}
						(Some(Row::Station(i)), Some(delete)) => {
							e.skip(false);
							if me.ask_delete_station(&a, i) {
								delete(i);
							}
						}
						_ => e.skip(true),
					},
					_ => e.skip(true),
				}
			});

		// Collapsing or expanding a folder sticks across rebuilds.
		let me = self.clone();
		self.list.on_item_collapsed(move |e| {
			if let Some(item) = e.get_item()
				&& let Some(Row::Folder { key, .. }) = me.row(&item)
			{
				me.note_state(key, false);
			}
		});
		let me = self.clone();
		self.list.on_item_expanding(move |e| {
			if let Some(item) = e.get_item()
				&& let Some(Row::Folder { key, .. }) = me.row(&item)
			{
				me.note_state(key, true);
			}
		});

		// The Applications key, Shift+F10 and a right click.
		let (a, me) = (app.clone(), self.clone());
		self.list
			.bind_internal(EventType::CONTEXT_MENU, move |e: Event| {
				e.skip(false);
				// A folder row is the user's own, as in Browse: it pastes,
				// renames, deletes and exports.
				if let Some(Row::Folder { home, .. }) = me.selected_row() {
					let (data, has_clip) = {
						let held = a.borrow();
						(
							folder_export(&held.library, &home),
							held.station_clip.is_some(),
						)
					};
					match data {
						Some((name, stations, folders)) => {
							let mut items = Vec::new();
							// Paste lands at this row's own level: on a
							// collection row, beside its folders.
							if has_clip {
								items.push(context_menu::Item::PasteStation);
							}
							if !is_custom_root(&home) {
								items.extend([
									context_menu::Item::Rename,
									context_menu::Item::Delete,
								]);
							}
							items.push(context_menu::Item::Export);
							match context_menu::popup(&me.list, &items) {
								Some(context_menu::Choice::Export) => {
									context_menu::export_playlist(
										&a, &me.list, &name, &stations, &folders,
									);
								}
								Some(context_menu::Choice::PasteStation) => {
									let (id, path) = home;
									a.borrow_mut().paste_into(
										crate::app::PasteDest::Collection(id, path),
									);
								}
								Some(context_menu::Choice::Rename) => {
									me.rename_folder(&a, &home);
								}
								Some(context_menu::Choice::Delete) => {
									me.delete_folder(&a, &home);
								}
								_ => {}
							}
						}
						None => a.borrow_mut().say(s::NOTHING_TO_DO),
					}
					return;
				}
				let focused = me
					.current()
					.and_then(|i| me.stations().get(i).cloned().map(|st| (i, st)));
				let target = match &focused {
					Some((_, st)) => Target::Station(st),
					None => Target::Other,
				};
				let mut items = context_menu::items_for(
					&target,
					&a.borrow().library,
					a.borrow().station_clip.is_some(),
				);
				// Every row here is filed in a list, so it can leave it as the Delete key
				// does. In Custom Stations, Delete Custom Station already says that.
				let in_custom = {
					let held = a.borrow();
					me.selected_home(&held.library, held.settings.main_list.as_deref())
						.is_some_and(|(id, _)| id == CUSTOM)
				};
				if let Ok(items) = &mut items
					&& !in_custom
				{
					items.push(context_menu::Item::Delete);
				}
				match (items, focused) {
					(Ok(items), Some((i, st))) => match context_menu::popup(&me.list, &items) {
						Some(context_menu::Choice::Delete) => {
							if me.ask_delete_station(&a, i) {
								a.borrow_mut().delete_from_dial(i);
							}
						}
						Some(choice) => context_menu::apply(&a, &me.list, &st, choice),
						None => {}
					},
					(Err(text), _) => a.borrow_mut().say(text),
					(Ok(_), None) => {}
				}
			});
	}
}

/// Runs a modal with the app told a dialog is up, and not borrowed: the
/// poll timer borrows it meanwhile.
fn modal<R>(app: &Rc<RefCell<App>>, show: impl FnOnce() -> R) -> R {
	app.borrow_mut().dialog_open = true;
	let r = show();
	app.borrow_mut().dialog_open = false;
	r
}

/// Custom Stations' own row in the All Lists view: it stays.
fn is_custom_root(home: &FolderHome) -> bool {
	home.1.is_empty() && home.0 == CUSTOM
}

/// What a dial folder row holds, as (name, loose stations, folders): the
/// folder itself, or — at the top of the All Lists view, where a folder row
/// is a whole collection — everything in it.
fn folder_export(
	lib: &Library,
	(id, path): &FolderHome,
) -> Option<(String, Vec<Station>, Vec<Folder>)> {
	let c = lib.collections.iter().find(|c| c.id == *id)?;
	if path.is_empty() {
		return Some((c.name.clone(), c.stations.clone(), c.folders.clone()));
	}
	let f = c.folder_at(path)?;
	Some((f.name.clone(), f.stations.clone(), f.folders.clone()))
}

/// How many stations live under a folder node, its subfolders included.
fn count_nodes(kids: &[DialNode]) -> usize {
	kids.iter()
		.map(|k| match k {
			DialNode::Station(_) => 1,
			DialNode::Folder { kids, .. } => count_nodes(kids),
		})
		.sum()
}
