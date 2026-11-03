//! Automatic bookmarks in the time-shift buffer: every title a station
//! sends marks where a song or program segment starts, so playback can
//! jump between them and each can be written to its own file.
//!
//! Marks sit on the tuner's millisecond timeline: the decoded buffer's
//! frame time, or the encoded ring's session time. Both are the audio's
//! own axis, so a mark stays on its song whatever the wall clock did.

use std::collections::VecDeque;
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::Arc;

use crate::Result;
use crate::decode::Decoder;
use crate::encoded_ring::EncodedRing;
use crate::record::Recorder;
use crate::resample::Resampler;
use crate::ringfeed::skip_frames;
use crate::settings::RecordSettings;
use crate::timeshift::ShiftBuffer;

/// A first title this close to the buffer's start names the audio before
/// it too: tuning in lands mid-song and the title follows within seconds.
const FIRST_TITLE_MS: u64 = 15_000;
/// Previous Segment this far into a segment goes to its start, as a media
/// player's previous does; sooner, it goes to the one before.
const RESTART_MS: u64 = 3_000;
/// Decoded audio is copied to the recorder this much at a time.
const PIECE_SECONDS: u64 = 30;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Segment {
	pub title: String,
	pub start_ms: u64,
	pub end_ms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Which {
	Previous,
	Current,
	Next,
}

/// Where a segment jump lands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Jump {
	To(Segment),
	Live,
	Nowhere,
}

#[derive(Debug)]
struct Mark {
	ms: u64,
	title: String,
}

/// The titles heard, oldest first. Blank titles mark nothing, and a repeat
/// of the current title is the same segment.
#[derive(Debug, Default)]
pub struct Marks(VecDeque<Mark>);

impl Marks {
	pub fn add(&mut self, title: &str, now_ms: u64, start_ms: u64) {
		let title = title.trim();
		if title.is_empty() || self.0.back().is_some_and(|m| m.title == title) {
			return;
		}
		// Only the mark the buffer's oldest audio falls under is still needed.
		while self.0.get(1).is_some_and(|m| m.ms <= start_ms) {
			self.0.pop_front();
		}
		let first = self.0.is_empty() && now_ms.saturating_sub(start_ms) <= FIRST_TITLE_MS;
		self.0.push_back(Mark {
			ms: if first { start_ms } else { now_ms },
			title: title.to_owned(),
		});
	}

	pub fn title_at(&self, ms: u64) -> Option<&str> {
		self.0
			.iter()
			.rev()
			.find(|m| m.ms <= ms)
			.map(|m| m.title.as_str())
	}

	/// The segments still in the buffer. The newest runs to `now_ms`, and
	/// the oldest starts where the buffer does.
	pub fn segments(&self, start_ms: u64, now_ms: u64) -> Vec<Segment> {
		self.0
			.iter()
			.enumerate()
			.filter_map(|(i, m)| {
				let end_ms = self.0.get(i + 1).map_or(now_ms, |next| next.ms.min(now_ms));
				let begin = m.ms.max(start_ms);
				(begin < end_ms).then(|| Segment {
					title: m.title.clone(),
					start_ms: begin,
					end_ms,
				})
			})
			.collect()
	}
}

/// The segment holding `ms`; `None` before the first.
fn index_at(segments: &[Segment], ms: u64) -> Option<usize> {
	segments.iter().rposition(|s| s.start_ms <= ms)
}

/// The segment at, before or after the playhead.
pub fn pick(segments: &[Segment], playhead_ms: u64, which: Which) -> Option<&Segment> {
	let at = index_at(segments, playhead_ms);
	match which {
		Which::Current => segments.get(at?),
		Which::Previous => segments.get(at?.checked_sub(1)?),
		Which::Next => segments.get(at.map_or(0, |i| i + 1)),
	}
}

/// Next past the last segment returns to live; `shifted` says there is a
/// live edge to return to. Previous restarts the segment it is well into.
pub fn jump(segments: &[Segment], playhead_ms: u64, forward: bool, shifted: bool) -> Jump {
	let found = if forward {
		pick(segments, playhead_ms, Which::Next)
	} else {
		pick(segments, playhead_ms, Which::Current)
			.filter(|s| playhead_ms - s.start_ms > RESTART_MS)
			.or_else(|| pick(segments, playhead_ms, Which::Previous))
	};
	match found {
		Some(segment) => Jump::To(segment.clone()),
		None if forward && shifted => Jump::Live,
		None => Jump::Nowhere,
	}
}

/// The audio a writer reads: one of the tuner's two kinds of buffer.
pub(crate) enum Audio {
	Decoded(Arc<ShiftBuffer>),
	Ring(Arc<EncodedRing>),
}

/// Writes segments to their own files, away from the UI thread: encoding a
/// buffer's worth of them takes far longer than a key press may. It holds
/// the buffer itself, so tuning away while it runs costs nothing.
pub struct SegmentWriter {
	pub(crate) audio: Audio,
	pub(crate) dir: PathBuf,
	pub(crate) station: String,
	pub(crate) settings: RecordSettings,
	pub(crate) rate: u32,
	pub(crate) ext: String,
	/// The timeline and the wall clock at the same moment: names a file
	/// for when its segment played.
	pub(crate) now_ms: u64,
	pub(crate) now_epoch: u64,
}

impl SegmentWriter {
	/// One file per segment, in order. Stops at the first failure: the same
	/// disk would fail the next one too.
	pub fn write(&self, segments: &[Segment]) -> Result<Vec<PathBuf>> {
		segments.iter().map(|s| self.write_one(s)).collect()
	}

	/// The folder the files go to.
	pub fn dir(&self) -> &std::path::Path {
		&self.dir
	}

	fn write_one(&self, segment: &Segment) -> Result<PathBuf> {
		let ago = self.now_ms.saturating_sub(segment.start_ms) / 1000;
		let mut rec = Recorder::start_segment(
			&self.dir,
			&self.station,
			&segment.title,
			self.now_epoch.saturating_sub(ago),
			&self.settings,
			self.rate,
			&self.ext,
		)?;
		let encoded = match &self.audio {
			Audio::Decoded(buffer) => &buffer.encoded,
			Audio::Ring(ring) => ring.as_ref(),
		};
		if rec.is_original() {
			rec.write_bytes(&encoded.span(segment.start_ms, segment.end_ms))?;
		} else {
			match &self.audio {
				Audio::Decoded(buffer) => copy_decoded(buffer, segment, &mut rec)?,
				Audio::Ring(ring) => decode_span(ring, segment, self.rate, &mut rec)?,
			}
		}
		rec.stop()
	}
}

/// The segment's frames in pieces, so a long one never sits in memory
/// twice. The buffer keeps trimming meanwhile: audio already gone is skipped.
fn copy_decoded(buffer: &ShiftBuffer, segment: &Segment, rec: &mut Recorder) -> Result<()> {
	let rate = u64::from(buffer.rate());
	let mut at = segment.start_ms * rate / 1000;
	let end = segment.end_ms * rate / 1000;
	let mut piece = vec![0.0f32; (rate * PIECE_SECONDS * 2) as usize];
	while at < end {
		at = at.max(buffer.start());
		let want = ((end.saturating_sub(at) * 2) as usize).min(piece.len());
		let n = buffer.read_at(at, &mut piece[..want]);
		if n == 0 {
			break;
		}
		rec.write_pcm(&piece[..n])?;
		at += (n / 2) as u64;
	}
	Ok(())
}

/// Decodes the ring's bytes for the segment. Chunks start before the
/// target and end after it, so the extra audio is cut both ways.
fn decode_span(ring: &EncodedRing, segment: &Segment, rate: u32, rec: &mut Recorder) -> Result<()> {
	let to_rate = u64::from(rate);
	let mut skip = ring
		.chunk_at(segment.start_ms)
		.map_or(0, |(_, start)| (segment.start_ms - start) * to_rate / 1000);
	let mut left = (segment.end_ms - segment.start_ms) * to_rate / 1000;
	let bytes = ring.span(segment.start_ms, segment.end_ms);
	let mut decoder = Decoder::new(Box::new(Cursor::new(bytes)), None)?;
	let mut from_rate = decoder.sample_rate();
	let mut resampler = Resampler::new(from_rate, rate);
	let (mut pcm, mut out) = (Vec::new(), Vec::new());
	// An error here is the span's last chunk cut short: what came before stands.
	while left > 0 && matches!(decoder.next_stereo(&mut pcm), Ok(true)) {
		if decoder.sample_rate() != from_rate {
			from_rate = decoder.sample_rate();
			resampler = Resampler::new(from_rate, rate);
		}
		resampler.process(&pcm, &mut out);
		pcm.clear();
		skip_frames(&mut out, &mut skip);
		let frames = ((out.len() / 2) as u64).min(left);
		rec.write_pcm(&out[..frames as usize * 2])?;
		left -= frames;
		out.clear();
	}
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::settings::RecordFormat;

	fn marks(list: &[(&str, u64)]) -> Marks {
		let mut m = Marks::default();
		for (title, ms) in list {
			m.0.push_back(Mark {
				ms: *ms,
				title: (*title).into(),
			});
		}
		m
	}

	fn seg(title: &str, start_ms: u64, end_ms: u64) -> Segment {
		Segment {
			title: title.into(),
			start_ms,
			end_ms,
		}
	}

	#[test]
	fn titles_mark_once_and_blanks_mark_nothing() {
		let mut m = Marks::default();
		m.add("  ", 100_000, 0);
		m.add("One", 100_000, 0);
		m.add("One", 150_000, 0);
		m.add("", 160_000, 0);
		m.add("Two", 200_000, 0);
		m.add("One", 300_000, 0);
		assert_eq!(
			m.segments(0, 400_000),
			[
				seg("One", 100_000, 200_000),
				seg("Two", 200_000, 300_000),
				seg("One", 300_000, 400_000)
			]
		);
	}

	#[test]
	fn a_first_title_soon_after_tuning_in_names_the_audio_before_it() {
		let mut m = Marks::default();
		m.add("Soon", 5_000, 1_000);
		assert_eq!(m.segments(1_000, 9_000), [seg("Soon", 1_000, 9_000)]);
		let mut m = Marks::default();
		m.add("Late", 60_000, 0);
		assert_eq!(m.segments(0, 90_000), [seg("Late", 60_000, 90_000)]);
		assert_eq!(pick(&m.segments(0, 90_000), 10_000, Which::Current), None);
	}

	#[test]
	fn the_oldest_segment_starts_where_the_buffer_does_and_old_marks_go() {
		let mut m = marks(&[("A", 0), ("B", 10_000), ("C", 20_000)]);
		assert_eq!(
			m.segments(15_000, 30_000),
			[seg("B", 15_000, 20_000), seg("C", 20_000, 30_000)]
		);
		assert_eq!(m.segments(20_000, 30_000), [seg("C", 20_000, 30_000)]);
		m.add("D", 40_000, 25_000);
		assert_eq!(m.0.front().unwrap().title, "C", "A and B left the buffer");
		assert_eq!(m.title_at(25_000), Some("C"));
	}

	#[test]
	fn title_at_names_the_song_of_the_moment() {
		let m = marks(&[("One", 100), ("Two", 200)]);
		assert_eq!(m.title_at(99), None);
		assert_eq!(m.title_at(100), Some("One"));
		assert_eq!(m.title_at(199), Some("One"));
		assert_eq!(m.title_at(200), Some("Two"));
		assert_eq!(m.title_at(999), Some("Two"));
	}

	#[test]
	fn pick_is_relative_to_the_playhead() {
		let s = [seg("A", 0, 10), seg("B", 10, 20), seg("C", 20, 30)];
		assert_eq!(pick(&s, 15, Which::Current).unwrap().title, "B");
		assert_eq!(pick(&s, 15, Which::Previous).unwrap().title, "A");
		assert_eq!(pick(&s, 15, Which::Next).unwrap().title, "C");
		assert_eq!(pick(&s, 5, Which::Previous), None);
		assert_eq!(pick(&s, 25, Which::Next), None);
		let late = [seg("A", 100, 200)];
		assert_eq!(pick(&late, 50, Which::Next).unwrap().title, "A", "before the first");
		assert_eq!(pick(&late, 50, Which::Previous), None);
	}

	#[test]
	fn jumps_restart_step_and_return_to_live() {
		let s = [seg("A", 0, 10_000), seg("B", 10_000, 20_000)];
		assert_eq!(jump(&s, 15_000, false, true), Jump::To(s[1].clone()), "restart");
		assert_eq!(jump(&s, 11_000, false, true), Jump::To(s[0].clone()), "just began");
		assert_eq!(jump(&s, 1_000, false, true), Jump::Nowhere, "first, just began");
		assert_eq!(jump(&s, 5_000, true, true), Jump::To(s[1].clone()));
		assert_eq!(jump(&s, 15_000, true, true), Jump::Live, "shifted past the last");
		assert_eq!(jump(&s, 20_000, true, false), Jump::Nowhere, "already live");
		assert_eq!(jump(&[], 0, true, true), Jump::Live);
		assert_eq!(jump(&[], 0, false, true), Jump::Nowhere);
	}

	fn writer(audio: Audio, dir: &std::path::Path, format: RecordFormat, rate: u32) -> SegmentWriter {
		SegmentWriter {
			audio,
			dir: dir.to_path_buf(),
			station: "Test FM".into(),
			settings: RecordSettings {
				format,
				..RecordSettings::default()
			},
			rate,
			ext: "mp3".into(),
			now_ms: 100_000,
			now_epoch: 1_793_513_100,
		}
	}

	/// Frame `i` holds the sample `i`, so a file shows which audio it got.
	fn ramp(frames: usize) -> Vec<f32> {
		(0..frames)
			.flat_map(|i| [i as f32 / 32767.0, i as f32 / 32767.0])
			.collect()
	}

	fn first_and_len(path: &std::path::Path) -> (i16, u32) {
		let mut reader = hound::WavReader::open(path).unwrap();
		let frames = reader.duration();
		(reader.samples::<i16>().next().unwrap().unwrap(), frames)
	}

	#[test]
	fn each_segment_gets_its_own_file_with_its_own_audio() {
		let d = tempfile::tempdir().unwrap();
		let buffer = Arc::new(ShiftBuffer::with_seconds(1000, 60));
		buffer.push(&ramp(5000));
		let w = writer(Audio::Decoded(buffer), d.path(), RecordFormat::Wav, 1000);
		let paths = w
			.write(&[seg("A - One", 0, 2000), seg("B / Two", 2000, 5000)])
			.unwrap();
		assert_eq!(paths.len(), 2);
		let name = paths[1].file_name().unwrap().to_str().unwrap();
		assert!(name.starts_with("Test FM - B - Two "), "{name}");
		// Written 98 s after the segment began, by the wall clock in the name.
		assert!(name.ends_with("2026-11-01 060322.wav"), "{name}");
		let (first, frames) = first_and_len(&paths[0]);
		assert_eq!((first.abs_diff(0) <= 1, frames), (true, 2000));
		let (first, frames) = first_and_len(&paths[1]);
		assert_eq!((first.abs_diff(2000) <= 1, frames), (true, 3000));
	}

	#[test]
	fn audio_trimmed_while_writing_is_skipped_not_replaced() {
		let d = tempfile::tempdir().unwrap();
		let buffer = Arc::new(ShiftBuffer::with_seconds(1000, 2));
		buffer.push(&ramp(5000)); // only frames 3000..5000 remain
		let w = writer(Audio::Decoded(buffer), d.path(), RecordFormat::Wav, 1000);
		let paths = w.write(&[seg("Old", 1000, 4000)]).unwrap();
		let (first, frames) = first_and_len(&paths[0]);
		assert_eq!((first.abs_diff(3000) <= 1, frames), (true, 1000));
	}

	#[test]
	fn writing_a_segment_again_never_overwrites_the_last_file() {
		let d = tempfile::tempdir().unwrap();
		let buffer = Arc::new(ShiftBuffer::with_seconds(1000, 60));
		buffer.push(&ramp(3000));
		let w = writer(Audio::Decoded(buffer), d.path(), RecordFormat::Wav, 1000);
		let first = w.write(&[seg("Song", 0, 3000)]).unwrap();
		let second = w.write(&[seg("Song", 0, 1000)]).unwrap();
		assert_ne!(first, second);
		assert_eq!(first_and_len(&first[0]).1, 3000);
		assert_eq!(first_and_len(&second[0]).1, 1000);
	}

	#[test]
	fn original_format_keeps_the_stream_bytes() {
		let d = tempfile::tempdir().unwrap();
		let ring = Arc::new(EncodedRing::new(1000));
		for (i, byte) in [1u8, 2, 3].into_iter().enumerate() {
			ring.push(i as u64 * 1000, &[byte]);
		}
		let w = writer(Audio::Ring(ring), d.path(), RecordFormat::Original, 1000);
		let paths = w.write(&[seg("Song", 1000, 2000)]).unwrap();
		assert_eq!(std::fs::read(&paths[0]).unwrap(), [2]);
	}

	#[test]
	fn ring_segments_decode_to_the_recording_format() {
		let tone = std::fs::read(concat!(
			env!("CARGO_MANIFEST_DIR"),
			"/tests/fixtures/tone.mp3"
		))
		.unwrap();
		let ring = Arc::new(EncodedRing::new(usize::MAX));
		ring.push(0, &tone);
		let d = tempfile::tempdir().unwrap();
		let w = writer(Audio::Ring(ring), d.path(), RecordFormat::Wav, 48_000);
		let paths = w.write(&[seg("Song", 200, 700)]).unwrap();
		let frames = hound::WavReader::open(&paths[0]).unwrap().duration();
		assert_eq!(frames, 24_000, "half a second, cut both ways");
	}
}
