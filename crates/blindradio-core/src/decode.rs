use std::io::Read;

use bytes::{Buf, Bytes};
use crossbeam_channel::{Receiver, Sender};
use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::codecs::registry::CodecRegistry;
use symphonia::core::common::Tier;
use symphonia::core::errors::Error;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, TrackType};
use symphonia::core::io::{MediaSourceStream, ReadOnlySource};
use symphonia::core::meta::MetadataOptions;

use crate::{CoreError, Result};

/// Back-to-back unsupported packets tolerated after first audio before the stream counts as unsupported.
const MAX_UNSUPPORTED_RUN: u32 = 4096;

/// Blocking `Read` over chunks sent from the async network side.
pub struct ByteReader {
	rx: Receiver<Bytes>,
	chunk: Bytes,
}

pub fn byte_pipe() -> (Sender<Bytes>, ByteReader) {
	let (tx, rx) = crossbeam_channel::bounded(64);
	(
		tx,
		ByteReader {
			rx,
			chunk: Bytes::new(),
		},
	)
}

impl Read for ByteReader {
	fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
		while self.chunk.is_empty() {
			match self.rx.recv() {
				Ok(chunk) => self.chunk = chunk,
				Err(_) => return Ok(0),
			}
		}
		let n = buf.len().min(self.chunk.len());
		self.chunk.copy_to_slice(&mut buf[..n]);
		Ok(n)
	}
}

fn codecs() -> CodecRegistry {
	let mut codecs = CodecRegistry::new();
	symphonia::default::register_enabled_codecs(&mut codecs);
	// FDK handles HE-AAC (SBR/PS), which symphonia's native AAC decoder does not.
	codecs.register_audio_decoder_at_tier::<symphonia_adapter_fdk_aac::AacDecoder>(Tier::Preferred);
	codecs.register_audio_decoder::<symphonia_adapter_libopus::OpusDecoder>();
	codecs
}

fn no_data() -> CoreError {
	CoreError::Decode("No data".into())
}

fn decode_err(e: Error) -> CoreError {
	CoreError::Decode(e.to_string())
}

/// What the playing stream is, for the status bar: codec, the bitrate the
/// server declares (ICY `icy-br`; often absent), sample rate and channels.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StreamFormat {
	pub codec: String,
	pub bitrate_kbps: Option<u32>,
	pub sample_rate: u32,
	pub channels: u32,
}

impl StreamFormat {
	/// "MP3, 128 kbps, 44.1 kHz, stereo" — parts drop out when unknown.
	pub fn describe(&self) -> String {
		let mut out = self.codec.clone();
		if let Some(k) = self.bitrate_kbps {
			out += &format!(", {k} kbps");
		}
		if self.sample_rate > 0 {
			out += &format!(", {} kHz", trim_rate(self.sample_rate));
		}
		match self.channels {
			1 => out += ", mono",
			2 => out += ", stereo",
			n if n > 2 => out += &format!(", {n} channels"),
			_ => {}
		}
		out
	}
}

fn trim_rate(rate: u32) -> String {
	let s = format!("{:.1}", rate as f64 / 1000.0);
	s.strip_suffix(".0").map(str::to_string).unwrap_or(s)
}

/// The status-bar label for a codec id; unknown ids read as "Audio".
fn codec_label(id: symphonia::core::codecs::audio::AudioCodecId) -> &'static str {
	use symphonia::core::codecs::audio::well_known::*;
	match id {
		CODEC_ID_MP3 => "MP3",
		CODEC_ID_AAC => "AAC",
		CODEC_ID_VORBIS => "Vorbis",
		CODEC_ID_OPUS => "Opus",
		CODEC_ID_FLAC => "FLAC",
		CODEC_ID_ALAC => "ALAC",
		_ => "Audio",
	}
}

pub struct Decoder {
	format: Box<dyn FormatReader>,
	codecs: CodecRegistry,
	decoder: Box<dyn AudioDecoder>,
	track_id: u32,
	rate: u32,
	codec: &'static str,
	channels: u32,
	scratch: Vec<f32>,
	first: Vec<f32>,
}

impl Decoder {
	pub fn new(mut reader: Box<dyn Read + Send + Sync>, ext_hint: Option<&str>) -> Result<Decoder> {
		// One byte read up front tells an empty stream apart from an unrecognised one.
		let mut head = [0u8];
		if reader.read(&mut head)? == 0 {
			return Err(no_data());
		}
		let reader = Read::chain(std::io::Cursor::new(head), reader);
		let mss = MediaSourceStream::new(Box::new(ReadOnlySource::new(reader)), Default::default());
		let mut hint = Hint::new();
		if let Some(ext) = ext_hint {
			hint.with_extension(ext);
		}
		let format = symphonia::default::get_probe()
			.probe(
				&hint,
				mss,
				FormatOptions::default(),
				MetadataOptions::default(),
			)
			.map_err(|_| CoreError::Decode("Unsupported format".into()))?;
		let codecs = codecs();
		let (decoder, track_id, codec) = open_track(format.as_ref(), &codecs)?;
		let mut dec = Decoder {
			format,
			codecs,
			decoder,
			track_id,
			rate: 0,
			codec,
			channels: 0,
			scratch: Vec::new(),
			first: Vec::new(),
		};
		// Decode one packet now so sample_rate() is right before the caller sizes its resampler.
		let mut first = Vec::new();
		if !dec.next_stereo(&mut first)? {
			return Err(no_data());
		}
		dec.first = first;
		Ok(dec)
	}

	/// Rate of the audio last produced, read from each decoded buffer, so HE-AAC's SBR rate wins over the container's.
	/// It can change after a decoder reset (chained Ogg): re-read it after each `next_stereo`.
	pub fn sample_rate(&self) -> u32 {
		self.rate
	}

	/// The codec label ("MP3", "AAC", …) of the track being decoded.
	pub fn codec(&self) -> &'static str {
		self.codec
	}

	/// Channel count of the audio last produced; 0 before the first buffer.
	pub fn channels(&self) -> u32 {
		self.channels
	}

	/// Appends one packet as interleaved stereo at `sample_rate()`; `false` at end of stream.
	pub fn next_stereo(&mut self, out: &mut Vec<f32>) -> Result<bool> {
		if !self.first.is_empty() {
			out.append(&mut self.first);
			return Ok(true);
		}
		let mut unsupported = 0;
		loop {
			let packet = match self.format.next_packet() {
				Ok(Some(p)) => p,
				Ok(None) => return Ok(false),
				Err(Error::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
					return Ok(false);
				}
				// Chained Ogg (a new logical stream per song on Icecast) needs a fresh decoder.
				Err(Error::ResetRequired) => {
					(self.decoder, self.track_id, self.codec) =
						open_track(self.format.as_ref(), &self.codecs)?;
					continue;
				}
				// Corrupt live data: the demuxer drops the bad header bytes and resyncs.
				Err(Error::DecodeError(_)) => continue,
				// A false ADTS sync can claim unsupported features; a genuinely unsupported
				// stream fails before its first packet, when no audio has been produced yet. A run of
				// them means the stream really changed to something unsupported (chained Ogg).
				Err(Error::Unsupported(_))
					if self.rate != 0 && unsupported < MAX_UNSUPPORTED_RUN =>
				{
					unsupported += 1;
					continue;
				}
				Err(e) => return Err(decode_err(e)),
			};
			if packet.track_id != self.track_id {
				continue;
			}
			let buf = match self.decoder.decode(&packet) {
				Ok(buf) => buf,
				Err(Error::DecodeError(_) | Error::IoError(_)) => continue,
				Err(e) => return Err(decode_err(e)),
			};
			if buf.frames() == 0 {
				continue;
			}
			self.rate = buf.spec().rate();
			let channels = buf.spec().channels().count();
			self.channels = channels as u32;
			buf.copy_to_vec_interleaved(&mut self.scratch);
			match channels {
				1 => out.extend(self.scratch.iter().flat_map(|&s| [s, s])),
				2 => out.extend_from_slice(&self.scratch),
				// Planes are in symphonia's canonical order, front left and right first.
				n => out.extend(self.scratch.chunks_exact(n).flat_map(|f| [f[0], f[1]])),
			}
			return Ok(true);
		}
	}
}

fn open_track(
	format: &dyn FormatReader,
	codecs: &CodecRegistry,
) -> Result<(Box<dyn AudioDecoder>, u32, &'static str)> {
	let track = format
		.default_track(TrackType::Audio)
		.ok_or_else(|| CoreError::Decode("no audio track".into()))?;
	let params = track
		.codec_params
		.as_ref()
		.and_then(|p| p.audio())
		.ok_or_else(|| CoreError::Decode("no audio codec parameters".into()))?;
	let codec = codec_label(params.codec);
	let decoder = codecs
		.make_audio_decoder(params, &AudioDecoderOptions::default())
		.map_err(decode_err)?;
	Ok((decoder, track.id, codec))
}

#[cfg(test)]
mod tests {
	use super::*;

	fn fixture(name: &str) -> std::fs::File {
		let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
		std::fs::File::open(path).unwrap()
	}

	fn decode_all(reader: Box<dyn Read + Send + Sync>, ext: &str) -> (u32, Vec<f32>) {
		let mut dec = Decoder::new(reader, Some(ext)).unwrap();
		let mut out = Vec::new();
		while dec.next_stereo(&mut out).unwrap() {}
		(dec.sample_rate(), out)
	}

	fn peak(samples: impl Iterator<Item = f32>) -> f32 {
		samples.fold(0.0, |m, s| m.max(s.abs()))
	}

	#[test]
	fn decodes_each_fixture() {
		// Opus always decodes at 48 kHz whatever the encoder's input rate was.
		for (name, rate) in [
			("tone.mp3", 44100),
			("tone.aac", 44100),
			("tone.opus", 48000),
			("tone.ogg", 44100),
			("tone.flac", 44100),
		] {
			let ext = name.rsplit('.').next().unwrap();
			let (got_rate, out) = decode_all(Box::new(fixture(name)), ext);
			assert_eq!(got_rate, rate, "{name}");
			let frames = (out.len() / 2) as f64;
			let want = 5.0 * rate as f64;
			assert!(
				(frames - want).abs() <= want * 0.02,
				"{name}: {frames} frames"
			);
			assert!(peak(out.into_iter()) > 0.05, "{name}: silent");
		}
	}

	#[test]
	fn decode_mono_to_stereo() {
		let (rate, out) = decode_all(Box::new(fixture("mono22k.mp3")), "mp3");
		assert_eq!(rate, 22050);
		assert!(!out.is_empty());
		assert!(out.as_chunks::<2>().0.iter().all(|[l, r]| l == r));
	}

	#[test]
	fn decode_surround_takes_front_pair() {
		// The fixture has the tone on front left/right only; the other four channels are silent.
		let (rate, out) = decode_all(Box::new(fixture("surround.flac")), "flac");
		assert_eq!(rate, 48000);
		assert_eq!(out.len() / 2, 2 * 48000);
		assert!(peak(out.iter().step_by(2).copied()) > 0.05);
		assert!(peak(out.iter().skip(1).step_by(2).copied()) > 0.05);
	}

	#[test]
	fn corrupt_bytes_are_skipped() {
		let mut data = Vec::new();
		fixture("tone.mp3").read_to_end(&mut data).unwrap();
		let mid = data.len() / 2;
		data[mid..mid + 2000].fill(0xAB);
		let (_, out) = decode_all(Box::new(std::io::Cursor::new(data)), "mp3");
		assert!(out.len() / 2 > 4 * 44100);
	}

	#[test]
	fn corrupt_adts_bytes_are_skipped() {
		let mut data = Vec::new();
		fixture("tone.aac").read_to_end(&mut data).unwrap();
		// False ADTS sync words: a forbidden sample rate (demuxer DecodeError), then a valid-looking
		// header claiming 4 raw data blocks (demuxer Unsupported).
		let third = data.len() / 3;
		for (at, pattern) in [
			(third, &[0xFF, 0xF1][..]),
			(2 * third, &[0xFF, 0xF1, 0x50, 0x80, 0x10, 0x1F, 0xFF][..]),
		] {
			for (b, p) in data[at..at + 1400].iter_mut().zip(pattern.iter().cycle()) {
				*b = *p;
			}
		}
		let (_, out) = decode_all(Box::new(std::io::Cursor::new(data)), "aac");
		assert!(out.len() / 2 > 4 * 44100, "{} frames", out.len() / 2);
	}

	#[test]
	fn decoder_reports_codec_channels_and_rate() {
		let dec = Decoder::new(Box::new(fixture("tone.mp3")), Some("mp3")).unwrap();
		assert_eq!(dec.codec(), "MP3");
		assert_eq!(dec.sample_rate(), 44100);
		assert_eq!(dec.channels(), 2);
		let dec = Decoder::new(Box::new(fixture("tone.opus")), Some("opus")).unwrap();
		assert_eq!(dec.codec(), "Opus");
		let dec = Decoder::new(Box::new(fixture("mono22k.mp3")), Some("mp3")).unwrap();
		assert_eq!(dec.channels(), 1);
	}

	#[test]
	fn format_describe_drops_unknown_parts() {
		let f = StreamFormat {
			codec: "MP3".into(),
			bitrate_kbps: Some(128),
			sample_rate: 44100,
			channels: 2,
		};
		assert_eq!(f.describe(), "MP3, 128 kbps, 44.1 kHz, stereo");
		let f = StreamFormat {
			codec: "AAC".into(),
			bitrate_kbps: None,
			sample_rate: 48000,
			channels: 1,
		};
		assert_eq!(f.describe(), "AAC, 48 kHz, mono");
		let f = StreamFormat {
			codec: "Audio".into(),
			bitrate_kbps: None,
			sample_rate: 0,
			channels: 0,
		};
		assert_eq!(f.describe(), "Audio");
	}

	fn open_piped(data: &'static [u8]) -> Result<Decoder> {
		let (tx, reader) = byte_pipe();
		if !data.is_empty() {
			tx.send(Bytes::from_static(data)).unwrap();
		}
		drop(tx);
		Decoder::new(Box::new(reader), None)
	}

	#[test]
	fn garbage_then_eof_is_an_error() {
		for garbage in [&[0xFF; 100][..], &[0xAB; 100][..]] {
			assert!(matches!(open_piped(garbage), Err(CoreError::Decode(_))));
		}
		let Err(e) = open_piped(b"") else {
			panic!("empty stream opened")
		};
		assert_eq!(e.to_string(), "No data");
	}

	#[test]
	fn decodes_through_byte_pipe() {
		let (tx, reader) = byte_pipe();
		std::thread::spawn(move || {
			let mut data = Vec::new();
			fixture("tone.aac").read_to_end(&mut data).unwrap();
			for chunk in data.chunks(1500) {
				tx.send(Bytes::copy_from_slice(chunk)).unwrap();
			}
		});
		let (rate, out) = decode_all(Box::new(reader), "aac");
		assert_eq!(rate, 44100);
		assert!(out.len() / 2 > 4 * 44100);
	}

	#[test]
	fn byte_reader_eof_on_sender_drop() {
		let (tx, mut reader) = byte_pipe();
		tx.send(Bytes::from_static(b"ab")).unwrap();
		tx.send(Bytes::from_static(b"c")).unwrap();
		drop(tx);
		let mut got = Vec::new();
		reader.read_to_end(&mut got).unwrap();
		assert_eq!(got, b"abc");
	}
}
