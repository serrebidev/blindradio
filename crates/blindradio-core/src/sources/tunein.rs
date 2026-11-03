//! TuneIn (OPML API in JSON): browse by category and location, search.

use super::{Node, SearchQuery};
use crate::Result;
use crate::net::get_json;
use crate::station::{Source, Station};
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use serde::Deserialize;

pub struct TuneIn {
	base: String,
}

#[derive(Deserialize)]
struct Reply {
	#[serde(default)]
	body: Vec<Outline>,
}

#[derive(Deserialize, Default)]
struct Outline {
	#[serde(default, rename = "type")]
	kind: String,
	#[serde(default)]
	item: String,
	#[serde(default)]
	text: String,
	#[serde(default)]
	subtext: String,
	#[serde(default)]
	guide_id: String,
	#[serde(default)]
	key: String,
	#[serde(default, rename = "URL")]
	url: String,
	#[serde(default)]
	children: Vec<Outline>,
}

/// The Browse query of a link worth following. Shows and podcasts are not stations.
fn browse_query(o: &Outline) -> Option<String> {
	if o.kind != "link" || matches!(o.key.as_str(), "nextShows" | "podcast") {
		return None;
	}
	let url = reqwest::Url::parse(&o.url).ok()?;
	let query = url.query()?;
	let shows = query
		.split('&')
		.any(|p| p == "c=podcast" || p.starts_with("filter=p"));
	(url.path().ends_with("/Browse.ashx") && !shows).then(|| query.to_owned())
}

impl TuneIn {
	pub fn new(base: impl Into<String>) -> TuneIn {
		TuneIn { base: base.into() }
	}

	fn url(&self, page: &str, query: &str) -> String {
		format!("{}/{page}.ashx?{query}", self.base.trim_end_matches('/'))
	}

	/// Folders and stations; groups such as "Stations" are flattened. "More Stations" is the
	/// rest of the list.
	fn nodes(&self, outlines: Vec<Outline>, out: &mut Vec<Node>) {
		for o in outlines {
			if o.kind == "link" && o.guide_id.starts_with('r') {
				out.push(Node::Folder {
					id: format!("tunein/{}", o.guide_id),
					name: o.text.trim().to_owned(),
				});
			} else if let Some(query) = browse_query(&o) {
				let id = format!("tunein/{query}");
				out.push(if o.key == "nextStations" {
					Node::LoadMore { id }
				} else {
					Node::Folder {
						id,
						name: o.text.trim().to_owned(),
					}
				});
			} else if let Some(s) = self.station(&o) {
				out.push(Node::Station(s));
			}
			self.nodes(o.children, out);
		}
	}

	fn station(&self, o: &Outline) -> Option<Station> {
		if o.kind != "audio" || o.item != "station" || !o.guide_id.starts_with('s') {
			return None;
		}
		// "Z100 - Not Supported": TuneIn lists it but only serves a spoken refusal.
		if o.key == "unavailable" {
			return None;
		}
		let info = o.subtext.trim();
		let guide = utf8_percent_encode(&o.guide_id, NON_ALPHANUMERIC);
		Some(Station {
			id: format!("tunein:{}", o.guide_id),
			name: o.text.trim().to_owned(),
			// Without formats= TuneIn answers with a "notcompatible" stream.
			url: self.url("Tune", &format!("id={guide}&formats=mp3,aac,ogg,hls")),
			source: Source::TuneIn,
			info: (!info.is_empty()).then(|| info.to_owned()),
			details: Default::default(),
		})
	}

	pub async fn children(&self, client: &reqwest::Client, id: &str) -> Result<Vec<Node>> {
		let query = match id.strip_prefix("tunein/") {
			// TuneIn's own query, as its link gave it.
			Some(query) if query.contains('=') => query.to_owned(),
			Some(guide) => format!("id={}", utf8_percent_encode(guide, NON_ALPHANUMERIC)),
			None if id == "tunein" => {
				return Ok([
					("tunein/c=local", "Local Radio"),
					("tunein/c=music", "Music"),
					("tunein/c=talk", "Talk"),
					("tunein/c=sports", "Sports"),
					("tunein/r0", "By Location"),
				]
				.map(|(id, name)| Node::Folder {
					id: id.into(),
					name: name.into(),
				})
				.into());
			}
			None => return Ok(vec![]),
		};
		let reply: Reply =
			get_json(client, &self.url("Browse", &format!("{query}&render=json"))).await?;
		let mut out = Vec::new();
		self.nodes(reply.body, &mut out);
		// A station can sit in both "Local Stations" and "Stations".
		let mut seen = std::collections::HashSet::new();
		out.retain(|n| !matches!(n, Node::Station(s) if !seen.insert(s.id.clone())));
		Ok(out)
	}

	/// TuneIn has no genre, country or language filters; they are ignored.
	pub async fn search(&self, client: &reqwest::Client, q: &SearchQuery) -> Result<Vec<Station>> {
		let name = utf8_percent_encode(q.name.trim(), NON_ALPHANUMERIC);
		let reply: Reply = get_json(
			client,
			&self.url("Search", &format!("query={name}&types=station&render=json")),
		)
		.await?;
		let mut out = Vec::new();
		self.nodes(reply.body, &mut out);
		Ok(out
			.into_iter()
			.filter_map(|n| match n {
				Node::Station(s) => Some(s),
				_ => None,
			})
			.collect())
	}
}
