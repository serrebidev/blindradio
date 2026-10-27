use std::path::Path;

use crate::library::{CUSTOM, Library};
use crate::playlist::{PlaylistKind, parse_m3u_counted, parse_pls_counted, sniff};
use crate::station::{Source, Station};
use crate::{CoreError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportTarget {
	Custom,
	CollectionNamedAfterFile,
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
	let (entries, skipped) = if kind == PlaylistKind::Pls {
		parse_pls_counted(&body)
	} else {
		parse_m3u_counted(&body)
	};

	let target_id = match target {
		ImportTarget::Custom => CUSTOM.to_string(),
		ImportTarget::CollectionNamedAfterFile => {
			let stem = path
				.file_stem()
				.and_then(|s| s.to_str())
				.filter(|s| !s.is_empty())
				.unwrap_or("Imported");
			lib.create_collection(stem)
		}
	};

	let mut report = ImportReport {
		added: 0,
		had: 0,
		skipped,
	};
	for e in entries {
		let name = e.title.as_deref().unwrap_or(&e.url);
		let has = |lib: &Library| {
			lib.collections
				.iter()
				.find(|c| c.id == target_id)
				.is_some_and(|c| c.stations.iter().any(|s| s.url == e.url))
		};
		if has(lib) {
			report.had += 1;
		} else if target == ImportTarget::Custom {
			lib.add_custom(name, &e.url);
			report.added += 1;
		} else if let Some(c) = lib.collections.iter_mut().find(|c| c.id == target_id) {
			c.stations.push(Station {
				id: format!("custom:{}", uuid::Uuid::new_v4()),
				name: name.into(),
				url: e.url.clone(),
				source: Source::Custom,
				info: None,
			});
			report.added += 1;
		}
	}
	Ok(report)
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
