//! Moondust: the 12moon commons open station dataset (CC0), published by a
//! bot as one NDJSON file per country in a GitHub repo, read live through
//! raw URLs. One record per station with stable ids; streams are
//! third-party addresses the dataset has not playback-checked, so records
//! arrive unchecked exactly as Radio Browser's do.
//! Data model: github.com/12moon-open/moonbase, docs/data-model.md.

use super::{Node, SearchQuery};
use crate::Result;
use crate::station::{Details, Source, Station};
use percent_encoding::percent_decode_str;
use serde::Deserialize;

const PAGE: usize = 500;

/// Records per country file run into the thousands (the US file alone is
/// over six thousand); a search scans this many of the largest files.
const SEARCH_FILES: [&str; 12] = [
	"US", "DE", "GB", "FR", "CA", "BR", "IT", "ES", "AU", "NL", "PL", "MX",
];

fn folder(id: impl Into<String>, name: impl Into<String>) -> Node {
	Node::Folder {
		id: id.into(),
		name: name.into(),
	}
}

/// The country files the dataset publishes, as published. The bot adds
/// files as sources land; a file the dataset drops just answers 404 and
/// its folder stays empty until this list is refreshed.
// ponytail: a fixed list of the 227 published shards; listing them live
// through the contents API per Browse open is the upgrade path.
pub const COUNTRIES: &[&str] = &[
	"AD", "AE", "AF", "AG", "AI", "AL", "AM", "AO", "AQ", "AR", "AS", "AT", "AU", "AW", "AX",
	"AZ", "BA", "BB", "BD", "BE", "BF", "BG", "BH", "BI", "BJ", "BM", "BN", "BO", "BQ", "BR",
	"BS", "BT", "BW", "BY", "BZ", "CA", "CC", "CD", "CF", "CG", "CH", "CI", "CL", "CM", "CN",
	"CO", "CR", "CU", "CV", "CW", "CY", "CZ", "DE", "DJ", "DK", "DM", "DO", "DZ", "EC", "EE",
	"EG", "ER", "ES", "ET", "FI", "FJ", "FM", "FO", "FR", "GA", "GB", "GD", "GE", "GF", "GG",
	"GH", "GI", "GL", "GM", "GN", "GP", "GQ", "GR", "GT", "GU", "GW", "GY", "HK", "HN", "HR",
	"HT", "HU", "ID", "IE", "IL", "IM", "IN", "IQ", "IR", "IS", "IT", "JE", "JM", "JO", "JP",
	"KE", "KG", "KH", "KI", "KM", "KN", "KP", "KR", "KW", "KY", "KZ", "LA", "LB", "LC", "LI",
	"LK", "LR", "LS", "LT", "LU", "LV", "LY", "MA", "MC", "MD", "ME", "MF", "MG", "MH", "MK",
	"ML", "MM", "MN", "MO", "MQ", "MR", "MT", "MU", "MV", "MW", "MX", "MY", "MZ", "NA", "NC",
	"NE", "NG", "NI", "NL", "NO", "NP", "NZ", "OM", "PA", "PE", "PF", "PG", "PH", "PK", "PL",
	"PM", "PR", "PS", "PT", "PW", "PY", "QA", "RE", "RO", "RS", "RU", "RW", "SA", "SB", "SC",
	"SD", "SE", "SG", "SH", "SI", "SK", "SM", "SN", "SO", "SR", "SS", "ST", "SV", "SX", "SY",
	"SZ", "TC", "TD", "TG", "TH", "TJ", "TL", "TM", "TN", "TO", "TR", "TT", "TW", "TZ", "UA",
	"UG", "US", "UY", "UZ", "VC", "VE", "VG", "VI", "VN", "VU", "WS", "XK", "YE", "YT", "ZA",
	"ZM", "ZW",
];

/// An English name for a country code, for folder names. Codes the table
/// does not know show as the code itself.
fn country_name(code: &str) -> &str {
	match code {
		"AD" => "Andorra",
		"AE" => "United Arab Emirates",
		"AF" => "Afghanistan",
		"AG" => "Antigua and Barbuda",
		"AI" => "Anguilla",
		"AL" => "Albania",
		"AM" => "Armenia",
		"AO" => "Angola",
		"AQ" => "Antarctica",
		"AR" => "Argentina",
		"AS" => "American Samoa",
		"AT" => "Austria",
		"AU" => "Australia",
		"AW" => "Aruba",
		"AX" => "Åland",
		"AZ" => "Azerbaijan",
		"BA" => "Bosnia and Herzegovina",
		"BB" => "Barbados",
		"BD" => "Bangladesh",
		"BE" => "Belgium",
		"BF" => "Burkina Faso",
		"BG" => "Bulgaria",
		"BH" => "Bahrain",
		"BI" => "Burundi",
		"BJ" => "Benin",
		"BM" => "Bermuda",
		"BN" => "Brunei",
		"BO" => "Bolivia",
		"BQ" => "Bonaire",
		"BR" => "Brazil",
		"BS" => "Bahamas",
		"BT" => "Bhutan",
		"BW" => "Botswana",
		"BY" => "Belarus",
		"BZ" => "Belize",
		"CA" => "Canada",
		"CC" => "Cocos Islands",
		"CD" => "Congo (DRC)",
		"CF" => "Central African Republic",
		"CG" => "Congo",
		"CH" => "Switzerland",
		"CI" => "Côte d'Ivoire",
		"CL" => "Chile",
		"CM" => "Cameroon",
		"CN" => "China",
		"CO" => "Colombia",
		"CR" => "Costa Rica",
		"CU" => "Cuba",
		"CV" => "Cabo Verde",
		"CW" => "Curaçao",
		"CY" => "Cyprus",
		"CZ" => "Czechia",
		"DE" => "Germany",
		"DJ" => "Djibouti",
		"DK" => "Denmark",
		"DM" => "Dominica",
		"DO" => "Dominican Republic",
		"DZ" => "Algeria",
		"EC" => "Ecuador",
		"EE" => "Estonia",
		"EG" => "Egypt",
		"ER" => "Eritrea",
		"ES" => "Spain",
		"ET" => "Ethiopia",
		"FI" => "Finland",
		"FJ" => "Fiji",
		"FM" => "Micronesia",
		"FO" => "Faroe Islands",
		"FR" => "France",
		"GA" => "Gabon",
		"GB" => "United Kingdom",
		"GD" => "Grenada",
		"GE" => "Georgia",
		"GF" => "French Guiana",
		"GG" => "Guernsey",
		"GH" => "Ghana",
		"GI" => "Gibraltar",
		"GL" => "Greenland",
		"GM" => "Gambia",
		"GN" => "Guinea",
		"GP" => "Guadeloupe",
		"GQ" => "Equatorial Guinea",
		"GR" => "Greece",
		"GT" => "Guatemala",
		"GU" => "Guam",
		"GW" => "Guinea-Bissau",
		"GY" => "Guyana",
		"HK" => "Hong Kong",
		"HN" => "Honduras",
		"HR" => "Croatia",
		"HT" => "Haiti",
		"HU" => "Hungary",
		"ID" => "Indonesia",
		"IE" => "Ireland",
		"IL" => "Israel",
		"IM" => "Isle of Man",
		"IN" => "India",
		"IQ" => "Iraq",
		"IR" => "Iran",
		"IS" => "Iceland",
		"IT" => "Italy",
		"JE" => "Jersey",
		"JM" => "Jamaica",
		"JO" => "Jordan",
		"JP" => "Japan",
		"KE" => "Kenya",
		"KG" => "Kyrgyzstan",
		"KH" => "Cambodia",
		"KI" => "Kiribati",
		"KM" => "Comoros",
		"KN" => "Saint Kitts and Nevis",
		"KP" => "North Korea",
		"KR" => "South Korea",
		"KW" => "Kuwait",
		"KY" => "Cayman Islands",
		"KZ" => "Kazakhstan",
		"LA" => "Laos",
		"LB" => "Lebanon",
		"LC" => "Saint Lucia",
		"LI" => "Liechtenstein",
		"LK" => "Sri Lanka",
		"LR" => "Liberia",
		"LS" => "Lesotho",
		"LT" => "Lithuania",
		"LU" => "Luxembourg",
		"LV" => "Latvia",
		"LY" => "Libya",
		"MA" => "Morocco",
		"MC" => "Monaco",
		"MD" => "Moldova",
		"ME" => "Montenegro",
		"MF" => "Saint Martin",
		"MG" => "Madagascar",
		"MH" => "Marshall Islands",
		"MK" => "North Macedonia",
		"ML" => "Mali",
		"MM" => "Myanmar",
		"MN" => "Mongolia",
		"MO" => "Macao",
		"MQ" => "Martinique",
		"MR" => "Mauritania",
		"MT" => "Malta",
		"MU" => "Mauritius",
		"MV" => "Maldives",
		"MW" => "Malawi",
		"MX" => "Mexico",
		"MY" => "Malaysia",
		"MZ" => "Mozambique",
		"NA" => "Namibia",
		"NC" => "New Caledonia",
		"NE" => "Niger",
		"NG" => "Nigeria",
		"NI" => "Nicaragua",
		"NL" => "Netherlands",
		"NO" => "Norway",
		"NP" => "Nepal",
		"NZ" => "New Zealand",
		"OM" => "Oman",
		"PA" => "Panama",
		"PE" => "Peru",
		"PF" => "French Polynesia",
		"PG" => "Papua New Guinea",
		"PH" => "Philippines",
		"PK" => "Pakistan",
		"PL" => "Poland",
		"PM" => "Saint Pierre and Miquelon",
		"PR" => "Puerto Rico",
		"PS" => "Palestine",
		"PT" => "Portugal",
		"PW" => "Palau",
		"PY" => "Paraguay",
		"QA" => "Qatar",
		"RE" => "Réunion",
		"RO" => "Romania",
		"RS" => "Serbia",
		"RU" => "Russia",
		"RW" => "Rwanda",
		"SA" => "Saudi Arabia",
		"SB" => "Solomon Islands",
		"SC" => "Seychelles",
		"SD" => "Sudan",
		"SE" => "Sweden",
		"SG" => "Singapore",
		"SH" => "Saint Helena",
		"SI" => "Slovenia",
		"SK" => "Slovakia",
		"SM" => "San Marino",
		"SN" => "Senegal",
		"SO" => "Somalia",
		"SR" => "Suriname",
		"SS" => "South Sudan",
		"ST" => "São Tomé and Príncipe",
		"SV" => "El Salvador",
		"SX" => "Sint Maarten",
		"SY" => "Syria",
		"SZ" => "Eswatini",
		"TC" => "Turks and Caicos",
		"TD" => "Chad",
		"TG" => "Togo",
		"TH" => "Thailand",
		"TJ" => "Tajikistan",
		"TL" => "Timor-Leste",
		"TM" => "Turkmenistan",
		"TN" => "Tunisia",
		"TO" => "Tonga",
		"TR" => "Türkiye",
		"TT" => "Trinidad and Tobago",
		"TW" => "Taiwan",
		"TZ" => "Tanzania",
		"UA" => "Ukraine",
		"UG" => "Uganda",
		"US" => "United States",
		"UY" => "Uruguay",
		"UZ" => "Uzbekistan",
		"VC" => "Saint Vincent and the Grenadines",
		"VE" => "Venezuela",
		"VG" => "British Virgin Islands",
		"VI" => "U.S. Virgin Islands",
		"VN" => "Vietnam",
		"VU" => "Vanuatu",
		"WS" => "Samoa",
		"XK" => "Kosovo",
		"YE" => "Yemen",
		"YT" => "Mayotte",
		"ZA" => "South Africa",
		"ZM" => "Zambia",
		"ZW" => "Zimbabwe",
		other => other,
	}
}

#[derive(Deserialize)]
struct Record {
	id: String,
	name: String,
	#[serde(default)]
	status: String,
	#[serde(default)]
	tags: Vec<String>,
	#[serde(default)]
	languages: Vec<String>,
	#[serde(default)]
	homepage: Option<String>,
	#[serde(default)]
	streams: Vec<StreamRec>,
}

#[derive(Deserialize)]
struct StreamRec {
	url: String,
	#[serde(default)]
	codec: Option<String>,
	#[serde(default)]
	bitrate_kbps: Option<u32>,
	#[serde(default)]
	role: Option<String>,
}

/// The stream a record plays: its primary, else the first. One station
/// row per record; alternates are the dataset's mirrors, not channels.
fn stream_of(r: &Record) -> Option<&StreamRec> {
	r.streams
		.iter()
		.find(|s| s.role.as_deref() == Some("primary"))
		.or_else(|| r.streams.first())
}

fn station_of(country: &str, r: Record) -> Option<Station> {
	// Unknown status values count as retired, per the data model.
	if r.status != "active" {
		return None;
	}
	let stream = stream_of(&r)?;
	let codec = stream.codec.as_deref().map(str::to_uppercase);
	let bitrate_kbps = stream.bitrate_kbps;
	let url = stream.url.clone();
	let facts: Vec<String> = [
		r.tags.first().map(|t| capitalized(t)),
		country_name(country).to_owned().into(),
		match (codec.as_deref(), bitrate_kbps) {
			(Some(c), Some(b)) => Some(format!("{c} {b}k")),
			(Some(c), None) => Some(c.to_owned()),
			_ => None,
		},
	]
	.into_iter()
	.flatten()
	.collect();
	Some(Station {
		id: format!("moondust:{}", r.id),
		name: r.name,
		url,
		source: Source::Moondust,
		info: (!facts.is_empty()).then(|| facts.join(", ")),
		details: Details {
			genre: r.tags.first().map(|t| capitalized(t)),
			country: Some(country_name(country).to_owned()),
			language: r.languages.into_iter().next(),
			codec,
			bitrate_kbps,
			homepage: r.homepage,
			..Default::default()
		},
	})
}

fn capitalized(tag: &str) -> String {
	let mut chars = tag.chars();
	chars
		.next()
		.map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect())
}

/// One country's records as stations, in file order (the dataset sorts
/// by id; names follow as they come).
fn stations_of(country: &str, body: &str) -> Vec<Station> {
	body.lines()
		.filter_map(|line| serde_json::from_str::<Record>(line).ok())
		.filter_map(|r| station_of(country, r))
		.collect()
}

/// One page of a country's stations, with a Load More row while more
/// of the file is left.
fn page(country: &str, stations: &[Station], offset: usize) -> Vec<Node> {
	let mut nodes: Vec<Node> = stations
		.iter()
		.skip(offset)
		.take(PAGE)
		.cloned()
		.map(Node::Station)
		.collect();
	if offset + PAGE < stations.len() {
		nodes.push(Node::LoadMore {
			id: format!("moondust/{country}@{}", offset + PAGE),
		});
	}
	nodes
}

pub struct Moondust {
	/// The dataset root, no trailing slash: `<base>/v1/stations/CA.ndjson`.
	base: String,
}

impl Moondust {
	pub fn new(base: String) -> Moondust {
		Moondust {
			base: base.trim_end_matches('/').to_owned(),
		}
	}

	fn countries() -> Vec<Node> {
		let mut rows: Vec<(&str, &str)> = COUNTRIES
			.iter()
			.map(|c| (*c, country_name(c)))
			.collect();
		rows.sort_by_key(|(_, name)| name.to_lowercase());
		rows.into_iter()
			.map(|(code, name)| folder(format!("moondust/{code}"), name))
			.collect()
	}

	async fn file(&self, client: &reqwest::Client, code: &str) -> Result<String> {
		let url = format!("{}/v1/stations/{code}.ndjson", self.base);
		let bytes = crate::net::get_bytes(client, &url).await?;
		Ok(String::from_utf8_lossy(&bytes).into_owned())
	}

	pub async fn children(&self, client: &reqwest::Client, id: &str) -> Result<Vec<Node>> {
		let (head, tail) = id.split_once('@').unwrap_or((id, ""));
		let offset: usize = tail.parse().unwrap_or(0);
		match head {
			"moondust" => Ok(Self::countries()),
			_ => {
				let Some(code) = head.strip_prefix("moondust/") else {
					return Ok(vec![]);
				};
				let code = percent_decode_str(code).decode_utf8_lossy().into_owned();
				let body = self.file(client, &code).await?;
				Ok(page(&code, &stations_of(&code, &body), offset))
			}
		}
	}

	/// By name across the largest country files; a query with a country
	/// filter reads that country's file only.
	pub async fn search(&self, client: &reqwest::Client, q: &SearchQuery) -> Result<Vec<Station>> {
		let codes: Vec<&str> = match &q.country {
			Some(want) => COUNTRIES
				.iter()
				.copied()
				.filter(|c| country_name(c).eq_ignore_ascii_case(want))
				.collect(),
			None => SEARCH_FILES.to_vec(),
		};
		let mut out = Vec::new();
		for code in codes {
			if let Ok(body) = self.file(client, code).await {
				out.extend(stations_of(code, &body));
			}
		}
		Ok(out)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	const CA: &str = concat!(
		r#"{"country":"CA","id":"st_aaa","name":"Alpha FM","status":"active","tags":["jazz"],"languages":["en"],"homepage":"https://alpha.example","streams":[{"codec":"mp3","bitrate_kbps":128,"role":"primary","url":"https://a.example/live.mp3"},{"codec":"mp3","role":"alternate","url":"https://a.example/alt.mp3"}]}"#,
		"\n",
		r#"{"country":"CA","id":"st_bbb","name":"Beta Radio","status":"retired","streams":[{"role":"primary","url":"https://b.example/live"}]}"#,
		"\n",
		r#"{"country":"CA","id":"st_ccc","name":"Gamma","status":"active","streams":[]}"#,
		"\n",
		"not json\n",
	);

	#[test]
	fn records_read_as_published() {
		let got = stations_of("CA", CA);
		assert_eq!(got.len(), 1, "retired, streamless and broken rows are out");
		let s = &got[0];
		assert_eq!(s.id, "moondust:st_aaa");
		assert_eq!(s.name, "Alpha FM");
		assert_eq!(s.url, "https://a.example/live.mp3", "the primary stream plays");
		assert_eq!(s.source, Source::Moondust);
		assert_eq!(s.info.as_deref(), Some("Jazz, Canada, MP3 128k"));
		assert_eq!(s.details.homepage.as_deref(), Some("https://alpha.example"));
	}

	#[test]
	fn countries_list_by_name_with_codes() {
		let nodes = Moondust::countries();
		assert_eq!(nodes.len(), COUNTRIES.len());
		assert!(matches!(&nodes[0], Node::Folder { name, .. } if name == "Afghanistan"));
		let ca = nodes
			.iter()
			.find(|n| matches!(n, Node::Folder { id, .. } if id == "moondust/CA"));
		assert!(matches!(ca, Some(Node::Folder { name, .. }) if name == "Canada"));
	}

	#[test]
	fn pages_carry_a_load_more_row() {
		let stations: Vec<Station> = (0..PAGE + 1)
			.map(|i| Station {
				id: format!("moondust:st_{i}"),
				name: format!("S{i}"),
				url: "http://x".into(),
				source: Source::Moondust,
				info: None,
				details: Default::default(),
			})
			.collect();
		let first = page("US", &stations, 0);
		assert_eq!(first.len(), PAGE + 1);
		assert!(matches!(first.last(), Some(Node::LoadMore { id }) if id == "moondust/US@500"));
		let rest = page("US", &stations, PAGE);
		assert_eq!(rest.len(), 1);
	}
}
