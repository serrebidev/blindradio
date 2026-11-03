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

/// A group's folder path: segments split on the shared path separator,
/// so a group exported from nested folders browses as nested folders.
fn group_path(group: &Option<String>) -> Vec<String> {
	match group {
		Some(g) => g
			.split(crate::import::GROUP_PATH_SEP)
			.map(|s| s.trim().to_owned())
			.filter(|s| !s.is_empty())
			.collect(),
		None => Vec::new(),
	}
}

fn station_of(stem: &str, i: usize, e: &crate::playlist::PlaylistEntry) -> Node {
	Node::Station(Station {
		id: format!("brdir:{stem}:{i}"),
		name: e.title.clone().unwrap_or_else(|| e.url.clone()),
		url: e.url.clone(),
		source: Source::Directory,
		info: e.group.clone(),
		details: Default::default(),
	})
}

/// One browse level of a playlist file: the stations whose group path
/// is exactly `prefix`, plus a folder per next segment under it, all in
/// file order — the groups a playlist carries are its subdirectories.
fn level(stem: &str, body: &str, prefix: &[String]) -> Vec<Node> {
	let entries = parse_m3u(body);
	let mut nodes = Vec::new();
	let mut seen: Vec<String> = Vec::new();
	for (i, e) in entries.iter().enumerate() {
		let path = group_path(&e.group);
		if path.len() < prefix.len() || path[..prefix.len()] != *prefix {
			continue;
		}
		if path.len() == prefix.len() {
			nodes.push(station_of(stem, i, e));
		} else {
			let next = &path[prefix.len()];
			if !seen.contains(next) {
				seen.push(next.clone());
				let mut id = format!("brdir/{}", enc(stem));
				for seg in prefix.iter().chain(std::iter::once(next)) {
					id += &format!("/{}", enc(seg));
				}
				nodes.push(folder(id, next.clone()));
			}
		}
	}
	nodes
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
		let mut segments = id.split('/');
		let _ = segments.next(); // "brdir"
		let Some(enc_stem) = segments.next() else {
			return Ok(vec![]);
		};
		let stem = percent_decode_str(enc_stem).decode_utf8_lossy().into_owned();
		let prefix: Vec<String> = segments
			.map(|s| percent_decode_str(s).decode_utf8_lossy().into_owned())
			.collect();
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
		Ok(level(&stem, &String::from_utf8_lossy(&bytes), &prefix))
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

	const BODY: &str = "#EXTM3U\n#EXTINF:-1 group-title=\"BBC\",BBC Radio 1\nhttp://x/1\n#EXTINF:-1,Loose\nhttp://x/2\n#EXTINF:-1 group-title=\"BBC\",BBC Radio 2\nhttp://x/3\n#EXTINF:-1 group-title=\"News / World\",World News\nhttp://x/4\n";

	#[test]
	fn groups_are_folders_loose_stations_stay_at_the_top() {
		let got = level("UK", BODY, &[]);
		assert_eq!(got.len(), 3);
		assert_eq!(got[0], folder("brdir/UK/BBC", "BBC"));
		let Node::Station(loose) = &got[1] else {
			panic!("station row")
		};
		assert_eq!(loose.id, "brdir:UK:1");
		assert_eq!(loose.name, "Loose");
		assert_eq!(loose.source, Source::Directory);
		assert_eq!(got[2], folder("brdir/UK/News", "News"));
	}

	#[test]
	fn a_folder_level_lists_its_stations() {
		let got = level("UK", BODY, &["BBC".to_string()]);
		assert_eq!(got.len(), 2);
		let Node::Station(first) = &got[0] else {
			panic!("station row")
		};
		assert_eq!(first.id, "brdir:UK:0");
		assert_eq!(first.name, "BBC Radio 1");
		assert_eq!(first.info.as_deref(), Some("BBC"));
		let Node::Station(second) = &got[1] else {
			panic!("station row")
		};
		assert_eq!(second.name, "BBC Radio 2");
	}

	#[test]
	fn a_group_path_browses_as_nested_folders() {
		let got = level("UK", BODY, &["News".to_string()]);
		assert_eq!(got, vec![folder("brdir/UK/News/World", "World")]);
		let got = level("UK", BODY, &["News".to_string(), "World".to_string()]);
		assert_eq!(got.len(), 1);
		let Node::Station(only) = &got[0] else {
			panic!("station row")
		};
		assert_eq!(only.name, "World News");
		assert_eq!(only.info.as_deref(), Some("News / World"));
	}
}
