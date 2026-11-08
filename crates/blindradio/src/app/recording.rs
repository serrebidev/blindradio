//! Recording ends, buffer saves, segments and scheduled recordings.

use blindradio_core::segments::{Jump, Segment, Which};

use super::*;

impl App {
	/// Ends the active recording, if any, and says where it went. A
	/// station change or a stop ends a recording: its slot belongs to
	/// the old stream, and an unfinalized file would be lost.
	pub(super) fn finish_recording(&mut self) {
		if self.tuner.is_recording() {
			match self.tuner.stop_recording() {
				Some(Ok(path)) => self.say(&format!("{}{}", s::rec_saved(), path.display())),
				Some(Err(e)) => self.say(&e.to_string()),
				None => {}
			}
		}
	}

	/// The folder Settings chooses, or the default.
	fn record_dir(&self) -> PathBuf {
		self.settings
			.recording
			.dir
			.as_deref()
			.map(PathBuf::from)
			.unwrap_or_else(blindradio_core::record::default_dir)
	}

	/// Open Record Folder. Made first: the default folder does not exist
	/// until something has been recorded.
	pub(super) fn open_record_folder(&mut self) {
		let dir = self.record_dir();
		if let Err(e) = std::fs::create_dir_all(&dir) {
			return self.say(&e.to_string());
		}
		if !wxdragon::prelude::launch_default_application(&dir.to_string_lossy()) {
			self.say(&s::no_record_folder());
		}
	}

	/// Save Buffer: the whole time-shift buffer to a WAV in the
	/// recordings folder, streamed through in 30-second pieces so a long
	/// buffer never sits in memory all at once.
	pub fn save_buffer(&mut self) {
		use blindradio_core::record::WavSave;
		let Some(station) = self.tuner.playing().cloned() else {
			return self.say(&s::no_buffer());
		};
		let Some((buffer, _)) = self.tuner.shift() else {
			return self.say(&s::no_buffer());
		};
		if buffer.buffered_seconds() < 1.0 {
			return self.say(&s::no_buffer());
		}
		let dir = self.record_dir();
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
			Ok(path) => self.say(&format!("{}{}", s::buffer_saved(), path.display())),
			Err(e) => self.say(&e.to_string()),
		}
	}

	/// Previous or Next Segment: playback moves there and says what it is.
	pub(super) fn segment_jump(&mut self, forward: bool) {
		match self.tuner.segment_jump(forward) {
			Jump::To(segment) => {
				// Said here, so the once-a-second title check stays quiet.
				self.shift_song = Some(segment.title.clone());
				let behind = self
					.tuner
					.live_ms()
					.map_or(0, |live| live.saturating_sub(segment.start_ms));
				let mut text = segment.title;
				if self.settings.announce_status {
					let behind = fmt_dur(behind as f32 / 1000.0);
					text = format!("{text} - {behind}{}", s::behind_live());
				}
				self.say(&text);
			}
			Jump::Live => self.run(Action::GoLive),
			Jump::Nowhere if self.tuner.segments().is_empty() => self.say(&s::no_segments()),
			Jump::Nowhere if forward => self.say(&s::no_next_segment()),
			Jump::Nowhere => self.say(&s::no_previous_segment()),
		}
	}

	/// Write Current, Previous or Next Segment: says which one it took,
	/// since the choice follows the playhead.
	pub(super) fn write_segment(&mut self, which: Which) {
		let Some(segment) = self.tuner.segment(which) else {
			return self.say(
				&(match which {
					_ if self.tuner.segments().is_empty() => s::no_segments(),
					Which::Previous => s::no_previous_segment(),
					Which::Next => s::no_next_segment(),
					Which::Current => s::no_segments(),
				}),
			);
		};
		self.say(&format!("{}{}", s::writing_segment(), segment.title));
		self.write_segments(vec![segment]);
	}

	/// Write All Segments: one file for each, the unfinished live one too.
	pub(super) fn write_all_segments(&mut self) {
		let segments = self.tuner.segments();
		if segments.is_empty() {
			return self.say(&s::no_segments());
		}
		self.say(&format!("{}{}{}", s::writing_segment(), segments.len(), s::segments()));
		self.write_segments(segments);
	}

	/// Off the UI thread: encoding a whole buffer takes far longer than a
	/// window may stall under a screen reader.
	fn write_segments(&mut self, segments: Vec<Segment>) {
		let Some(writer) = self.tuner.segment_writer() else {
			return self.say(&s::no_segments());
		};
		let dir = writer.dir().to_path_buf();
		self.bridge.spawn(
			WindowToken::new(),
			async move {
				tokio::task::spawn_blocking(move || writer.write(&segments).map_err(|e| e.to_string()))
					.await
					.map_err(|e| e.to_string())
					.and_then(|written| written)
			},
			move |app: &mut App, written| match written.as_deref() {
				Ok([path]) => app.say(&format!("{}{}", s::segment_saved(), path.display())),
				Ok(paths) => app.say(&format!(
					"{}{}{}{}",
					paths.len(),
					s::segments(),
					s::segments_saved(),
					dir.display()
				)),
				Err(e) => app.say(e),
			},
		);
	}

	/// Scheduled recordings run on their own background stream, so
	/// nothing was ever muted and nothing needs restoring at the end.
	pub(super) fn end_silent(&mut self) {}

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
							self.say(&format!("{}{}", s::schedule_saved(), path.display()))
						}
						Some(Err(e)) => self.say(&e.to_string()),
						None => {}
					}
				}
				self.end_silent();
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
				self.say(&format!("{}{}", s::alarm_started(), entry.station.name));
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
				self.say(&s::schedule_skipped());
				continue;
			}
			let name = entry.station.name.clone();
			// Start the recording on its own background stream from
			// the outset: playback stays wherever the listener is,
			// silent or not. A silent entry also mutes nothing —
			// there is nothing of it to hear.
			match self.tuner.start_background_recording(&entry.station) {
				Ok(_) => {
					if entry.silent {
						self.say(&format!("{}{name}{}", s::schedule_started_silent(), s::schedule_quiet_tail()));
					} else {
						self.say(&format!("{}{name}", s::schedule_started()));
					}
					self.sched_active = Some((entry.id.clone(), occ));
				}
				Err(e) => self.say(&e.to_string()),
			}
			break;
		}
	}
}
