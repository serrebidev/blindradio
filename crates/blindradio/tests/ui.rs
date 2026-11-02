//! UI automation over the real window: needs a desktop, so `#[ignore]` and one at a time.
//! Run: `cargo test -p blindradio --test ui -- --ignored --test-threads=1`
#![cfg(windows)]

use std::path::PathBuf;
use std::process::{Child, Command};
use std::time::Duration;

use blindradio_core::library::{Folder, Library};
use blindradio_core::settings::Settings;
use blindradio_core::station::{Source, Station};
use uiautomation::actions::{ExpandCollapse, SelectionItem};
use uiautomation::controls::{ListItemControl, TreeItemControl, WindowControl};
use uiautomation::inputs::Keyboard;
use uiautomation::types::ControlType;
use uiautomation::{UIAutomation, UIElement};

const WAIT: Duration = Duration::from_secs(20);

#[link(name = "user32")]
unsafe extern "system" {
	fn keybd_event(key: u8, scan: u8, flags: u32, extra: usize);
}

fn station(id: &str, name: &str) -> Station {
	Station {
		id: id.into(),
		name: name.into(),
		url: format!("http://127.0.0.1:9/{id}"),
		source: Source::Custom,
		info: None,
	}
}

/// The app on its own data folder, killed and cleaned up on drop.
struct App {
	child: Child,
	automation: UIAutomation,
	data: PathBuf,
	window: UIElement,
}

impl App {
	fn start(name: &str, library: Library) -> App {
		Self::with_settings(name, library, Settings::default())
	}

	fn with_settings(name: &str, library: Library, mut settings: Settings) -> App {
		let data =
			std::env::temp_dir().join(format!("blindradio-ui-{}-{name}", std::process::id()));
		let _ = std::fs::remove_dir_all(&data);
		std::fs::create_dir_all(&data).expect("data folder");
		library.save(&data).expect("library");
		settings.check_updates = false;
		settings.muted = true;
		settings.save(&data).expect("settings");
		let child = Command::new(env!("CARGO_BIN_EXE_blindradio"))
			.env("BLINDRADIO_DATA", &data)
			.spawn()
			.expect("the app starts");
		let automation = UIAutomation::new().expect("UIAutomation");
		let window = automation
			.create_matcher()
			.from(automation.get_root_element().expect("root"))
			.timeout(WAIT.as_millis() as u64)
			.filter_fn(Box::new({
				let pid = child.id();
				move |e: &UIElement| Ok(e.get_process_id()? == pid)
			}))
			.name("BlindRadio")
			.control_type(ControlType::Window)
			.find_first()
			.expect("the window appears");
		if let Ok(window) = WindowControl::try_from(window.clone()) {
			let _ = window.set_foregrand();
		}
		window.set_focus().ok();
		App {
			child,
			automation,
			data,
			window,
		}
	}

	/// A window or dialog with this name, if it shows up within `wait`.
	fn find(&self, name: &str, wait: Duration) -> Option<UIElement> {
		self.automation
			.create_matcher()
			.from(self.automation.get_root_element().ok()?)
			.timeout(wait.as_millis() as u64)
			.filter_fn(Box::new({
				let pid = self.child.id();
				move |e: &UIElement| Ok(e.get_process_id()? == pid)
			}))
			.name(name)
			.control_type(ControlType::Window)
			.find_first()
			.ok()
	}

	/// A control of the main window with this name.
	fn control(&self, name: &str, kind: ControlType, wait: Duration) -> Option<UIElement> {
		self.automation
			.create_matcher()
			.from(self.window.clone())
			.timeout(wait.as_millis() as u64)
			.filter_fn(Box::new({
				let pid = self.child.id();
				move |e: &UIElement| Ok(e.get_process_id()? == pid)
			}))
			.name(name)
			.control_type(kind)
			.find_first()
			.ok()
	}

	fn has_menu(&self, wait: Duration) -> bool {
		self.automation
			.create_matcher()
			.from(self.automation.get_root_element().expect("root"))
			.timeout(wait.as_millis() as u64)
			.control_type(ControlType::Menu)
			.find_first()
			.is_ok()
	}

	fn press(&self, keys: &str) {
		// UIAutomation's sender omits extended-key flags, turning arrows into numpad keys.
		let arrow = match keys {
			"{down}" => Some((0x28, 0x50, false)),
			"{delete}" => Some((0x2e, 0x53, false)),
			"{alt}{left}" => Some((0x25, 0x4b, true)),
			"{alt}{right}" => Some((0x27, 0x4d, true)),
			_ => None,
		};
		let mut keyboard = Keyboard::new().interval(60);
		if let Some((key, scan, alt)) = arrow {
			if alt {
				keyboard.begin_hold_keys("{alt}").expect("Alt");
			}
			// SAFETY: inject a matched extended-key down/up pair on this test's desktop.
			unsafe {
				keybd_event(key, scan, 1, 0);
				keybd_event(key, scan, 3, 0);
			}
			if alt {
				keyboard.end_hold_keys().expect("release Alt");
			}
		} else {
			keyboard.send_keys(keys).expect("keys");
		}
		std::thread::sleep(Duration::from_millis(400));
	}

	fn row(&self, parent: &UIElement, name: &str) -> UIElement {
		self.automation
			.create_matcher()
			.from(parent.clone())
			.timeout(5000)
			.name(name)
			.control_type(ControlType::TreeItem)
			.find_first()
			.unwrap_or_else(|e| panic!("tree row {name}: {e}"))
	}

	fn select_row(&self, parent: &UIElement, name: &str) -> UIElement {
		let row = self.row(parent, name);
		TreeItemControl::try_from(row.clone())
			.unwrap()
			.select()
			.expect("select row");
		row.set_focus().expect("focus row");
		std::thread::sleep(Duration::from_millis(400));
		row
	}

	fn assert_selected(&self, tree: &UIElement, name: &str) {
		let row = self.row(tree, name);
		assert!(
			TreeItemControl::try_from(row.clone())
				.unwrap()
				.is_selected()
				.unwrap(),
			"{name} selected"
		);
		assert!(!row.is_offscreen().unwrap(), "{name} revealed");
	}
}

impl Drop for App {
	fn drop(&mut self) {
		let _ = self.child.kill();
		let _ = self.child.wait();
		let _ = std::fs::remove_dir_all(&self.data);
	}
}

#[test]
#[ignore = "needs a desktop"]
fn starts_on_play_button() {
	// No SerrebiRadio list: the dial is empty, so focus is on Play.
	let app = App::start("play", Library::default());
	let play = app
		.control("Play", ControlType::Button, Duration::from_secs(5))
		.expect("Play");
	assert!(play.has_keyboard_focus().unwrap_or(false), "starts focused");
}

#[test]
#[ignore = "needs a desktop"]
fn search_dialog_opens_and_escape_closes() {
	let app = App::start("search", Library::default());
	app.press("{ctrl}f");
	assert!(
		app.find("Search", Duration::from_secs(5)).is_some(),
		"Search opens"
	);
	app.press("{esc}");
	std::thread::sleep(Duration::from_millis(600));
	assert!(
		app.find("Search", Duration::from_millis(400)).is_none(),
		"Escape closes it"
	);
}

#[test]
#[ignore = "needs a desktop"]
fn favorites_dial_tunes_on_arrow() {
	let mut library = Library::default();
	library.toggle_favorite(&station("custom:one", "One"));
	library.toggle_favorite(&station("custom:two", "Two"));
	let app = App::start("favorites", library);
	app.press("{ctrl}b");
	assert!(
		app.find("Favorites", Duration::from_secs(5)).is_some(),
		"Favorites opens"
	);
	// Landing on the first favorite plays it; Down is the second.
	assert!(
		app.window.get_name().unwrap().contains("One"),
		"first plays"
	);
	app.press("{down}");
	assert!(
		app.window.get_name().unwrap().contains("Two"),
		"the next station plays"
	);
}

#[test]
#[ignore = "needs a desktop"]
fn context_menu_on_category_says_nothing_to_do() {
	let app = App::start("menu", Library::default());
	app.press("{ctrl}g");
	let browse = app
		.find("Browse", Duration::from_secs(5))
		.expect("Browse opens");
	// The tree opens on Radio Browser, a category: no menu, one message.
	browse.send_keys("{shift}{f10}", 60).expect("Applications");
	assert!(
		app.control(
			"Nothing to do here",
			ControlType::Text,
			Duration::from_secs(5)
		)
		.is_some(),
		"the message line says it"
	);
	assert!(
		!app.has_menu(Duration::from_millis(400)),
		"and no menu opened"
	);
}

#[test]
#[ignore = "needs a desktop"]
fn one_window_at_a_time() {
	let app = App::start("windows", Library::default());
	app.press("{ctrl}f");
	assert!(app.find("Search", Duration::from_secs(5)).is_some());
	app.press("{ctrl}b");
	assert!(
		app.find("Favorites", Duration::from_secs(5)).is_some(),
		"Favorites opens"
	);
	assert!(
		app.find("Search", Duration::from_millis(400)).is_none(),
		"Search closed"
	);
}

#[test]
#[ignore = "needs a desktop"]
fn second_instance_raises_first() {
	let app = App::start("single", Library::default());
	// Hide it, then start another copy: that one exits and this one comes back.
	app.press("{ctrl}{alt}{shift}y");
	let mut second = Command::new(env!("CARGO_BIN_EXE_blindradio"))
		.env("BLINDRADIO_DATA", &app.data)
		.spawn()
		.expect("the second copy starts");
	let mut code = None;
	for _ in 0..60 {
		if let Some(status) = second.try_wait().expect("wait") {
			code = status.code();
			break;
		}
		std::thread::sleep(Duration::from_millis(50));
	}
	let _ = second.kill();
	assert_eq!(code, Some(0), "the second copy exits by itself");
	assert!(
		app.find("BlindRadio", Duration::from_secs(3)).is_some(),
		"the first copy is shown again"
	);
}

#[test]
#[ignore = "needs a desktop"]
fn browse_pastes_from_the_selected_collection_and_refreshes_in_place() {
	let mut library = Library::default();
	let copy = station("custom:copy", "Copy me");
	let cut = station("custom:cut", "Cut me");
	library.collections[0].stations = vec![copy.clone(), cut.clone()];
	let id = library.create_collection("English");
	library.collection_mut(&id).unwrap().folders.push(Folder {
		name: "Misc".into(),
		stations: vec![copy, cut],
		folders: Vec::new(),
	});
	let app = App::with_settings(
		"paste-refresh",
		library,
		Settings {
			play_on_move: false,
			..Settings::default()
		},
	);
	app.press("{ctrl}g");
	let browse = app.find("Browse", WAIT).expect("Browse");
	let collections = app.row(&browse, "Collections");
	TreeItemControl::try_from(collections)
		.unwrap()
		.expand()
		.unwrap();
	let english = app.row(&browse, "English 2");
	TreeItemControl::try_from(english.clone())
		.unwrap()
		.expand()
		.unwrap();
	let misc = app.row(&english, "Misc 2");
	TreeItemControl::try_from(misc.clone())
		.unwrap()
		.expand()
		.unwrap();
	for (name, chord, remaining) in [("Copy me", "{ctrl}c", 2), ("Cut me", "{ctrl}x", 1)] {
		let misc = app.row(&browse, "Misc 2");
		TreeItemControl::try_from(misc.clone())
			.unwrap()
			.expand()
			.unwrap();
		app.select_row(&misc, name).send_keys(chord, 60).unwrap();
		app.select_row(
			&browse,
			if name == "Copy me" {
				"English 2"
			} else {
				"English 3"
			},
		)
		.send_keys("{ctrl}v", 60)
		.unwrap();
		std::thread::sleep(Duration::from_millis(500));
		let saved = Library::load(&app.data);
		let collection = saved.collections.iter().find(|c| c.id == id).unwrap();
		assert!(
			collection.stations.iter().any(|s| s.name == name),
			"paste reaches English root"
		);
		assert!(
			saved.collections[0].stations.iter().any(|s| s.name == name),
			"Custom copy retained"
		);
		assert_eq!(collection.folders[0].stations.len(), remaining);
		let english = app.row(&browse, "English 3");
		let root_row = app.row(&english, name);
		let walker = app.automation.create_tree_walker().unwrap();
		assert!(
			app.automation
				.compare_elements(&walker.get_parent(&root_row).unwrap(), &english)
				.unwrap(),
			"pasted row is at English root"
		);
		app.row(&browse, &format!("Misc {remaining}"));
	}
}

#[test]
#[ignore = "needs a desktop"]
fn move_to_playing_station_returns_from_another_row() {
	let mut library = Library::default();
	let one = station("custom:one", "One");
	library.collections[0].stations = vec![one.clone(), station("custom:two", "Two")];
	library.favorites.push(one);
	let app = App::with_settings("move-playing", library, Settings {
		main_list: Some("custom".into()), play_on_move: false, ..Settings::default()
	});
	app.press("{alt}1");
	let tree = app.automation.create_matcher().from(app.window.clone())
		.control_type(ControlType::Tree).timeout(5000).find_first().unwrap();
	app.select_row(&tree, "Two");
	app.assert_selected(&tree, "Two");
	assert!(app.window.get_name().unwrap().contains("One"));
	for _ in 0..2 {
		app.press("{alt}s");
		app.press("m");
		app.assert_selected(&tree, "One");
		assert!(app.row(&tree, "One").has_keyboard_focus().unwrap());
	}
}

#[test]
#[ignore = "needs a desktop"]
fn playback_reveals_the_main_tree_and_file_exports_are_reachable() {
	let mut library = Library::default();
	let one = station("custom:one", "One");
	let two = station("custom:two", "Two");
	library.favorites = vec![one.clone(), two.clone()];
	library.collections[0].folders.push(Folder {
		name: "Hidden".into(),
		stations: vec![one, two],
		folders: Vec::new(),
	});
	let app = App::with_settings(
		"selection",
		library,
		Settings {
			main_list: Some("custom".into()),
			expand_dial_folders: false,
			..Settings::default()
		},
	);
	let tree = app
		.automation
		.create_matcher()
		.from(app.window.clone())
		.control_type(ControlType::Tree)
		.timeout(5000)
		.find_first()
		.expect("main tree");
	for (keys, name) in [
		("{alt}1", "One"),
		("{alt}2", "Two"),
		("{alt}{left}", "One"),
		("{alt}{right}", "Two"),
	] {
		app.press(keys);
		app.assert_selected(&tree, name);
	}
	app.press("{ctrl}b");
	let favorites = app.find("Favorites", WAIT).expect("Favorites");
	app.select_row(&favorites, "One")
		.send_keys("{enter}", 60)
		.unwrap();
	app.press("{esc}");
	std::thread::sleep(Duration::from_millis(400));
	app.assert_selected(&tree, "One");
	app.press("{ctrl}h");
	let recent = app.find("Recently Played", WAIT).expect("Recently Played");
	let two = app
		.automation
		.create_matcher()
		.from(recent)
		.name("Two")
		.control_type(ControlType::ListItem)
		.timeout(5000)
		.find_first()
		.unwrap();
	ListItemControl::try_from(two.clone())
		.unwrap()
		.select()
		.unwrap();
	two.set_focus().unwrap();
	app.press("{enter}");
	if app
		.find("Recently Played", Duration::from_millis(200))
		.is_some()
	{
		app.press("{esc}");
	}
	std::thread::sleep(Duration::from_millis(400));
	app.assert_selected(&tree, "Two");
	app.press("{ctrl}g");
	let browse = app.find("Browse", WAIT).expect("Browse");
	let custom = app.row(&browse, "Custom Stations 2");
	TreeItemControl::try_from(custom.clone())
		.unwrap()
		.expand()
		.unwrap();
	let hidden = app.row(&custom, "Hidden 2");
	TreeItemControl::try_from(hidden.clone())
		.unwrap()
		.expand()
		.unwrap();
	app.select_row(&hidden, "One")
		.send_keys("{enter}", 60)
		.unwrap();
	app.press("{esc}");
	std::thread::sleep(Duration::from_millis(400));
	app.assert_selected(&tree, "One");
	app.press("{alt}s");
	let move_item = app
		.automation
		.create_matcher()
		.from(app.automation.get_root_element().unwrap())
		.filter_fn(Box::new({
			let pid = app.child.id();
			move |e: &UIElement| Ok(e.get_process_id()? == pid)
		}))
		.control_type(ControlType::MenuItem)
		.name("Move to Playing Station")
		.timeout(5000)
		.find_first()
		.expect("Stations menu command");
	move_item.send_keys("m", 60).unwrap();
	std::thread::sleep(Duration::from_millis(400));
	app.assert_selected(&tree, "One");
	app.press("{alt}f");
	for name in [
		"Export Current Playlist…",
		"Export All Playlists…",
		"Export Favorites…",
	] {
		app.automation
			.create_matcher()
			.from(app.automation.get_root_element().unwrap())
			.filter_fn(Box::new({
				let pid = app.child.id();
				move |e: &UIElement| Ok(e.get_process_id()? == pid)
			}))
			.control_type(ControlType::MenuItem)
			.name(name)
			.timeout(5000)
			.find_first()
			.unwrap_or_else(|e| panic!("{name}: {e}"));
	}
	app.press("{esc}");
	app.press("{alt}1");
	app.assert_selected(&tree, "One");
	tree.set_focus().unwrap();
	app.press("{delete}");
	app.assert_selected(&tree, "Two");
	assert!(
		app.window.get_name().unwrap().contains("Two"),
		"deleting the playing station tunes its replacement"
	);
	assert!(!Library::load(&app.data).collections[0].contains_id("custom:one"));
}
