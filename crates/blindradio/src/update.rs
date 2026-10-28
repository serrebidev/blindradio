//! Updates: ship-shape against this repository's GitHub Releases, verified with minisign.
//!
//! The public key below is the one `cargo xtask minisign-key` made; its secret half signs every
//! release asset and lives in the `MINISIGN_KEY` secret, never in git.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use blindradio_core::store;
use ship_shape::ui::{self, CheckTrigger};
use ship_shape::{InstallKind, UpdateChannel, UpdaterConfig};
use wxdragon::prelude::WxWidget;

/// The second line of `minisign.pub`.
const PUBLIC_KEY: &str = "RWQ/pjmJi4QeFQf1jrQjt0KxMJgPAQo7rTMwLgyhNqdRjZtQnN9PLx3O";
const DAY: u64 = 24 * 60 * 60;

/// Before the first show, so the relaunch after an update comes to the front.
pub fn config() -> Arc<UpdaterConfig> {
	Arc::new(
		UpdaterConfig::new(
			"serrebidev/blindradio",
			"BlindRadio",
			"BlindRadio",
			PUBLIC_KEY,
			env!("CARGO_PKG_VERSION"),
		)
		.with_commit(env!("BLINDRADIO_COMMIT_HASH"))
		.with_install_kind(install_kind(store::is_portable())),
	)
}

/// The portable zip updates from the archive; an install updates from the installer.
pub fn install_kind(portable: bool) -> InstallKind {
	if portable {
		InstallKind::Portable
	} else {
		InstallKind::Installer
	}
}

/// Once a day: never checked, a day has passed, or the clock went backwards.
pub fn daily_check_due(last: Option<u64>, now: u64) -> bool {
	last.is_none_or(|checked| checked > now || now - checked >= DAY)
}

pub fn now() -> u64 {
	SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.map(|d| d.as_secs())
		.unwrap_or_default()
}

/// Runs off the UI thread; only the result comes back to it.
pub fn check(parent: &dyn WxWidget, trigger: CheckTrigger) {
	ui::run_update_check(config(), parent, UpdateChannel::Stable, trigger);
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn daily_check_due_policy() {
		assert!(daily_check_due(None, 1_000));
		assert!(daily_check_due(Some(0), 2 * DAY));
		assert!(!daily_check_due(Some(1_000), 1_000));
		assert!(!daily_check_due(Some(1_000), 1_000 + DAY - 1));
		assert!(daily_check_due(Some(1_000), 1_000 + DAY));
		// A clock that went backwards does not wedge the check forever.
		assert!(daily_check_due(Some(DAY * 10), 0));
	}

	#[test]
	fn install_kind_from_portable() {
		assert_eq!(install_kind(true), InstallKind::Portable);
		assert_eq!(install_kind(false), InstallKind::Installer);
	}

	#[test]
	fn config_names_the_release_assets() {
		let config = config();
		assert_eq!(config.github_repo, "serrebidev/blindradio");
		assert_eq!(config.app_name, "BlindRadio");
		assert_eq!(config.current_version, env!("CARGO_PKG_VERSION"));
		assert!(!config.minisign_public_key.is_empty());
		// Every asset is published under this name, so the updater must look for it.
		assert!(config.app_name == "BlindRadio");
	}
}
