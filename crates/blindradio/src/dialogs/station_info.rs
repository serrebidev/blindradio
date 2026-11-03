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
	let waiting = || live.is_none().then(|| s::INFO_WHILE_PLAYING.to_string());
	[
		(s::INFO_NAME, Some(station.name.clone())),
		(s::COLUMN_INFO, station.info.clone()),
		(s::INFO_CALL_SIGN, details.call_sign.clone()),
		(s::INFO_GENRE, details.genre.clone()),
		(s::INFO_COUNTRY, details.country.clone()),
		(s::INFO_LANGUAGE, details.language.clone()),
		(s::INFO_CODEC, codec),
		(s::INFO_BITRATE, bitrate),
		(s::INFO_SAMPLE_RATE, rate.or_else(waiting)),
		(s::INFO_CHANNELS, channels.or_else(waiting)),
		(s::COLUMN_DIRECTORY, Some(station.source.label().to_string())),
		(s::INFO_HOMEPAGE, details.homepage.clone()),
		(s::INFO_STREAM_URL, Some(station.url.clone())),
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
	let dialog = Dialog::builder(&frame, s::STATION_INFO_TITLE).build();
	let label = StaticText::builder(&dialog)
		.with_label(s::STATION_INFO_LABEL)
		.build();
	let text = TextCtrl::builder(&dialog)
		.with_style(TextCtrlStyle::MultiLine | TextCtrlStyle::ReadOnly)
		.build();
	text.set_value(&lines(station, live.as_ref()));
	let close = Button::builder(&dialog)
		.with_id(ID_CANCEL)
		.with_label(s::CLOSE)
		.build();
	let sizer = BoxSizer::builder(Orientation::Vertical).build();
	sizer.add(&label, 0, SizerFlag::Left | SizerFlag::Right | SizerFlag::Top, 5);
	sizer.add(&text, 1, SizerFlag::Expand | SizerFlag::All, 5);
	sizer.add(&close, 0, SizerFlag::All, 5);
	dialog.set_sizer(sizer, true);
	dialog.set_size(Size::new(560, 400));
	text.set_focus();
	crate::dialogs::run(app, dialog, Action::StationInfo, text, &WindowToken::new())
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
			},
		}
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
