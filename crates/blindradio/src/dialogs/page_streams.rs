//! Detect Audio Stream on Page: a web page's address in, its streams out, in a station list
//! like Search's: Enter plays, the context menu files them.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use blindradio_core::Result;
use blindradio_core::station::Station;
use wxdragon::prelude::*;

use crate::action::Action;
use crate::app::App;
use crate::bridge::WindowToken;
use crate::station_list::{Hooks, StationList};
use crate::strings as s;

/// `1 stream found`, `2 streams found`.
fn count_said(n: usize) -> String {
	let word = if n == 1 { "stream" } else { "streams" };
	format!("{n} {word}{}", s::streams_suffix())
}

pub fn show(app: &Rc<RefCell<App>>) -> Option<Action> {
	let frame = app.borrow().win.frame;
	let dialog = Dialog::builder(&frame, &s::detect_title()).build();
	let sizer = BoxSizer::builder(Orientation::Vertical).build();
	// Each label is created just before the control it names.
	let url_label = StaticText::builder(&dialog).with_label(&s::page_url()).build();
	let url = TextCtrl::builder(&dialog).build();
	let detect = Button::builder(&dialog).with_label(&s::detect_button()).build();
	detect.set_default();
	let streams = StationList::new(&dialog, &s::streams_found());
	let close = Button::builder(&dialog)
		.with_id(ID_CANCEL)
		.with_label(&s::close())
		.build();
	let top = SizerFlag::Left | SizerFlag::Right | SizerFlag::Top;
	sizer.add(&url_label, 0, top, 5);
	sizer.add(&url, 0, SizerFlag::Expand | SizerFlag::All, 5);
	sizer.add(&detect, 0, SizerFlag::All, 5);
	sizer.add(&streams.label, 0, top, 5);
	sizer.add(&streams.list, 1, SizerFlag::Expand | SizerFlag::All, 5);
	sizer.add(&close, 0, SizerFlag::All, 5);
	dialog.set_sizer(sizer, true);
	dialog.set_size(Size::new(640, 480));

	let token = WindowToken::new();
	// A newer look wins over one still in flight.
	let generation = Rc::new(Cell::new(0u64));
	let (a, list, tok) = (app.clone(), streams.clone(), token.clone());
	detect.on_click(move |_| {
		let page = url.get_value();
		if page.trim().is_empty() {
			return url.set_focus();
		}
		generation.set(generation.get() + 1);
		let (mine, generation, list) = (generation.get(), generation.clone(), list.clone());
		let mut app = a.borrow_mut();
		app.say(&s::detecting());
		app.bridge.spawn(
			tok.clone(),
			async move {
				blindradio_core::page::detect(&blindradio_core::net::client(), &page).await
			},
			move |app: &mut App, found: Result<Vec<Station>>| {
				if generation.get() != mine {
					return;
				}
				let found = match found {
					Ok(found) => found,
					Err(e) => {
						list.set_stations(Vec::new(), 0);
						return app.say(&e.to_string());
					}
				};
				let count = found.len();
				list.set_stations(found, 0);
				if count == 0 {
					return app.say(&s::no_streams());
				}
				list.list.set_focus();
				list.land(app);
				app.say(&count_said(count));
			},
		);
	});
	streams.bind(
		app,
		Hooks {
			close: Some(dialog),
			..Hooks::default()
		},
	);
	url.set_focus();
	crate::dialogs::run(app, dialog, Action::DetectStreams, url, &token)
}

#[cfg(test)]
mod tests {
	use super::count_said;

	#[test]
	fn the_count_reads_as_a_sentence() {
		assert_eq!(count_said(1), "1 stream found");
		assert_eq!(count_said(3), "3 streams found");
	}
}
