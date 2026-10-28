//! Public Internet-Radio pages and their published playlist targets.

use super::{Node, SearchQuery};
use crate::net::get_bytes;
use crate::station::{Source, Station};
use crate::{CoreError, Result};
use percent_encoding::{NON_ALPHANUMERIC, percent_decode_str, utf8_percent_encode};
use reqwest::Url;
use scraper::{ElementRef, Html, Selector};
use std::collections::HashSet;

pub struct InternetRadio {
	base: String,
}

struct Page {
	stations: Vec<Station>,
	next: Option<Url>,
}

impl InternetRadio {
	pub fn new(base: impl Into<String>) -> Self {
		Self { base: base.into() }
	}

	pub async fn children(&self, client: &reqwest::Client, id: &str) -> Result<Vec<Node>> {
		if id == "internet-radio" {
			return Ok([
				("internet-radio/featured", "Featured Stations"),
				("internet-radio/genres", "By Genre"),
			]
			.map(|(id, name)| Node::Folder {
				id: id.into(),
				name: name.into(),
			})
			.into());
		}
		let base = Url::parse(&self.base).map_err(|_| bad_reply())?;
		let url = match id {
			"internet-radio/featured" => base.join("/").map_err(|_| bad_reply())?,
			"internet-radio/genres" => base.join("/stations/").map_err(|_| bad_reply())?,
			_ => {
				let Some(encoded) = id.strip_prefix("internet-radio/page/") else {
					return Ok(vec![]);
				};
				let decoded = percent_decode_str(encoded)
					.decode_utf8()
					.map_err(|_| bad_reply())?;
				navigation(&base, &decoded).ok_or_else(bad_reply)?
			}
		};
		let bytes = get_bytes(client, url.as_str()).await?;
		if id == "internet-radio/genres" {
			return genres(&base, &bytes);
		}
		let page = parse_page(&base, &url, &bytes)?;
		let mut nodes: Vec<_> = page.stations.into_iter().map(Node::Station).collect();
		if let Some(next) = page.next {
			nodes.push(Node::LoadMore { id: page_id(&next) });
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
		let base = Url::parse(&self.base).map_err(|_| bad_reply())?;
		let mut url = base.join("/search/").map_err(|_| bad_reply())?;
		url.query_pairs_mut()
			.append_pair("radio", query.name.trim());
		let mut visited = HashSet::new();
		let mut streams = HashSet::new();
		let mut stations = Vec::new();
		for _ in 0..3 {
			if !visited.insert(url.to_string()) {
				break;
			}
			let bytes = get_bytes(client, url.as_str()).await?;
			let page = parse_page(&base, &url, &bytes)?;
			stations.extend(
				page.stations
					.into_iter()
					.filter(|s| streams.insert(s.url.clone())),
			);
			let Some(next) = page.next else { break };
			url = next;
		}
		Ok(stations)
	}
}

fn bad_reply() -> CoreError {
	CoreError::Http("Bad reply".into())
}

fn selector(css: &str) -> Selector {
	Selector::parse(css).expect("static selector")
}

fn text(element: ElementRef<'_>) -> String {
	element
		.text()
		.collect::<String>()
		.split_whitespace()
		.collect::<Vec<_>>()
		.join(" ")
}

fn page_id(url: &Url) -> String {
	format!(
		"internet-radio/page/{}",
		utf8_percent_encode(url.as_str(), NON_ALPHANUMERIC)
	)
}

fn http_url(value: &str) -> Option<Url> {
	let url = Url::parse(value).ok()?;
	let authority = value.split_once("://")?.1.split(['/', '?', '#']).next()?;
	(matches!(url.scheme(), "http" | "https")
		&& url.has_host()
		&& url.username().is_empty()
		&& url.password().is_none()
		&& !authority.contains('@'))
	.then_some(url)
}

fn navigation(base: &Url, href: &str) -> Option<Url> {
	let url = base.join(href).ok()?;
	if !matches!(url.scheme(), "http" | "https")
		|| url.origin() != base.origin()
		|| !url.username().is_empty()
		|| url.password().is_some()
	{
		return None;
	}
	let parts: Vec<_> = url.path().split('/').filter(|s| !s.is_empty()).collect();
	let allowed = match parts.as_slice() {
		["search"] | ["stations", _] => true,
		["stations", _, page] => page
			.strip_prefix("page")
			.is_some_and(|n| n.parse::<u32>().is_ok_and(|n| n > 0)),
		_ => false,
	};
	allowed.then_some(url)
}

fn genre_url(base: &Url, href: &str) -> Option<Url> {
	let url = navigation(base, href)?;
	let parts: Vec<_> = url.path().split('/').filter(|s| !s.is_empty()).collect();
	(matches!(parts.as_slice(), ["stations", _]) && url.query().is_none()).then_some(url)
}

fn genres(base: &Url, bytes: &[u8]) -> Result<Vec<Node>> {
	let html = Html::parse_document(&String::from_utf8_lossy(bytes));
	let mut seen = HashSet::new();
	let nodes: Vec<_> = html
		.select(&selector("a[href]"))
		.filter_map(|a| {
			let url = genre_url(base, a.value().attr("href")?)?;
			let name = text(a);
			(!name.is_empty() && seen.insert(url.to_string())).then(|| Node::Folder {
				id: page_id(&url),
				name,
			})
		})
		.collect();
	if nodes.is_empty() && html.select(&selector("h1")).next().is_none() {
		return Err(bad_reply());
	}
	Ok(nodes)
}

fn parse_page(base: &Url, current: &Url, bytes: &[u8]) -> Result<Page> {
	let html = Html::parse_document(&String::from_utf8_lossy(bytes));
	if html.select(&selector("table, h1")).next().is_none() {
		return Err(bad_reply());
	}
	let heading = selector("h4");
	let anchors = selector("a[href]");
	let mut seen = HashSet::new();
	let stations = html
		.select(&selector("tr"))
		.filter_map(|row| {
			let title = row.select(&heading).next()?;
			let name = text(title);
			if name.is_empty() {
				return None;
			}
			let stream = row.select(&anchors).find_map(|a| {
				let link = base.join(a.value().attr("href")?).ok()?;
				if link.origin() != base.origin()
					|| link.path() != "/servers/tools/playlistgenerator/"
				{
					return None;
				}
				let target = link.query_pairs().find(|(key, _)| key == "u")?.1;
				http_url(&target)
			})?;
			if !seen.insert(stream.to_string()) {
				return None;
			}
			let slug = title.select(&anchors).find_map(|a| {
				let link = base.join(a.value().attr("href")?).ok()?;
				if link.origin() != base.origin() {
					return None;
				}
				let parts: Vec<_> = link.path().split('/').filter(|s| !s.is_empty()).collect();
				match parts.as_slice() {
					["station", slug] => Some((*slug).to_owned()),
					_ => None,
				}
			});
			let mut genres = Vec::new();
			for anchor in row.select(&anchors) {
				if anchor
					.value()
					.attr("href")
					.and_then(|href| genre_url(base, href))
					.is_some()
				{
					let name = text(anchor);
					if !name.is_empty() && !genres.contains(&name) {
						genres.push(name);
					}
				}
			}
			Some(Station {
				id: format!(
					"internet-radio:{}",
					slug.unwrap_or_else(|| stream.to_string())
				),
				name,
				url: stream.into(),
				source: Source::InternetRadio,
				info: (!genres.is_empty()).then(|| genres.join(", ")),
			})
		})
		.collect();
	let next = html
		.select(&selector(".next:not(.disabled) a[href], a[rel=next]"))
		.find_map(|a| {
			let url = navigation(base, a.value().attr("href")?)?;
			(url != *current).then_some(url)
		});
	Ok(Page { stations, next })
}
