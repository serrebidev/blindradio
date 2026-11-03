//! radio.net's public API: the Top 100, local stations, its tag lists and search.
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

/// radio.net's name for a tag list, and the folder that opens it.
const TAGS: [(&str, &str); 4] = [
	("genres", "By Genre"),
	("countries", "By Country"),
	("languages", "By Language"),
	("topics", "By Topic"),
];

#[derive(Deserialize, Default)]
#[serde(default)]
struct Tags {
	genres: Vec<Tag>,
	countries: Vec<Tag>,
	languages: Vec<Tag>,
	topics: Vec<Tag>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Tag {
	name: String,
	slug: String,
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
	let details = crate::station::Details {
		genre: crate::station::known(&genres),
		country: crate::station::known(&p.country),
		..Default::default()
	};
	let info = if !genres.is_empty() { genres } else { place };
	Some(Station {
		id: format!("radionet:{}", p.id.trim()),
		name: name.into(),
		url: url.into(),
		source: Source::RadioNet,
		info: (!info.is_empty()).then_some(info),
		details,
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
			let mut nodes = vec![
				folder("radionet/top", "Top Stations"),
				folder("radionet/local", "Local Stations"),
			];
			nodes.extend(TAGS.map(|(kind, name)| folder(format!("radionet/tags/{kind}"), name)));
			return Ok(nodes);
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
		if let Some(kind) = id.strip_prefix("radionet/tags/") {
			let mut all: Tags = get_json(client, &self.url("/stations/tags")).await?;
			let tags = match kind {
				"genres" => &mut all.genres,
				"countries" => &mut all.countries,
				"languages" => &mut all.languages,
				"topics" => &mut all.topics,
				_ => return Ok(vec![]),
			};
			tags.sort_by_key(|t| t.name.to_lowercase());
			return Ok(tags
				.iter()
				.filter(|t| !t.name.trim().is_empty() && !t.slug.is_empty())
				.map(|t| folder(format!("radionet/tag/{kind}/{}", t.slug), t.name.trim()))
				.collect());
		}
		let (head, offset) = match id.rsplit_once('@') {
			Some((head, n)) => (head, n.parse().unwrap_or(0)),
			None => (id, 0),
		};
		if head == "radionet/local" {
			return self.page(client, head, "/stations/local?", offset).await;
		}
		if let Some((kind, slug)) = head
			.strip_prefix("radionet/tag/")
			.and_then(|rest| rest.split_once('/'))
		{
			let (kind, slug) = (
				utf8_percent_encode(kind, NON_ALPHANUMERIC),
				utf8_percent_encode(slug, NON_ALPHANUMERIC),
			);
			let path = format!("/stations/by-tag?tagType={kind}&slug={slug}&");
			return self.page(client, head, &path, offset).await;
		}
		Ok(vec![])
	}

	/// One page of a long list. `path` ends ready for more query parameters.
	async fn page(
		&self,
		client: &reqwest::Client,
		head: &str,
		path: &str,
		offset: usize,
	) -> Result<Vec<Node>> {
		let listing: Listing = get_json(
			client,
			&self.url(&format!("{path}count={PAGE}&offset={offset}")),
		)
		.await?;
		// Offset counts what radio.net sent: rows without a stream are dropped below.
		let sent = listing.playables.len();
		let mut nodes: Vec<Node> = stations(&listing).into_iter().map(Node::Station).collect();
		if sent > 0 && offset + sent < listing.total {
			nodes.push(Node::LoadMore {
				id: format!("{head}@{}", offset + sent),
			});
		}
		Ok(nodes)
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
