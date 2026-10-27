use crate::playlist::{PlaylistKind, parse_m3u, parse_pls, sniff};
use crate::{CoreError, Result};
use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::LazyLock;
use std::time::Duration;

const MAX_DEPTH: u8 = 3;
const SNIFF_LIMIT: usize = 64 * 1024;
const SNIFF_DEADLINE: Duration = Duration::from_secs(3);
/// Head start each playlist entry gets before the next one is tried alongside it.
const ENTRY_STAGGER: Duration = Duration::from_millis(500);

static CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
	reqwest::Client::builder()
		// SomaFM drops any user agent containing "blindradio" (any case) with an empty reply.
		.user_agent(concat!("Blind-Radio/", env!("CARGO_PKG_VERSION")))
		.connect_timeout(Duration::from_secs(4))
		.redirect(reqwest::redirect::Policy::limited(10))
		.build()
		.expect("http client")
});

/// One process-wide client; cloning shares the connection pool.
pub fn client() -> reqwest::Client {
	CLIENT.clone()
}

pub enum Opened {
	Audio {
		response: reqwest::Response,
		final_url: String,
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
		let mut response = client
			.get(&url)
			.header("Icy-MetaData", "1")
			.header("Accept-Encoding", "identity")
			.send()
			.await
			.map_err(http_err)?;
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
			});
		}
		if !explicit_playlist && !ct.is_empty() && !ct.starts_with("text/") {
			return Err(not_a_stream());
		}

		// A slow or endless text body is judged on what arrived by the deadline.
		let mut body = Vec::new();
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
		let map = std::fs::read(path)
			.ok()
			.and_then(|b| serde_json::from_slice(&b).ok())
			.unwrap_or_default();
		ResolveCache {
			path: path.to_owned(),
			map,
		}
	}

	pub fn get(&self, url: &str) -> Option<String> {
		self.map.get(url).cloned()
	}

	pub fn put(&mut self, url: &str, final_url: &str) {
		self.map.insert(url.to_owned(), final_url.to_owned());
	}

	pub fn remove(&mut self, url: &str) {
		self.map.remove(url);
	}

	pub fn save(&self) -> Result<()> {
		if let Some(dir) = self.path.parent() {
			std::fs::create_dir_all(dir)?;
		}
		let json = serde_json::to_vec_pretty(&self.map).map_err(std::io::Error::from)?;
		std::fs::write(&self.path, json)?;
		Ok(())
	}
}
