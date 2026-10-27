//! iHeartRadio: live stations by genre or all, search, and the station's track history.

use super::{HistoryItem, Node, SearchQuery};
use crate::Result;
use crate::net::get_json;
use crate::station::{Source, Station};
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use serde::Deserialize;
use std::time::{Duration, UNIX_EPOCH};

const PAGE: usize = 100;

pub struct IHeart {
	base: String,
}

#[derive(Deserialize)]
struct Genres {
	#[serde(default)]
	hits: Vec<Genre>,
}

#[derive(Deserialize)]
struct Genre {
	id: u64,
	name: String,
	#[serde(default)]
	sort: i64,
}

#[derive(Deserialize)]
struct Live {
	#[serde(default)]
	total: usize,
	#[serde(default)]
	hits: Vec<Hit>,
}

#[derive(Deserialize)]
struct Hit {
	id: u64,
	#[serde(default)]
	name: String,
	#[serde(default)]
	freq: String,
	#[serde(default)]
	band: String,
	#[serde(default)]
	markets: Vec<Market>,
	#[serde(default)]
	streams: Streams,
}

#[derive(Deserialize)]
struct Market {
	#[serde(default)]
	city: String,
}

#[derive(Deserialize, Default)]
struct Streams {
	secure_shoutcast_stream: Option<String>,
	shoutcast_stream: Option<String>,
	secure_hls_stream: Option<String>,
	hls_stream: Option<String>,
}

#[derive(Deserialize)]
struct Found {
	results: FoundResults,
}

#[derive(Deserialize)]
struct FoundResults {
	#[serde(default)]
	stations: Vec<FoundStation>,
}

#[derive(Deserialize)]
struct FoundStation {
	id: u64,
}

#[derive(Deserialize)]
struct Tracks {
	#[serde(default)]
	data: Vec<Track>,
}

#[derive(Deserialize)]
struct Track {
	#[serde(default)]
	title: String,
	#[serde(default)]
	artist: String,
	#[serde(default, rename = "startTime")]
	start_time: Option<u64>,
}

fn to_station(h: Hit) -> Option<Station> {
	let s = h.streams;
	// ICY first: titles and the lowest start latency.
	let url = [
		s.secure_shoutcast_stream,
		s.shoutcast_stream,
		s.secure_hls_stream,
		s.hls_stream,
	]
	.into_iter()
	.flatten()
	.find(|u| !u.is_empty())?;
	let dial = format!("{} {}", h.freq.trim(), h.band.trim());
	let city = h.markets.first().map_or("", |m| m.city.trim());
	let info = [dial.trim(), city]
		.into_iter()
		.filter(|p| !p.is_empty())
		.collect::<Vec<_>>()
		.join(", ");
	Some(Station {
		id: format!("iheart:{}", h.id),
		name: h.name.trim().to_owned(),
		url,
		source: Source::IHeart,
		info: (!info.is_empty()).then_some(info),
	})
}

fn folder(id: impl Into<String>, name: impl Into<String>) -> Node {
	Node::Folder {
		id: id.into(),
		name: name.into(),
	}
}

impl IHeart {
	pub fn new(base: impl Into<String>) -> IHeart {
		IHeart { base: base.into() }
	}

	fn url(&self, path: &str) -> String {
		format!("{}{path}", self.base.trim_end_matches('/'))
	}

	pub async fn children(&self, client: &reqwest::Client, id: &str) -> Result<Vec<Node>> {
		let (head, offset) = match id.rsplit_once('@') {
			Some((h, o)) => (h, o.parse().unwrap_or(0)),
			None => (id, 0),
		};
		let parts: Vec<&str> = head.split('/').collect();
		let genre = match parts.as_slice() {
			["iheart"] => {
				return Ok(vec![
					folder("iheart/all", "All Stations"),
					folder("iheart/genres", "Genres"),
				]);
			}
			["iheart", "genres"] => {
				let mut g: Genres =
					get_json(client, &self.url("/api/v2/content/genre?limit=100")).await?;
				g.hits.sort_by_key(|g| g.sort);
				return Ok(g
					.hits
					.into_iter()
					.map(|g| folder(format!("iheart/genre/{}", g.id), g.name))
					.collect());
			}
			["iheart", "all"] => String::new(),
			["iheart", "genre", g] => {
				format!("&genreId={}", utf8_percent_encode(g, NON_ALPHANUMERIC))
			}
			_ => return Ok(vec![]),
		};
		let live: Live = get_json(
			client,
			&self.url(&format!(
				"/api/v2/content/liveStations?countryCode=US&limit={PAGE}&offset={offset}{genre}"
			)),
		)
		.await?;
		let mut nodes: Vec<Node> = live
			.hits
			.into_iter()
			.filter_map(to_station)
			.map(Node::Station)
			.collect();
		if offset + PAGE < live.total {
			nodes.push(Node::LoadMore {
				id: format!("{head}@{}", offset + PAGE),
			});
		}
		Ok(nodes)
	}

	/// Search has no streams, so the hits are looked up by id. Genre, country and language are ignored.
	pub async fn search(&self, client: &reqwest::Client, q: &SearchQuery) -> Result<Vec<Station>> {
		let name = utf8_percent_encode(q.name.trim(), NON_ALPHANUMERIC);
		let found: Found = get_json(
			client,
			&self.url(&format!(
				"/api/v3/search/all?keywords={name}&maxRows=50&station=true&artist=false\
				&album=false&track=false&playlist=false&podcast=false"
			)),
		)
		.await?;
		let ids: Vec<u64> = found.results.stations.iter().map(|s| s.id).collect();
		if ids.is_empty() {
			return Ok(vec![]);
		}
		// Repeated id= params keep only one id; the comma form returns them all.
		let list = ids.iter().map(u64::to_string).collect::<Vec<_>>().join(",");
		let live: Live = get_json(
			client,
			&self.url(&format!(
				"/api/v2/content/liveStations?id={list}&limit={}",
				ids.len()
			)),
		)
		.await?;
		let mut hits = live.hits;
		hits.sort_by_key(|h| ids.iter().position(|i| *i == h.id));
		Ok(hits.into_iter().filter_map(to_station).collect())
	}

	/// Recent tracks, newest first. `id` is `iheart:<n>` or the bare number.
	pub async fn history(&self, client: &reqwest::Client, id: &str) -> Result<Vec<HistoryItem>> {
		let id = id.strip_prefix("iheart:").unwrap_or(id);
		let id = utf8_percent_encode(id, NON_ALPHANUMERIC);
		let tracks: Tracks = get_json(
			client,
			&self.url(&format!(
				"/api/v3/live-meta/stream/{id}/trackHistory?limit=20"
			)),
		)
		.await?;
		Ok(tracks
			.data
			.into_iter()
			.filter(|t| !t.title.trim().is_empty())
			.map(|t| {
				let (title, artist) = (t.title.trim(), t.artist.trim());
				HistoryItem {
					title: if artist.is_empty() {
						title.to_owned()
					} else {
						format!("{artist} - {title}")
					},
					played_at: t.start_time.map(|s| UNIX_EPOCH + Duration::from_secs(s)),
				}
			})
			.collect())
	}
}
