//! YouTube and Twitch live streams: yt-dlp finds the audio's address, the HLS path plays it.
//!
//! A process, not a crate: the Rust wrappers are GPL or pull in FFmpeg, and the pure Rust
//! extractors stopped keeping up with YouTube. BlindRadio keeps its own nightly yt-dlp and, for
//! YouTube's challenges, its own Deno, fetched on first use. A station saves the page link, never
//! the stream: stream addresses expire and are tied to the listener's IP.
//!
//! ponytail: downloads are checked against the SHA-256 their release publishes, over TLS.
//! Upgrade path: also verify yt-dlp's SHA2-256SUMS.sig with its key compiled in.

use std::io::Read;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use reqwest::Url;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::process::Command;
use tokio::sync::Mutex;

use crate::{CoreError, Result};

/// A lookup, including one retry after an update.
pub(crate) const BUDGET: Duration = Duration::from_secs(75);
pub(crate) const INSTALLING: &str = "Downloading live stream support";
const LOOKUP: Duration = Duration::from_secs(30);
const DOWNLOAD: Duration = Duration::from_secs(600);
const CHECK_EVERY: Duration = Duration::from_secs(24 * 60 * 60);
const NOT_LIVE: &str = "Not live";
const NIGHTLY: &str = "https://github.com/yt-dlp/yt-dlp-nightly-builds/releases/latest/download";
const DENO_RELEASE: &str = "https://github.com/denoland/deno/releases/latest/download";

#[cfg(windows)]
const YT_DLP: Helper = Helper {
	file: "yt-dlp.exe",
	asset: "yt-dlp.exe",
	release: NIGHTLY,
	sums: "SHA2-256SUMS",
	zipped: false,
};
#[cfg(target_os = "macos")]
const YT_DLP: Helper = Helper {
	file: "yt-dlp",
	asset: "yt-dlp_macos",
	release: NIGHTLY,
	sums: "SHA2-256SUMS",
	zipped: false,
};
#[cfg(all(unix, not(target_os = "macos")))]
const YT_DLP: Helper = Helper {
	file: "yt-dlp",
	asset: "yt-dlp_linux",
	release: NIGHTLY,
	sums: "SHA2-256SUMS",
	zipped: false,
};
#[cfg(windows)]
const DENO: Helper = Helper {
	file: "deno.exe",
	asset: "deno-x86_64-pc-windows-msvc.zip",
	release: DENO_RELEASE,
	sums: "deno-x86_64-pc-windows-msvc.zip.sha256sum",
	zipped: true,
};
#[cfg(target_os = "macos")]
const DENO: Helper = Helper {
	file: "deno",
	asset: "deno-aarch64-apple-darwin.zip",
	release: DENO_RELEASE,
	sums: "deno-aarch64-apple-darwin.zip.sha256sum",
	zipped: true,
};
#[cfg(all(unix, not(target_os = "macos")))]
const DENO: Helper = Helper {
	file: "deno",
	asset: "deno-x86_64-unknown-linux-gnu.zip",
	release: DENO_RELEASE,
	sums: "deno-x86_64-unknown-linux-gnu.zip.sha256sum",
	zipped: true,
};

/// One writer at a time; a lookup that finds its helpers in place never waits on this.
static WRITING: Mutex<()> = Mutex::const_new(());

#[derive(Clone, Copy)]
struct Helper {
	file: &'static str,
	asset: &'static str,
	release: &'static str,
	sums: &'static str,
	zipped: bool,
}

impl Helper {
	fn path(self) -> PathBuf {
		dir().join(self.file)
	}

	/// Holds the SHA-256 of what is installed. Its age says when the release was last asked.
	fn state(self) -> PathBuf {
		dir().join(format!("{}.sha256", self.file))
	}

	fn stale(self) -> bool {
		std::fs::metadata(self.state())
			.and_then(|m| m.modified())
			.ok()
			.and_then(|at| at.elapsed().ok())
			.is_none_or(|age| age > CHECK_EVERY)
	}
}

/// The cache folder, not the data folder: these are programs for this PC, and a portable data
/// folder is often a synced one.
fn dir() -> PathBuf {
	dirs::cache_dir()
		.map(|d| d.join("BlindRadio"))
		.unwrap_or_else(crate::store::data_dir)
		.join("helpers")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Site {
	YouTube,
	Twitch,
}

impl Site {
	fn helpers(self) -> &'static [Helper] {
		match self {
			Site::YouTube => &[YT_DLP, DENO],
			Site::Twitch => &[YT_DLP],
		}
	}
}

fn site(url: &str) -> Option<Site> {
	let url = Url::parse(url).ok()?;
	if !matches!(url.scheme(), "http" | "https") || url.path().trim_matches('/').is_empty() {
		return None;
	}
	let host = url.host_str()?.to_ascii_lowercase();
	let host = ["www.", "m.", "music."]
		.iter()
		.find_map(|p| host.strip_prefix(p))
		.unwrap_or(&host);
	match host {
		"youtube.com" | "youtu.be" => Some(Site::YouTube),
		"twitch.tv" => Some(Site::Twitch),
		_ => None,
	}
}

/// A YouTube or Twitch link, which only yt-dlp can turn into a stream.
pub(crate) fn is_page(url: &str) -> bool {
	site(url).is_some()
}

pub(crate) struct Live {
	pub(crate) stream: String,
	pub(crate) name: String,
}

/// Whether playing `url` has helpers to download first.
pub(crate) fn needs_install(url: &str) -> bool {
	site(url).is_some_and(|s| s.helpers().iter().any(|h| !h.path().exists()))
}

/// Fetches what `url` needs and is missing.
pub(crate) async fn install(client: &reqwest::Client, url: &str) -> Result<()> {
	let Some(site) = site(url) else {
		return Ok(());
	};
	for helper in site.helpers() {
		if !helper.path().exists() {
			refresh(client, *helper).await?;
		}
	}
	Ok(())
}

/// The audio's address right now and the channel's name.
pub(crate) async fn lookup(client: &reqwest::Client, url: &str) -> Result<Live> {
	let site = site(url).ok_or_else(crate::net::not_a_stream)?;
	install(client, url).await?;
	for helper in site.helpers().iter().filter(|h| h.stale()) {
		let (client, helper) = (client.clone(), *helper);
		tokio::spawn(async move { drop(refresh(&client, helper).await) });
	}
	match query(url, site).await {
		// YouTube changed something since the last check: a new nightly is the fix, so look now.
		Err(e) if e.to_string() != NOT_LIVE => {
			let mut updated = false;
			for helper in site.helpers() {
				updated |= refresh(client, *helper).await.unwrap_or(false);
			}
			if updated { query(url, site).await } else { Err(e) }
		}
		result => result,
	}
}

async fn query(url: &str, site: Site) -> Result<Live> {
	let mut command = Command::new(YT_DLP.path());
	command
		.args(["--ignore-config", "--no-warnings", "--no-playlist"])
		.args(["--extractor-retries", "0", "-J"]);
	if site == Site::YouTube {
		command
			.arg("--js-runtimes")
			.arg(format!("deno:{}", DENO.path().display()));
	}
	// After `--`, and already matched to a known host: a link can never become a flag.
	command
		.arg("--")
		.arg(url)
		.current_dir(dir())
		.env("DENO_NO_UPDATE_CHECK", "1")
		.stdin(Stdio::null())
		.kill_on_drop(true);
	#[cfg(windows)]
	command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
	let output = tokio::time::timeout(LOOKUP, command.output())
		.await
		.map_err(|_| CoreError::Http("No data".into()))?
		.map_err(|_| CoreError::Http("Cannot run yt-dlp".into()))?;
	if !output.status.success() {
		return Err(CoreError::Http(reason(&String::from_utf8_lossy(
			&output.stderr,
		))));
	}
	let info: Value =
		serde_json::from_slice(&output.stdout).map_err(|_| CoreError::Http("Bad reply".into()))?;
	parse(&info)
}

fn parse(info: &Value) -> Result<Live> {
	if info["live_status"] != "is_live" {
		return Err(CoreError::Http(NOT_LIVE.into()));
	}
	let stream = pick(info).ok_or_else(|| CoreError::Decode("Unsupported format".into()))?;
	let name = ["channel", "uploader", "title"]
		.iter()
		.find_map(|key| crate::station::known(info[*key].as_str()?))
		.unwrap_or_else(|| "Live stream".to_owned());
	Ok(Live {
		stream: stream.to_owned(),
		name,
	})
}

/// Audio only when it exists: YouTube's AAC 128k (itag 234) over its HE-AAC 48k, whose entries
/// state no bitrate, then Twitch's `audio_only`, then any. Else the cheapest HLS with sound.
fn pick(info: &Value) -> Option<&str> {
	let hls: Vec<&Value> = info["formats"]
		.as_array()?
		.iter()
		.filter(|f| f["protocol"].as_str().is_some_and(|p| p.contains("m3u8")))
		.filter(|f| f["acodec"] != "none")
		.collect();
	let audio = || hls.iter().filter(|f| f["vcodec"] == "none");
	let id = |id: &str| audio().find(|f| f["format_id"] == id);
	let bitrate = |f: &&&Value| f["tbr"].as_f64().unwrap_or(0.0) as u64;
	id("234")
		.or_else(|| id("audio_only"))
		.or_else(|| audio().max_by_key(bitrate))
		.or_else(|| hls.iter().min_by_key(bitrate))?["url"]
		.as_str()
}

/// What yt-dlp's last error says, without its prefixes.
fn reason(stderr: &str) -> String {
	let line = stderr
		.lines()
		.rev()
		.find_map(|l| l.trim().strip_prefix("ERROR:"))
		.unwrap_or("No data")
		.trim();
	let lower = line.to_ascii_lowercase();
	if ["not currently live", "offline", "will begin", "premieres in"]
		.iter()
		.any(|w| lower.contains(w))
	{
		return NOT_LIVE.to_owned();
	}
	let line = line.split(" (caused by").next().unwrap_or(line);
	match line.strip_prefix('[').and_then(|l| l.split_once("] ")) {
		Some((_, rest)) => rest.to_owned(),
		None => line.to_owned(),
	}
}

/// The hash a release lists for `asset`. A list naming other files only never yields one that
/// matches, so a wrong line fails the download instead of passing it.
fn listed_hash(text: &str, asset: &str) -> Option<String> {
	let line = text
		.lines()
		.find(|l| l.split_whitespace().any(|t| t.trim_start_matches('*') == asset))
		.unwrap_or(text);
	line.split_whitespace()
		.find(|t| t.len() == 64 && t.bytes().all(|b| b.is_ascii_hexdigit()))
		.map(str::to_ascii_lowercase)
}

/// Brings `helper` up to the newest release; `true` when it replaced the file. Runs on its own
/// task: moving to another station must not throw away a download.
async fn refresh(client: &reqwest::Client, helper: Helper) -> Result<bool> {
	let client = client.clone();
	tokio::spawn(async move {
		let _writing = WRITING.lock().await;
		let sums_url = format!("{}/{}", helper.release, helper.sums);
		let sums = crate::net::get_bytes(&client, &sums_url).await?;
		let wanted = listed_hash(&String::from_utf8_lossy(&sums), helper.asset)
			.ok_or_else(|| CoreError::Http("No checksum".into()))?;
		let held = std::fs::read_to_string(helper.state()).unwrap_or_default();
		if held.trim() == wanted && helper.path().exists() {
			// Rewritten so its age counts from this check.
			let _ = std::fs::write(helper.state(), &wanted);
			return Ok(false);
		}
		let url = format!("{}/{}", helper.release, helper.asset);
		let response = client
			.get(&url)
			.timeout(DOWNLOAD)
			.send()
			.await
			.map_err(crate::net::http_err)?
			.error_for_status()
			.map_err(crate::net::http_err)?;
		let body = response.bytes().await.map_err(crate::net::http_err)?;
		tokio::task::spawn_blocking(move || write_verified(helper, &body, &wanted))
			.await
			.map_err(|_| CoreError::Http("Download failed".into()))??;
		Ok(true)
	})
	.await
	.map_err(|_| CoreError::Http("Download failed".into()))?
}

/// Refuses a file that does not match its hash. The old copy stays until the new one is whole.
fn write_verified(helper: Helper, body: &[u8], wanted: &str) -> Result<()> {
	let failed = |_| CoreError::Http("Download failed".into());
	let found: String = Sha256::digest(body)
		.iter()
		.map(|b| format!("{b:02x}"))
		.collect();
	if found != wanted {
		return Err(CoreError::Http("Bad download".into()));
	}
	let program = if helper.zipped {
		let mut archive = zip::ZipArchive::new(std::io::Cursor::new(body))
			.map_err(|_| CoreError::Http("Bad download".into()))?;
		let mut member = archive
			.by_name(helper.file)
			.map_err(|_| CoreError::Http("Bad download".into()))?;
		let mut program = Vec::new();
		member.read_to_end(&mut program).map_err(failed)?;
		program
	} else {
		body.to_vec()
	};
	std::fs::create_dir_all(dir()).map_err(failed)?;
	let part = dir().join(format!("{}.part", helper.file));
	std::fs::write(&part, program).map_err(failed)?;
	#[cfg(unix)]
	{
		use std::os::unix::fs::PermissionsExt;
		std::fs::set_permissions(&part, std::fs::Permissions::from_mode(0o755)).map_err(failed)?;
	}
	std::fs::rename(&part, helper.path()).map_err(failed)?;
	std::fs::write(helper.state(), wanted).map_err(failed)
}

#[cfg(test)]
mod tests {
	use super::*;
	use serde_json::json;

	#[test]
	fn only_real_youtube_and_twitch_links_are_pages() {
		for url in [
			"https://www.youtube.com/watch?v=bLAJZTSl2_Y",
			"https://youtube.com/@BBCPersian/live",
			"https://m.youtube.com/watch?v=x",
			"https://music.youtube.com/watch?v=x",
			"https://youtu.be/bLAJZTSl2_Y",
			"https://www.twitch.tv/buster",
			"HTTPS://WWW.TWITCH.TV/buster",
		] {
			assert!(is_page(url), "{url}");
		}
		for url in [
			"https://www.youtube.com/",
			"https://twitch.tv",
			"https://youtube.com.evil.example/watch?v=x",
			"https://notyoutube.com/watch?v=x",
			"ftp://youtube.com/watch?v=x",
			"--exec=calc",
			"https://stream.example/live.m3u8",
		] {
			assert!(!is_page(url), "{url}");
		}
		assert_eq!(site("https://youtu.be/x"), Some(Site::YouTube));
		assert_eq!(site("https://twitch.tv/x"), Some(Site::Twitch));
	}

	fn format(id: &str, protocol: &str, acodec: &str, vcodec: &str, tbr: f64) -> Value {
		json!({"format_id": id, "protocol": protocol, "acodec": acodec, "vcodec": vcodec,
			"tbr": tbr, "url": format!("http://x/{id}")})
	}

	fn live(formats: Vec<Value>) -> Value {
		json!({"live_status": "is_live", "channel": "BBC Persian", "formats": formats})
	}

	#[test]
	fn pick_takes_the_best_audio_only_stream() {
		// YouTube, as yt-dlp reported bLAJZTSl2_Y: audio entries have no acodec or bitrate.
		let youtube = live(vec![
			json!({"format_id": "233", "protocol": "m3u8_native", "vcodec": "none", "url": "http://x/233"}),
			json!({"format_id": "234", "protocol": "m3u8_native", "vcodec": "none", "url": "http://x/234"}),
			format("269", "m3u8_native", "none", "avc1", 290.0),
		]);
		assert_eq!(pick(&youtube), Some("http://x/234"));
		let twitch = live(vec![
			format("audio_only", "m3u8_native", "mp4a.40.2", "none", 160.0),
			format("160p30", "m3u8_native", "mp4a.40.2", "avc1", 216.0),
		]);
		assert_eq!(pick(&twitch), Some("http://x/audio_only"));
		let other_audio = live(vec![
			format("low", "m3u8", "aac", "none", 48.0),
			format("high", "m3u8", "aac", "none", 128.0),
		]);
		assert_eq!(pick(&other_audio), Some("http://x/high"));
		let muxed = live(vec![
			format("720", "m3u8", "aac", "avc1", 3000.0),
			format("144", "m3u8", "aac", "avc1", 300.0),
			format("silent", "m3u8", "none", "avc1", 100.0),
			format("sabr", "https", "aac", "none", 128.0),
		]);
		assert_eq!(pick(&muxed), Some("http://x/144"));
		assert_eq!(pick(&live(vec![format("sabr", "https", "aac", "none", 128.0)])), None);
	}

	#[test]
	fn parse_needs_a_live_stream_and_names_the_channel() {
		let found = parse(&live(vec![format("234", "m3u8", "aac", "none", 1.0)])).unwrap();
		assert_eq!(found.stream, "http://x/234");
		assert_eq!(found.name, "BBC Persian");
		let video = json!({"live_status": "not_live", "formats": [format("234", "m3u8", "aac", "none", 1.0)]});
		assert_eq!(parse(&video).err().unwrap().to_string(), NOT_LIVE);
		// A channel's page comes back as a list, with no status of its own.
		assert_eq!(parse(&json!({"entries": []})).err().unwrap().to_string(), NOT_LIVE);
	}

	#[test]
	fn errors_read_short() {
		assert_eq!(reason("ERROR: [twitch:stream] buster: The channel is not currently live\n"), NOT_LIVE);
		assert_eq!(reason("ERROR: [youtube] x: This live event will begin in 3 hours.\n"), NOT_LIVE);
		assert_eq!(reason("WARNING: a\nERROR: [youtube] x: Video unavailable"), "x: Video unavailable");
		assert_eq!(
			reason("ERROR: [youtube:tab] @x/live: Unable to download API page: HTTP Error 404: Not Found (caused by <HTTPError 404: Not Found>)"),
			"@x/live: Unable to download API page: HTTP Error 404: Not Found"
		);
		assert_eq!(reason("nothing useful"), "No data");
	}

	#[test]
	fn listed_hash_finds_the_right_line_in_either_form() {
		let (a, b) = ("a".repeat(64), "B".repeat(64));
		let sums = format!("{a}  yt-dlp_linux\n{b}  yt-dlp.exe\n");
		assert_eq!(listed_hash(&sums, "yt-dlp.exe"), Some(b.to_ascii_lowercase()));
		assert_eq!(listed_hash(&format!("{a} *yt-dlp.exe\n"), "yt-dlp.exe"), Some(a.clone()));
		// Deno's Windows file has the hash on a line of its own, with no name beside it.
		let powershell = format!("Algorithm : SHA256\r\nHash      : {b}\r\nPath      : C:\\x.zip\r\n");
		assert_eq!(listed_hash(&powershell, "deno.zip"), Some(b.to_ascii_lowercase()));
		assert_eq!(listed_hash("no hash here", "x"), None);
	}

	#[test]
	fn a_download_that_fails_its_hash_is_refused() {
		let error = write_verified(YT_DLP, b"tampered", &"0".repeat(64)).unwrap_err();
		assert_eq!(error.to_string(), "Bad download");
	}
}
