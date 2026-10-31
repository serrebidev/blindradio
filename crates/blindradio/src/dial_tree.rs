//! The main window's station tree: the startup list with its folders, where
//! the flat list used to drop them. Rows carry the station's index into the
//! flat dial order, so tuning, Play On Move and the menus behave exactly as
//! they did over the list. Letters typed in the tree jump to stations by
//! their first letters (the tree's own navigation); filtering lives in the
//! main window's separate filter box.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;

use blindradio_core::library::{Folder, Library};
use blindradio_core::station::Station;
use wxdragon::event::{Event, EventType, WxEvtHandler};
use wxdragon::prelude::*;

use crate::action::Action;
use crate::app::{ALL_LISTS, App, dial_collection};
use crate::context_menu::{self, Target};
use crate::main_window::dispatch;
use crate::strings as s;

/// What a tree row is. A station row carries its index into the flat dial
/// order; a folder row carries its path key, for keeping it collapsed or
/// expanded across rebuilds.
#[derive(Clone, PartialEq, Eq)]
enum Row {
	Folder(String),
	Station(usize),
}

/// The tree structure the app hands over: folders of folders, with station
/// rows as flat-order indices.
pub enum DialNode {
	Folder(String, Vec<DialNode>),
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
	pub fn selected_row(&self) -> Option<Row> {
		let item = self.list.get_selection()?;
		self.row(&item)
	}

	fn item_of(&self, index: usize) -> Option<TreeItemId> {
		self.rows
			.borrow()
			.iter()
			.find(|(_, i)| *i == index)
			.map(|(item, _)| item.clone())
	}

	/// Rebuilds the tree from `nodes`; never tunes. Focus lands on the
	/// station with flat index `focus`.
	pub fn set_dial(&self, nodes: &[DialNode], stations: Vec<Station>, focus: usize) {
		self.quiet.set(true);
		self.list.delete_children(&self.root);
		self.rows.borrow_mut().clear();
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
		if let Some(item) = self.item_of(focus) {
			self.list.select_item(&item);
			self.list.set_focused_item(&item);
			self.list.ensure_visible(&item);
		}
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
			DialNode::Folder(name, kids) => {
				let key = if path.is_empty() {
					name.clone()
				} else {
					format!("{path}/{name}")
				};
				let Some(item) = self.list.append_item(parent, name, None, None) else {
					return;
				};
				self.list
					.set_custom_data_direct(&item, Row::Folder(key.clone()));
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

		let me = self.clone();
		let on_delete = hooks.on_delete.clone();
		self.list
			.bind_internal(EventType::KEY_DOWN, move |e: Event| {
				let Some(code) = e.get_key_code() else {
					return e.skip(true);
				};
				let plain = !(e.control_down() || e.alt_down() || e.shift_down());
				match code {
					WXK_DELETE if plain => match (me.current(), &on_delete) {
						(Some(i), Some(delete)) => {
							e.skip(false);
							delete(i);
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
				&& let Some(Row::Folder(key)) = me.row(&item)
			{
				me.note_state(key, false);
			}
		});
		let me = self.clone();
		self.list.on_item_expanding(move |e| {
			if let Some(item) = e.get_item()
				&& let Some(Row::Folder(key)) = me.row(&item)
			{
				me.note_state(key, true);
			}
		});

		// The Applications key, Shift+F10 and a right click.
		let (a, me) = (app.clone(), self.clone());
		self.list
			.bind_internal(EventType::CONTEXT_MENU, move |e: Event| {
				e.skip(false);
				// A folder row exports, like Browse's folders do; the
				// main window used to offer a folder nothing at all.
				if let Some(Row::Folder(key)) = me.selected_row() {
					let data = {
						let held = a.borrow();
						folder_export(&held.library, held.settings.main_list.as_deref(), &key)
					};
					match data {
						Some((name, stations, folders)) => {
							let items = [context_menu::Item::Export];
							if let Some(context_menu::Choice::Export) =
								context_menu::popup(&me.list, &items)
							{
								context_menu::export_playlist(
									&a, &me.list, &name, &stations, &folders,
								);
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
				let items = context_menu::items_for(
					&target,
					&a.borrow().library,
					a.borrow().station_clip.is_some(),
				);
				match (items, focused) {
					(Ok(items), Some((_, st))) => {
						if let Some(choice) = context_menu::popup(&me.list, &items) {
							context_menu::apply(&a, &me.list, &st, choice);
						}
					}
					(Err(text), _) => a.borrow_mut().say(text),
					(Ok(_), None) => {}
				}
			});
	}
}

/// A folder found by walking names, level by level.
fn folder_by_names<'a>(folders: &'a [Folder], names: &[&str]) -> Option<&'a Folder> {
	let (first, rest) = names.split_first()?;
	let folder = folders.iter().find(|f| f.name == *first)?;
	if rest.is_empty() {
		Some(folder)
	} else {
		folder_by_names(&folder.folders, rest)
	}
}

/// What Export writes for a dial folder row, as (name, loose stations,
/// folders): the folder itself, or — at the top of the All Lists view,
/// where a folder row is a whole collection — everything in it.
fn folder_export(
	lib: &Library,
	main_list: Option<&str>,
	key: &str,
) -> Option<(String, Vec<Station>, Vec<Folder>)> {
	let names: Vec<&str> = key.split('/').collect();
	if main_list == Some(ALL_LISTS) {
		let collection = lib.collections.iter().find(|c| c.name == names[0])?;
		if names.len() == 1 {
			return Some((
				collection.name.clone(),
				collection.stations.clone(),
				collection.folders.clone(),
			));
		}
		let folder = folder_by_names(&collection.folders, &names[1..])?;
		return Some((folder.name.clone(), Vec::new(), vec![folder.clone()]));
	}
	let collection = dial_collection(lib, main_list)?;
	let folder = folder_by_names(&collection.folders, &names)?;
	Some((folder.name.clone(), Vec::new(), vec![folder.clone()]))
}
