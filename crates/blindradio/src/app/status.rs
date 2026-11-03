//! The now-playing line, the status line and the title.

use super::*;

impl App {
	/// The window title and the now playing line; `None` is stopped.
	pub(super) fn now_playing(&self, text: Option<&str>) {
		*self.status_base.borrow_mut() = text.unwrap_or("").to_string();
		self.show_line(text.unwrap_or(""), text.unwrap_or(""));
	}

	/// `text` is the full line (the now-playing label and what the
	/// announce chord speaks); `bar` is what the status bar carries.
	/// They differ: the bar drops the station and song — those are in
	/// the title already, and on the bar they crowd out the played
	/// time, the position and the format (Brandon's call).
	pub(super) fn show_line(&self, text: &str, bar: &str) {
		// The title stays as it has always been: the app's name and
		// the base line (station and song), never the status
		// suffixes — the played time and format live on the status
		// bar, not up here (Brandon, explicit, after 0.2.22 tried
		// putting the whole line in the title and he rejected it).
		let base = self.status_base.borrow().clone();
		let title = if base.is_empty() {
			s::APP_NAME.to_string()
		} else {
			format!("{} - {}", s::APP_NAME, base)
		};
		// Brandon's one addition to the title: while a recording is
		// in progress the word Recording rides at the end, so Alt+Tab
		// and the taskbar say it too. Nothing else goes up here.
		let title = if self.tuner.is_recording() {
			format!("{} - {}", title, s::TITLE_RECORDING)
		} else {
			title
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
		let bar = if bar.is_empty() { plain } else { bar };
		self.win.status_bar.set_status_text(bar, 0);
		#[cfg(windows)]
		crate::status_strip::set_text(bar);
		// NVDA's status text assembly reads the bar object's own name
		// first; mirror the line so it lives on the bar itself, not
		// only in its pane.
		self.win.status_bar.set_name(bar);
	}

	/// How far playback trails the live edge; 0 at live, and 0 with no
	/// time-shift buffer.
	pub(super) fn behind(&self) -> f32 {
		self.tuner
			.shift()
			.map_or(0.0, |(buffer, status)| status.behind_seconds(&buffer))
	}

	/// The song heard `behind` seconds short of live: the playhead's in
	/// the buffer, the live one at the live edge.
	pub(super) fn heard_song_at(&self, behind: f32) -> Option<String> {
		if !is_shifted(behind) {
			return self.song.clone();
		}
		let when = SystemTime::now()
			.checked_sub(Duration::from_secs_f32(behind))
			.unwrap_or(SystemTime::UNIX_EPOCH);
		self.history.title_at(when)
	}

	/// The title's song follows the time-shift position: shifted
	/// back, the song at the buffer's moment is what is playing, not
	/// the live one — titles are timestamped as they arrive, so the
	/// session history names the song of any moment in the buffer.
	/// A song change at the position is announced like a live one.
	pub(super) fn sync_title_song(&mut self, behind: f32) {
		// The name is cloned up front: announcing below needs &mut self.
		let Some(name) = self.tuner.playing().map(|s| s.name.clone()) else {
			return;
		};
		let shifted = is_shifted(behind);
		let song = if shifted {
			let song = self.heard_song_at(behind);
			// Shifted listening hears song changes too: as playback
			// moves through the buffer, crossing into the next song
			// announces it, the way a live change is announced. Live
			// listening is the tuner's own Announce event, so this
			// only speaks while shifted.
			if shift_song_changed(&self.shift_song, &song) {
				let text = song.clone().unwrap_or_default();
				self.say_kind(Kind::Title, &text);
			}
			self.shift_song = song.clone();
			song
		} else {
			self.shift_song = None;
			self.song.clone()
		};
		// A split recording follows the audio it is writing, and in the
		// buffer that audio is the playhead's song — the live title is
		// still ahead of it, so splitting on that cut each song early
		// and left its start in the next file. The split therefore
		// happens here while shifted, and at the live title at the edge.
		if shifted
			&& let Some(song) = &song
			&& let Some(done) = self.tuner.split_recording(song)
			&& let Err(e) = done
		{
			self.say(&e.to_string());
		}
		let base = match &song {
			Some(song) => format!("{name} - {song}"),
			None => name,
		};
		if *self.status_base.borrow() != base {
			self.now_playing(Some(&base));
		}
	}

	/// Once a second: the status line gains how long the station has played,
	/// the time-shift position and the recording marker.
	pub(super) fn refresh_status_line(&mut self, now: Instant) {
		if now.duration_since(self.last_status_tick) < Duration::from_secs(1) {
			return;
		}
		self.last_status_tick = now;
		if self.tuner.playing().is_none() {
			return;
		}
		let mut behind = 0.0;
		let mut paused = false;
		if let Some((buffer, status)) = self.tuner.shift() {
			behind = status.behind_seconds(&buffer);
			paused = status.paused.load(std::sync::atomic::Ordering::Relaxed);
		}
		self.sync_title_song(behind);
		let base = self.status_base.borrow().clone();
		if base.is_empty() {
			return;
		}
		// The parts after the station and song. The full line joins
		// them onto the base; the status bar carries the parts alone.
		let mut parts: Vec<String> = Vec::new();
		if let Some(since) = self.playing_since {
			parts.push(format!(
				"{}{}",
				fmt_dur(since.elapsed().as_secs_f32()),
				s::PLAYED_FOR
			));
		}
		if behind >= 1.0 {
			parts.push(format!("{}{}", fmt_dur(behind), s::BEHIND_LIVE));
		}
		if paused {
			parts.push(s::STATUS_PAUSED.trim_start_matches(" - ").to_string());
		}
		if behind < 1.0 && !paused {
			parts.push(s::STATUS_LIVE.trim_start_matches(" - ").to_string());
		}
		if self.tuner.is_recording() {
			parts.push(
				if self.tuner.recording_paused() {
					s::STATUS_REC_PAUSED
				} else {
					s::STATUS_RECORDING
				}
				.trim_start_matches(" - ")
				.to_string(),
			);
		}
		if let Some(format) = self.tuner.format() {
			let described = format.describe();
			if !described.is_empty() {
				parts.push(described);
			}
		}
		let joined = parts.join(" - ");
		let line = if joined.is_empty() {
			base.clone()
		} else {
			format!("{base} - {joined}")
		};
		let bar = if joined.is_empty() { base } else { joined };
		self.show_line(&line, &bar);
	}

	/// Button labels, the favorites menu item and the volume slider follow the state.
	pub(super) fn refresh(&self) {
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
			.station_here()
			.is_some_and(|st| self.library.is_favorite(&st.id));
		let label = menus::favorite_label(favorite);
		if set_label(win.favorite, &menus::plain(label))
			&& let Some(bar) = win.frame.get_menu_bar()
		{
			set_menu_item_label(
				&bar,
				self.menus.favorite,
				label,
				&crate::keys::menu_chord(&self.settings, &Action::ToggleFavorite),
			);
		}
		if win.volume.get_value() != self.volume() {
			win.volume.set_value(self.volume());
		}
		if let Some(tray) = &self.tray {
			tray.update(self);
		}
	}
}
