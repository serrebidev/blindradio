//! radio.net's public API: the Top 100, local stations and search.
//! No key; station entries carry their stream URLs inline.

use super::{Node, SearchQuery};
use crate::Result;
use crate::net::get_json;
use crate::station::{Source, Station};
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use serde::Deserialize;
use std::collections::HashSet;

const PAGE: usize = 50;
const TOP: usize = 100;

pub struct RadioNet {
	base: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Listing {
	playables: Vec<Playable>,
	#[serde(rename = "totalCount")]
	total: usize,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Playable {
	id: String,
	name: String,
	genres: Vec<String>,
	city: String,
	country: String,
	streams: Vec<StreamEntry>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct StreamEntry {
	url: String,
	status: String,
}

fn folder(id: impl Into<String>, name: impl Into<String>) -> Node {
	Node::Folder {
		id: id.into(),
		name: name.into(),
	}
}

fn station(p: &Playable) -> Option<Station> {
	let name = p.name.trim();
	if name.is_empty() || p.id.trim().is_empty() {
		return None;
	}
	let raw = p
		.streams
		.iter()
		.find(|s| s.status.eq_ignore_ascii_case("VALID") && !s.url.trim().is_empty())
		.or_else(|| p.streams.iter().find(|s| !s.url.trim().is_empty()))?;
	let url = reqwest::Url::parse(raw.url.trim()).ok()?;
	if !matches!(url.scheme(), "http" | "https")
		|| url.host_str().is_none()
		|| !url.username().is_empty()
		|| url.password().is_some()
	{
		return None;
	}
	let genres = p
		.genres
		.iter()
		.map(|g| g.trim())
		.filter(|g| !g.is_empty())
		.take(3)
		.collect::<Vec<_>>()
		.join(", ");
	let place = [p.city.trim(), p.country.trim()]
		.into_iter()
		.filter(|s| !s.is_empty())
		.collect::<Vec<_>>()
		.join(", ");
	let info = if !genres.is_empty() { genres } else { place };
	Some(Station {
		id: format!("radionet:{}", p.id.trim()),
		name: name.into(),
		url: url.into(),
		source: Source::RadioNet,
		info: (!info.is_empty()).then_some(info),
	})
}

fn stations(listing: &Listing) -> Vec<Station> {
	let mut seen = HashSet::new();
	listing
		.playables
		.iter()
		.filter_map(station)
		.filter(|s| seen.insert(s.url.clone()))
		.collect()
}

impl RadioNet {
	pub fn new(base: impl Into<String>) -> Self {
		Self { base: base.into() }
	}

	fn url(&self, path: &str) -> String {
		format!("{}{path}", self.base.trim_end_matches('/'))
	}

	pub async fn children(&self, client: &reqwest::Client, id: &str) -> Result<Vec<Node>> {
		if id == "radionet" {
			return Ok(vec![
				folder("radionet/top", "Top Stations"),
				folder("radionet/local", "Local Stations"),
			]);
		}
		if id == "radionet/top" {
			let listing: Listing = get_json(
				client,
				&self.url(&format!(
					"/stations/list-by-system-name?systemName=STATIONS_TOP&count={TOP}"
				)),
			)
			.await?;
			return Ok(stations(&listing).into_iter().map(Node::Station).collect());
		}
		if let Some(rest) = id.strip_prefix("radionet/local") {
			let offset: usize = rest
				.strip_prefix('@')
				.and_then(|n| n.parse().ok())
				.unwrap_or(0);
			let listing: Listing = get_json(
				client,
				&self.url(&format!("/stations/local?count={PAGE}&offset={offset}")),
			)
			.await?;
			let page = stations(&listing);
			let got = page.len();
			let mut nodes: Vec<Node> = page.into_iter().map(Node::Station).collect();
			if got > 0 && offset + got < listing.total {
				nodes.push(Node::LoadMore {
					id: format!("radionet/local@{}", offset + got),
				});
			}
			return Ok(nodes);
		}
		Ok(vec![])
	}

	pub async fn search(
		&self,
		client: &reqwest::Client,
		query: &SearchQuery,
	) -> Result<Vec<Station>> {
		if query.name.trim().is_empty() {
			return Ok(vec![]);
		}
		let q = utf8_percent_encode(query.name.trim(), NON_ALPHANUMERIC);
		let listing: Listing = get_json(
			client,
			&self.url(&format!("/stations/search?query={q}&count={PAGE}&offset=0")),
		)
		.await?;
		Ok(stations(&listing))
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn playable() -> Playable {
		Playable {
			id: "jazz24".into(),
			name: " Jazz 24 ".into(),
			genres: vec!["Jazz".into(), "Smooth".into()],
			city: "Seattle".into(),
			country: "US".into(),
			streams: vec![
				StreamEntry {
					url: "https://bad.example/dead".into(),
					status: "INVALID".into(),
				},
				StreamEntry {
					url: "https://stream.example/jazz".into(),
					status: "VALID".into(),
				},
			],
		}
	}

	#[test]
	fn maps_a_playable_to_a_station() {
		let st = station(&playable()).expect("station");
		assert_eq!(st.id, "radionet:jazz24");
		assert_eq!(st.name, "Jazz 24");
		assert_eq!(st.url, "https://stream.example/jazz", "first valid stream");
		assert_eq!(st.source, Source::RadioNet);
		assert_eq!(st.info.as_deref(), Some("Jazz, Smooth"));
	}

	#[test]
	fn skips_unplayable_entries() {
		let mut p = playable();
		p.streams.clear();
		assert!(station(&p).is_none());
		let mut p = playable();
		p.name = " ".into();
		assert!(station(&p).is_none());
	}
}
