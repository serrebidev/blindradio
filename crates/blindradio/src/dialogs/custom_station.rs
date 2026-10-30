//! Add Custom Station (Ctrl+N): a name and a stream URL.

use std::cell::RefCell;
use std::rc::Rc;

use blindradio_core::station::Station;
use wx_utils::add_ok_cancel_footer;
use wxdragon::prelude::*;

use crate::action::Action;
use crate::app::App;
use crate::bridge::WindowToken;
use crate::strings as s;

const OK_ID: Id = ID_HIGHEST + 7000;

/// Only a stream an HTTP client can open will play.
pub fn is_stream_url(url: &str) -> bool {
	let url = url.trim().to_ascii_lowercase();
	url.starts_with("http://") || url.starts_with("https://")
}

pub fn show(app: &Rc<RefCell<App>>) -> Option<Action> {
	let frame = app.borrow().win.frame;
	let dialog = Dialog::builder(&frame, s::ADD_CUSTOM_TITLE).build();
	let name_label = StaticText::builder(&dialog).with_label(s::NAME).build();
	let name = TextCtrl::builder(&dialog).build();
	let url_label = StaticText::builder(&dialog)
		.with_label(s::STREAM_URL)
		.build();
	let url = TextCtrl::builder(&dialog).build();
	let sizer = BoxSizer::builder(Orientation::Vertical).build();
	for (label, control) in [(&name_label, &name), (&url_label, &url)] {
		sizer.add(
			label,
			0,
			SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
			5,
		);
		sizer.add(control, 0, SizerFlag::Expand | SizerFlag::All, 5);
	}
	// A bad URL must keep the window open, so OK is not a stock button.
	let ok = Button::builder(&dialog)
		.with_id(OK_ID)
		.with_label(s::OK)
		.build();
	let cancel = Button::builder(&dialog)
		.with_id(ID_CANCEL)
		.with_label(s::CANCEL)
		.build();
	dialog.set_affirmative_id(OK_ID);
	dialog.set_escape_id(ID_CANCEL);
	ok.set_default();
	add_ok_cancel_footer(sizer, ok, cancel);
	dialog.set_sizer_and_fit(sizer, true);

	let token = WindowToken::new();
	{
		let (a, d) = (app.clone(), dialog);
		ok.on_click(move |_| {
			let link = url.get_value();
			if !is_stream_url(&link) {
				return a.borrow_mut().say(s::NOT_A_STREAM_URL);
			}
			let link = link.trim().to_string();
			let typed = name.get_value().trim().to_string();
			// A station with no name is unreadable in every list: the URL stands in.
			let typed = if typed.is_empty() {
				link.clone()
			} else {
				typed
			};
			let station = {
				let mut app = a.borrow_mut();
				let station = app.library.add_custom(&typed, &link);
				app.save_library();
				app.refresh_dial();
				station
			};
			a.borrow_mut().say(&format!("{}{}", s::ADDED, station.name));
			d.end_modal(ID_OK);
		});
	}

	name.set_focus();
	crate::dialogs::run(app, dialog, Action::NewCustomStation, name, &token)
}

/// Edit Station: the Add Custom fields filled in with the station's name and
/// URL. Saving rewrites every held copy (collections, favorites), so an
/// imported playlist's station can be renamed and re-pointed in place.
pub fn show_edit(app: &Rc<RefCell<App>>, station: &Station) -> Option<Action> {
	let frame = app.borrow().win.frame;
	let dialog = Dialog::builder(&frame, s::EDIT_STATION_TITLE).build();
	let name_label = StaticText::builder(&dialog).with_label(s::NAME).build();
	let name = TextCtrl::builder(&dialog).build();
	name.set_value(&station.name);
	let url_label = StaticText::builder(&dialog)
		.with_label(s::STREAM_URL)
		.build();
	let url = TextCtrl::builder(&dialog).build();
	url.set_value(&station.url);
	let sizer = BoxSizer::builder(Orientation::Vertical).build();
	for (label, control) in [(&name_label, &name), (&url_label, &url)] {
		sizer.add(
			label,
			0,
			SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
			5,
		);
		sizer.add(control, 0, SizerFlag::Expand | SizerFlag::All, 5);
	}
	let ok = Button::builder(&dialog)
		.with_id(OK_ID)
		.with_label(s::OK)
		.build();
	let cancel = Button::builder(&dialog)
		.with_id(ID_CANCEL)
		.with_label(s::CANCEL)
		.build();
	dialog.set_affirmative_id(OK_ID);
	dialog.set_escape_id(ID_CANCEL);
	ok.set_default();
	add_ok_cancel_footer(sizer, ok, cancel);
	dialog.set_sizer_and_fit(sizer, true);

	let token = WindowToken::new();
	{
		let (a, d) = (app.clone(), dialog);
		let id = station.id.clone();
		ok.on_click(move |_| {
			let link = url.get_value();
			if !is_stream_url(&link) {
				return a.borrow_mut().say(s::NOT_A_STREAM_URL);
			}
			let link = link.trim().to_string();
			let typed = name.get_value().trim().to_string();
			let typed = if typed.is_empty() {
				link.clone()
			} else {
				typed
			};
			{
				let mut app = a.borrow_mut();
				if app.library.update_station(&id, &typed, &link) {
					app.save_library();
					app.refresh_dial();
				}
			}
			a.borrow_mut().say(&format!("{}{}", s::SAVED, typed));
			d.end_modal(ID_OK);
		});
	}

	name.set_focus();
	crate::dialogs::run(app, dialog, Action::EditStation, name, &token)
}

#[cfg(test)]
mod tests {
	use super::is_stream_url;

	#[test]
	fn custom_url_validation() {
		assert!(is_stream_url("http://example.com/a"));
		assert!(is_stream_url("  HTTPS://example.com/a  "));
		assert!(!is_stream_url("example.com/a"));
		assert!(!is_stream_url("ftp://example.com/a"));
		assert!(!is_stream_url(""));
	}
}
