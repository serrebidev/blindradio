//! The clipboard, read back: a copy that did not land must not claim it did.

use wxdragon::prelude::*;

use crate::strings as s;

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
pub fn message(ok: bool) -> &'static str {
	if ok { s::COPIED } else { s::COPY_FAILED }
}

#[cfg(test)]
mod tests {
	use super::message;

	#[test]
	fn copy_verify_messages() {
		assert_eq!(message(true), "Copied");
		assert_eq!(message(false), "Copy failed");
	}
}
