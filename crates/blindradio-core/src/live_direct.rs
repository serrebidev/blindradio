//! YouTube and Twitch live streams without yt-dlp, for Android, where no helper program can run.
//!
//! The same requests the services' own players make, as yt-dlp's `visionos` client and
//! streamlink's Twitch plugin make them (both permissively licensed references; nothing copied
//! from GPL extractors). YouTube: the page gives the visitor id YouTube asks for (and, for a
//! channel's `/live` link, which video is on), then the visionOS player API hands out the live
//! HLS manifest, no JavaScript or PO token needed. Twitch: a playback token from GQL, then usher.
//! Both answer with the audio-only rendition's playlist, which the HLS path plays.
//!
//! ponytail: client versions and Twitch's persisted query hash are pinned constants; when a
//! service rejects them, take the current values from yt-dlp `_base.py` / streamlink `twitch.py`.

use reqwest::Url;
use reqwest::header::{ACCEPT_LANGUAGE, COOKIE, USER_AGENT};
use serde_json::{Value, json};

use crate::live::{Live, NOT_LIVE, Site};
use crate::{CoreError, Result};

const UA_WEB: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/134.0.0.0 Safari/537.36";
const UA_VISIONOS: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 15_7_3) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Safari/605.1.15";
const VISIONOS_VERSION: &str = "1.02";
const TWITCH_CLIENT_ID: &str = "kimne78kx3ncx6brgo4mv6wki5h1ko";
const TWITCH_TOKEN_QUERY: &str = "ed230aa1e33e07eebb8928504583da78a5173989fadfb1ac94be06a04f3cdbe9";

fn http(e: impl std::fmt::Display) -> CoreError {
	CoreError::Http(e.to_string())
}

pub(crate) async fn lookup(client: &reqwest::Client, url: &str, site: Site) -> Result<Live> {
	match site {
		Site::YouTube => youtube(client, url).await,
		Site::Twitch => twitch(client, url).await,
	}
}

/// The video id a link names outright: `watch?v=`, `youtu.be/`, `/live/ID`, `/shorts/ID`.
pub(crate) fn video_id(url: &str) -> Option<String> {
	let u = Url::parse(url).ok()?;
	let ok = |id: &str| (id.len() == 11 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')).then(|| id.to_string());
	if let Some((_, v)) = u.query_pairs().find(|(k, _)| k == "v") {
		return ok(&v);
	}
	let parts: Vec<&str> = u.path().trim_matches('/').split('/').collect();
	if u.host_str()?.ends_with("youtu.be") {
		return ok(parts.first()?);
	}
	match parts.as_slice() {
		["live" | "shorts" | "embed", id, ..] => ok(id),
		_ => None,
	}
}

/// The page to read: a channel link goes to its `/live` page, which names the video on air.
pub(crate) fn page_url(url: &str) -> String {
	if let Some(id) = video_id(url) {
		return format!("https://www.youtube.com/watch?v={id}");
	}
	let mut u = Url::parse(url).map(|u| u.to_string()).unwrap_or_else(|_| url.to_string());
	if let Some(q) = u.find(['?', '#']) {
		u.truncate(q);
	}
	let u = u.trim_end_matches('/');
	if u.ends_with("/live") { u.to_string() } else { format!("{u}/live") }
}

/// The text between `"key":"` and the next quote.
fn quoted<'a>(body: &'a str, key: &str) -> Option<&'a str> {
	let start = body.find(key)? + key.len();
	body[start..].split('"').next()
}

/// A live audio rendition from YouTube's master playlist: AAC 128k (234), then HE-AAC (233),
/// then any audio group, else the cheapest variant with sound.
pub(crate) fn youtube_audio(master: &str) -> Option<String> {
	let uri = |l: &str| quoted(l, "URI=\"").map(String::from);
	let media: Vec<&str> = master.lines().filter(|l| l.starts_with("#EXT-X-MEDIA") && l.contains("TYPE=AUDIO")).collect();
	for group in ["GROUP-ID=\"234\"", "GROUP-ID=\"233\""] {
		if let Some(l) = media.iter().find(|l| l.contains(group)) {
			return uri(l);
		}
	}
	if let Some(l) = media.first() {
		return uri(l);
	}
	cheapest_variant(master)
}

/// Twitch's `audio_only` variant, else the cheapest one.
pub(crate) fn twitch_audio(master: &str) -> Option<String> {
	let lines: Vec<&str> = master.lines().collect();
	lines
		.iter()
		.position(|l| l.starts_with("#EXT-X-STREAM-INF") && l.contains("\"audio_only\""))
		.and_then(|i| lines[i + 1..].iter().find(|l| l.starts_with("http")))
		.map(|l| l.to_string())
		.or_else(|| cheapest_variant(master))
}

fn cheapest_variant(master: &str) -> Option<String> {
	let lines: Vec<&str> = master.lines().collect();
	let bandwidth = |l: &str| l.split("BANDWIDTH=").nth(1)?.split(',').next()?.parse::<u64>().ok();
	(0..lines.len())
		.filter(|&i| lines[i].starts_with("#EXT-X-STREAM-INF"))
		.filter_map(|i| Some((bandwidth(lines[i]).unwrap_or(u64::MAX), lines.get(i + 1)?.to_string())))
		.filter(|(_, u)| u.starts_with("http"))
		.min_by_key(|(b, _)| *b)
		.map(|(_, u)| u)
}

async fn youtube(client: &reqwest::Client, url: &str) -> Result<Live> {
	let page = client
		.get(page_url(url))
		.header(USER_AGENT, UA_WEB)
		.header(ACCEPT_LANGUAGE, "en-US")
		// Past the EU consent page.
		.header(COOKIE, "SOCS=CAI")
		.send()
		.await
		.map_err(http)?
		.text()
		.await
		.map_err(http)?;
	let visitor = quoted(&page, "\"VISITOR_DATA\":\"")
		.or_else(|| quoted(&page, "\"visitorData\":\""))
		.ok_or_else(|| http("No data"))?
		.to_string();
	let id = video_id(url)
		.or_else(|| quoted(&page, "<link rel=\"canonical\" href=\"https://www.youtube.com/watch?v=").map(String::from))
		.filter(|id| id.len() == 11)
		.ok_or_else(|| http(NOT_LIVE))?;
	let body = json!({
		"context": { "client": {
			"clientName": "VISIONOS", "clientVersion": VISIONOS_VERSION,
			"deviceMake": "Apple", "deviceModel": "RealityDevice17,1",
			"osName": "visionOS", "osVersion": "26.5.23O471",
			"hl": "en", "gl": "US", "visitorData": visitor,
		}},
		"videoId": id, "contentCheckOk": true, "racyCheckOk": true,
	});
	let player: Value = client
		.post("https://www.youtube.com/youtubei/v1/player?prettyPrint=false")
		.header(USER_AGENT, UA_VISIONOS)
		.header("X-YouTube-Client-Name", "101")
		.header("X-YouTube-Client-Version", VISIONOS_VERSION)
		.header("X-Goog-Visitor-Id", &visitor)
		.header("Origin", "https://www.youtube.com")
		.json(&body)
		.send()
		.await
		.map_err(http)?
		.json()
		.await
		.map_err(http)?;
	let status = &player["playabilityStatus"];
	if status["status"] == "LOGIN_REQUIRED" {
		return Err(http(crate::live::YOUTUBE_SIGN_IN));
	}
	if status["status"] != "OK" {
		let reason = status["reason"].as_str().unwrap_or("Not playable");
		return Err(http(if status["liveStreamability"].is_object() { NOT_LIVE } else { reason }));
	}
	let hls = player["streamingData"]["hlsManifestUrl"].as_str().ok_or_else(|| http(NOT_LIVE))?;
	let master = client.get(hls).header(USER_AGENT, UA_VISIONOS)
		.header(reqwest::header::ACCEPT_ENCODING, "identity")
		.send().await.map_err(http)?.text().await.map_err(http)?;
	let stream = youtube_audio(&master).ok_or_else(|| CoreError::Decode("Unsupported format".into()))?;
	let name = crate::station::known(player["videoDetails"]["author"].as_str().unwrap_or_default())
		.or_else(|| crate::station::known(player["videoDetails"]["title"].as_str().unwrap_or_default()))
		.unwrap_or_else(|| "Live stream".into());
	Ok(Live { stream, name })
}

/// `twitch.tv/<channel>` is live; `twitch.tv/videos/<id>` is a past broadcast.
pub(crate) fn twitch_target(url: &str) -> Option<(bool, String)> {
	let u = Url::parse(url).ok()?;
	let parts: Vec<&str> = u.path().trim_matches('/').split('/').filter(|p| !p.is_empty()).collect();
	match parts.as_slice() {
		["videos", id, ..] => Some((false, id.to_string())),
		[login, ..] if login.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') => Some((true, login.to_ascii_lowercase())),
		_ => None,
	}
}

async fn twitch(client: &reqwest::Client, url: &str) -> Result<Live> {
	let (live, target) = twitch_target(url).ok_or_else(crate::net::not_a_stream)?;
	let query = json!({
		"operationName": "PlaybackAccessToken",
		"extensions": { "persistedQuery": { "version": 1, "sha256Hash": TWITCH_TOKEN_QUERY } },
		"variables": {
			"isLive": live, "login": if live { target.as_str() } else { "" },
			"isVod": !live, "vodID": if live { "" } else { target.as_str() },
			"playerType": "embed", "platform": "site",
		},
	});
	let reply: Value = client
		.post("https://gql.twitch.tv/gql")
		.header("Client-ID", TWITCH_CLIENT_ID)
		.header(USER_AGENT, UA_WEB)
		.json(&query)
		.send()
		.await
		.map_err(http)?
		.json()
		.await
		.map_err(http)?;
	let token = &reply["data"][if live { "streamPlaybackAccessToken" } else { "videoPlaybackAccessToken" }];
	let (Some(value), Some(sig)) = (token["value"].as_str(), token["signature"].as_str()) else {
		return Err(http(NOT_LIVE));
	};
	let mut usher = Url::parse(&if live {
		format!("https://usher.ttvnw.net/api/channel/hls/{target}.m3u8")
	} else {
		format!("https://usher.ttvnw.net/vod/{target}.m3u8")
	})
	.map_err(http)?;
	{
		let mut q = usher.query_pairs_mut();
		q.append_pair("platform", "web")
			.append_pair("p", &(std::process::id() % 999_999).to_string())
			.append_pair("allow_source", "true")
			.append_pair("allow_audio_only", "true")
			.append_pair("playlist_include_framerate", "true")
			.append_pair("supported_codecs", "h264");
		if live {
			q.append_pair("sig", sig).append_pair("token", value).append_pair("fast_bread", "true");
		} else {
			q.append_pair("nauthsig", sig).append_pair("nauth", value);
		}
	}
	let reply = client.get(usher).header(USER_AGENT, UA_WEB).send().await.map_err(http)?;
	// Usher answers 404 for a channel that is not broadcasting.
	if reply.status() == reqwest::StatusCode::NOT_FOUND {
		return Err(http(NOT_LIVE));
	}
	let master = reply.error_for_status().map_err(http)?.text().await.map_err(http)?;
	let stream = twitch_audio(&master).ok_or_else(|| CoreError::Decode("Unsupported format".into()))?;
	let name = serde_json::from_str::<Value>(value)
		.ok()
		.and_then(|v| v["channel"].as_str().map(String::from))
		.unwrap_or(target);
	Ok(Live { stream, name })
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn video_ids_and_pages() {
		assert_eq!(video_id("https://www.youtube.com/watch?v=4t4AQ15yj1M&t=3").as_deref(), Some("4t4AQ15yj1M"));
		assert_eq!(video_id("https://youtu.be/4t4AQ15yj1M").as_deref(), Some("4t4AQ15yj1M"));
		assert_eq!(video_id("https://www.youtube.com/live/4t4AQ15yj1M?si=x").as_deref(), Some("4t4AQ15yj1M"));
		assert_eq!(video_id("https://www.youtube.com/@LofiGirl/live"), None);
		assert_eq!(page_url("https://www.youtube.com/@LofiGirl"), "https://www.youtube.com/@LofiGirl/live");
		assert_eq!(page_url("https://www.youtube.com/@LofiGirl/live?x=1"), "https://www.youtube.com/@LofiGirl/live");
		assert_eq!(page_url("https://youtu.be/4t4AQ15yj1M"), "https://www.youtube.com/watch?v=4t4AQ15yj1M");
	}

	#[test]
	fn youtube_picks_aac_audio_group() {
		let master = "#EXTM3U\n\
			#EXT-X-MEDIA:URI=\"https://m/233.m3u8\",TYPE=AUDIO,GROUP-ID=\"233\",NAME=\"Default\"\n\
			#EXT-X-MEDIA:URI=\"https://m/234.m3u8\",TYPE=AUDIO,GROUP-ID=\"234\",NAME=\"Default\"\n\
			#EXT-X-STREAM-INF:BANDWIDTH=507418,CODECS=\"avc1.4D4015,mp4a.40.5\",AUDIO=\"233\"\nhttps://m/v1.m3u8\n";
		assert_eq!(youtube_audio(master).as_deref(), Some("https://m/234.m3u8"));
		let no_groups = "#EXT-X-STREAM-INF:BANDWIDTH=900,CODECS=\"x\"\nhttps://m/big\n#EXT-X-STREAM-INF:BANDWIDTH=300,CODECS=\"x\"\nhttps://m/small\n";
		assert_eq!(youtube_audio(no_groups).as_deref(), Some("https://m/small"));
	}

	#[test]
	fn twitch_picks_audio_only() {
		let master = "#EXTM3U\n\
			#EXT-X-MEDIA:TYPE=VIDEO,GROUP-ID=\"chunked\",NAME=\"1080p (source)\"\n\
			#EXT-X-STREAM-INF:BANDWIDTH=6000000,CODECS=\"avc1.64002A,mp4a.40.2\",VIDEO=\"chunked\"\nhttps://t/source.m3u8\n\
			#EXT-X-MEDIA:TYPE=VIDEO,GROUP-ID=\"audio_only\",NAME=\"audio_only\"\n\
			#EXT-X-STREAM-INF:BANDWIDTH=160000,CODECS=\"mp4a.40.2\",VIDEO=\"audio_only\"\nhttps://t/audio.m3u8\n";
		assert_eq!(twitch_audio(master).as_deref(), Some("https://t/audio.m3u8"));
		assert_eq!(twitch_target("https://www.twitch.tv/LofiGirl"), Some((true, "lofigirl".into())));
		assert_eq!(twitch_target("https://www.twitch.tv/videos/123"), Some((false, "123".into())));
	}

	/// Network: `cargo test -p blindradio-core live_direct_network -- --ignored --nocapture`.
	#[tokio::test]
	#[ignore]
	async fn live_direct_network() {
		let client = crate::net::client();
		for url in ["https://www.youtube.com/@LofiGirl/live", "https://www.twitch.tv/lofigirl", "https://www.twitch.tv/this_channel_is_not_live_x"] {
			let site = crate::live::site(url).unwrap();
			match lookup(&client, url, site).await {
				Ok(live) => {
					println!("{url}: {} -> {}", live.name, &live.stream[..80.min(live.stream.len())]);
					let playlist = client.get(&live.stream).send().await.unwrap().text().await.unwrap();
					assert!(playlist.starts_with("#EXTM3U"), "{playlist:.200}");
				}
				Err(e) => println!("{url}: {e}"),
			}
		}
	}

	#[tokio::test]
	#[ignore = "requires the reported broadcast to be live"]
	async fn reported_youtube_live_link_resolves_with_android_client() {
		let client = crate::net::client();
		let live = lookup(&client, "https://www.youtube.com/watch?v=_raDh9nyXVo", Site::YouTube)
			.await.unwrap();
		let opened = crate::net::open(&client, &live.stream).await.unwrap();
		assert!(matches!(opened, crate::net::Opened::Hls { .. }));
	}

	#[tokio::test]
	#[ignore = "network"]
	async fn talksport_resolves_with_android_client() {
		let client = crate::net::client();
		let live = lookup(&client, "https://www.youtube.com/watch?v=uLWEssCrXYc", Site::YouTube)
			.await
			.unwrap();
		assert_eq!(live.name, "talkSPORT");
		assert!(matches!(crate::net::open(&client, &live.stream).await.unwrap(), crate::net::Opened::Hls { .. }));
	}
}
