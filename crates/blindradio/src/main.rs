// Debug builds keep a console, so the stderr log is visible.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod action;
mod app;
mod bridge;
mod clipboard;
mod context_menu;
mod dialogs;
mod help;
mod hotkeys;
mod i18n;
mod instance;
mod jaws;
mod keys;
mod dial_tree;
mod main_window;
mod menus;
mod nvda;
#[cfg(windows)]
mod power;
#[cfg(windows)]
mod status_strip;
mod speech;
mod station_list;
mod strings;
mod tray;
mod update;

use std::cell::RefCell;
use std::rc::Rc;

use wxdragon::prelude::*;

use crate::app::App;
use crate::strings as s;

// The compiled translation catalogs, embedded by patois-build; the wx
// loader serves the wxWidgets stock strings in the same language.
patois::embed_domain!();
patois::embed_wx_translations!();

/// Says why the app cannot start, then closes; with no window left the app exits.
fn fail(frame: Frame, error: &dyn std::fmt::Display) {
	MessageDialog::builder(&frame, &format!("{}{error}", s::failed()), s::APP_NAME)
		.build()
		.show_modal();
	frame.close(true);
}

fn main() {
	let _ = wxdragon::main(|_| {
		// Before any window: a second copy pokes the first and exits here.
		let claim = instance::claim();
		// Translations before the first label: the saved language wins.
		let saved = blindradio_core::settings::Settings::load(&app::data_dir());
		i18n::init(saved.language.as_deref());
		let win = main_window::build();
		let frame = win.frame;
		let app = match App::new(win) {
			Ok(a) => Rc::new(RefCell::new(a)),
			Err(e) => return fail(frame, &e),
		};
		app.borrow_mut().instance = claim;
		main_window::bind(&app);
		let tray = tray::install(&app);
		app.borrow_mut().tray = tray;
		if app.borrow().settings.maximize_on_start {
			frame.maximize(true);
		}
		frame.show(true);
		frame.centre();
		// A copy the updater just relaunched takes the foreground on Windows.
		ship_shape::ui::bring_to_front_after_update(&frame);
		// Focuses the Stations list, else Play.
		app.borrow_mut().start();
	});
}
