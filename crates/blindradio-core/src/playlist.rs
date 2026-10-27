#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistEntry {
	pub url: String,
	pub title: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaylistKind {
	Pls,
	M3u,
	Hls,
}

fn lines(body: &str) -> impl Iterator<Item = &str> {
	body.trim_start_matches('\u{feff}')
		.lines()
		.map(str::trim)
		.filter(|l| !l.is_empty())
}

fn is_url(s: &str) -> bool {
	let prefix = |p: &str| s.get(..p.len()).is_some_and(|h| h.eq_ignore_ascii_case(p));
	prefix("http://") || prefix("https://")
}

/// The title after the first comma outside double quotes: attributes like `tvg-name="a, b"` may hold commas.
fn extinf_title(rest: &str) -> Option<String> {
	let mut quoted = false;
	let comma = rest.char_indices().find_map(|(i, c)| match c {
		'"' => {
			quoted = !quoted;
			None
		}
		',' if !quoted => Some(i),
		_ => None,
	})?;
	Some(rest[comma + 1..].trim().to_string()).filter(|t| !t.is_empty())
}

pub fn sniff(body: &str) -> Option<PlaylistKind> {
	let mut m3u = false;
	for l in lines(body) {
		if l.eq_ignore_ascii_case("[playlist]") {
			return Some(PlaylistKind::Pls);
		}
		if l.starts_with("#EXT-X-") {
			return Some(PlaylistKind::Hls);
		}
		if l.starts_with("#EXTM3U") || is_url(l) {
			m3u = true;
		}
	}
	m3u.then_some(PlaylistKind::M3u)
}

pub fn parse_pls(body: &str) -> Vec<PlaylistEntry> {
	use std::collections::BTreeMap;
	let mut files = BTreeMap::new();
	let mut titles = BTreeMap::new();
	for l in lines(body) {
		let Some((key, val)) = l.split_once('=') else {
			continue;
		};
		let key = key.trim().to_ascii_lowercase();
		let val = val.trim();
		if let Some(n) = key.strip_prefix("file").and_then(|n| n.parse::<u32>().ok()) {
			files.insert(n, val.to_string());
		} else if let Some(n) = key
			.strip_prefix("title")
			.and_then(|n| n.parse::<u32>().ok())
		{
			titles.insert(n, val.to_string());
		}
	}
	files
		.into_iter()
		.filter(|(_, url)| !url.is_empty())
		.map(|(n, url)| PlaylistEntry {
			url,
			title: titles.remove(&n).filter(|t| !t.is_empty()),
		})
		.collect()
}

pub fn parse_m3u(body: &str) -> Vec<PlaylistEntry> {
	let mut out = Vec::new();
	let mut title = None;
	for l in lines(body) {
		if let Some(rest) = l.strip_prefix("#EXTINF:") {
			title = extinf_title(rest);
		} else if l.starts_with('#') {
			// other directives do not consume the pending title
		} else {
			// Any non-directive line consumes the pending title, even a skipped local file.
			let title = title.take();
			if is_url(l) {
				out.push(PlaylistEntry {
					url: l.to_string(),
					title,
				});
			}
		}
	}
	out
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn sniff_detects_each_kind() {
		assert_eq!(sniff("[playlist]\nFile1=http://a"), Some(PlaylistKind::Pls));
		assert_eq!(sniff("[PlayList]\n"), Some(PlaylistKind::Pls));
		assert_eq!(
			sniff("#EXTM3U\n#EXT-X-TARGETDURATION:10\n"),
			Some(PlaylistKind::Hls)
		);
		assert_eq!(
			sniff("#EXTM3U\n#EXTINF:-1,A\nhttp://a"),
			Some(PlaylistKind::M3u)
		);
		assert_eq!(sniff("https://a.example/stream"), Some(PlaylistKind::M3u));
		assert_eq!(sniff("<html></html>"), None);
	}

	#[test]
	fn pls_pairs_titles_by_number() {
		let body = "[playlist]\nFile2=http://b\nTitle2=Two\nFile1=http://a\nTitle1=One\nNumberOfEntries=2\n";
		let e = parse_pls(body);
		assert_eq!(e.len(), 2);
		assert_eq!(
			(e[0].url.as_str(), e[0].title.as_deref()),
			("http://a", Some("One"))
		);
		assert_eq!(
			(e[1].url.as_str(), e[1].title.as_deref()),
			("http://b", Some("Two"))
		);
		let e = parse_pls("[playlist]\nfile1 = http://c\n");
		assert_eq!(e[0].url, "http://c");
		assert_eq!(e[0].title, None);
	}

	#[test]
	fn m3u_takes_extinf_titles_and_skips_local_files() {
		let body = "#EXTM3U\n#EXTINF:-1,Jazz\nhttp://a\nC:\\music\\a.mp3\n#EXTINF:-1,Lost\nsong.mp3\nhttps://b\n";
		let e = parse_m3u(body);
		assert_eq!(e.len(), 2);
		assert_eq!(
			(e[0].url.as_str(), e[0].title.as_deref()),
			("http://a", Some("Jazz"))
		);
		assert_eq!(
			(e[1].url.as_str(), e[1].title.as_deref()),
			("https://b", None)
		);
	}

	#[test]
	fn m3u_title_after_quoted_comma_and_uppercase_scheme() {
		let body =
			"#EXTINF:-1 tvg-name=\"Jazz, Blues\" tvg-logo=\"x\",Jazz FM\nHTTP://A.example/s\n";
		let e = parse_m3u(body);
		assert_eq!(e.len(), 1);
		assert_eq!(e[0].url, "HTTP://A.example/s");
		assert_eq!(e[0].title.as_deref(), Some("Jazz FM"));
	}

	#[test]
	fn pls_skips_empty_file() {
		let e = parse_pls("[playlist]\nFile1=\nFile2=http://b\n");
		assert_eq!(e.len(), 1);
		assert_eq!(e[0].url, "http://b");
	}

	#[test]
	fn m3u_handles_bom_and_crlf() {
		let e = parse_m3u("\u{feff}#EXTM3U\r\n\r\n#EXTINF:-1,T\r\nhttp://a\r\n");
		assert_eq!(e.len(), 1);
		assert_eq!(e[0].title.as_deref(), Some("T"));
		assert_eq!(sniff("\u{feff}#EXTM3U\r\n"), Some(PlaylistKind::M3u));
	}
}
