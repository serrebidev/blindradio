//! What the global hot keys do.

use super::*;

impl App {
	pub(super) fn report_hotkeys(&mut self) {
		let text = hotkeys::report(&self.hotkeys.taken, self.hotkeys.unavailable);
		if let Some(text) = text {
			self.say(&text);
		}
	}

	/// A chord pressed anywhere.
	pub(super) fn hotkey(&mut self, action: HotkeyAction) {
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

	pub(super) fn step_favorite(&mut self, forward: bool) {
		let playing = self.tuner.playing().map(|st| st.id.clone());
		let Some(i) = hotkeys::next_favorite(&self.library.favorites, playing.as_deref(), forward)
		else {
			return self.say(s::NO_FAVORITES);
		};
		let station = self.library.favorites[i].clone();
		self.play(station);
	}

	pub(super) fn toggle_window(&mut self) {
		let shown = self.win.frame.is_shown();
		self.toggle_window_to(!shown);
	}

	/// Shows the window with focus, or hides it to the tray.
	pub(super) fn toggle_window_to(&mut self, show: bool) {
		let frame = self.win.frame;
		if show {
			frame.show(true);
			frame.raise();
			frame.set_focus();
		} else {
			frame.hide();
		}
	}

	pub(super) fn toggle_eq(&mut self) {
		let mut eq = self.settings.eq.clone();
		eq.enabled = !eq.enabled;
		let text = if eq.enabled { s::EQ_ON } else { s::EQ_OFF };
		self.set_eq(eq);
		self.say(text);
	}
}
