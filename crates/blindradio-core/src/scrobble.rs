use crate::{Result, net};

/// ListenBrainz submit-listens body for one finished (or playing) track.
fn listenbrainz_body(artist: &str, title: &str, album: Option<&str>, listened_at: u64) -> serde_json::Value {
	let mut track = serde_json::json!({
		"artist_name": artist,
		"track_name": title,
	});
	if let Some(album) = album {
		track["release_name"] = serde_json::Value::String(album.to_string());
	}
	serde_json::json!({
		"listen_type": "single",
		"payload": [{
			"listened_at": listened_at,
			"track_metadata": track,
		}],
	})
}

/// Scrobbles one listen to ListenBrainz with the user's token.
pub async fn listenbrainz(
	token: &str,
	artist: &str,
	title: &str,
	album: Option<&str>,
	listened_at: u64,
) -> Result<()> {
	net::client()
		.post("https://api.listenbrainz.org/1/submit-listens")
		.header("Authorization", format!("Token {token}"))
		.json(&listenbrainz_body(artist, title, album, listened_at))
		.send()
		.await
		.map_err(net::http_err)?
		.error_for_status()
		.map_err(net::http_err)?;
	Ok(())
}

/// last.fm API signature: md5 of the parameters sorted by name, each as
/// name+value with no separator, plus the shared secret.
fn lastfm_signature(params: &[(String, String)], secret: &str) -> String {
	let mut sorted: Vec<&(String, String)> = params.iter().collect();
	sorted.sort_by(|a, b| a.0.cmp(&b.0));
	let mut plain = String::new();
	for (k, v) in sorted {
		plain.push_str(k);
		plain.push_str(v);
	}
	plain.push_str(secret);
	format!("{:x}", md5::compute(plain))
}

/// Scrobbles one track to last.fm, or to a Libre.fm base URL when set.
pub async fn lastfm(
	base: Option<&str>,
	api_key: &str,
	secret: &str,
	session_key: &str,
	artist: &str,
	title: &str,
	timestamp: u64,
) -> Result<()> {
	let mut params: Vec<(String, String)> = vec![
		("method".into(), "track.scrobble".into()),
		("api_key".into(), api_key.into()),
		("sk".into(), session_key.into()),
		("artist".into(), artist.into()),
		("track".into(), title.into()),
		("timestamp".into(), timestamp.to_string()),
	];
	let sig = lastfm_signature(&params, secret);
	params.push(("api_sig".into(), sig));
	params.push(("format".into(), "json".into()));
	let url = format!("{}/2.0/", base.unwrap_or("https://ws.audioscrobbler.com"));
	net::client()
		.post(url)
		.form(&params)
		.send()
		.await
		.map_err(net::http_err)?
		.error_for_status()
		.map_err(net::http_err)?;
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn signature_matches_reference() {
		// md5("api_keykey" + "methodauth.getSession" + "tokentok" + "secret"),
		// computed independently of this crate.
		let params: Vec<(String, String)> = vec![
			("token".into(), "tok".into()),
			("api_key".into(), "key".into()),
			("method".into(), "auth.getSession".into()),
		];
		assert_eq!(
			lastfm_signature(&params, "secret"),
			"04e870be4bb79756721b7bc1937fe83d"
		);
	}

	#[test]
	fn listenbrainz_body_shape() {
		let body = listenbrainz_body("A", "T", Some("Al"), 42);
		assert_eq!(body["listen_type"], "single");
		assert_eq!(body["payload"][0]["listened_at"], 42);
		assert_eq!(body["payload"][0]["track_metadata"]["artist_name"], "A");
		assert_eq!(body["payload"][0]["track_metadata"]["release_name"], "Al");
		let bare = listenbrainz_body("A", "T", None, 42);
		assert!(bare["payload"][0]["track_metadata"].get("release_name").is_none());
	}
}
