//! Signing in with Google (OpenID Connect, authorization code flow with PKCE). It is on when the
//! settings name a Google OAuth client (`google_client_id`) and the environment has its secret
//! (`FIGCRAFT_GOOGLE_CLIENT_SECRET`). The ID token comes straight from Google's token endpoint over
//! TLS, so its claims are trusted without checking its signature (OpenID Connect Core 3.1.3.7).

use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use figcraft_collab_net::token;
use serde::Deserialize;
use sha2::{Digest, Sha256};

/// The environment variable holding the client secret (never in the settings file).
pub const SECRET_ENV: &str = "FIGCRAFT_GOOGLE_CLIENT_SECRET";
/// The client id can come from the environment too (when `google_client_id` is empty).
pub const ID_ENV: &str = "FIGCRAFT_GOOGLE_CLIENT_ID";

/// The client id to use: the setting, else `env` (the environment's), when it looks like one.
pub fn client_id(setting: &str, env: &str) -> Option<String> {
    let id = if setting.trim().is_empty() { env.trim() } else { setting.trim() };
    (!id.is_empty() && id.len() <= 256 && !id.contains(char::is_whitespace) && !id.contains(char::is_control)).then(|| id.to_string())
}

const AUTHORIZE: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN: &str = "https://oauth2.googleapis.com/token";

/// A Google OAuth client.
#[derive(Clone, Debug, PartialEq)]
pub struct Client {
    pub id: String,
    pub secret: String,
    /// Where Google sends the browser back: `<web_url>auth/google/callback`.
    pub redirect: String,
}

/// A sign-in on its way through Google.
#[derive(Clone, Debug, PartialEq)]
pub struct Started {
    /// Google's page to send the browser to.
    pub url: String,
    /// Ties the answer to this browser (also kept in a cookie).
    pub state: String,
    /// The PKCE secret, sent with the code.
    pub verifier: String,
}

/// Who Google says signed in.
#[derive(Clone, Debug, PartialEq)]
pub struct Identity {
    pub sub: String,
    pub email: String,
    pub name: String,
}

/// Start a sign-in.
pub fn start(client: &Client) -> Result<Started, String> {
    let state = token::secret()?;
    let verifier = format!("{}{}{}", token::secret()?, token::secret()?, token::secret()?);
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let query = form_urlencoded::Serializer::new(String::new())
        .append_pair("client_id", &client.id)
        .append_pair("redirect_uri", &client.redirect)
        .append_pair("response_type", "code")
        .append_pair("scope", "openid email profile")
        .append_pair("state", &state)
        .append_pair("code_challenge", &challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("prompt", "select_account")
        .finish();
    Ok(Started { url: format!("{AUTHORIZE}?{query}"), state, verifier })
}

#[derive(Deserialize)]
struct TokenReply {
    id_token: Option<String>,
}

/// Swap the code Google sent back for who signed in.
pub fn finish(client: &Client, code: &str, verifier: &str) -> Result<Identity, String> {
    let config = ureq::Agent::config_builder().http_status_as_error(false).timeout_global(Some(Duration::from_secs(15))).build();
    let agent = ureq::Agent::new_with_config(config);
    let mut reply = agent
        .post(TOKEN)
        .send_form([
            ("code", code),
            ("client_id", client.id.as_str()),
            ("client_secret", client.secret.as_str()),
            ("redirect_uri", client.redirect.as_str()),
            ("grant_type", "authorization_code"),
            ("code_verifier", verifier),
        ])
        .map_err(|e| format!("can't reach Google: {e}"))?;
    let status = reply.status();
    let body = reply.body_mut().with_config().limit(1 << 20).read_to_string().map_err(|e| format!("can't read Google's answer: {e}"))?;
    if !status.is_success() {
        log::warn!("Google sign-in: the token endpoint answered {status}");
        return Err("Google didn't accept the sign-in. Try again.".into());
    }
    let reply: TokenReply = serde_json::from_str(&body).map_err(|_| "Google's answer can't be read.".to_string())?;
    let id_token = reply.id_token.ok_or("Google's answer has no ID token.")?;
    identity(&id_token, &client.id, crate::store::now())
}

#[derive(Deserialize)]
struct Claims {
    iss: String,
    aud: String,
    sub: String,
    exp: u64,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    email_verified: Option<bool>,
    #[serde(default)]
    name: Option<String>,
}

/// The identity in an ID token from Google's token endpoint, checked for `client` at time `now`.
pub fn identity(id_token: &str, client: &str, now: u64) -> Result<Identity, String> {
    let payload = id_token.split('.').nth(1).ok_or("The ID token can't be read.")?;
    let bytes = URL_SAFE_NO_PAD.decode(payload.trim_end_matches('=')).map_err(|_| "The ID token can't be read.".to_string())?;
    let c: Claims = serde_json::from_slice(&bytes).map_err(|_| "The ID token can't be read.".to_string())?;
    if c.iss != "https://accounts.google.com" && c.iss != "accounts.google.com" {
        return Err("The ID token isn't from Google.".into());
    }
    if c.aud != client {
        return Err("The ID token is for another app.".into());
    }
    if c.exp <= now {
        return Err("The sign-in took too long. Try again.".into());
    }
    if c.sub.is_empty() || c.sub.len() > 255 {
        return Err("The ID token can't be read.".into());
    }
    let email = c.email.filter(|_| c.email_verified == Some(true)).ok_or("Google hasn't verified this account's email.")?;
    Ok(Identity { sub: c.sub, email, name: c.name.unwrap_or_default() })
}
