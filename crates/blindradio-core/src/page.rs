//! Detect Audio Stream on Page: the streams a web page names, each opened to prove it plays.
//!
//! ponytail: reads the source of the page, its frames and its own scripts; nothing runs.
//! A player that builds its URL at run time stays hidden. Upgrade path: a headless web view.

use std::collections::{HashSet, VecDeque};
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use reqwest::Url;
use scraper::{Html, Selector};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tokio::time::timeout;

use crate::net::{Opened, http_err, open};
use crate::station::{Details, Source, Station, known};
use crate::{CoreError, Result};

/// Frames and scripts read for one page, the page included.
const MAX_FETCHES: usize = 12;
const MAX_FRAMES: usize = 4;
const MAX_SCRIPTS: usize = 7;
/// A page is read this far: player addresses sit in the source, not in megabytes of data.
const MAX_BYTES: usize = 2 * 1024 * 1024;
const FETCH_DEADLINE: Duration = Duration::from_secs(10);
/// Addresses opened to see whether they play, best guesses first.
const MAX_CANDIDATES: usize = 40;
const CHECKS_AT_ONCE: usize = 8;
const CHECK_DEADLINE: Duration = Duration::from_secs(8);
const ALL_CHECKS_DEADLINE: Duration = Duration::from_secs(30);

const AUDIO_EXTENSIONS: [&str; 12] = [
	"mp3", "aac", "aacp", "m4a", "ogg", "oga", "opus", "flac", "m3u", "m3u8", "pls", "mp2",
];
const PAGE_EXTENSIONS: [&str; 19] = [
	"html", "htm", "js", "mjs", "css", "png", "jpg", "jpeg", "gif", "svg", "webp", "ico", "woff",
	"woff2", "ttf", "json", "xml", "pdf", "txt",
];
/// Words a stream's path tends to hold when no file extension says what it is.
const STREAM_PATH_WORDS: [&str; 5] = ["stream", "icecast", "shoutcast", "listen", "/live"];
/// The same for its host. Not `radio` or `live`: every link on a radio station's site has those.
const STREAM_HOST_WORDS: [&str; 3] = ["stream", "icecast", "shoutcast"];

/// How sure the address is, best first. An audio element named it, its extension says audio,
/// or it only holds a telling word.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Rank {
	Element,
	Extension,
	Word,
}

#[derive(Debug, PartialEq, Eq)]
struct Candidate {
	url: Url,
	rank: Rank,
	/// The link's own text, when a link named it.
	label: Option<String>,
}

#[derive(Debug, Default)]
struct Scan {
	title: Option<String>,
	candidates: Vec<Candidate>,
	frames: Vec<Url>,
	scripts: Vec<Url>,
}

fn is_web(url: &Url) -> bool {
	matches!(url.scheme(), "http" | "https")
}

/// A typed address may leave out its scheme.
fn page_url(text: &str) -> Result<Url> {
	let text = text.trim();
	let with_scheme = if text.contains("://") {
		text.to_owned()
	} else {
		format!("https://{text}")
	};
	Url::parse(&with_scheme)
		.ok()
		.filter(|u| is_web(u) && u.host_str().is_some())
		.ok_or_else(|| CoreError::Http("Not a web address".into()))
}

/// Loopback and private addresses: a public page has no business making this PC call them.
/// Names that resolve to such an address are not caught; no secret comes back from them
/// either, only whether they play.
pub(crate) fn is_local(url: &Url) -> bool {
	let Some(host) = url.host_str() else {
		return true;
	};
	if host.eq_ignore_ascii_case("localhost") || host.ends_with(".local") {
		return true;
	}
	match host.trim_matches(['[', ']']).parse::<IpAddr>() {
		Ok(IpAddr::V4(ip)) => {
			ip.is_loopback() || ip.is_private() || ip.is_link_local() || ip.is_unspecified()
		}
		Ok(IpAddr::V6(ip)) => {
			let first = ip.segments()[0];
			ip.is_loopback()
				|| ip.is_unspecified()
				|| first & 0xfe00 == 0xfc00
				|| first & 0xffc0 == 0xfe80
		}
		Err(_) => false,
	}
}

fn rank_of(url: &Url) -> Option<Rank> {
	if !is_web(url) {
		return None;
	}
	let path = url.path().to_ascii_lowercase();
	// SHOUTcast's own forms: `/;`, `/;stream.mp3`, `/stream;`.
	if path.ends_with("/;") || path.contains(";stream") {
		return Some(Rank::Extension);
	}
	let last = path.rsplit('/').next().unwrap_or_default();
	if let Some((_, extension)) = last.rsplit_once('.') {
		if AUDIO_EXTENSIONS.contains(&extension) {
			return Some(Rank::Extension);
		}
		if PAGE_EXTENSIONS.contains(&extension) {
			return None;
		}
	}
	let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
	let telling = STREAM_PATH_WORDS.iter().any(|word| path.contains(word))
		|| STREAM_HOST_WORDS.iter().any(|word| host.contains(word));
	telling.then_some(Rank::Word)
}

pub(crate) fn selector(css: &str) -> Selector {
	Selector::parse(css).expect("valid selector")
}

/// Text as scripts and attributes escape it, so its addresses read plainly.
fn unescape(text: &str) -> String {
	text.replace("\\/", "/")
		.replace("\\u002F", "/")
		.replace("\\u002f", "/")
		.replace("&amp;", "&")
		.replace("\\u0026", "&")
}

/// Every absolute web address in `text`, wherever it sits: scripts, JSON, comments.
fn addresses(text: &str) -> Vec<Url> {
	let text = unescape(text);
	let ends = |c: char| {
		c.is_whitespace() || matches!(c, '"' | '\'' | '`' | '<' | '>' | '\\' | ')' | ']' | '}' | ',')
	};
	let mut out = Vec::new();
	let mut rest = text.as_str();
	while let Some(at) = rest.find("http") {
		rest = &rest[at..];
		let end = rest.find(ends).unwrap_or(rest.len());
		if rest.starts_with("http://") || rest.starts_with("https://") {
			out.extend(Url::parse(&rest[..end]));
		}
		// Past the word `http` at least: the address may be junk with another inside it.
		rest = &rest[4..];
	}
	out
}

/// What one page or script names.
fn scan(body: &str, base: &Url, is_html: bool) -> Scan {
	let mut found = Scan::default();
	let mut add = |url: Url, rank: Rank, label: Option<String>| {
		found.candidates.push(Candidate { url, rank, label });
	};
	if is_html {
		let html = Html::parse_document(body);
		let join = |value: &str| base.join(&unescape(value.trim())).ok().filter(is_web);
		for element in html.select(&selector("audio[src], audio source[src], video source[src]")) {
			if let Some(url) = element.attr("src").and_then(join) {
				add(url, Rank::Element, None);
			}
		}
		for element in html.select(&selector(r#"meta[property="og:audio"], meta[property="og:audio:url"]"#)) {
			if let Some(url) = element.attr("content").and_then(join) {
				add(url, Rank::Element, None);
			}
		}
		for element in html.select(&selector("a[href]")) {
			if let Some(url) = element.attr("href").and_then(join)
				&& let Some(rank) = rank_of(&url)
			{
				let text = element.text().collect::<String>();
				let label = known(&text.split_whitespace().collect::<Vec<_>>().join(" "));
				add(url, rank, label);
			}
		}
		// Players keep their address in an attribute of their own: data-src, data-stream.
		for element in html.select(&selector("*")) {
			for (name, value) in element.value().attrs() {
				if (name == "src" || name == "data" || name == "value" || name.starts_with("data-"))
					&& element.value().name() != "script"
					&& element.value().name() != "iframe"
					&& let Some(url) = join(value)
					&& let Some(rank) = rank_of(&url)
				{
					add(url, rank, None);
				}
			}
		}
		found.title = html
			.select(&selector("title"))
			.next()
			.and_then(|t| known(&t.text().collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ")));
		found.frames = html
			.select(&selector("iframe[src], frame[src]"))
			.filter_map(|e| e.attr("src").and_then(join))
			.take(MAX_FRAMES)
			.collect();
		found.scripts = html
			.select(&selector("script[src]"))
			.filter_map(|e| e.attr("src").and_then(join))
			.filter(|u| u.host_str() == base.host_str())
			.take(MAX_SCRIPTS)
			.collect();
	}
	for url in addresses(body) {
		if let Some(rank) = rank_of(&url) {
			add(url, rank, None);
		}
	}
	found
}

/// The body when it is text a page or script is made of.
async fn fetch_text(client: &reqwest::Client, url: &Url) -> Result<Option<(String, bool)>> {
	let agent = concat!("Mozilla/5.0 (compatible; BlindRadio/", env!("CARGO_PKG_VERSION"), ")");
	let get = client.get(url.clone()).header("User-Agent", agent).send();
	let mut response = timeout(FETCH_DEADLINE, get)
		.await
		.map_err(|_| CoreError::Http("Unreachable".into()))?
		.map_err(http_err)?;
	let status = response.status().as_u16();
	if status >= 400 {
		return Err(CoreError::Http(format!("HTTP {status}")));
	}
	let kind = response
		.headers()
		.get("content-type")
		.and_then(|v| v.to_str().ok())
		.unwrap_or_default()
		.to_ascii_lowercase();
	let is_html = kind.contains("html");
	if !(kind.is_empty() || is_html || kind.starts_with("text/") || kind.contains("script") || kind.contains("json")) {
		return Ok(None);
	}
	let mut body = Vec::new();
	let deadline = tokio::time::Instant::now() + FETCH_DEADLINE;
	while body.len() < MAX_BYTES {
		match tokio::time::timeout_at(deadline, response.chunk()).await {
			Ok(Ok(Some(chunk))) => body.extend_from_slice(&chunk),
			_ => break,
		}
	}
	Ok(Some((String::from_utf8_lossy(&body).into_owned(), is_html)))
}

/// What a checked address turned out to be.
#[derive(Debug, Default, PartialEq, Eq)]
struct Sound {
	name: Option<String>,
	codec: Option<String>,
	bitrate_kbps: Option<u32>,
}

fn codec_of(content_type: &str) -> Option<&'static str> {
	Some(match content_type {
		"audio/mpeg" | "audio/mp3" => "MP3",
		"audio/aac" | "audio/aacp" | "audio/x-aac" => "AAC",
		"audio/ogg" | "application/ogg" => "Ogg",
		"audio/opus" => "Opus",
		"audio/flac" | "audio/x-flac" => "FLAC",
		"video/mp2t" => "MPEG-TS",
		_ => return None,
	})
}

/// Opens the address as a station would. `sure` trusts an unlabeled body: a word in the
/// address alone must not turn a download into a stream.
async fn check(client: &reqwest::Client, url: &Url, sure: bool) -> Option<Sound> {
	let opened = timeout(CHECK_DEADLINE, open(client, url.as_str()))
		.await
		.ok()?
		.ok()?;
	let response = match opened {
		Opened::Hls { .. } => {
			return Some(Sound {
				codec: Some("HLS".into()),
				..Sound::default()
			});
		}
		Opened::Audio { response, .. } => response,
	};
	let headers = response.headers();
	let header = |name: &str| headers.get(name).and_then(|v| v.to_str().ok()).and_then(known);
	let kind = header("content-type")
		.and_then(|v| v.split(';').next().map(|t| t.trim().to_ascii_lowercase()))
		.unwrap_or_default();
	let icy = headers.keys().any(|k| k.as_str().starts_with("icy-"));
	let plays = icy
		|| kind.starts_with("audio/")
		|| kind == "application/ogg"
		|| kind == "video/mp2t"
		|| (sure && (kind.is_empty() || kind == "application/octet-stream"));
	plays.then(|| Sound {
		name: header("icy-name").filter(|n| !n.eq_ignore_ascii_case("no name")),
		codec: codec_of(&kind).map(str::to_owned),
		bitrate_kbps: header("icy-br")
			.and_then(|v| v.split(',').next()?.trim().parse().ok())
			.filter(|&b| b > 0),
	})
}

fn station_of(found: &Candidate, sound: Sound, title: Option<&str>, page: &Url) -> Station {
	let name = sound
		.name
		.or_else(|| found.label.clone())
		.or_else(|| title.map(str::to_owned))
		.or_else(|| page.host_str().map(str::to_owned))
		.unwrap_or_else(|| found.url.to_string());
	let rate = sound.bitrate_kbps.map(|k| format!("{k} kbps"));
	let info = [sound.codec.clone(), rate]
		.into_iter()
		.flatten()
		.collect::<Vec<_>>()
		.join(", ");
	Station {
		id: format!("url:{}", found.url),
		name,
		url: found.url.to_string(),
		source: Source::Custom,
		info: known(&info),
		details: Details {
			codec: sound.codec,
			bitrate_kbps: sound.bitrate_kbps,
			homepage: Some(page.to_string()),
			..Details::default()
		},
	}
}

/// Rows that would read the same get their address's last part, to tell them apart.
fn tell_apart(stations: &mut [Station]) {
	let names: Vec<String> = stations.iter().map(|s| s.name.clone()).collect();
	for station in stations.iter_mut() {
		if names.iter().filter(|n| **n == station.name).count() > 1
			&& let Some(last) = station.url.rsplit('/').find(|p| !p.is_empty())
		{
			station.name = format!("{} - {last}", station.name);
		}
	}
}

/// Best guesses first, each address once; `http` and `https` of one address count once.
fn shortlist(mut found: Vec<Candidate>, page: &Url) -> Vec<Candidate> {
	found.sort_by_key(|c| c.rank);
	let mut seen = HashSet::new();
	found.retain(|c| {
		let plain = c.url.as_str().split_once("://").map_or("", |(_, rest)| rest);
		seen.insert(plain.to_owned()) && (is_local(page) || !is_local(&c.url))
	});
	found.truncate(MAX_CANDIDATES);
	found
}

/// The streams `page` names; an address that is itself a stream is its own answer. Fails only
/// when the page cannot be read.
pub async fn detect(client: &reqwest::Client, page: &str) -> Result<Vec<Station>> {
	let page = page_url(page)?;
	if crate::live::is_page(page.as_str()) {
		let live = crate::live::lookup(client, page.as_str()).await?;
		return Ok(vec![Station {
			id: format!("url:{page}"),
			name: live.name,
			url: page.to_string(),
			source: Source::Custom,
			info: None,
			details: Details::default(),
		}]);
	}
	let direct = Candidate {
		url: page.clone(),
		rank: Rank::Extension,
		label: None,
	};
	if let Some(sound) = check(client, &page, rank_of(&page) == Some(Rank::Extension)).await {
		return Ok(vec![station_of(&direct, sound, None, &page)]);
	}
	let mut title = None;
	let mut found = Vec::new();
	let mut queue = VecDeque::from([(page.clone(), true)]);
	let mut read = HashSet::new();
	while let Some((url, first)) = queue.pop_front() {
		if read.len() >= MAX_FETCHES || !read.insert(url.clone()) {
			continue;
		}
		if !first && !is_local(&page) && is_local(&url) {
			continue;
		}
		let text = match fetch_text(client, &url).await {
			Ok(text) => text,
			Err(e) if first => return Err(e),
			Err(_) => continue,
		};
		let Some((body, is_html)) = text else {
			continue;
		};
		let scanned = scan(&body, &url, is_html);
		if first {
			title = scanned.title;
		}
		found.extend(scanned.candidates);
		// Frames first: a player's frame is more telling than the site's scripts.
		for frame in scanned.frames.into_iter().rev() {
			queue.push_front((frame, false));
		}
		queue.extend(scanned.scripts.into_iter().map(|s| (s, false)));
	}

	let candidates = Arc::new(shortlist(found, &page));
	let permits = Arc::new(Semaphore::new(CHECKS_AT_ONCE));
	let mut checks = JoinSet::new();
	for i in 0..candidates.len() {
		let (client, candidates, permits) = (client.clone(), candidates.clone(), permits.clone());
		checks.spawn(async move {
			let _permit = permits.acquire().await.ok()?;
			let found = &candidates[i];
			let sound = check(&client, &found.url, found.rank < Rank::Word).await?;
			Some((i, sound))
		});
	}
	let deadline = tokio::time::Instant::now() + ALL_CHECKS_DEADLINE;
	let mut sounds = Vec::new();
	// What answered by the deadline is the answer; dropping the set cancels the rest.
	while let Ok(Some(done)) = tokio::time::timeout_at(deadline, checks.join_next()).await {
		sounds.extend(done.ok().flatten());
	}
	sounds.sort_by_key(|(i, _)| *i);
	let mut stations: Vec<Station> = sounds
		.into_iter()
		.map(|(i, sound)| station_of(&candidates[i], sound, title.as_deref(), &page))
		.collect();
	tell_apart(&mut stations);
	Ok(stations)
}

#[cfg(test)]
mod tests {
	use super::*;

	fn urls(scan: &Scan) -> Vec<(String, Rank)> {
		scan.candidates
			.iter()
			.map(|c| (c.url.to_string(), c.rank))
			.collect()
	}

	#[test]
	fn a_typed_address_may_leave_out_its_scheme() {
		assert_eq!(page_url(" radio.example/live ").unwrap().as_str(), "https://radio.example/live");
		assert_eq!(page_url("http://a.example/").unwrap().as_str(), "http://a.example/");
		assert!(page_url("ftp://a.example/").is_err());
		assert!(page_url("").is_err());
	}

	#[test]
	fn elements_links_attributes_and_scripts_all_name_streams() {
		let base = Url::parse("https://radio.example/player/").unwrap();
		let html = r#"<html><head><title> Jazz
			Radio </title>
			<meta property="og:audio" content="https://cdn.example/og.mp3"></head><body>
			<audio src="live.aac"></audio>
			<a href="/hi.pls">High quality</a>
			<a href="/about.html">About the stream</a>
			<div data-stream="//cast.example:8000/;"></div>
			<img src="/stream/logo.png">
			<iframe src="https://embed.example/player"></iframe>
			<script src="/app.js"></script>
			<script src="https://other.example/x.js"></script>
			<script>var config = {"hls":"https:\/\/cdn.example\/a\/chunklist.m3u8?x=1&amp;y=2","site":"https://radio.example/"};</script>
			</body></html>"#;
		let found = scan(html, &base, true);
		assert_eq!(found.title.as_deref(), Some("Jazz Radio"));
		let got = urls(&found);
		for (url, rank) in [
			("https://radio.example/player/live.aac", Rank::Element),
			("https://cdn.example/og.mp3", Rank::Element),
			("https://radio.example/hi.pls", Rank::Extension),
			("https://cast.example:8000/;", Rank::Extension),
			("https://cdn.example/a/chunklist.m3u8?x=1&y=2", Rank::Extension),
		] {
			assert!(got.contains(&(url.into(), rank)), "{url} in {got:?}");
		}
		assert!(
			!got.iter().any(|(u, _)| u.contains("about") || u.contains("logo")),
			"pages and pictures are not streams: {got:?}"
		);
		let label = found
			.candidates
			.iter()
			.find(|c| c.url.path() == "/hi.pls")
			.and_then(|c| c.label.as_deref());
		assert_eq!(label, Some("High quality"));
		assert_eq!(found.frames, [Url::parse("https://embed.example/player").unwrap()]);
		assert_eq!(found.scripts, [Url::parse("https://radio.example/app.js").unwrap()]);
	}

	#[test]
	fn a_telling_word_is_the_weakest_guess() {
		let rank = |u: &str| rank_of(&Url::parse(u).unwrap());
		assert_eq!(rank("https://a.example/stream"), Some(Rank::Word));
		assert_eq!(rank("https://icecast.a.example:8000/x"), Some(Rank::Word));
		assert_eq!(rank("https://a.example/x.MP3"), Some(Rank::Extension));
		assert_eq!(rank("http://a.example:8000/;stream.nsv"), Some(Rank::Extension));
		assert_eq!(rank("https://a.example/stream.css"), None);
		assert_eq!(rank("https://a.example/news"), None);
		assert_eq!(rank("https://liveradio.example/about"), None, "only the site's name");
		assert_eq!(rank("ftp://a.example/x.mp3"), None);
	}

	#[test]
	fn a_public_page_cannot_send_checks_to_this_network() {
		let candidate = |u: &str| Candidate {
			url: Url::parse(u).unwrap(),
			rank: Rank::Extension,
			label: None,
		};
		let list = || {
			vec![
				candidate("http://192.168.1.1/x.mp3"),
				candidate("http://localhost/x.mp3"),
				candidate("http://[::1]/x.mp3"),
				candidate("http://a.example/x.mp3"),
				candidate("https://a.example/x.mp3"),
			]
		};
		let public = Url::parse("https://radio.example/").unwrap();
		let kept = shortlist(list(), &public);
		assert_eq!(kept.len(), 1, "{kept:?}");
		assert_eq!(kept[0].url.as_str(), "http://a.example/x.mp3");
		let local = Url::parse("http://192.168.1.9/").unwrap();
		assert_eq!(shortlist(list(), &local).len(), 4, "a local page may name local streams");
	}

	#[test]
	fn rows_that_read_the_same_are_told_apart() {
		let station = |url: &str, name: &str| Station {
			id: url.into(),
			name: name.into(),
			url: url.into(),
			source: Source::Custom,
			info: None,
			details: Details::default(),
		};
		let mut rows = [
			station("http://a/hi.mp3", "Jazz"),
			station("http://a/lo.mp3", "Jazz"),
			station("http://a/x.mp3", "Blues"),
		];
		tell_apart(&mut rows);
		assert_eq!(rows[0].name, "Jazz - hi.mp3");
		assert_eq!(rows[1].name, "Jazz - lo.mp3");
		assert_eq!(rows[2].name, "Blues");
	}
}
