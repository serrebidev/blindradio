//! The main window's station tree: the startup list with its folders, where
//! the flat list used to drop them. Rows carry the station's index into the
//! flat dial order, so tuning, Play On Move and the menus behave exactly as
//! they did over the list; typing filters the tree to matching stations.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;

use blindradio_core::station::Station;
use wxdragon::event::{Event, EventType, WxEvtHandler};
use wxdragon::prelude::*;

use crate::action::Action;
use crate::app::App;
use crate::context_menu::{self, Target};
use crate::main_window::dispatch;

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

/// What the main window does on Delete and while the user types a filter.
#[derive(Default)]
pub struct DialHooks {
	/// The focused station's flat index.
	pub on_delete: Option<Rc<dyn Fn(usize)>>,
	/// A letter typed in the tree: it extends the filter.
	pub on_type: Option<Rc<dyn Fn(char)>>,
	/// Backspace: the filter loses its last character.
	pub on_backspace: Option<Rc<dyn Fn()>>,
	/// Escape: the filter goes, the whole tree comes back.
	pub on_clear_filter: Option<Rc<dyn Fn()>>,
}

#[derive(Clone)]
pub struct DialTree {
	pub label: StaticText,
	pub list: TreeCtrl,
	root: TreeItemId,
	stations: Rc<RefCell<Rc<[Station]>>>,
	/// Station rows in tree order: item and flat index.
	rows: Rc<RefCell<Vec<(TreeItemId, usize)>>>,
	/// Folder path keys the user collapsed; everything else opens expanded.
	collapsed: Rc<RefCell<HashSet<String>>>,
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
			collapsed: Rc::new(RefCell::new(HashSet::new())),
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
		self.quiet.set(false);
		if let Some(item) = self.item_of(focus) {
			self.list.select_item(&item);
			self.list.set_focused_item(&item);
			self.list.ensure_visible(&item);
		}
	}

	fn add_node(&self, parent: &TreeItemId, node: &DialNode, stations: &[Station], path: &str) {
		match node {
			DialNode::Station(i) => {
				let Some(st) = stations.get(*i) else { return };
				let text = match st.info.as_deref().filter(|t| !t.is_empty()) {
					Some(info) => format!("{} - {}", st.name, info),
					None => st.name.clone(),
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
				if !self.collapsed.borrow().contains(&key) {
					self.list.expand(&item);
				}
			}
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

		// Typing filters the tree.
		let on_type = hooks.on_type.clone();
		self.list.bind_internal(EventType::CHAR, move |e: Event| {
			let c = e
				.get_unicode_key()
				.and_then(|k| char::from_u32(k as u32))
				.filter(|c| !c.is_control() && *c != ' ');
			match c.filter(|_| !e.control_down() && !e.alt_down()) {
				Some(c) if on_type.is_some() => {
					e.skip(false);
					if let Some(hook) = &on_type {
						hook(c);
					}
				}
				_ => e.skip(true),
			}
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
					WXK_BACK if hooks.on_backspace.is_some() => {
						e.skip(false);
						if let Some(hook) = &hooks.on_backspace {
							hook();
						}
					}
					WXK_ESCAPE if hooks.on_clear_filter.is_some() => {
						e.skip(false);
						if let Some(hook) = &hooks.on_clear_filter {
							hook();
						}
					}
					_ => e.skip(true),
				}
			});

		// Collapsing or expanding a folder sticks across rebuilds.
		let me = self.clone();
		self.list.on_item_collapsed(move |e| {
			if let Some(item) = e.get_item()
				&& let Some(Row::Folder(key)) = me.row(&item)
			{
				me.collapsed.borrow_mut().insert(key);
			}
		});
		let me = self.clone();
		self.list.on_item_expanding(move |e| {
			if let Some(item) = e.get_item()
				&& let Some(Row::Folder(key)) = me.row(&item)
			{
				me.collapsed.borrow_mut().remove(&key);
			}
		});

		// The Applications key, Shift+F10 and a right click.
		let (a, me) = (app.clone(), self.clone());
		self.list
			.bind_internal(EventType::CONTEXT_MENU, move |e: Event| {
				e.skip(false);
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
