//! Feature Settings (Ctrl+T) and Station Settings (context menu): the
//! time-shift, recording, recognition and scrobble options, applied on OK.

use std::cell::RefCell;
use std::rc::Rc;

use blindradio_core::settings::{RecordFormat, RecognitionProvider, StationSettings};
use blindradio_core::station::Station;
use wx_utils::add_ok_cancel_footer;
use wxdragon::prelude::*;

use crate::action::Action;
use crate::app::App;
use crate::bridge::WindowToken;
use crate::strings as s;

const OK_ID: Id = ID_HIGHEST + 7001;

fn field(dialog: &Dialog, sizer: &BoxSizer, label: &str, value: &str) -> TextCtrl {
	let text = StaticText::builder(dialog).with_label(label).build();
	let control = TextCtrl::builder(dialog).build();
	control.set_value(value);
	sizer.add(
		&text,
		0,
		SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
		5,
	);
	sizer.add(&control, 0, SizerFlag::Expand | SizerFlag::All, 5);
	control
}

fn check(dialog: &Dialog, sizer: &BoxSizer, label: &str, value: bool) -> CheckBox {
	let control = CheckBox::builder(dialog).with_label(label).build();
	control.set_value(value);
	sizer.add(&control, 0, SizerFlag::All, 5);
	control
}

fn choice(
	dialog: &Dialog,
	sizer: &BoxSizer,
	label: &str,
	items: &[&str],
	at: usize,
) -> Choice {
	let text = StaticText::builder(dialog).with_label(label).build();
	let control = Choice::builder(dialog).build();
	for item in items {
		control.append(item);
	}
	control.set_selection(at as u32);
	sizer.add(
		&text,
		0,
		SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
		5,
	);
	sizer.add(&control, 0, SizerFlag::Expand | SizerFlag::All, 5);
	control
}

fn footer(dialog: &Dialog, sizer: BoxSizer) -> Button {
	let ok = Button::builder(dialog)
		.with_id(OK_ID)
		.with_label(s::OK)
		.build();
	let cancel = Button::builder(dialog)
		.with_id(ID_CANCEL)
		.with_label(s::CANCEL)
		.build();
	dialog.set_affirmative_id(OK_ID);
	dialog.set_escape_id(ID_CANCEL);
	ok.set_default();
	add_ok_cancel_footer(sizer, ok, cancel);
	ok
}

fn parse_or<T: std::str::FromStr>(text: &str, fallback: T) -> T {
	text.trim().parse().unwrap_or(fallback)
}

fn blank_none(text: &str) -> Option<String> {
	let t = text.trim();
	if t.is_empty() { None } else { Some(t.to_string()) }
}

const FORMATS: [RecordFormat; 4] = [
	RecordFormat::Original,
	RecordFormat::Wav,
	RecordFormat::Mp3,
	RecordFormat::Flac,
];

pub fn show(app: &Rc<RefCell<App>>) -> Option<Action> {
	let frame = app.borrow().win.frame;
	let current = app.borrow().settings.clone();
	let dialog = Dialog::builder(&frame, s::FEATURES_TITLE).build();
	let sizer = BoxSizer::builder(Orientation::Vertical).build();

	let ts_on = check(&dialog, &sizer, s::TS_ENABLE, current.timeshift.enabled);
	let ts_minutes = field(
		&dialog,
		&sizer,
		s::TS_MINUTES,
		&current.timeshift.minutes.to_string(),
	);
	let seek_step = field(
		&dialog,
		&sizer,
		s::SEEK_STEP,
		&current.timeshift.seek_step_seconds.to_string(),
	);
	let format_at = FORMATS
		.iter()
		.position(|f| *f == current.recording.format)
		.unwrap_or(0);
	let format = choice(
		&dialog,
		&sizer,
		s::REC_FORMAT,
		&[s::FMT_ORIGINAL, s::FMT_WAV, s::FMT_MP3, s::FMT_FLAC],
		format_at,
	);
	let bitrate = field(
		&dialog,
		&sizer,
		s::REC_BITRATE,
		&current.recording.mp3_bitrate_kbps.to_string(),
	);
	let vbr = check(&dialog, &sizer, s::REC_VBR, current.recording.mp3_vbr);
	let rec_dir = field(
		&dialog,
		&sizer,
		s::REC_DIR,
		current.recording.dir.as_deref().unwrap_or(""),
	);
	let recog_on = check(&dialog, &sizer, s::RECOG_ENABLE, current.recognition.enabled);
	let provider = choice(
		&dialog,
		&sizer,
		s::RECOG_PROVIDER,
		&[s::PROV_METADATA, s::PROV_AUDD],
		match current.recognition.provider {
			RecognitionProvider::Metadata => 0,
			RecognitionProvider::Audd => 1,
		},
	);
	let audd_token = field(
		&dialog,
		&sizer,
		s::RECOG_TOKEN,
		current.recognition.audd_token.as_deref().unwrap_or(""),
	);
	let only_meta = check(
		&dialog,
		&sizer,
		s::RECOG_ONLY_META,
		current.recognition.only_when_no_metadata,
	);
	let recog_announce = check(
		&dialog,
		&sizer,
		s::RECOG_ANNOUNCE,
		current.recognition.announce,
	);
	let scrob_on = check(&dialog, &sizer, s::SCROB_ENABLE, current.scrobble.enabled);
	let lb_token = field(
		&dialog,
		&sizer,
		s::SCROB_LB_TOKEN,
		current.scrobble.listenbrainz_token.as_deref().unwrap_or(""),
	);
	let fm_key = field(
		&dialog,
		&sizer,
		s::SCROB_KEY,
		current.scrobble.lastfm_api_key.as_deref().unwrap_or(""),
	);
	let fm_secret = field(
		&dialog,
		&sizer,
		s::SCROB_SECRET,
		current.scrobble.lastfm_secret.as_deref().unwrap_or(""),
	);
	let fm_session = field(
		&dialog,
		&sizer,
		s::SCROB_SESSION,
		current.scrobble.lastfm_session_key.as_deref().unwrap_or(""),
	);
	let fm_base = field(
		&dialog,
		&sizer,
		s::SCROB_BASE,
		current.scrobble.lastfm_base.as_deref().unwrap_or(""),
	);
	let ok = footer(&dialog, sizer);
	dialog.set_sizer_and_fit(sizer, true);

	let token = WindowToken::new();
	{
		let (a, d) = (app.clone(), dialog);
		ok.on_click(move |_| {
			let mut app = a.borrow_mut();
			let st = &mut app.settings;
			st.timeshift.enabled = ts_on.get_value();
			st.timeshift.minutes = parse_or(&ts_minutes.get_value(), st.timeshift.minutes);
			st.timeshift.seek_step_seconds =
				parse_or(&seek_step.get_value(), st.timeshift.seek_step_seconds);
			st.recording.format = FORMATS
				.get(format.get_selection().unwrap_or(0) as usize)
				.copied()
				.unwrap_or_default();
			st.recording.mp3_bitrate_kbps =
				parse_or(&bitrate.get_value(), st.recording.mp3_bitrate_kbps);
			st.recording.mp3_vbr = vbr.get_value();
			st.recording.dir = blank_none(&rec_dir.get_value());
			st.recognition.enabled = recog_on.get_value();
			st.recognition.provider = if provider.get_selection() == Some(1) {
				RecognitionProvider::Audd
			} else {
				RecognitionProvider::Metadata
			};
			st.recognition.audd_token = blank_none(&audd_token.get_value());
			st.recognition.only_when_no_metadata = only_meta.get_value();
			st.recognition.announce = recog_announce.get_value();
			st.scrobble.enabled = scrob_on.get_value();
			st.scrobble.listenbrainz_token = blank_none(&lb_token.get_value());
			st.scrobble.lastfm_api_key = blank_none(&fm_key.get_value());
			st.scrobble.lastfm_secret = blank_none(&fm_secret.get_value());
			st.scrobble.lastfm_session_key = blank_none(&fm_session.get_value());
			st.scrobble.lastfm_base = blank_none(&fm_base.get_value());
			app.save_settings();
			d.end_modal(ID_OK);
		});
	}

	ts_on.set_focus();
	crate::dialogs::run(app, dialog, Action::FeatureSettings, ts_on, &token)
}

/// A tri-state override as a choice index: 0 global, 1 on, 2 off.
fn tri(value: Option<bool>) -> usize {
	match value {
		None => 0,
		Some(true) => 1,
		Some(false) => 2,
	}
}

fn untri(at: Option<u32>) -> Option<bool> {
	match at {
		Some(1) => Some(true),
		Some(2) => Some(false),
		_ => None,
	}
}

pub fn show_station(app: &Rc<RefCell<App>>, station: &Station) -> Option<Action> {
	let frame = app.borrow().win.frame;
	let current = app
		.borrow()
		.settings
		.station_overrides
		.get(&station.id)
		.cloned()
		.unwrap_or_default();
	let title = format!("{}: {}", s::STATION_SETTINGS_TITLE, station.name);
	let dialog = Dialog::builder(&frame, &title).build();
	let sizer = BoxSizer::builder(Orientation::Vertical).build();

	let minutes = field(
		&dialog,
		&sizer,
		s::ST_TS_MINUTES,
		&current
			.timeshift_minutes
			.map(|m| m.to_string())
			.unwrap_or_default(),
	);
	let recog = choice(
		&dialog,
		&sizer,
		s::ST_RECOG,
		&[s::USE_GLOBAL, s::ON, s::OFF],
		tri(current.recognition),
	);
	let scrob = choice(
		&dialog,
		&sizer,
		s::ST_SCROB,
		&[s::USE_GLOBAL, s::ON, s::OFF],
		tri(current.scrobble),
	);
	let ok = footer(&dialog, sizer);
	dialog.set_sizer_and_fit(sizer, true);

	let token = WindowToken::new();
	let id = station.id.clone();
	{
		let (a, d) = (app.clone(), dialog);
		ok.on_click(move |_| {
			let entry = StationSettings {
				timeshift_minutes: blank_none(&minutes.get_value())
					.and_then(|t| t.parse().ok()),
				recognition: untri(recog.get_selection()),
				scrobble: untri(scrob.get_selection()),
			};
			let mut app = a.borrow_mut();
			if entry == StationSettings::default() {
				app.settings.station_overrides.remove(&id);
			} else {
				app.settings.station_overrides.insert(id.clone(), entry);
			}
			app.save_settings();
			d.end_modal(ID_OK);
		});
	}

	minutes.set_focus();
	crate::dialogs::run(app, dialog, Action::StationSettings, minutes, &token)
}
