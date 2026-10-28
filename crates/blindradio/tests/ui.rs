//! UI automation over the real window: needs a desktop, so `#[ignore]` and one at a time.
//! Run: `cargo test -p blindradio --test ui -- --ignored --test-threads=1`
#![cfg(windows)]

use std::path::PathBuf;
use std::process::{Child, Command};
use std::time::Duration;

use blindradio_core::library::Library;
use blindradio_core::station::{Source, Station};
use uiautomation::controls::WindowControl;
use uiautomation::types::ControlType;
use uiautomation::{UIAutomation, UIElement};

const WAIT: Duration = Duration::from_secs(20);

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
		let data =
			std::env::temp_dir().join(format!("blindradio-ui-{}-{name}", std::process::id()));
		let _ = std::fs::remove_dir_all(&data);
		std::fs::create_dir_all(&data).expect("data folder");
		library.save(&data).expect("library");
		let child = Command::new(env!("CARGO_BIN_EXE_blindradio"))
			.env("BLINDRADIO_DATA", &data)
			.spawn()
			.expect("the app starts");
		let automation = UIAutomation::new().expect("UIAutomation");
		let window = automation
			.create_matcher()
			.from(automation.get_root_element().expect("root"))
			.timeout(WAIT.as_millis() as u64)
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
		self.window.send_keys(keys, 60).expect("keys");
		std::thread::sleep(Duration::from_millis(400));
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
	app.window.send_keys("{esc}", 60).expect("Escape");
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
		app.find("One", Duration::from_secs(5)).is_some(),
		"first plays"
	);
	app.window.send_keys("{down}", 60).expect("Down");
	assert!(
		app.find("Two", Duration::from_secs(5)).is_some(),
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
	app.window.send_keys("{ctrl}b", 60).expect("Ctrl+B");
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
