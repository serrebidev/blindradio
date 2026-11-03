//! Tokens in settings.json. On Windows they are sealed to this user with DPAPI, so the file can
//! be copied, synced or shared without them; a copy on another PC or account reads them as blank.
//! Elsewhere they stay plain, and `store::save` makes the file readable by its owner only.

use serde::{Deserialize, Deserializer, Serializer};

const PREFIX: &str = "dpapi:";

pub fn seal<S: Serializer>(value: &Option<String>, out: S) -> Result<S::Ok, S::Error> {
	match value {
		Some(text) => out.serialize_some(&sealed(text)),
		None => out.serialize_none(),
	}
}

/// A plain value, as older versions wrote it, reads as it is and is sealed on the next save.
pub fn open<'de, D: Deserializer<'de>>(input: D) -> Result<Option<String>, D::Error> {
	Ok(Option::<String>::deserialize(input)?.and_then(|text| opened(&text)))
}

#[cfg(windows)]
fn sealed(text: &str) -> String {
	// Kept plain if DPAPI refuses: a token in the clear beats a token lost.
	dpapi::protect(text.as_bytes()).map_or_else(
		|| text.to_string(),
		|bytes| {
			let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
			format!("{PREFIX}{hex}")
		},
	)
}

#[cfg(not(windows))]
fn sealed(text: &str) -> String {
	text.to_string()
}

fn opened(text: &str) -> Option<String> {
	match text.strip_prefix(PREFIX) {
		Some(hex) => unseal(hex),
		None => Some(text.to_string()),
	}
}

#[cfg(windows)]
fn unseal(hex: &str) -> Option<String> {
	if !hex.is_ascii() || !hex.len().is_multiple_of(2) {
		return None;
	}
	let bytes: Option<Vec<u8>> = (0..hex.len())
		.step_by(2)
		.map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
		.collect();
	String::from_utf8(dpapi::unprotect(&bytes?)?).ok()
}

#[cfg(not(windows))]
fn unseal(_hex: &str) -> Option<String> {
	None
}

#[cfg(windows)]
mod dpapi {
	use std::ffi::c_void;
	use std::ptr::{null, null_mut};

	#[repr(C)]
	struct Blob {
		len: u32,
		data: *mut u8,
	}

	const UI_FORBIDDEN: u32 = 1;

	#[link(name = "crypt32")]
	unsafe extern "system" {
		fn CryptProtectData(
			input: *const Blob,
			description: *const u16,
			entropy: *const Blob,
			reserved: *mut c_void,
			prompt: *mut c_void,
			flags: u32,
			output: *mut Blob,
		) -> i32;
		fn CryptUnprotectData(
			input: *const Blob,
			description: *mut *mut u16,
			entropy: *const Blob,
			reserved: *mut c_void,
			prompt: *mut c_void,
			flags: u32,
			output: *mut Blob,
		) -> i32;
	}

	#[link(name = "kernel32")]
	unsafe extern "system" {
		fn LocalFree(memory: *mut c_void) -> *mut c_void;
	}

	fn input(bytes: &[u8]) -> Blob {
		Blob {
			len: bytes.len() as u32,
			// DPAPI only reads its input.
			data: bytes.as_ptr().cast_mut(),
		}
	}

	/// Copies what DPAPI allocated, then frees it.
	fn take(output: &Blob) -> Vec<u8> {
		// SAFETY: on success DPAPI hands over `len` bytes at `data`, freed with LocalFree.
		unsafe {
			let bytes = std::slice::from_raw_parts(output.data, output.len as usize).to_vec();
			LocalFree(output.data.cast());
			bytes
		}
	}

	pub fn protect(bytes: &[u8]) -> Option<Vec<u8>> {
		let mut output = Blob { len: 0, data: null_mut() };
		// SAFETY: every pointer is valid for the call or null where the API allows it.
		let ok = unsafe {
			CryptProtectData(
				&input(bytes),
				null(),
				null(),
				null_mut(),
				null_mut(),
				UI_FORBIDDEN,
				&mut output,
			)
		};
		(ok != 0).then(|| take(&output))
	}

	pub fn unprotect(bytes: &[u8]) -> Option<Vec<u8>> {
		let mut output = Blob { len: 0, data: null_mut() };
		// SAFETY: every pointer is valid for the call or null where the API allows it.
		let ok = unsafe {
			CryptUnprotectData(
				&input(bytes),
				null_mut(),
				null(),
				null_mut(),
				null_mut(),
				UI_FORBIDDEN,
				&mut output,
			)
		};
		(ok != 0).then(|| take(&output))
	}
}

#[cfg(test)]
mod tests {
	use crate::settings::Settings;

	fn with_token(token: &str) -> Settings {
		let mut settings = Settings::default();
		settings.recognition.audd_token = Some(token.into());
		settings.scrobble.lastfm_secret = Some(format!("{token}-secret"));
		settings
	}

	#[test]
	fn tokens_come_back_from_the_file() {
		let dir = tempfile::tempdir().unwrap();
		with_token("hunter2").save(dir.path()).unwrap();
		let loaded = Settings::load(dir.path());
		assert_eq!(loaded.recognition.audd_token.as_deref(), Some("hunter2"));
		assert_eq!(loaded.scrobble.lastfm_secret.as_deref(), Some("hunter2-secret"));
		let file = std::fs::read_to_string(dir.path().join("settings.json")).unwrap();
		if cfg!(windows) {
			assert!(!file.contains("hunter2"), "sealed in the file");
		}
	}

	#[test]
	fn a_plain_token_from_an_older_version_still_reads() {
		let dir = tempfile::tempdir().unwrap();
		std::fs::write(
			dir.path().join("settings.json"),
			r#"{"recognition":{"audd_token":"old-plain"}}"#,
		)
		.unwrap();
		let loaded = Settings::load(dir.path());
		assert_eq!(loaded.recognition.audd_token.as_deref(), Some("old-plain"));
	}

	#[test]
	fn a_token_sealed_elsewhere_reads_blank_and_keeps_the_rest() {
		let dir = tempfile::tempdir().unwrap();
		std::fs::write(
			dir.path().join("settings.json"),
			r#"{"fm_sound":false,"recognition":{"audd_token":"dpapi:00ff"}}"#,
		)
		.unwrap();
		let loaded = Settings::load(dir.path());
		assert_eq!(loaded.recognition.audd_token, None);
		assert!(!loaded.fm_sound, "the file is not thrown away for it");
	}
}
