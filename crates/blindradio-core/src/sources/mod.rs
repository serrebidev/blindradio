//! Station directories and the browse tree they share.

use crate::Result;
use crate::library::Folder;
use crate::station::{Source, Station};
use serde::Deserialize;
use std::collections::HashSet;
use std::time::{Duration, UNIX_EPOCH};
use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

type TreeFuture<'a> = std::pin::Pin<
	Box<dyn std::future::Future<Output = Result<(Vec<Station>, Vec<Folder>)>> + Send + 'a>,
>;

// A short-lived list row; boxing Station would touch every match for no measured gain.
#[expect(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq)]
pub enum Node {
	Folder { id: String, name: String },
	Station(Station),
	LoadMore { id: String },
}

impl Node {
	/// The row's display name; a Load More row has none.
	pub fn name(&self) -> &str {
		match self {
			Node::Folder { name, .. } => name,
			Node::Station(s) => &s.name,
			Node::LoadMore { .. } => "",
		}
	}
}

pub mod directory;
pub mod icecast;
pub mod iheart;
pub mod internet_radio;
pub mod iprd;
pub mod live365;
pub mod networks;
pub mod orb;
mod played;
pub mod radio_browser;
pub mod radionet;
pub mod shoutcast;
pub mod tunein;

/// What the Search window sends to every directory. Empty `name` and unset filters are ignored.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SearchQuery {
	pub name: String,
	pub genre: Option<String>,
	pub country: Option<String>,
	pub language: Option<String>,
	pub sort: Sort,
}

/// One Radio Browser `order` value for station lists. `Default` sends no order at all,
/// which leaves the API's own default (name, A to Z). Only Radio Browser supports
/// ordering; the other directories ignore a sort.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SortField {
	Default,
	Name,
	Votes,
	ClickCount,
	ClickTrend,
	Bitrate,
	Codec,
	Country,
	State,
	Language,
	Tags,
	Url,
	Homepage,
	Favicon,
	LastCheckOk,
	LastCheckTime,
	ClickTimestamp,
	ChangeTimestamp,
	Random,
}

impl SortField {
	/// Every field, in the order a sort control lists them.
	pub const ALL: [SortField; 19] = [
		SortField::Default,
		SortField::Name,
		SortField::Votes,
		SortField::ClickCount,
		SortField::ClickTrend,
		SortField::Bitrate,
		SortField::Codec,
		SortField::Country,
		SortField::State,
		SortField::Language,
		SortField::Tags,
		SortField::Url,
		SortField::Homepage,
		SortField::Favicon,
		SortField::LastCheckOk,
		SortField::LastCheckTime,
		SortField::ClickTimestamp,
		SortField::ChangeTimestamp,
		SortField::Random,
	];

	/// Short label for a sort control.
	pub fn label(self) -> &'static str {
		match self {
			SortField::Default => "Default",
			SortField::Name => "Name",
			SortField::Votes => "Votes",
			SortField::ClickCount => "Clicks",
			SortField::ClickTrend => "Click trend",
			SortField::Bitrate => "Bitrate",
			SortField::Codec => "Codec",
			SortField::Country => "Country",
			SortField::State => "State",
			SortField::Language => "Language",
			SortField::Tags => "Tags",
			SortField::Url => "URL",
			SortField::Homepage => "Homepage",
			SortField::Favicon => "Favicon",
			SortField::LastCheckOk => "Last check OK",
			SortField::LastCheckTime => "Last check time",
			SortField::ClickTimestamp => "Last clicked",
			SortField::ChangeTimestamp => "Last changed",
			SortField::Random => "Random",
		}
	}

	/// The API value; empty for `Default`.
	fn value(self) -> &'static str {
		match self {
			SortField::Default => "",
			SortField::Name => "name",
			SortField::Votes => "votes",
			SortField::ClickCount => "clickcount",
			SortField::ClickTrend => "clicktrend",
			SortField::Bitrate => "bitrate",
			SortField::Codec => "codec",
			SortField::Country => "country",
			SortField::State => "state",
			SortField::Language => "language",
			SortField::Tags => "tags",
			SortField::Url => "url",
			SortField::Homepage => "homepage",
			SortField::Favicon => "favicon",
			SortField::LastCheckOk => "lastcheckok",
			SortField::LastCheckTime => "lastchecktime",
			SortField::ClickTimestamp => "clicktimestamp",
			SortField::ChangeTimestamp => "changetimestamp",
			SortField::Random => "random",
		}
	}
}

/// Station list order: a field plus the API's `reverse` direction. The default is the
/// order browse and search have always used, most plays first.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sort {
	pub field: SortField,
	pub reverse: bool,
}

impl Default for Sort {
	fn default() -> Self {
		Sort {
			field: SortField::ClickCount,
			reverse: true,
		}
	}
}

impl Sort {
	/// The `order`/`reverse` query fragment; empty when the API default applies.
	pub fn query(self) -> String {
		let value = self.field.value();
		if value.is_empty() {
			String::new()
		} else {
			format!("order={value}&reverse={}", self.reverse)
		}
	}
}

/// What a directory can order its stations by. A value the directory does not
/// publish stays zero or empty.
#[derive(Default)]
pub struct Sortable {
	pub name: String,
	pub bitrate: u32,
	pub codec: String,
	pub tags: String,
	pub url: String,
	/// Plays or listeners, where the directory publishes one.
	pub popularity: u64,
}

/// One row's value under the chosen field; `None` when the directory does not
/// publish that field at all.
enum Key {
	Text(String),
	Num(u64),
}

impl PartialEq for Key {
	fn eq(&self, other: &Self) -> bool {
		match (self, other) {
			(Key::Text(a), Key::Text(b)) => a == b,
			(Key::Num(a), Key::Num(b)) => a == b,
			_ => false,
		}
	}
}

impl Eq for Key {}

impl PartialOrd for Key {
	fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
		Some(self.cmp(other))
	}
}

impl Ord for Key {
	fn cmp(&self, other: &Self) -> std::cmp::Ordering {
		match (self, other) {
			(Key::Text(a), Key::Text(b)) => a.cmp(b),
			(Key::Num(a), Key::Num(b)) => a.cmp(b),
			(Key::Text(_), Key::Num(_)) => std::cmp::Ordering::Less,
			(Key::Num(_), Key::Text(_)) => std::cmp::Ordering::Greater,
		}
	}
}

fn key_of(row: &Sortable, field: SortField) -> Option<Key> {
	Some(match field {
		SortField::Name => Key::Text(row.name.to_lowercase()),
		SortField::Bitrate => Key::Num(row.bitrate.into()),
		SortField::Codec => Key::Text(row.codec.to_lowercase()),
		SortField::Tags => Key::Text(row.tags.to_lowercase()),
		SortField::Url => Key::Text(row.url.to_lowercase()),
		// The closest thing a directory other than Radio Browser publishes.
		SortField::Votes | SortField::ClickCount | SortField::ClickTrend => {
			Key::Num(row.popularity)
		}
		_ => return None,
	})
}

/// Orders rows by the chosen field, in place. A field the directory does not publish,
/// or one where every row is equal, leaves the provider's order alone, `Reverse`
/// included. `Default` is the provider's own order.
pub fn sort_rows(rows: &mut [(Sortable, Station)], sort: Sort) {
	if sort.field == SortField::Random {
		// A cheap pseudo-random key per row; the seed makes each request differ.
		let mut seed = std::time::SystemTime::now()
			.duration_since(std::time::UNIX_EPOCH)
			.map_or(1, |d| d.as_nanos() as u64);
		rows.sort_by_cached_key(|_| {
			seed = seed
				.wrapping_mul(6364136223846793005)
				.wrapping_add(1442695040888963407);
			seed >> 33
		});
		return;
	}
	let Some(first) = rows.first().and_then(|(row, _)| key_of(row, sort.field)) else {
		return;
	};
	if !rows
		.iter()
		.any(|(row, _)| key_of(row, sort.field).is_some_and(|key| key != first))
	{
		return;
	}
	rows.sort_by_cached_key(|(row, _)| key_of(row, sort.field).unwrap_or(Key::Num(0)));
	if sort.reverse {
		rows.reverse();
	}
}

/// One entry of a station's published track history.
#[derive(Clone, Debug, PartialEq)]
pub struct HistoryItem {
	pub title: String,
	pub played_at: Option<std::time::SystemTime>,
}

/// Base URLs of the directories; tests point them at a local server.
#[derive(Clone, Debug)]
pub struct Endpoints {
	pub radio_browser: String,
	pub tunein: String,
	pub iheart: String,
	pub somafm: String,
	pub icecast: String,
	pub shoutcast: String,
	pub internet_radio: String,
	pub iprd: String,
	pub radionet: String,
	pub live365: String,
	pub orb: String,
	pub directory: String,
	pub recommended: String,
	pub triton: String,
	pub live365_api: String,
	pub laut_fm: String,
	pub bbc: String,
	pub kexp: String,
	pub radio_paradise: String,
}

impl Default for Endpoints {
	fn default() -> Self {
		Endpoints {
			radio_browser: "https://de1.api.radio-browser.info".into(),
			tunein: "https://opml.radiotime.com".into(),
			iheart: "https://api.iheart.com".into(),
			somafm: "https://somafm.com".into(),
			icecast: "https://dir.xiph.org".into(),
			shoutcast: "https://directory.shoutcast.com".into(),
			internet_radio: "https://www.internet-radio.com".into(),
			iprd: "https://iprd-org.github.io/iprd".into(),
			radionet: "https://prod.radio-api.net".into(),
			live365: "https://live365.com".into(),
			orb: "https://onlineradiobox.com".into(),
			directory: "https://api.github.com/repos/serrebidev/BlindRadio-directory".into(),
			recommended: "https://api.github.com/repos/deroverda/recommended-radio-streams".into(),
			triton: "https://np.tritondigital.com".into(),
			live365_api: "https://api.live365.com".into(),
			laut_fm: "https://api.laut.fm".into(),
			bbc: "https://rms.api.bbc.co.uk".into(),
			kexp: "https://api.kexp.org".into(),
			radio_paradise: "https://api.radioparadise.com".into(),
		}
	}
}

/// Values for the Search window's filter lists.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Filters {
	pub genres: Vec<String>,
	pub countries: Vec<String>,
	pub languages: Vec<String>,
}

/// All directories behind one browse tree and one search.
pub struct Sources {
	client: reqwest::Client,
	rb: radio_browser::RadioBrowser,
	tunein: tunein::TuneIn,
	iheart: iheart::IHeart,
	icecast: icecast::Icecast,
	shoutcast: shoutcast::Shoutcast,
	internet_radio: internet_radio::InternetRadio,
	iprd: iprd::Iprd,
	radionet: radionet::RadioNet,
	live365: live365::Live365,
	orb: orb::Orb,
	played: played::Played,
	directory: directory::Directory,
	recommended: directory::Directory,
	somafm_base: String,
}

const SEARCH_TIMEOUT: Duration = Duration::from_secs(8);

/// Lowercase letters and digits only, accents gone, with the offsets where words begin.
/// The end of the text counts as an edge too.
fn fold(text: &str) -> (String, Vec<usize>) {
	let mut folded = String::new();
	let mut edges = Vec::new();
	let mut previous = None::<char>;
	for c in text.nfd().filter(|c| !is_combining_mark(*c)) {
		if c.is_alphanumeric()
			&& previous.is_none_or(|p| !p.is_alphanumeric() || (p.is_lowercase() && c.is_uppercase()))
		{
			edges.push(folded.len());
		}
		if c.is_alphanumeric() {
			folded.extend(c.to_lowercase());
		}
		previous = Some(c);
	}
	edges.push(folded.len());
	(folded, edges)
}

/// Whether `text` holds every word of `query`. A word has to touch a word edge there, so
/// "cknw" finds "730 CKNW" but not "BlacknWhita". Spaces and case do not count, so
/// "serrebiradio" finds "Serrebi Radio" and "fm" finds "KISSFM". Scripts written without
/// spaces have no edges to touch: any place in the text will do.
/// ponytail: letters only, "radio 1" misses "Radio One"; add synonyms if that bites.
fn holds_words(text: &str, query: &str) -> bool {
	let (text, edges) = fold(text);
	query.split_whitespace().all(|word| {
		let word = fold(word).0;
		text.match_indices(&word).any(|(at, _)| {
			!word.is_ascii() || edges.contains(&at) || edges.contains(&(at + word.len()))
		}) || word.is_empty()
	})
}

/// Directories answer loosely: radio.net and iHeart by spelling distance, TuneIn by sound,
/// others by the song playing now. Only name, genre and call sign count as a match.
fn answers(station: &Station, name: &str) -> bool {
	let details = &station.details;
	let text = [
		Some(station.name.as_str()),
		details.genre.as_deref(),
		details.call_sign.as_deref(),
	]
	.into_iter()
	.flatten()
	.collect::<Vec<_>>()
	.join(" ");
	holds_words(&text, name)
}

/// `own` then the rows of `older` from before it. A list without times cannot be joined.
fn older_rows_follow(
	own: Option<Vec<HistoryItem>>,
	older: Option<Vec<HistoryItem>>,
) -> Option<Vec<HistoryItem>> {
	let Some(mut own) = own.filter(|o| !o.is_empty()) else {
		return older;
	};
	if let (Some(oldest), Some(older)) = (own.last().and_then(|i| i.played_at), older) {
		own.extend(older.into_iter().filter(|i| i.played_at.is_some_and(|at| at < oldest)));
	}
	Some(own)
}

#[derive(Deserialize)]
struct SomaSongs {
	#[serde(default)]
	songs: Vec<SomaSong>,
}

#[derive(Deserialize)]
struct SomaSong {
	#[serde(default)]
	title: String,
	#[serde(default)]
	artist: String,
	#[serde(default)]
	date: String,
}

impl Sources {
	pub fn new(client: reqwest::Client, endpoints: Endpoints) -> Sources {
		Sources {
			client,
			played: played::Played::new(&endpoints),
			orb: orb::Orb::new(endpoints.orb),
			rb: radio_browser::RadioBrowser::new(endpoints.radio_browser),
			tunein: tunein::TuneIn::new(endpoints.tunein),
			iheart: iheart::IHeart::new(endpoints.iheart),
			icecast: icecast::Icecast::new(endpoints.icecast),
			shoutcast: shoutcast::Shoutcast::new(endpoints.shoutcast),
			internet_radio: internet_radio::InternetRadio::new(endpoints.internet_radio),
			iprd: iprd::Iprd::new(endpoints.iprd),
			radionet: radionet::RadioNet::new(endpoints.radionet),
			live365: live365::Live365::new(endpoints.live365),
			directory: directory::Directory::new(endpoints.directory),
			recommended: directory::Directory::recommended(endpoints.recommended),
			somafm_base: endpoints.somafm,
		}
	}

	pub fn roots() -> Vec<Node> {
		[("dir", "Directories"), ("net", "Networks")]
			.map(|(id, name)| Node::Folder {
				id: id.into(),
				name: name.into(),
			})
			.into()
	}

	/// The station directories, the children of the `dir` root, in name order.
	fn directories() -> Vec<Node> {
		let mut dirs: Vec<Node> = [
			("rb", "Radio Browser"),
			("tunein", "TuneIn"),
			("iheart", "iHeartRadio"),
			("icecast", "Icecast"),
			("shoutcast", "SHOUTcast"),
			("internet-radio", "Internet-Radio"),
			("iprd", "IPRD"),
			("radionet", "Radio.net"),
			("live365", "Live365"),
			("orb", "Online Radio Box"),
			("brdir", "BlindRadio Directory"),
			("recommended", "Recommended Radio Streams"),
		]
		.map(|(id, name)| Node::Folder {
			id: id.into(),
			name: name.into(),
		})
		.into();
		dirs.sort_by_key(|n| n.name().to_lowercase());
		dirs
	}

	/// Children of a browse folder. `sort` orders Radio Browser station lists; the
	/// other directories have no ordering and ignore it.
	pub async fn children(&self, id: &str, sort: Sort) -> Result<Vec<Node>> {
		let head = id.split(['/', '@']).next().unwrap_or("");
		match head {
			"dir" => Ok(Self::directories()),
			"rb" => self.rb.children(&self.client, id, sort).await,
			"tunein" => self.tunein.children(&self.client, id).await,
			"iheart" => self.iheart.children(&self.client, id).await,
			"icecast" => self.icecast.children(&self.client, id, sort).await,
			"shoutcast" => self.shoutcast.children(&self.client, id, sort).await,
			"internet-radio" => self.internet_radio.children(&self.client, id).await,
			"iprd" => self.iprd.children(&self.client, id, sort).await,
			"radionet" => self.radionet.children(&self.client, id).await,
			"live365" => self.live365.children(&self.client, id, sort).await,
			"orb" => self.orb.children(&self.client, id).await,
			"brdir" => self.directory.children(&self.client, id).await,
			"recommended" => self.recommended.children(&self.client, id).await,
			"net" => Ok(networks::children(id)),
			_ => Ok(vec![]),
		}
	}

	/// The whole subtree under `id` as export material: the level's
	/// own stations, and its folders as library folders with their
	/// subtrees inside, so an export keeps the structure instead of
	/// flattening every station into one group named after the root.
	/// Load More rows are followed to the end of their list. A folder
	/// id already visited is not walked again, so a looping source
	/// cannot spin forever.
	pub async fn tree(&self, id: &str, sort: Sort) -> Result<(Vec<Station>, Vec<Folder>)> {
		let mut seen = HashSet::new();
		seen.insert(id.to_string());
		self.tree_walk(id, sort, &mut seen).await
	}

	fn tree_walk<'a>(
		&'a self,
		id: &'a str,
		sort: Sort,
		seen: &'a mut HashSet<String>,
	) -> TreeFuture<'a> {
		Box::pin(async move {
			let mut stations = Vec::new();
			let mut folders = Vec::new();
			let mut level = vec![id.to_string()];
			// A directory's pages can overlap.
			let mut station_ids = HashSet::new();
			while let Some(next) = level.pop() {
				for node in self.children(&next, sort).await? {
					match node {
						Node::Station(s) => {
							if station_ids.insert(s.id.clone()) {
								stations.push(s);
							}
						}
						Node::Folder { id, name } => {
							if seen.insert(id.clone()) {
								let (inner_s, inner_f) = self.tree_walk(&id, sort, seen).await?;
								folders.push(Folder {
									name,
									stations: inner_s,
									folders: inner_f,
								});
							}
						}
						Node::LoadMore { id } => level.push(id),
					}
				}
			}
			Ok((stations, folders))
		})
	}

	/// A directory that fails or is slow is left out. Without a name only Radio Browser can
	/// apply the filters; the others would return unrelated stations.
	pub async fn search(&self, q: &SearchQuery) -> Vec<Station> {
		async fn bounded(f: impl Future<Output = Result<Vec<Station>>>) -> Vec<Station> {
			tokio::time::timeout(SEARCH_TIMEOUT, f)
				.await
				.ok()
				.and_then(|r| r.ok())
				.unwrap_or_default()
		}
		let name_given = !q.name.trim().is_empty();
		let (rb, tunein, iheart, icecast, shoutcast, internet_radio, iprd, radionet, live365, orb) = tokio::join!(
			bounded(self.rb.search(&self.client, q)),
			async {
				if name_given {
					bounded(self.tunein.search(&self.client, q)).await
				} else {
					vec![]
				}
			},
			async {
				if name_given {
					bounded(self.iheart.search(&self.client, q)).await
				} else {
					vec![]
				}
			},
			async {
				if name_given {
					bounded(self.icecast.search(&self.client, q)).await
				} else {
					vec![]
				}
			},
			async {
				if name_given {
					bounded(self.shoutcast.search(&self.client, q)).await
				} else {
					vec![]
				}
			},
			async {
				if name_given {
					bounded(self.internet_radio.search(&self.client, q)).await
				} else {
					vec![]
				}
			},
			async {
				if name_given {
					bounded(self.iprd.search(&self.client, q)).await
				} else {
					vec![]
				}
			},
			async {
				if name_given {
					bounded(self.radionet.search(&self.client, q)).await
				} else {
					vec![]
				}
			},
			async {
				if name_given {
					bounded(self.live365.search(&self.client, q)).await
				} else {
					vec![]
				}
			},
			async {
				if name_given {
					bounded(self.orb.search(&self.client, q)).await
				} else {
					vec![]
				}
			},
		);
		let mut seen = HashSet::new();
		rb.into_iter()
			.chain(tunein)
			.chain(iheart)
			.chain(icecast)
			.chain(shoutcast)
			.chain(internet_radio)
			.chain(iprd)
			.chain(radionet)
			.chain(live365)
			.chain(orb)
			.chain(networks::search(q))
			.filter(|s| answers(s, &q.name) && seen.insert(s.url.clone()))
			.collect()
	}

	pub async fn filters(&self) -> Filters {
		let (genres, countries, languages) = tokio::join!(
			self.rb.genres(&self.client),
			self.rb.countries(&self.client),
			self.rb.languages(&self.client),
		);
		Filters {
			genres: genres.unwrap_or_default(),
			countries: countries.unwrap_or_default(),
			languages: languages.unwrap_or_default(),
		}
	}

	/// Counts a play with the directory that wants it: only Radio Browser does.
	pub async fn report_play(&self, station_id: &str) -> Result<()> {
		match station_id.strip_prefix("rb:") {
			Some(uuid) => self.rb.report_play(&self.client, uuid).await,
			None => Ok(()),
		}
	}

	/// What Station Info can add from TuneIn, by guide id.
	pub async fn tunein_details(&self, guide_id: &str) -> Result<crate::station::Details> {
		self.tunein.describe(&self.client, guide_id).await
	}

	/// Published song history, newest first; `None` when the station has none or it failed.
	/// The station's own list is the most exact and covers hours. Online Radio Box carries on
	/// from there for the rest of the week.
	pub async fn history(&self, station: &Station) -> Option<Vec<HistoryItem>> {
		let (own, week) = tokio::join!(
			self.own_history(station),
			tokio::time::timeout(SEARCH_TIMEOUT, self.orb.history(&self.client, station)),
		);
		older_rows_follow(own, week.ok().flatten())
	}

	async fn own_history(&self, station: &Station) -> Option<Vec<HistoryItem>> {
		// Dedupe keeps the first copy of a stream, so look up by URL as well as by id.
		let ih = match &station.source {
			Source::IHeart => Some(station.id.clone()),
			_ => iheart::id_from_url(&station.url),
		};
		if let Some(id) = ih {
			return self.iheart.history(&self.client, &id).await.ok();
		}
		if let Some(items) = self.played.history(&self.client, &station.url).await {
			return Some(items);
		}
		let key = networks::networks()
			.iter()
			.flat_map(|n| n.stations.iter().map(move |s| (n, s)))
			.find(|(n, s)| {
				station.id == format!("net:{}:{}", n.slug, s.slug) || station.url == s.url
			})
			.and_then(|(_, s)| s.history.as_deref())?;
		let soma = key.strip_prefix("somafm:")?;
		let url = format!(
			"{}/songs/{soma}.json",
			self.somafm_base.trim_end_matches('/')
		);
		let songs: SomaSongs = crate::net::get_json(&self.client, &url).await.ok()?;
		Some(
			songs
				.songs
				.into_iter()
				.filter(|s| !s.title.trim().is_empty())
				.map(|s| {
					let (title, artist) = (s.title.trim(), s.artist.trim());
					HistoryItem {
						title: if artist.is_empty() {
							title.to_owned()
						} else {
							format!("{artist} - {title}")
						},
						played_at: s
							.date
							.parse()
							.ok()
							.map(|t| UNIX_EPOCH + Duration::from_secs(t)),
					}
				})
				.collect(),
		)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn holds_words_keeps_real_matches_and_drops_loose_ones() {
		for (text, query) in [
			("730 CKNW", "cknw"),
			("Global News Radio 980 CKNW", "CKNW"),
			("SerrebiRadio HD Opus", "serrebiradio"),
			("Serrebi Radio", "serrebiradio"),
			("KISSFM", "fm"),
			("KISS-FM", "kiss fm"),
			("Rádio Comercial", "radio"),
			("Radio Comercial", "rádio"),
			("R&B Soul", "r&b"),
			("93.9 WNYC", "wnyc"),
			("Z100 WHTZ-FM", "whtz"),
			("日本ラジオ局", "ラジオ"),
			("anything", ""),
			("anything", "!!"),
		] {
			assert!(holds_words(text, query), "{text} should hold {query}");
		}
		for (text, query) in [
			("CKNB 95", "cknw"),
			("KFI AM 640", "cknw"),
			("BlacknWhita", "cknw"),
			("Country Music Radio - Conway Twitty", "cknw"),
			("CerebroDigital", "serrebiradio"),
			("Radio Surprise The Live DJ Station", "serrebiradio"),
			("Paradise FM", "radio paradise"),
			("BBC Radio 2", "bbc radio 1"),
			("Z104.3", "z100"),
			("WNCI 97.9", "wnyc"),
		] {
			assert!(!holds_words(text, query), "{text} should not hold {query}");
		}
	}

	#[test]
	fn sort_query_fragments() {
		assert_eq!(Sort::default().query(), "order=clickcount&reverse=true");
		assert_eq!(
			Sort {
				field: SortField::Default,
				reverse: true
			}
			.query(),
			""
		);
		assert_eq!(
			Sort {
				field: SortField::Name,
				reverse: false
			}
			.query(),
			"order=name&reverse=false"
		);
		assert_eq!(
			Sort {
				field: SortField::Bitrate,
				reverse: true
			}
			.query(),
			"order=bitrate&reverse=true"
		);
	}

	#[test]
	fn sort_fields_have_labels_and_values() {
		assert_eq!(SortField::ALL.len(), 19);
		for f in SortField::ALL {
			assert!(!f.label().is_empty());
		}
		assert_eq!(SortField::ClickCount.value(), "clickcount");
		assert_eq!(SortField::Default.value(), "");
	}

	fn rows() -> Vec<(Sortable, Station)> {
		[
			("Charli", 64, "AAC", "Jazz", "https://x/c", 5),
			("alpha", 192, "MP3", "Rock", "https://x/a", 900),
			("Bravo", 128, "MP3", "Rock", "https://x/b", 50),
		]
		.into_iter()
		.map(|(name, bitrate, codec, tags, url, popularity)| {
			let station = Station {
				id: name.into(),
				name: name.into(),
				url: url.into(),
				source: Source::Custom,
				info: None,
				details: Default::default(),
			};
			let key = Sortable {
				name: name.into(),
				bitrate,
				codec: codec.into(),
				tags: tags.into(),
				url: url.into(),
				popularity,
			};
			(key, station)
		})
		.collect()
	}

	/// The names in the order the sort left them, comma-joined for a readable assert.
	fn sorted(field: SortField, reverse: bool) -> String {
		let mut r = rows();
		sort_rows(&mut r, Sort { field, reverse });
		r.iter()
			.map(|(_, s)| s.name.as_str())
			.collect::<Vec<_>>()
			.join(",")
	}

	#[test]
	fn sort_rows_orders_every_published_field() {
		assert_eq!(sorted(SortField::Name, false), "alpha,Bravo,Charli");
		assert_eq!(sorted(SortField::Name, true), "Charli,Bravo,alpha");
		assert_eq!(sorted(SortField::Bitrate, false), "Charli,Bravo,alpha");
		assert_eq!(sorted(SortField::Bitrate, true), "alpha,Bravo,Charli");
		assert_eq!(sorted(SortField::Codec, false), "Charli,alpha,Bravo");
		assert_eq!(sorted(SortField::Codec, true), "Bravo,alpha,Charli");
		assert_eq!(sorted(SortField::Tags, false), "Charli,alpha,Bravo");
		assert_eq!(sorted(SortField::Tags, true), "Bravo,alpha,Charli");
		assert_eq!(sorted(SortField::Url, false), "alpha,Bravo,Charli");
		assert_eq!(sorted(SortField::ClickTrend, true), "alpha,Bravo,Charli");
		assert_eq!(sorted(SortField::Votes, false), "Charli,Bravo,alpha");
		assert_eq!(sorted(SortField::ClickCount, true), "alpha,Bravo,Charli");
	}

	#[test]
	fn sort_rows_leaves_unpublished_fields_in_place() {
		for field in [
			SortField::Default,
			SortField::Country,
			SortField::State,
			SortField::Language,
			SortField::Homepage,
			SortField::Favicon,
			SortField::LastCheckOk,
			SortField::LastCheckTime,
			SortField::ClickTimestamp,
			SortField::ChangeTimestamp,
		] {
			assert_eq!(sorted(field, false), "Charli,alpha,Bravo", "{field:?}");
			assert_eq!(sorted(field, true), "Charli,alpha,Bravo", "{field:?}");
		}
	}

	#[test]
	fn random_keeps_every_row() {
		let mut r = rows();
		sort_rows(
			&mut r,
			Sort {
				field: SortField::Random,
				reverse: false,
			},
		);
		let mut names: Vec<_> = r.iter().map(|(_, s)| s.name.as_str()).collect();
		names.sort_unstable();
		assert_eq!(names, ["Bravo", "Charli", "alpha"]);
	}
}
