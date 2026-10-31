use std::time::SystemTime;

use crate::sources::HistoryItem;

/// Titles heard since the current station was tuned in.
#[derive(Default)]
pub struct SessionHistory {
	station: Option<String>,
	oldest_first: Vec<HistoryItem>,
}

impl SessionHistory {
	pub fn station_changed(&mut self, id: &str) {
		if self.station.as_deref() != Some(id) {
			self.station = Some(id.to_owned());
			self.oldest_first.clear();
		}
	}

	pub fn title(&mut self, title: Option<&str>, now: SystemTime) {
		let Some(t) = title.map(str::trim).filter(|t| !t.is_empty()) else {
			return;
		};
		if self.oldest_first.last().is_some_and(|i| i.title == t) {
			return;
		}
		self.oldest_first.push(HistoryItem {
			title: t.to_owned(),
			played_at: Some(now),
		});
	}

	pub fn items(&self) -> Vec<HistoryItem> {
		self.oldest_first.iter().rev().cloned().collect()
	}

	/// The title playing at `when`: the latest one heard at or before
	/// it. The time-shift buffer's position is a moment like this, so
	/// the title can name the song the buffer is actually playing
	/// instead of the live one.
	pub fn title_at(&self, when: SystemTime) -> Option<String> {
		self.oldest_first
			.iter()
			.rev()
			.find(|i| i.played_at.is_some_and(|t| t <= when))
			.map(|i| i.title.clone())
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HistoryKind {
	Published,
	Session,
	Nothing,
}

pub fn view(
	published: Option<Vec<HistoryItem>>,
	session: &SessionHistory,
) -> (HistoryKind, Vec<HistoryItem>) {
	if let Some(p) = published.filter(|p| !p.is_empty()) {
		return (HistoryKind::Published, p);
	}
	let s = session.items();
	if s.is_empty() {
		(HistoryKind::Nothing, s)
	} else {
		(HistoryKind::Session, s)
	}
}

pub fn heading(kind: HistoryKind) -> &'static str {
	match kind {
		HistoryKind::Published => "Station history",
		HistoryKind::Session => "Heard since you tuned in",
		HistoryKind::Nothing => "No song information",
	}
}

pub fn ago(t: SystemTime, now: SystemTime) -> String {
	let mins = now.duration_since(t).map_or(0, |d| d.as_secs() / 60);
	match mins {
		0 => "Just now".into(),
		1..=59 => format!("{mins} min"),
		60..=1439 => match (mins / 60, mins % 60) {
			(h, 0) => format!("{h} h"),
			(h, m) => format!("{h} h {m} min"),
		},
		_ => format!("{} d", mins / 1440),
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::time::Duration;

	fn t(secs: u64) -> SystemTime {
		SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
	}

	#[test]
	fn session_clears_on_station_change() {
		let mut s = SessionHistory::default();
		s.station_changed("a");
		s.title(Some("one"), t(1));
		s.station_changed("a");
		assert_eq!(s.items().len(), 1);
		s.station_changed("b");
		assert!(s.items().is_empty());
	}

	#[test]
	fn session_skips_repeats_and_none() {
		let mut s = SessionHistory::default();
		s.title(None, t(1));
		s.title(Some("one"), t(2));
		s.title(Some("one"), t(3));
		s.title(Some("two"), t(4));
		s.title(Some("one"), t(5));
		let titles: Vec<_> = s.items().into_iter().map(|i| i.title).collect();
		assert_eq!(titles, ["one", "two", "one"]);
		assert_eq!(s.items()[0].played_at, Some(t(5)));
	}

	#[test]
	fn title_at_finds_the_song_of_the_moment() {
		let mut s = SessionHistory::default();
		s.station_changed("a");
		assert_eq!(s.title_at(t(10)), None, "nothing heard yet");
		s.title(Some("one"), t(100));
		s.title(Some("two"), t(200));
		assert_eq!(s.title_at(t(99)), None, "before the first title");
		assert_eq!(s.title_at(t(100)).as_deref(), Some("one"));
		assert_eq!(s.title_at(t(150)).as_deref(), Some("one"));
		assert_eq!(s.title_at(t(200)).as_deref(), Some("two"));
		assert_eq!(s.title_at(t(999)).as_deref(), Some("two"), "after the last");
	}

	#[test]
	fn published_wins_when_present() {
		let mut s = SessionHistory::default();
		assert_eq!(view(None, &s), (HistoryKind::Nothing, vec![]));
		assert_eq!(view(Some(vec![]), &s).0, HistoryKind::Nothing);
		s.title(Some("heard"), t(1));
		assert_eq!(view(None, &s).0, HistoryKind::Session);
		let p = vec![HistoryItem {
			title: "pub".into(),
			played_at: None,
		}];
		assert_eq!(view(Some(p.clone()), &s), (HistoryKind::Published, p));
	}

	#[test]
	fn headings_exact() {
		assert_eq!(heading(HistoryKind::Published), "Station history");
		assert_eq!(heading(HistoryKind::Session), "Heard since you tuned in");
		assert_eq!(heading(HistoryKind::Nothing), "No song information");
	}

	#[test]
	fn ago_formats() {
		let now = t(1_000_000);
		let a = |s: u64| ago(t(1_000_000 - s), now);
		assert_eq!(a(0), "Just now");
		assert_eq!(a(59), "Just now");
		assert_eq!(a(300), "5 min");
		assert_eq!(a(3600), "1 h");
		assert_eq!(a(4800), "1 h 20 min");
		assert_eq!(a(86_400), "1 d");
		assert_eq!(a(2 * 86_400 + 5), "2 d");
		assert_eq!(ago(t(2_000_000), now), "Just now");
	}
}
