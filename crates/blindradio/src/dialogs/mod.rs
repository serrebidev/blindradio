//! Modal windows: one at a time, app keys inside them, and switching between them.

pub mod add_to_collection;
pub mod browse;
pub mod collections;
pub mod custom_station;
pub mod equalizer;
pub mod favorites;
pub mod features;
pub mod history;
pub mod hotkeys;
pub mod keys;
pub mod recent;
pub mod schedules;
pub mod search;

use std::cell::RefCell;
use std::rc::Rc;

use wxdragon::prelude::*;

use crate::action::{Action, app_key};
use crate::app::App;
use crate::bridge::WindowToken;
use crate::main_window::dispatch;

/// What a key pressed in a window does.
#[derive(Debug, PartialEq, Eq)]
pub enum Route {
	/// To the focused control: typing, arrows.
	Pass,
	Run(Action),
	/// The window's own shortcut: back to its first control.
	FocusFirst,
	/// Another window's shortcut: close this one, open that one.
	Switch(Action),
	Close,
}

fn opens_window(a: &Action) -> bool {
	matches!(
		a,
		Action::OpenSearch
			| Action::OpenBrowse
			| Action::OpenFavorites
			| Action::OpenRecent
			| Action::OpenCollections
			| Action::NewCustomStation
			| Action::SongHistory
			| Action::Equalizer
			| Action::GlobalHotkeys
			| Action::KeyboardShortcuts
			| Action::FeatureSettings
	)
}

/// Route with the focused control in mind: in a text field, the
/// clipboard trio edits text; everywhere else it works on stations.
pub fn route_focused(
	settings: &blindradio_core::settings::Settings,
	own: &Action,
	code: i32,
	ctrl: bool,
	alt: bool,
	shift: bool,
	in_text: bool,
) -> Route {
	if in_text && crate::action::is_text_edit_chord(code, ctrl, alt, shift) {
		return Route::Pass;
	}
	route(settings, own, code, ctrl, alt, shift)
}

/// Whether the key event started in a text field. A char hook's event
/// object is the window the key was aimed at, so its class answers.
pub(crate) fn event_in_text(e: &WindowEventData) -> bool {
	let WindowEventData::Keyboard(key) = e else {
		return false;
	};
	key.event
		.event
		.get_event_object()
		.and_then(|w| w.get_class_name())
		.is_some_and(|c| c == "wxTextCtrl")
}

/// `own` is the shortcut that opens this window.
pub fn route(
	settings: &blindradio_core::settings::Settings,
	own: &Action,
	code: i32,
	ctrl: bool,
	alt: bool,
	shift: bool,
) -> Route {
	match app_key(settings, code, ctrl, alt, shift, false) {
		None => Route::Pass,
		Some(Action::Close) => Route::Close,
		Some(a) if a == *own => Route::FocusFirst,
		Some(a) if opens_window(&a) => Route::Switch(a),
		Some(a) => Route::Run(a),
	}
}

/// Shows `dialog` modally with the app keys; returns the window to open next, if any.
/// Results for `token` are dropped once it closes.
pub fn run(
	app: &Rc<RefCell<App>>,
	dialog: Dialog,
	own: Action,
	first: impl WxWidget + Copy + 'static,
	token: &WindowToken,
) -> Option<Action> {
	let next = Rc::new(RefCell::new(None));
	let (a, n) = (app.clone(), next.clone());
	dialog.on_char_hook(move |e| {
		let in_text = event_in_text(&e);
		let WindowEventData::Keyboard(key) = e else {
			return;
		};
		let route = key.get_key_code().map_or(Route::Pass, |code| {
			route_focused(
				&a.borrow().settings,
				&own,
				code,
				key.control_down(),
				key.alt_down(),
				key.shift_down(),
				in_text,
			)
		});
		if route == Route::Pass {
			return key.event.skip(true);
		}
		// Consumed, or the focused control gets it too.
		key.event.skip(false);
		match route {
			Route::Run(action) => dispatch(&a, action),
			Route::FocusFirst => first.set_focus(),
			Route::Switch(action) => {
				*n.borrow_mut() = Some(action);
				dialog.end_modal(ID_CANCEL);
			}
			Route::Close => dialog.end_modal(ID_CANCEL),
			Route::Pass => {}
		}
	});
	dialog.centre();
	app.borrow_mut().dialog_open = true;
	dialog.show_modal();
	token.close();
	{
		let mut a = app.borrow_mut();
		a.dialog_open = false;
		a.selected = None;
		a.tuner.list_closed();
	}
	dialog.destroy();
	next.take()
}

#[cfg(test)]
mod tests {
	use super::*;

	/// The old signature over a fresh install's defaults.
	fn route(own: &Action, code: i32, ctrl: bool, alt: bool, shift: bool) -> Route {
		super::route(
			&blindradio_core::settings::Settings::default(),
			own,
			code,
			ctrl,
			alt,
			shift,
		)
	}

	/// The old signature over a fresh install's defaults.
	fn route_focused(
		own: &Action,
		code: i32,
		ctrl: bool,
		alt: bool,
		shift: bool,
		in_text: bool,
	) -> Route {
		super::route_focused(
			&blindradio_core::settings::Settings::default(),
			own,
			code,
			ctrl,
			alt,
			shift,
			in_text,
		)
	}

	#[test]
	fn app_keys_in_text_field() {
		let own = Action::OpenSearch;
		let key = |c: char| c as i32;
		assert_eq!(
			route(&own, key('P'), true, false, false),
			Route::Run(Action::PlayPause)
		);
		assert_eq!(
			route(&own, WXK_UP, true, false, false),
			Route::Run(Action::VolumeUp)
		);
		// Typing reaches the text box.
		for c in ['A', 'a', 'P', '1', ' '] {
			assert_eq!(route(&own, key(c), false, false, false), Route::Pass, "{c}");
			assert_eq!(
				route(&own, key(c), false, false, true),
				Route::Pass,
				"Shift+{c}"
			);
		}
		assert_eq!(route(&own, WXK_LEFT, false, false, false), Route::Pass);
		assert_eq!(route(&own, WXK_RETURN, false, false, false), Route::Pass);
		// Main window only keys type in a dialog.
		assert_eq!(route(&own, key('I'), true, false, false), Route::Pass);
		assert_eq!(route(&own, key('1'), false, true, false), Route::Pass);

		assert_eq!(route(&own, key('F'), true, false, false), Route::FocusFirst);
		assert_eq!(
			route(&own, key('B'), true, false, false),
			Route::Switch(Action::OpenFavorites)
		);
		assert_eq!(route(&own, WXK_ESCAPE, false, false, false), Route::Close);
	}

	#[test]
	fn clipboard_trio_edits_text_in_a_text_field() {
		let own = Action::OpenSearch;
		let key = |c: char| c as i32;
		// In a text field the trio edits text, or Edit Station answers
		// "Nothing to paste" and the pasted URL never lands.
		for c in ['C', 'X', 'V'] {
			assert_eq!(
				route_focused(&own, key(c), true, false, false, true),
				Route::Pass,
				"Ctrl+{c} in text"
			);
			// On a list the same chords keep their station actions.
			assert!(matches!(
				route_focused(&own, key(c), true, false, false, false),
				Route::Run(_)
			));
		}
		// Other app keys still run from a text field.
		assert_eq!(
			route_focused(&own, key('P'), true, false, false, true),
			Route::Run(Action::PlayPause)
		);
	}
}
