//! Settings (Ctrl+T), in tabs: general, playback, recording, and recognition and scrobbling.
//! Every tab applies on OK.

use std::cell::RefCell;
use std::rc::Rc;

use blindradio_core::settings::{BufferSettings, RecordFormat, RecognitionProvider};
use wx_utils::add_ok_cancel_footer;
use wxdragon::prelude::*;

use crate::action::Action;
use crate::app::App;
use crate::bridge::WindowToken;
use crate::strings as s;

const OK_ID: Id = ID_HIGHEST + 7001;

/// A page of Settings, the first one added showing. The caller fills the sizer, then sets it.
fn tab(notebook: &Notebook, title: &str) -> (Panel, BoxSizer) {
	let panel = Panel::builder(notebook)
		.with_style(PanelStyle::TabTraversal)
		.build();
	notebook.add_page(&panel, title, notebook.get_page_count() == 0, None);
	(panel, BoxSizer::builder(Orientation::Vertical).build())
}

pub(crate) fn field(parent: &impl WxWidget, sizer: &BoxSizer, label: &str, value: &str) -> TextCtrl {
	let text = StaticText::builder(parent).with_label(label).build();
	let control = TextCtrl::builder(parent).build();
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

/// A number as a text field and a slider that follow each other. The field takes any number
/// typed; the slider shows it inside `range`, and the arrows move it by `step`. The field is
/// what is read back, so a value is whatever was last typed or slid to.
fn slider_field(
	parent: &impl WxWidget,
	sizer: &BoxSizer,
	label: &str,
	range: (u32, u32),
	step: i32,
	value: u32,
) -> TextCtrl {
	let text = field(parent, sizer, label, &value.to_string());
	let (min, max) = (range.0 as i32, range.1 as i32);
	let caption = StaticText::builder(parent)
		.with_label(&format!(
			"{}{}",
			label.trim_end_matches(':'),
			s::SLIDER_SUFFIX
		))
		.build();
	let slider = Slider::builder(parent)
		.with_min_value(min)
		.with_max_value(max)
		.with_value(value as i32)
		.build();
	sizer.add(
		&caption,
		0,
		SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
		5,
	);
	sizer.add(&slider, 0, SizerFlag::Expand | SizerFlag::All, 5);
	slider.on_slider(move |e| text.change_value(&e.get_value().to_string()));
	slider.on_key_down(move |e| {
		let WindowEventData::Keyboard(key) = e else {
			return;
		};
		let plain = !(key.control_down() || key.alt_down() || key.shift_down());
		let delta = key
			.get_key_code()
			.and_then(crate::main_window::volume_step)
			.filter(|_| plain);
		match delta {
			Some(delta) => {
				// Consumed, or the native trackbar moves it as well (the wrong way). The
				// volume keys step by 5 and page by 10: here, by one step and ten.
				key.event.skip(false);
				let by = if delta.abs() == 10 { step * 10 } else { step };
				let moved = slider.get_value() + by * delta.signum();
				let moved = moved.clamp(min, max);
				slider.set_value(moved);
				text.change_value(&moved.to_string());
			}
			None => key.event.skip(true),
		}
	});
	text.on_text_updated(move |_| {
		if let Ok(typed) = text.get_value().trim().parse::<i32>() {
			slider.set_value(typed.clamp(min, max));
		}
	});
	text
}

fn check(parent: &impl WxWidget, sizer: &BoxSizer, label: &str, value: bool) -> CheckBox {
	let control = CheckBox::builder(parent).with_label(label).build();
	control.set_value(value);
	sizer.add(&control, 0, SizerFlag::All, 5);
	control
}

pub(crate) fn choice(
	parent: &impl WxWidget,
	sizer: &BoxSizer,
	label: &str,
	items: &[&str],
	at: usize,
) -> Choice {
	let text = StaticText::builder(parent).with_label(label).build();
	let control = Choice::builder(parent).build();
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

pub(crate) fn blank_none(text: &str) -> Option<String> {
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
	// The startup list choices: the default stations, every playlist merged,
	// or any one collection (an imported playlist is a collection).
	let mut collections: Vec<(String, String)> = app
		.borrow()
		.library
		.collections
		.iter()
		.map(|c| (c.id.clone(), c.name.clone()))
		.collect();
	collections.sort_by_key(|(_, name)| name.to_lowercase());
	let dialog = Dialog::builder(&frame, s::FEATURES_TITLE).build();
	let notebook = Notebook::builder(&dialog).build();

	let (panel, sizer) = tab(&notebook, s::TAB_GENERAL);
	let ann_titles = check(&panel, &sizer, s::ANNOUNCE_TITLES, current.announce_titles);
	let ann_status = check(&panel, &sizer, s::ANNOUNCE_STATUS, current.announce_status);
	let ann_actions = check(&panel, &sizer, s::ANNOUNCE_ACTIONS, current.announce_actions);
	let close_lists = check(
		&panel,
		&sizer,
		s::CLOSE_LISTS,
		current.close_lists_when_playing,
	);
	let play_last = check(&panel, &sizer, s::PLAY_LAST, current.play_last_at_startup);
	let startup_items: Vec<&str> = std::iter::once(s::STARTUP_DEFAULT)
		.chain(std::iter::once(s::STARTUP_ALL))
		.chain(collections.iter().map(|(_, name)| name.as_str()))
		.collect();
	let startup_at = match current.main_list.as_deref() {
		Some(crate::app::ALL_LISTS) => 1,
		_ => current
			.main_list
			.as_ref()
			.and_then(|id| collections.iter().position(|(cid, _)| cid == id))
			.map(|i| i + 2)
			.unwrap_or(0),
	};
	let startup_list = choice(&panel, &sizer, s::STARTUP_LIST, &startup_items, startup_at);
	let expand_dial = check(
		&panel,
		&sizer,
		s::EXPAND_DIAL_FOLDERS,
		current.expand_dial_folders,
	);
	let updates_auto = check(&panel, &sizer, s::CHECK_UPDATES_AUTO, current.check_updates);
	let play_move = check(&panel, &sizer, s::PLAY_ON_MOVE, current.play_on_move);
	let tray = check(&panel, &sizer, s::CLOSE_TO_TRAY, current.close_to_tray);
	let max_start = check(&panel, &sizer, s::MAXIMIZE_START, current.maximize_on_start);
	panel.set_sizer(sizer, true);

	let (panel, sizer) = tab(&notebook, s::TAB_PLAYBACK);
	let fm_sound = check(&panel, &sizer, s::FM_SOUND, current.fm_sound);
	let buffer_start = slider_field(
		&panel,
		&sizer,
		s::BUFFER_START,
		BufferSettings::START_MS,
		100,
		current.buffers.start_ms,
	);
	let buffer_refill = slider_field(
		&panel,
		&sizer,
		s::BUFFER_REFILL,
		BufferSettings::REFILL_SECONDS,
		1,
		current.buffers.refill_limit_seconds,
	);
	let buffer_hls = slider_field(
		&panel,
		&sizer,
		s::BUFFER_HLS,
		BufferSettings::HLS_SEGMENTS,
		1,
		current.buffers.hls_segments_behind,
	);
	let ts_on = check(&panel, &sizer, s::TS_ENABLE, current.timeshift.enabled);
	let ts_minutes = field(
		&panel,
		&sizer,
		s::TS_MINUTES,
		&current.timeshift.minutes.to_string(),
	);
	let ts_ring = check(&panel, &sizer, s::TS_RING, current.timeshift.encoded_ring);
	let ts_max_mb = field(
		&panel,
		&sizer,
		s::TS_MAX_MB,
		&current.timeshift.max_mb.to_string(),
	);
	let seek_step = field(
		&panel,
		&sizer,
		s::SEEK_STEP,
		&current.timeshift.seek_step_seconds.to_string(),
	);
	panel.set_sizer(sizer, true);

	let (panel, sizer) = tab(&notebook, s::TAB_RECORDING);
	let format_at = FORMATS
		.iter()
		.position(|f| *f == current.recording.format)
		.unwrap_or(0);
	let format = choice(
		&panel,
		&sizer,
		s::REC_FORMAT,
		&[s::FMT_ORIGINAL, s::FMT_WAV, s::FMT_MP3, s::FMT_FLAC],
		format_at,
	);
	let bitrate = field(
		&panel,
		&sizer,
		s::REC_BITRATE,
		&current.recording.mp3_bitrate_kbps.to_string(),
	);
	let vbr = check(&panel, &sizer, s::REC_VBR, current.recording.mp3_vbr);
	let rec_dir = field(
		&panel,
		&sizer,
		s::REC_DIR,
		current.recording.dir.as_deref().unwrap_or(""),
	);
	panel.set_sizer(sizer, true);

	let (panel, sizer) = tab(&notebook, s::TAB_SONGS);
	let recog_on = check(&panel, &sizer, s::RECOG_ENABLE, current.recognition.enabled);
	let provider = choice(
		&panel,
		&sizer,
		s::RECOG_PROVIDER,
		&[s::PROV_METADATA, s::PROV_AUDD],
		match current.recognition.provider {
			RecognitionProvider::Metadata => 0,
			RecognitionProvider::Audd => 1,
		},
	);
	let audd_token = field(
		&panel,
		&sizer,
		s::RECOG_TOKEN,
		current.recognition.audd_token.as_deref().unwrap_or(""),
	);
	let only_meta = check(
		&panel,
		&sizer,
		s::RECOG_ONLY_META,
		current.recognition.only_when_no_metadata,
	);
	let recog_announce = check(
		&panel,
		&sizer,
		s::RECOG_ANNOUNCE,
		current.recognition.announce,
	);
	let scrob_on = check(&panel, &sizer, s::SCROB_ENABLE, current.scrobble.enabled);
	let lb_token = field(
		&panel,
		&sizer,
		s::SCROB_LB_TOKEN,
		current.scrobble.listenbrainz_token.as_deref().unwrap_or(""),
	);
	let fm_key = field(
		&panel,
		&sizer,
		s::SCROB_KEY,
		current.scrobble.lastfm_api_key.as_deref().unwrap_or(""),
	);
	let fm_secret = field(
		&panel,
		&sizer,
		s::SCROB_SECRET,
		current.scrobble.lastfm_secret.as_deref().unwrap_or(""),
	);
	let fm_session = field(
		&panel,
		&sizer,
		s::SCROB_SESSION,
		current.scrobble.lastfm_session_key.as_deref().unwrap_or(""),
	);
	let fm_base = field(
		&panel,
		&sizer,
		s::SCROB_BASE,
		current.scrobble.lastfm_base.as_deref().unwrap_or(""),
	);
	panel.set_sizer(sizer, true);

	let sizer = BoxSizer::builder(Orientation::Vertical).build();
	sizer.add(&notebook, 1, SizerFlag::Expand | SizerFlag::All, 5);
	let ok = footer(&dialog, sizer);
	dialog.set_sizer_and_fit(sizer, true);

	let token = WindowToken::new();
	{
		let (a, d) = (app.clone(), dialog);
		ok.on_click(move |_| {
			let mut app = a.borrow_mut();
			let st = &mut app.settings;
			st.announce_titles = ann_titles.get_value();
			st.announce_status = ann_status.get_value();
			st.announce_actions = ann_actions.get_value();
			st.close_lists_when_playing = close_lists.get_value();
			st.play_last_at_startup = play_last.get_value();
			st.main_list = match startup_list.get_selection() {
				Some(1) => Some(crate::app::ALL_LISTS.to_string()),
				Some(i) if i > 1 => collections
					.get(i as usize - 2)
					.map(|(id, _)| id.clone()),
				_ => None,
			};
			st.check_updates = updates_auto.get_value();
			st.play_on_move = play_move.get_value();
			st.expand_dial_folders = expand_dial.get_value();
			st.fm_sound = fm_sound.get_value();
			st.close_to_tray = tray.get_value();
			st.maximize_on_start = max_start.get_value();
			st.buffers = BufferSettings {
				start_ms: parse_or(&buffer_start.get_value(), st.buffers.start_ms),
				refill_limit_seconds: parse_or(
					&buffer_refill.get_value(),
					st.buffers.refill_limit_seconds,
				),
				hls_segments_behind: parse_or(
					&buffer_hls.get_value(),
					st.buffers.hls_segments_behind,
				),
			}
			.clamped();
			st.timeshift.enabled = ts_on.get_value();
			st.timeshift.minutes = parse_or(&ts_minutes.get_value(), st.timeshift.minutes);
			st.timeshift.encoded_ring = ts_ring.get_value();
			st.timeshift.max_mb = parse_or(&ts_max_mb.get_value(), st.timeshift.max_mb);
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
			let recog_no_token = st.recognition.enabled
				&& st.recognition.provider == RecognitionProvider::Audd
				&& st.recognition.audd_token.is_none();
			app.save_settings();
			app.apply_fm_sound();
			app.apply_buffers();
			app.refresh_dial();
			if recog_no_token {
				crate::speech::focus_returning();
				app.say_announced(s::RECOG_NO_TOKEN);
			}
			d.end_modal(ID_OK);
		});
	}

	// The tab strip first, so the tabs are heard. A hidden page's control cannot take focus,
	// which is also why Ctrl+T comes back here and not to a check box.
	notebook.set_focus();
	crate::dialogs::run(app, dialog, Action::FeatureSettings, notebook, &token)
}

/// A tri-state override as a choice index: 0 global, 1 on, 2 off.
pub(crate) fn tri(value: Option<bool>) -> usize {
	match value {
		None => 0,
		Some(true) => 1,
		Some(false) => 2,
	}
}

pub(crate) fn untri(at: Option<u32>) -> Option<bool> {
	match at {
		Some(1) => Some(true),
		Some(2) => Some(false),
		_ => None,
	}
}
