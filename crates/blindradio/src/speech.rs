//! Spoken messages: a live region the screen reader watches, plus what may be said at all.

use blindradio_core::settings::Settings;
use live_region::Priority;
use wxdragon::prelude::*;

/// What kind of message it is; decides whether it is spoken at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
	/// Connection status: only with Announce Connection Status on, never while dial tuning.
	Status,
	/// Song titles: only with Announce Song Titles on.
	Title,
	/// Failures and reconnections: always.
	Failure,
	/// Answers and confirmations: only with Announce Actions on.
	Info,
}

/// `tuning` is a dial focus move in progress: the audio confirms it.
pub fn should_speak(kind: Kind, settings: &Settings, tuning: bool) -> bool {
	match kind {
		Kind::Status => settings.announce_status && !tuning,
		Kind::Title => settings.announce_titles,
		Kind::Info => settings.announce_actions,
		Kind::Failure => true,
	}
}

/// Failures interrupt, titles let the current utterance finish, the rest queue quietly.
pub fn priority(kind: Kind) -> Priority {
	match kind {
		Kind::Failure => Priority::High,
		Kind::Title => Priority::Medium,
		Kind::Status | Kind::Info => Priority::Low,
	}
}

pub struct Speech {
	/// The live region the screen reader watches.
	label: StaticText,
}

impl Speech {
	pub fn new(frame: &Frame) -> Speech {
		let label = StaticText::builder(frame).with_label("").build();
		live_region::set_live_region(frame);
		Speech { label }
	}

	pub fn say(&self, text: &str, priority: Priority) {
		live_region::announce_with_priority(self.label, text, priority);
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn settings(status: bool, titles: bool) -> Settings {
		Settings {
			announce_status: status,
			announce_titles: titles,
			..Settings::default()
		}
	}

	#[test]
	fn should_speak_policy() {
		for (status, titles) in [(true, true), (true, false), (false, true), (false, false)] {
			let s = settings(status, titles);
			assert_eq!(should_speak(Kind::Status, &s, false), status, "{status}");
			assert!(
				!should_speak(Kind::Status, &s, true),
				"dial tuning is never spoken: {status} {titles}"
			);
			assert_eq!(should_speak(Kind::Title, &s, true), titles, "{titles}");
			assert!(should_speak(Kind::Failure, &s, true));
			// Announce Actions defaults on.
			assert!(should_speak(Kind::Info, &s, true));
		}
		let quiet = Settings {
			announce_actions: false,
			..Settings::default()
		};
		assert!(!should_speak(Kind::Info, &quiet, false));
		// Failures speak even with every announcement off.
		assert!(should_speak(Kind::Failure, &quiet, false));
		assert_eq!(priority(Kind::Failure), Priority::High);
		assert_eq!(priority(Kind::Title), Priority::Medium);
		assert_eq!(priority(Kind::Info), Priority::Low);
		assert_eq!(priority(Kind::Status), Priority::Low);
	}
}
