//! The main window: the Stations dial, now playing, the playback buttons, Volume and the message
//! line.

use std::cell::RefCell;
use std::rc::Rc;

use wx_utils::AboutBoxBuilder;
use wxdragon::prelude::*;

use crate::action::{Action, app_key};
use crate::app::App;
use crate::dialogs;
use crate::menus;
use crate::station_list::{Hooks, StationList};
use crate::strings as s;

const POLL_MS: i32 = 50;

#[derive(Clone)]
pub struct MainWindow {
	pub frame: Frame,
	/// The dial: the SerrebiRadio collection, or the collection last played from.
	pub stations: StationList,
	/// `Stopped`, `<station>` or `<song> - <station>`.
	pub now_playing: StaticText,
	pub play: Button,
	pub stop: Button,
	pub mute: Button,
	pub favorite: Button,
	pub volume: Slider,
	/// The last message; it stays until the next one.
	pub message: StaticText,
}

/// A label shown as typed: a lone `&` would underline the next letter.
pub fn escape(text: &str) -> String {
	text.replace('&', "&&")
}

pub fn build() -> MainWindow {
	let frame = Frame::builder()
		.with_title(s::APP_NAME)
		.with_size(Size::new(640, 520))
		.build();
	let panel = Panel::builder(&frame).build();
	let stations = StationList::new(&panel, s::STATIONS);
	let now_playing = StaticText::builder(&panel).with_label(s::STOPPED).build();
	let play = Button::builder(&panel).with_label(s::PLAY).build();
	let stop = Button::builder(&panel).with_label(s::STOP).build();
	let mute = Button::builder(&panel).with_label(s::MUTE).build();
	let favorite = Button::builder(&panel)
		.with_label(&menus::plain(menus::favorite_label(false)))
		.build();
	// wxMSW names a control after the label created just before it.
	let volume_label = StaticText::builder(&panel).with_label(s::VOLUME).build();
	let volume = Slider::builder(&panel)
		.with_min_value(0)
		.with_max_value(100)
		.build();
	let message = StaticText::builder(&panel).with_label("").build();

	let buttons = BoxSizer::builder(Orientation::Horizontal).build();
	for b in [&play, &stop, &mute, &favorite] {
		buttons.add(b, 0, SizerFlag::All, 5);
	}
	let sizer = BoxSizer::builder(Orientation::Vertical).build();
	sizer.add(
		&stations.label,
		0,
		SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
		5,
	);
	sizer.add(&stations.list, 1, SizerFlag::Expand | SizerFlag::All, 5);
	sizer.add(&now_playing, 0, SizerFlag::Expand | SizerFlag::All, 5);
	sizer.add_sizer(&buttons, 0, SizerFlag::All, 0);
	sizer.add(&volume_label, 0, SizerFlag::All, 5);
	sizer.add(&volume, 0, SizerFlag::Expand | SizerFlag::All, 5);
	sizer.add(&message, 0, SizerFlag::Expand | SizerFlag::All, 5);
	panel.set_sizer(sizer, true);
	MainWindow {
		frame,
		stations,
		now_playing,
		play,
		stop,
		mute,
		favorite,
		volume,
		message,
	}
}

/// Every menu item, button and key runs through here.
pub fn dispatch(app: &Rc<RefCell<App>>, action: Action) {
	match action {
		// Modal: the poll timer borrows the app while it is open.
		Action::About => {
			let frame = app.borrow().win.frame;
			AboutBoxBuilder::new(&frame)
				.name(s::APP_NAME)
				.version(env!("CARGO_PKG_VERSION"))
				.description(s::ABOUT_DESCRIPTION)
				.website(s::WEBSITE)
				.show();
		}
		Action::OpenSearch => {
			if let Some(next) = dialogs::search::show(app) {
				dispatch(app, next);
			}
		}
		Action::OpenBrowse => {
			if let Some(next) = dialogs::browse::show(app) {
				dispatch(app, next);
			}
		}
		Action::OpenFavorites => {
			if let Some(next) = dialogs::favorites::show(app) {
				dispatch(app, next);
			}
		}
		Action::OpenRecent => {
			if let Some(next) = dialogs::recent::show(app) {
				dispatch(app, next);
			}
		}
		Action::NewCustomStation => {
			if let Some(next) = dialogs::custom_station::show(app) {
				dispatch(app, next);
			}
		}
		Action::OpenCollections => {
			if let Some(next) = dialogs::collections::show(app) {
				dispatch(app, next);
			}
		}
		Action::AddToCollection => {
			let station = app.borrow().selected_or_playing();
			match station {
				Some(st) => {
					let frame = app.borrow().win.frame;
					dialogs::add_to_collection::choose(app, &frame, &st);
				}
				None => app.borrow_mut().say(s::NOTHING_PLAYING),
			}
		}
		Action::Import => dialogs::collections::import(app, false),
		Action::ImportAsCollection => dialogs::collections::import(app, true),
		Action::SongHistory => {
			if let Some(next) = dialogs::history::show(app) {
				dispatch(app, next);
			}
		}
		Action::Equalizer => {
			if let Some(next) = dialogs::equalizer::show(app) {
				dispatch(app, next);
			}
		}
		a => app.borrow_mut().run(a),
	}
}

/// Volume slider keys: Up and PageUp raise (a native trackbar lowers on them).
pub fn volume_step(code: i32) -> Option<i32> {
	match code {
		WXK_UP | WXK_RIGHT => Some(5),
		WXK_DOWN | WXK_LEFT => Some(-5),
		WXK_PAGEUP => Some(10),
		WXK_PAGEDOWN => Some(-10),
		_ => None,
	}
}

pub fn bind(app: &Rc<RefCell<App>>) {
	let win = app.borrow().win.clone();

	let a = app.clone();
	win.stations.bind(
		app,
		Hooks {
			on_delete: Some(Rc::new(move |i| a.borrow_mut().delete_from_dial(i))),
			..Hooks::default()
		},
	);

	let a = app.clone();
	win.frame.on_menu_selected(move |e| {
		let action = a.borrow().menus.action(e.get_id());
		if let Some(action) = action {
			dispatch(&a, action);
		}
	});

	for (button, action) in [
		(win.play, Action::PlayPause),
		(win.stop, Action::Stop),
		(win.mute, Action::Mute),
		(win.favorite, Action::ToggleFavorite),
	] {
		let a = app.clone();
		button.on_click(move |_| dispatch(&a, action.clone()));
	}

	let a = app.clone();
	win.volume.on_slider(move |e| {
		dispatch(&a, Action::SetVolume(e.get_value().clamp(0, 100) as u8));
	});
	let (a, volume) = (app.clone(), win.volume);
	win.volume.on_key_down(move |e| {
		let WindowEventData::Keyboard(key) = e else {
			return;
		};
		let plain = !(key.control_down() || key.alt_down() || key.shift_down());
		match key.get_key_code().and_then(volume_step).filter(|_| plain) {
			Some(step) => {
				// Consumed, or the native trackbar moves it as well (the wrong way).
				key.event.skip(false);
				let v = (volume.get_value() + step).clamp(0, 100) as u8;
				dispatch(&a, Action::SetVolume(v));
			}
			None => key.event.skip(true),
		}
	});

	let a = app.clone();
	win.frame.on_char_hook(move |e| {
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
			// wxDragon skips by default: unconsumed, the key would also reach the menu
			// accelerators and the focused control, and run twice.
			Some(action) => {
				key.event.skip(false);
				dispatch(&a, action);
			}
			None => key.event.skip(true),
		}
	});

	let a = app.clone();
	win.frame.on_close(move |e| {
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
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn volume_keys_direction() {
		assert_eq!(volume_step(WXK_UP), Some(5));
		assert_eq!(volume_step(WXK_PAGEUP), Some(10));
		assert_eq!(volume_step(WXK_DOWN), Some(-5));
		assert_eq!(volume_step(WXK_PAGEDOWN), Some(-10));
		assert_eq!(volume_step(WXK_HOME), None, "Home and End stay native");
	}

	#[test]
	fn escape_keeps_ampersands() {
		assert_eq!(escape("R&B Hits"), "R&&B Hits");
	}
}
