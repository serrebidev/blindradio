//! AC-3 and E-AC-3 for symphonia, which demuxes them from MP4 but decodes neither and has no
//! reader for a bare stream, as DVB and ATSC channels carry them. The decoding is oxideav-ac3,
//! pure Rust and compiled in. Output is always stereo: its downmix keeps the centre channel, where
//! a 5.1 channel's dialogue lives.

use parking_lot::Mutex;
use symphonia::core::audio::layouts::CHANNEL_LAYOUT_STEREO;
use symphonia::core::audio::{AsGenericAudioBufferRef, AudioBuffer, AudioMut, AudioSpec, GenericAudioBufferRef};
use symphonia::core::codecs::audio::well_known::{CODEC_ID_AC3, CODEC_ID_EAC3};
use symphonia::core::codecs::audio::{
	AudioCodecParameters, AudioDecoder, AudioDecoderOptions, FinalizeResult,
};
use symphonia::core::codecs::registry::{RegisterableAudioDecoder, SupportedAudioCodec};
use symphonia::core::codecs::{CodecInfo, CodecParameters};
use symphonia::core::common::FourCc;
use symphonia::core::errors::{Error, Result, SeekErrorKind, decode_error, seek_error};
use symphonia::core::formats::prelude::*;
use symphonia::core::formats::probe::{
	ProbeDataMatchSpec, ProbeFormatData, ProbeableFormat, Score, Scoreable,
};
use symphonia::core::io::{MediaSourceStream, ReadBytes, ScopedStream, SeekBuffered};
use symphonia::core::meta::{Metadata, MetadataLog};
use symphonia::core::packet::PacketRef;

/// The most samples per channel one syncframe holds.
const FRAME_SAMPLES: usize = 1536;
const HEADER_BYTES: usize = 7;

const FORMAT: FormatInfo = FormatInfo {
	format: FormatId::new(FourCc::new(*b"ac-3")),
	short_name: "ac3",
	long_name: "AC-3 / E-AC-3 elementary stream",
};

/// One syncframe's header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SyncFrame {
	bytes: usize,
	rate: u32,
	samples: u64,
	eac3: bool,
	/// Dependent substreams and further programs extend or sit beside the main one, which alone
	/// makes a complete stereo downmix.
	main: bool,
}

/// `None` for anything that cannot be a syncframe, so a stray 0B 77 in the data is passed over.
fn parse_header(h: &[u8]) -> Option<SyncFrame> {
	const RATES: [u32; 3] = [48_000, 44_100, 32_000];
	const KBPS: [usize; 19] = [
		32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384, 448, 512, 576, 640,
	];
	let [0x0B, 0x77, b2, b3, b4, b5, ..] = *h else {
		return None;
	};
	let (fscod, bsid) = (usize::from(b4 >> 6), b5 >> 3);
	match bsid {
		..=10 => {
			let code = usize::from(b4 & 0x3F);
			let kbps = *KBPS.get(code / 2)?;
			let words = match fscod {
				0 => kbps * 2,
				1 => kbps * 320 / 147 + (code & 1),
				2 => kbps * 3,
				_ => return None,
			};
			Some(SyncFrame {
				bytes: words * 2,
				rate: RATES[fscod],
				samples: FRAME_SAMPLES as u64,
				eac3: false,
				main: true,
			})
		}
		11..=16 => {
			let strmtyp = b2 >> 6;
			let substream = (b2 >> 3) & 7;
			let bytes = (usize::from(b2 & 7) << 8 | usize::from(b3)) * 2 + 2;
			let (rate, blocks) = match fscod {
				3 => (
					*[24_000, 22_050, 16_000].get(usize::from((b4 >> 4) & 3))?,
					6,
				),
				_ => (RATES[fscod], [1, 2, 3, 6][usize::from((b4 >> 4) & 3)]),
			};
			if strmtyp == 3 || bytes < HEADER_BYTES {
				return None;
			}
			Some(SyncFrame {
				bytes,
				rate,
				samples: blocks * 256,
				eac3: true,
				main: strmtyp != 1 && substream == 0,
			})
		}
		_ => None,
	}
}

pub struct Ac3Reader<'s> {
	reader: MediaSourceStream<'s>,
	media_info: MediaInfo,
	tracks: Vec<Track>,
	metadata: MetadataLog,
	next_ts: Timestamp,
}

impl<'s> Ac3Reader<'s> {
	fn try_new(mut mss: MediaSourceStream<'s>) -> Result<Self> {
		let mut head = [0; HEADER_BYTES];
		mss.read_buf_exact(&mut head)?;
		mss.seek_buffered_rev(HEADER_BYTES);
		let Some(frame) = parse_header(&head) else {
			return decode_error("ac3: not a syncframe");
		};
		let mut params = AudioCodecParameters::new();
		params
			.for_codec(if frame.eac3 { CODEC_ID_EAC3 } else { CODEC_ID_AC3 })
			.with_sample_rate(frame.rate)
			.with_channels(CHANNEL_LAYOUT_STEREO);
		let mut track = Track::new(0);
		track.with_codec_params(CodecParameters::Audio(params));
		Ok(Ac3Reader {
			reader: mss,
			media_info: MediaInfo::from_track(&track),
			tracks: vec![track],
			metadata: MetadataLog::default(),
			next_ts: Timestamp::new(0),
		})
	}

	/// The next syncframe of the main stream; `None` at the end of the data.
	fn next_frame(&mut self) -> std::io::Result<Option<(SyncFrame, Vec<u8>)>> {
		loop {
			let mut sync = 0u16;
			while sync != 0x0B77 {
				sync = sync << 8 | u16::from(self.reader.read_u8()?);
			}
			let mut head = [0x0B, 0x77, 0, 0, 0, 0, 0];
			self.reader.read_buf_exact(&mut head[2..])?;
			let Some(frame) = parse_header(&head) else {
				// A false sync; the real one may be inside what was just read.
				self.reader.seek_buffered_rev(HEADER_BYTES - 2);
				continue;
			};
			let mut data = head.to_vec();
			data.resize(frame.bytes.max(HEADER_BYTES), 0);
			self.reader.read_buf_exact(&mut data[HEADER_BYTES..])?;
			if frame.main {
				return Ok(Some((frame, data)));
			}
		}
	}
}

impl Scoreable for Ac3Reader<'_> {
	/// Two syncframes a frame apart: 0B 77 alone turns up by chance in any other stream.
	fn score(mut src: ScopedStream<&mut MediaSourceStream<'_>>) -> Result<Score> {
		let mut head = [0; HEADER_BYTES];
		src.read_buf_exact(&mut head)?;
		let Some(frame) = parse_header(&head) else {
			return decode_error("ac3: not a syncframe");
		};
		src.ignore_bytes((frame.bytes.max(HEADER_BYTES) - HEADER_BYTES) as u64)?;
		src.read_buf_exact(&mut head)?;
		if parse_header(&head).is_none() {
			return decode_error("ac3: no second syncframe");
		}
		Ok(Score::Supported(255))
	}
}

impl ProbeableFormat<'_> for Ac3Reader<'_> {
	fn try_probe_new(
		mss: MediaSourceStream<'_>,
		_opts: FormatOptions,
	) -> Result<Box<dyn FormatReader + '_>> {
		Ok(Box::new(Ac3Reader::try_new(mss)?))
	}

	fn probe_data() -> &'static [ProbeFormatData] {
		&[ProbeFormatData {
			spec: ProbeDataMatchSpec {
				extensions: &["ac3", "eac3"],
				mime_types: &["audio/ac3", "audio/eac3"],
				markers: &[&[0x0B, 0x77]],
			},
			info: FORMAT,
		}]
	}
}

impl FormatReader for Ac3Reader<'_> {
	fn format_info(&self) -> &FormatInfo {
		&FORMAT
	}

	fn media_info(&self) -> &MediaInfo {
		&self.media_info
	}

	fn metadata(&mut self) -> Metadata<'_> {
		self.metadata.metadata()
	}

	fn seek(&mut self, _mode: SeekMode, _to: SeekTo) -> Result<SeekedTo> {
		seek_error(SeekErrorKind::Unseekable)
	}

	fn tracks(&self) -> &[Track] {
		&self.tracks
	}

	fn next_packet(&mut self) -> Result<Option<Packet>> {
		match self.next_frame() {
			Ok(Some((frame, data))) => {
				let duration = Duration::new(frame.samples);
				let ts = self.next_ts;
				let Some(next) = ts.checked_add(duration) else {
					return Ok(None);
				};
				self.next_ts = next;
				Ok(Some(Packet::new(0, ts, duration, data.into_boxed_slice())))
			}
			Ok(None) => Ok(None),
			Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => Ok(None),
			Err(e) => Err(Error::IoError(e)),
		}
	}

	fn into_inner<'s>(self: Box<Self>) -> MediaSourceStream<'s>
	where
		Self: 's,
	{
		self.reader
	}
}

pub struct Ac3Decoder {
	// Symphonia wants its decoders Sync; only `get_mut` is ever used, so this never waits.
	ac3: Mutex<Box<dyn oxideav_core::Decoder>>,
	eac3: Mutex<Box<dyn oxideav_core::Decoder>>,
	buf: AudioBuffer<f32>,
	codec_params: AudioCodecParameters,
	interleaved: Vec<f32>,
	rate: u32,
}

impl std::fmt::Debug for Ac3Decoder {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("Ac3Decoder")
			.field("codec_params", &self.codec_params)
			.finish_non_exhaustive()
	}
}

fn stereo_buffer(rate: u32, frames: usize) -> AudioBuffer<f32> {
	AudioBuffer::new(AudioSpec::new(rate, CHANNEL_LAYOUT_STEREO), frames)
}

impl Ac3Decoder {
	fn try_new(params: &AudioCodecParameters) -> Result<Self> {
		let make = |id: &str| {
			let mut p = oxideav_core::CodecParameters::audio(oxideav_core::CodecId::new(id));
			// Asking for two channels is what selects the proper downmix.
			p.channels = Some(2);
			p
		};
		let unsupported = |_| Error::Unsupported("ac3: decoder refused its parameters");
		let rate = params.sample_rate.unwrap_or(48_000);
		Ok(Ac3Decoder {
			ac3: Mutex::new(oxideav_ac3::decoder::make_decoder(&make("ac3")).map_err(unsupported)?),
			eac3: Mutex::new(
				oxideav_ac3::decoder::make_eac3_decoder(&make("eac3")).map_err(unsupported)?,
			),
			buf: stereo_buffer(rate, FRAME_SAMPLES),
			codec_params: params.clone(),
			interleaved: Vec::with_capacity(FRAME_SAMPLES * 2),
			rate,
		})
	}
}

impl AudioDecoder for Ac3Decoder {
	fn reset(&mut self) {
		let _ = self.ac3.get_mut().reset();
		let _ = self.eac3.get_mut().reset();
	}

	fn codec_info(&self) -> &CodecInfo {
		&Self::supported_codecs()
			.first()
			.expect("missing codecs")
			.info
	}

	fn codec_params(&self) -> &AudioCodecParameters {
		&self.codec_params
	}

	fn decode_ref(&mut self, packet: &PacketRef) -> Result<GenericAudioBufferRef<'_>> {
		let data = packet.as_buf_reader().read_buf_bytes_available_ref();
		// Each frame says what it is: the container's parameters can lag a change of programme.
		let Some(frame) = parse_header(data) else {
			return decode_error("ac3: not a syncframe");
		};
		let decoder = if frame.eac3 {
			self.eac3.get_mut()
		} else {
			self.ac3.get_mut()
		};
		let packet_in = oxideav_core::Packet::new(
			0,
			oxideav_core::TimeBase::new(1, frame.rate as i64),
			data.to_vec(),
		);
		let failed = |_| Error::DecodeError("ac3: frame decode failed");
		decoder.send_packet(&packet_in).map_err(failed)?;
		let oxideav_core::Frame::Audio(audio) = decoder.receive_frame().map_err(failed)? else {
			return decode_error("ac3: no audio in frame");
		};
		let pcm = audio.data.first().map_or(&[][..], Vec::as_slice);
		self.interleaved.clear();
		self.interleaved.extend(
			pcm.as_chunks::<2>()
				.0
				.iter()
				.map(|b| f32::from(i16::from_le_bytes(*b)) / 32_768.0),
		);
		let frames = self.interleaved.len() / 2;
		if self.rate != frame.rate || frames > self.buf.capacity() {
			self.rate = frame.rate;
			self.buf = stereo_buffer(frame.rate, frames.max(FRAME_SAMPLES));
		}
		self.interleaved.truncate(frames * 2);
		self.buf.clear();
		self.buf.render_uninit(Some(frames));
		self.buf.copy_from_slice_interleaved(&self.interleaved);
		self.buf.trim(
			packet.trim_start.get() as usize,
			packet.trim_end.get() as usize,
		);
		Ok(self.buf.as_generic_audio_buffer_ref())
	}

	fn finalize(&mut self) -> FinalizeResult {
		FinalizeResult::default()
	}

	fn last_decoded(&self) -> GenericAudioBufferRef<'_> {
		self.buf.as_generic_audio_buffer_ref()
	}
}

impl RegisterableAudioDecoder for Ac3Decoder {
	fn try_registry_new(
		params: &AudioCodecParameters,
		_opts: &AudioDecoderOptions,
	) -> Result<Box<dyn AudioDecoder>>
	where
		Self: Sized,
	{
		Ok(Box::new(Ac3Decoder::try_new(params)?))
	}

	fn supported_codecs() -> &'static [SupportedAudioCodec] {
		const CODECS: &[SupportedAudioCodec] = &[
			SupportedAudioCodec {
				id: CODEC_ID_AC3,
				info: CodecInfo {
					short_name: "ac3",
					long_name: "Dolby Digital",
					profiles: &[],
				},
			},
			SupportedAudioCodec {
				id: CODEC_ID_EAC3,
				info: CodecInfo {
					short_name: "eac3",
					long_name: "Dolby Digital Plus",
					profiles: &[],
				},
			},
		];
		CODECS
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn headers_give_the_frame_size_and_reject_junk() {
		// 48 kHz, 96 kbps AC-3: frmsizecod 12, 96 * 2 words.
		let ac3 = parse_header(&[0x0B, 0x77, 0, 0, 0x0C, 0x40, 0]).unwrap();
		assert_eq!((ac3.bytes, ac3.rate, ac3.eac3), (384, 48_000, false));
		// 44.1 kHz rounds by frame: frmsizecod 13 is the odd one of the pair.
		let odd = parse_header(&[0x0B, 0x77, 0, 0, 0x4D, 0x40, 0]).unwrap();
		assert_eq!(odd.bytes, (96 * 320 / 147 + 1) * 2);
		// E-AC-3 states its size; substream 1 and dependent frames are not the main stream.
		let eac3 = parse_header(&[0x0B, 0x77, 0x00, 0x7F, 0x30, 0x80, 0]).unwrap();
		assert_eq!((eac3.bytes, eac3.samples, eac3.main), (256, 1536, true));
		assert!(!parse_header(&[0x0B, 0x77, 0x40, 0x7F, 0x30, 0x80, 0]).unwrap().main);
		assert!(!parse_header(&[0x0B, 0x77, 0x08, 0x7F, 0x30, 0x80, 0]).unwrap().main);
		// Reserved rate, out-of-range size, unknown bsid, no sync.
		assert_eq!(parse_header(&[0x0B, 0x77, 0, 0, 0xC0, 0x40, 0]), None);
		assert_eq!(parse_header(&[0x0B, 0x77, 0, 0, 0x3F, 0x40, 0]), None);
		assert_eq!(parse_header(&[0x0B, 0x77, 0, 0, 0x0C, 0xF8, 0]), None);
		assert_eq!(parse_header(&[0xFF, 0xFB, 0, 0, 0, 0, 0]), None);
	}
}
