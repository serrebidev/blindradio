//! Curated networks (BBC, NPR, SomaFM and others), shipped as data in `data/networks.json`.
//! A network with a single station is not listed; it is not a network — unless
//! the service is flagged `listed`, for named single-station services.

use super::{Node, SearchQuery};
use crate::station::{Source, Station};
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Network {
	pub name: String,
	pub slug: String,
	/// Named single-station services stay listed despite the >1 rule.
	#[serde(default, skip_serializing_if = "std::ops::Not::not")]
	pub listed: bool,
	/// The language folder it is browsed under: a `LANGUAGES` code. Unset is English.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub language: Option<String>,
	pub stations: Vec<NetStation>,
}

/// Networks browse by language: `net/en` and so on. A network's own folder stays `net/<slug>`,
/// so saved Sources switches keep working.
pub const LANGUAGES: [(&str, &str); 3] = [("en", "English"), ("pl", "Polish"), ("ru", "Russian")];

/// Playlists browsed beside a language's networks: language, browse id, name.
const PLAYLISTS: [(&str, &str, &str); 1] = [("pl", super::rstreams::ROOT, super::rstreams::NAME)];

fn language_of(n: &Network) -> &str {
	n.language.as_deref().unwrap_or("en")
}

/// Listed in Browse and search: multi-station networks, plus flagged services.
fn is_listed(n: &Network) -> bool {
	n.stations.len() > 1 || n.listed
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NetStation {
	pub slug: String,
	pub name: String,
	pub url: String,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub info: Option<String>,
	/// Where published song history comes from, e.g. `somafm:groovesalad`.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub history: Option<String>,
}

const JSON: &str = include_str!("../../data/networks.json");

static NETWORKS: LazyLock<Vec<Network>> =
	LazyLock::new(|| serde_json::from_str(JSON).expect("data/networks.json"));

pub fn networks() -> &'static [Network] {
	&NETWORKS
}

fn station(n: &Network, s: &NetStation) -> Station {
	Station {
		id: format!("net:{}:{}", n.slug, s.slug),
		name: s.name.clone(),
		url: s.url.clone(),
		source: Source::Network(n.name.clone()),
		info: s.info.clone(),
		details: Default::default(),
	}
}

/// Every listed network's folder in name order, whatever its language: the Sources switches.
pub fn folders() -> Vec<Node> {
	language_folders(None)
}

fn language_folders(language: Option<&str>) -> Vec<Node> {
	let folder = |id: String, name: &str| Node::Folder { id, name: name.to_owned() };
	let mut folders: Vec<Node> = networks()
		.iter()
		.filter(|n| is_listed(n) && language.is_none_or(|l| language_of(n) == l))
		.map(|n| folder(format!("net/{}", n.slug), &n.name))
		.chain(
			PLAYLISTS.iter()
				.filter(|(l, ..)| language.is_none_or(|language| language == *l))
				.map(|(_, id, name)| folder((*id).to_owned(), name)),
		)
		.collect();
	folders.sort_by_key(|n| n.name().to_lowercase());
	folders
}

pub fn children(id: &str) -> Vec<Node> {
	if id == "net" {
		return LANGUAGES
			.iter()
			.map(|(code, name)| Node::Folder { id: format!("net/{code}"), name: (*name).to_owned() })
			.collect();
	}
	let Some(slug) = id.strip_prefix("net/") else {
		return vec![];
	};
	if LANGUAGES.iter().any(|(code, _)| *code == slug) {
		return language_folders(Some(slug));
	}
	let mut stations: Vec<Node> = networks()
		.iter()
		.filter(|n| n.slug == slug)
		.flat_map(|n| n.stations.iter().map(|s| Node::Station(station(n, s))))
		.collect();
	stations.sort_by_key(|n| n.name().to_lowercase());
	stations
}

/// Name only; a blank name finds nothing rather than every station.
pub fn search(q: &SearchQuery) -> Vec<Station> {
	let name = q.name.trim().to_lowercase();
	if name.is_empty() {
		return vec![];
	}
	networks()
		.iter()
		.filter(|n| is_listed(n))
		.flat_map(|n| n.stations.iter().map(move |s| (n, s)))
		.filter(|(_, s)| s.name.to_lowercase().contains(&name))
		.map(|(n, s)| station(n, s))
		.collect()
}
