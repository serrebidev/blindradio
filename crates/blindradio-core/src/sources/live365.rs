//! Live365: genres and search. Its station API refuses anonymous callers, but its web pages
//! carry each whole list as React Router page data, and a station streams from its mount id.
//! ponytail: reads page data, not an API. A site rewrite empties this directory until fixed.

use super::{Node, SearchQuery, Sort, Sortable, sort_rows};
use crate::net::get_bytes;
use crate::station::{Details, Source, Station};
use crate::{CoreError, Result};
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

const PAGE: usize = 100;
const STREAM: &str = "https://streaming.live365.com/";
/// Page data nests only a few levels; this stops a malformed page from recursing for ever.
const MAX_DEPTH: usize = 32;

pub struct Live365 {
	base: String,
}

#[derive(Deserialize)]
struct Genre {
	#[serde(default)]
	name: String,
}

#[derive(Deserialize)]
struct Row {
	#[serde(default)]
	name: String,
	#[serde(default, rename = "mountId")]
	mount: String,
	#[serde(default)]
	genres: Vec<Genre>,
}

fn bad_reply() -> CoreError {
	CoreError::Http("Bad reply".into())
}

/// The page's address for a genre name, the way the site builds its own links.
fn slug(name: &str) -> String {
	let name = name
		.to_lowercase()
		.replace(' ', "-")
		.replace('\'', "")
		.replace('/', "-")
		.replace('&', "n");
	let name: String = name
		.chars()
		.filter(|c| c.is_ascii_alphanumeric() || *c == '-')
		.collect();
	format!("{name}-music-radio")
}

fn folder(kind: &str, name: &str) -> Node {
	Node::Folder {
		id: format!("live365/{kind}/{}", slug(name)),
		name: name.trim().to_owned(),
	}
}

/// React Router's page data: a flat list where an object's `_3: 7` means key `list[3]`,
/// value `list[7]`, and a negative index means no value.
fn expand(flat: &[Value], index: i64, depth: usize) -> Value {
	let Some(value) = usize::try_from(index).ok().and_then(|i| flat.get(i)) else {
		return Value::Null;
	};
	if depth > MAX_DEPTH {
		return Value::Null;
	}
	let refer = |v: &Value| v.as_i64().map_or(Value::Null, |i| expand(flat, i, depth + 1));
	match value {
		Value::Object(fields) => {
			let mut out = Map::new();
			for (key, target) in fields {
				let name = key
					.strip_prefix('_')
					.and_then(|k| k.parse::<usize>().ok())
					.and_then(|k| flat.get(k))
					.and_then(Value::as_str);
				if let Some(name) = name {
					out.insert(name.to_owned(), refer(target));
				}
			}
			Value::Object(out)
		}
		Value::Array(items) => Value::Array(items.iter().map(refer).collect()),
		other => other.clone(),
	}
}

/// The route data a page was rendered with, from its `enqueue("...")` script calls.
fn page_data(html: &str) -> Result<Value> {
	const CALL: &str = "enqueue(\"";
	let mut text = String::new();
	let mut rest = html;
	while let Some(at) = rest.find(CALL) {
		rest = &rest[at + CALL.len()..];
		let mut escaped = false;
		let end = rest
			.char_indices()
			.find(|&(_, c)| {
				let closes = c == '"' && !escaped;
				escaped = c == '\\' && !escaped;
				closes
			})
			.map(|(i, _)| i)
			.ok_or_else(bad_reply)?;
		let part: String =
			serde_json::from_str(&format!("\"{}\"", &rest[..end])).map_err(|_| bad_reply())?;
		text.push_str(&part);
		rest = &rest[end..];
	}
	let line = text
		.lines()
		.find(|l| l.starts_with('['))
		.ok_or_else(bad_reply)?;
	let flat: Vec<Value> = serde_json::from_str(line).map_err(|_| bad_reply())?;
	Ok(expand(&flat, 0, 0))
}

/// A list a page's route data holds under `key`; `None` when no route has it.
fn list<T: DeserializeOwned>(data: &Value, key: &str) -> Option<Vec<T>> {
	data.get("loaderData")?
		.as_object()?
		.values()
		.find_map(|route| serde_json::from_value(route.get(key)?.clone()).ok())
}

fn rows(found: Vec<Row>) -> Vec<(Sortable, Station)> {
	found
		.into_iter()
		.filter(|r| {
			!r.name.trim().is_empty()
				&& !r.mount.is_empty()
				&& r.mount
					.bytes()
					.all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
		})
		.map(|r| {
			let genres = r
				.genres
				.iter()
				.map(|g| g.name.trim())
				.filter(|g| !g.is_empty())
				.collect::<Vec<_>>();
			let shown = genres.iter().take(3).copied().collect::<Vec<_>>().join(", ");
			let all = genres.join(", ");
			let url = format!("{STREAM}{}", r.mount);
			let sortable = Sortable {
				name: r.name.trim().to_owned(),
				tags: all.clone(),
				url: url.clone(),
				..Default::default()
			};
			let station = Station {
				id: format!("live365:{}", r.mount),
				name: r.name.trim().to_owned(),
				url,
				source: Source::Live365,
				info: (!shown.is_empty()).then_some(shown),
				details: Details {
					genre: crate::station::known(&all),
					..Default::default()
				},
			};
			(sortable, station)
		})
		.collect()
}

impl Live365 {
	pub fn new(base: impl Into<String>) -> Live365 {
		Live365 { base: base.into() }
	}

	async fn page(&self, client: &reqwest::Client, path: &str) -> Result<Value> {
		let url = format!("{}{path}", self.base.trim_end_matches('/'));
		let bytes = get_bytes(client, &url).await?;
		page_data(&String::from_utf8_lossy(&bytes))
	}

	pub async fn children(
		&self,
		client: &reqwest::Client,
		id: &str,
		sort: Sort,
	) -> Result<Vec<Node>> {
		if id == "live365" {
			let data = self.page(client, "/listen/genres").await?;
			let genres: Vec<Genre> = list(&data, "genres").ok_or_else(bad_reply)?;
			return Ok(genres.iter().map(|g| folder("genre", &g.name)).collect());
		}
		let (head, offset) = match id.rsplit_once('@') {
			Some((head, n)) => (head, n.parse().unwrap_or(0)),
			None => (id, 0),
		};
		// A sub-genre's page lists its sisters as related: only a top genre shows them.
		let (slug, top) = match (head.strip_prefix("live365/genre/"), head.strip_prefix("live365/sub/")) {
			(Some(slug), _) => (slug, true),
			(_, Some(slug)) => (slug, false),
			_ => return Ok(vec![]),
		};
		let data = self.page(client, &format!("/listen/{slug}")).await?;
		let mut nodes = Vec::new();
		if top && offset == 0 {
			let related: Vec<Genre> = list(&data, "relatedGenres").unwrap_or_default();
			nodes.extend(related.iter().map(|g| folder("sub", &g.name)));
		}
		// A few of the site's own genre names lead to no list: the folder is empty.
		let mut stations = rows(list(&data, "allStations").unwrap_or_default());
		sort_rows(&mut stations, sort);
		let total = stations.len();
		nodes.extend(
			stations
				.into_iter()
				.skip(offset)
				.take(PAGE)
				.map(|(_, station)| Node::Station(station)),
		);
		if offset + PAGE < total {
			nodes.push(Node::LoadMore {
				id: format!("{head}@{}", offset + PAGE),
			});
		}
		Ok(nodes)
	}

	pub async fn search(&self, client: &reqwest::Client, q: &SearchQuery) -> Result<Vec<Station>> {
		let name = q.name.trim();
		if name.is_empty() {
			return Ok(vec![]);
		}
		let query = utf8_percent_encode(name, NON_ALPHANUMERIC);
		let data = self
			.page(client, &format!("/listen/search?query={query}"))
			.await?;
		let found = rows(list(&data, "results").unwrap_or_default());
		Ok(found.into_iter().map(|(_, station)| station).collect())
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn slugs_match_the_sites_links() {
		for (name, want) in [
			("Jazz", "jazz-music-radio"),
			("Electronic/Dance", "electronic-dance-music-radio"),
			("Urban and R&B", "urban-and-rnb-music-radio"),
			("90's Alternative", "90s-alternative-music-radio"),
			("Music To ... To", "music-to--to-music-radio"),
		] {
			assert_eq!(slug(name), want);
		}
	}

	#[test]
	fn page_data_expands_references_and_ignores_the_rest() {
		// Two enqueue calls split the line; quotes and escapes survive; -5 is no value.
		let html = r#"<script>x.enqueue("[{\"_1\":2},\"loaderData\",{\"_3\":4},\"route\",{\"_5\":6,\"_9\":-5},\"allStations\",[7],{\"_8\":10},\"name\",\"gone\",");</script>
<script>x.enqueue("\"Radio \\\"One\\\"\"]\n[\"later\"]\n");</script>"#;
		let data = page_data(html).unwrap();
		assert_eq!(
			data["loaderData"]["route"]["allStations"][0]["name"],
			"Radio \"One\""
		);
		assert!(data["loaderData"]["route"]["gone"].is_null());
		assert!(page_data("<html>no data</html>").is_err());
	}

	#[test]
	fn expand_survives_a_page_that_refers_to_itself() {
		let flat: Vec<Value> = serde_json::from_str(r#"[[0]]"#).unwrap();
		assert!(expand(&flat, 0, 0).is_array());
		assert!(expand(&flat, 9, 0).is_null());
		assert!(expand(&flat, -5, 0).is_null());
	}

	#[test]
	fn rows_map_to_stations_and_skip_what_cannot_play() {
		let row = |name: &str, mount: &str| Row {
			name: name.into(),
			mount: mount.into(),
			genres: vec![
				Genre { name: "Jazz".into() },
				Genre { name: "Bop".into() },
				Genre { name: "Swing".into() },
				Genre { name: "Blues".into() },
			],
		};
		let got = rows(vec![row(" One ", "a1"), row("", "a2"), row("Three", ""), row("Bad", "a/../b")]);
		assert_eq!(got.len(), 1);
		let st = &got[0].1;
		assert_eq!(st.id, "live365:a1");
		assert_eq!(st.name, "One");
		assert_eq!(st.url, "https://streaming.live365.com/a1");
		assert_eq!(st.source, Source::Live365);
		assert_eq!(st.info.as_deref(), Some("Jazz, Bop, Swing"));
		assert_eq!(st.details.genre.as_deref(), Some("Jazz, Bop, Swing, Blues"));
	}
}
