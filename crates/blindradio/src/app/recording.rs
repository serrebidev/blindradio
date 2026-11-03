//! Recording ends, buffer saves and scheduled recordings.

use super::*;

impl App {
	/// Ends the active recording, if any, and says where it went. A
	/// station change or a stop ends a recording: its slot belongs to
	/// the old stream, and an unfinalized file would be lost.
	pub(super) fn finish_recording(&mut self) {
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
	pub(super) fn check_schedules(&mut self) {
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
			if entry.alarm {
				// An alarm plays its station at its start; the
				// recording below owns the window logic.
				let Some(occ) = entry.alarm_due(now) else {
					continue;
				};
				if self.sched_fired.contains(&(entry.id.clone(), occ)) {
					continue;
				}
				self.sched_fired.insert((entry.id.clone(), occ));
				self.say(&format!("{}{}", s::ALARM_STARTED, entry.station.name));
				self.play(entry.station.clone());
				if entry.repeat == Repeat::Once
					&& let Some(e) = self.settings.schedules.iter_mut().find(|e| e.id == entry.id)
				{
					e.enabled = false;
					self.save_settings();
				}
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
			match self.tuner.start_recording(None) {
				Ok(_) => {
					self.say(&format!("{}{name}", s::SCHEDULE_STARTED));
					self.sched_active = Some((entry.id.clone(), occ));
				}
				Err(e) => self.say(&e.to_string()),
			}
			break;
		}
	}
}
