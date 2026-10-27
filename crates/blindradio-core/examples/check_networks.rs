//! Checks every curated network station live:
//! `cargo run -p blindradio-core --example check_networks --release`.
//! `--refresh-somafm` first rewrites the SomaFM entry of `data/networks.json` from SomaFM's channel list.

use std::sync::Arc;
use std::time::Duration;

use blindradio_core::net::{self, Opened};
use blindradio_core::sources::networks::{NetStation, Network, networks};
use serde::Deserialize;
use tokio::sync::Semaphore;

const PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/data/networks.json");
const PARALLEL: usize = 16;
const TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Deserialize)]
struct Channels {
	channels: Vec<Channel>,
}

#[derive(Deserialize)]
struct Channel {
	id: String,
	title: String,
	genre: String,
	playlists: Vec<Playlist>,
}

#[derive(Deserialize)]
struct Playlist {
	url: String,
	format: String,
	quality: String,
}

/// `Ok` once the stream answers: audio with a first chunk, or an HLS playlist.
async fn check(client: &reqwest::Client, url: &str) -> Result<(), String> {
	let opened = tokio::time::timeout(TIMEOUT, async {
		match net::open(client, url).await.map_err(|e| e.to_string())? {
			Opened::Hls { .. } => Ok(()),
			Opened::Audio { mut response, .. } => match response.chunk().await {
				Ok(Some(_)) => Ok(()),
				_ => Err("No audio".to_owned()),
			},
		}
	})
	.await;
	opened.unwrap_or_else(|_| Err("Timeout".into()))
}

fn genre(g: &str) -> String {
	g.split('|')
		.map(|w| {
			let mut c = w.chars();
			c.next()
				.map(|f| f.to_uppercase().chain(c).collect())
				.unwrap_or_default()
		})
		.collect::<Vec<String>>()
		.join(", ")
}

async fn refresh_somafm(client: &reqwest::Client) -> Result<(), Box<dyn std::error::Error>> {
	let list: Channels = net::get_json(client, "https://api.somafm.com/channels.json").await?;
	let mut stations = Vec::new();
	for c in list.channels {
		let direct = format!("https://ice1.somafm.com/{}-128-mp3", c.id);
		let url = match check(client, &direct).await {
			Ok(()) => direct,
			Err(e) => {
				let Some(p) = c
					.playlists
					.iter()
					.find(|p| p.format == "mp3" && p.quality == "highest")
				else {
					eprintln!("skip somafm/{}: {e}, no mp3 playlist", c.id);
					continue;
				};
				p.url.clone()
			}
		};
		stations.push(NetStation {
			slug: c.id.clone(),
			name: c.title,
			url,
			info: Some(format!("{}, MP3 128k", genre(&c.genre))),
			history: Some(format!("somafm:{}", c.id)),
		});
	}
	stations.sort_by_key(|s| s.name.to_lowercase());
	let mut all: Vec<Network> = serde_json::from_str(&std::fs::read_to_string(PATH)?)?;
	let soma = all
		.iter_mut()
		.find(|n| n.slug == "somafm")
		.ok_or("no somafm entry")?;
	println!("somafm: {} channels", stations.len());
	soma.stations = stations;
	std::fs::write(PATH, serde_json::to_string_pretty(&all)? + "\n")?;
	println!("wrote {PATH}; rebuild to check the new list");
	Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
	let client = net::client();
	if std::env::args().any(|a| a == "--refresh-somafm") {
		return refresh_somafm(&client).await;
	}
	let slots = Arc::new(Semaphore::new(PARALLEL));
	let mut tasks = tokio::task::JoinSet::new();
	for n in networks() {
		for s in &n.stations {
			let (client, slots) = (client.clone(), Arc::clone(&slots));
			let label = format!("{}/{}", n.slug, s.slug);
			let url = s.url.clone();
			tasks.spawn(async move {
				let _slot = slots.acquire_owned().await;
				(label, check(&client, &url).await)
			});
		}
	}
	let (mut ok, mut total) = (0, 0);
	while let Some(done) = tasks.join_next().await {
		let (label, result) = done?;
		total += 1;
		match result {
			Ok(()) => ok += 1,
			Err(e) => println!("FAIL {label} {e}"),
		}
	}
	println!("ok {ok}/{total}");
	Ok(())
}
