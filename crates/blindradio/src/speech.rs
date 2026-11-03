//! Spoken messages: a live region the screen reader watches, plus what may be said at all.

use std::cell::RefCell;
use std::time::{Duration, Instant};

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

/// How long a message waits after focus starts coming back. NVDA starts reading the
/// focused control about 125 ms after a menu closes and cancels speech as it does.
/// Later is safe: NVDA queues the message behind what it is reading.
const FOCUS_SETTLES: Duration = Duration::from_millis(500);

/// Messages waiting for the focus reading to start.
#[derive(Default)]
struct Hold {
	until: Option<Instant>,
	held: Vec<(String, Priority)>,
}

impl Hold {
	fn focus_returning(&mut self, now: Instant) {
		self.until = Some(now + FOCUS_SETTLES);
	}

	/// Keeps the message and answers true while it must wait. A later message waits
	/// behind an earlier one, so order holds.
	fn keep(&mut self, text: &str, priority: Priority, now: Instant) -> bool {
		let wait = self.until.is_some_and(|until| now < until) || !self.held.is_empty();
		if wait {
			self.held.push((text.to_owned(), priority));
		}
		wait
	}

	fn due(&mut self, now: Instant) -> Vec<(String, Priority)> {
		if self.until.is_some_and(|until| now < until) {
			return Vec::new();
		}
		self.until = None;
		std::mem::take(&mut self.held)
	}
}

thread_local! {
	static HOLD: RefCell<Hold> = RefCell::default();
}

/// Call when a menu closes or the window gets focus back. The screen reader is about to
/// read the focused control, and that reading cuts off anything said just before it.
pub fn focus_returning() {
	HOLD.with_borrow_mut(|hold| hold.focus_returning(Instant::now()));
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
		if !HOLD.with_borrow_mut(|hold| hold.keep(text, priority, Instant::now())) {
			self.speak(text, priority);
		}
	}

	/// From poll(): says what waited for the focus to settle.
	pub fn flush(&self) {
		for (text, priority) in HOLD.with_borrow_mut(|hold| hold.due(Instant::now())) {
			self.speak(&text, priority);
		}
	}

	fn speak(&self, text: &str, priority: Priority) {
		// Through NVDA itself when it is running: the live region is
		// only heard while BlindRadio is the foreground app, which made
		// the global hot key's announcement silent everywhere else.
		// NVDA not running (or another reader in charge) keeps the
		// live region.
		if !crate::nvda::speak(text, priority == Priority::High) {
			live_region::announce_with_priority(self.label, text, priority);
		}
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
	fn messages_wait_for_the_focus_to_settle_and_keep_their_order() {
		let start = Instant::now();
		let mut hold = Hold::default();
		assert!(!hold.keep("free", Priority::Low, start), "nothing to wait for");
		hold.focus_returning(start);
		assert!(hold.keep("one", Priority::Low, start));
		let settled = start + FOCUS_SETTLES;
		assert!(hold.due(settled - Duration::from_millis(1)).is_empty());
		assert!(hold.keep("two", Priority::High, settled), "stays behind one");
		let due: Vec<_> = hold.due(settled).into_iter().map(|(text, _)| text).collect();
		assert_eq!(due, ["one", "two"]);
		assert!(!hold.keep("three", Priority::Low, settled));
		assert!(hold.due(settled).is_empty());
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
