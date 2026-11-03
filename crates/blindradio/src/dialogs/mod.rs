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
#[allow(clippy::too_many_arguments)]
pub fn route_focused(
	settings: &blindradio_core::settings::Settings,
	own: &Action,
	code: i32,
	ctrl: bool,
	alt: bool,
	shift: bool,
	in_text: bool,
	on_list: bool,
) -> Route {
	if in_text && crate::action::is_text_edit_chord(code, ctrl, alt, shift) {
		return Route::Pass;
	}
	if crate::action::space_stays_native(code, ctrl, alt, shift, on_list) {
		return Route::Pass;
	}
	route(settings, own, code, ctrl, alt, shift)
}

/// The class of the window a key event was aimed at: a char hook's
/// event object is that window.
fn event_class(e: &WindowEventData) -> Option<String> {
	let WindowEventData::Keyboard(key) = e else {
		return None;
	};
	key.event.event.get_event_object()?.get_class_name()
}

/// Whether the key event started in a text field.
pub(crate) fn event_in_text(e: &WindowEventData) -> bool {
	event_class(e).is_some_and(|c| c == "wxTextCtrl")
}

/// Whether the key event started on a station list, a tree or a slider.
pub(crate) fn event_on_list(e: &WindowEventData) -> bool {
	event_class(e)
		.is_some_and(|c| c.contains("TreeCtrl") || c.contains("ListCtrl") || c.contains("Slider"))
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
	run_with_refresh(app, dialog, own, first, token, || {})
}

/// A dialog whose local view follows library changes made by app shortcuts.
pub fn run_with_refresh(
	app: &Rc<RefCell<App>>,
	dialog: Dialog,
	own: Action,
	first: impl WxWidget + Copy + 'static,
	token: &WindowToken,
	refresh: impl Fn() + 'static,
) -> Option<Action> {
	let next = Rc::new(RefCell::new(None));
	let (a, n) = (app.clone(), next.clone());
	dialog.on_char_hook(move |e| {
		let in_text = event_in_text(&e);
		let on_list = event_on_list(&e);
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
				on_list,
			)
		});
		if route == Route::Pass {
			return key.event.skip(true);
		}
		// Consumed, or the focused control gets it too.
		key.event.skip(false);
		match route {
			Route::Run(action) => {
				let pasted = action == Action::PasteStation;
				dispatch(&a, action);
				if pasted {
					refresh();
				}
			}
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
	// Edit Station can open over Browse, Collections or Favorites: closing
	// it hands everything back to the window still open underneath.
	let outer = {
		let mut a = app.borrow_mut();
		let outer = a
			.dialog_open
			.then(|| (a.selected.clone(), a.paste_home.clone()));
		a.dialog_open = true;
		outer
	};
	dialog.show_modal();
	token.close();
	{
		let mut a = app.borrow_mut();
		if let Some((selected, home)) = outer {
			a.selected = selected;
			a.paste_home = home;
		} else {
			a.dialog_open = false;
			a.selected = a
				.win
				.stations
				.current()
				.and_then(|i| a.win.stations.stations().get(i).cloned());
			a.reveal_after_dialog();
			// A paste home belongs to the dialog that stated it.
			a.paste_home = None;
			a.tuner.list_closed();
		}
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
			!in_text,
		)
	}

	#[test]
	fn space_plays_or_pauses_only_on_a_list() {
		let own = Action::OpenSearch;
		let space = ' ' as i32;
		let settings = blindradio_core::settings::Settings::default();
		let on = |in_text, on_list| {
			super::route_focused(&settings, &own, space, false, false, false, in_text, on_list)
		};
		assert_eq!(on(false, true), Route::Run(Action::PlayPause), "list or tree");
		assert_eq!(on(true, false), Route::Pass, "typing a space");
		assert_eq!(on(false, false), Route::Pass, "a button or check box presses");
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
		for c in ['A', 'a', 'P', '1'] {
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
