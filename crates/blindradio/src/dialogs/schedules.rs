//! Scheduled Recordings: the list, and the add/edit form. Entries live
//! in the settings; App::check_schedules starts and stops them.

use std::cell::RefCell;
use std::rc::Rc;

use blindradio_core::schedule::{Repeat, ScheduledRecording};
use blindradio_core::station::Station;
use wx_utils::add_ok_cancel_footer;
use wxdragon::prelude::*;

use crate::action::Action;
use crate::app::App;
use crate::bridge::WindowToken;
use crate::strings as s;
use crate::update;

const OK_ID: Id = ID_HIGHEST + 7100;

// --- Local time -------------------------------------------------------
// No timezone database is linked in; the offset is calibrated from
// wx's own clock (wxDateTime::now broken down as if UTC, minus the true
// epoch) and used for every conversion.
// ponytail: one fixed offset — an entry across a DST changeover can be
// off by an hour; a real tz database is the upgrade path.

fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
	let y = if m <= 2 { y - 1 } else { y };
	let era = if y >= 0 { y } else { y - 399 } / 400;
	let yoe = y - era * 400;
	let mp = if m > 2 { m - 3 } else { m + 9 } as i64;
	let doy = (153 * mp + 2) / 5 + d as i64 - 1;
	let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
	era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
	let z = z + 719_468;
	let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
	let doe = z - era * 146_097;
	let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
	let y = yoe + era * 400;
	let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
	let mp = (5 * doy + 2) / 153;
	let d = doy - (153 * mp + 2) / 5 + 1;
	let m = if mp < 10 { mp + 3 } else { mp - 9 };
	(if m <= 2 { y + 1 } else { y }, m as u32, d as u32)
}

fn local_offset() -> i64 {
	let now = DateTime::now();
	let naive = days_from_civil(i64::from(now.year()), now.month() as u32, u32::from(now.day()))
		* 86_400
		+ i64::from(now.hour()) * 3600
		+ i64::from(now.minute()) * 60
		+ i64::from(now.second());
	naive - update::now() as i64
}

/// "2026-11-02 19:30" in local time to Unix seconds.
pub fn parse_local(text: &str) -> Option<u64> {
	let (date, time) = text.trim().split_once(' ')?;
	let mut d = date.split('-');
	let (y, mo, da) = (
		d.next()?.parse::<i64>().ok()?,
		d.next()?.parse::<u32>().ok()?,
		d.next()?.parse::<u32>().ok()?,
	);
	if d.next().is_some() || !(1..=12).contains(&mo) || !(1..=31).contains(&da) {
		return None;
	}
	let mut t = time.split(':');
	let (h, mi) = (
		t.next()?.parse::<u32>().ok()?,
		t.next()?.parse::<u32>().ok()?,
	);
	if t.next().is_some() || h > 23 || mi > 59 {
		return None;
	}
	let naive = days_from_civil(y, mo, da) * 86_400 + i64::from(h) * 3600 + i64::from(mi) * 60;
	Some(naive.checked_sub(local_offset())? as u64)
}

/// Unix seconds as "2026-11-02 19:30" in local time.
pub fn format_local(unix: u64) -> String {
	let local = unix as i64 + local_offset();
	let (days, rem) = (local.div_euclid(86_400), local.rem_euclid(86_400));
	let (y, m, d) = civil_from_days(days);
	format!(
		"{y:04}-{m:02}-{d:02} {:02}:{:02}",
		rem / 3600,
		rem % 3600 / 60
	)
}

// --- The list -----------------------------------------------------------

fn entry_label(e: &ScheduledRecording) -> String {
	// The next start still ahead, or the first one for a spent entry.
	let shown = e
		.next_after(update::now().saturating_sub(1))
		.unwrap_or(e.start_unix);
	format!(
		"{} - {} - {} min - {} - {}",
		e.station.name,
		format_local(shown),
		e.duration_minutes,
		e.repeat.label(),
		if e.enabled { "on" } else { "off" }
	)
}

fn fill(list: &ListBox, app: &App) {
	list.clear();
	for e in &app.settings.schedules {
		list.append(&entry_label(e));
	}
	if app.settings.schedules.is_empty() {
		list.append(s::NO_SCHEDULES);
	}
}

fn selected(list: &ListBox, app: &App) -> Option<usize> {
	let i = list.get_selection()? as usize;
	(i < app.settings.schedules.len()).then_some(i)
}

/// Every station the form can offer: the library's, by name.
fn station_choices(app: &App) -> Vec<Station> {
	let mut out: Vec<Station> = Vec::new();
	for c in &app.library.collections {
		out.extend(c.all_stations());
	}
	out.extend(app.library.favorites.iter().cloned());
	out.extend(app.library.fav_folders.iter().flat_map(|f| f.all_stations()));
	out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
	out.dedup_by(|a, b| a.id == b.id);
	out
}

pub fn show(app: &Rc<RefCell<App>>) -> Option<Action> {
	let frame = app.borrow().win.frame;
	let dialog = Dialog::builder(&frame, s::SCHEDULES_TITLE).build();
	let label = StaticText::builder(&dialog)
		.with_label(s::SCHEDULES_LIST)
		.build();
	let list = ListBox::builder(&dialog).build();
	let add = Button::builder(&dialog).with_label(s::ADD_BUTTON).build();
	let edit = Button::builder(&dialog).with_label(s::EDIT_BUTTON).build();
	let delete = Button::builder(&dialog).with_label(s::DELETE_BUTTON).build();
	let close = Button::builder(&dialog)
		.with_id(ID_CANCEL)
		.with_label(s::CLOSE)
		.build();
	let sizer = BoxSizer::builder(Orientation::Vertical).build();
	sizer.add(&label, 0, SizerFlag::Left | SizerFlag::Right | SizerFlag::Top, 5);
	sizer.add(&list, 1, SizerFlag::Expand | SizerFlag::All, 5);
	let buttons = BoxSizer::builder(Orientation::Horizontal).build();
	for b in [&add, &edit, &delete, &close] {
		buttons.add(b, 0, SizerFlag::All, 5);
	}
	sizer.add_sizer(&buttons, 0, SizerFlag::AlignRight, 0);
	dialog.set_sizer(sizer, true);
	dialog.set_size(Size::new(560, 380));
	fill(&list, &app.borrow());
	if !app.borrow().settings.schedules.is_empty() {
		list.set_selection(0, true);
	}

	{
		let (a, d) = (app.clone(), dialog);
		add.on_click(move |_| {
			let stations = station_choices(&a.borrow());
			if stations.is_empty() {
				return a.borrow_mut().say(s::NOTHING_TO_PLAY);
			}
			if let Some(entry) = form(&a, &d, None, &stations) {
				let mut app = a.borrow_mut();
				app.settings.schedules.push(entry);
				app.save_settings();
				fill(&list, &app);
			}
		});
	}
	{
		let (a, d) = (app.clone(), dialog);
		edit.on_click(move |_| edit_entry(&a, &d, &list));
	}
	{
		let a = app.clone();
		delete.on_click(move |_| {
			let Some(i) = selected(&list, &a.borrow()) else {
				return;
			};
			let mut app = a.borrow_mut();
			app.settings.schedules.remove(i);
			app.save_settings();
			fill(&list, &app);
		});
	}
	{
		let (a, d) = (app.clone(), dialog);
		list.on_item_double_clicked(move |_| edit_entry(&a, &d, &list));
	}

	let token = WindowToken::new();
	list.set_focus();
	crate::dialogs::run(app, dialog, Action::ScheduledRecordings, list, &token)
}

fn edit_entry(app: &Rc<RefCell<App>>, dialog: &Dialog, list: &ListBox) {
	let Some(i) = selected(list, &app.borrow()) else {
		return;
	};
	let stations = station_choices(&app.borrow());
	let current = app.borrow().settings.schedules[i].clone();
	if let Some(entry) = form(app, dialog, Some(&current), &stations) {
		let mut app = app.borrow_mut();
		app.settings.schedules[i] = entry;
		app.save_settings();
		fill(list, &app);
		list.set_selection(i as u32, true);
	}
}

// --- The form -----------------------------------------------------------

fn form(
	app: &Rc<RefCell<App>>,
	parent: &Dialog,
	existing: Option<&ScheduledRecording>,
	stations_in: &[Station],
) -> Option<ScheduledRecording> {
	let stations: Rc<Vec<Station>> = Rc::new(stations_in.to_vec());
	let dialog = Dialog::builder(parent, s::SCHEDULES_TITLE).build();
	let station_label = StaticText::builder(&dialog)
		.with_label(s::SCHEDULE_STATION)
		.build();
	let station = ListBox::builder(&dialog).build();
	for st in stations.iter() {
		station.append(&st.name);
	}
	let start_label = StaticText::builder(&dialog)
		.with_label(s::SCHEDULE_START)
		.build();
	let start = TextCtrl::builder(&dialog).build();
	let duration_label = StaticText::builder(&dialog)
		.with_label(s::SCHEDULE_DURATION)
		.build();
	let duration = TextCtrl::builder(&dialog).build();
	let repeat_label = StaticText::builder(&dialog)
		.with_label(s::SCHEDULE_REPEAT)
		.build();
	let repeat = Choice::builder(&dialog).build();
	for r in [Repeat::Once, Repeat::Daily, Repeat::Weekly] {
		repeat.append(r.label());
	}
	let enabled = CheckBox::builder(&dialog).with_label(s::ENABLED).build();

	let mut entry = match existing {
		Some(e) => e.clone(),
		None => ScheduledRecording {
			id: format!("sched:{}", update::now()),
			station: stations[0].clone(),
			start_unix: update::now() + 3600,
			duration_minutes: 60,
			repeat: Repeat::Once,
			enabled: true,
		},
	};
	if let Some(i) = stations.iter().position(|st| st.id == entry.station.id) {
		station.set_selection(i as u32, true);
	} else {
		station.set_selection(0, true);
	}
	start.set_value(&format_local(entry.start_unix));
	duration.set_value(&entry.duration_minutes.to_string());
	repeat.set_selection(match entry.repeat {
		Repeat::Once => 0,
		Repeat::Daily => 1,
		Repeat::Weekly => 2,
	});
	enabled.set_value(entry.enabled);

	let sizer = BoxSizer::builder(Orientation::Vertical).build();
	fn row(sizer: &BoxSizer, label: &StaticText, control: &impl WxWidget) {
		sizer.add(
			label,
			0,
			SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
			5,
		);
		sizer.add(control, 0, SizerFlag::Expand | SizerFlag::All, 5);
	}
	row(&sizer, &station_label, &station);
	row(&sizer, &start_label, &start);
	row(&sizer, &duration_label, &duration);
	row(&sizer, &repeat_label, &repeat);
	sizer.add(&enabled, 0, SizerFlag::All, 5);
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

	// OK validates in place: a bad field keeps the form open and says why.
	let out = Rc::new(RefCell::new(None));
	{
		let stations = stations.clone();
		let (a, d, out) = (app.clone(), dialog, out.clone());
		ok.on_click(move |_| {
			let Some(st) = station
				.get_selection()
				.and_then(|i| stations.get(i as usize))
			else {
				return;
			};
			entry.station = st.clone();
			match parse_local(&start.get_value()) {
				Some(unix) => entry.start_unix = unix,
				None => return a.borrow_mut().say(s::BAD_SCHEDULE_TIME),
			}
			match duration.get_value().trim().parse::<u32>() {
				Ok(m) if (1..=1440).contains(&m) => entry.duration_minutes = m,
				_ => return a.borrow_mut().say(s::BAD_SCHEDULE_DURATION),
			}
			entry.repeat = match repeat.get_selection() {
				Some(1) => Repeat::Daily,
				Some(2) => Repeat::Weekly,
				_ => Repeat::Once,
			};
			entry.enabled = enabled.get_value();
			*out.borrow_mut() = Some(entry.clone());
			d.end_modal(ID_OK);
		});
	}
	station.set_focus();
	if dialog.show_modal() == ID_OK {
		out.borrow().clone()
	} else {
		None
	}
}

#[cfg(test)]
mod tests {
	use super::{civil_from_days, days_from_civil};

	#[test]
	fn civil_math_round_trips() {
		for (y, m, d) in [
			(1970, 1, 1),
			(2000, 2, 29),
			(2026, 10, 30),
			(2026, 3, 1),
			(1999, 12, 31),
		] {
			assert_eq!(civil_from_days(days_from_civil(y, m, d)), (y, m, d));
		}
		assert_eq!(days_from_civil(1970, 1, 1), 0);
	}
}
