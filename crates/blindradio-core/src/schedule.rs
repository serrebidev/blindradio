//! Scheduled recordings: a station, a start time, a duration and a repeat
//! rule. Pure arithmetic on Unix seconds; the app owns the clock, the
//! dialog and the tuner.

use serde::{Deserialize, Serialize};

use crate::station::Station;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Repeat {
	#[default]
	Once,
	Daily,
	Weekly,
}

impl Repeat {
	pub fn label(self) -> &'static str {
		match self {
			Repeat::Once => "Once",
			Repeat::Daily => "Daily",
			Repeat::Weekly => "Weekly",
		}
	}
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ScheduledRecording {
	/// Unique per entry; assigned by the app when the entry is created.
	pub id: String,
	/// A snapshot, so the entry survives the station being edited or the
	/// collection it came from being deleted.
	pub station: Station,
	/// The first occurrence, seconds since the Unix epoch.
	pub start_unix: u64,
	pub duration_minutes: u32,
	pub repeat: Repeat,
	pub enabled: bool,
}

impl Default for ScheduledRecording {
	fn default() -> Self {
		ScheduledRecording {
			id: String::new(),
			station: Station {
				id: String::new(),
				name: String::new(),
				url: String::new(),
				source: crate::station::Source::Custom,
				info: None,
			},
			start_unix: 0,
			duration_minutes: 60,
			repeat: Repeat::Once,
			enabled: true,
		}
	}
}

const DAY: u64 = 86_400;
const WEEK: u64 = 7 * DAY;

impl ScheduledRecording {
	fn period(&self) -> Option<u64> {
		match self.repeat {
			Repeat::Once => None,
			Repeat::Daily => Some(DAY),
			Repeat::Weekly => Some(WEEK),
		}
	}

	/// The occurrence that starts at or before `now`: the one that may be
	/// recording right now. None when the first start is still ahead.
	pub fn occurrence_at(&self, now: u64) -> Option<u64> {
		if now < self.start_unix {
			return None;
		}
		match self.period() {
			None => Some(self.start_unix),
			Some(p) => Some(self.start_unix + (now - self.start_unix) / p * p),
		}
	}

	/// The first occurrence strictly after `now`, if any.
	pub fn next_after(&self, now: u64) -> Option<u64> {
		if now < self.start_unix {
			return Some(self.start_unix);
		}
		match self.period() {
			None => None,
			Some(p) => Some(self.start_unix + (now - self.start_unix) / p * p + p),
		}
	}

	/// When the recording of `occurrence` stops.
	pub fn end_of(&self, occurrence: u64) -> u64 {
		occurrence + u64::from(self.duration_minutes) * 60
	}

	/// True while `now` falls inside an occurrence's window.
	pub fn is_due(&self, now: u64) -> bool {
		match self.occurrence_at(now) {
			Some(o) => now < self.end_of(o),
			None => false,
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::station::Source;

	fn entry(start: u64, repeat: Repeat) -> ScheduledRecording {
		ScheduledRecording {
			id: "s1".into(),
			station: Station {
				id: "st".into(),
				name: "St".into(),
				url: "http://x".into(),
				source: Source::Custom,
				info: None,
			},
			start_unix: start,
			duration_minutes: 30,
			repeat,
			enabled: true,
		}
	}

	#[test]
	fn once_fires_in_its_window_only() {
		let e = entry(1_000, Repeat::Once);
		assert_eq!(e.occurrence_at(999), None);
		assert_eq!(e.occurrence_at(1_000), Some(1_000));
		assert!(e.is_due(1_000 + 29 * 60));
		assert!(!e.is_due(1_000 + 30 * 60));
		assert_eq!(e.next_after(999), Some(1_000));
		assert_eq!(e.next_after(1_000), None);
	}

	#[test]
	fn daily_and_weekly_step_by_their_period() {
		let d = entry(1_000, Repeat::Daily);
		assert_eq!(d.occurrence_at(1_000 + 2 * DAY + 5), Some(1_000 + 2 * DAY));
		assert_eq!(d.next_after(1_000 + 2 * DAY + 5), Some(1_000 + 3 * DAY));
		assert!(d.is_due(1_000 + 5 * DAY + 60));
		let w = entry(1_000, Repeat::Weekly);
		assert_eq!(w.occurrence_at(1_000 + 3 * WEEK), Some(1_000 + 3 * WEEK));
		assert_eq!(w.next_after(1_000), Some(1_000 + WEEK));
	}
}
