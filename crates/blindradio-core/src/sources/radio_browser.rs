//! Radio Browser (radio-browser.info): browse by genre and country, search, and the filter lists.

use super::{Node, SearchQuery, Sort};
use crate::Result;
use crate::net::get_json;
use crate::station::{Source, Station};
use percent_encoding::{NON_ALPHANUMERIC, percent_decode_str, utf8_percent_encode};
use reqwest::Url;
use serde::Deserialize;
use std::collections::HashSet;

const PAGE: usize = 100;
/// Most pages fetched for one state request when exact-match filtering empties them.
const STATE_PAGES: usize = 5;
/// Countries with at least this many stations are browsed by state.
const BIG_COUNTRY: u32 = 1000;
const LIST_TOP: &str = "limit=200&order=stationcount&reverse=true&hidebroken=true";

pub struct RadioBrowser {
	base: String,
}

#[derive(Deserialize)]
struct Named {
	name: String,
	#[serde(default)]
	iso_3166_1: String,
	#[serde(default)]
	stationcount: u32,
	#[serde(default)]
	country: String,
}

#[derive(Deserialize, Default)]
struct RbStation {
	#[serde(default)]
	stationuuid: String,
	#[serde(default)]
	name: String,
	#[serde(default)]
	url: String,
	#[serde(default)]
	url_resolved: String,
	#[serde(default)]
	tags: String,
	#[serde(default)]
	country: String,
	#[serde(default)]
	state: String,
	#[serde(default)]
	codec: String,
	#[serde(default)]
	bitrate: u32,
}

fn enc(s: &str) -> String {
	utf8_percent_encode(s, NON_ALPHANUMERIC).to_string()
}

fn dec(s: &str) -> String {
	percent_decode_str(s).decode_utf8_lossy().into_owned()
}

fn folder(id: impl Into<String>, name: impl Into<String>) -> Node {
	Node::Folder {
		id: id.into(),
		name: name.into(),
	}
}

fn unescape(s: &str) -> String {
	let mut out = String::with_capacity(s.len());
	let mut rest = s;
	while let Some(i) = rest.find('&') {
		out.push_str(&rest[..i]);
		rest = &rest[i..];
		let decoded = rest.find(';').filter(|&e| e <= 8).and_then(|e| {
			let c = match &rest[1..e] {
				"amp" => '&',
				"lt" => '<',
				"gt" => '>',
				"quot" => '"',
				"apos" => '\'',
				n => {
					let n = n.strip_prefix('#')?;
					let code = match n.strip_prefix(['x', 'X']) {
						Some(h) => u32::from_str_radix(h, 16).ok()?,
						None => n.parse().ok()?,
					};
					char::from_u32(code)?
				}
			};
			Some((c, e))
		});
		match decoded {
			Some((c, e)) => {
				out.push(c);
				rest = &rest[e + 1..];
			}
			None => {
				out.push('&');
				rest = &rest[1..];
			}
		}
	}
	out + rest
}

fn to_station(r: RbStation) -> Option<Station> {
	let url = if r.url_resolved.is_empty() {
		r.url
	} else {
		r.url_resolved
	};
	if r.stationuuid.is_empty() || url.is_empty() {
		return None;
	}
	let mut name = unescape(&r.name).trim().to_owned();
	if name.is_empty() {
		name = Url::parse(&url)
			.ok()
			.and_then(|u| u.host_str().map(str::to_owned))
			.unwrap_or_else(|| url.clone());
	}
	let tag = r.tags.split(',').next().unwrap_or("").trim();
	let mut tag_cap = tag.chars();
	let tag = tag_cap
		.next()
		.map(|c| c.to_uppercase().chain(tag_cap).collect::<String>())
		.unwrap_or_default();
	let rate = match (r.codec.trim(), r.bitrate) {
		("", 0) => String::new(),
		(c, 0) => c.to_owned(),
		("", b) => format!("{b}k"),
		(c, b) => format!("{c} {b}k"),
	};
	let info = [tag.as_str(), r.country.trim(), rate.as_str()]
		.into_iter()
		.filter(|p| !p.is_empty())
		.collect::<Vec<_>>()
		.join(", ");
	Some(Station {
		id: format!("rb:{}", r.stationuuid),
		name,
		url,
		source: Source::RadioBrowser,
		info: (!info.is_empty()).then_some(info),
	})
}

/// `rb/genre/jazz@200` into (`rb/genre/jazz`, 200).
fn split_offset(id: &str) -> (&str, usize) {
	match id.rsplit_once('@') {
		Some((head, off)) => (head, off.parse().unwrap_or(0)),
		None => (id, 0),
	}
}

/// A page of stations, with Load More when the server's page was full. `keep` filters after
/// paging, so a filtered page still offers more.
fn page_nodes(
	raw: Vec<RbStation>,
	id: &str,
	offset: usize,
	keep: impl Fn(&RbStation) -> bool,
) -> Vec<Node> {
	let full = raw.len() >= PAGE;
	let mut nodes: Vec<Node> = raw
		.into_iter()
		.filter(|r| keep(r))
		.filter_map(to_station)
		.map(Node::Station)
		.collect();
	if full {
		nodes.push(Node::LoadMore {
			id: format!("{id}@{}", offset + PAGE),
		});
	}
	nodes
}

impl RadioBrowser {
	pub fn new(base: impl Into<String>) -> RadioBrowser {
		RadioBrowser { base: base.into() }
	}

	/// `{base}/json/<segments>` with each segment percent-encoded, then the raw query.
	fn url(&self, segments: &[&str], query: &str) -> String {
		let mut url = format!("{}/json", self.base.trim_end_matches('/'));
		for s in segments {
			url.push('/');
			url.push_str(&utf8_percent_encode(s, NON_ALPHANUMERIC).to_string());
		}
		// The states endpoint wants a trailing slash after the country name.
		if segments.first() == Some(&"states") {
			url.push('/');
		}
		if !query.is_empty() {
			url.push('?');
			url.push_str(query);
		}
		url
	}

	async fn list(&self, client: &reqwest::Client, kind: &str, query: &str) -> Result<Vec<Named>> {
		get_json(client, &self.url(&[kind], query)).await
	}

	async fn stations(
		&self,
		client: &reqwest::Client,
		segments: &[&str],
		extra: &str,
		offset: usize,
		sort: Sort,
	) -> Result<Vec<RbStation>> {
		let mut query = String::new();
		for part in [extra, &sort.query(), "hidebroken=true"] {
			if !part.is_empty() {
				if !query.is_empty() {
					query.push('&');
				}
				query.push_str(part);
			}
		}
		query.push_str(&format!("&limit={PAGE}&offset={offset}"));
		get_json(client, &self.url(segments, &query)).await
	}

	pub async fn children(
		&self,
		client: &reqwest::Client,
		id: &str,
		sort: Sort,
	) -> Result<Vec<Node>> {
		let (head, offset) = split_offset(id);
		let parts: Vec<&str> = head.split('/').collect();
		match parts.as_slice() {
			["rb"] => Ok(vec![
				folder("rb/genres", "By Genre"),
				folder("rb/countries", "By Country"),
				folder("rb/languages", "By Language"),
				folder("rb/states", "By State"),
				folder("rb/top", "Top Stations"),
			]),
			["rb", "genres"] => {
				let tags = self.list(client, "tags", LIST_TOP).await?;
				Ok(tags
					.into_iter()
					.map(|t| {
						let name = t.name.trim().to_owned();
						folder(format!("rb/genre/{}", enc(&t.name)), name)
					})
					.collect())
			}
			["rb", "genre", tag] => {
				let tag = dec(tag);
				let raw = self
					.stations(client, &["stations", "bytagexact", &tag], "", offset, sort)
					.await?;
				Ok(page_nodes(raw, head, offset, |_| true))
			}
			["rb", "countries"] => {
				let all = self
					.list(client, "countries", "order=name&hidebroken=true")
					.await?;
				Ok(all
					.into_iter()
					.filter(|c| !c.iso_3166_1.is_empty())
					.map(|c| folder(format!("rb/country/{}", c.iso_3166_1), c.name.trim()))
					.collect())
			}
			["rb", "country", iso] => self.country(client, iso, head, offset, sort).await,
			["rb", "country", iso, "all"] => {
				let raw = self
					.stations(
						client,
						&["stations", "bycountrycodeexact", iso],
						"",
						offset,
						sort,
					)
					.await?;
				Ok(page_nodes(raw, head, offset, |_| true))
			}
			["rb", "languages"] => {
				let all = self
					.list(client, "languages", "order=name&hidebroken=true")
					.await?;
				let mut seen = HashSet::new();
				Ok(all
					.into_iter()
					.filter_map(|l| {
						let name = l.name.trim().to_owned();
						(!name.is_empty() && seen.insert(name.clone()))
							.then(|| folder(format!("rb/language/{}", enc(&name)), name))
					})
					.collect())
			}
			["rb", "language", lang] => {
				let lang = dec(lang);
				let raw = self
					.stations(client, &["stations", "bylanguageexact", &lang], "", offset, sort)
					.await?;
				Ok(page_nodes(raw, head, offset, |_| true))
			}
			["rb", "states"] => {
				let all = self
					.list(client, "states", "order=name&hidebroken=true")
					.await?;
				let mut seen = HashSet::new();
				Ok(all
					.into_iter()
					.filter_map(|st| {
						let name = st.name.trim().to_owned();
						if name.is_empty() || !seen.insert(name.clone()) {
							return None;
						}
						let country = st.country.trim();
						let label = if country.is_empty() {
							name.clone()
						} else {
							format!("{name} ({country})")
						};
						Some(folder(format!("rb/state/{}", enc(&name)), label))
					})
					.collect())
			}
			["rb", "state", state] => {
				let state = dec(state);
				// The server matches `state` as a substring; keep exact matches only.
				let extra = format!("state={}", enc(&state));
				let keep = |r: &RbStation| r.state.trim() == state;
				let mut off = offset;
				// A full page can filter to nothing; look a few pages further for a hit.
				for pages in 1.. {
					let raw = self
						.stations(client, &["stations", "search"], &extra, off, sort)
						.await?;
					if pages == STATE_PAGES || raw.len() < PAGE || raw.iter().any(keep) {
						return Ok(page_nodes(raw, head, off, keep));
					}
					off += PAGE;
				}
				unreachable!()
			}
			["rb", "top"] => {
				let raw = self
					.stations(client, &["stations", "topvote"], "", offset, sort)
					.await?;
				Ok(page_nodes(raw, head, offset, |_| true))
			}
			["rb", "country", iso, "state", state] => {
				let state = dec(state);
				// The server matches `state` as a substring; keep exact matches only.
				let extra = format!("state={}&countrycode={}", enc(&state), enc(iso));
				let keep = |r: &RbStation| r.state.trim() == state;
				let mut off = offset;
				// A full page can filter to nothing; look a few pages further for a hit.
				for pages in 1.. {
					let raw = self
						.stations(client, &["stations", "search"], &extra, off, sort)
						.await?;
					if pages == STATE_PAGES || raw.len() < PAGE || raw.iter().any(keep) {
						return Ok(page_nodes(raw, head, off, keep));
					}
					off += PAGE;
				}
				unreachable!()
			}
			_ => Ok(vec![]),
		}
	}

	async fn country(
		&self,
		client: &reqwest::Client,
		iso: &str,
		head: &str,
		offset: usize,
		sort: Sort,
	) -> Result<Vec<Node>> {
		let all = self
			.list(client, "countries", "order=name&hidebroken=true")
			.await?;
		let Some(country) = all.into_iter().find(|c| c.iso_3166_1 == iso) else {
			return Ok(vec![]);
		};
		if country.stationcount < BIG_COUNTRY {
			let raw = self
				.stations(
					client,
					&["stations", "bycountrycodeexact", iso],
					"",
					offset,
					sort,
				)
				.await?;
			return Ok(page_nodes(raw, head, offset, |_| true));
		}
		let states: Vec<Named> = get_json(
			client,
			&self.url(&["states", &country.name], "order=name&hidebroken=true"),
		)
		.await?;
		let mut seen = HashSet::new();
		let mut nodes = vec![folder(format!("rb/country/{iso}/all"), "All stations")];
		for s in states {
			let name = s.name.trim();
			if !name.is_empty() && seen.insert(name.to_owned()) {
				nodes.push(folder(
					format!("rb/country/{iso}/state/{}", enc(name)),
					name,
				));
			}
		}
		Ok(nodes)
	}

	pub async fn search(&self, client: &reqwest::Client, q: &SearchQuery) -> Result<Vec<Station>> {
		let mut query = String::new();
		let name = q.name.trim();
		for (key, value) in [
			("name", Some(name)),
			("tag", q.genre.as_deref()),
			("country", q.country.as_deref()),
			("language", q.language.as_deref()),
		] {
			if let Some(v) = value.filter(|v| !v.is_empty()) {
				query.push_str(&format!("{key}={}&", enc(v)));
			}
		}
		let order = q.sort.query();
		if !order.is_empty() {
			query.push_str(&order);
			query.push('&');
		}
		query.push_str(&format!("limit={PAGE}&hidebroken=true"));
		let raw: Vec<RbStation> =
			get_json(client, &self.url(&["stations", "search"], &query)).await?;
		Ok(raw.into_iter().filter_map(to_station).collect())
	}

	async fn names(&self, client: &reqwest::Client, kind: &str) -> Result<Vec<String>> {
		let mut names: Vec<String> = self
			.list(client, kind, LIST_TOP)
			.await?
			.into_iter()
			.map(|n| n.name.trim().to_owned())
			.filter(|n| !n.is_empty())
			.collect();
		names.sort_by_key(|n| n.to_lowercase());
		Ok(names)
	}

	pub async fn genres(&self, client: &reqwest::Client) -> Result<Vec<String>> {
		self.names(client, "tags").await
	}

	pub async fn countries(&self, client: &reqwest::Client) -> Result<Vec<String>> {
		self.names(client, "countries").await
	}

	pub async fn languages(&self, client: &reqwest::Client) -> Result<Vec<String>> {
		self.names(client, "languages").await
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn unescape_entities() {
		assert_eq!(
			unescape("A &amp; B &#39;x&#x41; &bogus; &"),
			"A & B 'xA &bogus; &"
		);
	}

	#[test]
	fn offset_split() {
		assert_eq!(split_offset("rb/genre/a@100"), ("rb/genre/a", 100));
		assert_eq!(split_offset("rb/genre/a"), ("rb/genre/a", 0));
	}
}
