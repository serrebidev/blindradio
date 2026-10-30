use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::CoreError;
use crate::settings::{RecordFormat, RecordSettings};
use crate::Result;

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

enum Kind {
	Passthrough(BufWriter<File>),
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
	},
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
		std::fs::create_dir_all(dir)?;
		let now = SystemTime::now()
			.duration_since(UNIX_EPOCH)
			.map(|d| d.as_secs())
			.unwrap_or(0);
		let stem = format!("{} {}", file_stem(station), stamp(now));
		let make = |ext: &str| dir.join(format!("{stem}.{ext}"));
		match settings.format {
			RecordFormat::Original => {
				let path = make(source_ext);
				let file = BufWriter::new(File::create(&path)?);
				Ok(Recorder {
					kind: Kind::Passthrough(file),
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
						pcm: Vec::new(),
						rate,
						base: path.clone(),
						part: 1,
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

	/// Encoded stream bytes; only `Original` recordings take them.
	pub fn write_bytes(&mut self, bytes: &[u8]) -> Result<()> {
		if self.paused {
			return Ok(());
		}
		if let Kind::Passthrough(file) = &mut self.kind {
			file.write_all(bytes)?;
		}
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
			Kind::Flac { pcm, rate, base, part } => {
				pcm.extend(to_i16(stereo));
				if pcm.len() as u64 >= *rate as u64 * 2 * FLAC_PART_SECONDS {
					write_flac_part(base, *part, pcm, *rate)?;
					pcm.clear();
					*part += 1;
				}
				Ok(())
			}
		}
	}

	pub fn stop(mut self) -> Result<PathBuf> {
		match &mut self.kind {
			Kind::Passthrough(file) => file.flush()?,
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
			Kind::Flac { pcm, rate, base, part } => {
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
		let mut reader = hound::WavReader::open(&path).unwrap();
		assert_eq!(reader.spec().sample_rate, 8000);
		assert_eq!(reader.duration(), 8000);
	}

	#[test]
	fn wav_recording_round_trips() {
		let d = tempfile::tempdir().unwrap();
		let mut r = Recorder::start(d.path(), "Test FM", &settings(RecordFormat::Wav), 8000, "mp3").unwrap();
		r.write_pcm(&vec![0.25; 16_000]).unwrap();
		let path = r.stop().unwrap();
		let mut reader = hound::WavReader::open(&path).unwrap();
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
	fn stamp_formats_utc() {
		assert_eq!(stamp(0), "1970-01-01 000000");
		assert_eq!(stamp(1_793_513_100), "2026-11-01 060500");
	}
}
