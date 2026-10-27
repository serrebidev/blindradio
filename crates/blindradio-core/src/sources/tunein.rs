//! TuneIn (OPML API in JSON): browse by location, search.

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
	children: Vec<Outline>,
}

impl TuneIn {
	pub fn new(base: impl Into<String>) -> TuneIn {
		TuneIn { base: base.into() }
	}

	fn url(&self, page: &str, query: &str) -> String {
		format!("{}/{page}.ashx?{query}", self.base.trim_end_matches('/'))
	}

	/// Location folders and stations; groups such as "Stations" are flattened.
	fn nodes(&self, outlines: Vec<Outline>, out: &mut Vec<Node>) {
		for o in outlines {
			if o.kind == "link" && o.guide_id.starts_with('r') {
				out.push(Node::Folder {
					id: format!("tunein/{}", o.guide_id),
					name: o.text.trim().to_owned(),
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
		let info = o.subtext.trim();
		let guide = utf8_percent_encode(&o.guide_id, NON_ALPHANUMERIC);
		Some(Station {
			id: format!("tunein:{}", o.guide_id),
			name: o.text.trim().to_owned(),
			// Without formats= TuneIn answers with a "notcompatible" stream.
			url: self.url("Tune", &format!("id={guide}&formats=mp3,aac,ogg,hls")),
			source: Source::TuneIn,
			info: (!info.is_empty()).then(|| info.to_owned()),
		})
	}

	pub async fn children(&self, client: &reqwest::Client, id: &str) -> Result<Vec<Node>> {
		let guide = match id.strip_prefix("tunein/") {
			Some(g) => g,
			None if id == "tunein" => "r0",
			None => return Ok(vec![]),
		};
		let guide = utf8_percent_encode(guide, NON_ALPHANUMERIC);
		let reply: Reply = get_json(
			client,
			&self.url("Browse", &format!("id={guide}&render=json")),
		)
		.await?;
		let mut out = Vec::new();
		self.nodes(reply.body, &mut out);
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
