//! Station directories and the browse tree they share.

use crate::Result;
use crate::station::{Source, Station};
use serde::Deserialize;
use std::collections::HashSet;
use std::time::{Duration, UNIX_EPOCH};

#[derive(Clone, Debug, PartialEq)]
pub enum Node {
	Folder { id: String, name: String },
	Station(Station),
	LoadMore { id: String },
}

pub mod iheart;
pub mod networks;
pub mod radio_browser;
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
}

impl Default for Endpoints {
	fn default() -> Self {
		Endpoints {
			radio_browser: "https://de1.api.radio-browser.info".into(),
			tunein: "https://opml.radiotime.com".into(),
			iheart: "https://api.iheart.com".into(),
			somafm: "https://somafm.com".into(),
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

/// All four directories behind one browse tree and one search.
pub struct Sources {
	client: reqwest::Client,
	rb: radio_browser::RadioBrowser,
	tunein: tunein::TuneIn,
	iheart: iheart::IHeart,
	somafm_base: String,
}

const SEARCH_TIMEOUT: Duration = Duration::from_secs(8);

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
			rb: radio_browser::RadioBrowser::new(endpoints.radio_browser),
			tunein: tunein::TuneIn::new(endpoints.tunein),
			iheart: iheart::IHeart::new(endpoints.iheart),
			somafm_base: endpoints.somafm,
		}
	}

	pub fn roots() -> Vec<Node> {
		[
			("rb", "Radio Browser"),
			("tunein", "TuneIn"),
			("iheart", "iHeartRadio"),
			("net", "Networks"),
		]
		.map(|(id, name)| Node::Folder {
			id: id.into(),
			name: name.into(),
		})
		.into()
	}

	/// Children of a browse folder. `sort` orders Radio Browser station lists; the
	/// other directories have no ordering and ignore it.
	pub async fn children(&self, id: &str, sort: Sort) -> Result<Vec<Node>> {
		let head = id.split(['/', '@']).next().unwrap_or("");
		match head {
			"rb" => self.rb.children(&self.client, id, sort).await,
			"tunein" => self.tunein.children(&self.client, id).await,
			"iheart" => self.iheart.children(&self.client, id).await,
			"net" => Ok(networks::children(id)),
			_ => Ok(vec![]),
		}
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
		let (rb, tunein, iheart) = tokio::join!(
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
		);
		let mut seen = HashSet::new();
		rb.into_iter()
			.chain(tunein)
			.chain(iheart)
			.chain(networks::search(q))
			.filter(|s| seen.insert(s.url.clone()))
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

	/// Published song history, newest first; `None` when the station has none or it failed.
	pub async fn history(&self, station: &Station) -> Option<Vec<HistoryItem>> {
		// Dedupe keeps the first copy of a stream, so look up by URL as well as by id.
		let ih = match &station.source {
			Source::IHeart => Some(station.id.clone()),
			_ => iheart::id_from_url(&station.url),
		};
		if let Some(id) = ih {
			return self.iheart.history(&self.client, &id).await.ok();
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
}
