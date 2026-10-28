// The main window here is still the plan 1 dial list; Task 2 builds the real one.
// Debug builds keep a console, so the stderr log is visible.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod action;
mod app;
mod bridge;
mod strings;

use std::cell::RefCell;
use std::rc::Rc;

use blindradio_core::station::Station;
use wxdragon::prelude::*;

use crate::action::app_key;
use crate::app::App;
use crate::strings as s;

const STATIONS: &str = include_str!("../../blindradio-core/examples/stations.json");
const POLL_MS: i32 = 50;

/// Says why the app cannot start, then closes; with no window left the app exits.
fn fail(frame: Frame, error: &dyn std::fmt::Display) {
	MessageDialog::builder(&frame, &format!("{}{error}", s::FAILED), s::APP_NAME)
		.build()
		.show_modal();
	frame.close(true);
}

fn main() {
	let stations: Rc<Vec<Station>> =
		Rc::new(serde_json::from_str(STATIONS).expect("embedded stations.json"));
	let _ = wxdragon::main(move |_| {
		let frame = Frame::builder().with_title(s::APP_NAME).build();
		let panel = Panel::builder(&frame).build();
		// wxMSW names the list after the label created just before it.
		let label = StaticText::builder(&panel).with_label(s::STATIONS).build();
		let list = ListBox::builder(&panel)
			.with_choices(stations.iter().map(|s| s.name.clone()).collect())
			.build();
		let message = StaticText::builder(&panel).with_label("").build();
		let sizer = BoxSizer::builder(Orientation::Vertical).build();
		sizer.add(&label, 0, SizerFlag::All, 5);
		sizer.add(&list, 1, SizerFlag::Expand | SizerFlag::All, 5);
		sizer.add(&message, 0, SizerFlag::Expand | SizerFlag::All, 5);
		panel.set_sizer(sizer, true);

		let app = match App::new(frame, message) {
			Ok(a) => Rc::new(RefCell::new(a)),
			Err(e) => return fail(frame, &e),
		};

		let (a, list_stations) = (app.clone(), stations.clone());
		list.on_selection_changed(move |e| {
			if let Some(i) = e.get_selection().and_then(|i| usize::try_from(i).ok()) {
				a.borrow_mut().tune(&list_stations, i);
			}
		});

		let a = app.clone();
		frame.on_char_hook(move |e| {
			let WindowEventData::Keyboard(key) = e else {
				return;
			};
			let action = key.get_key_code().and_then(|code| {
				app_key(
					code,
					key.control_down(),
					key.alt_down(),
					key.shift_down(),
					true,
				)
			});
			match action {
				Some(action) => a.borrow_mut().run(action),
				None => key.event.skip(true),
			}
		});

		let a = app.clone();
		frame.on_close(move |e| {
			// Exit already shut down and still holds the app.
			if let Ok(mut app) = a.try_borrow_mut() {
				app.shutdown();
			}
			if let WindowEventData::General(e) = e {
				e.skip(true);
			}
		});

		let a = app.clone();
		if let Some(timer) = &app.borrow().timer {
			timer.on_tick(move |_| a.borrow_mut().poll());
			timer.start(POLL_MS, false);
		}

		frame.show(true);
		frame.centre();
		// Landing on a station plays it (owner rule), including the first one at start.
		list.set_selection(0, true);
		list.set_focus();
		app.borrow_mut().tune(&stations, 0);
	});
}
