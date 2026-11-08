//! App translations via patois: one manager for the whole process, set
//! up before any window exists so every string resolves in the chosen
//! language from the first label on.

use std::sync::OnceLock;

use parking_lot::Mutex;
use patois::LanguageInfo;
use patois::wx::WxTranslationManager;

use crate::WxStdCatalogLoader;

static MANAGER: OnceLock<Mutex<WxTranslationManager>> = OnceLock::new();

/// Starts translations: the saved language when there is one, else the
/// best fit for the system language, else English. Also puts the
/// wxWidgets stock strings (OK, Cancel, …) in the same language.
pub fn init(saved: Option<&str>) {
	let mut manager = WxTranslationManager::new(env!("CARGO_PKG_NAME"));
	manager.initialize(WxStdCatalogLoader);
	if let Some(code) = saved {
		manager.set_language(code, WxStdCatalogLoader);
	}
	let _ = MANAGER.set(Mutex::new(manager));
}

/// The languages the Settings choice offers: English first, then the
/// translated catalogs by native name.
pub fn available() -> Vec<LanguageInfo> {
	match MANAGER.get() {
		Some(manager) => manager.lock().available_languages(),
		None => patois::LanguageManager::new(env!("CARGO_PKG_NAME")).available(),
	}
}

/// Switches the live language; strings already on screen keep the old
/// one until their window is rebuilt. False when unavailable.
pub fn set_language(code: &str) -> bool {
	match MANAGER.get() {
		Some(manager) => manager.lock().set_language(code, WxStdCatalogLoader),
		None => false,
	}
}
