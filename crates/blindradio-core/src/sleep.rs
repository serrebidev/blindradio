use std::time::{Duration, Instant};

/// Ctrl+1 to Ctrl+6, in order.
pub const SLEEP_MINUTES: [u32; 6] = [15, 30, 45, 60, 90, 120];

#[derive(Default)]
pub struct SleepTimer {
	deadline: Option<Instant>,
}

impl SleepTimer {
	pub fn set(&mut self, minutes: u32, now: Instant) -> String {
		self.deadline = Some(now + Duration::from_secs(u64::from(minutes) * 60));
		if minutes.is_multiple_of(60) {
			format!("Sleep {} h", minutes / 60)
		} else {
			format!("Sleep {minutes} min")
		}
	}

	pub fn cancel(&mut self) -> String {
		self.deadline = None;
		"Sleep off".into()
	}

	pub fn remaining(&self, now: Instant) -> Option<Duration> {
		self.deadline.map(|d| d.saturating_duration_since(now))
	}

	pub fn due(&mut self, now: Instant) -> bool {
		let due = self.deadline.is_some_and(|d| now >= d);
		if due {
			self.deadline = None;
		}
		due
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn labels_exact() {
		let mut s = SleepTimer::default();
		let now = Instant::now();
		let l: Vec<_> = SLEEP_MINUTES.iter().map(|&m| s.set(m, now)).collect();
		assert_eq!(
			l,
			[
				"Sleep 15 min",
				"Sleep 30 min",
				"Sleep 45 min",
				"Sleep 1 h",
				"Sleep 90 min",
				"Sleep 2 h"
			]
		);
	}

	#[test]
	fn set_replaces_and_cancel() {
		let mut s = SleepTimer::default();
		let now = Instant::now();
		assert_eq!(s.remaining(now), None);
		s.set(15, now);
		s.set(30, now);
		assert_eq!(s.remaining(now), Some(Duration::from_secs(1800)));
		assert_eq!(s.cancel(), "Sleep off");
		assert_eq!(s.remaining(now), None);
		assert!(!s.due(now + Duration::from_secs(9999)));
	}

	#[test]
	fn due_fires_once() {
		let mut s = SleepTimer::default();
		let now = Instant::now();
		s.set(15, now);
		assert!(!s.due(now + Duration::from_secs(899)));
		assert!(s.due(now + Duration::from_secs(900)));
		assert!(!s.due(now + Duration::from_secs(901)));
		assert_eq!(s.remaining(now), None);
	}
}
