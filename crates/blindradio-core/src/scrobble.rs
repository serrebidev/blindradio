use crate::{CoreError, Result, net};

/// The hosted ListenBrainz; a self-hosted server, or one that speaks its API (Maloja, Koito),
/// replaces it in settings.
pub const LISTENBRAINZ: &str = "https://api.listenbrainz.org";

const LIBREFM: &str = "https://libre.fm/2.0/";
/// Libre.fm (GNU FM) checks only that the key and the signature are 32 characters long.
const LIBREFM_KEY: &str = "b11ad7ad10b11ad7ad10b11ad7ad10ff";
const LIBREFM_SECRET: &str = "b11ad7ad10b11ad7ad10b11ad7ad10ee";

/// The API root of a ListenBrainz server as typed: blank is the hosted one, and a trailing
/// slash or `/1` is dropped, since every call adds `/1/...`.
fn listenbrainz_root(server: Option<&str>) -> String {
	let root = server.map(str::trim).filter(|s| !s.is_empty()).unwrap_or(LISTENBRAINZ);
	let root = root.trim_end_matches('/');
	let root = root.strip_suffix("/1").unwrap_or(root);
	if root.contains("://") {
		root.to_string()
	} else {
		format!("https://{root}")
	}
}

/// ListenBrainz submit-listens body for one finished track.
fn listenbrainz_body(artist: &str, title: &str, listened_at: u64) -> serde_json::Value {
	serde_json::json!({
		"listen_type": "single",
		"payload": [{
			"listened_at": listened_at,
			"track_metadata": {
				"artist_name": artist,
				"track_name": title,
				"additional_info": {
					"submission_client": "BlindRadio",
					"submission_client_version": env!("CARGO_PKG_VERSION"),
				},
			},
		}],
	})
}

/// A refused ListenBrainz call: its own message when it gave one, else the HTTP status.
async fn listenbrainz_reply(response: reqwest::Response) -> Result<serde_json::Value> {
	let status = response.status();
	let body: serde_json::Value = response.json().await.unwrap_or_default();
	if status.is_success() {
		return Ok(body);
	}
	let message = body["error"].as_str().map(str::to_string);
	Err(CoreError::Http(message.unwrap_or_else(|| status.to_string())))
}

/// Scrobbles one listen to a ListenBrainz server with the user's token.
pub async fn listenbrainz(
	server: Option<&str>,
	token: &str,
	artist: &str,
	title: &str,
	listened_at: u64,
) -> Result<()> {
	let response = net::client()
		.post(format!("{}/1/submit-listens", listenbrainz_root(server)))
		.header("Authorization", format!("Token {}", token.trim()))
		.json(&listenbrainz_body(artist, title, listened_at))
		.send()
		.await
		.map_err(net::http_err)?;
	listenbrainz_reply(response).await.map(drop)
}

/// Checks a ListenBrainz token on a server; the user name it belongs to.
pub async fn listenbrainz_user(server: Option<&str>, token: &str) -> Result<String> {
	let response = net::client()
		.get(format!("{}/1/validate-token", listenbrainz_root(server)))
		.header("Authorization", format!("Token {}", token.trim()))
		.send()
		.await
		.map_err(net::http_err)?;
	let body = listenbrainz_reply(response).await?;
	match body["user_name"].as_str() {
		Some(name) if body["valid"] == true => Ok(name.to_string()),
		_ => Err(CoreError::Http(
			body["message"].as_str().unwrap_or("Token invalid.").to_string(),
		)),
	}
}

/// Audioscrobbler signature: md5 of the parameters sorted by name, each as name+value with
/// no separator, plus the secret.
fn signature(params: &[(&str, String)], secret: &str) -> String {
	let mut sorted: Vec<&(&str, String)> = params.iter().collect();
	sorted.sort_by(|a, b| a.0.cmp(b.0));
	let mut plain = String::new();
	for (k, v) in sorted {
		plain.push_str(k);
		plain.push_str(v);
	}
	plain.push_str(secret);
	format!("{:x}", md5::compute(plain))
}

/// GNU FM's login token: md5 of the lowercased user name and md5 of the password.
fn librefm_auth_token(user: &str, password: &str) -> String {
	let hashed = format!("{:x}", md5::compute(password));
	format!("{:x}", md5::compute(format!("{}{hashed}", user.to_lowercase())))
}

/// One signed Libre.fm call. It answers errors with HTTP 200 and an `error` field.
async fn librefm_call(mut params: Vec<(&str, String)>) -> Result<serde_json::Value> {
	params.push(("api_key", LIBREFM_KEY.into()));
	let sig = signature(&params, LIBREFM_SECRET);
	params.push(("api_sig", sig));
	params.push(("format", "json".into()));
	let body: serde_json::Value = net::client()
		.post(LIBREFM)
		.form(&params)
		.send()
		.await
		.map_err(net::http_err)?
		.json()
		.await
		.map_err(net::http_err)?;
	if body.get("error").is_some() {
		let message = body["message"].as_str().unwrap_or("Refused");
		return Err(CoreError::Http(message.to_string()));
	}
	Ok(body)
}

/// Signs in to Libre.fm with a user name and password; the name as Libre.fm spells it and the
/// session key. The password is not kept.
pub async fn librefm_login(user: &str, password: &str) -> Result<(String, String)> {
	let user = user.trim();
	let body = librefm_call(vec![
		("method", "auth.getMobileSession".into()),
		("username", user.into()),
		("authToken", librefm_auth_token(user, password)),
	])
	.await?;
	let session = &body["session"];
	match (session["name"].as_str(), session["key"].as_str()) {
		(Some(name), Some(key)) if !key.is_empty() => Ok((name.to_string(), key.to_string())),
		_ => Err(CoreError::Http("No session".into())),
	}
}

/// Scrobbles one track to Libre.fm.
pub async fn librefm(session_key: &str, artist: &str, title: &str, timestamp: u64) -> Result<()> {
	librefm_call(vec![
		("method", "track.scrobble".into()),
		("sk", session_key.into()),
		("artist", artist.into()),
		("track", title.into()),
		("timestamp", timestamp.to_string()),
	])
	.await
	.map(drop)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn signature_matches_reference() {
		// md5("api_keykey" + "methodauth.getSession" + "tokentok" + "secret"),
		// computed independently of this crate.
		let params = [
			("token", "tok".to_string()),
			("api_key", "key".to_string()),
			("method", "auth.getSession".to_string()),
		];
		assert_eq!(signature(&params, "secret"), "04e870be4bb79756721b7bc1937fe83d");
	}

	#[test]
	fn librefm_key_passes_gnu_fm_length_check() {
		assert_eq!(LIBREFM_KEY.len(), 32);
		assert_eq!(LIBREFM_SECRET.len(), 32);
	}

	#[test]
	fn auth_token_lowercases_user() {
		// md5("bob" + md5("pw")), computed independently.
		assert_eq!(librefm_auth_token("Bob", "pw"), librefm_auth_token("bob", "pw"));
		assert_eq!(librefm_auth_token("bob", "pw"), "20b9cae31805a95d0ccc3aed5a4ea397");
	}

	#[test]
	fn listenbrainz_root_forms() {
		assert_eq!(listenbrainz_root(None), LISTENBRAINZ);
		assert_eq!(listenbrainz_root(Some("  ")), LISTENBRAINZ);
		assert_eq!(listenbrainz_root(Some("http://pi:8100/")), "http://pi:8100");
		assert_eq!(listenbrainz_root(Some("https://lb.example/1/")), "https://lb.example");
		assert_eq!(
			listenbrainz_root(Some("maloja.example/apis/listenbrainz")),
			"https://maloja.example/apis/listenbrainz"
		);
	}

	#[test]
	fn listenbrainz_body_shape() {
		let body = listenbrainz_body("A", "T", 42);
		assert_eq!(body["listen_type"], "single");
		assert_eq!(body["payload"][0]["listened_at"], 42);
		let track = &body["payload"][0]["track_metadata"];
		assert_eq!(track["artist_name"], "A");
		assert_eq!(track["track_name"], "T");
		assert_eq!(track["additional_info"]["submission_client"], "BlindRadio");
	}
}
