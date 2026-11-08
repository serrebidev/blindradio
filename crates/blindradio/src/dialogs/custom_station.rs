//! Add Custom Station (Ctrl+N): a name and a stream URL.

use std::cell::RefCell;
use std::rc::Rc;

use blindradio_core::library::StationDest;
use blindradio_core::settings::StationSettings;
use blindradio_core::station::Station;
use wx_utils::add_ok_cancel_footer;
use wxdragon::prelude::*;

use crate::action::Action;
use crate::app::App;
use crate::bridge::WindowToken;
use crate::dialogs::equalizer;
use crate::dialogs::features::{blank_none, choice, field, tri, untri};
use crate::strings as s;

const OK_ID: Id = ID_HIGHEST + 7000;

/// What a typed or pasted URL means. Copies from web pages and chat can
/// carry a BOM or zero-width characters in front of the scheme; those
/// bytes are part of neither the address nor its scheme, and a plain
/// prefix check on the raw text rejects a good URL for them.
pub fn clean_url(url: &str) -> String {
	url.trim()
		.trim_start_matches(['\u{feff}', '\u{200b}', '\u{200c}', '\u{200d}'])
		.trim()
		.to_string()
}

/// Only a stream an HTTP client can open will play.
pub fn is_stream_url(url: &str) -> bool {
	let url = clean_url(url).to_ascii_lowercase();
	url.starts_with("http://") || url.starts_with("https://")
}

/// The Name Automatically button, under the URL: fills the name field with what the station calls
/// itself and says it. A failed lookup says so and leaves what was typed; either way the name
/// stays an ordinary field to edit.
fn name_button(
	app: &Rc<RefCell<App>>,
	dialog: &Dialog,
	sizer: &BoxSizer,
	name: TextCtrl,
	url: TextCtrl,
	token: &WindowToken,
) {
	let button = Button::builder(dialog)
		.with_label(&s::name_automatically())
		.build();
	sizer.add(&button, 0, SizerFlag::All, 5);
	let (a, token) = (app.clone(), token.clone());
	button.on_click(move |_| {
		let link = url.get_value();
		if !is_stream_url(&link) {
			return a.borrow_mut().say(&s::not_a_stream_url());
		}
		let link = clean_url(&link);
		let mut app = a.borrow_mut();
		app.say(&s::looking_up_name());
		app.bridge.spawn(
			token.clone(),
			app.station_name_lookup(link),
			move |app: &mut App, found: Option<String>| match found {
				Some(found) => {
					name.set_value(&found);
					app.say(&found);
				}
				None => app.say(&s::no_name_found()),
			},
		);
	});
}

pub fn show(app: &Rc<RefCell<App>>) -> Option<Action> {
	let frame = app.borrow().win.frame;
	let token = WindowToken::new();
	let dialog = Dialog::builder(&frame, &s::add_custom_title()).build();
	let name_label = StaticText::builder(&dialog).with_label(&s::name()).build();
	let name = TextCtrl::builder(&dialog).build();
	let url_label = StaticText::builder(&dialog)
		.with_label(&s::stream_url())
		.build();
	let url = TextCtrl::builder(&dialog).build();
	// Where the new station goes: a collection or favorites folder, not
	// always Custom Stations (the ask behind the picker).
	let dests: Vec<(String, StationDest)> = app.borrow().library.destinations();
	let dest_label = StaticText::builder(&dialog)
		.with_label(&s::destination())
		.build();
	let dest = Choice::builder(&dialog).build();
	for (label, _) in &dests {
		dest.append(label);
	}
	dest.set_selection(0);
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
	name_button(app, &dialog, &sizer, name, url, &token);
	sizer.add(
		&dest_label,
		0,
		SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
		5,
	);
	sizer.add(&dest, 0, SizerFlag::Expand | SizerFlag::All, 5);
	// A bad URL must keep the window open, so OK is not a stock button.
	let ok = Button::builder(&dialog)
		.with_id(OK_ID)
		.with_label(&s::ok())
		.build();
	let cancel = Button::builder(&dialog)
		.with_id(ID_CANCEL)
		.with_label(&s::cancel())
		.build();
	dialog.set_affirmative_id(OK_ID);
	dialog.set_escape_id(ID_CANCEL);
	ok.set_default();
	add_ok_cancel_footer(sizer, ok, cancel);
	dialog.set_sizer_and_fit(sizer, true);

	{
		let (a, d) = (app.clone(), dialog);
		ok.on_click(move |_| {
			let link = url.get_value();
			if !is_stream_url(&link) {
				return a.borrow_mut().say(&s::not_a_stream_url());
			}
			let link = clean_url(&link);
			let typed = name.get_value().trim().to_string();
			// A station with no name is unreadable in every list: the URL stands in.
			let typed = if typed.is_empty() {
				link.clone()
			} else {
				typed
			};
			let chosen = dest
				.get_selection()
				.and_then(|i| dests.get(i as usize))
				.map(|(_, d)| d.clone())
				.unwrap_or(StationDest::Collection {
					id: blindradio_core::library::CUSTOM.into(),
					path: Vec::new(),
				});
			let station = {
				let mut app = a.borrow_mut();
				let station = app.library.add_custom_to(&typed, &link, &chosen);
				app.save_library();
				app.refresh_dial();
				station
			};
			crate::speech::focus_returning();
			a.borrow_mut().say(&format!("{}{}", s::added(), station.name));
			d.end_modal(ID_OK);
		});
	}

	name.set_focus();
	crate::dialogs::run(app, dialog, Action::NewCustomStation, name, &token)
}

/// Edit Station: the Add Custom fields filled in with the station's name,
/// info and URL, and the station's own settings (buffer minutes, recognition,
/// scrobbling, equalizer) below them — one dialog since the merge; there is no
/// separate Station Settings anymore. Saving rewrites every held copy
/// (collections, favorites), so an imported playlist's station can be
/// renamed and re-pointed in place.
pub fn show_edit(app: &Rc<RefCell<App>>, station: &Station) -> Option<Action> {
	let frame = app.borrow().win.frame;
	let token = WindowToken::new();
	let current = app
		.borrow()
		.settings
		.station_overrides
		.get(&station.id)
		.cloned()
		.unwrap_or_default();
	let dialog = Dialog::builder(&frame, &s::edit_station_title()).build();
	let name_label = StaticText::builder(&dialog).with_label(&s::name()).build();
	let name = TextCtrl::builder(&dialog).build();
	name.set_value(&station.name);
	// The text after the name in the lists ("Station - Country, AAC+ 47k").
	let info_label = StaticText::builder(&dialog)
		.with_label(&s::column_info())
		.build();
	let info = TextCtrl::builder(&dialog).build();
	info.set_value(station.info.as_deref().unwrap_or_default());
	let url_label = StaticText::builder(&dialog)
		.with_label(&s::stream_url())
		.build();
	let url = TextCtrl::builder(&dialog).build();
	url.set_value(&station.url);
	let sizer = BoxSizer::builder(Orientation::Vertical).build();
	for (label, control) in [
		(&name_label, &name),
		(&info_label, &info),
		(&url_label, &url),
	] {
		sizer.add(
			label,
			0,
			SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
			5,
		);
		sizer.add(control, 0, SizerFlag::Expand | SizerFlag::All, 5);
	}
	name_button(app, &dialog, &sizer, name, url, &token);
	let minutes = field(
		&dialog,
		&sizer,
		&s::st_ts_minutes(),
		&current
			.timeshift_minutes
			.map(|m| m.to_string())
			.unwrap_or_default(),
	);
	let metadata_offset = field(
		&dialog,
		&sizer,
		&s::st_metadata_offset(),
		&current.metadata_offset_seconds.map(|n| n.to_string()).unwrap_or_default(),
	);
	let (global, on, off) = (s::use_global(), s::on(), s::off());
	let recog = choice(
		&dialog,
		&sizer,
		&s::st_recog(),
		&[&global, &on, &off],
		tri(current.recognition),
	);
	let scrob = choice(
		&dialog,
		&sizer,
		&s::st_scrob(),
		&[&global, &on, &off],
		tri(current.scrobble),
	);
	let eq = app.borrow().settings.eq.clone();
	let (eq_at, preset_at) = equalizer::station_choices(&eq, current.eq.as_ref());
	let eq_mode = choice(
		&dialog,
		&sizer,
		&s::st_eq(),
		&[&global, &on, &off],
		eq_at,
	);
	let eq_preset = choice(
		&dialog,
		&sizer,
		&s::st_eq_preset(),
		&equalizer::preset_labels(&eq),
		preset_at,
	);
	// Only On has a preset: Use Global and Off leave this out of the tab order.
	eq_preset.enable(eq_at == 1);
	// Heard as it is chosen when this station is playing. OK keeps it; every
	// other way out takes it back below.
	let preview = {
		let (a, id, eq) = (app.clone(), station.id.clone(), eq.clone());
		move || {
			let own = equalizer::station_eq(&eq, eq_mode.get_selection(), eq_preset.get_selection());
			a.borrow_mut().preview_station_eq(&id, own);
		}
	};
	{
		let preview = preview.clone();
		eq_mode.on_selection_changed(move |_| {
			eq_preset.enable(eq_mode.get_selection() == Some(1));
			preview();
		});
	}
	eq_preset.on_selection_changed(move |_| preview());
	let ok = Button::builder(&dialog)
		.with_id(OK_ID)
		.with_label(&s::ok())
		.build();
	let cancel = Button::builder(&dialog)
		.with_id(ID_CANCEL)
		.with_label(&s::cancel())
		.build();
	dialog.set_affirmative_id(OK_ID);
	dialog.set_escape_id(ID_CANCEL);
	ok.set_default();
	add_ok_cancel_footer(sizer, ok, cancel);
	dialog.set_sizer_and_fit(sizer, true);

	{
		let (a, d) = (app.clone(), dialog);
		let id = station.id.clone();
		ok.on_click(move |_| {
			let link = url.get_value();
			if !is_stream_url(&link) {
				return a.borrow_mut().say(&s::not_a_stream_url());
			}
			let link = clean_url(&link);
			let typed = name.get_value().trim().to_string();
			let typed = if typed.is_empty() {
				link.clone()
			} else {
				typed
			};
			let metadata_offset_seconds = match blank_none(&metadata_offset.get_value()) {
				None => None,
				Some(value) => match value.parse::<i8>() {
					Ok(value) if (-30..=30).contains(&value) => Some(value),
					_ => return a.borrow_mut().say(&s::st_metadata_offset_invalid()),
				},
			};
			let entry = StationSettings {
				timeshift_minutes: blank_none(&minutes.get_value())
					.and_then(|t| t.parse().ok()),
				recognition: untri(recog.get_selection()),
				scrobble: untri(scrob.get_selection()),
				// Not this dialog's to set: the level the station
				// remembers from playing it.
				volume: current.volume,
				eq: equalizer::station_eq(&eq, eq_mode.get_selection(), eq_preset.get_selection()),
				metadata_offset_seconds,
			};
			{
				let mut app = a.borrow_mut();
				let info = blank_none(&info.get_value());
				if app.update_station(&id, &typed, &link, info.as_deref()) {
					app.save_library();
					app.refresh_dial();
				}
				if entry == StationSettings::default() {
					app.settings.station_overrides.remove(&id);
				} else {
					app.settings.station_overrides.insert(id.clone(), entry);
				}
				app.save_settings();
				app.refresh_eq();
			}
			crate::speech::focus_returning();
			a.borrow_mut().say(&format!("{}{}", s::saved(), typed));
			d.end_modal(ID_OK);
		});
	}

	name.set_focus();
	let next = crate::dialogs::run(app, dialog, Action::EditStation, name, &token);
	app.borrow_mut().refresh_eq();
	next
}

#[cfg(test)]
mod tests {
	use super::{clean_url, is_stream_url};

	#[test]
	fn custom_url_validation() {
		assert!(is_stream_url("http://example.com/a"));
		assert!(is_stream_url("  HTTPS://example.com/a  "));
		assert!(!is_stream_url("example.com/a"));
		assert!(!is_stream_url("ftp://example.com/a"));
		assert!(!is_stream_url(""));
	}

	#[test]
	fn pasted_url_baggage_is_cleaned() {
		assert!(is_stream_url("\u{feff}https://example.com/a"));
		assert!(is_stream_url("\u{200b}https://example.com/a"));
		assert_eq!(
			clean_url(" \u{feff}https://example.com/a "),
			"https://example.com/a"
		);
	}
}
