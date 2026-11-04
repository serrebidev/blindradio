use serde::{Deserialize, Serialize};

/// Where a station came from. Ids follow the source: `rb:<uuid>`, `tunein:<guide_id>`,
/// `iheart:<id>`, `icecast:<url>`, `shoutcast:<id>`, `internet-radio:<slug>`,
/// `iprd:<id>`, `live365:<mount>`, `recommended:<file>:<row>`, `net:<network-slug>:<slug>`, `custom:<uuid-v4>`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Source {
	RadioBrowser,
	TuneIn,
	IHeart,
	Icecast,
	Shoutcast,
	InternetRadio,
	/// A directory no longer offered. Stays so saved stations from it still load.
	LautFm,
	Iprd,
	RadioNet,
	Live365,
	/// The BlindRadio Directory of submitted stations.
	Directory,
	Recommended,
	/// A curated network; the string is its display name.
	Network(String),
	#[default]
	Custom,
}

impl Source {
	pub fn label(&self) -> &str {
		match self {
			Source::RadioBrowser => "Radio Browser",
			Source::TuneIn => "TuneIn",
			Source::IHeart => "iHeartRadio",
			Source::Icecast => "Icecast",
			Source::Shoutcast => "SHOUTcast",
			Source::InternetRadio => "Internet-Radio",
			Source::LautFm => "laut.fm",
			Source::Iprd => "IPRD",
			Source::RadioNet => "radio.net",
			Source::Live365 => "Live365",
			Source::Directory => "BlindRadio Directory",
			Source::Recommended => "Recommended Radio Streams",
			Source::Network(name) => name,
			Source::Custom => "Custom",
		}
	}
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Station {
	pub id: String,
	pub name: String,
	pub url: String,
	#[serde(default)]
	pub source: Source,
	/// Short second column, e.g. `Jazz, France, AAC 128k`.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub info: Option<String>,
	/// What the directory says about it, for Station Info.
	#[serde(default, skip_serializing_if = "Details::is_empty")]
	pub details: Details,
}

/// A directory's facts about a station, each only where the directory gives it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Details {
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub genre: Option<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub country: Option<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub language: Option<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub codec: Option<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub bitrate_kbps: Option<u32>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub homepage: Option<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub call_sign: Option<String>,
	/// Why the directory says it will not play. Said only when playing it fails.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub unavailable: Option<String>,
}

impl Details {
	pub fn is_empty(&self) -> bool {
		*self == Details::default()
	}
}

/// `None` for blank directory text.
pub fn known(text: &str) -> Option<String> {
	Some(text.trim().to_owned()).filter(|t| !t.is_empty())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn plan1_json_still_loads() {
		let s: Station = serde_json::from_str(r#"{"id":"a","name":"A","url":"http://x"}"#).unwrap();
		assert_eq!(s.source, Source::Custom);
		assert_eq!(s.info, None);
	}

	#[test]
	fn source_labels() {
		assert_eq!(Source::RadioBrowser.label(), "Radio Browser");
		assert_eq!(Source::TuneIn.label(), "TuneIn");
		assert_eq!(Source::IHeart.label(), "iHeartRadio");
		assert_eq!(Source::LautFm.label(), "laut.fm");
		assert_eq!(Source::Network("BBC".into()).label(), "BBC");
		assert_eq!(Source::Custom.label(), "Custom");
	}
}

#[cfg(test)]
mod json_form_tests {
	use super::*;

	#[test]
	fn source_json_form_is_pinned() {
		for (source, json) in [
			(Source::RadioBrowser, r#""RadioBrowser""#),
			(Source::TuneIn, r#""TuneIn""#),
			(Source::IHeart, r#""IHeart""#),
			(Source::Icecast, r#""Icecast""#),
			(Source::Shoutcast, r#""Shoutcast""#),
			(Source::InternetRadio, r#""InternetRadio""#),
			(Source::LautFm, r#""LautFm""#),
			(Source::Iprd, r#""Iprd""#),
			(Source::Live365, r#""Live365""#),
			(Source::Recommended, r#""Recommended""#),
			(Source::Network("BBC".into()), r#"{"Network":"BBC"}"#),
			(Source::Custom, r#""Custom""#),
		] {
			assert_eq!(serde_json::to_string(&source).unwrap(), json);
			assert_eq!(serde_json::from_str::<Source>(json).unwrap(), source);
		}
	}
}
