//! AAC in LOAS/LATM (MPEG-TS stream type 0x11, the usual DVB-T2 and DVB-S2 carriage) rewritten as
//! ADTS frames, which the AAC decoder already reads. Frame for frame, so it adds no delay.

const LOAS_HEADER: usize = 3;
const ADTS_HEADER: usize = 7;
const ADTS_MAX_FRAME: usize = 0x1FFF;

/// What an ADTS header needs from the stream's AudioSpecificConfig.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Config {
	/// The core object type: HE-AAC's SBR and PS are found again by the decoder, from the data.
	object_type: u8,
	rate_index: u8,
	channels: u8,
	subframes: u8,
}

struct Bits<'a> {
	data: &'a [u8],
	pos: usize,
}

impl Bits<'_> {
	fn read(&mut self, n: usize) -> Option<u32> {
		let mut value = 0u32;
		for _ in 0..n {
			let byte = *self.data.get(self.pos / 8)?;
			value = value << 1 | u32::from(byte >> (7 - self.pos % 8) & 1);
			self.pos += 1;
		}
		Some(value)
	}

	fn peek(&mut self, n: usize) -> Option<u32> {
		let at = self.pos;
		let value = self.read(n);
		self.pos = at;
		value
	}

	fn object_type(&mut self) -> Option<u32> {
		match self.read(5)? {
			31 => Some(32 + self.read(6)?),
			t => Some(t),
		}
	}

	/// A sample rate index; an explicit rate must be one ADTS can name.
	fn rate_index(&mut self) -> Option<u8> {
		const RATES: [u32; 13] = [
			96_000, 88_200, 64_000, 48_000, 44_100, 32_000, 24_000, 22_050, 16_000, 12_000, 11_025,
			8_000, 7_350,
		];
		match self.read(4)? {
			15 => {
				let rate = self.read(24)?;
				RATES.iter().position(|&r| r == rate).map(|i| i as u8)
			}
			i if (i as usize) < RATES.len() => Some(i as u8),
			_ => None,
		}
	}

	fn latm_value(&mut self) -> Option<u32> {
		let bytes = self.read(2)? as usize + 1;
		self.read(bytes * 8)
	}
}

/// AudioSpecificConfig: object type, rate index and channels.
fn audio_specific_config(b: &mut Bits) -> Option<(u8, u8, u8)> {
	let mut object_type = b.object_type()?;
	let rate_index = b.rate_index()?;
	let channels = b.read(4)? as u8;
	// Explicit SBR or PS: the core's type follows the extension's rate.
	if matches!(object_type, 5 | 29) {
		b.rate_index()?;
		object_type = b.object_type()?;
	}
	// ADTS has two bits for the type: Main, LC, SSR and LTP. A channel config of 0 needs a
	// program config element, which ADTS cannot carry in its header.
	if !matches!(object_type, 1..=4) || channels == 0 || channels > 7 {
		return None;
	}
	// GASpecificConfig.
	b.read(1)?;
	if b.read(1)? == 1 {
		b.read(14)?;
	}
	b.read(1)?;
	// Backward-compatible SBR signalling; a frame length type of 0, next otherwise, cannot match.
	if b.peek(11) == Some(0x2B7) {
		b.read(11)?;
		if b.object_type()? == 5 && b.read(1)? == 1 {
			b.rate_index()?;
			if b.peek(11) == Some(0x548) {
				b.read(12)?;
			}
		}
	}
	Some((object_type as u8, rate_index, channels))
}

/// StreamMuxConfig, for the one layout broadcasters use: one program, one layer, payloads that
/// state their own length.
fn stream_mux_config(b: &mut Bits) -> Option<Config> {
	let version = b.read(1)?;
	if version == 1 {
		if b.read(1)? != 0 {
			return None;
		}
		b.latm_value()?;
	}
	let same_time_framing = b.read(1)?;
	let subframes = b.read(6)? as u8;
	let (programs, layers) = (b.read(4)?, b.read(3)?);
	if same_time_framing != 1 || programs != 0 || layers != 0 {
		return None;
	}
	let (object_type, rate_index, channels) = if version == 1 {
		let length = b.latm_value()? as usize;
		let start = b.pos;
		let config = audio_specific_config(b)?;
		b.pos = start + length;
		config
	} else {
		audio_specific_config(b)?
	};
	if b.read(3)? != 0 {
		return None;
	}
	b.read(8)?;
	if b.read(1)? == 1 {
		if version == 1 {
			b.latm_value()?;
		} else {
			while b.read(1)? == 1 {
				b.read(8)?;
			}
			b.read(8)?;
		}
	}
	if b.read(1)? == 1 {
		b.read(8)?;
	}
	Some(Config {
		object_type,
		rate_index,
		channels,
		subframes,
	})
}

fn push_adts(config: &Config, payload: &[u8], out: &mut Vec<u8>) {
	let len = payload.len() + ADTS_HEADER;
	if len > ADTS_MAX_FRAME {
		return;
	}
	out.extend_from_slice(&[
		0xFF,
		0xF1,
		(config.object_type - 1) << 6 | config.rate_index << 2 | config.channels >> 2,
		config.channels << 6 | (len >> 11) as u8,
		(len >> 3) as u8,
		(len as u8) << 5 | 0x1F,
		0xFC,
	]);
	out.extend_from_slice(payload);
}

/// Whether these bytes start a LOAS frame.
pub(crate) fn is_loas(bytes: &[u8]) -> bool {
	matches!(bytes, [0x56, b, ..] if b & 0xE0 == 0xE0)
}

#[derive(Default)]
pub(crate) struct LatmToAdts {
	config: Option<Config>,
	carry: Vec<u8>,
}

impl LatmToAdts {
	/// Takes LOAS bytes in pieces of any size; appends the ADTS frames they complete.
	pub(crate) fn feed(&mut self, bytes: &[u8], out: &mut Vec<u8>) {
		self.carry.extend_from_slice(bytes);
		let buf = std::mem::take(&mut self.carry);
		let mut at = 0;
		while let Some(head) = buf.get(at..at + LOAS_HEADER) {
			if !is_loas(head) {
				at += 1;
				continue;
			}
			let len = usize::from(head[1] & 0x1F) << 8 | usize::from(head[2]);
			let Some(frame) = buf.get(at + LOAS_HEADER..at + LOAS_HEADER + len) else {
				break;
			};
			self.element(frame, out);
			at += LOAS_HEADER + len;
		}
		self.carry = buf[at..].to_vec();
	}

	/// One AudioMuxElement. A damaged one yields nothing; the next frame carries on.
	fn element(&mut self, data: &[u8], out: &mut Vec<u8>) -> Option<()> {
		let mut b = Bits { data, pos: 0 };
		if b.read(1)? == 0 {
			self.config = stream_mux_config(&mut b);
		}
		let config = self.config?;
		for _ in 0..=config.subframes {
			let mut length = 0;
			loop {
				let part = b.read(8)? as usize;
				length += part;
				if part != 255 {
					break;
				}
			}
			// Payloads need not start on a byte boundary.
			let payload: Option<Vec<u8>> =
				(0..length).map(|_| b.read(8).map(|v| v as u8)).collect();
			push_adts(&config, &payload?, out);
		}
		Some(())
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	/// Writes bits, most significant first.
	#[derive(Default)]
	struct Writer {
		bits: Vec<u8>,
	}

	impl Writer {
		fn put(&mut self, n: usize, value: u32) -> &mut Self {
			for i in (0..n).rev() {
				self.bits.push((value >> i & 1) as u8);
			}
			self
		}

		fn bytes(&self) -> Vec<u8> {
			self.bits
				.chunks(8)
				.map(|c| c.iter().enumerate().fold(0, |b, (i, bit)| b | bit << (7 - i)))
				.collect()
		}
	}

	fn loas(body: &[u8]) -> Vec<u8> {
		let mut out = vec![0x56, 0xE0 | (body.len() >> 8) as u8, body.len() as u8];
		out.extend_from_slice(body);
		out
	}

	/// New config (version 0, AAC-LC, 48 kHz, stereo), then `payload`.
	fn frame_with_config(payload: &[u8]) -> Vec<u8> {
		let mut w = Writer::default();
		w.put(1, 0).put(1, 0).put(1, 1).put(6, 0).put(4, 0).put(3, 0);
		w.put(5, 2).put(4, 3).put(4, 2).put(3, 0);
		w.put(3, 0).put(8, 0xFF).put(1, 0).put(1, 0);
		w.put(8, payload.len() as u32);
		for &b in payload {
			w.put(8, u32::from(b));
		}
		loas(&w.bytes())
	}

	#[test]
	fn latm_becomes_adts_whatever_the_chunking() {
		let payload = [0xDE, 0xAD, 0xBE, 0xEF, 0x01];
		let mut same = Writer::default();
		same.put(1, 1).put(8, 2).put(8, 0xAB).put(8, 0xCD);
		let stream = [frame_with_config(&payload), loas(&same.bytes())].concat();
		for size in [1, 2, 5, stream.len()] {
			let mut latm = LatmToAdts::default();
			let mut out = Vec::new();
			for chunk in stream.chunks(size) {
				latm.feed(chunk, &mut out);
			}
			let len = ADTS_HEADER + payload.len();
			assert_eq!(
				out[..ADTS_HEADER],
				[0xFF, 0xF1, 1 << 6 | 3 << 2, 2 << 6, (len >> 3) as u8, (len as u8) << 5 | 0x1F, 0xFC],
				"chunks of {size}"
			);
			assert_eq!(out[ADTS_HEADER..len], payload);
			assert_eq!(out[len + ADTS_HEADER..], [0xAB, 0xCD], "the reused config");
		}
	}

	#[test]
	fn sbr_and_ps_signalling_leave_the_core_in_the_header() {
		// Explicit HE-AAC: type 5, core rate 24 kHz, stereo, SBR rate 48 kHz, core AAC-LC.
		let mut w = Writer::default();
		w.put(5, 5).put(4, 6).put(4, 2).put(4, 3).put(5, 2).put(3, 0);
		let bytes = w.bytes();
		let config = audio_specific_config(&mut Bits { data: &bytes, pos: 0 });
		assert_eq!(config, Some((2, 6, 2)));
		// Backward compatible: plain AAC-LC, then the sync extensions, all consumed.
		let mut w = Writer::default();
		w.put(5, 2).put(4, 6).put(4, 2).put(3, 0);
		w.put(11, 0x2B7).put(5, 5).put(1, 1).put(4, 3).put(11, 0x548).put(1, 1);
		let bytes = w.bytes();
		let mut b = Bits { data: &bytes, pos: 0 };
		assert_eq!(audio_specific_config(&mut b), Some((2, 6, 2)));
		assert_eq!(b.pos, 16 + 21 + 12);
	}

	#[test]
	fn what_adts_cannot_carry_is_refused() {
		// Channel config 0 needs a program config element.
		let mut w = Writer::default();
		w.put(5, 2).put(4, 3).put(4, 0).put(3, 0);
		let bytes = w.bytes();
		assert_eq!(audio_specific_config(&mut Bits { data: &bytes, pos: 0 }), None);
		// More than one program.
		let mut w = Writer::default();
		w.put(1, 0).put(1, 1).put(6, 0).put(4, 1).put(3, 0).put(16, 0);
		let bytes = w.bytes();
		assert_eq!(stream_mux_config(&mut Bits { data: &bytes, pos: 0 }), None);
		assert!(is_loas(&[0x56, 0xE1]) && !is_loas(&[0x56, 0x10]) && !is_loas(&[0xFF, 0xF1]));
	}
}
