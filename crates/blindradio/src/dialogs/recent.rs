//! Recently Played (Ctrl+H): what played, newest first.

use std::cell::RefCell;
use std::rc::Rc;

use wxdragon::prelude::*;

use crate::action::Action;
use crate::app::App;
use crate::bridge::WindowToken;
use crate::main_window::dispatch;
use crate::station_list::{Hooks, StationList, favorite_label_of};
use crate::strings as s;

pub fn show(app: &Rc<RefCell<App>>) -> Option<Action> {
	let frame = app.borrow().win.frame;
	let dialog = Dialog::builder(&frame, s::RECENT_TITLE).build();
	let list = StationList::new(&dialog, s::RECENT_LIST);
	let favorite = Button::builder(&dialog)
		.with_label(s::ADD_TO_FAVORITES)
		.build();
	let close = Button::builder(&dialog)
		.with_id(ID_CANCEL)
		.with_label(s::CLOSE)
		.build();
	let buttons = BoxSizer::builder(Orientation::Horizontal).build();
	buttons.add(&favorite, 0, SizerFlag::All, 5);
	buttons.add(&close, 0, SizerFlag::All, 5);
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
	let recent = app.borrow().library.recent.clone();
	list.set_stations(recent, 0);

	// The button names what it will do to the focused station.
	let update: Rc<dyn Fn()> = Rc::new({
		let (a, me) = (app.clone(), list.clone());
		move || favorite.set_label(favorite_label_of(&a.borrow(), &me))
	});
	update();
	{
		let (a, update) = (app.clone(), update.clone());
		favorite.on_click(move |_| {
			dispatch(&a, Action::ToggleFavorite);
			update();
		});
	}
	list.bind(
		app,
		Hooks {
			close: Some(dialog),
			on_focus: Some(Rc::new({
				let update = update.clone();
				move |_| update()
			})),
			..Hooks::default()
		},
	);

	list.list.set_focus();
	// Landing on a station plays it.
	if list.current().is_some() {
		list.land(&mut app.borrow_mut());
	}
	crate::dialogs::run(app, dialog, Action::OpenRecent, list.list, &token)
}
