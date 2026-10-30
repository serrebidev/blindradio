//! Owns every core object and executes `Action`s. Lives in `Rc<RefCell<App>>` on the UI thread.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use blindradio_core::history::SessionHistory;
use blindradio_core::library::{Collection, Folder, Home, Library};
use blindradio_core::output::{DeviceEvent, MixerCmd, Output, output_devices};
use blindradio_core::settings::{EqSettings, Settings};
use blindradio_core::sleep::SleepTimer;
use blindradio_core::sources::{Endpoints, Filters, Sources};
use blindradio_core::station::Station;
use blindradio_core::tuner::{LiveFactory, Sink, Tuner, TunerEvent};
use blindradio_core::{Engine, import, store};
use crossbeam_channel::Receiver;
use ship_shape::ui::CheckTrigger;
use wx_utils::set_menu_item_label;
use wxdragon::prelude::*;

use crate::action::Action;
use crate::bridge::{Bridge, WindowToken};
use crate::clipboard;
use crate::dial_tree::DialNode;
use crate::dialogs::browse;
use crate::hotkeys::{self, HotkeyAction, Hotkeys};
use crate::main_window::{MainWindow, escape};
use crate::menus::{self, MenuIds};
use crate::speech::{self, Kind, Speech};
use crate::strings as s;
use crate::update;

const VOLUME_STEP: i32 = 5;
const DEVICE_POLL: Duration = Duration::from_secs(2);
/// The daily check waits until the window and the first station are settled.
const UPDATE_DELAY: Duration = Duration::from_secs(10);

/// The tuner sends mixer commands through it while the app switches devices on it.
pub struct SharedOutput(Rc<RefCell<Output>>);

impl Sink for SharedOutput {
	fn send(&self, cmd: MixerCmd) {
		self.0.borrow().send(cmd);
	}
}

/// Where a seek lands, in seconds behind live: the pre-seek distance
/// moved by the seek's seconds (forward seeks carry a positive delta),
/// clamped to the live edge and to the buffer's depth.
pub fn behind_after(behind: f32, delta_seconds: i32, depth: f32) -> f32 {
	(behind - delta_seconds as f32).clamp(0.0, depth.max(0.0))
}

/// The buffer's depth in seconds: live edge minus its oldest frame.
pub fn buffer_depth(buffer: &blindradio_core::timeshift::ShiftBuffer) -> f32 {
	let rate = buffer.rate();
	if rate == 0 {
		return 0.0;
	}
	buffer.live_edge().saturating_sub(buffer.start()) as f32 / rate as f32
}

/// `3:20` under an hour, `1:02:33` over it.
pub fn fmt_dur(seconds: f32) -> String {
	let total = seconds.max(0.0) as u64;
	if total >= 3600 {
		format!("{}:{:02}:{:02}", total / 3600, total % 3600 / 60, total % 60)
	} else {
		format!("{}:{:02}", total / 60, total % 60)
	}
}

#[derive(Debug, PartialEq, Eq)]
pub enum PlayPause<T> {
	Stop,
	Play(T),
	Nothing,
}

/// Playing: stop. Stopped: the station stopped last, else the selected one.
pub fn play_pause<T>(playing: bool, remembered: Option<T>, selected: Option<T>) -> PlayPause<T> {
	if playing {
		return PlayPause::Stop;
	}
	match remembered.or(selected) {
		Some(st) => PlayPause::Play(st),
		None => PlayPause::Nothing,
	}
}

/// A station landed on in a list: Play/Pause after a stop plays it, not last session's station.
fn landed<T: Clone>(selected: &mut Option<T>, remembered: &mut Option<T>, station: &T) {
	*selected = Some(station.clone());
	*remembered = Some(station.clone());
}

/// The main window list: the collection last played from, else SerrebiRadio.
/// `main_list` value meaning every collection merged: the dial shows all
/// imported playlists at once. Never a real collection id (those are
/// `col:` + a uuid), so it cannot collide.
pub const ALL_LISTS: &str = "all-playlists";

pub fn dial_collection<'a>(lib: &'a Library, main_list: Option<&str>) -> Option<&'a Collection> {
	if main_list == Some(ALL_LISTS) {
		// The merged view is no single collection (deleting from it, say,
		// would be ambiguous), so single-collection work sees nothing.
		return None;
	}
	main_list
		.and_then(|id| lib.collections.iter().find(|c| c.id == id))
		.or_else(|| {
			lib.collections
				.iter()
				.find(|c| c.name.eq_ignore_ascii_case(import::STARTER_COLLECTION))
		})
}

/// The stations the main window lists: one collection, or every collection
/// merged in name order under [`ALL_LISTS`].
pub fn dial_stations(lib: &Library, main_list: Option<&str>) -> Vec<Station> {
	if main_list == Some(ALL_LISTS) {
		let mut cols: Vec<&Collection> = lib.collections.iter().collect();
		cols.sort_by_key(|c| c.name.to_lowercase());
		return cols.iter().flat_map(|c| c.all_stations()).collect();
	}
	dial_collection(lib, main_list)
		.map(|c| c.all_stations())
		.unwrap_or_default()
}

/// The main window's tree: the dial stations (in exactly
/// [`dial_stations`] order) plus the folder structure the flat list
/// dropped. A non-empty `filter` flattens the tree to matching stations.
pub fn dial_tree(
	lib: &Library,
	main_list: Option<&str>,
	filter: &str,
) -> (Vec<Station>, Vec<DialNode>) {
	let stations = dial_stations(lib, main_list);
	if !filter.is_empty() {
		let q = filter.to_lowercase();
		let nodes = stations
			.iter()
			.enumerate()
			.filter(|(_, s)| s.name.to_lowercase().contains(&q))
			.map(|(i, _)| DialNode::Station(i))
			.collect();
		return (stations, nodes);
	}
	fn folder_node(folder: &Folder, next: &mut usize) -> DialNode {
		let mut kids: Vec<DialNode> = folder
			.stations
			.iter()
			.map(|_| {
				let i = *next;
				*next += 1;
				DialNode::Station(i)
			})
			.collect();
		for f in &folder.folders {
			kids.push(folder_node(f, next));
		}
		DialNode::Folder(folder.name.clone(), kids)
	}
	fn collection_nodes(c: &Collection, next: &mut usize) -> Vec<DialNode> {
		let mut nodes: Vec<DialNode> = c
			.stations
			.iter()
			.map(|_| {
				let i = *next;
				*next += 1;
				DialNode::Station(i)
			})
			.collect();
		for f in &c.folders {
			nodes.push(folder_node(f, next));
		}
		nodes
	}
	let mut next = 0;
	let nodes = if main_list == Some(ALL_LISTS) {
		let mut cols: Vec<&Collection> = lib.collections.iter().collect();
		cols.sort_by_key(|c| c.name.to_lowercase());
		cols.iter()
			.map(|c| DialNode::Folder(c.name.clone(), collection_nodes(c, &mut next)))
			.collect()
	} else {
		dial_collection(lib, main_list)
			.map(|c| collection_nodes(c, &mut next))
			.unwrap_or_default()
	};
	debug_assert_eq!(next, stations.len());
	(stations, nodes)
}

/// `BLINDRADIO_DATA` (tests) overrides the data folder.
pub fn data_dir() -> PathBuf {
	match std::env::var_os("BLINDRADIO_DATA") {
		Some(dir) => {
			let dir = PathBuf::from(dir);
			let _ = std::fs::create_dir_all(&dir);
			dir
		}
		None => store::data_dir(),
	}
}

/// A data folder that cannot be written: the app runs, and nothing is kept.
pub fn writable(dir: &std::path::Path) -> bool {
	let probe = dir.join(".probe");
	let wrote = std::fs::write(&probe, b"").is_ok();
	let _ = std::fs::remove_file(&probe);
	wrote
}

/// `Settings not saved`, said once and only when the folder cannot be written.
pub fn readonly_message(readonly: bool, said: bool) -> Option<&'static str> {
	(readonly && !said).then_some(s::NOT_SAVED)
}

/// A station on the clipboard, and where a cut station comes from
/// (a collection id, or Favorites) so Paste can take it out of there.
#[derive(Clone)]
pub struct Clip {
	pub station: Station,
	pub cut: bool,
	pub from_collection: Option<String>,
	pub from_favorites: bool,
}

/// Where Paste puts the clipboard station.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PasteDest {
	Collection(String, Vec<usize>),
	Favorites(Vec<usize>),
}

pub struct App {
	pub data: PathBuf,
	pub settings: Settings,
	pub library: Library,
	pub sources: Arc<Sources>,
	/// Search's filter lists, fetched once per run.
	pub filters: Option<Filters>,
	/// Browse's place and loaded levels, for the session.
	pub browse: browse::Memory,
	/// A modal window is open.
	pub dialog_open: bool,
	pub bridge: Bridge<App>,
	pub tuner: Tuner<LiveFactory, SharedOutput>,
	output: Rc<RefCell<Output>>,
	pub sleep: SleepTimer,
	pub history: SessionHistory,
	/// The station focused in the open list; station lists set it and clear it when they close.
	pub selected: Option<Station>,
	/// The station clipboard: Copy/Cut hold a station here until Paste.
	pub station_clip: Option<Clip>,
	/// What Play/Pause plays after a stop.
	remembered: Option<Station>,
	/// Stations played this session, oldest first, for Previous/Next
	/// Station (Alt+Left / Alt+Right).
	played: Vec<Station>,
	/// Index into `played` of the entry playing now.
	played_at: Option<usize>,
	/// The station tree's filter, typed in the filter box; empty shows all.
	dial_filter: String,
	/// The last tune was dial tuning (a focus move), not an explicit play.
	dial: bool,
	pub win: MainWindow,
	pub menus: MenuIds,
	/// Spoken messages; the message line shows them either way.
	pub speech: Speech,
	/// The chords that work from anywhere, and the ones another program holds.
	pub hotkeys: Hotkeys,
	/// The song playing, for Announce Now Playing.
	song: Option<String>,
	/// When the playing station started sounding; drives the status line.
	playing_since: Option<Instant>,
	/// The status line without its time suffixes.
	status_base: RefCell<String>,
	/// The status line as last written to the bar, suffixes and all; the
	/// Announce Now Playing chord speaks it verbatim.
	status_line: RefCell<String>,
	last_status_tick: Instant,
	/// The tray icon, once installed.
	pub tray: Option<crate::tray::Tray>,
	/// The single-instance claim; a later copy raises this window through it.
	pub instance: Option<crate::instance::Guard>,
	/// The data folder cannot be written: everything stays in memory.
	readonly: bool,
	readonly_said: bool,
	rendered: Receiver<(u64, Instant)>,
	events: Receiver<TunerEvent>,
	last_device_poll: Instant,
	/// When the daily update check is due this run.
	update_due: Option<Instant>,
	/// Scheduled recordings already started: (entry id, occurrence).
	sched_fired: std::collections::HashSet<(String, u64)>,
	/// The scheduled recording on air now: (entry id, occurrence).
	sched_active: Option<(String, u64)>,
	last_sched_poll: Instant,
	/// Taken and stopped on shutdown, while the frame it ticks on still exists.
	pub timer: Option<Timer<Frame>>,
	// Dropped last: the tuner's streams run on its runtime.
	engine: Engine,
}

impl App {
	pub fn new(win: MainWindow) -> blindradio_core::Result<App> {
		let data = data_dir();
		let settings = Settings::load(&data);
		let library = Library::load(&data);
		let output = Output::start(settings.audio_device.as_deref())?;
		output.send(MixerCmd::Volume(settings.volume));
		output.send(MixerCmd::Mute(settings.muted));
		output.send(MixerCmd::Eq(settings.eq.clone()));
		let engine = Engine::new(&data.join(store::RESOLVE_FILE), output.sample_rate)?;
		let rendered = output.rendered().clone();
		let output = Rc::new(RefCell::new(output));
		let (tx, events) = crossbeam_channel::unbounded();
		let menus = menus::build(&win.frame, &settings, &output_devices());
		let speech = Speech::new(&win.frame);
		let hotkeys = hotkeys::register(&settings);
		let readonly = !writable(&data);
		let timer = Timer::new(&win.frame);
		Ok(App {
			sources: Arc::new(Sources::new(
				engine.ctx.client.clone(),
				Endpoints::default(),
			)),
			filters: None,
			browse: browse::Memory::default(),
			dialog_open: false,
			bridge: Bridge::new(engine.ctx.rt.clone()),
			tuner: Tuner::new(
				LiveFactory(engine.ctx.clone()),
				SharedOutput(output.clone()),
				tx,
			),
			output,
			data,
			settings,
			library,
			sleep: SleepTimer::default(),
			history: SessionHistory::default(),
			selected: None,
			station_clip: None,
			remembered: None,
			played: Vec::new(),
			played_at: None,
			dial_filter: String::new(),
			dial: false,
			win,
			menus,
			speech,
			hotkeys,
			song: None,
			playing_since: None,
			status_base: RefCell::new(String::new()),
			status_line: RefCell::new(String::new()),
			last_status_tick: Instant::now(),
			tray: None,
			instance: None,
			readonly,
			readonly_said: false,
			rendered,
			events,
			last_device_poll: Instant::now(),
			update_due: None,
			sched_fired: std::collections::HashSet::new(),
			sched_active: None,
			last_sched_poll: Instant::now(),
			timer: Some(timer),
			engine,
		})
	}

	/// Right after the window shows.
	pub fn start(&mut self) {
		// The tuner reads its own snapshot of the settings for time-shift,
		// recording, recognition and scrobbling; save_settings refreshes it.
		self.tuner
			.set_settings(Arc::new(parking_lot::Mutex::new(self.settings.clone())));
		// Before anything saves the library.
		let first_run = store::first_run(&self.data);
		// Play/Pause right after start plays the station from last time.
		self.remembered = self.settings.last_station.clone();
		self.refresh_dial();
		// Owner rule: the dial plays on the first run or with Play Last Station at Startup;
		// otherwise it only takes focus and plays on the first arrow.
		if !self.land_dial(first_run || self.settings.play_last_at_startup) {
			self.win.play.set_focus();
			if self.settings.play_last_at_startup
				&& let Some(st) = self.remembered.clone()
			{
				self.play(st);
			}
		}
		if first_run {
			self.get_starter(true);
		}
		self.report_hotkeys();
		if self.settings.check_updates
			&& update::daily_check_due(self.settings.last_update_check, update::now())
		{
			self.update_due = Some(Instant::now() + UPDATE_DELAY);
		}
		if let Some(text) = readonly_message(self.readonly, self.readonly_said) {
			self.readonly_said = true;
			self.say(text);
		}
		self.refresh();
	}

	pub fn say(&mut self, text: &str) {
		self.say_kind(Kind::Info, text);
	}

	/// Shown in the message line, and spoken when the kind and settings allow.
	pub fn say_kind(&mut self, kind: Kind, text: &str) {
		self.win.message.set_label(&escape(text));
		if speech::should_speak(kind, &self.settings, self.dial) {
			self.speech.say(text, speech::priority(kind));
		}
	}

	/// Speaks on demand, bypassing the announcement settings: the user
	/// asked for this one by pressing its chord (Announce Now Playing
	/// promises to speak whatever the settings say).
	pub fn say_announced(&mut self, text: &str) {
		self.win.message.set_label(&escape(text));
		self.speech.say(text, speech::priority(Kind::Info));
	}

	/// Empties the message line without a word.
	pub fn clear_message(&mut self) {
		self.win.message.set_label("");
	}

	pub fn selected_or_playing(&self) -> Option<Station> {
		self.selected
			.clone()
			.or_else(|| self.tuner.playing().cloned())
	}

	/// Dial tuning: a user-driven focus move in a station list.
	pub fn tune(&mut self, stations: &[Station], index: usize) {
		let Some(station) = stations.get(index) else {
			return;
		};
		self.dial = true;
		self.song = None;
		landed(&mut self.selected, &mut self.remembered, station);
		self.history.station_changed(&station.id);
		// The window title follows focus at once; the song is added when known.
		self.now_playing(Some(&station.name));
		self.tuner.focus(stations, index, Instant::now());
		if self.settings.play_on_move {
			self.note_played(station);
		}
	}

	pub fn run(&mut self, a: Action) {
		match a {
			Action::Play => match self.selected_or_playing() {
				Some(st) => self.play(st),
				None => self.say(s::NOTHING_TO_PLAY),
			},
			Action::PlayPause => {
				// With a time-shift buffer, pause holds the stream instead of
				// disconnecting; resume picks up where it paused.
				if self.tuner.playing().is_some()
					&& let Some((_, status)) = self.tuner.shift()
				{
					let paused = status.paused.load(std::sync::atomic::Ordering::Relaxed);
					self.tuner.shift_pause(!paused);
					if self.settings.announce_status {
						self.say(if paused { s::RESUMED } else { s::PAUSED });
					}
					return;
				}
				match play_pause(
					self.tuner.playing().is_some(),
					self.remembered.clone(),
					self.selected.clone(),
				) {
					PlayPause::Stop => self.stop(true),
					PlayPause::Play(st) => self.play(st),
					PlayPause::Nothing => self.say(s::NOTHING_TO_PLAY),
				}
			}
			Action::Stop => self.stop(true),
			Action::PreviousStation => self.played_step(-1),
			Action::NextStation => self.played_step(1),
			Action::Record => {
				if self.tuner.is_recording() {
					self.finish_recording();
				} else {
					match self.tuner.start_recording() {
						Ok(_) => self.say(s::REC_STARTED),
						Err(e) => self.say(&e.to_string()),
					}
				}
			}
			Action::RecordPause => {
				if !self.tuner.is_recording() {
					self.say(s::NOT_RECORDING);
				} else if self.tuner.recording_paused() {
					self.tuner.set_recording_paused(false);
					self.say(s::REC_RESUMED);
				} else {
					self.tuner.set_recording_paused(true);
					self.say(s::REC_PAUSED);
				}
			}
			Action::SaveBuffer => self.save_buffer(),
			Action::SeekBack => self.seek(-1),
			Action::SeekForward => self.seek(1),
			Action::SeekStart => self.seek_start(),
			Action::GoLive => {
				self.tuner.shift_live();
				if self.settings.announce_status && self.tuner.shift().is_some() {
					self.say(s::LIVE);
				}
			}
			Action::Mute => {
				self.settings.muted = !self.settings.muted;
				self.mixer(MixerCmd::Mute(self.settings.muted));
				self.save_settings();
			}
			Action::VolumeUp => self.set_volume(self.volume() + VOLUME_STEP),
			Action::VolumeDown => self.set_volume(self.volume() - VOLUME_STEP),
			Action::SetVolume(v) => self.set_volume(i32::from(v)),
			Action::ToggleFavorite => match self.selected_or_playing() {
				Some(st) => {
					let added = self.library.toggle_favorite(&st);
					self.say(if added {
						s::ADDED_TO_FAVORITES
					} else {
						s::REMOVED_FROM_FAVORITES
					});
					self.save_library();
				}
				None => self.say(s::NOTHING_PLAYING),
			},
			Action::PlayFavorite(i) => match self.library.favorites.get(i).cloned() {
				Some(st) => self.play(st),
				None => self.say(&format!("{}{}", s::NO_FAVORITE, i + 1)),
			},
			Action::Sleep(minutes) => {
				let text = self.sleep.set(minutes, Instant::now());
				self.say(&text);
			}
			Action::SleepOff => {
				let text = self.sleep.cancel();
				self.say(&text);
			}
			Action::SetDevice(device) => {
				let switched = self.output.borrow_mut().switch(device.as_deref());
				match switched {
					Ok(()) => {
						self.settings.audio_device = device;
						self.save_settings();
						self.refresh_menus();
					}
					Err(_) => {
						let name = device.as_deref().unwrap_or(s::DEFAULT_DEVICE);
						self.say(&format!("{}{name}", s::FAILED));
					}
				}
			}
			Action::GetStarter => self.get_starter(false),
			Action::FocusVolume => self.win.volume.set_focus(),
			Action::Exit => {
				self.prepare_exit();
				self.win.frame.close(true);
			}
			// Windows, menus and the slider arrive in later tasks.
			Action::Close => {}
			// The main window shows them (`main_window::dispatch`): they are modal.
			Action::About
			| Action::Help
			| Action::OpenSearch
			| Action::OpenBrowse
			| Action::OpenFavorites
			| Action::OpenRecent
			| Action::NewCustomStation
			| Action::OpenCollections
			| Action::AddToCollection
			| Action::Import
			| Action::ImportAsCollection
			| Action::OpenUrl
			| Action::SongHistory
			| Action::Equalizer
			| Action::ScheduledRecordings
			| Action::GlobalHotkeys
			| Action::FeatureSettings
			| Action::StationSettings
			| Action::EditStation => {}
			Action::CopyUrl => self.copy_url(),
			Action::CopyStation => {
				if let Some(st) = self.selected.clone() {
					self.copy_station(&st, false);
				}
			}
			Action::CutStation => {
				if let Some(st) = self.selected.clone() {
					self.copy_station(&st, true);
				}
			}
			Action::PasteStation => match self.selected.clone() {
				Some(st) => self.paste_near(&st),
				None => match dial_collection(&self.library, self.settings.main_list.as_deref())
					.map(|c| c.id.clone())
				{
					Some(id) => self.paste_into(PasteDest::Collection(id, Vec::new())),
					None => self.say(s::NOWHERE_TO_PASTE),
				},
			},
			Action::CheckForUpdates => update::check(&self.win.frame, CheckTrigger::Manual),
		}
		self.refresh();
	}

	/// Every 50 ms.
	pub fn poll(&mut self) {
		#[cfg(windows)]
		crate::status_strip::sync(&self.win);
		let now = Instant::now();
		let rendered: Vec<_> = self.rendered.try_iter().collect();
		self.tuner.poll(now, &rendered);
		let events: Vec<_> = self.events.try_iter().collect();
		for event in events {
			self.tuner_event(event);
		}
		for done in self.bridge.ready() {
			done(self);
		}
		let pressed: Vec<HotkeyAction> = self.hotkeys.keys.try_iter().collect();
		for action in pressed {
			self.hotkey(action);
		}
		if let Some(guard) = &self.instance
			&& guard.raises.try_recv().is_ok()
		{
			self.toggle_window_to(true);
		}
		// Already stopped: the timer ends without a word.
		if self.sleep.due(now) && self.tuner.playing().is_some() {
			self.stop(false);
			self.say(s::SLEEP_STOPPING);
		}
		if now.duration_since(self.last_sched_poll) >= Duration::from_secs(1) {
			self.last_sched_poll = now;
			self.check_schedules();
		}
		if now.duration_since(self.last_device_poll) >= DEVICE_POLL {
			self.last_device_poll = now;
			let event = self.output.borrow_mut().poll_devices(&output_devices);
			let changed = event.is_some();
			match event {
				Some(DeviceEvent::Lost) => self.say(s::DEVICE_LOST),
				Some(DeviceEvent::Back(name)) => self.say(&format!("{}{name}", s::USING)),
				None => {}
			}
			// The Audio Device check mark follows what is there now.
			if changed {
				self.refresh_menus();
			}
		}
		// A device that could not keep the rate: decode again at the one it took.
		if let Some(rate) = self.output.borrow_mut().rate_changed() {
			let playing = self.tuner.playing().cloned();
			self.tuner.set_rate(rate);
			if let Some(st) = playing {
				self.tuner.play(&st, Instant::now());
			}
		}
		if let Some(due) = self.update_due
			&& now >= due
		{
			self.update_due = None;
			self.settings.last_update_check = Some(update::now());
			self.save_settings();
			update::check(&self.win.frame, CheckTrigger::Automatic);
		}
		self.refresh_status_line(now);
		self.refresh();
	}

	/// The menu bar is built from the settings: device events and a switch rebuild it.
	fn refresh_menus(&mut self) {
		if self.dialog_open {
			return;
		}
		self.menus = menus::build(&self.win.frame, &self.settings, &output_devices());
	}

	/// Registers the chords again after the editor changed them.
	pub fn reload_hotkeys(&mut self) {
		self.hotkeys = hotkeys::register(&self.settings);
		self.report_hotkeys();
	}

	fn report_hotkeys(&mut self) {
		let text = hotkeys::report(&self.hotkeys.taken, self.hotkeys.unavailable);
		if let Some(text) = text {
			self.say(&text);
		}
	}

	/// A chord pressed anywhere.
	fn hotkey(&mut self, action: HotkeyAction) {
		match action {
			HotkeyAction::PlayPause => self.run(Action::PlayPause),
			HotkeyAction::Stop => self.run(Action::Stop),
			HotkeyAction::VolumeUp => self.run(Action::VolumeUp),
			HotkeyAction::VolumeDown => self.run(Action::VolumeDown),
			HotkeyAction::NextFavorite => self.step_favorite(true),
			HotkeyAction::PreviousFavorite => self.step_favorite(false),
			HotkeyAction::AnnounceNowPlaying => {
				let playing = self.tuner.playing().map(|st| st.name.clone());
				// The chord speaks the status line itself: NVDA's own
				// status line command hit-tests the window's bottom-left
				// pixel and misses this frame's bar on some setups, so the
				// app guarantees the same words through its own key.
				let line = self.status_line.borrow().clone();
				let text = if playing.is_some() && !line.is_empty() {
					line
				} else {
					hotkeys::now_playing_text(self.song.as_deref(), playing.as_deref())
				};
				self.say_announced(&text);
			}
			HotkeyAction::ToggleWindow => self.toggle_window(),
			HotkeyAction::ToggleEq => self.toggle_eq(),
			HotkeyAction::Record => self.run(Action::Record),
			HotkeyAction::SeekBack => self.run(Action::SeekBack),
			HotkeyAction::SeekForward => self.run(Action::SeekForward),
			HotkeyAction::GoLive => self.run(Action::GoLive),
		}
	}

	fn step_favorite(&mut self, forward: bool) {
		let playing = self.tuner.playing().map(|st| st.id.clone());
		let Some(i) = hotkeys::next_favorite(&self.library.favorites, playing.as_deref(), forward)
		else {
			return self.say(s::NO_FAVORITES);
		};
		let station = self.library.favorites[i].clone();
		self.play(station);
	}

	fn toggle_window(&mut self) {
		let shown = self.win.frame.is_shown();
		self.toggle_window_to(!shown);
	}

	/// Shows the window with focus, or hides it to the tray.
	fn toggle_window_to(&mut self, show: bool) {
		let frame = self.win.frame;
		if show {
			frame.show(true);
			frame.raise();
			frame.set_focus();
		} else {
			frame.hide();
		}
	}

	fn toggle_eq(&mut self) {
		let mut eq = self.settings.eq.clone();
		eq.enabled = !eq.enabled;
		let text = if eq.enabled { s::EQ_ON } else { s::EQ_OFF };
		self.set_eq(eq);
		self.say(text);
	}

	/// Before the main window closes: nothing may tick or play into a destroyed window.
	pub fn shutdown(&mut self) {
		if let Some(timer) = self.timer.take() {
			timer.stop();
		}
		// Dial tuning does not save it on every focus move.
		let last = self.tuner.playing().or(self.remembered.as_ref()).cloned();
		if last.is_some() && last != self.settings.last_station {
			self.settings.last_station = last;
			self.save_settings();
		}
		self.tuner.stop();
		// `Engine` saves it on drop too, but the app may never drop on exit.
		let _ = self.engine.ctx.cache.lock().save();
	}

	/// A real quit: stop everything, remove the tray icon and end the main
	/// loop once the current event unwinds. Closing the frame alone leaves
	/// the process in the tray, because the icon outlives its handle.
	pub fn prepare_exit(&mut self) {
		self.shutdown();
		if let Some(tray) = self.tray.take() {
			tray.remove();
		}
		if let Some(wx) = get_app_instance() {
			wx.exit_main_loop();
		}
	}

	fn tuner_event(&mut self, event: TunerEvent) {
		match event {
			TunerEvent::Playing { station_id } => {
				self.playing_since = Some(Instant::now());
				// Dial tuning stays quiet: the audio is the confirmation.
				if let Some(name) = self.playing_name(&station_id) {
					self.say_kind(Kind::Status, &name);
				}
			}
			// A title queued for a station focus already left must not overwrite the playing one.
			TunerEvent::Title { station_id, title } => {
				let Some(name) = self.playing_name(&station_id) else {
					return;
				};
				self.song = title.clone();
				match &title {
					Some(song) => self.now_playing(Some(&format!("{name} - {song}"))),
					None => self.now_playing(Some(&name)),
				}
				self.history.station_changed(&station_id);
				self.history.title(title.as_deref(), SystemTime::now());
			}
			TunerEvent::Announce(song) => self.say_kind(Kind::Title, &song),
			TunerEvent::Reconnecting { .. } => self.say_kind(Kind::Failure, s::RECONNECTING),
			TunerEvent::Reconnected { .. } => self.say_kind(Kind::Info, s::RECONNECTED),
			TunerEvent::Failed { name, .. } => {
				self.say_kind(Kind::Failure, &format!("{}{name}", s::FAILED));
			}
			// The status line's per-second tick picks the format up.
			TunerEvent::Format { .. } => {}
			// Spec: measure and log warm and cold latency in debug builds.
			TunerEvent::Latency {
				station_id,
				ms,
				warm,
			} => {
				if cfg!(debug_assertions) {
					let name = self.playing_name(&station_id).unwrap_or_default();
					eprintln!("{name} warm={warm} {ms} ms");
				}
			}
		}
	}

	fn playing_name(&self, station_id: &str) -> Option<String> {
		self.tuner
			.playing()
			.filter(|p| p.id == station_id)
			.map(|p| p.name.clone())
	}

	/// Ends the active recording, if any, and says where it went. A
	/// station change or a stop ends a recording: its slot belongs to
	/// the old stream, and an unfinalized file would be lost.
	fn finish_recording(&mut self) {
		if self.tuner.is_recording() {
			match self.tuner.stop_recording() {
				Some(Ok(path)) => self.say(&format!("{}{}", s::REC_SAVED, path.display())),
				Some(Err(e)) => self.say(&e.to_string()),
				None => {}
			}
		}
	}

	/// Save Buffer: the whole time-shift buffer to a WAV in the
	/// recordings folder, streamed through in 30-second pieces so a long
	/// buffer never sits in memory all at once.
	pub fn save_buffer(&mut self) {
		use blindradio_core::record::{WavSave, default_dir};
		let Some(station) = self.tuner.playing().cloned() else {
			return self.say(s::NO_BUFFER);
		};
		let Some((buffer, _)) = self.tuner.shift() else {
			return self.say(s::NO_BUFFER);
		};
		if buffer.buffered_seconds() < 1.0 {
			return self.say(s::NO_BUFFER);
		}
		let dir = self
			.settings
			.recording
			.dir
			.as_deref()
			.map(PathBuf::from)
			.unwrap_or_else(default_dir);
		let mut save = match WavSave::create(&dir, &station.name, buffer.rate()) {
			Ok(save) => save,
			Err(e) => return self.say(&e.to_string()),
		};
		let (start, edge) = (buffer.start(), buffer.live_edge());
		let chunk = u64::from(buffer.rate()) * 30;
		let mut at = start;
		let mut piece = vec![0.0f32; (chunk * 2) as usize];
		while at < edge {
			let n = buffer.read_at(at, &mut piece);
			if n == 0 {
				break;
			}
			if let Err(e) = save.write(&piece[..n]) {
				return self.say(&e.to_string());
			}
			at += (n / 2) as u64;
		}
		match save.finish() {
			Ok(path) => self.say(&format!("{}{}", s::BUFFER_SAVED, path.display())),
			Err(e) => self.say(&e.to_string()),
		}
	}

	/// Once a second from poll(): start due scheduled recordings and
	/// stop them at their end. Nothing fires while a dialog is open; an
	/// entry whose window is still open fires when the dialog closes.
	fn check_schedules(&mut self) {
		use blindradio_core::schedule::Repeat;
		if self.dialog_open {
			return;
		}
		let now = update::now();
		if let Some((id, occ)) = self.sched_active.clone() {
			let end = self
				.settings
				.schedules
				.iter()
				.find(|e| e.id == id)
				.map(|e| e.end_of(occ));
			if end.is_some_and(|e| now >= e) || !self.tuner.is_recording() {
				if self.tuner.is_recording() {
					match self.tuner.stop_recording() {
						Some(Ok(path)) => {
							self.say(&format!("{}{}", s::SCHEDULE_SAVED, path.display()))
						}
						Some(Err(e)) => self.say(&e.to_string()),
						None => {}
					}
				}
				self.sched_active = None;
				if let Some(e) = self.settings.schedules.iter_mut().find(|e| e.id == id)
					&& e.repeat == Repeat::Once
				{
					e.enabled = false;
					self.save_settings();
				}
			}
			return;
		}
		for entry in self.settings.schedules.clone() {
			if !entry.enabled {
				continue;
			}
			let Some(occ) = entry.occurrence_at(now) else {
				continue;
			};
			if now >= entry.end_of(occ)
				|| self.sched_fired.contains(&(entry.id.clone(), occ))
			{
				continue;
			}
			self.sched_fired.insert((entry.id.clone(), occ));
			if self.tuner.is_recording() {
				self.say(s::SCHEDULE_SKIPPED);
				continue;
			}
			let name = entry.station.name.clone();
			self.play(entry.station.clone());
			match self.tuner.start_recording() {
				Ok(_) => {
					self.say(&format!("{}{name}", s::SCHEDULE_STARTED));
					self.sched_active = Some((entry.id.clone(), occ));
				}
				Err(e) => self.say(&e.to_string()),
			}
			break;
		}
	}

	pub fn play(&mut self, station: Station) {
		self.note_played(&station);
		self.play_now(station);
	}

	/// Records a play in the session history, browser-style: playing
	/// something new drops the entries ahead of the current one.
	fn note_played(&mut self, station: &Station) {
		if self
			.played_at
			.and_then(|i| self.played.get(i))
			.is_some_and(|s| s.id == station.id)
		{
			return;
		}
		let cut = self.played_at.map_or(0, |i| i + 1);
		self.played.truncate(cut);
		self.played.push(station.clone());
		if self.played.len() > 50 {
			self.played.remove(0);
		}
		self.played_at = Some(self.played.len() - 1);
	}

	/// Alt+Left / Alt+Right: move through the session history without
	/// recording the move itself.
	fn played_step(&mut self, delta: i32) {
		let Some(at) = self.played_at else {
			self.say(if delta < 0 {
				s::NO_EARLIER_STATION
			} else {
				s::NO_LATER_STATION
			});
			return;
		};
		let next = at as i32 + delta;
		if next < 0 {
			self.say(s::NO_EARLIER_STATION);
			return;
		}
		if next as usize >= self.played.len() {
			self.say(s::NO_LATER_STATION);
			return;
		}
		self.played_at = Some(next as usize);
		let station = self.played[next as usize].clone();
		self.play_now(station);
	}

	fn play_now(&mut self, station: Station) {
		self.finish_recording();
		self.dial = false;
		self.song = None;
		self.now_playing(Some(&station.name));
		// Clears the songs heard on the last station, even if this one never sends a title.
		self.history.station_changed(&station.id);
		self.tuner.play(&station, Instant::now());
		self.library.add_recent(&station);
		self.save_library();
		if self.settings.last_station.as_ref() != Some(&station) {
			self.settings.last_station = Some(station);
			self.save_settings();
		}
	}

	fn stop(&mut self, announce: bool) {
		self.finish_recording();
		if let Some(playing) = self.tuner.playing() {
			self.remembered = Some(playing.clone());
		}
		self.tuner.stop();
		self.song = None;
		self.playing_since = None;
		self.now_playing(None);
		if announce && self.settings.announce_status {
			self.say(s::STOPPED);
		}
	}

	/// One seek step in the time-shift buffer; says where playback landed.
	fn seek(&mut self, direction: i32) {
		let Some((buffer, status)) = self.tuner.shift() else {
			return;
		};
		let step = i32::try_from(self.settings.timeshift.seek_step_seconds).unwrap_or(15);
		// The status cursor only moves when the mixer processes the
		// seek, so reading it back here would announce the position
		// from BEFORE the seek (a first skip back from live said
		// "Live"). Compute where the seek lands instead.
		let before = status.behind_seconds(&buffer);
		self.tuner.shift_seek(direction * step);
		if self.settings.announce_status {
			let after = behind_after(before, direction * step, buffer_depth(&buffer));
			self.say_behind(after);
		}
	}

	fn seek_start(&mut self) {
		let Some((buffer, status)) = self.tuner.shift() else {
			return;
		};
		let _ = status;
		self.tuner.shift_start();
		if self.settings.announce_status {
			// The jump lands at the buffer's oldest audio: a full
			// buffer depth behind live, whatever the status still says.
			self.say_behind(buffer_depth(&buffer));
		}
	}

	fn say_behind(&mut self, behind: f32) {
		if behind >= 1.0 {
			self.say(&format!("{}{}", fmt_dur(behind), s::BEHIND_LIVE));
		} else {
			self.say(s::LIVE);
		}
	}

	/// The window title and the now playing line; `None` is stopped.
	fn now_playing(&self, text: Option<&str>) {
		*self.status_base.borrow_mut() = text.unwrap_or("").to_string();
		self.show_line(text.unwrap_or(""));
	}

	fn show_line(&self, text: &str) {
		// The frame title is what Alt+Tab and a screen reader's title
		// command announce: the app's name first, then the station and
		// the song, never the status suffixes.
		let base = self.status_base.borrow().clone();
		let title = if base.is_empty() {
			s::APP_NAME.to_string()
		} else {
			format!("{} - {}", s::APP_NAME, base)
		};
		if self.win.frame.get_title() != title {
			self.win.frame.set_title(&title);
		}
		let plain = if text.is_empty() { s::STOPPED } else { text };
		*self.status_line.borrow_mut() = plain.to_string();
		let line = escape(plain);
		if self.win.now_playing.get_label() != line {
			self.win.now_playing.set_label(&line);
		}
		self.win.status_bar.set_status_text(plain, 0);
		// NVDA's status text assembly reads the bar object's own name
		// first; mirror the line so it lives on the bar itself, not
		// only in its pane.
		self.win.status_bar.set_name(plain);
	}

	/// Once a second: the status line gains how long the station has played,
	/// the time-shift position and the recording marker.
	fn refresh_status_line(&mut self, now: Instant) {
		if now.duration_since(self.last_status_tick) < Duration::from_secs(1) {
			return;
		}
		self.last_status_tick = now;
		let base = self.status_base.borrow().clone();
		if base.is_empty() || self.tuner.playing().is_none() {
			return;
		}
		let mut line = base;
		if let Some(since) = self.playing_since {
			line += &format!(" - {}{}", fmt_dur(since.elapsed().as_secs_f32()), s::PLAYED_FOR);
		}
		let mut live = true;
		if let Some((buffer, status)) = self.tuner.shift() {
			let behind = status.behind_seconds(&buffer);
			if behind >= 1.0 {
				line += &format!(" - {}{}", fmt_dur(behind), s::BEHIND_LIVE);
				live = false;
			}
			if status.paused.load(std::sync::atomic::Ordering::Relaxed) {
				line += s::STATUS_PAUSED;
				live = false;
			}
		}
		if live {
			line += s::STATUS_LIVE;
		}
		if self.tuner.is_recording() {
			line += if self.tuner.recording_paused() {
				s::STATUS_REC_PAUSED
			} else {
				s::STATUS_RECORDING
			};
		}
		if let Some(format) = self.tuner.format() {
			let described = format.describe();
			if !described.is_empty() {
				line += &format!(" - {described}");
			}
		}
		self.show_line(&line);
	}

	/// Button labels, the favorites menu item and the volume slider follow the state.
	fn refresh(&self) {
		let win = &self.win;
		let playing = self.tuner.playing().is_some();
		let shift_paused = self
			.tuner
			.shift()
			.is_some_and(|(_, st)| st.paused.load(std::sync::atomic::Ordering::Relaxed));
		set_label(
			win.play,
			if playing && !shift_paused { s::PAUSE } else { s::PLAY },
		);
		set_label(
			win.mute,
			if self.settings.muted {
				s::UNMUTE
			} else {
				s::MUTE
			},
		);
		let favorite = self
			.selected_or_playing()
			.is_some_and(|st| self.library.is_favorite(&st.id));
		let label = menus::favorite_label(favorite);
		if set_label(win.favorite, &menus::plain(label))
			&& let Some(bar) = win.frame.get_menu_bar()
		{
			set_menu_item_label(&bar, self.menus.favorite, label, menus::FAVORITE_KEYS);
		}
		if win.volume.get_value() != self.volume() {
			win.volume.set_value(self.volume());
		}
		if let Some(tray) = &self.tray {
			tray.update(self);
		}
	}

	/// The SerrebiRadio starter list into its collection. The first run says only a failure.
	fn get_starter(&mut self, first_run: bool) {
		let client = self.engine.ctx.client.clone();
		self.bridge.spawn(
			WindowToken::new(),
			async move { import::fetch_starter(&client).await },
			move |app: &mut App, body| match body {
				Ok(body) => {
					let report = import::import_starter(&mut app.library, &body);
					app.save_library();
					app.refresh_dial();
					if !first_run {
						app.say(&report.message());
					} else if !app.dialog_open && app.tuner.playing().is_none() {
						// What a normal start does, a second late.
						app.land_dial(true);
					}
				}
				Err(e) => {
					eprintln!("starter: {e}");
					app.say(s::STARTER_NOT_LOADED);
				}
			},
		);
	}

	fn volume(&self) -> i32 {
		(self.settings.volume * 100.0).round() as i32
	}

	fn set_volume(&mut self, points: i32) {
		self.settings.volume = points.clamp(0, 100) as f32 / 100.0;
		self.mixer(MixerCmd::Volume(self.settings.volume));
		self.save_settings();
	}

	fn mixer(&self, cmd: MixerCmd) {
		self.output.borrow().send(cmd);
	}

	pub fn save_settings(&mut self) {
		if let Err(e) = self.settings.save(&self.data) {
			eprintln!("settings: {e}");
			self.say(s::NOT_SAVED);
		}
		self.tuner
			.set_settings(Arc::new(parking_lot::Mutex::new(self.settings.clone())));
	}

	/// Refills the main window list; focus stays on the same station, else the same place.
	pub fn refresh_dial(&mut self) {
		let (stations, nodes) = dial_tree(
			&self.library,
			self.settings.main_list.as_deref(),
			&self.dial_filter,
		);
		let list = &self.win.stations;
		let current = list.current();
		let focus = current
			.and_then(|i| list.stations().get(i).map(|s| s.id.clone()))
			.and_then(|id| stations.iter().position(|s| s.id == id))
			.or(current)
			.unwrap_or(0);
		list.set_dial(&nodes, stations, focus);
	}

	/// The filter box changed: the tree narrows to matching stations, or
	/// comes back whole when the box empties.
	pub fn dial_filter_set(&mut self, text: String) {
		if text == self.dial_filter {
			return;
		}
		self.dial_filter = text;
		if self.dial_filter.is_empty() {
			self.win.message.set_label("");
			self.refresh_dial();
		} else {
			self.apply_dial_filter();
		}
	}

	fn apply_dial_filter(&mut self) {
		let stations = dial_stations(&self.library, self.settings.main_list.as_deref());
		let q = self.dial_filter.to_lowercase();
		let hits: Vec<usize> = stations
			.iter()
			.enumerate()
			.filter(|(_, st)| st.name.to_lowercase().contains(&q))
			.map(|(i, _)| i)
			.collect();
		self.win.message.set_label(&if hits.is_empty() {
			format!("{}: {} - {}", s::FILTER, self.dial_filter, s::NO_MATCHES)
		} else {
			format!(
				"{}: {} - {} {}",
				s::FILTER,
				self.dial_filter,
				hits.len(),
				s::MATCHES
			)
		});
		self.refresh_dial();
		if let Some(&first) = hits.first() {
			self.win.stations.select(first, false);
			self.selected = stations.get(first).cloned();
		}
	}

	/// Focuses the main window list; false when it is empty. `play`: it plays now, the last
	/// station first with Play Last Station at Startup; silent otherwise.
	fn land_dial(&mut self, play: bool) -> bool {
		let list = self.win.stations.clone();
		let stations = list.stations();
		if stations.is_empty() {
			return false;
		}
		list.list.set_focus();
		if !play {
			return true;
		}
		match self
			.settings
			.play_last_at_startup
			.then(|| self.settings.last_station.clone())
			.flatten()
		{
			Some(last) => match stations.iter().position(|s| s.id == last.id) {
				Some(i) => {
					list.select(i, false);
					self.tune(&stations, i);
					// The startup station counts as played: Alt+Left from
					// the next station must be able to come back to it.
					// tune() only records it when Play on Move is on.
					self.note_played(&stations[i]);
				}
				None => self.play(last),
			},
			None => list.land(self),
		}
		true
	}

	/// Delete in the main window list: out of its collection; the playing station plays on.
	pub fn delete_from_dial(&mut self, index: usize) {
		let Some(c) = dial_collection(&self.library, self.settings.main_list.as_deref()) else {
			return;
		};
		let id = c.id.clone();
		let Some(st) = self.win.stations.stations().get(index).cloned() else {
			return;
		};
		self.library.remove_from_collection(&id, &st.id);
		self.save_library();
		self.refresh_dial();
	}

	/// Add to Collection: says which way it went.
	pub fn toggle_in_collection(&mut self, id: &str, station: &Station) {
		let Some(name) = self
			.library
			.collections
			.iter()
			.find(|c| c.id == id)
			.map(|c| c.name.clone())
		else {
			return;
		};
		let added = self.library.toggle_in_collection(id, station);
		let prefix = if added { s::ADDED_TO } else { s::REMOVED_FROM };
		self.say(&format!("{prefix}{name}"));
		self.save_library();
		self.refresh_dial();
	}

	/// Copy or cut a station onto the clipboard. A cut station stays
	/// where it is until Paste takes it out of its home.
	pub fn copy_station(&mut self, station: &Station, cut: bool) {
		let (from_collection, from_favorites) = match self.library.station_home(&station.id) {
			Some(Home::Collection { id, .. }) => (Some(id), false),
			Some(Home::Favorite { .. }) => (None, true),
			None => (None, false),
		};
		self.station_clip = Some(Clip {
			station: station.clone(),
			cut,
			from_collection,
			from_favorites,
		});
		let prefix = if cut { s::CUT_SAID } else { s::COPIED_STATION };
		self.say(&format!("{prefix}{}", station.name));
	}

	/// Put the clipboard station at `dest`. A cut station is taken out
	/// of its home first, so pasting it into another folder of its own
	/// collection moves it there.
	pub fn paste_into(&mut self, dest: PasteDest) {
		let Some(clip) = self.station_clip.clone() else {
			return self.say(s::NOTHING_TO_PASTE);
		};
		let st = &clip.station;
		let (held, dest_name) = match &dest {
			PasteDest::Collection(id, path) => {
				let Some(c) = self.library.collections.iter().find(|c| &c.id == id) else {
					return self.say(s::NOWHERE_TO_PASTE);
				};
				(
					c.stations_at(path)
						.is_some_and(|v| v.iter().any(|x| x.id == st.id)),
					c.name.clone(),
				)
			}
			PasteDest::Favorites(path) => (
				self.library
					.fav_stations_at(path)
					.is_some_and(|v| v.iter().any(|x| x.id == st.id)),
				s::FAVORITES_LIST.to_string(),
			),
		};
		if held {
			return self.say(&format!("{}{}{}", st.name, s::IS_ALREADY_IN, dest_name));
		}
		if clip.cut {
			if let Some(from) = &clip.from_collection
				&& let Some(c) = self.library.collection_mut(from)
			{
				c.remove_id(&st.id);
			}
			if clip.from_favorites {
				self.library.remove_favorite(&st.id);
			}
			self.station_clip = None;
		}
		let landed = match &dest {
			PasteDest::Collection(id, path) => self
				.library
				.collection_mut(id)
				.and_then(|c| c.stations_at_mut(path))
				.map(|v| v.push(st.clone()))
				.is_some(),
			PasteDest::Favorites(path) => self
				.library
				.fav_stations_at_mut(path)
				.map(|v| v.push(st.clone()))
				.is_some(),
		};
		if !landed {
			return self.say(s::NOWHERE_TO_PASTE);
		}
		self.say(&format!(
			"{}{}{}{}",
			s::PASTED,
			st.name,
			s::INTO,
			dest_name
		));
		self.save_library();
		self.refresh_dial();
	}

	/// Paste beside `station`: into the folder that holds it, or the
	/// dial's collection when it is held nowhere.
	pub fn paste_near(&mut self, station: &Station) {
		let dest = match self.library.station_home(&station.id) {
			Some(Home::Collection { id, path }) => PasteDest::Collection(id, path),
			Some(Home::Favorite { path }) => PasteDest::Favorites(path),
			None => match dial_collection(&self.library, self.settings.main_list.as_deref()) {
				Some(c) => PasteDest::Collection(c.id.clone(), Vec::new()),
				None => return self.say(s::NOWHERE_TO_PASTE),
			},
		};
		self.paste_into(dest);
	}

	/// New Collection: an existing name is that collection; the station is added, never removed.
	/// Focus landed on a station in a list or tree: it plays when Play On
	/// Move is on, and is only the selection when it is off.
	pub fn focus_station(&mut self, stations: &[Station], index: usize) {
		if self.settings.play_on_move {
			self.tune(stations, index);
		} else if let Some(station) = stations.get(index) {
			self.selected = Some(station.clone());
		}
	}

	/// Into a favorites folder; says so when it lands.
	pub fn add_favorite_to(&mut self, path: &[usize], station: &Station) {
		if self.library.add_favorite_to(path, station) {
			self.say(s::ADDED_TO_FAVORITES);
			self.save_library();
		}
	}

	/// Browse and the station menu: file a station into one folder of a
	/// collection (the root-only Add to Collection could not reach them).
	pub fn add_to_collection_folder(&mut self, id: &str, path: &[usize], station: &Station) {
		let Some(name) = self
			.library
			.collections
			.iter()
			.find(|c| c.id == id)
			.map(|c| c.name.clone())
		else {
			return;
		};
		if self.library.add_to_collection_folder(id, path, station) {
			self.say(&format!("{}{name}", s::ADDED_TO));
			self.save_library();
			self.refresh_dial();
		}
	}

	pub fn add_to_new_collection(&mut self, name: &str, station: &Station) {
		let id = self.library.create_collection(name);
		if !self
			.library
			.collections_containing(&station.id)
			.contains(&id)
		{
			self.library.toggle_in_collection(&id, station);
		}
		self.say(&format!("{}{name}", s::ADDED_TO));
		self.save_library();
		self.refresh_dial();
	}

	/// The context menu's Delete Custom Station where the list has no delete of its own.
	pub fn delete_custom(&mut self, id: &str) {
		self.library.delete_custom(id);
		self.save_library();
		self.refresh_dial();
	}

	/// Out of Favorites; the playing station plays on.
	/// Add All to Collection: says how many arrived.
	pub fn add_all(&mut self, from: &str, to: &str) {
		let n = self.library.add_all_to_collection(from, to);
		let name = self
			.library
			.collections
			.iter()
			.find(|c| c.id == to)
			.map(|c| c.name.clone())
			.unwrap_or_default();
		self.save_library();
		self.say(&format!("{}{n}{}{name}", s::ADDED, s::TO));
	}

	/// Copy Stream URL: the focused station's URL, read back from the clipboard.
	fn copy_url(&mut self) {
		match self.selected_or_playing() {
			Some(st) => {
				let ok = clipboard::copy(&st.url);
				self.say(clipboard::message(ok));
			}
			None => self.say(s::NOTHING_TO_COPY),
		}
	}

	/// The equalizer changed: heard at once and remembered.
	pub fn set_eq(&mut self, eq: EqSettings) {
		self.settings.eq = eq;
		self.mixer(MixerCmd::Eq(self.settings.eq.clone()));
		self.save_settings();
	}

	pub fn save_library(&mut self) {
		if let Err(e) = self.library.save(&self.data) {
			eprintln!("library: {e}");
			self.say(s::NOT_SAVED);
		}
	}
}

/// True when the label changed.
fn set_label(button: Button, label: &str) -> bool {
	let changed = button.get_label() != label;
	if changed {
		button.set_label(label);
	}
	changed
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn behind_after_seek_math() {
		// First skip back from live must not still say live.
		assert_eq!(behind_after(0.0, -15, 600.0), 15.0);
		// Forward past the edge clamps to live.
		assert_eq!(behind_after(10.0, 15, 600.0), 0.0);
		// Back past the buffer's start clamps to its depth.
		assert_eq!(behind_after(500.0, -300, 600.0), 600.0);
		// Ordinary steps move the distance by the step.
		assert_eq!(behind_after(120.0, -15, 600.0), 135.0);
		assert_eq!(behind_after(120.0, 15, 600.0), 105.0);
	}

	#[test]
	fn play_pause_rules() {
		assert_eq!(play_pause(true, Some(1), Some(2)), PlayPause::Stop);
		assert_eq!(play_pause(true, None::<u8>, None), PlayPause::Stop);
		assert_eq!(play_pause(false, Some(1), Some(2)), PlayPause::Play(1));
		assert_eq!(play_pause(false, None, Some(2)), PlayPause::Play(2));
		assert_eq!(play_pause(false, None::<u8>, None), PlayPause::Nothing);
	}

	#[test]
	fn landing_replaces_remembered() {
		// Seeded from last session at start.
		let (mut selected, mut remembered) = (None, Some("last"));
		landed(&mut selected, &mut remembered, &"b");
		assert_eq!(
			play_pause(false, remembered, selected),
			PlayPause::Play("b"),
			"after a stop, Play/Pause plays the station the user is on"
		);
	}

	#[test]
	fn read_only_data_says_once() {
		assert_eq!(readonly_message(true, false), Some("Not saved"));
		assert_eq!(readonly_message(true, true), None, "said once");
		assert_eq!(readonly_message(false, false), None, "writable: no message");
	}

	#[test]
	fn main_list_falls_back_to_starter() {
		let mut lib = Library::default();
		assert!(dial_collection(&lib, None).is_none());
		let starter = lib.create_collection("SerrebiRadio");
		let jazz = lib.create_collection("Jazz");
		assert_eq!(dial_collection(&lib, None).unwrap().id, starter);
		assert_eq!(dial_collection(&lib, Some(&jazz)).unwrap().id, jazz);
		assert_eq!(
			dial_collection(&lib, Some("col:gone")).unwrap().id,
			starter,
			"a deleted collection falls back"
		);
	}

	#[test]
	fn all_lists_merges_every_collection_by_name() {
		let mut lib = Library::default();
		let zeta = lib.create_collection("Zeta");
		let alpha = lib.create_collection("Alpha");
		let st = |name: &str| Station {
			id: format!("custom:{name}"),
			name: name.into(),
			url: "http://x".into(),
			source: blindradio_core::station::Source::RadioBrowser,
			info: None,
		};
		lib.collections
			.iter_mut()
			.find(|c| c.id == alpha)
			.unwrap()
			.stations
			.push(st("A One"));
		lib.collections
			.iter_mut()
			.find(|c| c.id == zeta)
			.unwrap()
			.stations
			.push(st("Z One"));
		assert!(
			dial_collection(&lib, Some(ALL_LISTS)).is_none(),
			"the merged view is no single collection"
		);
		let merged = dial_stations(&lib, Some(ALL_LISTS));
		let names: Vec<&str> = merged.iter().map(|s| s.name.as_str()).collect();
		assert_eq!(names, ["A One", "Z One"], "collections in name order");
	}

	#[test]
	fn dial_tree_keeps_folders_and_dial_order() {
		let mut lib = Library::default();
		let id = lib.create_collection("SerrebiRadio");
		let st = |name: &str| Station {
			id: format!("custom:{name}"),
			name: name.into(),
			url: "http://x".into(),
			source: blindradio_core::station::Source::RadioBrowser,
			info: None,
		};
		let c = lib
			.collections
			.iter_mut()
			.find(|c| c.id == id)
			.unwrap();
		c.stations.push(st("Root One"));
		c.stations.push(st("Jazz Root"));
		let mut sub = Folder::new("Sub");
		sub.stations.push(st("Deep Jazz"));
		let mut dir = Folder::new("Dir");
		dir.stations.push(st("Dir One"));
		dir.folders.push(sub);
		c.folders.push(dir);

		fn flat(nodes: &[DialNode], out: &mut Vec<usize>) {
			for n in nodes {
				match n {
					DialNode::Station(i) => out.push(*i),
					DialNode::Folder(_, kids) => flat(kids, out),
				}
			}
		}
		let (stations, nodes) = dial_tree(&lib, None, "");
		let names: Vec<&str> = stations.iter().map(|s| s.name.as_str()).collect();
		assert_eq!(names, ["Root One", "Jazz Root", "Dir One", "Deep Jazz"]);
		let mut order = Vec::new();
		flat(&nodes, &mut order);
		assert_eq!(order, [0, 1, 2, 3], "tree order is the dial order");
		assert!(
			matches!(&nodes[2], DialNode::Folder(name, _) if name == "Dir"),
			"the folder survives as a folder row"
		);

		let (_, all) = dial_tree(&lib, Some(ALL_LISTS), "");
		assert!(
			all.iter()
				.any(|n| matches!(n, DialNode::Folder(name, _) if name == "SerrebiRadio")),
			"all playlists: one folder per playlist"
		);

		let (_, hits) = dial_tree(&lib, None, "jazz");
		let mut order = Vec::new();
		flat(&hits, &mut order);
		assert_eq!(order, [1, 3], "the filter flattens to matches, in dial order");
	}
}
