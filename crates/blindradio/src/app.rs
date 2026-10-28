//! Owns every core object and executes `Action`s. Lives in `Rc<RefCell<App>>` on the UI thread.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use blindradio_core::history::SessionHistory;
use blindradio_core::library::Library;
use blindradio_core::output::{DeviceEvent, MixerCmd, Output, output_devices};
use blindradio_core::settings::Settings;
use blindradio_core::sleep::SleepTimer;
use blindradio_core::sources::{Endpoints, Sources};
use blindradio_core::station::Station;
use blindradio_core::tuner::{LiveFactory, Sink, Tuner, TunerEvent};
use blindradio_core::{Engine, import, store};
use crossbeam_channel::Receiver;
use wx_utils::set_menu_item_label;
use wxdragon::prelude::*;

use crate::action::{Action, SettingId};
use crate::bridge::{Bridge, WindowToken};
use crate::main_window::{MainWindow, escape};
use crate::menus::{self, MenuIds};
use crate::strings as s;

const VOLUME_STEP: i32 = 5;
const DEVICE_POLL: Duration = Duration::from_secs(2);

/// The tuner sends mixer commands through it while the app switches devices on it.
pub struct SharedOutput(Rc<RefCell<Output>>);

impl Sink for SharedOutput {
	fn send(&self, cmd: MixerCmd) {
		self.0.borrow().send(cmd);
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

pub struct App {
	pub data: PathBuf,
	pub settings: Settings,
	pub library: Library,
	#[expect(dead_code, reason = "Search and Browse use it (Tasks 3 and 4)")]
	pub sources: Arc<Sources>,
	pub bridge: Bridge<App>,
	pub tuner: Tuner<LiveFactory, SharedOutput>,
	output: Rc<RefCell<Output>>,
	pub sleep: SleepTimer,
	pub history: SessionHistory,
	/// The station focused in the open list; station lists set it and clear it when they close.
	pub selected: Option<Station>,
	/// What Play/Pause plays after a stop.
	remembered: Option<Station>,
	/// The last tune was dial tuning (a focus move), not an explicit play.
	dial: bool,
	pub win: MainWindow,
	pub menus: MenuIds,
	rendered: Receiver<(u64, Instant)>,
	events: Receiver<TunerEvent>,
	last_device_poll: Instant,
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
		Ok(App {
			sources: Arc::new(Sources::new(
				engine.ctx.client.clone(),
				Endpoints::default(),
			)),
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
			remembered: None,
			dial: false,
			win,
			menus,
			rendered,
			events,
			last_device_poll: Instant::now(),
			timer: Some(Timer::new(&win.frame)),
			engine,
		})
	}

	/// Right after the window shows.
	pub fn start(&mut self) {
		// Before anything saves the library.
		let first_run = store::first_run(&self.data);
		// Play/Pause right after start plays the station from last time.
		self.remembered = self.settings.last_station.clone();
		if self.settings.play_last_at_startup
			&& let Some(st) = self.remembered.clone()
		{
			self.play(st);
		}
		if first_run {
			self.get_starter(true);
		}
		self.refresh();
	}

	pub fn say(&mut self, text: &str) {
		self.win.message.set_label(&escape(text));
	}

	pub fn selected_or_playing(&self) -> Option<Station> {
		self.selected
			.clone()
			.or_else(|| self.tuner.playing().cloned())
	}

	/// Dial tuning: a user-driven focus move in a station list.
	#[expect(
		dead_code,
		reason = "the main window dial and station lists tune (Task 3)"
	)]
	pub fn tune(&mut self, stations: &[Station], index: usize) {
		let Some(station) = stations.get(index) else {
			return;
		};
		self.dial = true;
		self.selected = Some(station.clone());
		self.history.station_changed(&station.id);
		// The window title follows focus at once; the song is added when known.
		self.now_playing(Some(&station.name));
		self.tuner.focus(stations, index, Instant::now());
	}

	pub fn run(&mut self, a: Action) {
		match a {
			Action::Play => match self.selected_or_playing() {
				Some(st) => self.play(st),
				None => self.say(s::NOTHING_TO_PLAY),
			},
			Action::PlayPause => match play_pause(
				self.tuner.playing().is_some(),
				self.remembered.clone(),
				self.selected.clone(),
			) {
				PlayPause::Stop => self.stop(true),
				PlayPause::Play(st) => self.play(st),
				PlayPause::Nothing => self.say(s::NOTHING_TO_PLAY),
			},
			Action::Stop => self.stop(true),
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
					}
					Err(_) => {
						let name = device.as_deref().unwrap_or(s::DEFAULT_DEVICE);
						self.say(&format!("{}{name}", s::FAILED));
					}
				}
			}
			Action::ToggleSetting(id) => {
				let flag = match id {
					SettingId::AnnounceTitles => &mut self.settings.announce_titles,
					SettingId::AnnounceStatus => &mut self.settings.announce_status,
					SettingId::CloseListsWhenPlaying => &mut self.settings.close_lists_when_playing,
					SettingId::PlayLastAtStartup => &mut self.settings.play_last_at_startup,
				};
				*flag = !*flag;
				self.save_settings();
			}
			Action::GetStarter => self.get_starter(false),
			Action::FocusVolume => self.win.volume.set_focus(),
			Action::Exit => {
				self.shutdown();
				self.win.frame.close(true);
			}
			// Windows, menus and the slider arrive in Tasks 2 to 7.
			Action::OpenSearch
			| Action::OpenBrowse
			| Action::OpenFavorites
			| Action::OpenRecent
			| Action::OpenCollections
			| Action::NewCustomStation
			| Action::CopyUrl
			| Action::SongHistory
			| Action::AddToCollection
			| Action::Import
			| Action::ImportAsCollection
			| Action::Equalizer
			| Action::Help
			| Action::Close => {}
			// The main window shows it (`main_window::dispatch`): it is modal.
			Action::About => {}
		}
		self.refresh();
	}

	/// Every 50 ms.
	pub fn poll(&mut self) {
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
		// Already stopped: the timer ends without a word.
		if self.sleep.due(now) && self.tuner.playing().is_some() {
			self.stop(false);
			self.say(s::SLEEP_STOPPING);
		}
		if now.duration_since(self.last_device_poll) >= DEVICE_POLL {
			self.last_device_poll = now;
			let event = self.output.borrow_mut().poll_devices(&output_devices);
			match event {
				Some(DeviceEvent::Lost) => self.say(s::DEVICE_LOST),
				Some(DeviceEvent::Back(name)) => self.say(&format!("{}{name}", s::USING)),
				None => {}
			}
		}
		self.refresh();
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

	fn tuner_event(&mut self, event: TunerEvent) {
		match event {
			TunerEvent::Playing { station_id } => {
				// Dial tuning stays quiet: the audio is the confirmation.
				if self.settings.announce_status
					&& !self.dial && let Some(name) = self.playing_name(&station_id)
				{
					self.say(&name);
				}
			}
			// A title queued for a station focus already left must not overwrite the playing one.
			TunerEvent::Title { station_id, title } => {
				let Some(name) = self.playing_name(&station_id) else {
					return;
				};
				match &title {
					Some(song) => self.now_playing(Some(&format!("{song} - {name}"))),
					None => self.now_playing(Some(&name)),
				}
				self.history.station_changed(&station_id);
				self.history.title(title.as_deref(), SystemTime::now());
			}
			TunerEvent::Announce(song) => {
				if self.settings.announce_titles {
					self.say(&song);
				}
			}
			TunerEvent::Reconnecting { .. } => self.say(s::RECONNECTING),
			TunerEvent::Reconnected { .. } => self.say(s::RECONNECTED),
			TunerEvent::Failed { name, .. } => self.say(&format!("{}{name}", s::FAILED)),
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

	fn play(&mut self, station: Station) {
		self.dial = false;
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
		if let Some(playing) = self.tuner.playing() {
			self.remembered = Some(playing.clone());
		}
		self.tuner.stop();
		self.now_playing(None);
		if announce && self.settings.announce_status {
			self.say(s::STOPPED);
		}
	}

	/// The window title and the now playing line; `None` is stopped.
	fn now_playing(&self, text: Option<&str>) {
		let title = text.unwrap_or(s::APP_NAME);
		if self.win.frame.get_title() != title {
			self.win.frame.set_title(title);
		}
		let line = escape(text.unwrap_or(s::STOPPED));
		if self.win.now_playing.get_label() != line {
			self.win.now_playing.set_label(&line);
		}
	}

	/// Button labels, the favorites menu item and the volume slider follow the state.
	fn refresh(&self) {
		let win = &self.win;
		let playing = self.tuner.playing().is_some();
		set_label(win.play, if playing { s::PAUSE } else { s::PLAY });
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
					// TODO(Task 3): refresh the main window dial.
					if !first_run {
						app.say(&report.message());
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

	fn save_settings(&mut self) {
		if let Err(e) = self.settings.save(&self.data) {
			eprintln!("settings: {e}");
			self.say(s::NOT_SAVED);
		}
	}

	fn save_library(&mut self) {
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
	fn play_pause_rules() {
		assert_eq!(play_pause(true, Some(1), Some(2)), PlayPause::Stop);
		assert_eq!(play_pause(true, None::<u8>, None), PlayPause::Stop);
		assert_eq!(play_pause(false, Some(1), Some(2)), PlayPause::Play(1));
		assert_eq!(play_pause(false, None, Some(2)), PlayPause::Play(2));
		assert_eq!(play_pause(false, None::<u8>, None), PlayPause::Nothing);
	}
}
