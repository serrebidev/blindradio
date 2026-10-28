//! File > Global Hotkeys…: the chord editor.

use std::cell::RefCell;
use std::rc::Rc;

use wx_utils::shortcuts::prompt_for_shortcuts;

use crate::action::Action;
use crate::app::App;
use crate::hotkeys;

pub fn show(app: &Rc<RefCell<App>>) -> Option<Action> {
	let frame = app.borrow().win.frame;
	let model = hotkeys::Model::from_settings(&app.borrow().settings);
	// The poll timer must not rebuild the menu bar behind the dialog.
	app.borrow_mut().dialog_open = true;
	let edited = prompt_for_shortcuts(&frame, &model);
	let mut app = app.borrow_mut();
	app.dialog_open = false;
	if let Some(edited) = edited {
		app.settings.hotkeys = edited.to_settings();
		app.save_settings();
		app.reload_hotkeys();
	}
	None
}
