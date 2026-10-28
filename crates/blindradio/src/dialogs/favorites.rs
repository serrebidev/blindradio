//! Favorites (Ctrl+B): remove, reorder and export.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use blindradio_core::import;
use wxdragon::prelude::*;

use crate::action::Action;
use crate::app::App;
use crate::bridge::WindowToken;
use crate::station_list::{Hooks, StationList};
use crate::strings as s;

/// Where focus lands after removing `index` from a list that had `len` items.
pub fn focus_after_remove(len: usize, index: usize) -> Option<usize> {
	(len > 1).then(|| index.min(len - 2))
}

fn refill(app: &Rc<RefCell<App>>, list: &StationList, focus: usize) {
	let stations = app.borrow().library.favorites.clone();
	list.set_stations(stations, focus);
}

pub fn show(app: &Rc<RefCell<App>>) -> Option<Action> {
	let frame = app.borrow().win.frame;
	let dialog = Dialog::builder(&frame, s::FAVORITES_TITLE).build();
	let list = StationList::new(&dialog, s::FAVORITES_LIST);
	let remove = Button::builder(&dialog).with_label(s::REMOVE).build();
	let up = Button::builder(&dialog).with_label(s::MOVE_UP).build();
	let down = Button::builder(&dialog).with_label(s::MOVE_DOWN).build();
	let export = Button::builder(&dialog).with_label(s::EXPORT).build();
	let close = Button::builder(&dialog)
		.with_id(ID_CANCEL)
		.with_label(s::CLOSE)
		.build();
	let buttons = BoxSizer::builder(Orientation::Horizontal).build();
	for b in [&remove, &up, &down, &export, &close] {
		buttons.add(b, 0, SizerFlag::All, 5);
	}
	let sizer = BoxSizer::builder(Orientation::Vertical).build();
	sizer.add(
		&list.label,
		0,
		SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
		5,
	);
	sizer.add(&list.list, 1, SizerFlag::Expand | SizerFlag::All, 5);
	sizer.add_sizer(&buttons, 0, SizerFlag::All, 0);
	dialog.set_sizer(sizer, true);
	dialog.set_size(Size::new(640, 560));

	let token = WindowToken::new();
	refill(app, &list, 0);

	// Delete and &Remove: out of Favorites; the playing station plays on.
	let delete: Rc<dyn Fn(usize)> = Rc::new({
		let (a, me) = (app.clone(), list.clone());
		move |i| {
			let len = me.stations().len();
			let focus = focus_after_remove(len, i).unwrap_or(0);
			a.borrow_mut().remove_favorite(i);
			refill(&a, &me, focus);
			// What Ctrl+D acts on next, without tuning.
			a.borrow_mut().selected = me.stations().get(focus).cloned();
		}
	});
	// Alt+Up/Down and Move: focus goes with the moved station; the sound does not change.
	let moved: Rc<dyn Fn(usize, bool)> = Rc::new({
		let (a, me) = (app.clone(), list.clone());
		move |i, up| {
			let index = a.borrow_mut().move_favorite(i, up);
			refill(&a, &me, index);
			me.select(index, false);
			a.borrow_mut().selected = me.stations().get(index).cloned();
		}
	});
	{
		let (me, d) = (list.clone(), delete.clone());
		remove.on_click(move |_| {
			if let Some(i) = me.current() {
				d(i);
			}
		});
	}
	for (button, up) in [(up, true), (down, false)] {
		let (me, m) = (list.clone(), moved.clone());
		button.on_click(move |_| {
			if let Some(i) = me.current() {
				m(i, up);
			}
		});
	}
	{
		let (a, me) = (app.clone(), list.clone());
		export.on_click(move |_| {
			let stations = me.stations();
			let file = FileDialog::builder(&dialog)
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
			match import::export_m3u(&stations, Path::new(&path)) {
				Ok(()) => a
					.borrow_mut()
					.say(&format!("{}{}", s::EXPORTED, stations.len())),
				Err(e) => {
					eprintln!("export: {e}");
					a.borrow_mut().say(s::NOT_SAVED);
				}
			}
		});
	}

	list.bind(
		app,
		Hooks {
			close: Some(dialog),
			on_delete: Some(delete),
			on_move: Some(moved),
			..Hooks::default()
		},
	);

	list.list.set_focus();
	// Landing on a station plays it.
	if list.current().is_some() {
		list.land(&mut app.borrow_mut());
	}
	crate::dialogs::run(app, dialog, Action::OpenFavorites, list.list, &token)
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
