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
		Some("m3u" | "m3u8" | "m3u_plus") => PlaylistKind::M3u,
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
/// a group holding `GROUP_PATH_SEP` is a folder path and builds nested folders.
/// Exporters that tag only the first entry of a block rely on the group running on
/// until the next tag, so an untagged entry joins the group in progress. Entries
/// before any tag stay at the root. Entry order is kept throughout.
pub fn import_playlist_text(
	lib: &mut Library,
	body: &str,
	kind_hint: Option<PlaylistKind>,
	target: ImportTarget,
) -> ImportReport {
	import_playlists(
		lib,
		vec![PlaylistFile {
			path: Vec::new(),
			body: body.to_string(),
			kind: kind_hint,
		}],
		target,
	)
}

/// One playlist read from a directory tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistFile {
	/// The folder its stations land in, under the import target: the
	/// subdirectories it sat in, then its own file stem.
	pub path: Vec<String>,
	pub body: String,
	/// None sniffs the body.
	pub kind: Option<PlaylistKind>,
}

impl PlaylistFile {
	/// A playlist file by name: `dirs` above it, its stem last in `path`.
	/// None when the extension is no playlist's.
	pub fn new(mut dirs: Vec<String>, file_name: &str, body: String) -> Option<PlaylistFile> {
		let (stem, ext) = file_name.rsplit_once('.')?;
		let kind = match ext.to_ascii_lowercase().as_str() {
			"pls" => PlaylistKind::Pls,
			"m3u" | "m3u8" | "m3u_plus" => PlaylistKind::M3u,
			_ => return None,
		};
		if !stem.is_empty() {
			dirs.push(stem.to_string());
		}
		Some(PlaylistFile {
			path: dirs,
			body,
			kind: Some(kind),
		})
	}
}

/// Every playlist under `dir`, subdirectories included, sorted by path so
/// folders come in name order. Other files are passed over.
pub fn read_playlist_dir(dir: &Path) -> Result<Vec<PlaylistFile>> {
	fn walk(dir: &Path, trail: &mut Vec<String>, out: &mut Vec<PlaylistFile>) -> Result<()> {
		for entry in std::fs::read_dir(dir)? {
			let entry = entry?;
			let name = entry.file_name().to_string_lossy().into_owned();
			if entry.file_type()?.is_dir() {
				trail.push(name);
				walk(&entry.path(), trail, out)?;
				trail.pop();
			} else if let Some(f) = PlaylistFile::new(trail.clone(), &name, String::new()) {
				out.push(PlaylistFile {
					body: decode(std::fs::read(entry.path())?),
					..f
				});
			}
		}
		Ok(())
	}
	let mut out = Vec::new();
	walk(dir, &mut Vec::new(), &mut out)?;
	out.sort_by(|a, b| a.path.cmp(&b.path));
	Ok(out)
}

/// A whole directory of playlists: each file becomes a folder named after
/// it, each subdirectory a folder holding its files' folders. Into
/// Custom Stations, a folder named after the directory wraps them all;
/// as a collection, the collection is named after the directory.
pub fn import_playlist_dir(
	lib: &mut Library,
	dir: &Path,
	as_collection: bool,
) -> Result<ImportReport> {
	let files = read_playlist_dir(dir)?;
	if files.is_empty() {
		return Err(CoreError::Playlist("No playlists".into()));
	}
	let name = dir
		.file_name()
		.and_then(|n| n.to_str())
		.filter(|n| !n.is_empty())
		.unwrap_or("Imported")
		.to_string();
	let target = match as_collection {
		true => ImportTarget::Collection(name),
		false => ImportTarget::CustomFolder(name),
	};
	Ok(import_playlists(lib, files, target))
}

/// Several playlists into one target, each under its own `path`. Dedupe
/// spans the whole collection and every file: a URL lands once.
pub fn import_playlists(
	lib: &mut Library,
	files: Vec<PlaylistFile>,
	target: ImportTarget,
) -> ImportReport {
	let mut report = ImportReport {
		added: 0,
		had: 0,
		skipped: 0,
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
	let mut order = Vec::new();
	for file in files {
		let (entries, skipped) = match file.kind.or_else(|| sniff(&file.body)) {
			Some(PlaylistKind::Pls) => parse_pls_counted(&file.body),
			_ => parse_m3u_counted(&file.body),
		};
		report.skipped += skipped;
		order.extend(entries.iter().map(|entry| entry.url.clone()));
		let mut path = wrap.iter().chain(&file.path).filter(|s| !s.is_empty());
		let mut root = match path.next() {
			None => Root::Collection(&mut *c),
			Some(first) => {
				let mut f = c.child_mut(first);
				for seg in path {
					f = f.child_mut(seg);
				}
				Root::Folder(f)
			}
		};
		add_entries(&mut root, &mut seen, entries, &mut report);
	}
	let stations = c.all_stations();
	let mut by_url: std::collections::HashMap<_, _> = stations.iter()
		.map(|station| (station.url.as_str(), station.id.as_str()))
		.collect();
	let mut ordered: Vec<_> = order.iter()
		.filter_map(|url| by_url.remove(url.as_str()).map(str::to_owned))
		.collect();
	let mentioned: std::collections::HashSet<_> = ordered.iter().cloned().collect();
	ordered.extend(c.playlist_order.iter().filter(|id| !mentioned.contains(*id)).cloned());
	c.playlist_order = ordered;
	report
}

fn add_entries(
	root: &mut Root,
	seen: &mut std::collections::HashSet<String>,
	entries: Vec<crate::playlist::PlaylistEntry>,
	report: &mut ImportReport,
) {
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
			details: Default::default(),
		};
		match &group {
			Some(g) => {
				// A group path builds its folder chain, segment by segment.
				let mut folder: Option<&mut Folder> = None;
				for seg in g
					.split(GROUP_PATH_SEP)
					.map(str::trim)
					.filter(|s| !s.is_empty())
				{
					folder = Some(match folder.take() {
						None => root.child(seg),
						Some(f) => f.child_mut(seg),
					});
				}
				match folder {
					Some(f) => f.stations.push(station),
					None => root.stations().push(station),
				}
			}
			None => root.stations().push(station),
		}
		report.added += 1;
	}
}

/// The SerrebiRadio starter list, as text.
pub async fn fetch_starter(client: &reqwest::Client) -> Result<String> {
	fetch_playlist(client, STARTER_PLAYLIST_URL).await
}

/// The SerrebiRadio list only when it changed since the copy whose ETag is `tag`: None when
/// it did not. The body comes with its new ETag, to keep for the next check.
pub async fn fetch_starter_if_changed(client: &reqwest::Client, tag: Option<&str>) -> Result<Option<(String, Option<String>)>> {
	Ok(net::get_if_changed(client, STARTER_PLAYLIST_URL, tag)
		.await?
		.map(|(body, tag)| (decode(body.to_vec()), tag)))
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

/// How a folder path is written into a `group-title`: segments joined by
/// `" / "`, so nested folders survive the round trip and re-importing
/// rebuilds the same tree. A folder whose own name holds a bare `/`
/// (Top 40/Pop) is untouched — only the spaced separator splits.
pub const GROUP_PATH_SEP: &str = " / ";

/// Root stations plus folders, each folder's stations tagged with the
/// folder's full path as `group-title`, so re-importing rebuilds the
/// same folders, nested ones included.
pub fn export_m3u_tree(stations: &[Station], folders: &[Folder], path: &Path) -> Result<()> {
	std::fs::write(path, m3u_tree_text(stations, folders))?;
	Ok(())
}

/// The text [`export_m3u_tree`] writes.
pub fn m3u_tree_text(stations: &[Station], folders: &[Folder]) -> String {
	let mut out = String::from("#EXTM3U\n");
	for s in stations {
		write_entry(&mut out, s, None);
	}
	fn walk(out: &mut String, folders: &[Folder], trail: &mut Vec<String>) {
		for f in folders {
			trail.push(f.name.clone());
			let group = trail.join(GROUP_PATH_SEP);
			for s in &f.stations {
				write_entry(out, s, Some(&group));
			}
			walk(out, &f.folders, trail);
			trail.pop();
		}
	}
	walk(&mut out, folders, &mut Vec::new());
	out
}

/// A name made safe as a file or directory name on every OS.
fn safe_file_name(name: &str) -> String {
	let safe: String = name
		.chars()
		.map(|c| match c {
			'<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
			c if c.is_control() => '_',
			c => c,
		})
		.collect();
	let safe = safe.trim().trim_end_matches('.');
	match safe.is_empty() {
		true => "Playlist".to_string(),
		false => safe.to_string(),
	}
}

/// A list as a directory of playlists, the shape [`import_playlist_dir`]
/// reads back: each top-level folder its own `.m3u` (its subfolders as
/// groups), loose stations in `<name>.m3u`. File names safe and unique
/// (`Jazz (2).m3u`).
pub fn playlist_dir_files(
	name: &str,
	stations: &[Station],
	folders: &[Folder],
) -> Vec<(String, String)> {
	let mut taken = std::collections::HashSet::new();
	let mut file = |name: &str| {
		let safe = safe_file_name(name);
		(1..)
			.map(|n| match n {
				1 => safe.clone(),
				n => format!("{safe} ({n})"),
			})
			.find(|c| taken.insert(c.to_lowercase()))
			.map(|c| format!("{c}.m3u"))
			.unwrap_or_default()
	};
	let mut out = Vec::new();
	if !stations.is_empty() {
		out.push((file(name), m3u_tree_text(stations, &[])));
	}
	for f in folders {
		out.push((file(&f.name), m3u_tree_text(&f.stations, &f.folders)));
	}
	out
}

/// Writes [`playlist_dir_files`] into a new directory `<name>` under
/// `parent`, `<name> (2)` when taken: nothing is overwritten. Gives the
/// directory made and how many stations went.
pub fn export_playlist_dir(
	parent: &Path,
	name: &str,
	stations: &[Station],
	folders: &[Folder],
) -> Result<(std::path::PathBuf, usize)> {
	let base = safe_file_name(name);
	let mut n = 1;
	let dir = loop {
		let dir = parent.join(match n {
			1 => base.clone(),
			n => format!("{base} ({n})"),
		});
		match std::fs::create_dir(&dir) {
			Ok(()) => break dir,
			Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => n += 1,
			Err(e) => return Err(e.into()),
		}
	};
	for (file, text) in playlist_dir_files(name, stations, folders) {
		std::fs::write(dir.join(file), text)?;
	}
	let count = stations.len()
		+ folders
			.iter()
			.map(|f| f.all_stations().len())
			.sum::<usize>();
	Ok((dir, count))
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

#[cfg(test)]
#[test]
fn reimport_restores_interleaved_playlist_order_without_moving_stations() {
	let mut library = Library::default();
	let body = "#EXTM3U\n#EXTINF:-1 group-title=\"A\",One\nhttp://one\n\
		#EXTINF:-1 group-title=\"B\",Two\nhttp://two\n\
		#EXTINF:-1 group-title=\"A\",Three\nhttp://three\n";
	import_starter(&mut library, body);
	let collection = library.collections.iter_mut().find(|c| c.name == STARTER_COLLECTION).unwrap();
	collection.playlist_order.clear();
	collection.flat_view = Some(true);
	let before = collection.all_stations();
	assert_eq!(before.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["One", "Three", "Two"]);
	let report = import_starter(&mut library, body);
	assert_eq!((report.added, report.had), (0, 3));
	let collection = library.collections.iter().find(|c| c.name == STARTER_COLLECTION).unwrap();
	assert_eq!(collection.flat_view, Some(true));
	assert_eq!(collection.all_stations(), before);
	assert_eq!(collection.flat_indices(&before), [0, 2, 1]);
	assert_eq!(collection.station_location(2), Some((vec![1], 0)));
}
