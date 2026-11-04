//! Song history stations publish in the open, found from a stream's address, so it works
//! for a station from any directory.
//! ponytail: Triton and BBC document none of this. A change there empties that history until fixed.

use super::{Endpoints, HistoryItem};
use crate::net::{get_bytes, get_json};
use reqwest::Url;
use serde::Deserialize;
use std::cmp::Reverse;
use std::ops::Range;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Triton returns no more than this, about three hours.
const TRITON_ROWS: u32 = 50;
const KEXP_ROWS: u32 = 50;
const RADIO_PARADISE_ROWS: u32 = 50;
/// BBC answers 400 above about this.
const BBC_ROWS: u32 = 10;

pub struct Played {
	triton: String,
	live365: String,
	laut_fm: String,
	bbc: String,
	kexp: String,
	radio_paradise: String,
}

#[derive(Debug, PartialEq)]
enum Provider {
	Triton(String),
	Live365(String),
	LautFm(String),
	AzuraCast { base: String, code: String },
	Bbc(String),
	Kexp,
	RadioParadise(u32),
}

fn provider(url: &str) -> Option<Provider> {
	let url = Url::parse(url).ok()?;
	let host = url.host_str()?.to_ascii_lowercase();
	let parts: Vec<&str> = url.path_segments()?.filter(|p| !p.is_empty()).collect();
	let first = parts.first().copied().unwrap_or("");
	let stem = |part: &str| part.split('.').next().unwrap_or("").to_owned();
	if host.ends_with("streamtheworld.com") {
		let mount = stem(parts.last()?);
		return (!mount.is_empty()).then_some(Provider::Triton(mount));
	}
	if host == "streaming.live365.com" && !first.is_empty() {
		return Some(Provider::Live365(first.to_owned()));
	}
	if (host == "stream.laut.fm" || host.ends_with(".stream.laut.fm")) && !first.is_empty() {
		return Some(Provider::LautFm(first.to_owned()));
	}
	if host.contains("kexp") && (host.ends_with("streamguys1.com") || host.ends_with("kexp.org")) {
		return Some(Provider::Kexp);
	}
	if host == "stream.radioparadise.com" {
		return radio_paradise_channel(first).map(Provider::RadioParadise);
	}
	if host.ends_with("akamaized.net") || host.ends_with("bbci.co.uk") {
		return parts
			.iter()
			.map(|p| stem(p))
			.find(|p| p.starts_with("bbc_"))
			.map(Provider::Bbc);
	}
	if let ["listen", code, _, ..] = parts.as_slice() {
		return Some(Provider::AzuraCast {
			base: url.origin().ascii_serialization(),
			code: (*code).to_owned(),
		});
	}
	None
}

/// KFAT and Sanctuary publish no list under any channel number that answers.
fn radio_paradise_channel(mount: &str) -> Option<u32> {
	let name = mount.split('-').next().unwrap_or(mount);
	Some(match name {
		"aac" | "mp3" | "flac" => 0,
		"mellow" => 1,
		"rock" => 2,
		"global" => 3,
		"beyond" => 5,
		"serenity" => 42,
		"radio2050" => 2050,
		_ => return None,
	})
}

/// A mount's address may name one encoding; its titles sit under the plain name.
fn triton_mounts(mount: &str) -> Vec<&str> {
	let bare = mount.strip_suffix("_SC").unwrap_or(mount);
	let plain = bare.strip_suffix("AAC").unwrap_or(bare);
	let mut mounts = vec![mount];
	for m in [bare, plain] {
		if !mounts.contains(&m) {
			mounts.push(m);
		}
	}
	mounts
}

fn song(artist: &str, title: &str) -> Option<String> {
	let (artist, title) = (artist.trim(), title.trim());
	match (artist.is_empty(), title.is_empty()) {
		(_, true) => None,
		(true, false) => Some(title.to_owned()),
		(false, false) => Some(format!("{artist} - {title}")),
	}
}

fn at_seconds(seconds: i64) -> Option<SystemTime> {
	Some(UNIX_EPOCH + Duration::from_secs(u64::try_from(seconds).ok()?))
}

fn at_millis(millis: u64) -> SystemTime {
	UNIX_EPOCH + Duration::from_millis(millis)
}

/// Days since 1970 of a calendar date, by Howard Hinnant's method.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
	let year = if month <= 2 { year - 1 } else { year };
	let era = year.div_euclid(400);
	let year_of_era = year - era * 400;
	let shifted_month = if month > 2 { month - 3 } else { month + 9 };
	let day_of_year = (153 * shifted_month + 2) / 5 + day - 1;
	let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
	era * 146_097 + day_of_era - 719_468
}

/// `2026-11-04 08:05:16.45+00:00`, `2026-11-04 08:51:19 +0100` or `2026-11-03T23:53:51-08:00`.
fn parse_time(text: &str) -> Option<SystemTime> {
	let text = text.trim();
	let number = |range: Range<usize>| text.get(range)?.parse::<i64>().ok();
	let days = days_from_civil(number(0..4)?, number(5..7)?, number(8..10)?);
	let clock = number(11..13)? * 3600 + number(14..16)? * 60 + number(17..19)?;
	let zone = text.get(19..)?;
	let offset = match zone.find(['+', '-']) {
		Some(at) => {
			let digits: String = zone[at + 1..].chars().filter(char::is_ascii_digit).collect();
			let hours: i64 = digits.get(0..2)?.parse().ok()?;
			let minutes: i64 = digits.get(2..4)?.parse().ok()?;
			let seconds = hours * 3600 + minutes * 60;
			if zone[at..].starts_with('-') { -seconds } else { seconds }
		}
		None => 0,
	};
	at_seconds(days * 86_400 + clock - offset)
}

/// The BBC gives only a phrase: `Now Playing`, `1 Minute Ago`, `3 Hours Ago`.
fn bbc_time(label: &str, now: SystemTime) -> Option<SystemTime> {
	if label.eq_ignore_ascii_case("now playing") {
		return Some(now);
	}
	let mut words = label.split_whitespace();
	let count: u64 = words.next()?.parse().ok()?;
	let unit = match words.next()?.to_ascii_lowercase().trim_end_matches('s') {
		"minute" => 60,
		"hour" => 3600,
		_ => return None,
	};
	now.checked_sub(Duration::from_secs(count * unit))
}

#[derive(Deserialize)]
struct TritonList {
	#[serde(rename = "nowplaying-info", default)]
	info: Vec<TritonInfo>,
}

#[derive(Deserialize)]
struct TritonInfo {
	#[serde(default)]
	property: Vec<TritonProperty>,
}

#[derive(Deserialize)]
struct TritonProperty {
	#[serde(rename = "@name")]
	name: String,
	#[serde(rename = "$text", default)]
	value: String,
}

fn triton_items(xml: &[u8]) -> Option<Vec<HistoryItem>> {
	let list: TritonList = quick_xml::de::from_reader(xml).ok()?;
	Some(
		list.info
			.iter()
			.filter_map(|info| {
				let field = |name: &str| {
					info.property
						.iter()
						.find(|p| p.name == name)
						.map_or("", |p| p.value.as_str())
				};
				Some(HistoryItem {
					title: song(field("track_artist_name"), field("cue_title"))?,
					played_at: field("cue_time_start").parse().ok().map(at_millis),
				})
			})
			.collect(),
	)
}

#[derive(Deserialize)]
struct Live365Station {
	#[serde(rename = "current-track")]
	current: Option<Live365Track>,
	#[serde(rename = "last-played", default)]
	last: Vec<Live365Track>,
}

#[derive(Deserialize)]
struct Live365Track {
	#[serde(default)]
	title: String,
	#[serde(default)]
	artist: String,
	#[serde(default)]
	start: String,
}

#[derive(Deserialize)]
struct LautSong {
	#[serde(default)]
	title: String,
	artist: Option<LautArtist>,
	#[serde(default)]
	started_at: String,
}

#[derive(Deserialize)]
struct LautArtist {
	#[serde(default)]
	name: String,
}

#[derive(Deserialize)]
struct AzuraNow {
	now_playing: Option<AzuraPlay>,
	#[serde(default)]
	song_history: Vec<AzuraPlay>,
}

#[derive(Deserialize)]
struct AzuraPlay {
	played_at: i64,
	song: AzuraSong,
}

#[derive(Deserialize)]
struct AzuraSong {
	#[serde(default)]
	artist: String,
	#[serde(default)]
	title: String,
	#[serde(default)]
	text: String,
}

#[derive(Deserialize)]
struct BbcSegments {
	#[serde(default)]
	data: Vec<BbcSegment>,
}

#[derive(Deserialize)]
struct BbcSegment {
	#[serde(default)]
	segment_type: String,
	titles: BbcTitles,
	offset: BbcOffset,
}

#[derive(Deserialize)]
struct BbcTitles {
	primary: Option<String>,
	secondary: Option<String>,
}

#[derive(Deserialize)]
struct BbcOffset {
	#[serde(default)]
	label: String,
}

#[derive(Deserialize)]
struct KexpPlays {
	#[serde(default)]
	results: Vec<KexpPlay>,
}

#[derive(Deserialize)]
struct KexpPlay {
	#[serde(default)]
	play_type: String,
	artist: Option<String>,
	song: Option<String>,
	#[serde(default)]
	airdate: String,
}

#[derive(Deserialize)]
struct ParadiseList {
	#[serde(default)]
	song: Vec<ParadiseSong>,
}

#[derive(Deserialize)]
struct ParadiseSong {
	#[serde(default)]
	artist: String,
	#[serde(default)]
	title: String,
	play_time: Option<u64>,
}

impl Played {
	pub fn new(endpoints: &Endpoints) -> Played {
		Played {
			triton: endpoints.triton.clone(),
			live365: endpoints.live365_api.clone(),
			laut_fm: endpoints.laut_fm.clone(),
			bbc: endpoints.bbc.clone(),
			kexp: endpoints.kexp.clone(),
			radio_paradise: endpoints.radio_paradise.clone(),
		}
	}

	/// Newest first; `None` when this stream has no such list, it is empty, or it failed.
	pub async fn history(&self, client: &reqwest::Client, stream: &str) -> Option<Vec<HistoryItem>> {
		let mut items = match provider(stream)? {
			Provider::Triton(mount) => self.triton(client, &mount).await,
			Provider::Live365(mount) => self.live365(client, &mount).await,
			Provider::LautFm(name) => self.laut_fm(client, &name).await,
			Provider::AzuraCast { base, code } => azuracast(client, &base, &code).await,
			Provider::Bbc(service) => self.bbc(client, &service).await,
			Provider::Kexp => self.kexp(client).await,
			Provider::RadioParadise(channel) => self.radio_paradise(client, channel).await,
		}?;
		items.sort_by_key(|item| Reverse(item.played_at));
		(!items.is_empty()).then_some(items)
	}

	async fn triton(&self, client: &reqwest::Client, mount: &str) -> Option<Vec<HistoryItem>> {
		for mount in triton_mounts(mount) {
			let url = format!(
				"{}/public/nowplaying?mountName={mount}&numberToFetch={TRITON_ROWS}&eventType=track",
				self.triton.trim_end_matches('/')
			);
			let items = get_bytes(client, &url)
				.await
				.ok()
				.and_then(|body| triton_items(&body));
			if let Some(items) = items.filter(|i| !i.is_empty()) {
				return Some(items);
			}
		}
		None
	}

	async fn live365(&self, client: &reqwest::Client, mount: &str) -> Option<Vec<HistoryItem>> {
		let url = format!("{}/station/{mount}", self.live365.trim_end_matches('/'));
		let station: Live365Station = get_json(client, &url).await.ok()?;
		Some(
			station
				.current
				.into_iter()
				.chain(station.last)
				.filter_map(|t| {
					Some(HistoryItem {
						title: song(&t.artist, &t.title)?,
						played_at: parse_time(&t.start),
					})
				})
				.collect(),
		)
	}

	async fn laut_fm(&self, client: &reqwest::Client, name: &str) -> Option<Vec<HistoryItem>> {
		let url = format!("{}/station/{name}/last_songs", self.laut_fm.trim_end_matches('/'));
		let songs: Vec<LautSong> = get_json(client, &url).await.ok()?;
		Some(
			songs
				.iter()
				.filter_map(|s| {
					let artist = s.artist.as_ref().map_or("", |a| a.name.as_str());
					Some(HistoryItem {
						title: song(artist, &s.title)?,
						played_at: parse_time(&s.started_at),
					})
				})
				.collect(),
		)
	}

	async fn bbc(&self, client: &reqwest::Client, service: &str) -> Option<Vec<HistoryItem>> {
		let url = format!(
			"{}/v2/services/{service}/segments/latest?limit={BBC_ROWS}",
			self.bbc.trim_end_matches('/')
		);
		let segments: BbcSegments = get_json(client, &url).await.ok()?;
		let now = SystemTime::now();
		Some(
			segments
				.data
				.iter()
				.filter(|s| s.segment_type == "music")
				.filter_map(|s| {
					let artist = s.titles.primary.as_deref().unwrap_or("");
					let title = s.titles.secondary.as_deref().unwrap_or("");
					Some(HistoryItem {
						title: song(artist, title)?,
						played_at: bbc_time(&s.offset.label, now),
					})
				})
				.collect(),
		)
	}

	async fn kexp(&self, client: &reqwest::Client) -> Option<Vec<HistoryItem>> {
		// Without the order it can answer a stale cached list.
		let url = format!(
			"{}/v2/plays/?limit={KEXP_ROWS}&ordering=-airdate",
			self.kexp.trim_end_matches('/')
		);
		let plays: KexpPlays = get_json(client, &url).await.ok()?;
		Some(
			plays
				.results
				.iter()
				.filter(|p| p.play_type == "trackplay")
				.filter_map(|p| {
					let artist = p.artist.as_deref().unwrap_or("");
					Some(HistoryItem {
						title: song(artist, p.song.as_deref()?)?,
						played_at: parse_time(&p.airdate),
					})
				})
				.collect(),
		)
	}

	async fn radio_paradise(&self, client: &reqwest::Client, channel: u32) -> Option<Vec<HistoryItem>> {
		let url = format!(
			"{}/api/nowplaying_list_v2022?chan={channel}&list_num={RADIO_PARADISE_ROWS}",
			self.radio_paradise.trim_end_matches('/')
		);
		let list: ParadiseList = get_json(client, &url).await.ok()?;
		Some(
			list.song
				.iter()
				.filter_map(|s| {
					Some(HistoryItem {
						title: song(&s.artist, &s.title)?,
						played_at: s.play_time.map(at_millis),
					})
				})
				.collect(),
		)
	}
}

/// On the station's own host, which is already serving its audio.
async fn azuracast(client: &reqwest::Client, base: &str, code: &str) -> Option<Vec<HistoryItem>> {
	let now: AzuraNow = get_json(client, &format!("{base}/api/nowplaying/{code}")).await.ok()?;
	Some(
		now.now_playing
			.into_iter()
			.chain(now.song_history)
			.filter_map(|p| {
				let title = song(&p.song.artist, &p.song.title)
					.or_else(|| song("", &p.song.text))?;
				Some(HistoryItem {
					title,
					played_at: at_seconds(p.played_at),
				})
			})
			.collect(),
	)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn a_stream_address_names_its_provider() {
		let triton = |m: &str| Some(Provider::Triton(m.into()));
		for (url, want) in [
			(
				"https://playerservices.streamtheworld.com/api/livestream-redirect/977_HITS_SC.mp3",
				triton("977_HITS_SC"),
			),
			("http://17883.live.streamtheworld.com:3690/RADIO538.mp3?x=1", triton("RADIO538")),
			(
				"https://streaming.live365.com/a62479",
				Some(Provider::Live365("a62479".into())),
			),
			(
				"https://eins.stream.laut.fm/eins",
				Some(Provider::LautFm("eins".into())),
			),
			(
				"https://stream.laut.fm/teatime",
				Some(Provider::LautFm("teatime".into())),
			),
			(
				"https://azuracast.whff.radio/listen/my_whff_hip-hop/radio.mp3",
				Some(Provider::AzuraCast {
					base: "https://azuracast.whff.radio".into(),
					code: "my_whff_hip-hop".into(),
				}),
			),
			(
				"http://as-hls-ww-live.akamaized.net/pool_01505109/live/ww/bbc_radio_one/bbc_radio_one.isml/x.m3u8",
				Some(Provider::Bbc("bbc_radio_one".into())),
			),
			(
				"https://a.files.bbci.co.uk/ms6/live/x/audio/simulcast/hls/nonuk/v1/aks/bbc_1xtra.m3u8",
				Some(Provider::Bbc("bbc_1xtra".into())),
			),
			("https://kexp-mp3-128.streamguys1.com/kexp128.mp3", Some(Provider::Kexp)),
			(
				"https://stream.radioparadise.com/mellow-128",
				Some(Provider::RadioParadise(1)),
			),
			(
				"https://stream.radioparadise.com/radio2050-128",
				Some(Provider::RadioParadise(2050)),
			),
			("https://stream.radioparadise.com/kfat-128", None),
			("https://example.com/listen/live", None),
			("https://example.com/stream.mp3", None),
			("not a url", None),
		] {
			assert_eq!(provider(url), want, "{url}");
		}
	}

	#[test]
	fn triton_tries_the_plain_mount_last() {
		assert_eq!(triton_mounts("977_HITSAAC_SC"), ["977_HITSAAC_SC", "977_HITSAAC", "977_HITS"]);
		assert_eq!(triton_mounts("RADIO538"), ["RADIO538"]);
	}

	#[test]
	fn times_read_every_published_form() {
		// 2026-11-04 08:05:16 UTC.
		let want = UNIX_EPOCH + Duration::from_secs(1_793_779_516);
		assert_eq!(parse_time("2026-11-04 08:05:16.45+00:00"), Some(want));
		assert_eq!(parse_time("2026-11-04 09:05:16 +0100"), Some(want));
		assert_eq!(parse_time("2026-11-04T00:05:16-08:00"), Some(want));
		assert_eq!(parse_time("2026-11-04 08:05:16"), Some(want));
		assert_eq!(parse_time("soon"), None);
		assert_eq!(parse_time(""), None);
	}

	#[test]
	fn bbc_phrases_become_times() {
		let now = UNIX_EPOCH + Duration::from_secs(10_000);
		assert_eq!(bbc_time("Now Playing", now), Some(now));
		assert_eq!(bbc_time("1 Minute Ago", now), Some(now - Duration::from_secs(60)));
		assert_eq!(bbc_time("3 Hours Ago", now), Some(now - Duration::from_secs(10_800)));
		assert_eq!(bbc_time("Yesterday", now), None);
	}

	#[test]
	fn triton_rows_read_and_skip_a_row_with_no_title() {
		let xml = br#"<?xml version="1.0"?><nowplaying-info-list>
<nowplaying-info type="track"><property name="cue_time_start"><![CDATA[1793779693107]]></property><property name="cue_title"><![CDATA[Tastes So Good]]></property><property name="track_artist_name"><![CDATA[Niall & Co]]></property></nowplaying-info>
<nowplaying-info type="track"><property name="cue_time_start"><![CDATA[5]]></property></nowplaying-info>
</nowplaying-info-list>"#;
		let items = triton_items(xml).unwrap();
		assert_eq!(items.len(), 1);
		assert_eq!(items[0].title, "Niall & Co - Tastes So Good");
		assert_eq!(items[0].played_at, Some(at_millis(1_793_779_693_107)));
		assert!(triton_items(b"<nowplaying-info-list/>").unwrap().is_empty());
	}
}
