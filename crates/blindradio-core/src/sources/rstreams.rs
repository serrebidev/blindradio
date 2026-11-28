//! Polska, from rstreams.ovh: licensed Polish radio stations on air, one M3U file the
//! author updates every month or two, grouped by kind (Akademickie, Lokalne and so on).
//! Browse only, never searched: the author's server is small, so the file is read when
//! someone opens it, and kept for the session.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::Node;
use crate::Result;
use crate::net::http_err;
use crate::playlist::{PlaylistEntry, parse_m3u};
use crate::station::{Source, Station};

pub const ROOT: &str = "rstreams";
pub const NAME: &str = "Polska (rstreams.ovh)";
/// A file updated monthly is read again after this, not on every folder opened.
const KEEP: Duration = Duration::from_secs(6 * 60 * 60);

pub struct Rstreams {
	url: String,
	read: Mutex<Option<(Instant, Vec<PlaylistEntry>)>>,
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
		Rstreams { url, read: Mutex::new(None) }
	}

	async fn entries(&self, client: &reqwest::Client) -> Result<Vec<PlaylistEntry>> {
		if let Ok(read) = self.read.lock()
			&& let Some((at, entries)) = read.as_ref()
			&& at.elapsed() < KEEP
		{
			return Ok(entries.clone());
		}
		let body = client
			.get(&self.url)
			.header("User-Agent", "Blind-Radio/1")
			.send()
			.await
			.and_then(|r| r.error_for_status())
			.map_err(http_err)?
			.text()
			.await
			.map_err(http_err)?;
		let entries = parse_m3u(&body);
		if let Ok(mut read) = self.read.lock() {
			*read = Some((Instant::now(), entries.clone()));
		}
		Ok(entries)
	}

	pub async fn children(&self, client: &reqwest::Client, id: &str) -> Result<Vec<Node>> {
		Ok(nodes(&self.entries(client).await?, id))
	}

	/// Name search over the kept copy: one download serves every search for `KEEP`.
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
