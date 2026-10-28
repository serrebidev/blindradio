//! The list every station window shares: dial tuning, letter jump, Enter, Delete, Alt+Up/Down
//! and the context menu.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use blindradio_core::station::Station;
use wxdragon::event::{Event, EventType, WxEvtHandler};
use wxdragon::prelude::*;

use crate::action::Action;
use crate::app::App;
use crate::context_menu::{self, Target};
use crate::main_window::dispatch;
use crate::strings as s;

/// The next station after `from` whose name starts with `c` (any case), wrapping round.
pub fn letter_jump(names: &[&str], from: Option<usize>, c: char) -> Option<usize> {
	let start = from.map_or(0, |i| i + 1);
	let c: Vec<char> = c.to_lowercase().collect();
	(0..names.len())
		.map(|k| (start + k) % names.len())
		.find(|&i| {
			names[i]
				.chars()
				.next()
				.is_some_and(|first| first.to_lowercase().eq(c.iter().copied()))
		})
}

/// What the owning window does on Delete, Alt+Up/Down and Enter.
#[derive(Default)]
pub struct Hooks {
	/// Enter closes this window when Close Lists When Playing is on.
	pub close: Option<Dialog>,
	/// The focused index.
	pub on_delete: Option<Rc<dyn Fn(usize)>>,
	/// The focused index; true is up.
	pub on_move: Option<Rc<dyn Fn(usize, bool)>>,
}

#[derive(Clone)]
pub struct StationList {
	pub label: StaticText,
	pub list: ListCtrl,
	stations: Rc<RefCell<Rc<[Station]>>>,
	/// Set while the program changes the selection: only the user's moves tune.
	quiet: Rc<Cell<bool>>,
}

impl StationList {
	/// The label first: wxMSW names the list after it.
	pub fn new(parent: &impl WxWidget, label: &str) -> StationList {
		let label = StaticText::builder(parent).with_label(label).build();
		let list = ListCtrl::builder(parent)
			.with_style(ListCtrlStyle::Report | ListCtrlStyle::SingleSel)
			.build();
		for (i, (heading, width)) in [
			(s::COLUMN_NAME, 260),
			(s::COLUMN_INFO, 200),
			(s::COLUMN_DIRECTORY, 120),
		]
		.into_iter()
		.enumerate()
		{
			list.insert_column(i as i64, heading, ListColumnFormat::Left, width);
		}
		StationList {
			label,
			list,
			stations: Rc::new(RefCell::new(Rc::from([]))),
			quiet: Rc::new(Cell::new(false)),
		}
	}

	pub fn stations(&self) -> Rc<[Station]> {
		self.stations.borrow().clone()
	}

	pub fn current(&self) -> Option<usize> {
		usize::try_from(self.list.get_first_selected_item()).ok()
	}

	/// Replaces the list; never tunes. `focus` is clamped.
	pub fn set_stations(&self, stations: Vec<Station>, focus: usize) {
		self.quiet.set(true);
		self.list.delete_all_items();
		for (i, st) in stations.iter().enumerate() {
			let i = i as i64;
			self.list.insert_item(i, &st.name, None);
			self.list
				.set_item_text_by_column(i, 1, st.info.as_deref().unwrap_or_default());
			self.list.set_item_text_by_column(i, 2, st.source.label());
		}
		let len = stations.len();
		*self.stations.borrow_mut() = stations.into();
		self.quiet.set(false);
		if len > 0 {
			self.select(focus.min(len - 1), false);
		}
	}

	/// Selects and focuses `index`; `tune` makes it a user move.
	pub fn select(&self, index: usize, tune: bool) {
		self.quiet.set(!tune);
		let both = ListItemState::Selected | ListItemState::Focused;
		self.list.set_item_state(index as i64, both, both);
		self.list.ensure_visible(index as i64);
		self.quiet.set(false);
	}

	/// Opening a list or a search landing on it: the focused station plays at once.
	pub fn land(&self, app: &mut App) {
		if let Some(i) = self.current() {
			app.tune(&self.stations(), i);
		}
	}

	/// Handlers run with the app not borrowed; programmatic changes made while it is borrowed
	/// return before borrowing it (`quiet`).
	pub fn bind(&self, app: &Rc<RefCell<App>>, hooks: Hooks) {
		let (a, me) = (app.clone(), self.clone());
		self.list.on_item_selected(move |e| {
			if me.quiet.get() {
				return;
			}
			if let Ok(i) = usize::try_from(e.get_item_index()) {
				let stations = me.stations();
				a.borrow_mut().tune(&stations, i);
			}
		});

		let (a, me, close) = (app.clone(), self.clone(), hooks.close);
		self.list.on_item_activated(move |e| {
			let Some(st) = usize::try_from(e.get_item_index())
				.ok()
				.and_then(|i| me.stations().get(i).cloned())
			else {
				return;
			};
			a.borrow_mut().selected = Some(st);
			dispatch(&a, Action::Play);
			let close_lists = a.borrow().settings.close_lists_when_playing;
			if close_lists && let Some(dialog) = close {
				dialog.end_modal(ID_OK);
			}
		});

		let me = self.clone();
		self.list.bind_internal(EventType::CHAR, move |e: Event| {
			let c = e
				.get_unicode_key()
				.and_then(|k| char::from_u32(k as u32))
				.filter(|c| !c.is_control() && *c != ' ');
			match c.filter(|_| !e.control_down() && !e.alt_down()) {
				// Consumed: the native type-ahead would jump as well.
				Some(c) => {
					e.skip(false);
					let stations = me.stations();
					let names: Vec<&str> = stations.iter().map(|s| s.name.as_str()).collect();
					let to = letter_jump(&names, me.current(), c);
					if let Some(i) = to.filter(|&i| Some(i) != me.current()) {
						me.select(i, true);
					}
				}
				None => e.skip(true),
			}
		});

		let me = self.clone();
		let (on_delete, on_move) = (hooks.on_delete.clone(), hooks.on_move);
		self.list
			.bind_internal(EventType::KEY_DOWN, move |e: Event| {
				let (Some(code), Some(i)) = (e.get_key_code(), me.current()) else {
					return e.skip(true);
				};
				let plain = !(e.control_down() || e.alt_down() || e.shift_down());
				let alt = e.alt_down() && !e.control_down() && !e.shift_down();
				match (code, &on_delete, &on_move) {
					(WXK_DELETE, Some(delete), _) if plain => {
						e.skip(false);
						delete(i);
					}
					(WXK_UP | WXK_DOWN, _, Some(step)) if alt => {
						e.skip(false);
						step(i, code == WXK_UP);
					}
					_ => e.skip(true),
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
				let items = context_menu::items_for(&target, &a.borrow().library);
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

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn letter_jump_wraps() {
		let names = ["Jazz FM", "BBC", "jazz24", "Kiss", "Ñu"];
		let jump = |from, c| letter_jump(&names, from, c);
		assert_eq!(jump(None, 'j'), Some(0), "nothing focused: the first match");
		assert_eq!(jump(Some(0), 'j'), Some(2), "next match, any case");
		assert_eq!(jump(Some(2), 'J'), Some(0), "wraps round");
		assert_eq!(jump(Some(3), 'k'), Some(3), "the only match stays");
		assert_eq!(jump(Some(1), 'ñ'), Some(4));
		assert_eq!(jump(Some(1), 'z'), None, "no match");
		assert_eq!(letter_jump(&[], None, 'a'), None);
	}
}
