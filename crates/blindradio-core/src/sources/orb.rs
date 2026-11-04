//! Online Radio Box: stations by region, country and genre, search, and a week of each
//! station's songs.
//! ponytail: reads web pages, no public API, and its terms do not offer this use: owner's call,
//! 2026-11-04. A site rewrite empties this until fixed.

use super::{HistoryItem, Node, SearchQuery, fold, holds_words};
use crate::net::get_bytes;
use crate::station::{Details, Source, Station, known};
use crate::{CoreError, Result};
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use scraper::{ElementRef, Html, Selector};
use std::collections::HashSet;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Today and the seven days its playlist pages promise.
const DAYS: u64 = 8;
const REGIONS: [(&str, &str); 6] = [
	("Africa", "Africa"),
	("Asia", "Asia"),
	("Europe", "Europe"),
	("North-America", "North America"),
	("Oceania", "Oceania"),
	("South-America", "South America"),
];

pub struct Orb {
	base: String,
}

struct Row {
	alias: String,
	name: String,
	stream: String,
	tags: Vec<String>,
	country: String,
}

/// One playlist page: the station's clock where the page shows it, and its rows,
/// newest first. A row with no clock is the song playing now.
#[derive(Debug, Default, PartialEq)]
struct Day {
	now_in: Option<i64>,
	rows: Vec<(Option<i64>, String)>,
}

fn bad_reply() -> CoreError {
	CoreError::Http("Bad reply".into())
}

fn selector(css: &str) -> Selector {
	Selector::parse(css).expect("static selector")
}

fn text(element: ElementRef<'_>) -> String {
	element.text().collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Letters, digits, `-`, `_` and `.`: all the site uses, and safe to put in an address.
fn clean(slug: &str) -> bool {
	!slug.is_empty()
		&& slug
			.bytes()
			.all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

fn folder(id: String, name: impl Into<String>) -> Node {
	Node::Folder {
		id,
		name: name.into(),
	}
}

fn rows(html: &Html) -> Vec<Row> {
	let (item, play, tag, flag) = (
		selector("li.stations__station"),
		selector("button.station_play"),
		selector("ul.stations__station__tags a"),
		selector("a.i-flag"),
	);
	html.select(&item)
		.filter_map(|li| {
			let button = li.select(&play).next()?.value();
			let stream = button.attr("stream")?.trim();
			let alias = button.attr("radioid")?.trim();
			let name = button.attr("radioname")?.trim();
			let plays = stream.starts_with("http")
				&& button.attr("streamtype") != Some("mpd")
				&& !stream.split('?').next().unwrap_or(stream).ends_with(".mpd");
			(plays && clean(alias) && !name.is_empty()).then(|| Row {
				alias: alias.to_owned(),
				name: name.to_owned(),
				stream: stream.to_owned(),
				tags: li.select(&tag).map(text).filter(|t| !t.is_empty()).collect(),
				country: li
					.select(&flag)
					.next()
					.and_then(|a| a.value().attr("title"))
					.unwrap_or("")
					.trim()
					.to_owned(),
			})
		})
		.collect()
}

fn station(row: Row) -> Station {
	let shown = row.tags.iter().take(3).cloned().collect::<Vec<_>>().join(", ");
	Station {
		id: format!("orb:{}", row.alias),
		name: row.name,
		url: row.stream,
		source: Source::OnlineRadioBox,
		info: known(&shown),
		details: Details {
			genre: known(&row.tags.join(", ")),
			country: known(&row.country),
			..Default::default()
		},
	}
}

fn genres(html: &Html) -> Vec<Node> {
	let link = selector("#tags a");
	let mut seen = HashSet::new();
	let mut found: Vec<(String, String)> = html
		.select(&link)
		.filter_map(|a| {
			let slug = a.value().attr("href")?.strip_prefix("/genre/")?.trim_end_matches('/');
			// A slug ending in `-` is a group's own page, such as `artists-`.
			(clean(slug) && !slug.ends_with('-') && seen.insert(slug.to_owned()))
				.then(|| (text(a), slug.to_owned()))
		})
		.collect();
	found.sort_by_key(|(name, _)| name.to_lowercase());
	found
		.into_iter()
		.map(|(name, slug)| folder(format!("orb/genre/{slug}"), name))
		.collect()
}

fn countries(html: &Html) -> Vec<Node> {
	let link = selector("ul.countries__countries-list a");
	let mut seen = HashSet::new();
	html.select(&link)
		.filter_map(|a| {
			let code = a.value().attr("href")?.trim_matches('/');
			(clean(code) && seen.insert(code.to_owned())).then(|| folder(format!("orb/country/{code}"), text(a)))
		})
		.collect()
}

/// The highest `?p=` the page's pager links to. `?p=1` is the second page.
fn last_page(html: &Html) -> u32 {
	let link = selector(".pagination a");
	html.select(&link)
		.filter_map(|a| a.value().attr("href")?.rsplit_once("p=")?.1.parse().ok())
		.max()
		.unwrap_or(0)
}

fn clock(text: &str) -> Option<i64> {
	let (hours, minutes) = text.trim().split_once(':')?;
	let (hours, minutes): (i64, i64) = (hours.parse().ok()?, minutes.parse().ok()?);
	(hours < 24 && minutes < 60).then_some(hours * 60 + minutes)
}

fn day(html: &str) -> Day {
	let html = Html::parse_document(html);
	let (shown, row, time, track) = (
		selector(".station-onair__localtime nobr"),
		selector("table.tablelist-schedule tr"),
		selector("td.tablelist-schedule__time"),
		selector("td.track_history_item"),
	);
	Day {
		now_in: html.select(&shown).next().and_then(|n| {
			clock(text(n).rsplit(' ').next()?.trim_end_matches(')'))
		}),
		rows: html
			.select(&row)
			.filter_map(|tr| {
				let title = text(tr.select(&track).next()?);
				let at = tr.select(&time).next().map(text).and_then(|t| clock(&t));
				(!title.is_empty()).then_some((at, title))
			})
			.collect(),
	}
}

/// The page's clock minus UTC, in minutes. Prefer the station's clock. A page without one
/// lists times for wherever the listener's address looks to be, so the newest time, which
/// has just passed, gives it away: to the next 15 minutes, as zones run.
/// ponytail: one offset for the week, so older rows are an hour out past a clock change.
fn offset_minutes(days: &[Day], now: u64) -> Option<i64> {
	let utc = (now / 60 % 1440) as i64;
	let wrap = |minutes: i64| match minutes - utc {
		d if d > 840 => d - 1440,
		d if d < -720 => d + 1440,
		d => d,
	};
	if let Some(shown) = days.iter().find_map(|d| d.now_in) {
		return Some((wrap(shown) + 7).div_euclid(15) * 15);
	}
	let newest = days.first()?.rows.iter().find_map(|(at, _)| *at)?;
	Some((wrap(newest) + 14).div_euclid(15) * 15)
}

/// `days[n]` is `n` days back. Newest first; a row after now, or with no place on the
/// clock, counts as now on the first page and as unknown on the others.
fn items(days: &[Day], station_name: &str, now: SystemTime) -> Vec<HistoryItem> {
	let now_secs = now.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
	let offset = offset_minutes(days, now_secs);
	let name = fold(station_name).0;
	let mut out = Vec::new();
	for (back, day) in days.iter().enumerate() {
		for (at, title) in &day.rows {
			// Some stations send their own name between songs.
			if fold(title).0 == name {
				continue;
			}
			let played_at = match (at, offset) {
				(None, _) if back == 0 => Some(now),
				(Some(at), Some(offset)) => {
					let local_day = (now_secs as i64 + offset * 60).div_euclid(86_400) - back as i64;
					let secs = local_day * 86_400 + (at - offset) * 60;
					u64::try_from(secs)
						.ok()
						.map(|s| (UNIX_EPOCH + Duration::from_secs(s)).min(now))
				}
				_ => None,
			};
			out.push(HistoryItem {
				title: title.clone(),
				played_at,
			});
		}
	}
	out
}

/// The address without what varies between copies of a stream: scheme, query, file type, and
/// a mount's encoding as in `CHMPFMAAC` and `CHMPFM_SC`.
fn stream_key(url: &str) -> String {
	let url = url.to_lowercase();
	let url = url.split_once("://").map_or(url.as_str(), |(_, rest)| rest);
	let url = url.split(['?', '#']).next().unwrap_or(url).trim_end_matches('/');
	let url = [".mp3", ".aac", ".m3u8", ".m3u", ".pls"]
		.iter()
		.find_map(|ext| url.strip_suffix(ext))
		.unwrap_or(url);
	let url = url.strip_suffix("_sc").unwrap_or(url);
	url.strip_suffix("aac").unwrap_or(url).to_owned()
}

/// The row that is this station. A name alone is not enough: most names repeat across the
/// world, and the wrong row shows another station's songs. So the same stream, or the same
/// name in the same country. A curated network names no country, so it needs the stream.
fn matching<'a>(found: &'a [Row], station: &Station) -> Option<&'a Row> {
	let key = stream_key(&station.url);
	let name = fold(&station.name).0;
	found.iter().find(|r| stream_key(&r.stream) == key).or_else(|| {
		let country = station.details.country.as_deref()?;
		found.iter().find(|r| {
			fold(&r.name).0 == name && !r.country.is_empty() && holds_words(country, &r.country)
		})
	})
}

impl Orb {
	pub fn new(base: impl Into<String>) -> Orb {
		Orb { base: base.into() }
	}

	fn url(&self, path: &str) -> String {
		format!("{}{path}", self.base.trim_end_matches('/'))
	}

	async fn page(&self, client: &reqwest::Client, path: &str) -> Result<String> {
		let bytes = get_bytes(client, &self.url(path)).await?;
		Ok(String::from_utf8_lossy(&bytes).into_owned())
	}

	pub async fn children(&self, client: &reqwest::Client, id: &str) -> Result<Vec<Node>> {
		if id == "orb" {
			let mut nodes = vec![folder("orb/genres".into(), "By Genre")];
			nodes.extend(REGIONS.map(|(slug, name)| folder(format!("orb/region/{slug}"), name)));
			return Ok(nodes);
		}
		if id == "orb/genres" {
			let page = self.page(client, "/genres/").await?;
			return Ok(genres(&Html::parse_document(&page)));
		}
		if let Some(region) = id.strip_prefix("orb/region/") {
			if !clean(region) {
				return Err(bad_reply());
			}
			let page = self.page(client, &format!("/{region}/")).await?;
			return Ok(countries(&Html::parse_document(&page)));
		}
		let (head, number) = match id.rsplit_once('@') {
			Some((head, n)) => (head, n.parse::<u32>().unwrap_or(0)),
			None => (id, 0),
		};
		let path = match (head.strip_prefix("orb/genre/"), head.strip_prefix("orb/country/")) {
			(Some(slug), _) if clean(slug) => format!("/genre/{slug}/"),
			(_, Some(code)) if clean(code) => format!("/{code}/"),
			_ => return Ok(vec![]),
		};
		let path = if number == 0 {
			path
		} else {
			format!("{path}?p={number}")
		};
		let page = self.page(client, &path).await?;
		let html = Html::parse_document(&page);
		let mut nodes: Vec<Node> = rows(&html)
			.into_iter()
			.map(|row| Node::Station(station(row)))
			.collect();
		if number < last_page(&html) {
			nodes.push(Node::LoadMore {
				id: format!("{head}@{}", number + 1),
			});
		}
		Ok(nodes)
	}

	async fn found(&self, client: &reqwest::Client, name: &str) -> Result<Vec<Row>> {
		let query = utf8_percent_encode(name, NON_ALPHANUMERIC);
		let page = self.page(client, &format!("/search?q={query}")).await?;
		Ok(rows(&Html::parse_document(&page)))
	}

	pub async fn search(&self, client: &reqwest::Client, q: &SearchQuery) -> Result<Vec<Station>> {
		let name = q.name.trim();
		if name.is_empty() {
			return Ok(vec![]);
		}
		Ok(self.found(client, name).await?.into_iter().map(station).collect())
	}

	async fn alias(&self, client: &reqwest::Client, station: &Station) -> Option<String> {
		if let Some(alias) = station.id.strip_prefix("orb:") {
			return clean(alias).then(|| alias.to_owned());
		}
		// Directories add notes after the name: `Jazz Radio (128k)`, `1LIVE | WDR`.
		let name = station.name.split(['(', '[', '|']).next()?.trim();
		if name.is_empty() {
			return None;
		}
		let found = self.found(client, name).await.ok()?;
		matching(&found, station).map(|r| r.alias.clone())
	}

	/// The week of songs, newest first; `None` when this station is not there or lists none.
	pub async fn history(&self, client: &reqwest::Client, station: &Station) -> Option<Vec<HistoryItem>> {
		let alias = self.alias(client, station).await?;
		let (country, rest) = alias.split_once('.')?;
		let pages: Vec<_> = (0..DAYS)
			.map(|back| {
				let number = if back == 0 { String::new() } else { back.to_string() };
				let url = self.url(&format!("/{country}/{rest}/playlist/{number}?useStationLocation=1"));
				let client = client.clone();
				tokio::spawn(async move { get_bytes(&client, &url).await })
			})
			.collect();
		let mut days = Vec::with_capacity(pages.len());
		for page in pages {
			// A missing day stays in its place, so the days after it keep their dates.
			days.push(match page.await {
				Ok(Ok(bytes)) => day(&String::from_utf8_lossy(&bytes)),
				_ => Day::default(),
			});
		}
		let found = items(&days, &station.name, SystemTime::now());
		(!found.is_empty()).then_some(found)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn page(body: &str) -> Html {
		Html::parse_document(body)
	}

	const ROWS: &str = r#"<ul>
<li class="stations__station"><ul class="stations__station__tags"><li><a>indie</a></li><li><a>rock</a></li><li><a>pop</a></li><li><a>jazz</a></li></ul>
<ul class="stations__station__info"><li><a class="i-flag us" title="United States"></a></li></ul>
<button class="b-play station_play" stream="https://x.test/kexp64.aac" streamType="mp3" radioId="us.kexpfm" radioName="KEXP &amp; Co"></button></li>
<li class="stations__station"><button class="b-play station_play" stream="https://x.test/a.mpd" streamType="mpd" radioId="uk.dash" radioName="Dash"></button></li>
<li class="stations__station"><button class="b-play station_play" stream="" radioId="uk.none" radioName="None"></button></li>
<li class="stations__station"><button class="b-play station_play" stream="https://x.test/b" radioId="../bad" radioName="Bad"></button></li>
</ul>"#;

	#[test]
	fn rows_map_to_stations_and_skip_what_cannot_play() {
		let found = rows(&page(ROWS));
		assert_eq!(found.len(), 1);
		let st = station(found.into_iter().next().unwrap());
		assert_eq!(st.id, "orb:us.kexpfm");
		assert_eq!(st.name, "KEXP & Co");
		assert_eq!(st.url, "https://x.test/kexp64.aac");
		assert_eq!(st.source, Source::OnlineRadioBox);
		assert_eq!(st.info.as_deref(), Some("indie, rock, pop"));
		assert_eq!(st.details.genre.as_deref(), Some("indie, rock, pop, jazz"));
		assert_eq!(st.details.country.as_deref(), Some("United States"));
	}

	#[test]
	fn pager_links_give_the_last_page() {
		let html = page(
			r#"<dl class="pagination"><dd><a href="https://x/ca/?p=1">2</a></dd><dd><a href="https://x/ca/?p=53">54</a></dd><dd><a href="https://x/ca/?p=1">Next</a></dd></dl>"#,
		);
		assert_eq!(last_page(&html), 53);
		assert_eq!(last_page(&page("<p>one page</p>")), 0);
	}

	#[test]
	fn genres_drop_group_pages_and_repeats() {
		let html = page(
			r#"<div id="tags"><a href="/genre/rock-/">Rock music</a><a href="/genre/jazz/">jazz</a><a href="/genre/acid_jazz/">acid jazz</a><a href="/genre/jazz/">jazz</a></div>"#,
		);
		assert_eq!(
			genres(&html),
			[
				folder("orb/genre/acid_jazz".into(), "acid jazz"),
				folder("orb/genre/jazz".into(), "jazz")
			]
		);
	}

	#[test]
	fn playlist_page_reads_clock_and_rows() {
		let got = day(
			r#"<div class="station-onair__localtime"><nobr>(<a>now in London</a> 08:11)
</nobr></div><table class="tablelist-schedule"><tbody>
<tr class="active"><td class="tablelist-schedule__time"><span>Live</span></td><td class="track_history_item"><a>Band - Now</a></td></tr>
<tr><td class="tablelist-schedule__time"><span>08:05</span></td><td class="track_history_item"><a>Fall Out Boy - Sugar, We&#39;re
	Going</a></td></tr>
<tr><td class="tablelist-schedule__time"><span>08:01</span></td><td class="track_history_item">Plain Text</td></tr>
</tbody></table>"#,
		);
		assert_eq!(got.now_in, Some(8 * 60 + 11));
		assert_eq!(
			got.rows,
			[
				(None, "Band - Now".to_owned()),
				(Some(8 * 60 + 5), "Fall Out Boy - Sugar, We're Going".to_owned()),
				(Some(8 * 60 + 1), "Plain Text".to_owned()),
			]
		);
		assert_eq!(day("<html></html>"), Day::default());
	}

	/// 2026-11-04 08:12:00 UTC.
	const NOW: u64 = 1_793_779_920;

	fn at(secs: u64) -> Option<SystemTime> {
		Some(UNIX_EPOCH + Duration::from_secs(secs))
	}

	fn when(rows: &[(Option<i64>, &str)], now_in: Option<i64>) -> Day {
		Day {
			now_in,
			rows: rows.iter().map(|(a, t)| (*a, (*t).to_owned())).collect(),
		}
	}

	#[test]
	fn the_stations_clock_dates_every_day() {
		// Sydney, 19:12: its own day has already turned.
		let days = [
			when(&[(None, "Live"), (Some(19 * 60 + 6), "Today")], Some(19 * 60 + 12)),
			when(&[(Some(23 * 60 + 58), "Yesterday")], Some(19 * 60 + 12)),
		];
		let got = items(&days, "Station", UNIX_EPOCH + Duration::from_secs(NOW));
		assert_eq!(got[0].played_at, at(NOW));
		assert_eq!(got[1].played_at, at(NOW - 6 * 60));
		// 23:58 the day before: 19 h 14 min before 19:12.
		assert_eq!(got[2].played_at, at(NOW - (19 * 3600 + 14 * 60)));
	}

	#[test]
	fn with_no_clock_the_newest_row_gives_the_zone_away() {
		// Listener at UTC-7: 01:12 now, last song 01:05.
		let days = [when(&[(None, "Live"), (Some(65), "Today")], None)];
		let got = items(&days, "Station", UNIX_EPOCH + Duration::from_secs(NOW));
		assert_eq!(got[1].played_at, at(NOW - 7 * 60));
		// Nothing to read the zone from: older rows have no time rather than a wrong one.
		let days = [Day::default(), when(&[(Some(65), "Old")], None)];
		let got = items(&days, "Station", UNIX_EPOCH + Duration::from_secs(NOW));
		assert_eq!(got[0].played_at, None);
	}

	#[test]
	fn a_stations_own_name_is_not_a_song() {
		let days = [when(&[(None, "Deutschlandfunk Kultur"), (None, "Song")], None)];
		let got = items(&days, "Deutschlandfunk  Kultur", UNIX_EPOCH + Duration::from_secs(NOW));
		assert_eq!(got.len(), 1);
		assert_eq!(got[0].title, "Song");
	}

	#[test]
	fn a_later_clock_never_passes_now() {
		let days = [when(&[(Some(8 * 60 + 13), "Rounded")], Some(8 * 60 + 12))];
		let now = UNIX_EPOCH + Duration::from_secs(NOW);
		assert_eq!(items(&days, "S", now)[0].played_at, Some(now));
	}

	fn row(alias: &str, name: &str, stream: &str, country: &str) -> Row {
		Row {
			alias: alias.into(),
			name: name.into(),
			stream: stream.into(),
			tags: vec![],
			country: country.into(),
		}
	}

	fn wanted(name: &str, url: &str, country: Option<&str>) -> Station {
		Station {
			id: "rb:x".into(),
			name: name.into(),
			url: url.into(),
			source: Source::RadioBrowser,
			info: None,
			details: Details {
				country: country.map(Into::into),
				..Default::default()
			},
		}
	}

	#[test]
	fn matching_wants_the_stream_or_name_and_country() {
		let found = [
			row("de.thewave", "The Wave", "https://de.test/wave", "Germany"),
			row(
				"ca.985fm",
				"98,5 FM",
				"https://p.streamtheworld.com/api/livestream-redirect/CHMPFMAAC.m3u8?dist=orb",
				"Canada",
			),
			row(
				"us.thewave",
				"The Wave",
				"https://us.test/wave",
				"United States",
			),
		];
		let alias = |s: Station| matching(&found, &s).map(|r| r.alias.as_str()).map(str::to_owned);
		assert_eq!(
			alias(wanted(
				"Anything",
				"http://p.streamtheworld.com/api/livestream-redirect/CHMPFM_SC.mp3",
				None
			))
			.as_deref(),
			Some("ca.985fm")
		);
		assert_eq!(
			alias(wanted(
				"THE  WAVE",
				"https://other.test/x",
				Some("The United States Of America")
			))
			.as_deref(),
			Some("us.thewave")
		);
		assert_eq!(alias(wanted("The Wave", "https://other.test/x", None)), None);
		assert_eq!(alias(wanted("The Wave", "https://other.test/x", Some("France"))), None);
	}

	#[test]
	fn stream_keys_drop_what_varies() {
		assert_eq!(stream_key("HTTPS://X.test/a.mp3?t=1"), "x.test/a");
		assert_eq!(stream_key("http://x.test/a/"), "x.test/a");
		assert_eq!(stream_key("http://x.test/MOUNTAAC_SC.mp3"), "x.test/mount");
	}
}
