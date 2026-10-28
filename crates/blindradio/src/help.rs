//! F1 user guide, the About box and the licences page.

use std::io::Read;
use std::path::PathBuf;

use flate2::read::DeflateDecoder;
use wxdragon::prelude::*;

const GUIDE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/readme.html"));
const LICENSES: &str = include_str!("../../../doc/licenses.html");

/// The user guide, inflated from what the build embedded.
pub fn guide() -> String {
	let mut html = String::new();
	DeflateDecoder::new(GUIDE)
		.read_to_string(&mut html)
		.expect("the embedded guide inflates");
	html
}

pub fn licenses() -> &'static str {
	LICENSES
}

/// Writes the page into the temp folder and opens it in the browser.
pub fn open(html: &str, name: &str) -> bool {
	let dir: PathBuf = std::env::temp_dir().join("BlindRadio");
	if std::fs::create_dir_all(&dir).is_err() {
		return false;
	}
	let path = dir.join(name);
	if std::fs::write(&path, html).is_err() {
		return false;
	}
	launch_default_application(&path.to_string_lossy())
}

/// About BlindRadio, with the licences one button away.
pub fn about(frame: &Frame) {
	let dialog = Dialog::builder(frame, s::ABOUT_TITLE).build();
	let body = format!(
		"{} {}\n{}\n{}",
		s::APP_NAME,
		env!("CARGO_PKG_VERSION"),
		s::ABOUT_DESCRIPTION,
		s::WEBSITE
	);
	let text = StaticText::builder(&dialog).with_label(&body).build();
	let button = Button::builder(&dialog)
		.with_label(s::LICENSES_BUTTON)
		.build();
	let close = Button::builder(&dialog)
		.with_id(ID_CANCEL)
		.with_label(s::CLOSE)
		.build();
	dialog.set_escape_id(ID_CANCEL);
	let buttons = BoxSizer::builder(Orientation::Horizontal).build();
	buttons.add(&button, 0, SizerFlag::All, 5);
	buttons.add(&close, 0, SizerFlag::All, 5);
	let sizer = BoxSizer::builder(Orientation::Vertical).build();
	sizer.add(&text, 0, SizerFlag::Expand | SizerFlag::All, 10);
	sizer.add_sizer(&buttons, 0, SizerFlag::All, 0);
	dialog.set_sizer_and_fit(sizer, true);
	button.on_click(move |_| {
		open(licenses(), "licenses.html");
	});
	dialog.centre();
	dialog.show_modal();
	dialog.destroy();
}

use crate::strings as s;

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn guide_embedded_and_inflates() {
		let html = guide();
		assert!(html.contains("BlindRadio"));
		assert!(
			html.contains("Keyboard shortcuts"),
			"the guide has its sections"
		);
	}

	#[test]
	fn licenses_include_fdk() {
		assert!(
			licenses().contains("Fraunhofer"),
			"FDK AAC's notice is there"
		);
	}
}
