//! Search (Ctrl+F): every directory at once; the first result plays.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use blindradio_core::sources::{Filters, SearchQuery};
use blindradio_core::station::Station;
use wxdragon::prelude::*;

use crate::action::Action;
use crate::app::App;
use crate::bridge::WindowToken;
use crate::main_window::dispatch;
use crate::station_list::{Hooks, StationList, favorite_label_of};
use crate::strings as s;

#[derive(Clone, Copy)]
struct Fields {
	genre: ComboBox,
	country: ComboBox,
	language: ComboBox,
}

impl Fields {
	fn fill(&self, f: &Filters) {
		fn refs(v: &[String]) -> Vec<&str> {
			v.iter().map(String::as_str).collect()
		}
		// "Any" rather than a blank first item, which NVDA reads as "blank".
		for combo in [self.genre, self.country, self.language] {
			combo.append(s::ANY);
		}
		self.genre.append_items(&refs(&f.genres));
		self.country.append_items(&refs(&f.countries));
		self.language.append_items(&refs(&f.languages));
	}
}

/// An empty box and "Any" are the same: no filter.
fn filter(value: String) -> Option<String> {
	let value = value.trim();
	(!value.is_empty() && value != s::ANY).then(|| value.to_string())
}

pub fn show(app: &Rc<RefCell<App>>) -> Option<Action> {
	let frame = app.borrow().win.frame;
	let dialog = Dialog::builder(&frame, s::SEARCH_TITLE).build();
	let row = |label: &str, sizer: &BoxSizer| {
		let text = StaticText::builder(&dialog).with_label(label).build();
		sizer.add(
			&text,
			0,
			SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
			5,
		);
	};
	let sizer = BoxSizer::builder(Orientation::Vertical).build();
	// Each label is created just before the control it names.
	row(s::NAME, &sizer);
	let name = TextCtrl::builder(&dialog).build();
	sizer.add(&name, 0, SizerFlag::Expand | SizerFlag::All, 5);
	row(s::GENRE, &sizer);
	let genre = ComboBox::builder(&dialog).build();
	sizer.add(&genre, 0, SizerFlag::Expand | SizerFlag::All, 5);
	row(s::COUNTRY, &sizer);
	let country = ComboBox::builder(&dialog).build();
	sizer.add(&country, 0, SizerFlag::Expand | SizerFlag::All, 5);
	row(s::LANGUAGE, &sizer);
	let language = ComboBox::builder(&dialog)
		.with_style(ComboBoxStyle::ReadOnly)
		.build();
	sizer.add(&language, 0, SizerFlag::Expand | SizerFlag::All, 5);
	let search = Button::builder(&dialog)
		.with_label(s::SEARCH_BUTTON)
		.build();
	search.set_default();
	sizer.add(&search, 0, SizerFlag::All, 5);
	let results = StationList::new(&dialog, s::RESULTS);
	sizer.add(
		&results.label,
		0,
		SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
		5,
	);
	sizer.add(&results.list, 1, SizerFlag::Expand | SizerFlag::All, 5);
	let favorite = Button::builder(&dialog)
		.with_label(s::ADD_TO_FAVORITES)
		.build();
	let close = Button::builder(&dialog)
		.with_id(ID_CANCEL)
		.with_label(s::CLOSE)
		.build();
	let buttons = BoxSizer::builder(Orientation::Horizontal).build();
	buttons.add(&favorite, 0, SizerFlag::All, 5);
	buttons.add(&close, 0, SizerFlag::All, 5);
	sizer.add_sizer(&buttons, 0, SizerFlag::All, 0);
	dialog.set_sizer(sizer, true);
	dialog.set_size(Size::new(640, 560));

	let token = WindowToken::new();
	let fields = Fields {
		genre,
		country,
		language,
	};
	let cached = app.borrow().filters.clone();
	match cached {
		Some(f) => fields.fill(&f),
		None => {
			let a = app.borrow();
			let sources = a.sources.clone();
			a.bridge.spawn(
				token.clone(),
				async move { sources.filters().await },
				move |app: &mut App, f: Filters| {
					fields.fill(&f);
					// An empty answer is a failure: try again next time.
					if f != Filters::default() {
						app.filters = Some(f);
					}
				},
			);
		}
	}

	// A newer search wins over one still in flight.
	let generation = Rc::new(Cell::new(0u64));
	let (a, list, tok) = (app.clone(), results.clone(), token.clone());
	search.on_click(move |_| {
		let q = SearchQuery {
			name: name.get_value().trim().to_string(),
			genre: filter(fields.genre.get_value()),
			country: filter(fields.country.get_value()),
			language: filter(fields.language.get_value()),
		};
		generation.set(generation.get() + 1);
		let (mine, generation, list) = (generation.get(), generation.clone(), list.clone());
		let mut app = a.borrow_mut();
		app.say(s::SEARCHING);
		let sources = app.sources.clone();
		app.bridge.spawn(
			tok.clone(),
			async move { sources.search(&q).await },
			move |app: &mut App, found: Vec<Station>| {
				if generation.get() != mine {
					return;
				}
				if found.is_empty() {
					list.set_stations(Vec::new(), 0);
					return app.say(s::NO_RESULTS);
				}
				app.clear_message();
				list.set_stations(found, 0);
				list.list.set_focus();
				list.land(app);
				favorite.set_label(favorite_label_of(app, &list));
			},
		);
	});

	let (a, me) = (app.clone(), results.clone());
	favorite.on_click(move |_| {
		dispatch(&a, Action::ToggleFavorite);
		favorite.set_label(favorite_label_of(&a.borrow(), &me));
	});
	results.bind(
		app,
		Hooks {
			close: Some(dialog),
			on_focus: Some(Rc::new({
				let (a, me) = (app.clone(), results.clone());
				move |_| favorite.set_label(favorite_label_of(&a.borrow(), &me))
			})),
			..Hooks::default()
		},
	);

	name.set_focus();
	crate::dialogs::run(app, dialog, Action::OpenSearch, name, &token)
}

#[cfg(test)]
mod tests {
	use super::filter;
	use crate::strings as s;

	#[test]
	fn any_is_no_filter() {
		assert_eq!(filter(s::ANY.into()), None);
		assert_eq!(filter("  ".into()), None);
		assert_eq!(filter(" Jazz ".into()), Some("Jazz".into()));
	}
}
