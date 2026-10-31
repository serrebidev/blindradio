//! The BlindRadio Directory: community-submitted playlists in the
//! BlindRadio-directory GitHub repo, one `.m3u` file per folder. The repo
//! is read live through the GitHub contents API, so merged submissions
//! show up in Browse without an app update.

use super::Node;
use crate::Result;
use crate::error::CoreError;
use crate::net::http_err;
use crate::playlist::parse_m3u;
use crate::station::{Source, Station};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, percent_decode_str, utf8_percent_encode};
use serde::Deserialize;
use std::time::Duration;

/// One URL path segment: all but the unreserved marks is encoded, so a
/// stem's dots survive in `.m3u` file names.
const SEGMENT: &AsciiSet = &NON_ALPHANUMERIC
	.remove(b'.')
	.remove(b'-')
	.remove(b'_')
	.remove(b'~');

fn enc(s: &str) -> String {
	utf8_percent_encode(s, SEGMENT).to_string()
}

fn folder(id: impl Into<String>, name: impl Into<String>) -> Node {
	Node::Folder {
		id: id.into(),
		name: name.into(),
	}
}

#[derive(Deserialize)]
struct Entry {
	name: String,
	#[serde(rename = "type")]
	kind: String,
}

/// The repo's playlist files as folders, in name order. Anything that is
/// not an `.m3u` file (the README, folders) is not a browse folder.
fn folders(entries: Vec<Entry>) -> Vec<Node> {
	let mut files: Vec<&str> = entries
		.iter()
		.filter(|e| e.kind == "file")
		.filter_map(|e| e.name.strip_suffix(".m3u"))
		.collect();
	files.sort_by_key(|s| s.to_lowercase());
	files
		.into_iter()
		.map(|stem| folder(format!("brdir/{}", enc(stem)), stem))
		.collect()
}

/// One playlist file's stations, in file order.
fn stations(stem: &str, body: &str) -> Vec<Node> {
	parse_m3u(body)
		.into_iter()
		.enumerate()
		.map(|(i, e)| {
			Node::Station(Station {
				id: format!("brdir:{stem}:{i}"),
				name: e.title.unwrap_or_else(|| e.url.clone()),
				url: e.url,
				source: Source::Directory,
				info: e.group,
			})
		})
		.collect()
}

pub struct Directory {
	/// The repo's contents API root, no trailing slash.
	base: String,
}

impl Directory {
	pub fn new(base: String) -> Directory {
		Directory {
			base: base.trim_end_matches('/').to_owned(),
		}
	}

	pub async fn children(&self, client: &reqwest::Client, id: &str) -> Result<Vec<Node>> {
		if id == "brdir" {
			let entries: Vec<Entry> =
				crate::net::get_json(client, &format!("{}/contents", self.base)).await?;
			return Ok(folders(entries));
		}
		let Some(enc_stem) = id.strip_prefix("brdir/") else {
			return Ok(vec![]);
		};
		let stem = percent_decode_str(enc_stem).decode_utf8_lossy();
		let url = format!("{}/contents/{}", self.base, enc(&format!("{stem}.m3u")));
		// The raw media type answers with the file itself, not base64 JSON.
		let response = client
			.get(&url)
			.header("User-Agent", crate::net::user_agent_for(&url))
			.header("Accept", "application/vnd.github.raw+json")
			.timeout(Duration::from_secs(8))
			.send()
			.await
			.map_err(http_err)?;
		let status = response.status().as_u16();
		if status >= 400 {
			return Err(CoreError::Http(format!("HTTP {status}")));
		}
		let bytes = response.bytes().await.map_err(http_err)?;
		Ok(stations(&stem, &String::from_utf8_lossy(&bytes)))
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn entry(name: &str, kind: &str) -> Entry {
		Entry {
			name: name.into(),
			kind: kind.into(),
		}
	}

	#[test]
	fn folders_are_the_playlists_in_name_order() {
		let got = folders(vec![
			entry("UK.m3u", "file"),
			entry("README.md", "file"),
			entry("Public Radio.m3u", "file"),
			entry("extra", "dir"),
			entry("Belgium.m3u", "file"),
		]);
		assert_eq!(
			got,
			vec![
				folder("brdir/Belgium", "Belgium"),
				folder("brdir/Public%20Radio", "Public Radio"),
				folder("brdir/UK", "UK"),
			]
		);
	}

	#[test]
	fn stations_carry_name_group_and_directory_source() {
		let body = "#EXTM3U\n#EXTINF:-1 group-title=\"BBC\",BBC Radio 1\nhttp://x/1\n#EXTINF:-1,Loose\nhttp://x/2\nhttp://x/3\n";
		let got = stations("UK", body);
		assert_eq!(got.len(), 3);
		let Node::Station(first) = &got[0] else {
			panic!("station row")
		};
		assert_eq!(first.id, "brdir:UK:0");
		assert_eq!(first.name, "BBC Radio 1");
		assert_eq!(first.info.as_deref(), Some("BBC"));
		assert_eq!(first.source, Source::Directory);
		let Node::Station(third) = &got[2] else {
			panic!("station row")
		};
		assert_eq!(third.name, "http://x/3", "a bare URL names itself");
		assert_eq!(third.info, None);
	}
}
