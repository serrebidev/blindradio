//! Station Info: what is known about a station, read-only. One text box of `Label: value` lines,
//! so a screen reader goes line by line and the text can be copied.

use std::cell::RefCell;
use std::rc::Rc;

use blindradio_core::decode::StreamFormat;
use blindradio_core::station::Station;
use wxdragon::prelude::*;

use crate::action::Action;
use crate::app::App;
use crate::bridge::WindowToken;
use crate::strings as s;

/// `live` is what the stream itself reports while it plays: it beats what a directory says.
pub fn lines(station: &Station, live: Option<&StreamFormat>) -> String {
	let details = &station.details;
	let codec = live
		.map(|f| f.codec.clone())
		.filter(|c| !c.is_empty())
		.or_else(|| details.codec.clone());
	let bitrate = live
		.and_then(|f| f.bitrate_kbps)
		.or(details.bitrate_kbps)
		.map(|k| format!("{k} kbps"));
	let rate = live.filter(|f| f.sample_rate > 0).map(StreamFormat::rate_label);
	let channels = live.and_then(StreamFormat::channels_label);
	let waiting = || live.is_none().then(|| s::info_while_playing().to_string());
	[
		(s::info_name(), Some(station.name.clone())),
		(s::column_info(), station.info.clone()),
		(s::info_slogan(), details.slogan.clone()),
		(s::info_call_sign(), details.call_sign.clone()),
		(s::info_genre(), details.genre.clone()),
		(s::info_country(), details.country.clone()),
		(s::info_location(), details.location.clone()),
		(s::info_language(), details.language.clone()),
		(s::info_codec(), codec),
		(s::info_bitrate(), bitrate),
		(s::info_sample_rate(), rate.or_else(waiting)),
		(s::info_channels(), channels.or_else(waiting)),
		(s::column_directory(), Some(station.source.label().to_string())),
		(s::info_homepage(), details.homepage.clone()),
		(s::info_stream_url(), Some(station.url.clone())),
	]
	.into_iter()
	.filter_map(|(label, value)| value.map(|v| format!("{label}: {v}")))
	.collect::<Vec<_>>()
	.join("\n")
}

pub fn show(app: &Rc<RefCell<App>>, station: &Station) -> Option<Action> {
	let (frame, live) = {
		let a = app.borrow();
		let live = a
			.tuner
			.playing()
			.filter(|p| p.id == station.id)
			.and_then(|_| a.tuner.format().cloned());
		(a.win.frame, live)
	};
	let dialog = Dialog::builder(&frame, &s::station_info_title()).build();
	let label = StaticText::builder(&dialog)
		.with_label(&s::station_info_label())
		.build();
	let text = TextCtrl::builder(&dialog)
		.with_style(TextCtrlStyle::MultiLine | TextCtrlStyle::ReadOnly)
		.build();
	text.set_value(&lines(station, live.as_ref()));
	let close = Button::builder(&dialog)
		.with_id(ID_CANCEL)
		.with_label(&s::close())
		.build();
	let sizer = BoxSizer::builder(Orientation::Vertical).build();
	sizer.add(&label, 0, SizerFlag::Left | SizerFlag::Right | SizerFlag::Top, 5);
	sizer.add(&text, 1, SizerFlag::Expand | SizerFlag::All, 5);
	sizer.add(&close, 0, SizerFlag::All, 5);
	dialog.set_sizer(sizer, true);
	dialog.set_size(Size::new(560, 400));
	let token = WindowToken::new();
	// TuneIn's lists hold almost nothing, so it is asked here. The window does not wait: the lines
	// arrive while the reader is still on the first. A failure leaves what is already shown.
	if let Some(guide) = station.id.strip_prefix("tunein:").map(str::to_owned) {
		let mut shown = station.clone();
		let a = app.borrow();
		let sources = a.sources.clone();
		a.bridge.spawn(
			token.clone(),
			async move { sources.tunein_details(&guide).await },
			move |_: &mut App, found| {
				let Some(found) = found.ok().filter(|d| !d.is_empty()) else {
					return;
				};
				// The copy is dropped with this window: TuneIn's row is what stays saved.
				shown.details = found;
				let at = text.get_insertion_point();
				text.change_value(&lines(&shown, live.as_ref()));
				text.set_insertion_point(at);
			},
		);
	}
	text.set_focus();
	crate::dialogs::run(app, dialog, Action::StationInfo, text, &token)
}

#[cfg(test)]
mod tests {
	use super::*;
	use blindradio_core::station::{Details, Source};

	fn station() -> Station {
		Station {
			id: "rb:1".into(),
			name: "Jazz FM".into(),
			url: "http://jazz".into(),
			source: Source::RadioBrowser,
			info: Some("Jazz, France, MP3 128k".into()),
			details: Details {
				genre: Some("jazz, blues".into()),
				country: Some("France".into()),
				language: Some("french".into()),
				codec: Some("MP3".into()),
				bitrate_kbps: Some(128),
				homepage: Some("http://jazz.example".into()),
				call_sign: None,
				unavailable: None,
				..Details::default()
			},
		}
	}

	#[test]
	fn location_and_slogan_show_where_a_directory_gives_them() {
		let tunein = Station {
			details: Details {
				slogan: Some("Breaking News".into()),
				location: Some("London, UK".into()),
				..Details::default()
			},
			..station()
		};
		let got = lines(&tunein, None);
		assert!(got.contains("Slogan: Breaking News\n"), "{got}");
		assert!(got.contains("Location: London, UK\n"), "{got}");
		assert!(!lines(&station(), None).contains("Slogan"));
	}

	#[test]
	fn directory_facts_show_and_the_stream_waits_for_playback() {
		assert_eq!(
			lines(&station(), None),
			"Name: Jazz FM\n\
			Info: Jazz, France, MP3 128k\n\
			Genre: jazz, blues\n\
			Country: France\n\
			Language: french\n\
			Codec: MP3\n\
			Bitrate: 128 kbps\n\
			Sample rate: Known while the station plays\n\
			Channels: Known while the station plays\n\
			Directory: Radio Browser\n\
			Homepage: http://jazz.example\n\
			Stream URL: http://jazz"
		);
	}

	#[test]
	fn the_playing_stream_beats_the_directory_and_blanks_drop_out() {
		let live = StreamFormat {
			codec: "AAC".into(),
			bitrate_kbps: Some(96),
			sample_rate: 44_100,
			channels: 2,
			replayable: true,
		};
		let got = lines(&station(), Some(&live));
		for line in ["Codec: AAC", "Bitrate: 96 kbps", "Sample rate: 44.1 kHz", "Channels: stereo"] {
			assert!(got.contains(line), "{line} in {got}");
		}
		let bare = Station {
			info: None,
			details: Details::default(),
			..station()
		};
		let got = lines(&bare, Some(&live));
		assert!(!got.contains("Genre") && !got.contains("Info:") && !got.contains("Homepage"), "{got}");
	}
}
