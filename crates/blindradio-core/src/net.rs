use crate::playlist::{PlaylistKind, parse_m3u, parse_pls, sniff};
use crate::{CoreError, Result, store};
use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::LazyLock;
use std::time::Duration;

const MAX_DEPTH: u8 = 3;
const SNIFF_LIMIT: usize = 64 * 1024;
const SNIFF_DEADLINE: Duration = Duration::from_secs(3);
/// Bytes read from an unlabeled body before judging it: enough for a playlist's first line
/// or three TS packets.
const HEAD_BYTES: usize = 1024;
/// Head start each playlist entry gets before the next one is tried alongside it.
const ENTRY_STAGGER: Duration = Duration::from_millis(500);

static CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
	reqwest::Client::builder()
		// Per-request headers from `user_agent_for` override this; it covers any stray request.
		.user_agent(concat!("BlindRadio/", env!("CARGO_PKG_VERSION")))
		// 10 s, not 4: some station servers are slow to finish the TLS
		// handshake (AZPM's, measured), and a stream that connects slowly
		// still plays for hours. Patient players connect where 4 s failed.
		.connect_timeout(Duration::from_secs(10))
		.redirect(reqwest::redirect::Policy::limited(10))
		.build()
		.expect("http client")
});

/// SomaFM stream hosts (`ice<digits>.somafm.com`) drop any user agent containing "blindradio"
/// (any case) with an empty reply, so they get a hyphenated name; everything else gets the real one.
pub fn user_agent_for(url: &str) -> &'static str {
	let soma_stream = reqwest::Url::parse(url)
		.ok()
		.and_then(|u| u.host_str().map(str::to_ascii_lowercase))
		.and_then(|h| h.strip_suffix(".somafm.com").map(str::to_owned))
		.and_then(|h| h.strip_prefix("ice").map(str::to_owned))
		.is_some_and(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()));
	if soma_stream {
		concat!("Blind-Radio/", env!("CARGO_PKG_VERSION"))
	} else {
		concat!("BlindRadio/", env!("CARGO_PKG_VERSION"))
	}
}

/// One process-wide client; cloning shares the connection pool.
pub fn client() -> reqwest::Client {
	CLIENT.clone()
}

pub enum Opened {
	Audio {
		response: reqwest::Response,
		final_url: String,
		/// Body bytes already read to judge an unlabeled stream; they come before the response's.
		head: bytes::Bytes,
	},
	Hls {
		playlist_url: String,
		/// The playlist body already read, so the HLS reader starts without fetching it again.
		playlist: String,
	},
}

pub(crate) fn http_err(e: reqwest::Error) -> CoreError {
	if e.is_redirect() {
		CoreError::Http("Too many redirects".into())
	} else {
		CoreError::Http("Unreachable".into())
	}
}

pub(crate) fn not_a_stream() -> CoreError {
	CoreError::Playlist("Not a stream".into())
}

/// The plain-HTTP twin of an HTTPS URL.
fn http_twin(url: &str) -> Option<String> {
	url.strip_prefix("https://")
		.map(|rest| format!("http://{rest}"))
}

/// One GET with the headers every stream open sends.
async fn fetch(
	client: &reqwest::Client,
	url: &str,
) -> std::result::Result<reqwest::Response, reqwest::Error> {
	client
		.get(url)
		.header("User-Agent", user_agent_for(url))
		.header("Icy-MetaData", "1")
		.header("Accept-Encoding", "identity")
		.send()
		.await
}

fn is_playlist_type(ct: &str) -> bool {
	matches!(
		ct,
		"audio/x-scpls"
			| "audio/x-mpegurl"
			| "audio/mpegurl"
			| "application/vnd.apple.mpegurl"
			| "application/x-mpegurl"
	)
}

fn is_audio(ct: &str, headers: &reqwest::header::HeaderMap) -> bool {
	let icy = headers.keys().any(|k| k.as_str().starts_with("icy-"));
	ct.starts_with("audio/") || ct == "application/ogg" || icy
}

/// Types no stream or playlist is ever served as; anything else unlabeled gets a look at its bytes.
fn is_never_media(ct: &str) -> bool {
	ct.starts_with("image/") || ct.ends_with("json") || ct.ends_with("xml")
}

/// Reads about `HEAD_BYTES` of the body, stopping early at its end or the sniff deadline.
async fn read_head(response: &mut reqwest::Response) -> Result<Vec<u8>> {
	let mut head = Vec::new();
	let deadline = tokio::time::Instant::now() + SNIFF_DEADLINE;
	while head.len() < HEAD_BYTES {
		match tokio::time::timeout_at(deadline, response.chunk()).await {
			Ok(Ok(Some(c))) => head.extend_from_slice(&c),
			Ok(Ok(None)) | Err(_) => break,
			Ok(Err(e)) => return Err(http_err(e)),
		}
	}
	Ok(head)
}

/// Opens `url`, following redirects and playlists. Audio comes back still open
/// so the caller keeps streaming on the same connection.
pub async fn open(client: &reqwest::Client, url: &str) -> Result<Opened> {
	open_at(client, url.to_owned(), 0).await
}

fn open_at<'a>(
	client: &'a reqwest::Client,
	url: String,
	depth: u8,
) -> Pin<Box<dyn Future<Output = Result<Opened>> + Send + 'a>> {
	Box::pin(async move {
		// Depth 0 is the station URL itself, so MAX_DEPTH playlist levels mean MAX_DEPTH fetches in all.
		if depth >= MAX_DEPTH {
			return Err(not_a_stream());
		}
		let mut response = match fetch(client, &url).await {
			Ok(response) => response,
			Err(e) => match (e.is_connect(), http_twin(&url)) {
				// Some stream hosts (Shoutcast boxes, AZPM among them)
				// listen on 443 but their TLS handshake never completes,
				// while the same stream answers on plain HTTP. Players
				// like DVBViewer and VLC fall back; so does this one.
				(true, Some(twin)) => fetch(client, &twin).await.map_err(http_err)?,
				_ => return Err(http_err(e)),
			},
		};
		let status = response.status();
		if status.as_u16() >= 400 {
			return Err(CoreError::Http(format!("HTTP {}", status.as_u16())));
		}
		let final_url = response.url().to_string();
		let ct = response
			.headers()
			.get("content-type")
			.and_then(|v| v.to_str().ok())
			.map(|v| {
				v.split(';')
					.next()
					.unwrap_or("")
					.trim()
					.to_ascii_lowercase()
			})
			.unwrap_or_default();

		let explicit_playlist = is_playlist_type(&ct);
		if !explicit_playlist && is_audio(&ct, response.headers()) {
			return Ok(Opened::Audio {
				response,
				final_url,
				head: bytes::Bytes::new(),
			});
		}
		if is_never_media(&ct) {
			return Err(not_a_stream());
		}

		let mut body = Vec::new();
		// video/mp2t, application/octet-stream or no type at all: a playlist if its bytes say so,
		// else a stream for the decoder to judge.
		if !explicit_playlist && !ct.starts_with("text/") {
			body = read_head(&mut response).await?;
			if body.is_empty() {
				return Err(not_a_stream());
			}
			if sniff(&String::from_utf8_lossy(&body)).is_none() {
				return Ok(Opened::Audio {
					response,
					final_url,
					head: body.into(),
				});
			}
		}
		// A slow or endless text body is judged on what arrived by the deadline.
		let deadline = tokio::time::Instant::now() + SNIFF_DEADLINE;
		while body.len() < SNIFF_LIMIT {
			match tokio::time::timeout_at(deadline, response.chunk()).await {
				Ok(Ok(Some(c))) => body.extend_from_slice(&c),
				Ok(Ok(None)) | Err(_) => break,
				Ok(Err(e)) => return Err(http_err(e)),
			}
		}
		let text = String::from_utf8_lossy(&body).into_owned();
		let entries = match sniff(&text) {
			Some(PlaylistKind::Hls) => {
				return Ok(Opened::Hls {
					playlist_url: final_url,
					playlist: text,
				});
			}
			Some(PlaylistKind::Pls) => parse_pls(&text),
			Some(PlaylistKind::M3u) => parse_m3u(&text),
			None => return Err(not_a_stream()),
		};
		open_first(
			client,
			entries.into_iter().map(|e| e.url).collect(),
			depth + 1,
		)
		.await
	})
}

/// Opens playlist entries in order, starting the next one `ENTRY_STAGGER` after the last if
/// nothing has answered yet, so a dead first entry does not spend the whole start budget.
/// The first entry to open wins; dropping the set cancels the rest.
async fn open_first(client: &reqwest::Client, urls: Vec<String>, depth: u8) -> Result<Opened> {
	let mut urls = urls.into_iter();
	let mut running = tokio::task::JoinSet::new();
	let mut last = not_a_stream();
	loop {
		if let Some(url) = urls.next() {
			let client = client.clone();
			running.spawn(async move { open_at(&client, url, depth).await });
		}
		let finished = if urls.len() > 0 {
			match tokio::time::timeout(ENTRY_STAGGER, running.join_next()).await {
				Ok(done) => done,
				Err(_) => continue,
			}
		} else {
			running.join_next().await
		};
		match finished {
			Some(Ok(Ok(opened))) => return Ok(opened),
			Some(Ok(Err(e))) => last = e,
			Some(Err(_)) => {}
			None => return Err(last),
		}
	}
}

/// All of a Name Automatically lookup.
const NAME_DEADLINE: Duration = Duration::from_secs(8);
/// TS bytes read looking for the service name; a channel repeats it every couple of seconds.
const NAME_TS_BYTES: usize = 512 * 1024;

/// What a station calls itself, for Name Automatically: its ICY or Icecast name, the title its
/// playlist gives it, the service name in its TS, else a name made from the address. `Err` when
/// nothing answers at all. The user can always edit what this finds.
pub async fn station_name(client: &reqwest::Client, url: &str) -> Result<String> {
	tokio::time::timeout(NAME_DEADLINE, name_at(client, url.to_owned(), 0))
		.await
		.unwrap_or_else(|_| Err(CoreError::Http("No data".into())))
}

fn name_at<'a>(
	client: &'a reqwest::Client,
	url: String,
	depth: u8,
) -> Pin<Box<dyn Future<Output = Result<String>> + Send + 'a>> {
	Box::pin(async move {
		if depth >= MAX_DEPTH {
			return Err(not_a_stream());
		}
		let mut response = fetch(client, &url).await.map_err(http_err)?;
		let status = response.status().as_u16();
		if status >= 400 {
			return Err(CoreError::Http(format!("HTTP {status}")));
		}
		let final_url = response.url().to_string();
		for header in ["icy-name", "ice-name", "x-audiocast-name"] {
			let named = response
				.headers()
				.get(header)
				.and_then(|v| usable_name(&decode_text(v.as_bytes())));
			if let Some(name) = named {
				return Ok(name);
			}
		}
		let mut body = read_head(&mut response).await?;
		let text = String::from_utf8_lossy(&body).into_owned();
		let kind = sniff(&text);
		if matches!(kind, Some(PlaylistKind::Pls | PlaylistKind::M3u)) {
			let entries = match kind {
				Some(PlaylistKind::Pls) => parse_pls(&text),
				_ => parse_m3u(&text),
			};
			let first = entries.first().ok_or_else(not_a_stream)?;
			let titled = first.title.as_deref().and_then(usable_name);
			return match titled {
				Some(name) => Ok(strip_rank(&name).to_owned()),
				None => name_at(client, first.url.clone(), depth + 1).await,
			};
		}
		if crate::hls::looks_like_ts(&body) {
			while body.len() < NAME_TS_BYTES {
				if let Some(name) = crate::hls::ts_service_name(&body).as_deref().and_then(usable_name)
				{
					return Ok(name);
				}
				match response.chunk().await {
					Ok(Some(c)) => body.extend_from_slice(&c),
					_ => break,
				}
			}
		}
		Ok(name_from_url(&final_url))
	})
}

/// UTF-8, else Latin-1, as old servers send their names.
fn decode_text(bytes: &[u8]) -> String {
	match std::str::from_utf8(bytes) {
		Ok(text) => text.to_owned(),
		Err(_) => bytes.iter().map(|&b| char::from(b)).collect(),
	}
}

/// `None` for an empty name, an address, and the placeholders servers and muxers leave behind.
fn usable_name(name: &str) -> Option<String> {
	let name = name.trim();
	let lower = name.to_ascii_lowercase();
	let default_service = lower
		.strip_prefix("service")
		.is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
	let placeholder = matches!(
		lower.as_str(),
		"" | "-" | "no name" | "noname" | "unnamed" | "unknown" | "unspecified" | "stream" | "n/a"
	);
	let address = lower.starts_with("http://") || lower.starts_with("https://");
	(!(placeholder || default_service || address)).then(|| name.to_owned())
}

/// Shoutcast's playlists title a station `(#1 - 104/500) Name`.
fn strip_rank(title: &str) -> &str {
	title
		.strip_prefix("(#")
		.and_then(|rest| rest.split_once(") "))
		.map_or(title, |(_, name)| name.trim())
}

/// The last part of the address that says something, else its host.
fn name_from_url(url: &str) -> String {
	const GENERIC: [&str; 16] = [
		"chunklist", "playlist", "index", "master", "manifest", "stream", "live", "listen", "radio",
		"hls", "audio", "icecast", "shoutcast", "mp3", "aac", "media",
	];
	let Ok(parsed) = reqwest::Url::parse(url) else {
		return url.to_owned();
	};
	let said = parsed.path_segments().and_then(|segments| {
		segments.rev().find_map(|segment| {
			let stem = segment.split('.').next().unwrap_or_default();
			let lower = stem.to_ascii_lowercase();
			let generic = GENERIC.iter().any(|g| lower.starts_with(g));
			(!stem.is_empty() && !generic && !lower.bytes().all(|b| b.is_ascii_digit()))
				.then(|| stem.to_owned())
		})
	});
	match said {
		Some(stem) => percent_encoding::percent_decode_str(&stem)
			.decode_utf8_lossy()
			.replace('_', " "),
		None => parsed
			.host_str()
			.map(|h| h.trim_start_matches("www.").to_owned())
			.unwrap_or_else(|| url.to_owned()),
	}
}

/// Warms the OS resolver cache; failures are irrelevant here.
pub async fn warm_dns(hosts: Vec<String>) {
	for host in hosts {
		let _ = tokio::net::lookup_host((host.as_str(), 80)).await;
	}
}

/// Remembers where a station URL ended up after redirects and playlists.
pub struct ResolveCache {
	path: PathBuf,
	map: HashMap<String, String>,
}

impl ResolveCache {
	pub fn load(path: &Path) -> ResolveCache {
		ResolveCache {
			path: path.to_owned(),
			map: store::load(path),
		}
	}

	pub fn get(&self, url: &str) -> Option<String> {
		self.map.get(url).cloned()
	}

	/// A final URL with its own query (a session id or token, as amperwave and DLF hand out) is
	/// not kept: reusing it measured slower than redirecting again (1.4 s against 0.6 s).
	pub fn put(&mut self, url: &str, final_url: &str) {
		let query = |u: &str| {
			reqwest::Url::parse(u)
				.ok()
				.and_then(|u| u.query().map(str::to_owned))
		};
		if query(final_url).is_some_and(|q| Some(q) != query(url)) {
			self.map.remove(url);
			return;
		}
		self.map.insert(url.to_owned(), final_url.to_owned());
	}

	pub fn remove(&mut self, url: &str) {
		self.map.remove(url);
	}

	pub fn save(&self) -> Result<()> {
		store::save(&self.path, &self.map)
	}
}

/// Fetches the body of `url`, all within 8 s.
pub async fn get_bytes(client: &reqwest::Client, url: &str) -> Result<bytes::Bytes> {
	let response = client
		.get(url)
		.header("User-Agent", user_agent_for(url))
		.timeout(Duration::from_secs(8))
		.send()
		.await
		.map_err(http_err)?;
	let status = response.status().as_u16();
	if status >= 400 {
		return Err(CoreError::Http(format!("HTTP {status}")));
	}
	response.bytes().await.map_err(http_err)
}

/// Fetches `url` and parses the body as JSON, all within 8 s.
pub async fn get_json<T: serde::de::DeserializeOwned>(
	client: &reqwest::Client,
	url: &str,
) -> Result<T> {
	let body = get_bytes(client, url).await?;
	serde_json::from_slice(&body).map_err(|_| CoreError::Http("Bad reply".into()))
}

#[cfg(test)]
mod tests {
	use super::{http_twin, name_from_url, strip_rank, usable_name};

	#[test]
	fn a_name_can_come_from_the_address() {
		assert_eq!(
			name_from_url("https://radioip.rtp.pt/liveradio/antena180a/chunklist.m3u8"),
			"antena180a"
		);
		assert_eq!(name_from_url("http://x.example/Jazz_FM%20Live.mp3"), "Jazz FM Live");
		assert_eq!(name_from_url("http://www.kuaz.example:8000/stream"), "kuaz.example");
		assert_eq!(name_from_url("http://x.example/live/u/p/123.ts"), "p");
	}

	#[test]
	fn placeholders_are_no_names() {
		for junk in ["", "  ", "-", "no name", "Unnamed", "Service01", "http://x.example/a"] {
			assert_eq!(usable_name(junk), None, "{junk}");
		}
		assert_eq!(usable_name(" Radio One ").as_deref(), Some("Radio One"));
		assert_eq!(usable_name("Service Canada").as_deref(), Some("Service Canada"));
		assert_eq!(strip_rank("(#1 - 104/500) Groove Salad"), "Groove Salad");
		assert_eq!(strip_rank("Groove Salad"), "Groove Salad");
	}

	#[test]
	fn http_twin_swaps_only_the_scheme() {
		assert_eq!(
			http_twin("https://streaming.azpm.org/kuaz256").as_deref(),
			Some("http://streaming.azpm.org/kuaz256")
		);
		assert_eq!(http_twin("http://example.com/a"), None);
		assert_eq!(http_twin("example.com/a"), None);
	}
}
