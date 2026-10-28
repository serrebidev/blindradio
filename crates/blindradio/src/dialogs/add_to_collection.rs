//! Add to Collection (Ctrl+Shift+L): the collections, ticked where the station is.

use std::cell::RefCell;
use std::rc::Rc;

use blindradio_core::library::{CUSTOM, Library};
use blindradio_core::station::Station;
use wxdragon::prelude::*;

use crate::app::App;
use crate::context_menu::{self, Item};

/// Every collection but Custom Stations, with whether it holds `station`.
pub fn ticks(lib: &Library, station: &Station) -> Vec<(String, String, bool)> {
	lib.collections
		.iter()
		.filter(|c| c.id != CUSTOM)
		.map(|c| {
			let holds = c.stations.iter().any(|x| x.id == station.id);
			(c.id.clone(), c.name.clone(), holds)
		})
		.collect()
}

/// The popup for `station`: a tick per collection, then New Collection.
pub fn choose(app: &Rc<RefCell<App>>, parent: &dyn WxWidget, station: &Station) {
	let items = vec![
		Item::Collections(ticks(&app.borrow().library, station)),
		Item::NewCollection,
	];
	if let Some(choice) = context_menu::popup(parent, &items) {
		context_menu::apply(app, parent, station, choice);
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use blindradio_core::station::Source;

	fn station(id: &str) -> Station {
		Station {
			id: id.into(),
			name: id.into(),
			url: format!("http://{id}"),
			source: Source::RadioBrowser,
			info: None,
		}
	}

	#[test]
	fn add_to_collection_menu_ticks() {
		let mut lib = Library::default();
		let jazz = lib.create_collection("Jazz");
		lib.create_collection("News");
		let st = station("rb:1");
		lib.toggle_in_collection(&jazz, &st);
		lib.add_custom("Mine", "http://mine");
		assert_eq!(
			ticks(&lib, &st),
			vec![
				(jazz.clone(), "Jazz".into(), true),
				(lib.collections[2].id.clone(), "News".into(), false),
			],
			"Custom Stations is never offered"
		);
		assert!(
			ticks(&lib, &station("rb:2"))
				.iter()
				.all(|(_, _, held)| !held)
		);
		// What the popup offers for a station.
		let items = [Item::Collections(ticks(&lib, &st)), Item::NewCollection];
		assert!(matches!(&items[0], Item::Collections(v) if v.len() == 2));
	}
}
