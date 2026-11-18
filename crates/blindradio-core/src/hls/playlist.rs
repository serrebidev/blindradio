//! HLS playlists as this player needs them, read by the `m3u8-parser` crate.

use m3u8_parser::m3u8::error::ParseError;
use m3u8_parser::m3u8::playlist::Playlist as Parsed;
pub(super) use m3u8_parser::m3u8::playlist::MediaSegment;
pub(super) use m3u8_parser::m3u8::tags::Key;
use m3u8_parser::m3u8::tags::Tag;

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Playlist {
	MasterPlaylist(MasterPlaylist),
	MediaPlaylist(MediaPlaylist),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct MasterPlaylist {
	/// I-frame streams are left out: they hold no audio.
	pub variants: Vec<VariantStream>,
	pub alternatives: Vec<AlternativeMedia>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct VariantStream {
	pub uri: String,
	pub bandwidth: u64,
	pub codecs: Option<String>,
	/// The EXT-X-MEDIA group holding this variant's audio.
	pub audio: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct AlternativeMedia {
	pub is_audio: bool,
	pub group_id: String,
	pub uri: Option<String>,
	pub default: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct MediaPlaylist {
	pub target_duration: u64,
	pub media_sequence: u64,
	/// Each segment carries the key, map and byte range in force for it.
	pub segments: Vec<MediaSegment>,
	pub end_list: bool,
}

/// Lines of a playlist given up on before the whole playlist is.
const BAD_LINES_ALLOWED: usize = 32;

/// The parser refuses a whole playlist over one line it cannot read. A station keeps playing
/// without that line. #EXTINF without its comma is common enough to repair instead: losing the
/// line would lose the segment.
fn tolerant(text: &str) -> Option<Parsed> {
	let mut lines: Vec<String> = text
		.lines()
		.map(|line| {
			let line = line.trim();
			if line.starts_with("#EXTINF:") && !line.contains(',') {
				format!("{line},")
			} else {
				line.to_owned()
			}
		})
		.collect();
	for _ in 0..=BAD_LINES_ALLOWED {
		match lines.join("\n").parse() {
			Ok(parsed) => return Some(parsed),
			Err(ParseError::Syntax { line, .. }) if (1..=lines.len()).contains(&line) => {
				lines.remove(line - 1);
			}
			Err(_) => return None,
		}
	}
	None
}

/// The title of an #EXTINF. iHeart and StreamGuys write `title="A",artist="B",url="..."`:
/// only the first of those is kept, the rest is tracking data nobody wants read aloud.
fn extinf_title(text: &str) -> String {
	match text.split_once("\",") {
		Some((title, _)) if text.starts_with("title=\"") => format!("{title}\""),
		_ => text.to_owned(),
	}
}

fn master(parsed: &Parsed) -> MasterPlaylist {
	let mut out = MasterPlaylist::default();
	let mut pending = None;
	for tag in &parsed.tags {
		match tag {
			Tag::ExtXStreamInf {
				bandwidth,
				codecs,
				audio,
				..
			} => {
				pending = Some(VariantStream {
					uri: String::new(),
					bandwidth: u64::from(*bandwidth),
					codecs: codecs.clone(),
					audio: audio.clone(),
				});
			}
			Tag::ExtXMedia {
				type_,
				group_id,
				uri,
				default,
				..
			} => out.alternatives.push(AlternativeMedia {
				is_audio: type_ == "AUDIO",
				group_id: group_id.clone(),
				uri: uri.clone(),
				default: *default == Some(true),
			}),
			Tag::Uri(uri) => {
				if let Some(mut variant) = pending.take() {
					variant.uri.clone_from(uri);
					out.variants.push(variant);
				}
			}
			_ => {}
		}
	}
	out
}

fn media(parsed: &Parsed) -> MediaPlaylist {
	let mut out = MediaPlaylist {
		segments: parsed.media_segments(),
		..MediaPlaylist::default()
	};
	for segment in &mut out.segments {
		segment.title = segment
			.title
			.take()
			.map(|title| extinf_title(&title))
			.filter(|title| !title.is_empty());
	}
	for tag in &parsed.tags {
		match tag {
			Tag::ExtXTargetDuration(seconds) => out.target_duration = *seconds,
			Tag::ExtXMediaSequence(first) => out.media_sequence = *first,
			Tag::ExtXEndList => out.end_list = true,
			_ => {}
		}
	}
	out
}

/// `None` when the text does not open with #EXTM3U, or is mostly not HLS.
pub(super) fn parse(body: &[u8]) -> Option<Playlist> {
	if !body.starts_with(b"#EXTM3U") {
		return None;
	}
	let parsed = tolerant(&String::from_utf8_lossy(body))?;
	let is_master = parsed
		.tags
		.iter()
		.any(|t| matches!(t, Tag::ExtXStreamInf { .. } | Tag::ExtXMedia { .. }));
	Some(if is_master {
		Playlist::MasterPlaylist(master(&parsed))
	} else {
		Playlist::MediaPlaylist(media(&parsed))
	})
}

#[cfg(test)]
mod tests {
	use super::*;

	fn as_master(text: &str) -> MasterPlaylist {
		match parse(text.as_bytes()) {
			Some(Playlist::MasterPlaylist(m)) => m,
			other => panic!("not a master playlist: {other:?}"),
		}
	}

	fn as_media(text: &str) -> MediaPlaylist {
		match parse(text.as_bytes()) {
			Some(Playlist::MediaPlaylist(m)) => m,
			other => panic!("not a media playlist: {other:?}"),
		}
	}

	#[test]
	fn only_extm3u_opens_a_playlist() {
		assert_eq!(parse(b""), None);
		assert_eq!(parse(b"<html>#EXTM3U"), None);
		assert_eq!(as_media("#EXTM3U"), MediaPlaylist::default());
	}

	#[test]
	fn master_reads_variants_audio_groups_and_skips_i_frames() {
		let m = as_master(
			"#EXTM3U\r\n\
			#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"aac\",NAME=\"A, B\",DEFAULT=YES,URI=\"a.m3u8\"\r\n\
			#EXT-X-MEDIA:TYPE=SUBTITLES,GROUP-ID=\"s\",NAME=\"S\",URI=\"s.m3u8\"\r\n\
			#EXT-X-I-FRAME-STREAM-INF:BANDWIDTH=9,URI=\"i.m3u8\"\r\n\
			#EXT-X-STREAM-INF:BANDWIDTH=70000,CODECS=\"avc1.42e00a,mp4a.40.2\",AUDIO=\"aac\"\r\n\
			\r\n\
			v.m3u8?token=a=b,c\r\n",
		);
		assert_eq!(
			m.variants,
			[VariantStream {
				uri: "v.m3u8?token=a=b,c".into(),
				bandwidth: 70000,
				codecs: Some("avc1.42e00a,mp4a.40.2".into()),
				audio: Some("aac".into()),
			}]
		);
		assert_eq!(m.alternatives.len(), 2);
		assert!(m.alternatives[0].is_audio && m.alternatives[0].default);
		assert_eq!(m.alternatives[0].uri.as_deref(), Some("a.m3u8"));
		assert!(!m.alternatives[1].is_audio);
	}

	#[test]
	fn each_segment_knows_the_key_map_and_range_in_force() {
		let m = as_media(
			"#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-TARGETDURATION:6\n#EXT-X-MEDIA-SEQUENCE:42\n\
			#EXT-X-MAP:URI=\"init.mp4\",BYTERANGE=\"800@16\"\n\
			#EXT-X-KEY:METHOD=AES-128,URI=\"k,1\",IV=0x0A,KEYFORMAT=\"identity\"\n\
			#EXT-X-BYTERANGE:100@200\n#EXTINF:5.5,Artist, Song\nall.aac\n\
			#EXT-X-BYTERANGE:50\n#EXTINF:5,\nall.aac\n\
			#EXT-X-KEY:METHOD=NONE\n#EXTINF:5\nplain.aac\n#EXT-X-ENDLIST\n",
		);
		assert_eq!((m.target_duration, m.media_sequence, m.end_list), (6, 42, true));
		let [first, second, third] = &m.segments[..] else {
			panic!("three segments");
		};
		assert_eq!(first.title.as_deref(), Some("Artist, Song"));
		let range = first.byterange.unwrap();
		assert_eq!((range.length, range.offset), (100, Some(200)));
		let map = first.map.as_ref().unwrap();
		assert_eq!(map.uri, "init.mp4");
		let key = first.key.as_ref().unwrap();
		assert_eq!(key.method, "AES-128");
		assert_eq!(key.uri.as_deref(), Some("k,1"));
		assert_eq!(key.iv.as_deref(), Some("0x0A"));
		assert_eq!(key.keyformat.as_deref(), Some("identity"));
		assert_eq!(second.title, None);
		assert_eq!(second.key, first.key, "a key holds until the next one");
		assert_eq!(second.byterange.unwrap().offset, None);
		assert_eq!(third.key, None, "METHOD=NONE ends it");
	}

	#[test]
	fn commas_in_a_title_never_make_a_segment() {
		let m = as_media(
			"#EXTM3U\n#EXTINF:6.0,title=\"Marques Wyatt\",artist=\"Get Down\",url=\"x=\\\"1\\\"\"\nhttp://x/a.aac\n",
		);
		assert_eq!(m.segments.len(), 1);
		assert_eq!(m.segments[0].uri, "http://x/a.aac");
		assert_eq!(m.segments[0].title.as_deref(), Some("title=\"Marques Wyatt\""));
	}

	#[test]
	fn one_bad_line_never_stops_a_station() {
		let m = as_media(
			"#EXTM3U\n#EXT-X-TARGETDURATION:10.0\n#EXT-X-MEDIA-SEQUENCE:abc\n#EXT-X-KEY:URI=\"k\"\n\
			#EXTINF:10\na.aac\n#EXTINF:10,\nb.aac\n#EXTINF:10,\n",
		);
		let uris: Vec<&str> = m.segments.iter().map(|s| s.uri.as_str()).collect();
		assert_eq!(uris, ["a.aac", "b.aac"]);
		assert!(m.segments[0].key.is_none());
	}

	#[test]
	fn a_playlist_of_nothing_but_bad_lines_is_refused() {
		let body = format!("#EXTM3U\n{}", "#EXTINF:x,\n".repeat(BAD_LINES_ALLOWED + 2));
		assert_eq!(parse(body.as_bytes()), None);
	}
}
