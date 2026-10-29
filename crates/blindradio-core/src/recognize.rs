use crate::error::CoreError;
use crate::{Result, net};

/// A recognized song, from stream metadata or a recognition service.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SongId {
	pub artist: String,
	pub title: String,
	pub album: Option<String>,
}

impl SongId {
	/// "Artist - Title", the shape stream titles and announcements use.
	pub fn display(&self) -> String {
		if self.artist.is_empty() {
			self.title.clone()
		} else {
			format!("{} - {}", self.artist, self.title)
		}
	}
}

/// Encodes stereo f32 as a 16-bit WAV in memory, the sample format
/// recognition services accept.
pub fn wav_bytes(stereo: &[f32], rate: u32) -> Result<Vec<u8>> {
	let spec = hound::WavSpec {
		channels: 2,
		sample_rate: rate,
		bits_per_sample: 16,
		sample_format: hound::SampleFormat::Int,
	};
	let mut cursor = std::io::Cursor::new(Vec::new());
	{
		let mut w = hound::WavWriter::new(&mut cursor, spec)
			.map_err(|e| CoreError::Output(e.to_string()))?;
		for s in stereo {
			w.write_sample((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
				.map_err(|e| CoreError::Output(e.to_string()))?;
		}
		w.finalize().map_err(|e| CoreError::Output(e.to_string()))?;
	}
	Ok(cursor.into_inner())
}

/// AudD's response: `result` is the match, or null when nothing matched.
fn parse_audd(body: &str) -> Option<SongId> {
	let v: serde_json::Value = serde_json::from_str(body).ok()?;
	if v.get("status")?.as_str()? != "success" {
		return None;
	}
	let r = v.get("result")?;
	if r.is_null() {
		return None;
	}
	let title = r.get("title")?.as_str()?.trim().to_string();
	if title.is_empty() {
		return None;
	}
	Some(SongId {
		artist: r
			.get("artist")
			.and_then(|a| a.as_str())
			.unwrap_or("")
			.trim()
			.to_string(),
		title,
		album: r
			.get("album")
			.and_then(|a| a.as_str())
			.map(|a| a.trim().to_string())
			.filter(|a| !a.is_empty()),
	})
}

/// Identifies a WAV sample with AudD. Ok(None) is "no match".
pub async fn identify_audd(token: &str, wav: Vec<u8>) -> Result<Option<SongId>> {
	let file = reqwest::multipart::Part::bytes(wav)
		.file_name("sample.wav")
		.mime_str("audio/wav")
		.map_err(|e| CoreError::Http(e.to_string()))?;
	let form = reqwest::multipart::Form::new()
		.text("api_token", token.to_string())
		.part("file", file);
	let resp = net::client()
		.post("https://api.audd.io/")
		.multipart(form)
		.send()
		.await
		.map_err(net::http_err)?;
	let body = resp.text().await.map_err(net::http_err)?;
	Ok(parse_audd(&body))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn parses_a_match() {
		let body = r#"{"status":"success","result":{"artist":"k.d. lang","title":"Constant Craving","album":"Ingénue"}}"#;
		let song = parse_audd(body).unwrap();
		assert_eq!(song.display(), "k.d. lang - Constant Craving");
		assert_eq!(song.album.as_deref(), Some("Ingénue"));
	}

	#[test]
	fn no_match_and_errors_are_none() {
		assert_eq!(parse_audd(r#"{"status":"success","result":null}"#), None);
		assert_eq!(parse_audd(r#"{"status":"error"}"#), None);
		assert_eq!(parse_audd("not json"), None);
	}

	#[test]
	fn wav_bytes_are_a_wav() {
		let bytes = wav_bytes(&[0.0; 200], 8000).unwrap();
		assert_eq!(&bytes[..4], b"RIFF");
		assert_eq!(&bytes[8..12], b"WAVE");
	}
}
