//! Polska, from rstreams.ovh: licensed Polish radio stations on air, one M3U file the
//! author updates every month or two, grouped by kind (Akademickie, Lokalne and so on).
//! The author's server is small: the file is downloaded only when it changed (ETag), kept on
//! disk between runs, and asked about at most every few minutes.

use std::path::PathBuf;
use std::sync::{Mutex, RwLock};
use std::time::{Duration, Instant};

use super::Node;
use crate::Result;
use crate::net::get_if_changed;
use crate::playlist::{PlaylistEntry, parse_m3u};
use crate::station::{Source, Station};

pub const ROOT: &str = "rstreams";
pub const NAME: &str = "Polska (rstreams.ovh)";
/// Opening folders and searching within this reuse the copy without asking the server.
const RECHECK: Duration = Duration::from_secs(5 * 60);
const FILE: &str = "rstreams-polska.m3u";
const TAG_FILE: &str = "rstreams-polska.etag";

/// The copy in use: when the server was last asked, its ETag, the stations.
type Copy = (Instant, Option<String>, Vec<PlaylistEntry>);

pub struct Rstreams {
	url: String,
	read: Mutex<Option<Copy>>,
	/// Where the copy is kept between runs; None keeps it in memory only.
	dir: RwLock<Option<PathBuf>>,
}

fn station(entry: &PlaylistEntry) -> Station {
	Station {
		id: format!("{ROOT}:{}", entry.url),
		name: entry.title.clone().unwrap_or_else(|| entry.url.clone()),
		url: entry.url.clone(),
		source: Source::Network("Polska".into()),
		info: entry.group.clone(),
		details: Default::default(),
	}
}

/// The groups as folders in file order, then the stations of the group `id` names.
fn nodes(entries: &[PlaylistEntry], id: &str) -> Vec<Node> {
	let group = |e: &PlaylistEntry| e.group.clone().unwrap_or_else(|| "Inne".into());
	match id.strip_prefix(ROOT).and_then(|rest| rest.strip_prefix('/')) {
		None => {
			let mut seen = Vec::new();
			for e in entries {
				let g = group(e);
				if !seen.contains(&g) {
					seen.push(g);
				}
			}
			seen.into_iter()
				.map(|g| Node::Folder { id: format!("{ROOT}/{g}"), name: g })
				.collect()
		}
		Some(wanted) => entries
			.iter()
			.filter(|e| group(e) == wanted)
			.map(|e| Node::Station(station(e)))
			.collect(),
	}
}

impl Rstreams {
	pub fn new(url: String) -> Rstreams {
		Rstreams { url, read: Mutex::new(None), dir: RwLock::new(None) }
	}

	pub fn set_dir(&self, dir: PathBuf) {
		if let Ok(mut d) = self.dir.write() {
			*d = Some(dir);
		}
	}

	fn dir(&self) -> Option<PathBuf> {
		self.dir.read().ok().and_then(|d| d.clone())
	}

	/// The copy kept by an earlier run, if any.
	fn saved(&self) -> Option<(Option<String>, Vec<PlaylistEntry>)> {
		let dir = self.dir()?;
		let body = std::fs::read_to_string(dir.join(FILE)).ok()?;
		let tag = std::fs::read_to_string(dir.join(TAG_FILE)).ok().filter(|t| !t.is_empty());
		Some((tag, parse_m3u(&body)))
	}

	fn save(&self, body: &str, tag: Option<&str>) {
		if let Some(dir) = self.dir() {
			let _ = std::fs::create_dir_all(&dir);
			let _ = std::fs::write(dir.join(FILE), body);
			let _ = std::fs::write(dir.join(TAG_FILE), tag.unwrap_or_default());
		}
	}

	async fn entries(&self, client: &reqwest::Client) -> Result<Vec<PlaylistEntry>> {
		let held = self.read.lock().ok().and_then(|r| r.clone());
		if let Some((at, _, entries)) = &held
			&& at.elapsed() < RECHECK
		{
			return Ok(entries.clone());
		}
		let (tag, entries) = match held {
			Some((_, tag, entries)) => (tag, Some(entries)),
			None => self.saved().map_or((None, None), |(t, e)| (t, Some(e))),
		};
		// Only a copy we hold may be asked for by tag: without one a 304 leaves nothing.
		let ask = entries.as_ref().and(tag.as_deref());
		let (tag, entries) = match get_if_changed(client, &self.url, ask).await {
			Ok(Some((body, new_tag))) => {
				let body = String::from_utf8_lossy(&body).into_owned();
				self.save(&body, new_tag.as_deref());
				(new_tag, parse_m3u(&body))
			}
			Ok(None) => (tag, entries.unwrap_or_default()),
			// The server is down: the copy we have still plays.
			Err(e) => match entries {
				Some(entries) => (tag, entries),
				None => return Err(e),
			},
		};
		if let Ok(mut read) = self.read.lock() {
			*read = Some((Instant::now(), tag, entries.clone()));
		}
		Ok(entries)
	}

	pub async fn children(&self, client: &reqwest::Client, id: &str) -> Result<Vec<Node>> {
		Ok(nodes(&self.entries(client).await?, id))
	}

	/// Name search over the kept copy, downloaded again only when it changed.
	pub async fn search(&self, client: &reqwest::Client, name: &str) -> Result<Vec<Station>> {
		let name = name.trim().to_lowercase();
		Ok(self.entries(client).await?
			.iter()
			.filter(|e| e.title.as_deref().is_some_and(|t| t.to_lowercase().contains(&name)))
			.map(station)
			.collect())
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn groups_browse_as_folders_in_file_order() {
		let entries = parse_m3u(
			"#EXTM3U\n#EXTINF:-1 group-title=\"Akademickie\",Afera\nhttp://radio.afera.com.pl/afera128.mp3\n\
			#EXTINF:-1 group-title=\"Lokalne\",Fama Kielce\nhttp://fama.example/kielce\n\
			#EXTINF:-1 group-title=\"Akademickie\",Akadera\nhttp://stream4.nadaje.com:11846/;\n",
		);
		let names = |nodes: Vec<Node>| nodes.iter().map(|n| n.name().to_owned()).collect::<Vec<_>>();
		assert_eq!(names(nodes(&entries, ROOT)), ["Akademickie", "Lokalne"]);
		let akademickie = nodes(&entries, "rstreams/Akademickie");
		assert_eq!(names(akademickie.clone()), ["Afera", "Akadera"]);
		let Node::Station(s) = &akademickie[0] else { panic!("{akademickie:?}") };
		assert_eq!(s.id, "rstreams:http://radio.afera.com.pl/afera128.mp3");
		assert_eq!(s.source.label(), "Polska");
	}
}
