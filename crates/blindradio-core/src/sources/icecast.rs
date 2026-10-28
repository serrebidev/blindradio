//! Icecast's public XML directory, shared by browse and search.

use super::{Node, SearchQuery};
use crate::station::{Source, Station};
use crate::{CoreError, Result};
use percent_encoding::{NON_ALPHANUMERIC, percent_decode_str, utf8_percent_encode};
use serde::Deserialize;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

const PAGE: usize = 100;
const MAX_BODY: usize = 16 * 1024 * 1024;
const CACHE_AGE: Duration = Duration::from_secs(15 * 60);

pub struct Icecast {
	base: String,
	cache: Mutex<Option<(Instant, Arc<Vec<Entry>>)>>,
}

struct Entry {
	station: Station,
	genre: String,
}

#[derive(Deserialize)]
struct Directory {
	#[serde(default)]
	entry: Vec<RawEntry>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawEntry {
	server_name: String,
	listen_url: String,
	server_type: String,
	genre: String,
	bitrate: String,
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

fn to_entry(raw: RawEntry) -> Option<Entry> {
	let name = raw.server_name.trim();
	let kind = raw
		.server_type
		.split(';')
		.next()?
		.trim()
		.to_ascii_lowercase();
	if name.is_empty()
		|| !matches!(
			kind.as_str(),
			"audio/mpeg"
				| "audio/mp3"
				| "audio/x-mpeg"
				| "audio/aac"
				| "audio/aacp"
				| "audio/ogg"
				| "application/ogg"
		) {
		return None;
	}
	let url = reqwest::Url::parse(raw.listen_url.trim()).ok()?;
	if !matches!(url.scheme(), "http" | "https")
		|| url.host_str().is_none()
		|| !url.username().is_empty()
		|| url.password().is_some()
	{
		return None;
	}
	let genre = raw.genre.trim().to_owned();
	let rate = raw
		.bitrate
		.trim()
		.parse::<u32>()
		.ok()
		.filter(|b| *b > 0)
		.map(|b| format!("{b}k"))
		.unwrap_or_default();
	let info = [genre.as_str(), rate.as_str()]
		.into_iter()
		.filter(|s| !s.is_empty())
		.collect::<Vec<_>>()
		.join(", ");
	Some(Entry {
		station: Station {
			id: format!("icecast:{}", enc(url.as_str())),
			name: name.into(),
			url: url.into(),
			source: Source::Icecast,
			info: (!info.is_empty()).then_some(info),
		},
		genre,
	})
}

impl Icecast {
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
		let mut response = client
			.get(format!("{}/yp.xml", self.base.trim_end_matches('/')))
			.timeout(Duration::from_secs(8))
			.send()
			.await
			.map_err(crate::net::http_err)?;
		if !response.status().is_success() {
			return Err(CoreError::Http(format!(
				"HTTP {}",
				response.status().as_u16()
			)));
		}
		if response
			.content_length()
			.is_some_and(|n| n > MAX_BODY as u64)
		{
			return Err(CoreError::Http("Directory too large".into()));
		}
		let mut body = Vec::new();
		while let Some(chunk) = response.chunk().await.map_err(crate::net::http_err)? {
			if chunk.len() > MAX_BODY - body.len() {
				return Err(CoreError::Http("Directory too large".into()));
			}
			body.extend_from_slice(&chunk);
		}
		let directory: Directory = quick_xml::de::from_reader(body.as_slice())
			.map_err(|_| CoreError::Decode("Invalid directory".into()))?;
		let mut seen = HashSet::new();
		let mut entries: Vec<_> = directory
			.entry
			.into_iter()
			.filter_map(to_entry)
			.filter(|e| seen.insert(e.station.url.clone()))
			.collect();
		entries.sort_by_cached_key(|e| e.station.name.to_lowercase());
		let entries = Arc::new(entries);
		*cache = Some((Instant::now(), entries.clone()));
		Ok(entries)
	}

	pub async fn children(&self, client: &reqwest::Client, id: &str) -> Result<Vec<Node>> {
		if id == "icecast" {
			return Ok(vec![
				folder("icecast/all", "All Stations"),
				folder("icecast/genres", "By Genre"),
			]);
		}
		let (head, offset) = match id.rsplit_once('@') {
			Some((head, off)) => (head, off.parse::<usize>().unwrap_or(0)),
			None => (id, 0),
		};
		if head != "icecast/all" && head != "icecast/genres" && !head.starts_with("icecast/genre/")
		{
			return Ok(vec![]);
		}
		let entries = self.entries(client).await?;
		if head == "icecast/genres" {
			let mut genres: Vec<_> = entries
				.iter()
				.map(|e| e.genre.as_str())
				.filter(|g| !g.is_empty())
				.collect::<HashSet<_>>()
				.into_iter()
				.collect();
			genres.sort_by_key(|g| (g.to_lowercase(), *g));
			return Ok(genres
				.into_iter()
				.map(|g| folder(format!("icecast/genre/{}", enc(g)), g))
				.collect());
		}
		let genre = head
			.strip_prefix("icecast/genre/")
			.map(|g| percent_decode_str(g).decode_utf8_lossy());
		let mut matching = entries
			.iter()
			.filter(|e| genre.as_ref().is_none_or(|g| e.genre == *g))
			.skip(offset);
		let mut nodes: Vec<_> = matching
			.by_ref()
			.take(PAGE)
			.map(|e| Node::Station(e.station.clone()))
			.collect();
		if matching.next().is_some() {
			nodes.push(Node::LoadMore {
				id: format!("{head}@{}", offset + PAGE),
			});
		}
		Ok(nodes)
	}

	pub async fn search(
		&self,
		client: &reqwest::Client,
		query: &SearchQuery,
	) -> Result<Vec<Station>> {
		let name = query.name.trim().to_lowercase();
		let genre = query.genre.as_deref().unwrap_or("").trim().to_lowercase();
		Ok(self
			.entries(client)
			.await?
			.iter()
			.filter(|e| {
				e.station.name.to_lowercase().contains(&name)
					&& e.genre.to_lowercase().contains(&genre)
			})
			.map(|e| e.station.clone())
			.collect())
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[tokio::test]
	async fn cache_expires_after_fifteen_minutes() {
		let ice = Icecast::new("http://127.0.0.1:0");
		*ice.cache.lock().await = Some((Instant::now(), Arc::new(vec![])));
		assert!(ice.entries(&crate::net::client()).await.unwrap().is_empty());
		*ice.cache.lock().await = Some((Instant::now() - CACHE_AGE, Arc::new(vec![])));
		assert!(ice.entries(&crate::net::client()).await.is_err());
	}
}
