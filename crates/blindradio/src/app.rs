//! Owns every core object and executes `Action`s. Lives in `Rc<RefCell<App>>` on the UI thread.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

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
use wx_utils::{confirm, set_menu_item_label};
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

mod dial;
mod global_keys;
mod recording;
mod stations;
mod status;

const VOLUME_STEP: i32 = 5;
const DEVICE_POLL: Duration = Duration::from_secs(2);
/// Keys rest this long before the title and now playing line follow them.
const LINE_REST: Duration = Duration::from_millis(300);
/// Keys rest this long before a volume change is written to disk.
const SETTINGS_REST: Duration = Duration::from_secs(1);
/// The daily check waits until the window and the first station are settled.
const UPDATE_DELAY: Duration = Duration::from_secs(10);
/// Audio this long makes a play: moving along the dial hears a station without choosing it.
const PLAY_COUNTS: Duration = Duration::from_secs(10);

/// The station heard first is still the one playing, and has been for `PLAY_COUNTS`.
fn play_counts(heard: Option<&(String, Instant)>, playing: Option<&str>, now: Instant) -> bool {
	heard.is_some_and(|(id, since)| {
		playing == Some(id.as_str()) && now.saturating_duration_since(*since) >= PLAY_COUNTS
	})
}

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

/// Whether the song at the time-shift position just changed to a new
/// one worth announcing: a song (not the same one, not a gap) took over.
fn shift_song_changed(prev: &Option<String>, next: &Option<String>) -> bool {
	next.is_some() && prev != next
}

/// One second of slack: below it the mixer sounds the live edge, above
/// it the listener is hearing the time-shift buffer.
pub const SHIFTED_SLACK: f32 = 1.0;

/// Hearing the buffer rather than the live edge.
pub fn is_shifted(behind: f32) -> bool {
	behind >= SHIFTED_SLACK
}

/// What a mute toggle says: the state it left playback in.
pub fn mute_message(muted: bool) -> &'static str {
	if muted { s::MUTED } else { s::UNMUTED }
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

/// Snapshot an export scope before opening a modal save dialog.
fn playlist_export_data(
	lib: &Library,
	main_list: Option<&str>,
	action: &Action,
) -> Option<(String, Vec<Station>, Vec<Folder>)> {
	match action {
		Action::ExportFavorites => Some((
			menus::plain(s::FAVORITES).trim_end_matches('…').to_string(),
			lib.favorites.clone(),
			lib.fav_folders.clone(),
		)),
		Action::ExportAllPlaylists | Action::ExportCurrentList
			if *action == Action::ExportAllPlaylists || main_list == Some(ALL_LISTS) =>
		{
			Some((
				s::EXPORT_ALL_NAME.to_string(),
				Vec::new(),
				lib.collections
					.iter()
					.map(|c| Folder {
						name: c.name.clone(),
						stations: c.stations.clone(),
						folders: c.folders.clone(),
					})
					.collect(),
			))
		}
		Action::ExportCurrentList => dial_collection(lib, main_list)
			.map(|c| (c.name.clone(), c.stations.clone(), c.folders.clone())),
		_ => None,
	}
}

/// The stations the main window lists: one collection, or every collection
/// merged in the user's order under [`ALL_LISTS`].
pub fn dial_stations(lib: &Library, main_list: Option<&str>) -> Vec<Station> {
	if main_list == Some(ALL_LISTS) {
		return lib.collections.iter().flat_map(|c| c.all_stations()).collect();
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
	/// A level's rows: its stations, then a row for each folder.
	fn level_nodes(
		stations: &[Station],
		folders: &[Folder],
		id: &str,
		path: &[usize],
		next: &mut usize,
	) -> Vec<DialNode> {
		let mut nodes: Vec<DialNode> = stations
			.iter()
			.map(|_| {
				let i = *next;
				*next += 1;
				DialNode::Station(i)
			})
			.collect();
		for (i, f) in folders.iter().enumerate() {
			let mut path = path.to_vec();
			path.push(i);
			let kids = level_nodes(&f.stations, &f.folders, id, &path, next);
			nodes.push(DialNode::Folder {
				name: f.name.clone(),
				home: (id.to_owned(), path),
				kids,
			});
		}
		nodes
	}
	let mut next = 0;
	let nodes = if main_list == Some(ALL_LISTS) {
		lib.collections
			.iter()
			.map(|c| DialNode::Folder {
				name: c.name.clone(),
				home: (c.id.clone(), Vec::new()),
				kids: level_nodes(&c.stations, &c.folders, &c.id, &[], &mut next),
			})
			.collect()
	} else {
		dial_collection(lib, main_list)
			.map(|c| level_nodes(&c.stations, &c.folders, &c.id, &[], &mut next))
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
	/// The folder path inside `from_collection` (or inside Favorites)
	/// the station was taken from: a cut pasted back into the very
	/// list it came from has nowhere to go.
	pub from_path: Vec<usize>,
}

/// Where Paste puts the clipboard station.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PasteDest {
	Collection(String, Vec<usize>),
	Favorites(Vec<usize>),
}

impl PasteDest {
	fn for_station(lib: &Library, id: &str, focused: Option<&Self>) -> Option<Self> {
		// A station can belong to several lists; the view knows which copy is focused.
		if let Some(home) = focused {
			return Some(home.clone());
		}
		match lib.station_home(id) {
			Some(Home::Collection { id, path }) => Some(Self::Collection(id, path)),
			Some(Home::Favorite { path }) => Some(Self::Favorites(path)),
			None => None,
		}
	}
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
	/// A menu (the menu bar's or a popup) is open on the main window.
	pub menu_open: bool,
	pub bridge: Bridge<App>,
	pub tuner: Tuner<LiveFactory, SharedOutput>,
	output: Rc<RefCell<Output>>,
	pub sleep: SleepTimer,
	pub history: SessionHistory,
	/// The station focused in the open list; station lists set it and clear it when they close.
	pub selected: Option<Station>,
	/// The station clipboard: Copy/Cut hold a station here until Paste.
	pub station_clip: Option<Clip>,
	/// Where Paste lands, as stated by the view the user is standing
	/// in: Browse sets this from its focused row while it is open (a
	/// folder or playlist row included) and it is cleared when the
	/// dialog closes. Without it, Paste only knew the selected
	/// station, so pasting "into the parent folder" from a folder
	/// row went to the dial collection's root — or nowhere.
	pub paste_home: Option<PasteDest>,
	/// What Play/Pause plays after a stop.
	remembered: Option<Station>,
	/// Stations played this session, oldest first, for Previous/Next
	/// Station (Alt+Left / Alt+Right).
	played: Vec<Station>,
	/// Index into `played` of the entry playing now.
	played_at: Option<usize>,
	/// What the mixer was last told to play through.
	heard_eq: EqSettings,
	/// The station tree's filter, typed in the filter box; empty shows all.
	dial_filter: String,
	/// Playback changed behind a modal: the main tree catches up when it closes.
	reveal_on_close: bool,
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
	/// The song at the time-shift position, last tick: a change is a
	/// song change the listener hears, announced like a live one.
	shift_song: Option<String>,
	/// When the playing station started sounding; drives the status line.
	playing_since: Option<Instant>,
	/// The station whose audio started, and when, until its play has been counted.
	play_heard: Option<(String, Instant)>,
	/// The status line without its time suffixes.
	status_base: RefCell<String>,
	/// The status line as last written to the bar, suffixes and all; the
	/// Announce Now Playing chord speaks it verbatim.
	status_line: RefCell<String>,
	last_status_tick: Instant,
	/// When the windows show the words `now_playing` holds back.
	line_due: Cell<Option<Instant>>,
	/// When the settings `save_settings_soon` holds back are written.
	settings_due: Option<Instant>,
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
	pub fn export_playlist(app: &Rc<RefCell<App>>, action: &Action) {
		let (data, parent) = {
			let held = app.borrow();
			(
				playlist_export_data(&held.library, held.settings.main_list.as_deref(), action),
				held.win.frame,
			)
		};
		match data {
			Some((name, stations, folders)) => {
				crate::context_menu::export_playlist(app, &parent, &name, &stations, &folders)
			}
			None => app.borrow_mut().say(s::NOTHING_TO_PLAY),
		}
	}

	pub fn new(win: MainWindow) -> blindradio_core::Result<App> {
		let data = data_dir();
		let settings = Settings::load(&data);
		let library = Library::load(&data);
		// Before the mixer and the first stream, which read them as they are made.
		settings.buffers.apply();
		let output = Output::start(settings.audio_device.as_deref())?;
		output.send(MixerCmd::Volume(settings.volume));
		output.send(MixerCmd::Mute(settings.muted));
		let heard_eq = settings.eq.clone();
		output.send(MixerCmd::Eq(heard_eq.clone()));
		output.send(MixerCmd::Hiss(settings.fm_sound));
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
			sources: {
				let sources = Sources::new(engine.ctx.client.clone(), Endpoints::default());
				sources.set_disabled(settings.disabled_sources.clone());
				Arc::new(sources)
			},
			filters: None,
			browse: browse::Memory::default(),
			dialog_open: false,
			menu_open: false,
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
			paste_home: None,
			remembered: None,
			played: Vec::new(),
			played_at: None,
			heard_eq,
			dial_filter: String::new(),
			reveal_on_close: false,
			dial: false,
			win,
			menus,
			speech,
			hotkeys,
			song: None,
			shift_song: None,
			playing_since: None,
			play_heard: None,
			status_base: RefCell::new(String::new()),
			status_line: RefCell::new(String::new()),
			last_status_tick: Instant::now(),
			line_due: Cell::new(None),
			settings_due: None,
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

	/// What a station command acts on. `selected` outlives a move onto a main tree folder,
	/// and the folder is no station: nothing is, even with one playing.
	pub fn station_here(&self) -> Option<Station> {
		if !self.dialog_open && self.win.stations.on_folder() {
			return None;
		}
		self.selected_or_playing()
	}

	/// Dial tuning: a user-driven focus move in a station list.
	/// Moving to another station ends any recording in progress:
	/// play_now finishes it, and a tuner focus change drops the
	/// recorder with the old station's taps. Ask first (Brandon's
	/// rule), so an arrow key or a stray Enter cannot kill a
	/// recording by accident. No recording, or the same station
	/// again, never asks.
	fn may_change_station(&self, next: &Station) -> bool {
		if !self.tuner.is_recording() {
			return true;
		}
		if self.tuner.playing().is_some_and(|p| p.id == next.id) {
			return true;
		}
		confirm(&self.win.frame, s::REC_CHANGE_ASK, s::REC_CHANGE_TITLE)
	}

	pub fn tune(&mut self, stations: &[Station], index: usize) {
		let Some(station) = stations.get(index) else {
			return;
		};
		if !self.may_change_station(station) {
			return;
		}
		self.dial = true;
		self.song = None;
		self.shift_song = None;
		landed(&mut self.selected, &mut self.remembered, station);
		self.history.station_changed(&station.id);
		// The window title follows focus when the keys rest; the song is added when known.
		self.now_playing(Some(&station.name));
		self.tuner.focus(stations, index, Instant::now());
		self.apply_station_volume(station);
		self.refresh_eq();
		if self.settings.play_on_move {
			self.note_played(station);
		}
		self.reveal_station(station);
	}

	pub fn run(&mut self, a: Action) {
		match a {
			Action::Play => match self.station_here() {
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
			Action::MoveToPlayingStation => self.move_to_playing_station(),
			Action::Record => {
				if self.tuner.is_recording() {
					self.finish_recording();
				} else {
					// The first file is named for the song being heard: the
					// playhead's in the buffer, the live one at the edge.
					let song = self.heard_song_at(self.behind());
					match self.tuner.start_recording(song.as_deref()) {
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
			Action::RecordSongs => {
				self.settings.recording.split_songs = !self.settings.recording.split_songs;
				let on = self.settings.recording.split_songs;
				self.save_settings();
				self.refresh_menus();
				self.say(if on { s::REC_SONGS_ON } else { s::REC_SONGS_OFF });
			}
			Action::SaveBuffer => self.save_buffer(),
			Action::WriteCurrentSegment => self.write_segment(blindradio_core::segments::Which::Current),
			Action::WritePreviousSegment => self.write_segment(blindradio_core::segments::Which::Previous),
			Action::WriteNextSegment => self.write_segment(blindradio_core::segments::Which::Next),
			Action::WriteAllSegments => self.write_all_segments(),
			Action::OpenRecordFolder => self.open_record_folder(),
			Action::PreviousSegment => self.segment_jump(false),
			Action::NextSegment => self.segment_jump(true),
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
				// The toggle says what it did, like every other action.
				self.say(mute_message(self.settings.muted));
			}
			Action::VolumeUp => self.set_volume(self.volume() + VOLUME_STEP),
			Action::VolumeDown => self.set_volume(self.volume() - VOLUME_STEP),
			Action::SetVolume(v) => self.set_volume(i32::from(v)),
			Action::ToggleFavorite => match self.station_here() {
				Some(st) => {
					let added = self.library.toggle_favorite(&st);
					self.say(if added {
						s::ADDED_TO_FAVORITES
					} else {
						s::REMOVED_FROM_FAVORITES
					});
					self.save_library();
				}
				None => self.say(s::NO_STATION_HERE),
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
				// A recording in progress is the one thing quitting
				// destroys: ask first. What is recorded so far is
				// saved either way (the shutdown stops the tuner).
				if self.tuner.is_recording()
					&& !confirm(&self.win.frame, s::REC_CLOSE_ASK, s::REC_CHANGE_TITLE)
				{
					return;
				}
				self.prepare_exit();
				self.win.frame.close(true);
			}
			Action::Close => {}
			// The main window shows them (`main_window::dispatch`): they are modal.
			Action::About
			| Action::Help
			| Action::ViewChangelog
			| Action::GitHubPage
			| Action::OpenSearch
			| Action::OpenBrowse
			| Action::OpenFavorites
			| Action::OpenRecent
			| Action::NewCustomStation
			| Action::OpenCollections
			| Action::AddToCollection
			| Action::Import
			| Action::ImportAsCollection
			| Action::ExportCurrentList
			| Action::ExportAllPlaylists
			| Action::ExportFavorites
			| Action::OpenUrl
			| Action::DetectStreams
			| Action::StationInfo
			| Action::SongHistory
			| Action::Equalizer
			| Action::ScheduledRecordings
			| Action::GlobalHotkeys
			| Action::KeyboardShortcuts
			| Action::FeatureSettings
			| Action::EditStation => {}
			Action::CopyUrl => self.copy_url(),
			Action::CopyStation => match self.focused_station() {
				Some(st) => self.copy_station(&st, false),
				None => self.say(s::NOTHING_TO_COPY),
			},
			Action::CutStation => match self.focused_station() {
				Some(st) => self.copy_station(&st, true),
				None => self.say(s::NOTHING_TO_CUT),
			},
			Action::PasteStation => {
				// Paste lands where the user is standing: the open
				// dialog's focused row first (Browse states it), then
				// the main tree's focused row, then beside the
				// selected station as before.
				if let Some(dest) = self.paste_home.clone().or_else(|| self.tree_home()) {
					self.paste_into(dest);
				} else {
					match self.selected.clone() {
						Some(st) => self.paste_near(&st),
						None => {
							match dial_collection(&self.library, self.settings.main_list.as_deref())
								.map(|c| c.id.clone())
							{
								Some(id) => self.paste_into(PasteDest::Collection(id, Vec::new())),
								None => self.say(s::NOWHERE_TO_PASTE),
							}
						}
					}
				}
			}
			Action::CheckForUpdates => update::check(&self.win.frame, CheckTrigger::Manual),
		}
		self.refresh();
	}

	/// Every 50 ms.
	pub fn poll(&mut self) {
		#[cfg(windows)]
		crate::status_strip::sync(&self.win);
		self.speech.flush();
		// Alt+Tab back in occasionally leaves the keyboard focus on
		// the frame itself — no control holds it, and NVDA reads the
		// menu bar ("File Stations Playback Recording Help"). The
		// activation handler catches most returns, but not all the
		// paths Windows takes. Whenever the frame itself holds the
		// focus and neither a dialog nor a menu is open, the focus
		// belongs on the station tree: hand it over. A control that
		// holds the focus (the tree, the volume slider) is never
		// taken from.
		if !self.dialog_open && !self.menu_open && self.win.frame.has_focus() {
			self.win.stations.list.set_focus();
		}
		let now = Instant::now();
		let rendered: Vec<_> = self.rendered.try_iter().collect();
		self.tuner.poll(now, &rendered);
		let events: Vec<_> = self.events.try_iter().collect();
		for event in events {
			self.tuner_event(event);
		}
		if play_counts(
			self.play_heard.as_ref(),
			self.tuner.playing().map(|p| p.id.as_str()),
			now,
		) && let Some((id, _)) = self.play_heard.take()
		{
			self.report_play(id);
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
		if self.settings_due.is_some_and(|due| now >= due) {
			self.save_settings();
		}
		self.flush_line(now);
		self.refresh_status_line(now);
		self.refresh();
	}

	/// The menu bar is built from the settings: device events and a switch rebuild it.
	pub fn refresh_menus(&mut self) {
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
		if self.settings_due.is_some() {
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
				self.play_heard = Some((station_id.clone(), Instant::now()));
				// Dial tuning stays quiet: the audio is the confirmation.
				if let Some(name) = self.playing_name(&station_id) {
					self.say_kind(Kind::Status, &name);
				}
			}
			// A title queued for a station focus already left must not overwrite the playing one.
			TunerEvent::Title { station_id, title, at } => {
				let Some(name) = self.playing_name(&station_id) else {
					return;
				};
				self.song = title.clone();
				match &title {
					Some(song) => self.now_playing(Some(&format!("{name} - {song}"))),
					None => self.now_playing(Some(&name)),
				}
				self.history.station_changed(&station_id);
				self.history.title(title.as_deref(), at);
				// Song-split recording rides the same event at the live
				// edge: the tuner closes the finished song's file and
				// opens the new one when the mode is on; off, this does
				// nothing. While shifted, the recorded audio is the
				// playhead's and the live title is still ahead of it, so
				// this split would cut the song at the wrong place;
				// `sync_title_song` splits there instead.
				if let Some(song) = &title
					&& !is_shifted(self.behind())
					&& let Some(done) = self.tuner.split_recording(song)
					&& let Err(e) = done
				{
					self.say(&e.to_string());
				}
			}
			TunerEvent::Announce(song) => self.say_kind(Kind::Title, &song),
			TunerEvent::Reconnecting { .. } => self.say_kind(Kind::Failure, s::RECONNECTING),
			TunerEvent::Reconnected { .. } => self.say_kind(Kind::Info, s::RECONNECTED),
			TunerEvent::Failed {
				station_id, name, ..
			} => {
				let why = self
					.tuner
					.playing()
					.filter(|p| p.id == station_id)
					.and_then(|p| p.details.unavailable.as_deref())
					.map(|why| format!(" - {why}"))
					.unwrap_or_default();
				self.say_kind(Kind::Failure, &format!("{}{name}{why}", s::FAILED));
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

	pub fn play(&mut self, station: Station) {
		// Guard before note_played: a cancelled change must not
		// enter the played history either.
		if !self.may_change_station(&station) {
			return;
		}
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
		if !self.may_change_station(&station) {
			return;
		}
		self.finish_recording();
		self.dial = false;
		self.song = None;
		self.shift_song = None;
		self.now_playing(Some(&station.name));
		// Clears the songs heard on the last station, even if this one never sends a title.
		self.history.station_changed(&station.id);
		self.tuner.play(&station, Instant::now());
		// A modal's selection and paste home must describe the same row.
		if !self.dialog_open {
			self.selected = Some(station.clone());
		}
		self.remembered = None;
		self.reveal_station(&station);
		if !self.dialog_open
			&& (self.win.frame.has_focus()
				|| self.win.filter.has_focus()
				|| self.win.stations.list.has_focus()
				|| self.win.volume.has_focus()
				|| self.win.play.has_focus()
				|| self.win.stop.has_focus()
				|| self.win.mute.has_focus()
				|| self.win.favorite.has_focus())
		{
			self.win.stations.list.set_focus();
		}
		self.apply_station_volume(&station);
		self.refresh_eq();
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
		self.shift_song = None;
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
		let depth = self.tuner.buffer_depth_seconds();
		self.tuner.shift_seek(direction * step);
		if self.settings.announce_status {
			let after = behind_after(before, direction * step, depth);
			self.say_behind(after);
		}
	}

	fn seek_start(&mut self) {
		if self.tuner.shift().is_none() {
			return;
		}
		let depth = self.tuner.buffer_depth_seconds();
		self.tuner.shift_start();
		if self.settings.announce_status {
			// The jump lands at the buffer's oldest audio: a full
			// buffer depth behind live, whatever the status still says.
			self.say_behind(depth);
		}
	}

	fn say_behind(&mut self, behind: f32) {
		if behind >= 1.0 {
			self.say(&format!("{}{}", fmt_dur(behind), s::BEHIND_LIVE));
		} else {
			self.say(s::LIVE);
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
		// The level set while a station plays is that station's
		// remembered volume, applied again when it starts.
		if let Some(playing) = self.tuner.playing() {
			let id = playing.id.clone();
			self.settings
				.station_overrides
				.entry(id)
				.or_default()
				.volume = Some(self.settings.volume);
		}
		self.save_settings_soon();
	}

	/// Landing on a station with a remembered volume makes it the
	/// current volume; one without keeps the level as it is.
	fn apply_station_volume(&mut self, station: &Station) {
		let v = self.settings.volume_for(&station.id);
		if v != self.settings.volume {
			self.settings.volume = v;
			self.mixer(MixerCmd::Volume(v));
			self.save_settings_soon();
		}
	}

	fn mixer(&self, cmd: MixerCmd) {
		self.output.borrow().send(cmd);
	}

	/// For what a held or repeated key changes: a save waits on the disk on the UI thread.
	/// poll() writes it once the keys rest, shutdown() at the latest.
	fn save_settings_soon(&mut self) {
		self.settings_due = Some(Instant::now() + SETTINGS_REST);
	}

	pub fn save_settings(&mut self) {
		self.settings_due = None;
		if let Err(e) = self.settings.save(&self.data) {
			eprintln!("settings: {e}");
			self.say(s::NOT_SAVED);
		}
		self.tuner
			.set_settings(Arc::new(parking_lot::Mutex::new(self.settings.clone())));
	}

	/// The global equalizer changed: remembered, and heard at once unless
	/// the playing station has its own.
	pub fn set_eq(&mut self, eq: EqSettings) {
		self.settings.eq = eq;
		self.refresh_eq();
		self.save_settings();
	}

	/// The mixer takes what the playing station plays through. Sent only on
	/// a change: moving along the dial comes here per key.
	pub fn refresh_eq(&mut self) {
		let id = self.tuner.playing().map_or("", |p| p.id.as_str());
		let eq = crate::dialogs::equalizer::for_station(&self.settings, id);
		self.hear_eq(eq);
	}

	fn hear_eq(&mut self, eq: EqSettings) {
		if eq != self.heard_eq {
			self.heard_eq = eq.clone();
			self.mixer(MixerCmd::Eq(eq));
		}
	}

	/// Edit Station's equalizer choices, heard as they are made when that
	/// station is the one playing. Nothing is kept: `refresh_eq` undoes it.
	pub fn preview_station_eq(
		&mut self,
		station_id: &str,
		own: Option<blindradio_core::settings::StationEq>,
	) {
		if self.tuner.playing().is_some_and(|p| p.id == station_id) {
			self.hear_eq(crate::dialogs::equalizer::with_own(&self.settings, own.as_ref()));
		}
	}

	pub fn playing_has_own_eq(&self) -> bool {
		self.tuner.playing().is_some_and(|p| {
			self.settings
				.station_overrides
				.get(&p.id)
				.is_some_and(|o| o.eq.is_some())
		})
	}

	/// Best effort: a play that fails to count is nobody's problem.
	fn report_play(&self, station_id: String) {
		if !self.settings.report_plays {
			return;
		}
		let sources = self.sources.clone();
		self.bridge.spawn(
			WindowToken::new(),
			async move {
				let _ = sources.report_play(&station_id).await;
			},
			|_: &mut App, ()| {},
		);
	}

	/// The FM sound toggled in Settings: the mixer hears it at once.
	/// Name Automatically's lookup, on the shared HTTP client.
	pub fn station_name_lookup(
		&self,
		url: String,
	) -> impl Future<Output = Option<String>> + Send + 'static {
		let client = self.engine.ctx.client.clone();
		async move { blindradio_core::net::station_name(&client, &url).await.ok() }
	}

	pub fn apply_fm_sound(&self) {
		self.mixer(MixerCmd::Hiss(self.settings.fm_sound));
	}

	/// The playing station takes a new start buffer and limit at once; a new ring size and
	/// HLS position wait for the next station opened.
	pub fn apply_buffers(&self) {
		let buffers = self.settings.buffers;
		buffers.apply();
		self.mixer(MixerCmd::Buffers {
			start_ms: buffers.start_ms,
			refill_limit_ms: buffers.refill_limit_seconds * 1000,
		});
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
	fn a_play_counts_only_for_the_station_still_playing_after_the_wait() {
		let start = Instant::now();
		let later = start + PLAY_COUNTS;
		let heard = ("rb:a".to_string(), start);
		assert!(play_counts(Some(&heard), Some("rb:a"), later));
		assert!(!play_counts(Some(&heard), Some("rb:a"), later - Duration::from_secs(1)));
		assert!(!play_counts(Some(&heard), Some("rb:b"), later), "moved on");
		assert!(!play_counts(Some(&heard), None, later), "stopped");
		assert!(!play_counts(None, Some("rb:a"), later), "already counted");
	}

	#[test]
	fn file_exports_preserve_scopes_and_nested_folders() {
		let mut lib = Library::default();
		let custom = lib.add_custom("Custom", "http://custom");
		let id = lib.create_collection("Jazz");
		let mut inner = Folder::new("Inside");
		inner.stations.push(custom.clone());
		let mut outer = Folder::new("Outside");
		outer.folders.push(inner);
		lib.collection_mut(&id).unwrap().folders.push(outer.clone());
		lib.fav_folders.push(outer.clone());
		let (name, stations, folders) =
			playlist_export_data(&lib, Some(&id), &Action::ExportCurrentList).unwrap();
		assert_eq!(name, "Jazz");
		assert!(stations.is_empty());
		assert_eq!(folders, vec![outer.clone()]);
		let (_, stations, folders) =
			playlist_export_data(&lib, Some(&id), &Action::ExportFavorites).unwrap();
		assert!(stations.is_empty());
		assert_eq!(folders, vec![outer.clone()]);
		let all = playlist_export_data(&lib, Some(&id), &Action::ExportAllPlaylists).unwrap();
		assert!(all.1.is_empty());
		assert_eq!(all.2.len(), 2);
		assert_eq!(all.2[0].stations, vec![custom]);
		assert_eq!(all.2[1].folders, vec![outer]);
		assert_eq!(
			playlist_export_data(&lib, Some(ALL_LISTS), &Action::ExportCurrentList),
			Some(all)
		);
		assert!(
			playlist_export_data(&Library::default(), None, &Action::ExportCurrentList).is_none()
		);
	}

	#[test]
	fn clipboard_home_prefers_the_browse_row_over_another_copy() {
		let mut lib = Library::default();
		let station = lib.add_custom("WRN Europe", "http://wrn");
		let english = lib.create_collection("English");
		let misc = lib
			.collection_mut(&english)
			.unwrap()
			.add_folder(&[], "Misc")
			.unwrap();
		lib.collection_mut(&english)
			.unwrap()
			.stations_at_mut(&misc)
			.unwrap()
			.push(station.clone());
		let source = PasteDest::Collection(english.clone(), misc);
		let root = PasteDest::Collection(english, Vec::new());
		assert_eq!(
			PasteDest::for_station(&lib, &station.id, Some(&source)),
			Some(source)
		);
		assert_eq!(
			PasteDest::for_station(&lib, &station.id, Some(&root)),
			Some(root)
		);
		assert_eq!(
			PasteDest::for_station(&lib, &station.id, None),
			Some(PasteDest::Collection(
				blindradio_core::library::CUSTOM.into(),
				Vec::new()
			))
		);
	}

	#[test]
	fn mute_toggle_says_the_state_it_left() {
		assert_eq!(mute_message(true), "Muted");
		assert_eq!(mute_message(false), "Unmuted");
	}

	#[test]
	fn shifted_means_hearing_the_buffer() {
		// At the live edge the mixer's cushion is under a second.
		assert!(!is_shifted(0.0));
		assert!(!is_shifted(0.4));
		// A skip back is plainly the buffer.
		assert!(is_shifted(1.0));
		assert!(is_shifted(15.0));
		assert!(is_shifted(600.0));
	}

	#[test]
	fn shift_song_changes_announce_once_per_song() {
		let a = Some("Song A".to_string());
		let b = Some("Song B".to_string());
		assert!(shift_song_changed(&None, &a), "first song in the buffer");
		assert!(!shift_song_changed(&a, &a), "same song keeps playing");
		assert!(shift_song_changed(&a, &b), "next song takes over");
		assert!(!shift_song_changed(&a, &None), "a gap announces nothing");
		assert!(!shift_song_changed(&None, &None), "still no song");
	}

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
	fn all_lists_merges_every_collection_in_the_users_order() {
		let mut lib = Library::default();
		let zeta = lib.create_collection("Zeta");
		let alpha = lib.create_collection("Alpha");
		let st = |name: &str| Station {
			id: format!("custom:{name}"),
			name: name.into(),
			url: "http://x".into(),
			source: blindradio_core::station::Source::RadioBrowser,
			info: None,
			details: Default::default(),
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
		assert_eq!(names, ["Z One", "A One"], "collections in the library's order");
		// Moving a collection moves its row in the merged view.
		assert!(lib.move_collection(&alpha, true));
		let merged = dial_stations(&lib, Some(ALL_LISTS));
		let names: Vec<&str> = merged.iter().map(|s| s.name.as_str()).collect();
		assert_eq!(names, ["A One", "Z One"]);
		let (_, nodes) = dial_tree(&lib, Some(ALL_LISTS), "");
		assert!(
			matches!(&nodes[1], DialNode::Folder { name, .. } if name == "Alpha"),
			"the tree's rows follow, after Custom Stations"
		);
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
			details: Default::default(),
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
					DialNode::Folder { kids, .. } => flat(kids, out),
				}
			}
		}
		let (stations, nodes) = dial_tree(&lib, None, "");
		let names: Vec<&str> = stations.iter().map(|s| s.name.as_str()).collect();
		assert_eq!(names, ["Root One", "Jazz Root", "Dir One", "Deep Jazz"]);
		let mut order = Vec::new();
		flat(&nodes, &mut order);
		assert_eq!(order, [0, 1, 2, 3], "tree order is the dial order");
		let DialNode::Folder { name, home, kids } = &nodes[2] else {
			panic!("the folder survives as a folder row");
		};
		assert_eq!(name, "Dir");
		assert_eq!(*home, (id.clone(), vec![0]), "a row knows its own place");
		assert!(
			matches!(&kids[1], DialNode::Folder { home, .. } if *home == (id.clone(), vec![0, 0])),
			"at any depth"
		);

		let (_, all) = dial_tree(&lib, Some(ALL_LISTS), "");
		assert!(
			all.iter().any(|n| matches!(
				n,
				DialNode::Folder { name, home, .. }
					if name == "SerrebiRadio" && *home == (id.clone(), Vec::new())
			)),
			"all playlists: one folder per playlist"
		);

		let (_, hits) = dial_tree(&lib, None, "jazz");
		let mut order = Vec::new();
		flat(&hits, &mut order);
		assert_eq!(order, [1, 3], "the filter flattens to matches, in dial order");
	}
}
