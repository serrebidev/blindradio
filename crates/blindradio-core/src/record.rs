use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::Result;
use crate::error::CoreError;
use crate::settings::{RecordFormat, RecordSettings};

/// Seconds of audio per FLAC file. flacenc encodes a whole buffer in one
/// pass, so FLAC recordings are written in parts to bound memory; the other
/// formats stream.
// ponytail: 15-minute FLAC parts (~170 MB peak); a streaming FLAC encoder is the upgrade path.
const FLAC_PART_SECONDS: u64 = 900;

/// Where recordings go when the user has not chosen a folder.
pub fn default_dir() -> PathBuf {
	dirs::audio_dir()
		.or_else(dirs::home_dir)
		.unwrap_or_else(|| PathBuf::from("."))
		.join("BlindRadio")
}

/// Records the playing station to a file. PCM formats tap the decoded audio;
/// `Original` taps the stream's own encoded bytes before decoding.
pub struct Recorder {
	kind: Kind,
	path: PathBuf,
	/// While paused, writes are dropped: the file keeps only the takes
	/// the user let through, so ads can be skipped inside one recording.
	paused: bool,
}

/// An `Original` recording's file. Bytes arrive on network chunk
/// boundaries, which land mid-frame: everything before the first whole
/// frame is held back and dropped, so the file starts where a player
/// can start. A stream whose frames never sync (a container caught
/// before its format was known) is written whole past the cap.
struct Passthrough {
	file: BufWriter<File>,
	started: bool,
	head: Vec<u8>,
}

/// Before this many unsynced bytes, a part-frame prefix is still
/// plausible; past it, the bytes are not MPEG audio or ADTS at all.
const HEAD_CAP: usize = 8192;

/// Offset of the first MPEG-audio or ADTS frame sync: both begin with
/// eleven set bits.
fn sync_offset(bytes: &[u8]) -> Option<usize> {
	bytes
		.windows(2)
		.position(|w| w[0] == 0xFF && w[1] & 0xE0 == 0xE0)
}

impl Passthrough {
	fn write(&mut self, bytes: &[u8]) -> Result<()> {
		if self.started {
			self.file.write_all(bytes)?;
			return Ok(());
		}
		self.head.extend_from_slice(bytes);
		match sync_offset(&self.head) {
			Some(at) => {
				self.head.drain(..at);
				let head = std::mem::take(&mut self.head);
				self.file.write_all(&head)?;
				self.started = true;
			}
			None if self.head.len() > HEAD_CAP => {
				let head = std::mem::take(&mut self.head);
				self.file.write_all(&head)?;
				self.started = true;
			}
			None => {}
		}
		Ok(())
	}

	fn finish(&mut self) -> Result<()> {
		if !self.head.is_empty() {
			let head = std::mem::take(&mut self.head);
			self.file.write_all(&head)?;
		}
		self.file.flush()?;
		Ok(())
	}
}

enum Kind {
	Passthrough(Passthrough),
	/// Option so `stop` can take the writer and finalize it.
	Wav(Option<hound::WavWriter<BufWriter<File>>>),
	Mp3 {
		enc: mp3lame_encoder::Encoder,
		file: BufWriter<File>,
		out: Vec<u8>,
	},
	Flac {
		pcm: Vec<i16>,
		rate: u32,
		base: PathBuf,
		part: u32,
		/// The finished part being encoded off this thread: PCM is
		/// written from the audio callback, and a part takes seconds.
		writing: Option<std::thread::JoinHandle<Result<()>>>,
	},
}

/// Samples in one FLAC part, reserved up front so the audio
/// callback never copies a growing buffer.
fn flac_part_samples(rate: u32) -> usize {
	rate as usize * 2 * FLAC_PART_SECONDS as usize
}

fn to_i16(stereo: &[f32]) -> Vec<i16> {
	stereo
		.iter()
		.map(|s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
		.collect()
}

/// `2026-10-29 024500` in UTC, from epoch seconds (civil-from-days).
fn stamp(epoch: u64) -> String {
	let (days, rem) = ((epoch / 86_400) as i64, epoch % 86_400);
	let z = days + 719_468;
	let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
	let doe = z - era * 146_097;
	let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
	let y = yoe + era * 400;
	let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
	let mp = (5 * doy + 2) / 153;
	let d = doy - (153 * mp + 2) / 5 + 1;
	let m = if mp < 10 { mp + 3 } else { mp - 9 };
	let year = if m <= 2 { y + 1 } else { y };
	format!(
		"{year:04}-{m:02}-{d:02} {:02}{:02}{:02}",
		rem / 3600,
		rem % 3600 / 60,
		rem % 60
	)
}

fn file_stem(station: &str) -> String {
	let name: String = station
		.chars()
		.map(|c| {
			if c.is_alphanumeric() || c == ' ' || c == '-' {
				c
			} else {
				'-'
			}
		})
		.take(60)
		.collect();
	let name = name.trim().to_string();
	if name.is_empty() { "station".into() } else { name }
}

fn nearest_bitrate(kbps: u32) -> mp3lame_encoder::Bitrate {
	use mp3lame_encoder::Bitrate::*;
	let table: [(u32, mp3lame_encoder::Bitrate); 16] = [
		(8, Kbps8),
		(16, Kbps16),
		(24, Kbps24),
		(32, Kbps32),
		(40, Kbps40),
		(48, Kbps48),
		(64, Kbps64),
		(80, Kbps80),
		(96, Kbps96),
		(112, Kbps112),
		(128, Kbps128),
		(160, Kbps160),
		(192, Kbps192),
		(224, Kbps224),
		(256, Kbps256),
		(320, Kbps320),
	];
	table
		.into_iter()
		.min_by_key(|item| item.0.abs_diff(kbps))
		.map(|(_, b)| b)
		.unwrap_or(Kbps192)
}

/// A WAV file being written chunk by chunk (Save Buffer streams the
/// time-shift buffer through in pieces instead of holding it all).
pub struct WavSave {
	w: hound::WavWriter<BufWriter<File>>,
	path: PathBuf,
}

impl WavSave {
	/// Starts a WAV in the recordings folder, named like a recording.
	pub fn create(dir: &Path, station: &str, rate: u32) -> Result<WavSave> {
		std::fs::create_dir_all(dir)?;
		let now = SystemTime::now()
			.duration_since(UNIX_EPOCH)
			.map(|d| d.as_secs())
			.unwrap_or(0);
		let path = dir.join(format!("{} {}.wav", file_stem(station), stamp(now)));
		let spec = hound::WavSpec {
			channels: 2,
			sample_rate: rate,
			bits_per_sample: 16,
			sample_format: hound::SampleFormat::Int,
		};
		let w = hound::WavWriter::create(&path, spec)
			.map_err(|e| CoreError::Output(e.to_string()))?;
		Ok(WavSave { w, path })
	}

	pub fn write(&mut self, pcm: &[f32]) -> Result<()> {
		for s in to_i16(pcm) {
			self.w
				.write_sample(s)
				.map_err(|e| CoreError::Output(e.to_string()))?;
		}
		Ok(())
	}

	pub fn finish(self) -> Result<PathBuf> {
		self.w
			.finalize()
			.map_err(|e| CoreError::Output(e.to_string()))?;
		Ok(self.path)
	}
}

/// Writes decoded stereo PCM to a WAV file named like a recording
/// (Save Buffer: the time-shift buffer is decoded audio whatever the
/// stream's own format was). Returns the file's path.
pub fn save_wav(dir: &Path, station: &str, rate: u32, pcm: &[f32]) -> Result<PathBuf> {
	let mut save = WavSave::create(dir, station, rate)?;
	save.write(pcm)?;
	save.finish()
}

impl Recorder {
	/// `source_ext` names the stream's own format for `Original` recordings
	/// ("mp3", "aac", ...). `rate` is the decoded output rate.
	pub fn start(
		dir: &Path,
		station: &str,
		settings: &RecordSettings,
		rate: u32,
		source_ext: &str,
	) -> Result<Recorder> {
		let now = SystemTime::now()
			.duration_since(UNIX_EPOCH)
			.map(|d| d.as_secs())
			.unwrap_or(0);
		let stem = format!("{} {}", file_stem(station), stamp(now));
		Self::start_stem(dir, &stem, settings, rate, source_ext)
	}

	/// A song-split recording's file: named for the station and the
	/// song, so a folder of them reads as the songs themselves.
	pub fn start_song(
		dir: &Path,
		station: &str,
		song: &str,
		settings: &RecordSettings,
		rate: u32,
		source_ext: &str,
	) -> Result<Recorder> {
		let now = SystemTime::now()
			.duration_since(UNIX_EPOCH)
			.map(|d| d.as_secs())
			.unwrap_or(0);
		let stem = format!(
			"{} - {} {}",
			file_stem(station),
			file_stem(song),
			stamp(now)
		);
		Self::start_stem(dir, &stem, settings, rate, source_ext)
	}

	/// A file for one buffer segment: named for the station, the title and
	/// when it played. Numbered rather than overwriting, because the buffer
	/// may hold less of the segment than an earlier write of it saved.
	#[allow(clippy::too_many_arguments)]
	pub fn start_segment(
		dir: &Path,
		station: &str,
		title: &str,
		played_epoch: u64,
		settings: &RecordSettings,
		rate: u32,
		source_ext: &str,
	) -> Result<Recorder> {
		let base = format!(
			"{} - {} {}",
			file_stem(station),
			file_stem(title),
			stamp(played_epoch)
		);
		let ext = match settings.format {
			RecordFormat::Original => source_ext,
			RecordFormat::Wav => "wav",
			RecordFormat::Mp3 => "mp3",
			RecordFormat::Flac => "flac",
		};
		let mut stem = base.clone();
		let mut n = 1;
		while dir.join(format!("{stem}.{ext}")).exists() {
			n += 1;
			stem = format!("{base} {n}");
		}
		Self::start_stem(dir, &stem, settings, rate, source_ext)
	}

	fn start_stem(
		dir: &Path,
		stem: &str,
		settings: &RecordSettings,
		rate: u32,
		source_ext: &str,
	) -> Result<Recorder> {
		std::fs::create_dir_all(dir)?;
		let make = |ext: &str| dir.join(format!("{stem}.{ext}"));
		match settings.format {
			RecordFormat::Original => {
				let path = make(source_ext);
				let file = BufWriter::new(File::create(&path)?);
				Ok(Recorder {
					kind: Kind::Passthrough(Passthrough {
						file,
						started: false,
						head: Vec::new(),
					}),
					path,
					paused: false,
				})
			}
			RecordFormat::Wav => {
				let path = make("wav");
				let spec = hound::WavSpec {
					channels: 2,
					sample_rate: rate,
					bits_per_sample: 16,
					sample_format: hound::SampleFormat::Int,
				};
				let writer = hound::WavWriter::create(&path, spec)
					.map_err(|e| CoreError::Output(e.to_string()))?;
				Ok(Recorder {
					kind: Kind::Wav(Some(writer)),
					path,
					paused: false,
				})
			}
			RecordFormat::Mp3 => {
				let path = make("mp3");
				let mut b = mp3lame_encoder::Builder::new()
					.ok_or_else(|| CoreError::Output("lame init failed".into()))?;
				b.set_num_channels(2)
					.and_then(|_| b.set_sample_rate(rate))
					.map_err(|e| CoreError::Output(e.to_string()))?;
				if settings.mp3_vbr {
					b.set_vbr_mode(mp3lame_encoder::VbrMode::Mtrh)
						.and_then(|_| b.set_vbr_quality(mp3lame_encoder::Quality::NearBest))
						.map_err(|e| CoreError::Output(e.to_string()))?;
				} else {
					b.set_brate(nearest_bitrate(settings.mp3_bitrate_kbps))
						.map_err(|e| CoreError::Output(e.to_string()))?;
				}
				let enc = b.build().map_err(|e| CoreError::Output(e.to_string()))?;
				Ok(Recorder {
					kind: Kind::Mp3 {
						enc,
						file: BufWriter::new(File::create(&path)?),
						out: Vec::new(),
					},
					path,
					paused: false,
				})
			}
			RecordFormat::Flac => {
				let path = make("flac");
				Ok(Recorder {
					kind: Kind::Flac {
						pcm: Vec::with_capacity(flac_part_samples(rate)),
						rate,
						base: path.clone(),
						part: 1,
						writing: None,
					},
					path,
					paused: false,
				})
			}
		}
	}

	pub fn path(&self) -> &Path {
		&self.path
	}

	pub fn set_paused(&mut self, paused: bool) {
		self.paused = paused;
	}

	pub fn is_paused(&self) -> bool {
		self.paused
	}

	pub(crate) fn is_original(&self) -> bool {
		matches!(self.kind, Kind::Passthrough(_))
	}

	/// Encoded stream bytes; only `Original` recordings take them.
	pub fn write_bytes(&mut self, bytes: &[u8]) -> Result<()> {
		if self.paused {
			return Ok(());
		}
		if let Kind::Passthrough(pass) = &mut self.kind {
			pass.write(bytes)?;
		}
		Ok(())
	}

	/// Names an `Original` recording for the stream's real format,
	/// once decoding has reported it: a recording started before the
	/// first packet (a scheduled one, above all) was named from the
	/// station's address, which may say nothing — an AAC stream at an
	/// extensionless address was being saved as `.mp3`, and players
	/// believe the name. Renames the file, never copies it.
	pub(crate) fn align_source_ext(&mut self, ext: &str) -> Result<()> {
		let Kind::Passthrough(pass) = &mut self.kind else {
			return Ok(());
		};
		if self.path.extension().is_some_and(|e| e == ext) {
			return Ok(());
		}
		pass.finish()?;
		let old = std::mem::take(&mut self.path);
		// Consuming the writer closes the file, which the rename needs.
		let pass_file = std::mem::replace(
			&mut pass.file,
			BufWriter::new(std::fs::OpenOptions::new().append(true).open(&old)?),
		);
		drop(pass_file.into_inner().map_err(|e| e.into_error())?);
		let mut path = old.with_extension(ext);
		let mut n = 1;
		while path.exists() {
			n += 1;
			let stem = old.file_stem().unwrap_or_default().to_string_lossy();
			path = old.with_file_name(format!("{stem} {n}.{ext}"));
		}
		if let Err(e) = std::fs::rename(&old, &path) {
			// Keep recording under the old name rather than lose it.
			pass.file = BufWriter::new(
				std::fs::OpenOptions::new().append(true).open(&old)?,
			);
			self.path = old;
			return Err(e.into());
		}
		pass.file = BufWriter::new(
			std::fs::OpenOptions::new().append(true).open(&path)?,
		);
		self.path = path;
		Ok(())
	}

	/// Decoded stereo f32 frames; only the PCM formats take them.
	pub fn write_pcm(&mut self, stereo: &[f32]) -> Result<()> {
		if self.paused {
			return Ok(());
		}
		match &mut self.kind {
			Kind::Passthrough(_) => Ok(()),
			Kind::Wav(w) => {
				let w = w.as_mut().expect("wav writer present until stop");
				for s in to_i16(stereo) {
					w.write_sample(s).map_err(|e| CoreError::Output(e.to_string()))?;
				}
				Ok(())
			}
			Kind::Mp3 { enc, file, out } => {
				let pcm = to_i16(stereo);
				out.clear();
				// LAME writes into the vec's spare capacity; it needs room up front.
				out.reserve(pcm.len() * 2 + 7200);
				enc.encode_to_vec(mp3lame_encoder::InterleavedPcm(&pcm), out)
					.map_err(|e| CoreError::Output(e.to_string()))?;
				file.write_all(out)?;
				Ok(())
			}
			Kind::Flac { pcm, rate, base, part, writing } => {
				pcm.extend(to_i16(stereo));
				if pcm.len() >= flac_part_samples(*rate) {
					// A part still encoding after 15 min: wait for it.
					if let Some(prev) = writing.take() {
						join_part(prev)?;
					}
					let done = std::mem::replace(pcm, Vec::with_capacity(flac_part_samples(*rate)));
					let (path, rate) = (part_path(base, *part), *rate);
					*writing = Some(std::thread::spawn(move || write_flac(&path, &done, rate)));
					*part += 1;
				}
				Ok(())
			}
		}
	}

	pub fn stop(mut self) -> Result<PathBuf> {
		match &mut self.kind {
			Kind::Passthrough(pass) => pass.finish()?,
			Kind::Wav(w) => {
				if let Some(w) = w.take() {
					w.finalize().map_err(|e| CoreError::Output(e.to_string()))?;
				}
			}
			Kind::Mp3 { enc, file, out } => {
				out.clear();
				out.reserve(7200);
				enc.flush_to_vec::<mp3lame_encoder::FlushNoGap>(out)
					.map_err(|e| CoreError::Output(e.to_string()))?;
				file.write_all(out)?;
				file.flush()?;
			}
			Kind::Flac { pcm, rate, base, part, writing } => {
				if let Some(prev) = writing.take() {
					join_part(prev)?;
				}
				if !pcm.is_empty() {
					write_flac_part(base, *part, pcm, *rate)?;
				}
			}
		}
		Ok(self.path)
	}
}

fn part_path(base: &Path, part: u32) -> PathBuf {
	if part == 1 {
		return base.to_path_buf();
	}
	let stem = base.file_stem().unwrap_or_default().to_string_lossy();
	base.with_file_name(format!("{stem} part{part}.flac"))
}

fn join_part(handle: std::thread::JoinHandle<Result<()>>) -> Result<()> {
	handle
		.join()
		.unwrap_or_else(|_| Err(CoreError::Output("flac part failed".into())))
}

fn write_flac_part(base: &Path, part: u32, pcm: &[i16], rate: u32) -> Result<()> {
	write_flac(&part_path(base, part), pcm, rate)
}

fn write_flac(path: &Path, pcm: &[i16], rate: u32) -> Result<()> {
	use flacenc::component::BitRepr;
	use flacenc::error::Verify;
	let samples: Vec<i32> = pcm.iter().map(|s| *s as i32).collect();
	let source = flacenc::source::MemSource::from_samples(&samples, 2, 16, rate as usize);
	let config = flacenc::config::Encoder::default()
		.into_verified()
		.map_err(|_| CoreError::Output("flac config rejected".into()))?;
	let stream = flacenc::encode_with_fixed_block_size(&config, source, 4096)
		.map_err(|e| CoreError::Output(e.to_string()))?;
	let mut sink = flacenc::bitsink::ByteSink::new();
	stream
		.write(&mut sink)
		.map_err(|e| CoreError::Output(e.to_string()))?;
	std::fs::write(path, sink.as_slice())?;
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;

	fn settings(format: RecordFormat) -> RecordSettings {
		RecordSettings {
			format,
			..RecordSettings::default()
		}
	}

	#[test]
	fn save_wav_writes_a_named_recording() {
		let d = tempfile::tempdir().unwrap();
		let path = save_wav(d.path(), "Test FM", 8000, &vec![0.25; 16_000]).unwrap();
		assert!(path.file_name().unwrap().to_str().unwrap().ends_with(".wav"));
		let reader = hound::WavReader::open(&path).unwrap();
		assert_eq!(reader.spec().sample_rate, 8000);
		assert_eq!(reader.duration(), 8000);
	}

	#[test]
	fn song_recording_is_named_for_the_song() {
		let d = tempfile::tempdir().unwrap();
		let r = Recorder::start_song(
			d.path(),
			"Test FM",
			"Sylver - Take Me Back",
			&settings(RecordFormat::Wav),
			8000,
			"mp3",
		)
		.unwrap();
		let name = r.path().file_name().unwrap().to_str().unwrap().to_string();
		assert!(
			name.starts_with("Test FM - Sylver - Take Me Back "),
			"{name}"
		);
		assert!(name.ends_with(".wav"), "{name}");
		r.stop().unwrap();
	}

	#[test]
	fn wav_recording_round_trips() {
		let d = tempfile::tempdir().unwrap();
		let mut r = Recorder::start(d.path(), "Test FM", &settings(RecordFormat::Wav), 8000, "mp3").unwrap();
		r.write_pcm(&vec![0.25; 16_000]).unwrap();
		let path = r.stop().unwrap();
		let reader = hound::WavReader::open(&path).unwrap();
		assert_eq!(reader.spec().sample_rate, 8000);
		assert_eq!(reader.duration(), 8000);
	}

	#[test]
	fn mp3_recording_has_frames() {
		let d = tempfile::tempdir().unwrap();
		let mut r = Recorder::start(d.path(), "Test FM", &settings(RecordFormat::Mp3), 44100, "mp3").unwrap();
		for _ in 0..50 {
			r.write_pcm(&vec![0.2; 8820]).unwrap();
		}
		let path = r.stop().unwrap();
		let bytes = std::fs::read(&path).unwrap();
		assert!(bytes.len() > 1000, "mp3 bytes: {}", bytes.len());
	}

	#[test]
	fn flac_recording_is_a_flac() {
		let d = tempfile::tempdir().unwrap();
		let mut r = Recorder::start(d.path(), "Test FM", &settings(RecordFormat::Flac), 8000, "mp3").unwrap();
		r.write_pcm(&vec![0.1; 16_000]).unwrap();
		let path = r.stop().unwrap();
		let bytes = std::fs::read(&path).unwrap();
		assert_eq!(&bytes[..4], b"fLaC");
	}

	#[test]
	fn passthrough_copies_bytes() {
		let d = tempfile::tempdir().unwrap();
		let mut r = Recorder::start(d.path(), "Test FM", &settings(RecordFormat::Original), 44100, "aac").unwrap();
		assert!(r.path().to_string_lossy().ends_with(".aac"));
		r.write_bytes(b"hello").unwrap();
		r.write_pcm(&[0.0; 4]).unwrap(); // ignored by passthrough
		let path = r.stop().unwrap();
		assert_eq!(std::fs::read(&path).unwrap(), b"hello");
	}

	#[test]
	fn passthrough_starts_at_the_first_whole_frame() {
		let d = tempfile::tempdir().unwrap();
		let mut r = Recorder::start(d.path(), "Test FM", &settings(RecordFormat::Original), 44100, "aac").unwrap();
		// A network chunk boundary lands mid-frame: the part-frame goes.
		r.write_bytes(&[0xB1, 0x28, 0x5B]).unwrap();
		r.write_bytes(&[0xFF, 0xF1, 0x50, 0x80]).unwrap();
		let path = r.stop().unwrap();
		assert_eq!(std::fs::read(&path).unwrap(), [0xFF, 0xF1, 0x50, 0x80]);
	}

	#[test]
	fn passthrough_takes_the_real_format_name_when_it_arrives_late() {
		let d = tempfile::tempdir().unwrap();
		let mut r = Recorder::start(d.path(), "Test FM", &settings(RecordFormat::Original), 44100, "mp3").unwrap();
		let wrong = r.path().to_path_buf();
		r.write_bytes(&[0xFF, 0xF1, 0x50, 0x80]).unwrap();
		r.align_source_ext("aac").unwrap();
		let path = r.stop().unwrap();
		assert!(path.to_string_lossy().ends_with(".aac"), "{path:?}");
		assert!(!wrong.exists(), "the misnamed file is renamed, not copied");
		assert_eq!(std::fs::read(&path).unwrap(), [0xFF, 0xF1, 0x50, 0x80]);
	}

	#[test]
	fn stamp_formats_utc() {
		assert_eq!(stamp(0), "1970-01-01 000000");
		assert_eq!(stamp(1_793_513_100), "2026-11-01 060500");
	}
}
