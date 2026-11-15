//! The clipboard, read back: a copy that did not land must not claim it did.

use wxdragon::prelude::*;

use crate::strings as s;

/// A whole web address only: prose and multi-line copies leave URL fields empty.
fn url_text(text: &str) -> Option<String> {
	let text = crate::dialogs::custom_station::clean_url(text);
	if !crate::dialogs::custom_station::is_stream_url(&text)
		|| text.chars().any(|c| c.is_whitespace() || c.is_control())
	{
		return None;
	}
	let (_, address) = text.split_once("://")?;
	let host = address.split(['/', '?', '#']).next()?;
	(!host.is_empty()).then_some(text)
}

/// Read once when a new URL field opens. Never change the clipboard itself.
pub fn url() -> String {
	let clipboard = Clipboard::get();
	if !clipboard.open() {
		return String::new();
	}
	let text = clipboard.get_text();
	clipboard.close();
	text.as_deref().and_then(url_text).unwrap_or_default()
}

/// Puts `text` on the clipboard and reads it back.
pub fn copy(text: &str) -> bool {
	let clipboard = Clipboard::get();
	if !clipboard.open() {
		return false;
	}
	let set = clipboard.set_text(text);
	let back = clipboard.get_text();
	// Flush hands the text to the system for good. Unflushed clipboard
	// data is owned by this process and dies with it, so a copied URL
	// would vanish the moment BlindRadio closes.
	clipboard.flush();
	clipboard.close();
	set && back.as_deref() == Some(text)
}

/// What Copy Stream URL says for the clipboard's answer.
pub fn message(ok: bool) -> String {
	if ok { s::copied() } else { s::copy_failed() }
}

#[cfg(test)]
mod tests {
	use super::{message, url_text};

	#[test]
	fn only_whole_web_urls_prefill() {
		for text in [
			"",
			"Some notes",
			"ftp://example.com/a",
			"https://",
			"https:///path",
			"https://example.com/\nMore text",
			"https://example.com/a b",
			"https://example.com/\0",
		] {
			assert_eq!(url_text(text), None, "{text:?}");
		}
		assert_eq!(
			url_text(" \u{feff}HTTPS://example.com/a?b=c#d\r\n"),
			Some("HTTPS://example.com/a?b=c#d".into())
		);
		assert_eq!(
			url_text("http://127.0.0.1:8000/live"),
			Some("http://127.0.0.1:8000/live".into())
		);
	}

	#[test]
	fn copy_verify_messages() {
		assert_eq!(message(true), "Copied");
		assert_eq!(message(false), "Copy failed");
	}
}
