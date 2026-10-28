//! SHOUTcast's public website endpoints and published PLS links.

use super::{Node, SearchQuery, Sort, Sortable, sort_rows};
use crate::net::http_err;
use crate::station::{Source, Station};
use crate::{CoreError, Result};
use percent_encoding::{NON_ALPHANUMERIC, percent_decode_str, utf8_percent_encode};
use scraper::{Html, Selector};
use serde::Deserialize;
use std::collections::HashSet;
use std::time::Duration;

const PAGE: usize = 100;
const TIMEOUT: Duration = Duration::from_secs(8);

pub struct Shoutcast {
	base: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Row {
	#[serde(rename = "ID")]
	id: u64,
	name: String,
	format: String,
	bitrate: u64,
	genre: String,
	listeners: u64,
}

impl Shoutcast {
	pub fn new(base: impl Into<String>) -> Shoutcast {
		Shoutcast { base: base.into() }
	}

	fn url(&self, path: &str) -> String {
		format!("{}{path}", self.base.trim_end_matches('/'))
	}

	async fn stations(
		&self,
		client: &reqwest::Client,
		path: &str,
		form: &[(&str, &str)],
	) -> Result<Vec<(Sortable, Station)>> {
		let text = client
			.post(self.url(path))
			.timeout(TIMEOUT)
			.form(form)
			.send()
			.await
			.map_err(http_err)?
			.error_for_status()
			.map_err(http_err)?
			.text()
			.await
			.map_err(http_err)?;
		let rows: Vec<Row> = serde_json::from_str(&text)
			.map_err(|_| CoreError::Http("Invalid directory response".into()))?;
		let playlist = reqwest::Url::parse(&self.url("/sbin/tunein-station.pls"))
			.map_err(|_| CoreError::Http("Invalid directory URL".into()))?;
		Ok(rows
			.into_iter()
			.filter_map(|row| {
				let name = row.name.trim();
				if row.id == 0 || name.is_empty() {
					return None;
				}
				let codec = match row.format.as_str() {
					"audio/mpeg" => "MP3",
					"audio/aac" | "audio/aacp" => "AAC",
					_ => return None,
				};
				let mut url = playlist.clone();
				url.query_pairs_mut().append_pair("id", &row.id.to_string());
				let mut info = Vec::new();
				if !row.genre.trim().is_empty() {
					info.push(row.genre.trim().to_owned());
				}
				info.push(format!("{codec} {}k", row.bitrate));
				info.push(format!("{} listeners", row.listeners));
				let station = Station {
					id: format!("shoutcast:{}", row.id),
					name: name.into(),
					url: url.into(),
					source: Source::Shoutcast,
					info: Some(info.join(", ")),
				};
				let sortable = Sortable {
					name: station.name.clone(),
					bitrate: u32::try_from(row.bitrate).unwrap_or(0),
					codec: codec.into(),
					tags: row.genre.trim().to_owned(),
					url: station.url.clone(),
					popularity: row.listeners,
				};
				Some((sortable, station))
			})
			.collect())
	}

	async fn genres(&self, client: &reqwest::Client) -> Result<Vec<Node>> {
		let base = reqwest::Url::parse(&self.url("/"))
			.map_err(|_| CoreError::Http("Invalid directory URL".into()))?;
		let text = client
			.get(base.clone())
			.timeout(TIMEOUT)
			.send()
			.await
			.map_err(http_err)?
			.error_for_status()
			.map_err(http_err)?
			.text()
			.await
			.map_err(http_err)?;
		let document = Html::parse_document(&text);
		let selector = Selector::parse("a[href]").expect("genre link selector");
		let mut seen = HashSet::new();
		Ok(document
			.select(&selector)
			.filter_map(|link| {
				let url = base.join(link.value().attr("href")?).ok()?;
				if url.origin() != base.origin()
					|| !url.username().is_empty()
					|| url.password().is_some()
					|| !url.path().eq_ignore_ascii_case("/Genre")
				{
					return None;
				}
				let name = url.query_pairs().find(|(key, _)| key == "name")?.1;
				let name = name.trim();
				if name.is_empty() || !seen.insert(name.to_owned()) {
					return None;
				}
				Some(Node::Folder {
					id: format!(
						"shoutcast/genre/{}",
						utf8_percent_encode(name, NON_ALPHANUMERIC)
					),
					name: name.into(),
				})
			})
			.collect())
	}

	pub async fn children(
		&self,
		client: &reqwest::Client,
		id: &str,
		sort: Sort,
	) -> Result<Vec<Node>> {
		let (head, offset) = match id.rsplit_once('@') {
			Some((head, offset)) => match offset.parse::<usize>() {
				Ok(offset) => (head, offset),
				Err(_) => return Ok(vec![]),
			},
			None => (id, 0),
		};
		let mut stations = match head {
			"shoutcast" => {
				return Ok(vec![
					Node::Folder {
						id: "shoutcast/top".into(),
						name: "Top Stations".into(),
					},
					Node::Folder {
						id: "shoutcast/genres".into(),
						name: "By Genre".into(),
					},
				]);
			}
			"shoutcast/genres" => return self.genres(client).await,
			"shoutcast/top" => self.stations(client, "/Home/Top", &[]).await?,
			_ => {
				let Some(genre) = head.strip_prefix("shoutcast/genre/") else {
					return Ok(vec![]);
				};
				let genre = percent_decode_str(genre)
					.decode_utf8()
					.map_err(|_| CoreError::Http("Invalid genre".into()))?;
				self.stations(client, "/Home/BrowseByGenre", &[("genrename", &genre)])
					.await?
			}
		};
		sort_rows(&mut stations, sort);
		let total = stations.len();
		let mut nodes: Vec<_> = stations
			.into_iter()
			.skip(offset)
			.take(PAGE)
			.map(|(_, station)| Node::Station(station))
			.collect();
		if let Some(next) = offset.checked_add(PAGE).filter(|next| *next < total) {
			nodes.push(Node::LoadMore {
				id: format!("{head}@{next}"),
			});
		}
		Ok(nodes)
	}

	pub async fn search(&self, client: &reqwest::Client, q: &SearchQuery) -> Result<Vec<Station>> {
		let name = q.name.trim();
		if name.is_empty() {
			return Ok(vec![]);
		}
		let mut stations = self
			.stations(client, "/Search/UpdateSearch", &[("query", name)])
			.await?;
		sort_rows(&mut stations, q.sort);
		Ok(stations.into_iter().map(|(_, station)| station).collect())
	}
}
