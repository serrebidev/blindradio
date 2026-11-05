//! Station pages publish player data separately from their ordinary links and recommendations.

use std::io::Read;

use base64::Engine;
use reqwest::Url;
use scraper::Html;
use serde_json::Value;

use crate::page::{is_local, selector};

pub(crate) const MAX_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Site {
	RadioNet,
	TuneIn,
	Zeno,
	Streema,
	LiveOnline,
}

fn page(url: &str) -> Option<(Site, String)> {
	let url = Url::parse(url).ok()?;
	if !matches!(url.scheme(), "http" | "https") {
		return None;
	}
	let host = url
		.host_str()?
		.strip_prefix("www.")
		.unwrap_or(url.host_str()?);
	let path: Vec<_> = url.path().trim_matches('/').split('/').collect();
	let (site, slug) = match (host, path.as_slice()) {
		("radio.net", ["s", slug]) => (Site::RadioNet, *slug),
		("tunein.com", ["radio", slug]) => {
			let id = slug.rsplit_once('-')?.1.strip_prefix('s')?;
			if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
				return None;
			}
			return Some((Site::TuneIn, format!("s{id}")));
		}
		("zeno.fm", ["radio", slug]) => (Site::Zeno, *slug),
		(h, ["radios", slug]) if h == "streema.com" || h.ends_with(".streema.com") => {
			(Site::Streema, *slug)
		}
		("liveonlineradio.net", [slug]) => (Site::LiveOnline, *slug),
		_ => return None,
	};
	(!slug.is_empty()).then(|| (site, slug.to_owned()))
}

pub(crate) fn is_page(url: &str) -> bool {
	page(url).is_some()
}

/// Reuse the same playlist endpoint and formats as the TuneIn directory.
pub(crate) fn tune_url(url: &str) -> Option<String> {
	let (site, id) = page(url)?;
	(site == Site::TuneIn)
		.then(|| format!("https://opml.radiotime.com/Tune.ashx?id={id}&formats=mp3,aac,ogg,hls"))
}

fn radio_net(value: &Value, slug: &str) -> Vec<String> {
	if value.get("id").and_then(Value::as_str) == Some(slug)
		&& let Some(streams) = value.get("streams").and_then(Value::as_array)
	{
		let mut streams: Vec<_> = streams.iter().collect();
		streams.sort_by_key(|s| s.get("status").and_then(Value::as_str) != Some("VALID"));
		return streams
			.iter()
			.filter_map(|s| s.get("url")?.as_str().map(str::to_owned))
			.collect();
	}
	let children: Vec<&Value> = match value {
		Value::Object(fields) => fields.values().collect(),
		Value::Array(items) => items.iter().collect(),
		_ => return vec![],
	};
	children
		.into_iter()
		.find_map(|child| {
			let urls = radio_net(child, slug);
			(!urls.is_empty()).then_some(urls)
		})
		.unwrap_or_default()
}

/// Nuxt stores field values by index in its payload array.
fn nuxt(data: &[Value], slug: &str, site: Site) -> Option<String> {
	for object in data.iter().filter_map(Value::as_object) {
		let field = |key: &str| data.get(object.get(key)?.as_u64()? as usize);
		if field("slug").and_then(Value::as_str) != Some(slug) {
			continue;
		}
		if site == Site::Zeno {
			if let Some(url) = field("streamURL")
				.and_then(Value::as_str)
				.filter(|s| !s.is_empty())
			{
				return Some(url.to_owned());
			}
			let mount = field("streamName")?.as_str()?;
			return (!mount.is_empty() && mount.bytes().all(|b| b.is_ascii_alphanumeric()))
				.then(|| format!("https://stream.zeno.fm/{mount}"));
		}
		let encoded = field("encoded_stream_url")?.as_str()?;
		let bytes = base64::engine::general_purpose::STANDARD
			.decode(encoded)
			.ok()?;
		let mut url = String::new();
		flate2::read::ZlibDecoder::new(bytes.as_slice())
			.take(8192)
			.read_to_string(&mut url)
			.ok()?;
		return Some(url);
	}
	None
}

/// Only the requested station's player data counts; other stations on the page never do.
pub(crate) fn streams(url: &str, body: &str) -> Option<Vec<String>> {
	let (site, slug) = page(url)?;
	if site == Site::TuneIn {
		return None;
	}
	let html = Html::parse_document(body);
	let mut urls = Vec::new();
	if site == Site::LiveOnline {
		urls.extend(
			html.select(&selector("#a-player[data-stream]"))
				.filter_map(|e| e.attr("data-stream").map(str::to_owned)),
		);
	} else {
		let mut flight = String::new();
		for script in html.select(&selector("script")) {
			let text = script.text().collect::<String>();
			if site == Site::RadioNet {
				if let Some(raw) = text
					.strip_prefix("self.__next_f.push(")
					.and_then(|s| s.strip_suffix(')'))
				{
					if let Ok(value) = serde_json::from_str::<Value>(raw)
						&& let Some(payload) = value.get(1).and_then(Value::as_str)
					{
						flight.push_str(payload);
					}
				} else if script.attr("id") == Some("__NEXT_DATA__")
					&& let Ok(value) = serde_json::from_str::<Value>(&text)
				{
					urls.extend(radio_net(&value, &slug));
				}
			} else if script.attr("id") == Some("__NUXT_DATA__")
				&& let Ok(data) = serde_json::from_str::<Vec<Value>>(&text)
			{
				urls.extend(nuxt(&data, &slug, site));
			}
		}
		for line in flight.lines() {
			if let Some((_, data)) = line.split_once(':')
				&& let Ok(value) = serde_json::from_str::<Value>(data)
			{
				urls.extend(radio_net(&value, &slug));
			}
		}
	}
	let base = Url::parse(url).ok()?;
	let mut seen = std::collections::HashSet::new();
	Some(
		urls.into_iter()
			.filter_map(|raw| base.join(raw.trim()).ok())
			.filter(|u| {
				matches!(u.scheme(), "http" | "https")
					&& u.host_str().is_some()
					&& u.username().is_empty()
					&& u.password().is_none()
					&& !is_local(u) && u != &base
			})
			.map(String::from)
			.filter(|u| seen.insert(u.clone()))
			.take(40)
			.collect(),
	)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn supplied_pages_resolve_only_the_requested_station() {
		let serrebi = "https://radio.serrebiradio.com/listen/serrebiradio/SerrebiRadio";
		for (url, body, expected) in [
			(
				"https://www.radio.net/s/wnyc",
				include_str!("../tests/fixtures/pages/radionet.html"),
				"https://fm939.wnyc.org/wnycfm-web",
			),
			(
				"https://zeno.fm/radio/serrebiradio/",
				include_str!("../tests/fixtures/pages/zeno.html"),
				"http://radio.serrebiradio.com/listen/serrebiradio/SerrebiRadio",
			),
			(
				"https://fr.streema.com/radios/SerrebiRadio.ca",
				include_str!("../tests/fixtures/pages/streema.html"),
				serrebi,
			),
			(
				"https://liveonlineradio.net/serrebiradio",
				include_str!("../tests/fixtures/pages/liveonline.html"),
				serrebi,
			),
		] {
			assert_eq!(streams(url, body), Some(vec![expected.to_owned()]), "{url}");
		}
		assert!(
			streams(
				"https://www.radio.net/s/missing",
				include_str!("../tests/fixtures/pages/radionet.html")
			)
			.unwrap()
			.is_empty()
		);
		assert!(
			streams(
				"https://zeno.fm/radio/missing/",
				include_str!("../tests/fixtures/pages/zeno.html")
			)
			.unwrap()
			.is_empty()
		);
	}

	#[test]
	fn tunein_links_use_the_directorys_playable_formats() {
		assert_eq!(
			tune_url("https://tunein.com/radio/SerrebiRadio-s357750/?foo=bar#player").as_deref(),
			Some("https://opml.radiotime.com/Tune.ashx?id=s357750&formats=mp3,aac,ogg,hls")
		);
		for url in [
			"https://tunein.com/radio/Name-s/",
			"https://tunein.com/radio/Name-s12x/",
			"https://tunein.com.evil.example/radio/Name-s123/",
			"ftp://tunein.com/radio/Name-s123/",
		] {
			assert!(tune_url(url).is_none(), "{url}");
		}
	}

	#[test]
	fn invalid_data_does_not_become_a_stream_or_follow_recommendations() {
		for body in [
			"<html>Gone</html>",
			r#"<script id="__NUXT_DATA__">[{"slug":1,"encoded_stream_url":2},"SerrebiRadio.ca","bad"]</script>"#,
		] {
			assert!(
				streams("https://fr.streema.com/radios/SerrebiRadio.ca", body)
					.unwrap()
					.is_empty()
			);
		}
		for target in [
			"javascript:alert(1)",
			"file:///C:/secret",
			"http://127.0.0.1/live",
			"http://192.168.1.1/live",
			"http://[::1]/live",
			"https://user:pass@audio.example/live",
			"https://liveonlineradio.net/serrebiradio",
		] {
			let body = format!(r#"<div id="a-player" data-stream="{target}"></div>"#);
			assert!(
				streams("https://liveonlineradio.net/serrebiradio", &body)
					.unwrap()
					.is_empty(),
				"{target}"
			);
		}
		assert!(streams("https://liveonlineradio.net.evil.example/serrebiradio", "").is_none());
	}

	#[test]
	fn zeno_mount_and_older_radionet_payloads_still_work() {
		let body = r#"<script id="__NUXT_DATA__">[{"slug":1,"streamURL":2,"streamName":3},"jazz","","abc123"]</script>"#;
		assert_eq!(
			streams("https://zeno.fm/radio/jazz/", body).unwrap(),
			["https://stream.zeno.fm/abc123"]
		);
		let body = r#"<script id="__NEXT_DATA__">{"station":{"id":"wnyc","streams":[{"url":"https://audio.example/bad","status":"INVALID"},{"url":"https://audio.example/good","status":"VALID"}]}}</script>"#;
		assert_eq!(
			streams("https://www.radio.net/s/wnyc", body).unwrap(),
			["https://audio.example/good", "https://audio.example/bad"]
		);
	}
}
