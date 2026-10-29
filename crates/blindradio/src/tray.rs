//! The tray icon: hide to it, come back from it, and play from it.

use std::cell::RefCell;
use std::rc::Rc;

use blindradio_core::station::Station;
use wxdragon::prelude::*;

use crate::action::Action;
use crate::app::App;
use crate::main_window::dispatch;
use crate::strings as s;

const ICON_SVG: &[u8] = include_bytes!("../assets/blindradio.svg");

/// What the tray menu offers, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TrayItem {
	PlayPause,
	Stop,
	Favorite(usize),
	Show,
	Exit,
}

/// The menu's shape: Play/Pause, Stop, the favorites, then Show and Exit.
pub fn menu_items(favorites: &[Station]) -> Vec<TrayItem> {
	let mut items = vec![TrayItem::PlayPause, TrayItem::Stop];
	items.extend((0..favorites.len()).map(TrayItem::Favorite));
	items.push(TrayItem::Show);
	items.push(TrayItem::Exit);
	items
}

fn icon_bundle() -> Option<BitmapBundle> {
	BitmapBundle::from_svg_data(ICON_SVG, Size::new(16, 16))
}

fn raise(app: &Rc<RefCell<App>>) {
	let a = app.borrow();
	a.win.frame.show(true);
	a.win.frame.raise();
	a.win.frame.set_focus();
}

fn run(app: &Rc<RefCell<App>>, item: TrayItem) {
	match item {
		TrayItem::PlayPause => dispatch(app, Action::PlayPause),
		TrayItem::Stop => dispatch(app, Action::Stop),
		TrayItem::Favorite(i) => {
			let station = app.borrow().library.favorites.get(i).cloned();
			if let Some(station) = station {
				app.borrow_mut().selected = Some(station);
				dispatch(app, Action::Play);
			}
		}
		TrayItem::Show => raise(app),
		TrayItem::Exit => dispatch(app, Action::Exit),
	}
}

/// Builds the menu from the same model the test checks; ids map a choice back.
fn fill(app: &App, ids: &mut Vec<(Id, TrayItem)>) -> Menu {
	let menu = Menu::builder().build();
	let playing = app.tuner.playing().is_some();
	for (index, item) in menu_items(&app.library.favorites).into_iter().enumerate() {
		// A separator before the favorites and before Show.
		if index > 0 && (matches!(item, TrayItem::Show) || matches!(item, TrayItem::Favorite(0))) {
			menu.append_separator();
		}
		let label = match &item {
			TrayItem::PlayPause => if playing { s::PAUSE } else { s::PLAY }.to_string(),
			TrayItem::Stop => s::STOP.to_string(),
			TrayItem::Favorite(i) => app.library.favorites[*i].name.replace('&', "&&"),
			TrayItem::Show => s::SHOW_WINDOW.to_string(),
			TrayItem::Exit => s::EXIT.to_string(),
		};
		let id = ID_HIGHEST + 9000 + ids.len() as Id;
		ids.push((id, item));
		menu.append(id, &label, "", ItemKind::Normal);
	}
	menu
}

/// The installed icon: keeping it keeps the tray entry.
pub struct Tray {
	icon: TaskBarIcon,
	/// wx holds the pointer; this keeps it alive, and replaces it when the menu changes.
	menu: RefCell<Menu>,
	ids: Rc<RefCell<Vec<(Id, TrayItem)>>>,
	/// What the menu was built from, so it is only rebuilt when that changes.
	built: RefCell<Option<(bool, Vec<String>)>>,
	tooltip: RefCell<String>,
}

impl Tray {
	/// Takes the icon out of the tray; dropping the handle alone does not.
	pub fn remove(&self) {
		self.icon.remove_icon();
	}

	/// Follows the playing state, the favorites and the now-playing line.
	pub fn update(&self, app: &App) {
		let now = (
			app.tuner.playing().is_some(),
			app.library
				.favorites
				.iter()
				.map(|st| st.id.clone())
				.collect::<Vec<_>>(),
		);
		if self.built.borrow().as_ref() != Some(&now) {
			*self.built.borrow_mut() = Some(now);
			let mut ids = self.ids.borrow_mut();
			ids.clear();
			let mut menu = fill(app, &mut ids);
			drop(ids);
			self.icon.set_popup_menu(&mut menu);
			*self.menu.borrow_mut() = menu;
		}
		let said = app.win.now_playing.get_label().replace('&', "");
		let tooltip = if said.is_empty() || said == s::STOPPED {
			s::APP_NAME.to_string()
		} else {
			format!("{} - {said}", s::APP_NAME)
		};
		if *self.tooltip.borrow() != tooltip
			&& let Some(bitmap) = icon_bundle().and_then(|b| b.get_bitmap(Size::new(16, 16)))
		{
			self.icon.set_icon(&bitmap, &tooltip);
			*self.tooltip.borrow_mut() = tooltip;
		}
	}
}

/// Installs the tray icon; `None` when the icon asset cannot be loaded.
pub fn install(app: &Rc<RefCell<App>>) -> Option<Tray> {
	let bundle = icon_bundle()?;
	let icon = TaskBarIcon::builder()
		.with_icon_bundle(bundle)
		.with_tooltip(s::APP_NAME)
		.build();
	let ids: Rc<RefCell<Vec<(Id, TrayItem)>>> = Rc::new(RefCell::new(Vec::new()));
	let mut menu = fill(&app.borrow(), &mut ids.borrow_mut());
	icon.set_popup_menu(&mut menu);

	// Bound before the icon moves into the handle. macOS has no click event in wxDragon, so the
	// menu's Show is the only way in there.
	#[cfg(target_os = "windows")]
	{
		// A single click is the mouse way back in. Keyboard activation of the
		// icon (Enter in the notification area) and a double-click arrive as
		// a double-click event instead, never as left-up: bind both, or the
		// window cannot be brought back without the menu.
		let a = app.clone();
		icon.on_left_up(move |_| raise(&a));
		let a = app.clone();
		icon.on_left_double_click(move |_| raise(&a));
	}
	#[cfg(target_os = "linux")]
	{
		let a = app.clone();
		icon.on_left_down(move |_| raise(&a));
	}
	{
		let (a, ids) = (app.clone(), ids.clone());
		icon.on_menu(move |e| {
			let id = e.get_id();
			let item = ids
				.borrow()
				.iter()
				.find(|(known, _)| *known == id)
				.map(|(_, item)| item.clone());
			if let Some(item) = item {
				run(&a, item);
			}
		});
	}

	let tray = Tray {
		icon,
		menu: RefCell::new(menu),
		ids,
		built: RefCell::new(None),
		tooltip: RefCell::new(String::new()),
	};
	tray.update(&app.borrow());
	Some(tray)
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
	fn tray_menu_items() {
		assert_eq!(
			menu_items(&[]),
			vec![
				TrayItem::PlayPause,
				TrayItem::Stop,
				TrayItem::Show,
				TrayItem::Exit
			],
			"no favorites: no favorites group"
		);
		let favorites = [station("a"), station("b")];
		assert_eq!(
			menu_items(&favorites),
			vec![
				TrayItem::PlayPause,
				TrayItem::Stop,
				TrayItem::Favorite(0),
				TrayItem::Favorite(1),
				TrayItem::Show,
				TrayItem::Exit
			],
			"favorites in order, before Show"
		);
	}
}
