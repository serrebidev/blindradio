use serde::{Deserialize, Serialize};

/// Where a station came from. Ids follow the source: `rb:<uuid>`, `tunein:<guide_id>`,
/// `iheart:<id>`, `icecast:<url>`, `shoutcast:<id>`, `internet-radio:<slug>`,
/// `lautfm:<slug>`, `net:<network-slug>:<slug>`, `custom:<uuid-v4>`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Source {
	RadioBrowser,
	TuneIn,
	IHeart,
	Icecast,
	Shoutcast,
	InternetRadio,
	LautFm,
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
			(Source::Network("BBC".into()), r#"{"Network":"BBC"}"#),
			(Source::Custom, r#""Custom""#),
		] {
			assert_eq!(serde_json::to_string(&source).unwrap(), json);
			assert_eq!(serde_json::from_str::<Source>(json).unwrap(), source);
		}
	}
}
