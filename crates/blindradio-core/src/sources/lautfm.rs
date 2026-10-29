//! laut.fm's public JSON API: browse by genre, and search.

use super::{Node, SearchQuery, Sort, Sortable, sort_rows};
use crate::Result;
use crate::net::get_json;
use crate::station::{Source, Station};
use percent_encoding::{NON_ALPHANUMERIC, percent_decode_str, utf8_percent_encode};
use serde::Deserialize;
use std::collections::HashSet;

const PAGE: usize = 100;
/// Genres offered for browsing; laut.fm publishes hundreds, most popular first.
const GENRES: usize = 60;
const GENRE_STATIONS: usize = 250;
const SEARCH_STATIONS: usize = 50;

pub struct LautFm {
	base: String,
}

#[derive(Deserialize)]
struct Genre {
	name: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Raw {
	name: String,
	display_name: String,
	stream_url: String,
	format: String,
	location: String,
	genres: Vec<String>,
	active: bool,
}

#[derive(Deserialize)]
struct Search {
	#[serde(default)]
	results: Vec<Group>,
}

#[derive(Deserialize)]
struct Group {
	#[serde(default)]
	items: Vec<Item>,
}

#[derive(Deserialize)]
struct Item {
	station: Option<Raw>,
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

fn row(raw: &Raw) -> Option<(Sortable, Station)> {
	let name = if raw.display_name.trim().is_empty() {
		raw.name.trim()
	} else {
		raw.display_name.trim()
	};
	if name.is_empty() || !raw.active {
		return None;
	}
	let url = reqwest::Url::parse(raw.stream_url.trim()).ok()?;
	if !matches!(url.scheme(), "http" | "https")
		|| url.host_str().is_none()
		|| !url.username().is_empty()
		|| url.password().is_some()
	{
		return None;
	}
	let genres = raw
		.genres
		.iter()
		.map(|g| g.trim())
		.filter(|g| !g.is_empty())
		.take(3)
		.collect::<Vec<_>>()
		.join(", ");
	let info = [raw.location.trim(), genres.as_str()]
		.into_iter()
		.filter(|s| !s.is_empty())
		.collect::<Vec<_>>()
		.join(", ");
	let station = Station {
		id: format!("lautfm:{}", raw.name.trim()),
		name: name.into(),
		url: url.into(),
		source: Source::LautFm,
		info: (!info.is_empty()).then_some(info),
	};
	let sortable = Sortable {
		name: station.name.clone(),
		bitrate: 0,
		codec: String::new(),
		tags: raw.genres.join(", "),
		url: station.url.clone(),
		popularity: 0,
	};
	Some((sortable, station))
}

/// Rows keep laut.fm's own order; a sort reorders them, and `Default` stays as served.
fn rows(raws: &[Raw]) -> Vec<(Sortable, Station)> {
	let mut seen = HashSet::new();
	raws.iter()
		.filter_map(row)
		.filter(|(_, s)| seen.insert(s.url.clone()))
		.collect()
}

impl LautFm {
	pub fn new(base: impl Into<String>) -> Self {
		Self { base: base.into() }
	}

	fn url(&self, path: &str) -> String {
		format!("{}{path}", self.base.trim_end_matches('/'))
	}

	pub async fn children(
		&self,
		client: &reqwest::Client,
		id: &str,
		sort: Sort,
	) -> Result<Vec<Node>> {
		if id == "lautfm" {
			return Ok(vec![folder("lautfm/genres", "By Genre")]);
		}
		if id == "lautfm/genres" {
			let genres: Vec<Genre> = get_json(client, &self.url("/genres")).await?;
			return Ok(genres
				.into_iter()
				.take(GENRES)
				.map(|g| {
					let name = g.name.trim().to_owned();
					folder(format!("lautfm/genre/{}", enc(&name)), name)
				})
				.collect());
		}
		let (head, offset) = match id.rsplit_once('@') {
			Some((head, off)) => (head, off.parse::<usize>().unwrap_or(0)),
			None => (id, 0),
		};
		let Some(encoded) = head.strip_prefix("lautfm/genre/") else {
			return Ok(vec![]);
		};
		let genre = percent_decode_str(encoded).decode_utf8_lossy();
		let raws: Vec<Raw> = get_json(
			client,
			&self.url(&format!(
				"/stations/genre/{}?limit={GENRE_STATIONS}",
				enc(&genre)
			)),
		)
		.await?;
		let mut rows = rows(&raws);
		sort_rows(&mut rows, sort);
		let mut nodes: Vec<Node> = rows
			.into_iter()
			.skip(offset)
			.take(PAGE)
			.map(|(_, station)| Node::Station(station))
			.collect();
		if raws.len() > offset + PAGE {
			nodes.push(Node::LoadMore {
				id: format!("{head}@{}", offset + PAGE),
			});
		}
		Ok(nodes)
	}

	/// Name only: laut.fm has no genre, country or language filter on its search.
	pub async fn search(
		&self,
		client: &reqwest::Client,
		query: &SearchQuery,
	) -> Result<Vec<Station>> {
		let name = query.name.trim();
		if name.is_empty() {
			return Ok(vec![]);
		}
		let found: Search = get_json(
			client,
			&self.url(&format!(
				"/search/stations?query={}&limit={SEARCH_STATIONS}",
				enc(name)
			)),
		)
		.await?;
		let raws: Vec<Raw> = found
			.results
			.into_iter()
			.flat_map(|g| g.items.into_iter().filter_map(|i| i.station))
			.collect();
		let mut rows = rows(&raws);
		sort_rows(&mut rows, query.sort);
		Ok(rows.into_iter().map(|(_, station)| station).collect())
	}
}
