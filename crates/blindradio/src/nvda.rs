//! Speaking through NVDA itself, with its controller client.
//!
//! Live-region announcements ride on the foreground app's UI tree, so
//! the global hot key's announcement was only ever heard while
//! BlindRadio had focus — Brandon: "It's a global hot key and should
//! work everywhere." The controller client (LGPL 2.1, shipped beside
//! the exe as nvdaControllerClient.dll, license alongside) talks to
//! the running NVDA directly, whichever app is in front.
//!
//! Windows-only and fully optional: no DLL, no NVDA, or a failed call
//! all return false and the caller falls back to the live region.

/// Speak `text` through NVDA; `interrupt` cancels what NVDA is saying
/// first. True when NVDA took the text.
#[cfg(windows)]
pub fn speak(text: &str, interrupt: bool) -> bool {
	imp::speak(text, interrupt)
}

#[cfg(not(windows))]
pub fn speak(_text: &str, _interrupt: bool) -> bool {
	false
}

#[cfg(windows)]
mod imp {
	use std::sync::OnceLock;

	use libloading::{Library, Symbol};

	type SpeakText = unsafe extern "system" fn(*const u16) -> u32;
	type TestIfRunning = unsafe extern "system" fn() -> u32;
	type CancelSpeech = unsafe extern "system" fn() -> u32;

	struct Client {
		// Kept alive for the process: the function pointers live in it.
		_lib: Library,
		speak_text: SpeakText,
		test_if_running: TestIfRunning,
		cancel_speech: CancelSpeech,
	}

	// The function pointers are plain code addresses; the library
	// handle never leaves this cell.
	unsafe impl Send for Client {}
	unsafe impl Sync for Client {}

	static CLIENT: OnceLock<Option<Client>> = OnceLock::new();

	fn client() -> Option<&'static Client> {
		CLIENT
			.get_or_init(|| {
				let exe = std::env::current_exe().ok()?;
				let path = exe.parent()?.join("nvdaControllerClient.dll");
				let lib = unsafe { Library::new(path) }.ok()?;
				unsafe {
					let speak_text: Symbol<SpeakText> =
						lib.get(b"nvdaController_speakText").ok()?;
					let test_if_running: Symbol<TestIfRunning> =
						lib.get(b"nvdaController_testIfRunning").ok()?;
					let cancel_speech: Symbol<CancelSpeech> =
						lib.get(b"nvdaController_cancelSpeech").ok()?;
					Some(Client {
						speak_text: *speak_text,
						test_if_running: *test_if_running,
						cancel_speech: *cancel_speech,
						_lib: lib,
					})
				}
			})
			.as_ref()
	}

	pub fn speak(text: &str, interrupt: bool) -> bool {
		let Some(c) = client() else { return false };
		unsafe {
			if (c.test_if_running)() != 0 {
				return false;
			}
			if interrupt {
				let _ = (c.cancel_speech)();
			}
			let wide: Vec<u16> = text.encode_utf16().chain([0]).collect();
			(c.speak_text)(wide.as_ptr()) == 0
		}
	}
}
