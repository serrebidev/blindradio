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

/// Frames, players and scripts read for one page, the page included.
const MAX_FETCHES: usize = 16;
const MAX_FRAMES: usize = 4;
/// Listen pages the typed page links to, read one level deep.
const MAX_PLAYERS: usize = 3;
const MAX_SCRIPTS: usize = 7;
/// Mounts taken from one Triton station's list.
const MAX_TRITON_MOUNTS: usize = 8;
const TRITON: &str = "https://playerservices.streamtheworld.com/api/";
/// A page is read this far: player addresses sit in the source, not in megabytes of data.
const MAX_BYTES: usize = 2 * 1024 * 1024;
const FETCH_DEADLINE: Duration = Duration::from_secs(10);
/// Addresses opened to see whether they play, best guesses first.
const MAX_CANDIDATES: usize = 40;
const CHECKS_AT_ONCE: usize = 8;
const CHECK_DEADLINE: Duration = Duration::from_secs(8);
const ALL_CHECKS_DEADLINE: Duration = Duration::from_secs(30);
/// The whole search. A YouTube lookup that fails waits on a yt-dlp update with a 10 minute
/// download limit; nobody waits that long on a silent dialog.
const DETECT_DEADLINE: Duration = Duration::from_secs(90);

const AUDIO_EXTENSIONS: [&str; 13] = [
	"mp3", "aac", "aacp", "m4a", "ogg", "oga", "opus", "flac", "m3u", "m3u8", "pls", "mp2", "ts",
];
const PAGE_EXTENSIONS: [&str; 19] = [
	"html", "htm", "js", "mjs", "css", "png", "jpg", "jpeg", "gif", "svg", "webp", "ico", "woff",
	"woff2", "ttf", "json", "xml", "pdf", "txt",
];
/// Words a link to a page with the site's player tends to hold, in its address or text.
const PLAYER_WORDS: [&str; 6] = ["listen", "player", "stream", "webcast", "on-air", "tune in"];
/// Words a stream's path tends to hold when no file extension says what it is.
const STREAM_PATH_WORDS: [&str; 5] = ["stream", "icecast", "shoutcast", "listen", "/live"];
/// The same for its host. Not `radio` or `live`: every link on a radio station's site has those.
const STREAM_HOST_WORDS: [&str; 3] = ["stream", "icecast", "shoutcast"];

/// How sure the address is, best first. The stream host's own player data named it, an audio
/// element named it, its extension says audio, or it only holds a telling word.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Rank {
	Provider,
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
	/// Followed wherever found: frames, and stream hosts' player pages and station lists.
	frames: Vec<Url>,
	/// Listen pages; followed from the typed page only, or the search would crawl the site.
	players: Vec<Url>,
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

/// Absolute web addresses and quoted relative URLs in source text.
fn addresses(text: &str, base: &Url) -> Vec<Url> {
	let text = unescape(text);
	let ends = |c: char| {
		c.is_whitespace() || matches!(c, '"' | '\'' | '`' | '<' | '>' | '\\' | ')' | ']' | '}' | ',')
	};
	let mut out = Vec::new();
	for (at, quote) in text.match_indices(['"', '\'', '`']) {
		let tail = &text[at + 1..];
		if (tail.starts_with('/') || tail.starts_with("./") || tail.starts_with("../"))
			&& let Some(end) = tail.find(ends)
			&& tail[end..].starts_with(quote)
		{
			out.extend(base.join(&tail[..end]));
		}
	}
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
	if !is_html {
		// Triton's list names nothing else worth opening, only its schema address.
		let streams = triton_streams(body, base);
		if !streams.is_empty() {
			found.candidates = streams.into_iter()
				.map(|url| Candidate { url, rank: Rank::Provider, label: None })
				.collect();
			return found;
		}
	}
	let mut add = |url: Url, rank: Rank, label: Option<String>| {
		found.candidates.push(Candidate { url, rank, label });
	};
	let mut document_base = None;
	if is_html {
		let html = Html::parse_document(body);
		document_base = html.select(&selector("base[href]"))
			.next()
			.and_then(|element| element.attr("href"))
			.and_then(|href| base.join(href.trim()).ok())
			.filter(is_web);
		let join = |value: &str| {
			document_base.as_ref().unwrap_or(base)
				.join(&unescape(value.trim())).ok().filter(is_web)
		};
		for element in html.select(&selector("audio[src], audio source[src], video[src], video source[src]")) {
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
		for element in html.select(&selector("a[href]")) {
			let Some(url) = element.attr("href").and_then(join) else {
				continue;
			};
			if is_provider_page(&url) {
				found.frames.push(url);
			} else if rank_of(&url).is_none_or(|r| r == Rank::Word) && same_site(&url, base) {
				let text = element.text().collect::<String>().to_lowercase();
				if is_player_link(&url, &text) {
					found.players.push(url);
				}
			}
		}
		if let Some(stream) = securenet_stream(body, base) {
			add(stream, Rank::Provider, found.title.clone());
		}
		found.frames.extend(triton_mounts_named(body, base));
	}
	for url in popups(body, document_base.as_ref().unwrap_or(base)) {
		if is_provider_page(&url) {
			found.frames.push(url);
		} else if same_site(&url, base) && is_player_link(&url, "") {
			found.players.push(url);
		}
	}
	let mut seen = HashSet::new();
	found.players.retain(|u| u != base && seen.insert(u.clone()));
	found.players.truncate(MAX_PLAYERS);
	for url in addresses(body, document_base.as_ref().unwrap_or(base)) {
		if let Some(rank) = rank_of(&url) {
			add(url, rank, None);
		}
	}
	found
}

/// One site: the same host, or one is the other's subdomain (`www.` and `listen.` of one name).
fn same_site(url: &Url, base: &Url) -> bool {
	let (Some(a), Some(b)) = (url.host_str(), base.host_str()) else {
		return false;
	};
	let bare = |h: &str| h.strip_prefix("www.").unwrap_or(h).to_ascii_lowercase();
	let (a, b) = (bare(a), bare(b));
	a == b || a.ends_with(&format!(".{b}")) || b.ends_with(&format!(".{a}"))
}

fn is_player_link(url: &Url, text: &str) -> bool {
	let path = url.path().to_ascii_lowercase();
	let last = path.rsplit('/').next().unwrap_or_default();
	if last.rsplit_once('.').is_some_and(|(_, e)| PAGE_EXTENSIONS.contains(&e) && e != "html" && e != "htm") {
		return false;
	}
	let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
	PLAYER_WORDS.iter().any(|w| path.contains(w) || text.contains(w) || host.starts_with(&format!("{w}.")))
}

/// `window.open('/player.html')`: a Listen button's pop-up player.
fn popups(text: &str, base: &Url) -> Vec<Url> {
	text.match_indices("window.open(")
		.filter_map(|(at, call)| {
			let tail = text[at + call.len()..].trim_start();
			let quote = tail.chars().next().filter(|q| matches!(q, '"' | '\''))?;
			let value = &tail[1..tail[1..].find(quote)? + 1];
			base.join(value.trim()).ok().filter(is_web)
		})
		.collect()
}

/// A stream host's own name for a station or mount: letters, digits, `_` and `-`.
fn is_provider_id(id: &str) -> bool {
	(1..=40).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn host_is(url: &Url, host: &str) -> bool {
	url.host_str()
		.is_some_and(|h| h.eq_ignore_ascii_case(host) || h.to_ascii_lowercase().ends_with(&format!(".{host}")))
}

/// SecureNet Cirrus players (`…securenetsystems.net/cirruscontent/CALL`) and Triton's
/// (`player.listenlive.co/12345`): their data names the stream, not their links.
fn is_provider_page(url: &Url) -> bool {
	securenet_call(url).is_some() || is_listenlive(url)
}

fn securenet_call(url: &Url) -> Option<&str> {
	if url.scheme() != "https" || url.port().is_some() || !host_is(url, "securenetsystems.net") {
		return None;
	}
	let mut parts = url.path().trim_matches('/').split('/');
	let (Some("cirruscontent" | "v5"), Some(call), None) = (parts.next(), parts.next(), parts.next()) else {
		return None;
	};
	is_provider_id(call).then_some(call)
}

fn is_listenlive(url: &Url) -> bool {
	let id = url.path().trim_matches('/');
	url.scheme() == "https"
		&& url.port().is_none()
		&& url.host_str() == Some("player.listenlive.co")
		&& (1..=12).contains(&id.len())
		&& id.bytes().all(|b| b.is_ascii_digit())
}

/// The lasting address a SecureNet player keeps in `streamSrcDB`. Its other addresses carry a
/// play session that expires.
fn securenet_stream(body: &str, page: &Url) -> Option<Url> {
	let call = securenet_call(page)?;
	let at = body.find("streamSrcDB")?;
	let tail = body[at + "streamSrcDB".len()..].trim_start().strip_prefix('=')?.trim_start();
	let quote = tail.chars().next().filter(|q| matches!(q, '"' | '\''))?;
	let url = Url::parse(&tail[1..tail[1..].find(quote)? + 1]).ok()?;
	(url.scheme() == "https"
		&& url.port().is_none()
		&& url.query().is_none()
		&& host_is(&url, "securenetsystems.net")
		&& url.path().trim_matches('/').eq_ignore_ascii_case(call))
	.then_some(url)
}

/// A Triton player page names its mounts in its config; Triton's list for each says which play.
fn triton_mounts_named(body: &str, page: &Url) -> Vec<Url> {
	if !is_listenlive(page) {
		return Vec::new();
	}
	let mut out: Vec<Url> = Vec::new();
	for (at, key) in body.match_indices(r#""mount":""#) {
		let tail = &body[at + key.len()..];
		if let Some(mount) = tail.split('"').next().filter(|m| is_provider_id(m))
			&& let Ok(url) = Url::parse(&format!("{TRITON}livestream?version=1.9&mount={mount}"))
			&& !out.contains(&url)
		{
			out.push(url);
		}
	}
	out.truncate(MAX_FRAMES);
	out
}

#[derive(serde::Deserialize)]
struct TritonConfig {
	mountpoints: TritonMounts,
}

#[derive(serde::Deserialize)]
struct TritonMounts {
	#[serde(default)]
	mountpoint: Vec<TritonMount>,
}

#[derive(serde::Deserialize)]
struct TritonMount {
	mount: String,
	status: TritonStatus,
	#[serde(rename = "media-format")]
	format: TritonFormat,
	#[serde(default)]
	authentication: String,
}

#[derive(serde::Deserialize)]
struct TritonStatus {
	#[serde(rename = "status-code")]
	code: String,
}

#[derive(serde::Deserialize)]
struct TritonFormat {
	#[serde(default)]
	audio: Vec<TritonAudio>,
}

#[derive(serde::Deserialize)]
struct TritonAudio {
	#[serde(rename = "@codec")]
	codec: String,
}

/// Triton's station list: each open mount, through Triton's redirector rather than the edge
/// server it picked today. Monitor mounts are for the station's staff.
fn triton_streams(body: &str, page: &Url) -> Vec<Url> {
	if !page.as_str().starts_with(&format!("{TRITON}livestream?")) {
		return Vec::new();
	}
	let Ok(config) = quick_xml::de::from_str::<TritonConfig>(body) else {
		return Vec::new();
	};
	config.mountpoints.mountpoint.iter()
		.filter(|m| m.status.code.trim() == "200" && m.authentication.trim() == "0")
		.filter(|m| is_provider_id(m.mount.trim()) && !m.mount.to_ascii_uppercase().contains("MONITOR"))
		.filter_map(|m| {
			let extension = match m.format.audio.first()?.codec.to_ascii_lowercase().as_str() {
				"mp3" => "mp3",
				"aaclc" | "heaacv1" | "heaacv2" => "aac",
				_ => return None,
			};
			Url::parse(&format!("{TRITON}livestream-redirect/{}.{extension}", m.mount.trim())).ok()
		})
		.take(MAX_TRITON_MOUNTS)
		.collect()
}

/// The body when it is text a page or script is made of.
async fn fetch_text(client: &reqwest::Client, url: &Url, parent: Option<&Url>) -> Result<Option<(String, bool, Url)>> {
	let agent = concat!("Mozilla/5.0 (compatible; BlindRadio/", env!("CARGO_PKG_VERSION"), ")");
	let mut get = client.get(url.clone()).header("User-Agent", agent);
	if let Some(parent) = parent.filter(|parent| parent.origin() == url.origin()) {
		let mut referer = parent.clone();
		let _ = referer.set_username("");
		let _ = referer.set_password(None);
		referer.set_fragment(None);
		referer.set_query(None);
		get = get.header(reqwest::header::REFERER, referer.as_str());
	}
	let mut response = timeout(FETCH_DEADLINE, get.send())
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
	if !(kind.is_empty() || is_html || kind.starts_with("text/") || kind.contains("script") || kind.contains("json") || kind.contains("xml")) {
		return Ok(None);
	}
	let base = response.url().clone();
	let mut body = Vec::new();
	let deadline = tokio::time::Instant::now() + FETCH_DEADLINE;
	while body.len() < MAX_BYTES {
		match tokio::time::timeout_at(deadline, response.chunk()).await {
			Ok(Ok(Some(chunk))) => body.extend_from_slice(&chunk),
			_ => break,
		}
	}
	Ok(Some((String::from_utf8_lossy(&body).into_owned(), is_html, base)))
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
	// A stream host's own player names its station better than the server's generic icy-name.
	let provider = (found.rank == Rank::Provider)
		.then(|| found.label.clone().or_else(|| title.map(str::to_owned)))
		.flatten();
	let name = provider
		.or(sound.name)
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

/// The streams `page` names; an address that is itself a stream is its own answer. Fails when
/// the page cannot be read, the search breaks, or it runs past its deadline: it always answers.
pub async fn detect(client: &reqwest::Client, page: &str) -> Result<Vec<Station>> {
	let (client, page) = (client.clone(), crate::net::clean_url(page));
	guarded(DETECT_DEADLINE, async move { search(&client, &page).await }).await
}

/// Runs `search` as its own task: a panic answers "Detection failed" instead of nothing, and a
/// stall answers "Timed out" at `deadline`, cancelling the task.
async fn guarded<T: Send + 'static>(
	deadline: Duration,
	search: impl Future<Output = Result<T>> + Send + 'static,
) -> Result<T> {
	let mut task = tokio::spawn(search);
	match timeout(deadline, &mut task).await {
		Ok(Ok(result)) => result,
		Ok(Err(_)) => Err(CoreError::Http("Detection failed".into())),
		Err(_) => {
			task.abort();
			Err(CoreError::Http("Timed out".into()))
		}
	}
}

async fn search(client: &reqwest::Client, page: &str) -> Result<Vec<Station>> {
	let typed_bare = !page.contains("://");
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
	if crate::station_page::is_football360(page.as_str()) {
		let opened = timeout(CHECK_DEADLINE, open(client, page.as_str()))
			.await.map_err(|_| CoreError::Http("Timed out".into()))??;
		let Opened::Hls { .. } = opened else {
			return Err(crate::net::not_a_stream());
		};
		let sound = Sound { codec: Some("HLS".into()), ..Sound::default() };
		return Ok(vec![station_of(&direct, sound, None, &page)]);
	}
	if let Some(sound) = check(client, &page, rank_of(&page) == Some(Rank::Extension)).await {
		return Ok(vec![station_of(&direct, sound, None, &page)]);
	}
	let mut title = None;
	let mut found = Vec::new();
	let mut queue = VecDeque::from([(page.clone(), None)]);
	let mut read = HashSet::new();
	while let Some((url, parent)) = queue.pop_front() {
		let first = parent.is_none();
		if read.len() >= MAX_FETCHES || !read.insert(url.clone()) {
			continue;
		}
		if !first && !is_local(&page) && is_local(&url) {
			continue;
		}
		let text = match fetch_text(client, &url, parent.as_ref()).await {
			Ok(text) => text,
			// A typed `host:8000` with no scheme is often a stream server that speaks only http.
			Err(e) if first && typed_bare => {
				let mut http = url.clone();
				let _ = http.set_scheme("http");
				fetch_text(client, &http, None).await.map_err(|_| e)?
			}
			Err(e) if first => return Err(e),
			Err(_) => continue,
		};
		let Some((body, is_html, base)) = text else {
			continue;
		};
		let scanned = scan(&body, &base, is_html);
		if first {
			title = scanned.title;
		}
		found.extend(scanned.candidates);
		// Frames first: a player's frame is more telling than the site's scripts. Listen pages
		// next, ahead of scripts too.
		let players = if first { scanned.players } else { Vec::new() };
		for frame in scanned.frames.into_iter().chain(players).rev() {
			queue.push_front((frame, Some(base.clone())));
		}
		queue.extend(scanned.scripts.into_iter().map(|s| (s, Some(base.clone()))));
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

	/// A search that panicked or stalled never answered: the dialog sat silent, with no
	/// result and no error. It must always end with one or the other.
	#[tokio::test]
	async fn a_panicking_search_answers_with_an_error() {
		let error = guarded(Duration::from_secs(5), async {
			panic!("boom") as Result<()>
		})
		.await
		.unwrap_err();
		assert_eq!(error.to_string(), "Detection failed");
	}

	#[tokio::test]
	async fn a_stalled_search_answers_at_its_deadline() {
		let started = std::time::Instant::now();
		let error = guarded(Duration::from_millis(100), async {
			tokio::time::sleep(Duration::from_secs(30)).await;
			Ok(())
		})
		.await
		.unwrap_err();
		assert_eq!(error.to_string(), "Timed out");
		assert!(started.elapsed() < Duration::from_secs(5));
	}

	#[tokio::test]
	async fn a_finished_search_answers_with_its_result() {
		assert_eq!(
			guarded(Duration::from_secs(5), async { Ok(7) })
				.await
				.unwrap(),
			7
		);
	}

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
			<video src="live.ts"></video>
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
			("https://radio.example/player/live.ts", Rank::Element),
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
	fn listen_links_and_popups_lead_to_player_pages() {
		let base = Url::parse("https://www.radio.example/").unwrap();
		let html = r#"<a href="/listen-live">Listen Live</a>
			<a href="https://listen.radio.example/">On air</a>
			<a href="/news">News</a>
			<a href="https://other.example/player">Their player</a>
			<a href="/hi.pls">Listen in high quality</a>
			<a href="https://player.listenlive.co/49771">Listen</a>
			<a href="https://streamdb9web.securenetsystems.net/cirruscontent/TESTFM">Listen</a>
			<a href="https://streamdb9web.securenetsystems.net.evil.example/cirruscontent/TESTFM">Listen</a>
			<button onclick="window.open('/popup/player.html', 'p')">Play</button>"#;
		let found = scan(html, &base, true);
		let players: Vec<_> = found.players.iter().map(Url::as_str).collect();
		assert_eq!(
			players,
			["https://www.radio.example/listen-live", "https://listen.radio.example/", "https://www.radio.example/popup/player.html"]
		);
		let frames: Vec<_> = found.frames.iter().map(Url::as_str).collect();
		assert_eq!(
			frames,
			["https://player.listenlive.co/49771", "https://streamdb9web.securenetsystems.net/cirruscontent/TESTFM"]
		);
	}

	#[test]
	fn a_securenet_player_names_its_lasting_stream() {
		let page = Url::parse("https://streamdb3web.securenetsystems.net/cirruscontent/KCCR").unwrap();
		let html = r#"<title>1240 KCCR</title><script>
			var streamSRC = "https://ice7.securenetsystems.net/KCCR?playSessionID=46F9";
			var streamSrcDB = 'https://ice7.securenetsystems.net/KCCR';
			</script>"#;
		let found = scan(html, &page, true);
		let stream = found.candidates.iter().find(|c| c.rank == Rank::Provider).unwrap();
		assert_eq!(stream.url.as_str(), "https://ice7.securenetsystems.net/KCCR");
		assert_eq!(stream.label.as_deref(), Some("1240 KCCR"));
		assert!(!urls(&found).iter().any(|(u, _)| u.contains("playSessionID")), "session addresses expire");
		let elsewhere = Url::parse("https://radio.example/").unwrap();
		assert!(scan(html, &elsewhere, true).candidates.iter().all(|c| c.rank != Rank::Provider));
	}

	#[test]
	fn a_triton_player_leads_to_its_open_mounts() {
		let page = Url::parse("https://player.listenlive.co/49771").unwrap();
		let html = r#"<script>var c = {"stream":{"mount":"KLPXH2FM","streamid":"14991"},"x":{"mount":"KLPXH2FM"},"y":{"mount":"bad mount"}};</script>"#;
		let found = scan(html, &page, true);
		let frames: Vec<_> = found.frames.iter().map(Url::as_str).collect();
		assert_eq!(frames, [format!("{TRITON}livestream?version=1.9&mount=KLPXH2FM")]);

		let list = Url::parse(&format!("{TRITON}livestream?version=1.9&mount=KLPXH2FM")).unwrap();
		let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
			<live_stream_config version="1.9" xmlns="http://provisioning.streamtheworld.com/player/livestream-1.9">
			<mountpoints>
			<mountpoint><status><status-code>200</status-code></status><mount>CHMPFM</mount>
				<media-format container="flv"><audio index="0" codec="mp3" bitrate="96000"/></media-format>
				<authentication>0</authentication></mountpoint>
			<mountpoint><status><status-code>200</status-code></status><mount>CHMPFMAAC</mount>
				<media-format container="flv"><audio index="0" codec="heaacv2"/></media-format>
				<authentication>0</authentication></mountpoint>
			<mountpoint><status><status-code>200</status-code></status><mount>CHMPFMAAC_MONITOR</mount>
				<media-format container="flv"><audio index="0" codec="heaacv2"/></media-format>
				<authentication>0</authentication></mountpoint>
			<mountpoint><status><status-code>200</status-code></status><mount>CHMPFMPAID</mount>
				<media-format container="flv"><audio index="0" codec="mp3"/></media-format>
				<authentication>1</authentication></mountpoint>
			<mountpoint><status><status-code>404</status-code></status><mount>CHMPFMOFF</mount>
				<media-format container="flv"><audio index="0" codec="mp3"/></media-format>
				<authentication>0</authentication></mountpoint>
			</mountpoints></live_stream_config>"#;
		assert_eq!(
			urls(&scan(xml, &list, false)),
			[
				(format!("{TRITON}livestream-redirect/CHMPFM.mp3"), Rank::Provider),
				(format!("{TRITON}livestream-redirect/CHMPFMAAC.aac"), Rank::Provider),
			]
		);
		let elsewhere = Url::parse("https://radio.example/list.xml").unwrap();
		assert!(scan(xml, &elsewhere, false).candidates.iter().all(|c| c.rank != Rank::Provider));
	}

	#[test]
	fn a_telling_word_is_the_weakest_guess() {
		let rank = |u: &str| rank_of(&Url::parse(u).unwrap());
		assert_eq!(rank("https://a.example/stream"), Some(Rank::Word));
		assert_eq!(rank("https://icecast.a.example:8000/x"), Some(Rank::Word));
		assert_eq!(rank("https://a.example/x.MP3"), Some(Rank::Extension));
		assert_eq!(rank("https://a.example/x.ts"), Some(Rank::Extension));
		assert_eq!(rank("http://a.example:8000/;stream.nsv"), Some(Rank::Extension));
		assert_eq!(rank("https://a.example/stream.css"), None);
		assert_eq!(rank("https://a.example/news"), None);
		assert_eq!(rank("https://liveradio.example/about"), None, "only the site's name");
		assert_eq!(rank("ftp://a.example/x.mp3"), None);
	}

	#[test]
	fn resolves_player_paths_and_honors_the_document_base() {
		let base = Url::parse("https://radio.example/site/page").unwrap();
		let html = r#"<base href="/player/">
			<audio src="live"></audio>
			<script src="app.js"></script>
			<script>const hi = "./high.m3u8"; const low = '/audio/low.aac';</script>"#;
		let found = scan(html, &base, true);
		assert!(urls(&found).contains(&("https://radio.example/player/live".into(), Rank::Element)));
		assert!(urls(&found).contains(&("https://radio.example/player/high.m3u8".into(), Rank::Extension)));
		assert!(urls(&found).contains(&("https://radio.example/audio/low.aac".into(), Rank::Extension)));
		assert_eq!(found.scripts, [base.join("/player/app.js").unwrap()]);
		let script = scan(r#"const stream = "../live.mp3";"#, &found.scripts[0], false);
		assert_eq!(urls(&script), [("https://radio.example/live.mp3".into(), Rank::Extension)]);
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
