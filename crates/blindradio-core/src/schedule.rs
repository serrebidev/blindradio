//! Scheduled recordings: a station, a start time, a duration and a repeat
//! rule. Times are Unix seconds; repeats follow the local clock. The app
//! owns the clock, the dialog and the tuner.

use jiff::tz::TimeZone;
use jiff::{Timestamp, ToSpan};
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
	/// An alarm plays its station instead of recording it; the
	/// duration is ignored.
	#[serde(default)]
	pub alarm: bool,
	/// Record silently: the tuner is muted for the recording instead
	/// of taking over playback. Older entries omit the field and keep
	/// the audible behaviour they had.
	#[serde(default = "default_silent")]
	pub silent: bool,
}

fn default_silent() -> bool {
	true
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
				details: Default::default(),
			},
			start_unix: 0,
			duration_minutes: 60,
			repeat: Repeat::Once,
			enabled: true,
			alarm: false,
			silent: true,
		}
	}
}

const DAY: u64 = 86_400;

impl ScheduledRecording {
	/// Days between occurrences; None for a one-off.
	fn period_days(&self) -> Option<u32> {
		match self.repeat {
			Repeat::Once => None,
			Repeat::Daily => Some(1),
			Repeat::Weekly => Some(7),
		}
	}

	/// Occurrence `n`: the first one's clock time, `n` periods on, so 8:00 stays
	/// 8:00 when daylight saving changes. A time the change skips plays an hour on.
	fn nth(&self, n: u64, days: u32, tz: &TimeZone) -> u64 {
		let on_the_clock = || -> Option<u64> {
			let first = Timestamp::from_second(i64::try_from(self.start_unix).ok()?).ok()?;
			let steps = i64::try_from(n).ok()?.checked_mul(i64::from(days))?;
			let at = first.to_zoned_datetime(tz.clone()).checked_add(steps.days()).ok()?;
			u64::try_from(at.timestamp().as_second()).ok()
		};
		on_the_clock().unwrap_or_else(|| self.start_unix + n * u64::from(days) * DAY)
	}

	/// Which occurrence stands at `now`, at or after the first.
	fn index_at(&self, now: u64, days: u32, tz: &TimeZone) -> u64 {
		let mut n = (now - self.start_unix) / (u64::from(days) * DAY);
		while n > 0 && self.nth(n, days, tz) > now {
			n -= 1;
		}
		while self.nth(n + 1, days, tz) <= now {
			n += 1;
		}
		n
	}

	fn occurrence_in(&self, now: u64, tz: &TimeZone) -> Option<u64> {
		if now < self.start_unix {
			return None;
		}
		Some(match self.period_days() {
			None => self.start_unix,
			Some(days) => self.nth(self.index_at(now, days, tz), days, tz),
		})
	}

	fn next_in(&self, now: u64, tz: &TimeZone) -> Option<u64> {
		if now < self.start_unix {
			return Some(self.start_unix);
		}
		self.period_days()
			.map(|days| self.nth(self.index_at(now, days, tz) + 1, days, tz))
	}

	/// The occurrence that starts at or before `now`: the one that may be
	/// recording right now. None when the first start is still ahead.
	pub fn occurrence_at(&self, now: u64) -> Option<u64> {
		self.occurrence_in(now, &TimeZone::system())
	}

	/// The first occurrence strictly after `now`, if any.
	pub fn next_after(&self, now: u64) -> Option<u64> {
		self.next_in(now, &TimeZone::system())
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

	/// The occurrence an alarm fires for at `now`: one that started
	/// within the grace — the app starting a little late still wakes
	/// you; one from long ago is missed, never a blast hours later.
	pub fn alarm_due(&self, now: u64) -> Option<u64> {
		let occ = self.occurrence_at(now)?;
		(now - occ <= ALARM_GRACE).then_some(occ)
	}
}

/// How long after its start an alarm may still fire, in seconds.
pub const ALARM_GRACE: u64 = 300;

#[cfg(test)]
mod tests {
	use super::*;
	use crate::station::Source;

	const WEEK: u64 = 7 * DAY;
	const HOUR: u64 = 3_600;

	/// US Eastern: in 2026 clocks go forward on March 8 and back on November 1.
	fn eastern() -> TimeZone {
		TimeZone::posix("EST5EDT,M3.2.0,M11.1.0").unwrap()
	}

	fn clock(tz: &TimeZone, text: &str) -> u64 {
		let at = text.parse::<jiff::civil::DateTime>().unwrap();
		at.to_zoned_datetime(tz.clone()).unwrap().timestamp().as_second() as u64
	}

	#[test]
	fn a_daily_time_stays_on_the_clock_when_daylight_saving_changes() {
		let tz = eastern();
		let spring = entry(clock(&tz, "2026-03-07T08:00"), Repeat::Daily);
		let next = clock(&tz, "2026-03-08T08:00");
		assert_eq!(next - spring.start_unix, 23 * HOUR, "that day is an hour short");
		assert_eq!(spring.next_in(spring.start_unix, &tz), Some(next));
		assert_eq!(spring.occurrence_in(next - 1, &tz), Some(spring.start_unix));
		assert_eq!(spring.occurrence_in(next, &tz), Some(next));
		let summer = clock(&tz, "2026-07-01T08:00");
		assert_eq!(spring.occurrence_in(summer + 60, &tz), Some(summer));
		let autumn = entry(clock(&tz, "2026-10-31T08:00"), Repeat::Daily);
		let next = clock(&tz, "2026-11-01T08:00");
		assert_eq!(next - autumn.start_unix, 25 * HOUR, "that day is an hour long");
		assert_eq!(autumn.next_in(autumn.start_unix, &tz), Some(next));
	}

	#[test]
	fn a_weekly_time_stays_on_the_clock_when_daylight_saving_changes() {
		let tz = eastern();
		let e = entry(clock(&tz, "2026-03-02T21:30"), Repeat::Weekly);
		let next = clock(&tz, "2026-03-09T21:30");
		assert_eq!(next - e.start_unix, WEEK - HOUR);
		assert_eq!(e.next_in(e.start_unix + 60, &tz), Some(next));
		assert_eq!(e.occurrence_in(next + 60, &tz), Some(next));
	}

	#[test]
	fn a_time_the_clocks_skip_plays_an_hour_on_then_returns() {
		let tz = eastern();
		let e = entry(clock(&tz, "2026-03-07T02:30"), Repeat::Daily);
		let skipped = clock(&tz, "2026-03-08T03:30");
		assert_eq!(e.next_in(e.start_unix, &tz), Some(skipped));
		assert_eq!(e.next_in(skipped, &tz), Some(clock(&tz, "2026-03-09T02:30")));
	}

	fn entry(start: u64, repeat: Repeat) -> ScheduledRecording {
		ScheduledRecording {
			id: "s1".into(),
			station: Station {
				id: "st".into(),
				name: "St".into(),
				url: "http://x".into(),
				source: Source::Custom,
				info: None,
				details: Default::default(),
			},
			start_unix: start,
			duration_minutes: 30,
			repeat,
			enabled: true,
			alarm: false,
			silent: true,
		}
	}

	#[test]
	fn silent_defaults_to_on_and_round_trips() {
		// An entry written before the field existed loads as silent:
		// new recordings stay quiet by default from here on.
		let json = r#"{"id":"s1","station":{"id":"st","name":"St","url":"http://x","source":"Custom","details":{"genres":[],"languages":[],"tags":[]}},"start_unix":1000,"duration_minutes":30,"repeat":"once","enabled":true,"alarm":false}"#;
		let e: ScheduledRecording = serde_json::from_str(json).unwrap();
		assert!(e.silent);
		let mut loud = entry(1_000, Repeat::Once);
		loud.silent = false;
		let back: ScheduledRecording =
			serde_json::from_str(&serde_json::to_string(&loud).unwrap()).unwrap();
		assert!(!back.silent);
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
	fn alarm_fires_at_its_start_and_within_grace_only() {
		let mut e = entry(1_000, Repeat::Daily);
		e.alarm = true;
		assert_eq!(e.alarm_due(999), None);
		assert_eq!(e.alarm_due(1_000), Some(1_000));
		assert_eq!(e.alarm_due(1_000 + ALARM_GRACE), Some(1_000));
		assert_eq!(e.alarm_due(1_000 + ALARM_GRACE + 1), None);
		assert_eq!(e.alarm_due(1_000 + DAY + 60), Some(1_000 + DAY));
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
