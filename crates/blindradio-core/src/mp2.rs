//! MPEG Layer II (MP2) decoding for symphonia, via oxideav-mp2.
//!
//! Symphonia demuxes MPEG audio and tags Layer II tracks, but ships no
//! Layer II decoder, so MP2 stations — BBC relays served through
//! shorteners among them — failed as "unsupported audio codec" while
//! VLC and DVBViewer played them. One packet is one frame here: the
//! demuxer already splits the stream into frames.

use symphonia::core::audio::layouts::{CHANNEL_LAYOUT_MONO, CHANNEL_LAYOUT_STEREO};
use symphonia::core::audio::{
	AsGenericAudioBufferRef, AudioBuffer, AudioMut, AudioSpec, Channels, GenericAudioBufferRef,
};
use symphonia::core::codecs::CodecInfo;
use symphonia::core::codecs::audio::well_known::CODEC_ID_MP2;
use symphonia::core::codecs::audio::{
	AudioCodecParameters, AudioDecoder, AudioDecoderOptions, FinalizeResult,
};
use symphonia::core::codecs::registry::{RegisterableAudioDecoder, SupportedAudioCodec};
use symphonia::core::errors::{Error, Result, unsupported_error};
use symphonia::core::packet::PacketRef;

/// Samples per channel in one Layer II frame.
const FRAME_SAMPLES: usize = oxideav_mp2::frame::PCM_SAMPLES_PER_CHANNEL;

pub struct Mp2Decoder {
	state: oxideav_mp2::frame::FrameDecodeState,
	buf: AudioBuffer<f32>,
	codec_params: AudioCodecParameters,
	interleaved: Vec<f32>,
	rate: u32,
	channels: Channels,
}

impl std::fmt::Debug for Mp2Decoder {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("Mp2Decoder")
			.field("codec_params", &self.codec_params)
			.finish_non_exhaustive()
	}
}

fn channels_for(count: usize) -> Option<Channels> {
	match count {
		1 => Some(CHANNEL_LAYOUT_MONO),
		2 => Some(CHANNEL_LAYOUT_STEREO),
		_ => None,
	}
}

impl Mp2Decoder {
	fn try_new(params: &AudioCodecParameters, _opts: &AudioDecoderOptions) -> Result<Self> {
		let rate = params.sample_rate.unwrap_or(0);
		let channels = params
			.channels
			.as_ref()
			.and_then(|c| channels_for(c.count()))
			.unwrap_or(CHANNEL_LAYOUT_STEREO);
		Ok(Mp2Decoder {
			state: oxideav_mp2::frame::FrameDecodeState::new(),
			buf: AudioBuffer::new(AudioSpec::new(rate, channels.clone()), FRAME_SAMPLES),
			codec_params: params.clone(),
			interleaved: Vec::with_capacity(FRAME_SAMPLES * 2),
			rate,
			channels,
		})
	}
}

impl AudioDecoder for Mp2Decoder {
	fn reset(&mut self) {
		self.state.reset();
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
		let frame = oxideav_mp2::frame::decode_frame_with(data, &mut self.state)
			.map_err(|_| Error::DecodeError("mp2: frame decode failed"))?;
		let channels = frame.header.channels();
		let want = match channels_for(channels) {
			Some(c) => c,
			None => return unsupported_error("mp2: unsupported channel count"),
		};
		// The demuxer's parameters can lag the first frame; follow the frames.
		if self.rate != frame.header.sample_rate || self.channels != want {
			self.rate = frame.header.sample_rate;
			self.channels = want.clone();
			self.buf = AudioBuffer::new(
				AudioSpec::new(frame.header.sample_rate, want),
				FRAME_SAMPLES,
			);
		}
		self.interleaved.clear();
		for i in 0..FRAME_SAMPLES {
			for ch in &frame.pcm {
				self.interleaved.push(ch[i].clamp(-1.0, 1.0) as f32);
			}
		}
		self.buf.clear();
		self.buf.render_uninit(None);
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

impl RegisterableAudioDecoder for Mp2Decoder {
	fn try_registry_new(
		params: &AudioCodecParameters,
		opts: &AudioDecoderOptions,
	) -> Result<Box<dyn AudioDecoder>>
	where
		Self: Sized,
	{
		Ok(Box::new(Mp2Decoder::try_new(params, opts)?))
	}

	fn supported_codecs() -> &'static [SupportedAudioCodec] {
		const CODECS: &[SupportedAudioCodec] = &[SupportedAudioCodec {
			id: CODEC_ID_MP2,
			info: CodecInfo {
				short_name: "mp2",
				long_name: "MPEG Layer II",
				profiles: &[],
			},
		}];
		CODECS
	}
}
