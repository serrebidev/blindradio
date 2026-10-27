//! Station directories and the browse tree they share.

use crate::station::Station;

#[derive(Clone, Debug, PartialEq)]
pub enum Node {
	Folder { id: String, name: String },
	Station(Station),
	LoadMore { id: String },
}

pub mod iheart;
pub mod networks;
pub mod radio_browser;
pub mod tunein;

/// What the Search window sends to every directory. Empty `name` and unset filters are ignored.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SearchQuery {
	pub name: String,
	pub genre: Option<String>,
	pub country: Option<String>,
	pub language: Option<String>,
}

/// One entry of a station's published track history.
#[derive(Clone, Debug, PartialEq)]
pub struct HistoryItem {
	pub title: String,
	pub played_at: Option<std::time::SystemTime>,
}
