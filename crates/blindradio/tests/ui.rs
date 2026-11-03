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
use uiautomation::patterns::{UIInvokePattern, UIValuePattern};
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
			"{down}" => Some((0x28, 0x50, None)),
			"{left}" => Some((0x25, 0x4b, None)),
			"{right}" => Some((0x27, 0x4d, None)),
			"{delete}" => Some((0x2e, 0x53, None)),
			"{space}" => Some((0x20, 0x39, None)),
			"{alt}{left}" => Some((0x25, 0x4b, Some("{alt}"))),
			"{alt}{right}" => Some((0x27, 0x4d, Some("{alt}"))),
			"{shift}{up}" => Some((0x26, 0x48, Some("{shift}"))),
			"{shift}{down}" => Some((0x28, 0x50, Some("{shift}"))),
			"{ctrl}{up}" => Some((0x26, 0x48, Some("{ctrl}"))),
			_ => None,
		};
		let mut keyboard = Keyboard::new().interval(60);
		if let Some((key, scan, hold)) = arrow {
			if let Some(hold) = hold {
				keyboard.begin_hold_keys(hold).expect("hold modifier");
			}
			// SAFETY: inject a matched extended-key down/up pair on this test's desktop.
			unsafe {
				keybd_event(key, scan, 1, 0);
				keybd_event(key, scan, 3, 0);
			}
			if hold.is_some() {
				keyboard.end_hold_keys().expect("release modifier");
			}
		} else {
			keyboard.send_keys(keys).expect("keys");
		}
		std::thread::sleep(Duration::from_millis(400));
	}

	/// One Down with no wait after it, for timing what follows.
	fn tap_down(&self) {
		// SAFETY: inject a matched extended-key down/up pair on this test's desktop.
		unsafe {
			keybd_event(0x28, 0x50, 1, 0);
			keybd_event(0x28, 0x50, 3, 0);
		}
	}

	fn main_tree(&self) -> UIElement {
		self.automation
			.create_matcher()
			.from(self.window.clone())
			.control_type(ControlType::Tree)
			.timeout(5000)
			.find_first()
			.expect("main tree")
	}

	/// An item of the open menu.
	fn menu_item(&self, name: &str) -> UIElement {
		self.automation
			.create_matcher()
			.from(self.automation.get_root_element().unwrap())
			.filter_fn(Box::new({
				let pid = self.child.id();
				move |e: &UIElement| Ok(e.get_process_id()? == pid)
			}))
			.control_type(ControlType::MenuItem)
			.name(name)
			.timeout(5000)
			.find_first()
			.unwrap_or_else(|e| panic!("menu item {name}: {e}"))
	}

	fn invoke(&self, element: &UIElement) {
		element
			.get_pattern::<UIInvokePattern>()
			.expect("invokable")
			.invoke()
			.expect("invoke");
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
fn volume_is_saved_at_rest_and_on_an_immediate_exit() {
	let mut app = App::start("volume-save", Library::default());
	let volume = |app: &App| Settings::load(&app.data).volume;
	let first = volume(&app);
	app.press("{ctrl}{up}");
	std::thread::sleep(Duration::from_millis(1500));
	let rested = volume(&app);
	assert!(rested > first, "written once the key rests");
	app.press("{ctrl}{up}");
	app.press("{alt}{f4}");
	let mut exited = false;
	for _ in 0..100 {
		if app.child.try_wait().expect("wait").is_some() {
			exited = true;
			break;
		}
		std::thread::sleep(Duration::from_millis(50));
	}
	assert!(exited, "Alt+F4 exits");
	assert!(volume(&app) > rested, "written on exit");
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

/// Name Automatically fills the name field from the stream's ICY name.
#[test]
#[ignore = "needs a desktop"]
fn name_automatically_fills_the_name_from_the_stream() {
	use std::io::{Read, Write};
	let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
	let port = listener.local_addr().unwrap().port();
	std::thread::spawn(move || {
		while let Ok((mut sock, _)) = listener.accept() {
			let mut request = [0; 2048];
			let _ = sock.read(&mut request);
			let _ = sock.write_all(
				b"HTTP/1.1 200 OK\r\nContent-Type: audio/mpeg\r\nicy-name: Radio Named Itself\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
			);
		}
	});
	let app = App::start("name-auto", Library::default());
	app.press("{ctrl}n");
	let dialog = app
		.find("Add Custom Station", WAIT)
		.expect("Add Custom Station");
	let control = |name: &str, kind| {
		app.automation
			.create_matcher()
			.from(dialog.clone())
			.timeout(5000)
			.name(name)
			.control_type(kind)
			.find_first()
			.unwrap_or_else(|e| panic!("{name}: {e}"))
	};
	let value = |e: &UIElement| e.get_pattern::<UIValuePattern>().expect("value");
	let name = control("Name", ControlType::Edit);
	let url = control("Stream URL", ControlType::Edit);
	value(&url)
		.set_value(&format!("http://127.0.0.1:{port}/stream"))
		.expect("type the URL");
	app.invoke(&control("Name Automatically", ControlType::Button));
	let mut found = String::new();
	for _ in 0..50 {
		found = value(&name).get_value().expect("name");
		if !found.is_empty() {
			break;
		}
		std::thread::sleep(Duration::from_millis(100));
	}
	assert_eq!(found, "Radio Named Itself");
	// Still an ordinary field.
	value(&name).set_value("My Own Name").expect("edit the name");
	assert_eq!(value(&name).get_value().unwrap(), "My Own Name");
}

/// A buffer setting's slider and field follow each other, and OK saves what they show.
#[test]
#[ignore = "needs a desktop"]
fn buffer_slider_and_field_stay_together() {
	let app = App::start("buffers", Library::default());
	let control = |window: &UIElement, name: &str, kind| {
		app.automation
			.create_matcher()
			.from(window.clone())
			.timeout(5000)
			.name(name)
			.control_type(kind)
			.find_first()
			.unwrap_or_else(|e| panic!("{name}: {e}"))
	};
	let field_text = |field: &UIElement| {
		field
			.get_pattern::<UIValuePattern>()
			.expect("value")
			.get_value()
			.expect("text")
	};
	let saved = || Settings::load(&app.data).buffers.start_ms;
	assert_eq!(saved(), 1000, "the default is what it always was");

	// A just-built exe can still be starting when the window first shows, and lose the key.
	let dialog = (0..4)
		.find_map(|_| {
			app.press("{ctrl}t");
			app.find("Settings", Duration::from_secs(5))
		})
		.expect("Settings");
	app.press("{right}");
	let field = control(&dialog, "Start buffer, in milliseconds:", ControlType::Edit);
	let slider = control(
		&dialog,
		"Start buffer, in milliseconds slider",
		ControlType::Slider,
	);
	slider.set_focus().expect("focus slider");
	std::thread::sleep(Duration::from_millis(300));
	app.press("{right}");
	assert_eq!(field_text(&field), "1100", "the slider moves the field, upward");
	app.press("{left}");
	app.press("{left}");
	assert_eq!(field_text(&field), "900");
	app.invoke(&control(&dialog, "OK", ControlType::Button));
	std::thread::sleep(Duration::from_millis(500));
	assert_eq!(saved(), 900);

	// Any number can be typed; the same field reads back what the window saved.
	app.press("{ctrl}t");
	let dialog = app.find("Settings", WAIT).expect("Settings");
	app.press("{right}");
	let field = control(&dialog, "Start buffer, in milliseconds:", ControlType::Edit);
	assert_eq!(field_text(&field), "900");
	field
		.get_pattern::<UIValuePattern>()
		.expect("value")
		.set_value("2345")
		.expect("type");
	std::thread::sleep(Duration::from_millis(300));
	app.invoke(&control(&dialog, "OK", ControlType::Button));
	std::thread::sleep(Duration::from_millis(500));
	assert_eq!(saved(), 2345);
}

/// Settings opens on its tab strip; arrows and Ctrl+Tab change tab, from the OK button too, and
/// OK saves what every tab holds.
#[test]
#[ignore = "needs a desktop"]
fn settings_tabs_are_reached_by_keyboard_and_all_save() {
	let app = App::start("settings-tabs", Library::default());
	let dialog = (0..4)
		.find_map(|_| {
			app.press("{ctrl}t");
			app.find("Settings", Duration::from_secs(5))
		})
		.expect("Settings");
	let control = |name: &str, kind| {
		app.automation
			.create_matcher()
			.from(dialog.clone())
			.timeout(5000)
			.name(name)
			.control_type(kind)
			.find_first()
			.unwrap_or_else(|e| panic!("{name}: {e}"))
	};
	let focused = || app.automation.get_focused_element().expect("focus");
	let shows = |name: &str, kind| {
		let found = control(name, kind);
		assert!(!found.is_offscreen().unwrap(), "{name} shows");
		found
	};
	assert_eq!(focused().get_name().unwrap(), "General", "starts on the tab strip");
	// A tab strip too wide for the window grows these arrows and hides tabs behind them.
	assert!(
		app.automation
			.create_matcher()
			.from(dialog.clone())
			.timeout(500)
			.control_type(ControlType::Spinner)
			.find_first()
			.is_err(),
		"every tab fits"
	);
	shows("Announce Song Titles", ControlType::CheckBox);

	app.press("{right}");
	assert_eq!(focused().get_name().unwrap(), "Playback");
	shows("Seek Step (Seconds)", ControlType::Edit);
	app.press("{tab}");
	assert_eq!(
		focused().get_name().unwrap(),
		"FM sound between stations",
		"Tab goes into the page"
	);

	app.press("{ctrl}{tab}");
	shows("Recordings Folder (blank is the default)", ControlType::Edit);

	control("OK", ControlType::Button).set_focus().expect("focus OK");
	std::thread::sleep(Duration::from_millis(300));
	app.press("{ctrl}{tab}");
	let token = shows("AudD Token", ControlType::Edit);
	token
		.get_pattern::<UIValuePattern>()
		.expect("value")
		.set_value("from-the-last-tab")
		.expect("type");

	app.press("{ctrl}t");
	assert_eq!(
		focused().get_control_type().unwrap(),
		ControlType::TabItem,
		"Ctrl+T comes back to the tab strip"
	);
	app.invoke(&control("OK", ControlType::Button));
	std::thread::sleep(Duration::from_millis(500));
	assert_eq!(
		Settings::load(&app.data).recognition.audd_token.as_deref(),
		Some("from-the-last-tab")
	);
}

/// Shift+Up and Shift+Down move a favorite, as in Browse.
#[test]
#[ignore = "needs a desktop"]
fn favorites_shift_arrows_move_the_station() {
	let library = Library {
		favorites: vec![
			station("custom:one", "One"),
			station("custom:two", "Two"),
			station("custom:three", "Three"),
		],
		..Library::default()
	};
	let settings = Settings {
		play_on_move: false,
		..Settings::default()
	};
	let app = App::with_settings("fav-shift", library, settings);
	let order = |app: &App| -> Vec<String> {
		Library::load(&app.data)
			.favorites
			.iter()
			.map(|s| s.name.clone())
			.collect()
	};
	app.press("{ctrl}b");
	let favorites = app.find("Favorites", WAIT).expect("Favorites");
	app.select_row(&favorites, "Two");
	app.press("{shift}{up}");
	assert_eq!(order(&app), ["Two", "One", "Three"]);
	app.press("{shift}{down}");
	app.press("{shift}{down}");
	assert_eq!(order(&app), ["One", "Three", "Two"], "focus follows the row");
}

/// Space on the tree is Play/Pause; in the Filter box it is still a space.
#[test]
#[ignore = "needs a desktop"]
fn space_plays_on_the_tree_and_types_in_the_filter() {
	let mut library = Library::default();
	library.collections[0].stations = vec![station("custom:one", "One")];
	let app = App::with_settings(
		"space",
		library,
		Settings {
			main_list: Some("custom".into()),
			play_on_move: false,
			..Settings::default()
		},
	);
	let tree = app.main_tree();
	app.select_row(&tree, "One");
	assert!(!app.window.get_name().unwrap().contains("One"), "not yet playing");
	app.press("{space}");
	assert!(app.window.get_name().unwrap().contains("One"), "Space plays");
	let title = app.window.get_name().unwrap();
	let filter = app
		.automation
		.create_matcher()
		.from(app.window.clone())
		.control_type(ControlType::Edit)
		.timeout(5000)
		.find_first()
		.expect("filter box");
	filter.set_focus().expect("focus filter");
	app.press("o");
	app.press("{space}");
	app.press("e");
	let typed = filter
		.get_pattern::<UIValuePattern>()
		.expect("value")
		.get_value()
		.expect("text");
	assert_eq!(typed, "o e", "Space types in a text box");
	assert_eq!(app.window.get_name().unwrap(), title, "and leaves playback alone");
}

/// The main tree's own paste, with the station a row shows also filed earlier in
/// another folder: focus stays on the copy it moved to, and Paste lands in that
/// copy's folder instead of answering "already in" for the first one.
#[test]
#[ignore = "needs a desktop"]
fn main_window_pastes_beside_the_focused_copy() {
	let mut library = Library::default();
	let shared = station("custom:shared", "Shared");
	let (one, two) = (
		station("custom:one", "Paste one"),
		station("custom:two", "Paste two"),
	);
	let folder = |name: &str, stations| Folder {
		name: name.into(),
		stations,
		folders: Vec::new(),
	};
	library.collections[0].folders = vec![
		folder("Alpha", vec![shared.clone(), one, two]),
		folder("Beta", vec![shared]),
		folder("Gamma", Vec::new()),
	];
	let settings = Settings {
		main_list: Some("custom".into()),
		expand_dial_folders: true,
		..Settings::default()
	};
	let app = App::with_settings("paste-main", library, settings);
	let names = |saved: &Library, i: usize| -> Vec<String> {
		saved.collections[0].folders[i]
			.stations
			.iter()
			.map(|s| s.name.clone())
			.collect()
	};
	let tree = app.main_tree();
	app.select_row(&tree, "Paste one");
	app.press("{ctrl}c");
	// By keyboard, as it is used: Beta's row, then down onto its copy of Shared.
	app.select_row(&tree, "Beta 1");
	app.press("{down}");
	app.press("{ctrl}v");
	let saved = Library::load(&app.data);
	assert_eq!(names(&saved, 1), ["Shared", "Paste one"], "into Beta");
	assert_eq!(names(&saved, 0).len(), 3, "Alpha is untouched");

	// A folder row keeps focus across the paste: the next paste lands there too.
	let tree = app.main_tree();
	app.select_row(&tree, "Paste one");
	app.press("{ctrl}c");
	app.select_row(&tree, "Gamma 0");
	app.press("{ctrl}v");
	let tree = app.main_tree();
	app.select_row(&tree, "Paste two");
	app.press("{ctrl}c");
	app.select_row(&tree, "Gamma 1");
	app.press("{ctrl}v");
	let saved = Library::load(&app.data);
	assert_eq!(names(&saved, 2), ["Paste one", "Paste two"], "into Gamma");
}

/// Ctrl+C and Ctrl+X on a folder row leave the clipboard alone, whatever was last focused:
/// they answer that nothing is there, and Paste still does what the last real copy or cut asked.
#[test]
#[ignore = "needs a desktop"]
fn main_window_copy_and_cut_need_a_station_row() {
	let folder = |name: &str, stations| Folder {
		name: name.into(),
		stations,
		folders: Vec::new(),
	};
	let mut library = Library::default();
	library.collections[0].folders = vec![
		folder(
			"Alpha",
			vec![station("custom:one", "Row one"), station("custom:two", "Row two")],
		),
		folder("Beta", Vec::new()),
		folder("Gamma", Vec::new()),
	];
	let settings = Settings {
		main_list: Some("custom".into()),
		expand_dial_folders: true,
		play_on_move: false,
		..Settings::default()
	};
	let app = App::with_settings("copy-folder", library, settings);
	let names = |i: usize| -> Vec<String> {
		Library::load(&app.data).collections[0].folders[i]
			.stations
			.iter()
			.map(|s| s.name.clone())
			.collect()
	};
	let tree = app.main_tree();

	// On a station Ctrl+D acts; on a folder neither it nor Ctrl+U reaches the last station.
	let favorites = || Library::load(&app.data).favorites.len();
	app.select_row(&tree, "Row one");
	app.press("{ctrl}d");
	assert_eq!(favorites(), 1, "a station row");
	app.select_row(&tree, "Beta 0");
	app.press("{ctrl}d");
	assert!(
		app.control("No station here", ControlType::Text, Duration::from_secs(3)).is_some(),
		"it says so"
	);
	assert_eq!(favorites(), 1, "a folder row");
	app.press("{ctrl}u");
	assert!(
		app.control("Nothing to copy", ControlType::Text, Duration::from_secs(3)).is_some(),
		"no URL from a folder"
	);

	// A copy stays a copy: Ctrl+X on a folder must not turn it into a cut.
	app.select_row(&tree, "Row one");
	app.press("{ctrl}c");
	app.select_row(&tree, "Beta 0");
	app.press("{ctrl}x");
	assert!(
		app.control("Nothing to cut", ControlType::Text, Duration::from_secs(3)).is_some(),
		"it says so"
	);
	app.press("{ctrl}v");
	assert_eq!(names(1), ["Row one"], "into Beta");
	assert_eq!(names(0), ["Row one", "Row two"], "still in Alpha: it was a copy");

	// A cut stays a cut: Ctrl+C on a folder must not turn it into a copy.
	let tree = app.main_tree();
	app.select_row(&tree, "Row two");
	app.press("{ctrl}x");
	app.select_row(&tree, "Gamma 0");
	app.press("{ctrl}c");
	assert!(
		app.control("Nothing to copy", ControlType::Text, Duration::from_secs(3)).is_some(),
		"it says so"
	);
	app.press("{ctrl}v");
	assert_eq!(names(2), ["Row two"], "into Gamma");
	assert_eq!(names(0), ["Row one"], "gone from Alpha: it was a cut");
}

/// Answers Yes in the confirmation with this title.
fn answer_yes(app: &App, title: &str) {
	let dialog = app.find(title, WAIT).unwrap_or_else(|| panic!("{title} asks"));
	let yes = app
		.automation
		.create_matcher()
		.from(dialog)
		.timeout(5000)
		.name("Yes")
		.control_type(ControlType::Button)
		.find_first()
		.expect("Yes");
	app.invoke(&yes);
	std::thread::sleep(Duration::from_millis(500));
}

fn collection_names(app: &App) -> Vec<String> {
	Library::load(&app.data)
		.collections
		.iter()
		.map(|c| c.name.clone())
		.collect()
}

/// Shift+Up and Shift+Down on a collection's own row in Browse, an open one keeping
/// its rows, and the Delete key on it, which answered "only your own stations".
#[test]
#[ignore = "needs a desktop"]
fn browse_moves_and_deletes_collections() {
	let mut library = Library::default();
	library.create_collection("Alpha");
	let beta = library.create_collection("Beta");
	library.create_collection("Gamma");
	library.collection_mut(&beta).unwrap().stations = vec![station("custom:in", "In Beta")];
	let settings = Settings {
		play_on_move: false,
		..Settings::default()
	};
	let app = App::with_settings("browse-collections", library, settings);
	app.press("{ctrl}g");
	let browse = app.find("Browse", WAIT).expect("Browse");
	for name in ["Collections", "Beta 1"] {
		TreeItemControl::try_from(app.row(&browse, name))
			.unwrap()
			.expand()
			.unwrap();
	}
	app.select_row(&browse, "Beta 1");
	app.press("{shift}{up}");
	assert_eq!(
		collection_names(&app),
		["Custom Stations", "Beta", "Alpha", "Gamma"]
	);
	app.assert_selected(&browse, "Beta 1");
	app.row(&browse, "In Beta");
	app.press("{shift}{up}");
	assert_eq!(
		collection_names(&app),
		["Custom Stations", "Beta", "Alpha", "Gamma"],
		"never above Custom Stations"
	);
	app.press("{shift}{down}");
	assert_eq!(
		collection_names(&app),
		["Custom Stations", "Alpha", "Beta", "Gamma"],
		"focus goes with it"
	);
	app.select_row(&browse, "Gamma 0");
	app.press("{delete}");
	answer_yes(&app, "Delete");
	assert_eq!(
		collection_names(&app),
		["Custom Stations", "Alpha", "Beta"],
		"Delete takes the collection"
	);
}

/// The main tree's All Playlists view: Shift+Up and Shift+Down move a collection, a
/// folder and a station; a folder renames from its menu and goes with Delete.
#[test]
#[ignore = "needs a desktop"]
fn main_window_moves_renames_and_deletes_rows() {
	let mut library = Library::default();
	let alpha = library.create_collection("Alpha");
	library.create_collection("Beta");
	let folder = |name: &str, stations| Folder {
		name: name.into(),
		stations,
		folders: Vec::new(),
	};
	library.collection_mut(&alpha).unwrap().folders = vec![
		folder(
			"One",
			vec![station("custom:x", "Ex"), station("custom:y", "Why")],
		),
		folder("Two", Vec::new()),
		folder("Three", Vec::new()),
	];
	let settings = Settings {
		main_list: Some("all-playlists".into()),
		expand_dial_folders: true,
		play_on_move: false,
		..Settings::default()
	};
	let app = App::with_settings("main-moves", library, settings);
	let folders = |app: &App| -> Vec<String> {
		Library::load(&app.data).collections[1]
			.folders
			.iter()
			.map(|f| f.name.clone())
			.collect()
	};

	// A collection.
	app.select_row(&app.main_tree(), "Beta 0");
	app.press("{shift}{up}");
	assert_eq!(collection_names(&app), ["Custom Stations", "Beta", "Alpha"]);
	app.assert_selected(&app.main_tree(), "Beta 0");
	app.press("{shift}{down}");
	assert_eq!(collection_names(&app), ["Custom Stations", "Alpha", "Beta"]);

	// A station.
	app.select_row(&app.main_tree(), "Why");
	app.press("{shift}{up}");
	let saved = Library::load(&app.data);
	assert_eq!(saved.collections[1].folders[0].stations[0].name, "Why");
	app.assert_selected(&app.main_tree(), "Why");

	// A folder, twice: focus goes with it.
	app.select_row(&app.main_tree(), "Three 0");
	app.press("{shift}{up}");
	app.press("{shift}{up}");
	assert_eq!(folders(&app), ["Three", "One", "Two"]);
	app.assert_selected(&app.main_tree(), "Three 0");

	// Rename from the folder's menu.
	app.press("{shift}{f10}");
	app.invoke(&app.menu_item("Rename"));
	let prompt = app.find("Rename", WAIT).expect("Rename asks");
	prompt.send_keys("Tres{enter}", 60).expect("name");
	std::thread::sleep(Duration::from_millis(500));
	assert_eq!(folders(&app), ["Tres", "One", "Two"]);
	app.assert_selected(&app.main_tree(), "Tres 0");

	// Delete: the next folder takes its place.
	app.press("{delete}");
	answer_yes(&app, "Delete");
	assert_eq!(folders(&app), ["One", "Two"]);
	app.assert_selected(&app.main_tree(), "One 2");
	// Keys still reach the tree after the confirmation closes.
	app.press("{shift}{down}");
	assert_eq!(folders(&app), ["Two", "One"]);
}

/// Two folders with one name: Delete takes the row focus is on, not the first by name.
/// The same station filed in both: its Delete takes the copy focus is on. A station at
/// a single list's own level moves too.
#[test]
#[ignore = "needs a desktop"]
fn main_window_delete_takes_the_focused_row() {
	let mut library = Library::default();
	let shared = station("custom:shared", "Shared");
	let extra = station("custom:extra", "Extra");
	let folder = |stations| Folder {
		name: "Same".into(),
		stations,
		folders: Vec::new(),
	};
	library.collections[0].stations =
		vec![station("custom:one", "One"), station("custom:two", "Two")];
	library.collections[0].folders = vec![
		folder(vec![shared.clone()]),
		folder(vec![shared, extra]),
	];
	let settings = Settings {
		main_list: Some("custom".into()),
		expand_dial_folders: true,
		play_on_move: false,
		..Settings::default()
	};
	let app = App::with_settings("main-delete", library, settings);
	let custom = |app: &App| Library::load(&app.data).collections[0].clone();

	app.select_row(&app.main_tree(), "One");
	app.press("{shift}{down}");
	assert_eq!(custom(&app).stations[0].name, "Two");
	app.assert_selected(&app.main_tree(), "One");

	// Down from the second Same's row onto its copy of Shared.
	app.select_row(&app.main_tree(), "Same 2");
	app.press("{down}");
	app.press("{delete}");
	let saved = custom(&app);
	assert_eq!(saved.folders[0].stations.len(), 1, "the first copy stays");
	assert_eq!(saved.folders[1].stations[0].name, "Extra");

	// Up from Extra onto the second Same's own row.
	app.select_row(&app.main_tree(), "Extra");
	app.press("{left}");
	app.press("{delete}");
	answer_yes(&app, "Delete");
	let saved = custom(&app);
	assert_eq!(saved.folders.len(), 1, "one row goes");
	assert_eq!(saved.folders[0].stations[0].name, "Shared", "the focused one");
}

/// Ctrl+I's layout: the playlist is a folder inside Custom Stations, a copy of
/// the station also sits at the Custom root, and Play on Move is on. Pasting
/// onto the playlist row must not answer "already in Custom Stations".
#[test]
#[ignore = "needs a desktop"]
fn browse_pastes_into_an_imported_playlist_folder() {
	let mut library = Library::default();
	let copy = station("custom:copy", "Copy me");
	let cut = station("custom:cut", "Cut me");
	library.collections[0].stations = vec![copy.clone(), cut.clone()];
	library.collections[0].folders.push(Folder {
		name: "English".into(),
		stations: Vec::new(),
		folders: vec![Folder {
			name: "Misc".into(),
			stations: vec![copy, cut],
			folders: Vec::new(),
		}],
	});
	let app = App::start("paste-import", library);
	app.press("{ctrl}g");
	let browse = app.find("Browse", WAIT).expect("Browse");
	let expand = |row: &UIElement| {
		TreeItemControl::try_from(row.clone())
			.unwrap()
			.expand()
			.unwrap();
	};
	expand(&app.row(&browse, "Custom Stations 4"));
	expand(&app.row(&browse, "English 2"));
	expand(&app.row(&browse, "Misc 2"));
	for (name, chord) in [("Copy me", "{ctrl}c"), ("Cut me", "{ctrl}x")] {
		let misc = app.row(&browse, "Misc 2");
		expand(&misc);
		app.select_row(&misc, name);
		app.press(chord);
		// By keyboard, as it is used: up to Misc, close it, up to English.
		app.press("{left}");
		app.press("{left}");
		app.press("{left}");
		app.press("{ctrl}v");
		std::thread::sleep(Duration::from_millis(500));
		let saved = Library::load(&app.data);
		let folder = &saved.collections[0].folders[0];
		assert!(
			folder.stations.iter().any(|s| s.name == name),
			"{name} reaches the playlist folder"
		);
		assert!(
			saved.collections[0].stations.iter().any(|s| s.name == name),
			"{name}: the Custom root copy stays"
		);
	}
	let saved = Library::load(&app.data);
	assert_eq!(
		saved.collections[0].folders[0].folders[0].stations.len(),
		1,
		"the cut left Misc, the copy did not"
	);
}

#[test]
#[ignore = "needs a desktop"]
fn deleting_in_browse_plays_the_replacement() {
	let mut library = Library::default();
	library.collections[0].stations =
		vec![station("custom:one", "One"), station("custom:two", "Two")];
	let app = App::start("delete-browse", library);
	app.press("{ctrl}g");
	let browse = app.find("Browse", WAIT).expect("Browse");
	let custom = app.row(&browse, "Custom Stations 2");
	TreeItemControl::try_from(custom.clone())
		.unwrap()
		.expand()
		.unwrap();
	app.select_row(&custom, "One");
	assert!(app.window.get_name().unwrap().contains("One"));
	app.press("{delete}");
	assert!(
		app.window.get_name().unwrap().contains("Two"),
		"the station that took its place plays"
	);
}

#[test]
#[ignore = "needs a desktop"]
fn deleting_the_last_main_row_plays_the_one_before() {
	let mut library = Library::default();
	library.collections[0].stations =
		vec![station("custom:one", "One"), station("custom:two", "Two")];
	let app = App::with_settings(
		"delete-last",
		library,
		Settings {
			main_list: Some("custom".into()),
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
	app.select_row(&tree, "Two");
	assert!(app.window.get_name().unwrap().contains("Two"));
	app.press("{delete}");
	app.assert_selected(&tree, "One");
	assert!(
		app.window.get_name().unwrap().contains("One"),
		"the row before plays"
	);
}

#[test]
#[ignore = "needs a desktop"]
fn deleting_from_the_main_context_menu_plays_the_replacement() {
	let mut library = Library::default();
	library.collections[0].stations =
		vec![station("custom:one", "One"), station("custom:two", "Two")];
	let app = App::with_settings(
		"delete-menu",
		library,
		Settings {
			main_list: Some("custom".into()),
			..Settings::default()
		},
	);
	let tree = app.main_tree();
	app.select_row(&tree, "One");
	app.press("{shift}{f10}");
	app.invoke(&app.menu_item("Delete Custom Station"));
	std::thread::sleep(Duration::from_millis(600));
	app.assert_selected(&tree, "Two");
	assert!(
		app.window.get_name().unwrap().contains("Two"),
		"the station that took its place plays"
	);
	assert!(!Library::load(&app.data).collections[0].contains_id("custom:one"));
}

#[test]
#[ignore = "needs a desktop"]
fn edit_station_shows_and_clears_the_info() {
	let mut library = Library::default();
	let mut wnyc = station("custom:wnyc", "WNYC Operavore");
	wnyc.info = Some("Operavore, The United States Of America, AAC+ 47k".into());
	library.collections[0].stations = vec![wnyc];
	let app = App::with_settings(
		"edit-info",
		library,
		Settings {
			main_list: Some("custom".into()),
			play_on_move: false,
			..Settings::default()
		},
	);
	let tree = app.main_tree();
	let long = "WNYC Operavore - Operavore, The United States Of America, AAC+ 47k";
	app.select_row(&tree, long);
	app.press("{shift}{f10}");
	app.invoke(&app.menu_item("Edit Station…"));
	let dialog = app.find("Edit Station", WAIT).expect("Edit Station");
	let info = app
		.automation
		.create_matcher()
		.from(dialog.clone())
		.name("Info")
		.control_type(ControlType::Edit)
		.timeout(5000)
		.find_first()
		.expect("Info field");
	let value = info.get_pattern::<UIValuePattern>().expect("editable");
	assert_eq!(
		value.get_value().unwrap(),
		"Operavore, The United States Of America, AAC+ 47k"
	);
	value.set_value("").unwrap();
	let ok = app
		.automation
		.create_matcher()
		.from(dialog)
		.name("OK")
		.control_type(ControlType::Button)
		.timeout(5000)
		.find_first()
		.expect("OK");
	app.invoke(&ok);
	std::thread::sleep(Duration::from_millis(600));
	let saved = Library::load(&app.data);
	assert_eq!(saved.collections[0].stations[0].name, "WNYC Operavore");
	assert_eq!(saved.collections[0].stations[0].info, None);
	app.assert_selected(&tree, "WNYC Operavore");
}

/// Ctrl+V used to ignore the focused row here and land in the main list's root.
#[test]
#[ignore = "needs a desktop"]
fn collections_paste_follows_the_focused_row_and_refreshes() {
	let mut library = Library::default();
	let copy = station("custom:copy", "Copy me");
	library.collections[0].stations = vec![copy.clone()];
	library.collections[0].folders.push(Folder {
		name: "English".into(),
		stations: Vec::new(),
		folders: vec![Folder {
			name: "Misc".into(),
			stations: vec![copy],
			folders: Vec::new(),
		}],
	});
	let app = App::with_settings(
		"paste-collections",
		library,
		Settings {
			main_list: Some("custom".into()),
			..Settings::default()
		},
	);
	app.press("{ctrl}l");
	let window = app.find("Collections", WAIT).expect("Collections");
	let expand = |row: &UIElement| {
		TreeItemControl::try_from(row.clone())
			.unwrap()
			.expand()
			.unwrap();
	};
	expand(&app.row(&window, "Custom Stations"));
	let english = app.row(&window, "English");
	expand(&english);
	let misc = app.row(&english, "Misc");
	expand(&misc);
	app.select_row(&misc, "Copy me");
	app.press("{ctrl}c");
	app.select_row(&window, "English");
	app.press("{ctrl}v");
	std::thread::sleep(Duration::from_millis(500));
	let saved = Library::load(&app.data);
	let folder = &saved.collections[0].folders[0];
	assert_eq!(folder.stations.len(), 1, "it reaches the focused folder");
	assert_eq!(folder.folders[0].stations.len(), 1, "a copy leaves Misc alone");
	assert_eq!(saved.collections[0].stations.len(), 1, "and the root");
	let english = app.row(&window, "English");
	expand(&english);
	let misc = app.row(&english, "Misc");
	assert!(
		app.automation
			.create_tree_walker()
			.unwrap()
			.get_first_child(&english)
			.is_ok_and(|first| first.get_name().unwrap() == "Copy me"),
		"the window shows it at once, ahead of {}",
		misc.get_name().unwrap()
	);
}

#[test]
#[ignore = "needs a desktop"]
fn favorites_paste_follows_the_focused_row_and_refreshes() {
	let library = Library {
		fav_folders: vec![
			Folder {
				name: "First".into(),
				stations: vec![station("custom:one", "One")],
				folders: Vec::new(),
			},
			Folder {
				name: "Second".into(),
				stations: Vec::new(),
				folders: Vec::new(),
			},
		],
		..Library::default()
	};
	let app = App::start("paste-favorites", library);
	app.press("{ctrl}b");
	let window = app.find("Favorites", WAIT).expect("Favorites");
	let first = app.row(&window, "First");
	TreeItemControl::try_from(first.clone())
		.unwrap()
		.expand()
		.unwrap();
	app.select_row(&first, "One");
	app.press("{ctrl}x");
	app.select_row(&window, "Second");
	app.press("{ctrl}v");
	std::thread::sleep(Duration::from_millis(500));
	let saved = Library::load(&app.data);
	assert!(saved.fav_folders[0].stations.is_empty(), "the cut left First");
	assert_eq!(saved.fav_folders[1].stations.len(), 1, "it reached Second");
	let second = app.row(&window, "Second");
	TreeItemControl::try_from(second.clone())
		.unwrap()
		.expand()
		.unwrap();
	app.row(&second, "One");
}

/// Each arrow used to rebuild the hidden main tree, however long it was.
#[test]
#[ignore = "needs a desktop"]
fn arrows_in_a_modal_ignore_the_size_of_the_main_tree() {
	// The favorites take turns between two long lists, the costliest reveal:
	// each one would switch the main list and rebuild the whole tree.
	let mut library = Library::default();
	let other = library.create_collection("Other");
	let rows = |prefix: &str| -> Vec<Station> {
		(0..6000)
			.map(|i| station(&format!("{prefix}:{i}"), &format!("Row {i}")))
			.collect()
	};
	library.collections[0].stations = rows("custom");
	library.collection_mut(&other).unwrap().stations = rows("other");
	library.favorites = (0..6)
		.map(|i| station(&format!("url:{i}"), &format!("Loose {i}")))
		.collect();
	for (i, favorite) in library.favorites.clone().into_iter().enumerate() {
		let list = if i % 2 == 0 { "custom" } else { other.as_str() };
		library.collection_mut(list).unwrap().stations.push(favorite);
	}
	let app = App::with_settings(
		"modal-speed",
		library,
		Settings {
			main_list: Some("custom".into()),
			..Settings::default()
		},
	);
	app.press("{ctrl}b");
	app.find("Favorites", WAIT).expect("Favorites");
	std::thread::sleep(Duration::from_millis(500));
	let began = std::time::Instant::now();
	for _ in 0..5 {
		app.tap_down();
	}
	while !app.window.get_name().unwrap().contains("Loose 5") {
		assert!(began.elapsed() < WAIT, "the last favorite never played");
		std::thread::sleep(Duration::from_millis(10));
	}
	let took = began.elapsed();
	println!("five arrows over 6000 rows: {took:?}");
	assert!(took < Duration::from_millis(1500), "took {took:?}");
}

/// The report itself, against the live directories: the first result plays
/// and keeps focus, and Down moves on from it.
#[test]
#[ignore = "needs a desktop and the network"]
fn search_keeps_focus_when_its_first_result_plays() {
	let mut library = Library::default();
	library.collections[0].stations = (0..6000)
		.map(|i| station(&format!("custom:{i}"), &format!("Row {i}")))
		.collect();
	let app = App::with_settings(
		"search-focus",
		library,
		Settings {
			main_list: Some("custom".into()),
			..Settings::default()
		},
	);
	app.press("{ctrl}f");
	let search = app.find("Search", WAIT).expect("Search");
	app.press("WNYC{enter}");
	let first = app
		.automation
		.create_matcher()
		.from(search.clone())
		.control_type(ControlType::ListItem)
		.timeout(WAIT.as_millis() as u64)
		.find_first()
		.expect("results");
	std::thread::sleep(Duration::from_millis(1000));
	assert!(first.has_keyboard_focus().unwrap(), "the first result has focus");
	let began = std::time::Instant::now();
	let title = app.window.get_name().unwrap();
	app.tap_down();
	while app.window.get_name().unwrap() == title {
		assert!(began.elapsed() < WAIT, "Down never moved on");
		std::thread::sleep(Duration::from_millis(10));
	}
	println!("Down in Search: {:?}", began.elapsed());
	// The title follows 300 ms after the keys rest (App's LINE_REST), then a 50 ms poll.
	assert!(began.elapsed() < Duration::from_millis(650));
	assert!(!first.has_keyboard_focus().unwrap(), "focus went with the arrow");
}

/// An arrow in a modal tunes at once; the hidden tree follows when it closes.
#[test]
#[ignore = "needs a desktop"]
fn modal_navigation_leaves_the_main_tree_until_it_closes() {
	let mut library = Library::default();
	let (one, two) = (station("custom:one", "One"), station("custom:two", "Two"));
	library.collections[0].stations =
		vec![station("custom:zero", "Zero"), one.clone(), two.clone()];
	library.favorites = vec![one, two];
	let app = App::with_settings(
		"modal-reveal",
		library,
		Settings {
			main_list: Some("custom".into()),
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
	// Opening lands on the first favorite and the tree shows it, before the dialog is up.
	app.press("{ctrl}b");
	let favorites = app.find("Favorites", WAIT).expect("Favorites");
	app.press("{down}");
	assert!(app.window.get_name().unwrap().contains("Two"));
	app.assert_selected(&tree, "One");
	assert!(
		app.row(&favorites, "Two").has_keyboard_focus().unwrap(),
		"the dialog keeps focus"
	);
	app.press("{esc}");
	std::thread::sleep(Duration::from_millis(400));
	app.assert_selected(&tree, "Two");
}

/// Nothing holds the station: it plays, but it gets no row and the focused row stays.
#[test]
#[ignore = "needs a desktop"]
fn a_station_no_list_holds_gets_no_row() {
	let mut library = Library::default();
	library.collections[0].stations =
		vec![station("custom:one", "One"), station("custom:two", "Two")];
	library.favorites = vec![station("url:loose", "Loose")];
	let app = App::with_settings(
		"no-row",
		library,
		Settings {
			main_list: Some("custom".into()),
			play_on_move: false,
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
	app.select_row(&tree, "Two");
	app.press("{alt}1");
	assert!(app.window.get_name().unwrap().contains("Loose"), "it plays");
	assert!(
		app.automation
			.create_matcher()
			.from(tree.clone())
			.name("Loose")
			.control_type(ControlType::TreeItem)
			.timeout(800)
			.find_first()
			.is_err(),
		"no temporary row"
	);
	app.assert_selected(&tree, "Two");
	// Until the user moves, it is the station commands act on, not the row.
	app.press("{ctrl}d");
	let saved = Library::load(&app.data);
	assert!(!saved.is_favorite("url:loose"), "Ctrl+D took the playing one");
	assert!(!saved.is_favorite("custom:two"), "the row was left alone");
}

/// Edit Station over Browse: closing it must leave Browse's modal state whole.
#[test]
#[ignore = "needs a desktop"]
fn a_dialog_over_browse_leaves_the_main_tree_waiting() {
	let mut library = Library::default();
	library.collections[0].stations =
		vec![station("custom:one", "One"), station("custom:two", "Two")];
	let app = App::with_settings(
		"nested-dialog",
		library,
		Settings {
			main_list: Some("custom".into()),
			..Settings::default()
		},
	);
	let tree = app.main_tree();
	app.select_row(&tree, "One");
	app.press("{ctrl}g");
	let browse = app.find("Browse", WAIT).expect("Browse");
	let custom = app.row(&browse, "Custom Stations 2");
	TreeItemControl::try_from(custom.clone())
		.unwrap()
		.expand()
		.unwrap();
	app.select_row(&custom, "One");
	app.press("{shift}{f10}");
	app.invoke(&app.menu_item("Edit Station…"));
	app.find("Edit Station", WAIT).expect("Edit Station");
	app.press("{esc}");
	std::thread::sleep(Duration::from_millis(500));
	app.select_row(&custom, "Two");
	assert!(app.window.get_name().unwrap().contains("Two"));
	app.assert_selected(&tree, "One");
	app.press("{esc}");
	std::thread::sleep(Duration::from_millis(500));
	app.assert_selected(&tree, "Two");
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
