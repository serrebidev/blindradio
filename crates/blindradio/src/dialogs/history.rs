//! Song History (Ctrl+Y): the station's published list, else what played since tuning in.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::SystemTime;

use blindradio_core::history::{self, HistoryKind};
use blindradio_core::sources::HistoryItem;
use wxdragon::prelude::*;

use crate::action::Action;
use crate::app::App;
use crate::bridge::WindowToken;
use crate::clipboard;
use crate::strings as s;

/// Song alone, or Song and Played when the station publishes its times.
pub fn columns(kind: HistoryKind) -> usize {
	match kind {
		HistoryKind::Published => 2,
		HistoryKind::Session | HistoryKind::Nothing => 1,
	}
}

fn fill(
	list: &ListCtrl,
	heading: &StaticText,
	kind: HistoryKind,
	found: Vec<HistoryItem>,
	items: &RefCell<Vec<HistoryItem>>,
	played: &Cell<bool>,
) {
	heading.set_label(history::heading(kind));
	list.delete_all_items();
	if columns(kind) == 2 && !played.replace(true) {
		list.insert_column(1, s::COLUMN_PLAYED, ListColumnFormat::Left, 120);
	}
	let now = SystemTime::now();
	for (i, item) in found.iter().enumerate() {
		let row = i as i64;
		list.insert_item(row, &item.title, None);
		if columns(kind) == 2
			&& let Some(at) = item.played_at
		{
			list.set_item_text_by_column(row, 1, &history::ago(at, now));
		}
	}
	*items.borrow_mut() = found;
}

fn copy_focused(app: &Rc<RefCell<App>>, list: &ListCtrl, items: &Rc<RefCell<Vec<HistoryItem>>>) {
	let focused = usize::try_from(list.get_first_selected_item())
		.ok()
		.and_then(|i| items.borrow().get(i).cloned());
	let Some(item) = focused else {
		return;
	};
	let ok = clipboard::copy(&item.title);
	app.borrow_mut().say(clipboard::message(ok));
}

pub fn show(app: &Rc<RefCell<App>>) -> Option<Action> {
	let Some(station) = app.borrow().tuner.playing().cloned() else {
		app.borrow_mut().say(s::NOTHING_PLAYING);
		return None;
	};
	let frame = app.borrow().win.frame;
	let dialog = Dialog::builder(&frame, s::SONG_HISTORY_TITLE).build();
	let heading = StaticText::builder(&dialog).with_label("").build();
	// The label first: wxMSW names the list after it.
	let label = StaticText::builder(&dialog).with_label(s::SONGS).build();
	let list = ListCtrl::builder(&dialog)
		.with_style(ListCtrlStyle::Report | ListCtrlStyle::SingleSel)
		.build();
	list.insert_column(0, s::COLUMN_SONG, ListColumnFormat::Left, 360);
	let copy = Button::builder(&dialog).with_label(s::COPY).build();
	let close = Button::builder(&dialog)
		.with_id(ID_CANCEL)
		.with_label(s::CLOSE)
		.build();
	let buttons = BoxSizer::builder(Orientation::Horizontal).build();
	buttons.add(&copy, 0, SizerFlag::All, 5);
	buttons.add(&close, 0, SizerFlag::All, 5);
	let sizer = BoxSizer::builder(Orientation::Vertical).build();
	sizer.add(&heading, 0, SizerFlag::Expand | SizerFlag::All, 5);
	sizer.add(
		&label,
		0,
		SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
		5,
	);
	sizer.add(&list, 1, SizerFlag::Expand | SizerFlag::All, 5);
	sizer.add_sizer(&buttons, 0, SizerFlag::All, 0);
	dialog.set_sizer(sizer, true);
	dialog.set_size(Size::new(560, 480));

	let items = Rc::new(RefCell::new(Vec::new()));
	let played = Rc::new(Cell::new(false));
	let (kind, found) = history::view(None, &app.borrow().history);
	fill(&list, &heading, kind, found, &items, &played);

	// Published times replace the session list when they arrive.
	let token = WindowToken::new();
	{
		let (l, h, it, pc) = (list, heading, items.clone(), played.clone());
		let sources = app.borrow().sources.clone();
		app.borrow().bridge.spawn(
			token.clone(),
			async move { sources.history(&station).await },
			move |app: &mut App, published| {
				let (kind, found) = history::view(published, &app.history);
				fill(&l, &h, kind, found, &it, &pc);
			},
		);
	}

	{
		let (a, it) = (app.clone(), items.clone());
		copy.on_click(move |_| copy_focused(&a, &list, &it));
	}
	{
		let (a, it) = (app.clone(), items.clone());
		list.on_item_activated(move |_| copy_focused(&a, &list, &it));
	}

	list.set_focus();
	crate::dialogs::run(app, dialog, Action::SongHistory, list, &token)
}

#[cfg(test)]
mod tests {
	use super::columns;
	use blindradio_core::history::HistoryKind;

	#[test]
	fn history_columns_for_kind() {
		assert_eq!(columns(HistoryKind::Published), 2, "times are known");
		assert_eq!(columns(HistoryKind::Session), 1);
		assert_eq!(
			columns(HistoryKind::Nothing),
			1,
			"nothing to show either way"
		);
	}
}
