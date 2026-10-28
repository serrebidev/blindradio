// Debug builds keep a console, so the stderr log is visible.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod action;
mod app;
mod bridge;
mod context_menu;
mod dialogs;
mod main_window;
mod menus;
mod station_list;
mod strings;

use std::cell::RefCell;
use std::rc::Rc;

use wxdragon::prelude::*;

use crate::app::App;
use crate::strings as s;

/// Says why the app cannot start, then closes; with no window left the app exits.
fn fail(frame: Frame, error: &dyn std::fmt::Display) {
	MessageDialog::builder(&frame, &format!("{}{error}", s::FAILED), s::APP_NAME)
		.build()
		.show_modal();
	frame.close(true);
}

fn main() {
	let _ = wxdragon::main(|_| {
		let win = main_window::build();
		let frame = win.frame;
		let app = match App::new(win) {
			Ok(a) => Rc::new(RefCell::new(a)),
			Err(e) => return fail(frame, &e),
		};
		main_window::bind(&app);
		frame.show(true);
		frame.centre();
		// Focuses the Stations list, else Play.
		app.borrow_mut().start();
	});
}
