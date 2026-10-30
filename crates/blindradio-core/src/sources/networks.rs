//! Curated networks (BBC, NPR, SomaFM and others), shipped as data in `data/networks.json`.
//! A network with a single station is not listed; it is not a network.

use super::{Node, SearchQuery};
use crate::station::{Source, Station};
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Network {
	pub name: String,
	pub slug: String,
	pub stations: Vec<NetStation>,
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
	}
}

pub fn children(id: &str) -> Vec<Node> {
	if id == "net" {
		let mut folders: Vec<Node> = networks()
			.iter()
			.filter(|n| n.stations.len() > 1)
			.map(|n| Node::Folder {
				id: format!("net/{}", n.slug),
				name: n.name.clone(),
			})
			.collect();
		folders.sort_by_key(|n| n.name().to_lowercase());
		return folders;
	}
	let Some(slug) = id.strip_prefix("net/") else {
		return vec![];
	};
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
		.filter(|n| n.stations.len() > 1)
		.flat_map(|n| n.stations.iter().map(move |s| (n, s)))
		.filter(|(_, s)| s.name.to_lowercase().contains(&name))
		.map(|(n, s)| station(n, s))
		.collect()
}
