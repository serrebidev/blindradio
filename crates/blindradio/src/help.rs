//! F1 user guide, the changelog, the About box and the licences page.

use std::io::Read;
use std::path::PathBuf;

use flate2::read::DeflateDecoder;
use wxdragon::prelude::*;

const GUIDE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/readme.html"));
const CHANGELOG: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/changelog.html"));
const LICENSES: &str = include_str!("../../../doc/licenses.html");

fn inflate(page: &[u8]) -> String {
	let mut html = String::new();
	DeflateDecoder::new(page)
		.read_to_string(&mut html)
		.expect("an embedded page inflates");
	html
}

/// The user guide, inflated from what the build embedded.
pub fn guide() -> String {
	inflate(GUIDE)
}

/// CHANGELOG.md as a page, so it reads offline.
pub fn changelog() -> String {
	inflate(CHANGELOG)
}

/// The project's GitHub page in the browser.
pub fn open_website() -> bool {
	launch_default_browser(s::WEBSITE, BrowserLaunchFlags::Default)
}

/// Help > Report an issue: title and description asked here, where the screen
/// reader knows the boxes, then GitHub's new-issue page opens with both filled
/// in and the version line added. GitHub needs the user's own account, so the
/// last step, Create, is theirs in the browser. False if no browser opened.
pub fn report_issue(parent: &dyn WxWidget) -> bool {
	let title = s::report_issue_title();
	let Some(summary) = ask(parent, &s::report_issue_summary(), &title, TextEntryDialogStyle::Default)
		.filter(|text| !text.is_empty())
	else {
		return true;
	};
	let multiline = TextEntryDialogStyle::Default | TextEntryDialogStyle::MultiLine;
	let Some(details) = ask(parent, &s::report_issue_details(), &title, multiline) else {
		return true;
	};
	launch_default_browser(&issue_url(&summary, &details), BrowserLaunchFlags::Default)
}

/// Trimmed text, or None on Cancel. Multiline: Enter is a new line, Tab reaches OK.
fn ask(parent: &dyn WxWidget, message: &str, title: &str, style: TextEntryDialogStyle) -> Option<String> {
	let dialog = TextEntryDialog::builder(parent, message, title).with_style(style).build();
	let value = (dialog.show_modal() == ID_OK).then(|| dialog.get_value().unwrap_or_default());
	dialog.destroy();
	value.map(|text| text.trim().to_owned())
}

fn issue_url(summary: &str, details: &str) -> String {
	let body = format!(
		"{details}\n\n---\nBlindRadio {} on {} {}",
		env!("CARGO_PKG_VERSION"),
		std::env::consts::OS,
		std::env::consts::ARCH,
	);
	format!(
		"{}/issues/new?title={}&body={}",
		s::WEBSITE,
		query_encode(summary),
		query_encode(&body)
	)
}

/// Percent-encode a query value: everything but unreserved ASCII, as UTF-8 bytes.
fn query_encode(text: &str) -> String {
	text.bytes()
		.map(|b| match b {
			b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
			_ => format!("%{b:02X}"),
		})
		.collect()
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
	let dialog = Dialog::builder(frame, &s::about_title()).build();
	let body = format!(
		"{} {}\n{}\n\n{}",
		s::APP_NAME,
		env!("CARGO_PKG_VERSION"),
		s::about_description(),
		s::about_author()
	);
	let text = StaticText::builder(&dialog).with_label(&body).build();
	// Each link is its own button: Tab reaches it, Enter or Space opens it.
	let links = BoxSizer::builder(Orientation::Vertical).build();
	for (label, url) in s::about_links() {
		let link = Button::builder(&dialog).with_label(&label).build();
		link.on_click(move |_| {
			launch_default_browser(url, BrowserLaunchFlags::Default);
		});
		links.add(&link, 0, SizerFlag::Expand | SizerFlag::All, 3);
	}
	let button = Button::builder(&dialog)
		.with_label(&s::licenses_button())
		.build();
	let close = Button::builder(&dialog)
		.with_id(ID_CANCEL)
		.with_label(&s::close())
		.build();
	dialog.set_escape_id(ID_CANCEL);
	let buttons = BoxSizer::builder(Orientation::Horizontal).build();
	buttons.add(&button, 0, SizerFlag::All, 5);
	buttons.add(&close, 0, SizerFlag::All, 5);
	let sizer = BoxSizer::builder(Orientation::Vertical).build();
	sizer.add(&text, 0, SizerFlag::Expand | SizerFlag::All, 10);
	sizer.add_sizer(&links, 0, SizerFlag::Expand | SizerFlag::All, 7);
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
	#[test]
	fn issue_link_carries_title_body_and_version() {
		let url = super::issue_url("Photo & video", "Line 1\nПривет");
		assert!(url.starts_with(
			"https://github.com/serrebidev/blindradio/issues/new?title=Photo%20%26%20video&body=Line%201%0A%D0%9F"
		));
		assert!(url.contains(&super::query_encode(env!("CARGO_PKG_VERSION"))));
		assert!(!url[url.find('?').unwrap()..].contains(['\n', ' ', '#']));
	}

	#[test]
	fn about_has_every_project_link() {
		let urls: Vec<&str> = super::s::about_links()
			.into_iter()
			.map(|(_, url)| url)
			.collect();
		assert_eq!(
			urls,
			[
				"https://github.com/serrebidev/blindradio",
				"https://git.serrebiradio.com/serrebi/blindradio",
				"https://projects.serrebiradio.com",
				"https://t.me/SerrebiProjects",
				"https://git.serrebiradio.com/serrebi/blindradio/src/branch/main/CREDITS.md",
			]
		);
	}
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
	fn changelog_embedded_with_this_version() {
		assert!(changelog().contains(env!("CARGO_PKG_VERSION")));
	}

	#[test]
	fn licenses_include_fdk() {
		assert!(
			licenses().contains("Fraunhofer"),
			"FDK AAC's notice is there"
		);
	}
}
