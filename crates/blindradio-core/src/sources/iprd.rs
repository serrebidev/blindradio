//! International Public Radio Directory: one catalog file, shared by browse and search.
//!
//! ponytail: the whole catalog, about 24,000 stations, stays in memory once opened, as
//! Icecast's directory does. Upgrade path: `by_country/*.m3u`, which loses genres and search.

use super::{Node, SearchQuery, Sort, Sortable, sort_rows};
use crate::station::{Details, Source, Station, known};
use crate::{CoreError, Result};
use percent_encoding::{NON_ALPHANUMERIC, percent_decode_str, utf8_percent_encode};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

const PAGE: usize = 100;
/// Genres offered for browsing, the most used; the catalog has over a thousand.
const GENRES: usize = 100;
const SEARCH_STATIONS: usize = 100;
const MAX_BODY: usize = 64 * 1024 * 1024;
/// The file is 2 MB gzipped, more than the usual 8 s allows on a slow line.
const FETCH_DEADLINE: Duration = Duration::from_secs(30);
/// The catalog changes about weekly.
const CACHE_AGE: Duration = Duration::from_secs(60 * 60);

pub struct Iprd {
	base: String,
	cache: Mutex<Option<(Instant, Arc<Vec<Entry>>)>>,
}

struct Entry {
	station: Station,
	country: String,
	/// Lowercase: the catalog has both `pop` and `Pop`.
	genres: Vec<String>,
	bitrate: u32,
	codec: String,
}

#[derive(Deserialize)]
struct Catalog {
	#[serde(default)]
	stations: Vec<Raw>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Raw {
	id: String,
	name: String,
	country: String,
	language: Vec<String>,
	genres: Vec<String>,
	website: String,
	streams: Vec<RawStream>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawStream {
	url: String,
	format: String,
	bitrate: u32,
}

fn enc(s: &str) -> String {
	utf8_percent_encode(s, NON_ALPHANUMERIC).to_string()
}

fn folder(id: impl Into<String>, name: impl Into<String>) -> Node {
	Node::Folder {
		id: id.into(),
		name: name.into(),
	}
}

fn playable(url: &str) -> Option<String> {
	let url = reqwest::Url::parse(url.trim()).ok()?;
	(matches!(url.scheme(), "http" | "https")
		&& url.host_str().is_some()
		&& url.username().is_empty()
		&& url.password().is_none())
	.then(|| url.into())
}

fn to_entry(raw: Raw) -> Option<Entry> {
	let name = raw.name.trim();
	if name.is_empty() {
		return None;
	}
	let (url, stream) = raw
		.streams
		.iter()
		.find_map(|s| Some((playable(&s.url)?, s)))?;
	let codec = Some(stream.format.trim())
		.filter(|f| !f.eq_ignore_ascii_case("unknown"))
		.unwrap_or_default();
	let rate = if stream.bitrate > 0 {
		format!("{}k", stream.bitrate)
	} else {
		String::new()
	};
	let country = raw.country.trim();
	let genres: Vec<&str> = raw
		.genres
		.iter()
		.map(|g| g.trim())
		.filter(|g| !g.is_empty())
		.collect();
	let shown = genres
		.iter()
		.take(3)
		.copied()
		.collect::<Vec<_>>()
		.join(", ");
	let sound = [codec, rate.as_str()]
		.into_iter()
		.filter(|s| !s.is_empty())
		.collect::<Vec<_>>()
		.join(" ");
	let info = [shown.as_str(), country, sound.as_str()]
		.into_iter()
		.filter(|s| !s.is_empty())
		.collect::<Vec<_>>()
		.join(", ");
	Some(Entry {
		station: Station {
			id: format!("iprd:{}", raw.id.trim()),
			name: name.into(),
			url,
			source: Source::Iprd,
			info: known(&info),
			details: Details {
				genre: known(&shown),
				country: known(country),
				language: known(&raw.language.join(", ")),
				codec: known(codec),
				bitrate_kbps: Some(stream.bitrate).filter(|&b| b > 0),
				homepage: known(&raw.website),
				call_sign: None,
				unavailable: None,
				..Details::default()
			},
		},
		country: country.into(),
		genres: genres.iter().map(|g| g.to_lowercase()).collect(),
		bitrate: stream.bitrate,
		codec: codec.into(),
	})
}

fn entries_of(catalog: Catalog) -> Vec<Entry> {
	let mut seen = HashSet::new();
	catalog
		.stations
		.into_iter()
		.filter_map(to_entry)
		.filter(|e| seen.insert(e.station.url.clone()))
		.collect()
}

/// Name order first, so a sort's ties are alphabetical and `Default` stays alphabetical.
fn rows(entries: &[&Entry]) -> Vec<(Sortable, Station)> {
	let mut rows: Vec<_> = entries
		.iter()
		.map(|e| {
			(
				Sortable {
					name: e.station.name.clone(),
					bitrate: e.bitrate,
					codec: e.codec.clone(),
					tags: e.genres.join(", "),
					url: e.station.url.clone(),
					popularity: 0,
				},
				e.station.clone(),
			)
		})
		.collect();
	rows.sort_by_cached_key(|(k, _)| k.name.to_lowercase());
	rows
}

fn capitalized(genre: &str) -> String {
	let mut chars = genre.chars();
	chars
		.next()
		.map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect())
}

fn country_folders(entries: &[Entry]) -> Vec<Node> {
	let mut countries: Vec<&str> = entries
		.iter()
		.map(|e| e.country.as_str())
		.filter(|c| !c.is_empty())
		.collect::<HashSet<_>>()
		.into_iter()
		.collect();
	countries.sort_by_key(|c| c.to_lowercase());
	countries
		.into_iter()
		.map(|c| folder(format!("iprd/country/{}", enc(c)), c))
		.collect()
}

/// The most used genres, in name order.
fn genre_folders(entries: &[Entry]) -> Vec<Node> {
	let mut counts: HashMap<&str, usize> = HashMap::new();
	for genre in entries.iter().flat_map(|e| &e.genres) {
		*counts.entry(genre).or_default() += 1;
	}
	let mut genres: Vec<(&str, usize)> = counts.into_iter().collect();
	genres.sort_by_key(|&(g, n)| (std::cmp::Reverse(n), g));
	genres.truncate(GENRES);
	genres.sort_by_key(|&(g, _)| g);
	genres
		.into_iter()
		.map(|(g, _)| folder(format!("iprd/genre/{}", enc(g)), capitalized(g)))
		.collect()
}

impl Iprd {
	pub fn new(base: impl Into<String>) -> Self {
		Self {
			base: base.into(),
			cache: Mutex::new(None),
		}
	}

	async fn entries(&self, client: &reqwest::Client) -> Result<Arc<Vec<Entry>>> {
		// Keep the lock through fetching so concurrent browse/search calls share one download.
		let mut cache = self.cache.lock().await;
		if let Some((_, entries)) = cache
			.as_ref()
			.filter(|(fetched, _)| fetched.elapsed() < CACHE_AGE)
		{
			return Ok(entries.clone());
		}
		let url = format!(
			"{}/site_data/metadata/catalog.json",
			self.base.trim_end_matches('/')
		);
		let mut response = client
			.get(&url)
			.header("User-Agent", crate::net::user_agent_for(&url))
			.timeout(FETCH_DEADLINE)
			.send()
			.await
			.map_err(crate::net::http_err)?;
		if !response.status().is_success() {
			return Err(CoreError::Http(format!(
				"HTTP {}",
				response.status().as_u16()
			)));
		}
		let mut body = Vec::new();
		while let Some(chunk) = response.chunk().await.map_err(crate::net::http_err)? {
			if chunk.len() > MAX_BODY - body.len() {
				return Err(CoreError::Http("Directory too large".into()));
			}
			body.extend_from_slice(&chunk);
		}
		let catalog: Catalog = serde_json::from_slice(&body)
			.map_err(|_| CoreError::Decode("Invalid directory".into()))?;
		let entries = Arc::new(entries_of(catalog));
		*cache = Some((Instant::now(), entries.clone()));
		Ok(entries)
	}

	pub async fn children(
		&self,
		client: &reqwest::Client,
		id: &str,
		sort: Sort,
	) -> Result<Vec<Node>> {
		if id == "iprd" {
			return Ok(vec![
				folder("iprd/countries", "By Country"),
				folder("iprd/genres", "By Genre"),
			]);
		}
		let (head, offset) = match id.rsplit_once('@') {
			Some((head, off)) => (head, off.parse::<usize>().unwrap_or(0)),
			None => (id, 0),
		};
		let country = head.strip_prefix("iprd/country/");
		let genre = head.strip_prefix("iprd/genre/");
		if head != "iprd/countries" && head != "iprd/genres" && country.is_none() && genre.is_none()
		{
			return Ok(vec![]);
		}
		let entries = self.entries(client).await?;
		if head == "iprd/countries" {
			return Ok(country_folders(&entries));
		}
		if head == "iprd/genres" {
			return Ok(genre_folders(&entries));
		}
		let country = country.map(|c| percent_decode_str(c).decode_utf8_lossy());
		let genre = genre.map(|g| percent_decode_str(g).decode_utf8_lossy());
		let matching: Vec<&Entry> = entries
			.iter()
			.filter(|e| {
				country.as_ref().is_none_or(|c| e.country == *c)
					&& genre
						.as_ref()
						.is_none_or(|g| e.genres.iter().any(|x| x == g))
			})
			.collect();
		let mut rows = rows(&matching);
		sort_rows(&mut rows, sort);
		let mut nodes: Vec<Node> = rows
			.into_iter()
			.skip(offset)
			.take(PAGE)
			.map(|(_, station)| Node::Station(station))
			.collect();
		if matching.len() > offset + PAGE {
			nodes.push(Node::LoadMore {
				id: format!("{head}@{}", offset + PAGE),
			});
		}
		Ok(nodes)
	}

	/// Name only. The catalog's country names and genres differ from the Search window's lists.
	pub async fn search(
		&self,
		client: &reqwest::Client,
		query: &SearchQuery,
	) -> Result<Vec<Station>> {
		let name = query.name.trim().to_lowercase();
		if name.is_empty() {
			return Ok(vec![]);
		}
		let entries = self.entries(client).await?;
		let matching: Vec<&Entry> = entries
			.iter()
			.filter(|e| e.station.name.to_lowercase().contains(&name))
			.collect();
		let mut rows = rows(&matching);
		sort_rows(&mut rows, query.sort);
		Ok(rows
			.into_iter()
			.take(SEARCH_STATIONS)
			.map(|(_, station)| station)
			.collect())
	}
}
