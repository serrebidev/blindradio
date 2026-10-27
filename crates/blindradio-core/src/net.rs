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

static CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
	reqwest::Client::builder()
		.user_agent(concat!("BlindRadio/", env!("CARGO_PKG_VERSION")))
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
		if depth > MAX_DEPTH {
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

		let mut body = Vec::new();
		while body.len() < SNIFF_LIMIT {
			match response.chunk().await.map_err(http_err)? {
				Some(c) => body.extend_from_slice(&c),
				None => break,
			}
		}
		let text = String::from_utf8_lossy(&body);
		let entries = match sniff(&text) {
			Some(PlaylistKind::Hls) => {
				return Ok(Opened::Hls {
					playlist_url: final_url,
				});
			}
			Some(PlaylistKind::Pls) => parse_pls(&text),
			Some(PlaylistKind::M3u) => parse_m3u(&text),
			None => return Err(not_a_stream()),
		};
		let mut last = not_a_stream();
		for entry in entries {
			match open_at(client, entry.url, depth + 1).await {
				Ok(opened) => return Ok(opened),
				Err(e) => last = e,
			}
		}
		Err(last)
	})
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
