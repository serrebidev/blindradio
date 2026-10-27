use std::path::Path;

use crate::library::{CUSTOM, Library};
use crate::playlist::{PlaylistKind, parse_m3u_counted, parse_pls_counted, sniff};
use crate::station::{Source, Station};
use crate::{CoreError, Result, net};

pub const STARTER_PLAYLIST_URL: &str = "https://serrebiradio.com/radios.m3u";
pub const STARTER_COLLECTION: &str = "SerrebiRadio";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportTarget {
	Custom,
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
	Ok(import_playlist_text(lib, &body, Some(kind), target))
}

/// `kind_hint` None sniffs the body; anything but PLS is read as M3U. A URL already in the
/// target counts as had. New stations are `custom:` stations; an M3U `group-title` is their info.
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
	let target_id = match target {
		ImportTarget::Custom => {
			lib.ensure_custom();
			CUSTOM.to_string()
		}
		ImportTarget::Collection(name) => lib.create_collection(&name),
	};
	let Some(c) = lib.collections.iter_mut().find(|c| c.id == target_id) else {
		return report;
	};
	for e in entries {
		if c.stations.iter().any(|s| s.url == e.url) {
			report.had += 1;
			continue;
		}
		c.stations.push(Station {
			id: format!("custom:{}", uuid::Uuid::new_v4()),
			name: e.title.unwrap_or_else(|| e.url.clone()),
			url: e.url,
			source: Source::Custom,
			info: e.group,
		});
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
		// A newline in a name would split the EXTINF line.
		let name = s.name.replace(['\r', '\n'], " ");
		out.push_str(&format!("#EXTINF:-1,{name}\n{}\n", s.url));
	}
	std::fs::write(path, out)?;
	Ok(())
}
