use std::path::Path;

use crate::library::{CUSTOM, Folder, Library};
use crate::playlist::{PlaylistKind, parse_m3u_counted, parse_pls_counted, sniff};
use crate::station::{Source, Station};
use crate::{CoreError, Result, net};

pub const STARTER_PLAYLIST_URL: &str = "https://serrebiradio.com/radios.m3u";
pub const STARTER_COLLECTION: &str = "SerrebiRadio";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportTarget {
	Custom,
	/// A folder with this name inside Custom Stations, created if missing. File
	/// imports pass the file stem: `Public_Radio.m3u` makes a `Public_Radio` folder.
	CustomFolder(String),
	/// The collection with this name (any case), created if missing. Import as collection
	/// passes the file stem: `My Stations.m3u` makes `My Stations`.
	Collection(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImportReport {
	pub added: usize,
	pub had: usize,
	pub skipped: usize,
}

impl ImportReport {
	pub fn message(&self) -> String {
		format!(
			"Added {}. Had {}. Skipped {}.",
			self.added, self.had, self.skipped
		)
	}
}

/// UTF-8 (BOM stripped), else Latin-1.
fn decode(bytes: Vec<u8>) -> String {
	match String::from_utf8(bytes) {
		Ok(s) => s.trim_start_matches('\u{feff}').to_string(),
		Err(e) => e.into_bytes().into_iter().map(char::from).collect(),
	}
}

pub fn import_playlist(
	lib: &mut Library,
	path: &Path,
	target: ImportTarget,
) -> Result<ImportReport> {
	let body = decode(std::fs::read(path)?);
	let ext = path
		.extension()
		.and_then(|e| e.to_str())
		.map(str::to_ascii_lowercase);
	let kind = match ext.as_deref() {
		Some("pls") => PlaylistKind::Pls,
		Some("m3u" | "m3u8") => PlaylistKind::M3u,
		_ => match sniff(&body) {
			Some(PlaylistKind::Hls) | None => {
				return Err(CoreError::Playlist("Not a playlist".into()));
			}
			Some(k) => k,
		},
	};
	// A file imported into Custom Stations keeps its own name: everything lands in a
	// folder named after the file, so several playlists never lump together.
	let target = match target {
		ImportTarget::Custom => match path.file_stem().and_then(|s| s.to_str()) {
			Some(stem) if !stem.is_empty() => ImportTarget::CustomFolder(stem.to_string()),
			_ => ImportTarget::Custom,
		},
		t => t,
	};
	Ok(import_playlist_text(lib, &body, Some(kind), target))
}

/// Where imported stations land: a collection root, or one folder inside a
/// collection. Groups inside the playlist become folders under the root.
enum Root<'a> {
	Collection(&'a mut crate::library::Collection),
	Folder(&'a mut Folder),
}

impl Root<'_> {
	fn stations(&mut self) -> &mut Vec<Station> {
		match self {
			Root::Collection(c) => &mut c.stations,
			Root::Folder(f) => &mut f.stations,
		}
	}

	fn child(&mut self, name: &str) -> &mut Folder {
		match self {
			Root::Collection(c) => c.child_mut(name),
			Root::Folder(f) => f.child_mut(name),
		}
	}
}

/// `kind_hint` None sniffs the body; anything but PLS is read as M3U. A URL already in the
/// target collection counts as had. New stations are `custom:` stations.
///
/// Groups become folders. An M3U `group-title` names the group its entry belongs to;
/// exporters that tag only the first entry of a block rely on the group running on
/// until the next tag, so an untagged entry joins the group in progress. Entries
/// before any tag stay at the root. Entry order is kept throughout.
pub fn import_playlist_text(
	lib: &mut Library,
	body: &str,
	kind_hint: Option<PlaylistKind>,
	target: ImportTarget,
) -> ImportReport {
	let (entries, skipped) = match kind_hint.or_else(|| sniff(body)) {
		Some(PlaylistKind::Pls) => parse_pls_counted(body),
		_ => parse_m3u_counted(body),
	};
	let mut report = ImportReport {
		added: 0,
		had: 0,
		skipped,
	};
	let (target_id, wrap) = match target {
		ImportTarget::Custom => {
			lib.ensure_custom();
			(CUSTOM.to_string(), None)
		}
		ImportTarget::CustomFolder(name) => {
			lib.ensure_custom();
			(CUSTOM.to_string(), Some(name))
		}
		ImportTarget::Collection(name) => (lib.create_collection(&name), None),
	};
	let Some(c) = lib.collections.iter_mut().find(|c| c.id == target_id) else {
		return report;
	};
	// Dedupe spans the whole collection, folders included.
	let mut seen: std::collections::HashSet<String> =
		c.all_stations().iter().map(|s| s.url.clone()).collect();
	let mut root = match wrap {
		Some(name) => Root::Folder(c.child_mut(&name)),
		None => Root::Collection(c),
	};
	let mut group: Option<String> = None;
	for e in entries {
		if e.group.is_some() {
			group = e.group.clone();
		}
		if !seen.insert(e.url.clone()) {
			report.had += 1;
			continue;
		}
		let station = Station {
			id: format!("custom:{}", uuid::Uuid::new_v4()),
			name: e.title.unwrap_or_else(|| e.url.clone()),
			url: e.url,
			source: Source::Custom,
			info: e.group,
		};
		match &group {
			Some(g) => root.child(g).stations.push(station),
			None => root.stations().push(station),
		}
		report.added += 1;
	}
	report
}

/// The SerrebiRadio starter list, as text.
pub async fn fetch_starter(client: &reqwest::Client) -> Result<String> {
	fetch_playlist(client, STARTER_PLAYLIST_URL).await
}

/// A playlist body, decoded like a file; 8 s in all.
pub async fn fetch_playlist(client: &reqwest::Client, url: &str) -> Result<String> {
	Ok(decode(net::get_bytes(client, url).await?.to_vec()))
}

/// Into the SerrebiRadio collection; running it again adds only new URLs.
pub fn import_starter(lib: &mut Library, body: &str) -> ImportReport {
	import_playlist_text(
		lib,
		body,
		Some(PlaylistKind::M3u),
		ImportTarget::Collection(STARTER_COLLECTION.into()),
	)
}

pub fn export_m3u(stations: &[Station], path: &Path) -> Result<()> {
	let mut out = String::from("#EXTM3U\n");
	for s in stations {
		write_entry(&mut out, s, None);
	}
	std::fs::write(path, out)?;
	Ok(())
}

/// Root stations plus folders, each folder's stations tagged with its name as
/// `group-title`, so re-importing rebuilds the same folders.
pub fn export_m3u_tree(stations: &[Station], folders: &[Folder], path: &Path) -> Result<()> {
	let mut out = String::from("#EXTM3U\n");
	for s in stations {
		write_entry(&mut out, s, None);
	}
	fn walk(out: &mut String, folders: &[Folder]) {
		for f in folders {
			for s in &f.stations {
				write_entry(out, s, Some(&f.name));
			}
			walk(out, &f.folders);
		}
	}
	walk(&mut out, folders);
	std::fs::write(path, out)?;
	Ok(())
}

fn write_entry(out: &mut String, s: &Station, group: Option<&str>) {
	// A newline in a name would split the EXTINF line.
	let name = s.name.replace(['\r', '\n'], " ");
	match group {
		Some(g) => {
			let g = g.replace('"', "'");
			out.push_str(&format!("#EXTINF:-1 group-title=\"{g}\",{name}\n{}\n", s.url));
		}
		None => out.push_str(&format!("#EXTINF:-1,{name}\n{}\n", s.url)),
	}
}
