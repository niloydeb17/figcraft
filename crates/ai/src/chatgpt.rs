//! Sign in with ChatGPT for an app that runs on the user's own machine.
//!
//! The flow, as OpenAI documents it for open-source clients:
//! 1. open `authorize` in the browser with `client_id=dynamic_agent_client` (first time) and a
//!    callback on `http://127.0.0.1:<port>/auth/callback`;
//! 2. the callback brings a `code` and the `client_id` OpenAI registered for this app;
//! 3. the `code` and the PKCE verifier are exchanged at the token endpoint for an access token (one
//!    hour), a refresh token (30 days, replaced on every refresh) and an ID token;
//! 4. the access token is sent as a bearer token to `https://api.openai.com/v1`.

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::{AiError, http, percent, pkce, store::Credentials};

pub const DYNAMIC_CLIENT: &str = "dynamic_agent_client";
const AUTHORIZE_URL: &str = "https://auth.openai.com/api/accounts/authorize";
const TOKEN_URL: &str = "https://auth.openai.com/api/accounts/oauth/token";
/// The resource the tokens are for: OpenAI's public API.
pub const RESOURCE: &str = "https://api.openai.com/v1";
/// Identity, a refresh token, and the plan-backed use of the API (not an API key).
pub const SCOPE: &str = "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct";
/// The scope that lets a token be used for inference: without it nothing can be sent.
pub const USE_SCOPE: &str = "chatgpt.tokens.use.direct";
/// OpenAI's documented example port; any free port will do when it is taken.
const PREFERRED_PORT: u16 = 1455;
const CALLBACK_PATH: &str = "/auth/callback";

/// The parts of an authorization request.
pub struct AuthorizeParams<'a> {
    pub client_id: &'a str,
    /// The app's name, sent only on the first sign-in (registration).
    pub agent_name_hint: Option<&'a str>,
    /// Stands for this installation: a distinct value for every machine.
    pub host_id: &'a str,
    pub redirect_uri: &'a str,
    pub state: &'a str,
    pub nonce: &'a str,
    pub code_challenge: &'a str,
}

/// The URL to open in the browser.
pub fn authorize_url(p: &AuthorizeParams<'_>) -> String {
    let mut q: Vec<(&str, &str)> = vec![("client_id", p.client_id)];
    if let Some(name) = p.agent_name_hint {
        q.push(("agent_name_hint", name));
    }
    q.extend([
        ("ext_agent_host_id", p.host_id),
        ("response_type", "code"),
        ("redirect_uri", p.redirect_uri),
        ("scope", SCOPE),
        ("resource", RESOURCE),
        ("state", p.state),
        ("nonce", p.nonce),
        ("code_challenge_method", "S256"),
        ("code_challenge", p.code_challenge),
    ]);
    format!("{AUTHORIZE_URL}?{}", percent::form(&q))
}

/// What the browser brings back to the callback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Callback {
    pub code: String,
    /// The client OpenAI registered for this app (first sign-in).
    pub client_id: Option<String>,
}

/// `Some(outcome)` for a request to the callback path, `None` for any other (a favicon, a probe).
fn parse_callback(target: &str, state: &str) -> Option<Result<Callback, AiError>> {
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    if path != CALLBACK_PATH {
        return None;
    }
    let params = percent::parse_query(query);
    let get = |k: &str| params.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
    if let Some(err) = get("error") {
        let why = get("error_description").unwrap_or_default();
        return Some(Err(AiError::Auth(format!("{err} {why}").trim().to_string())));
    }
    if get("state").as_deref() != Some(state) {
        return Some(Err(AiError::Auth("the callback's state did not match: start the sign-in again".into())));
    }
    Some(match get("code") {
        Some(code) if !code.is_empty() => Ok(Callback { code, client_id: get("client_id").filter(|c| !c.is_empty()) }),
        _ => Err(AiError::Auth("the callback had no code".into())),
    })
}

/// The listener on `127.0.0.1` that the browser is sent back to.
pub struct Loopback {
    listener: TcpListener,
    port: u16,
}

impl Loopback {
    /// Listen on the documented port, or on any free one.
    pub fn bind() -> Result<Self, AiError> {
        let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, PREFERRED_PORT)))
            .or_else(|_| TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))))
            .map_err(|e| AiError::Auth(format!("could not listen on 127.0.0.1: {e}")))?;
        let port = listener.local_addr().map_err(|e| AiError::Auth(e.to_string()))?.port();
        listener.set_nonblocking(true).map_err(|e| AiError::Auth(e.to_string()))?;
        Ok(Self { listener, port })
    }

    /// The callback URL (`127.0.0.1`, never `localhost`, which OpenAI refuses).
    pub fn redirect_uri(&self) -> String {
        format!("http://127.0.0.1:{}{CALLBACK_PATH}", self.port)
    }

    /// Wait for the browser's callback, answering it with a page that says it can be closed.
    pub fn wait(&self, state: &str, timeout: Duration) -> Result<Callback, AiError> {
        let deadline = Instant::now() + timeout;
        loop {
            match self.listener.accept() {
                Ok((stream, _)) => {
                    if let Some(outcome) = handle(stream, state) {
                        return outcome;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        return Err(AiError::Timeout);
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(e) => return Err(AiError::Auth(e.to_string())),
            }
        }
    }
}

/// Read one request and answer it; `Some` when it was the callback.
fn handle(mut stream: TcpStream, state: &str) -> Option<Result<Callback, AiError>> {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    while buf.len() < 16 * 1024 && !buf.windows(4).any(|w| w == b"\r\n\r\n") {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => buf.extend_from_slice(chunk.get(..n).unwrap_or(&[])),
        }
    }
    let head = String::from_utf8_lossy(&buf);
    let target = head.lines().next().and_then(|l| l.strip_prefix("GET ")).and_then(|l| l.split(' ').next()).unwrap_or("");
    let outcome = parse_callback(target, state);
    let (status, text) = match &outcome {
        Some(Ok(_)) => ("200 OK", "You are signed in to FigCraft. You can close this tab."),
        Some(Err(_)) => ("400 Bad Request", "The sign-in did not complete. Go back to FigCraft and try again."),
        None => ("404 Not Found", "Not found."),
    };
    let page = format!("<!doctype html><meta charset=utf-8><title>FigCraft</title><body style=\"font-family:system-ui;margin:3em\"><p>{text}</p>");
    let reply =
        format!("HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}", page.len());
    let _ = stream.write_all(reply.as_bytes());
    outcome
}

/// A token response. Only `access_token` is required; the rest are kept when present.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Tokens {
    pub access_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_type: Option<String>,
    /// Seconds until the access token expires (3600).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_in: Option<u64>,
    /// The scopes granted, space-separated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
}

// Never print a token: debugging output names the fields only.
impl std::fmt::Debug for Tokens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tokens")
            .field("access_token", &"…")
            .field("refresh_token", &self.refresh_token.as_ref().map(|_| "…"))
            .field("expires_in", &self.expires_in)
            .field("scope", &self.scope)
            .finish()
    }
}

impl Tokens {
    /// Whether the plan-backed use of the API was granted (identity alone sends nothing).
    pub fn can_use_api(&self) -> bool {
        self.scope.as_deref().is_some_and(|s| s.split_whitespace().any(|x| x == USE_SCOPE))
    }
}

fn parse_tokens(reply: http::Reply) -> Result<Tokens, AiError> {
    let reply = http::ok(reply)?;
    serde_json::from_str(&reply.body).map_err(|e| AiError::Reply(format!("the token reply: {e}")))
}

/// Trade the callback's `code` for tokens.
pub fn exchange_code(client_id: &str, code: &str, verifier: &str, redirect_uri: &str) -> Result<Tokens, AiError> {
    let body = percent::form(&[
        ("grant_type", "authorization_code"),
        ("client_id", client_id),
        ("code", code),
        ("code_verifier", verifier),
        ("redirect_uri", redirect_uri),
        ("resource", RESOURCE),
    ]);
    parse_tokens(http::post_form(TOKEN_URL, &body)?)
}

/// New tokens for `refresh_token`. The reply carries a replacement refresh token: keep it, the old
/// one is spent.
pub fn refresh(client_id: &str, refresh_token: &str) -> Result<Tokens, AiError> {
    let body = percent::form(&[("grant_type", "refresh_token"), ("client_id", client_id), ("refresh_token", refresh_token), ("resource", RESOURCE)]);
    parse_tokens(http::post_form(TOKEN_URL, &body)?)
}

/// Open `url` in the user's default browser. False when none could be started.
pub fn open_in_browser(url: &str) -> bool {
    webbrowser::open(url).is_ok()
}

/// What a sign-in needs from its caller.
pub struct SignIn<'a> {
    /// The name OpenAI shows on the consent screen.
    pub app_name: &'a str,
    /// This installation's host id (see [`crate::store::host_id`]).
    pub host_id: &'a str,
    /// Sign-in already done on this machine: its client is reused.
    pub existing: Option<&'a Credentials>,
    /// Show `url` to the user (open the browser).
    pub open: &'a dyn Fn(&str),
    pub timeout: Duration,
}

/// Sign in: open the browser, wait for the callback and trade its code for tokens. Returns the
/// credentials to store.
pub fn sign_in(opts: &SignIn<'_>) -> Result<Credentials, AiError> {
    let host_id = opts.host_id.to_string();
    let known = opts.existing.map(|c| c.client_id.as_str());
    let client_id = known.unwrap_or(DYNAMIC_CLIENT);
    let (pkce, state, nonce) = (pkce::generate()?, pkce::random_token(16)?, pkce::random_token(16)?);
    let loopback = Loopback::bind()?;
    let redirect_uri = loopback.redirect_uri();
    let url = authorize_url(&AuthorizeParams {
        client_id,
        agent_name_hint: known.is_none().then_some(opts.app_name),
        host_id: &host_id,
        redirect_uri: &redirect_uri,
        state: &state,
        nonce: &nonce,
        code_challenge: &pkce.challenge,
    });
    (opts.open)(&url);
    let callback = loopback.wait(&state, opts.timeout)?;
    // The first sign-in is told which client OpenAI registered; later ones reuse it.
    let client_id = callback
        .client_id
        .clone()
        .or_else(|| known.map(str::to_string))
        .ok_or_else(|| AiError::Auth("OpenAI did not say which client it registered for this app (no client_id in the callback)".into()))?;
    let tokens = exchange_code(&client_id, &callback.code, &pkce.verifier, &redirect_uri)?;
    if !tokens.can_use_api() {
        return Err(AiError::Auth(format!(
            "the sign-in did not grant `{USE_SCOPE}`: granted scopes were {:?}",
            tokens.scope.as_deref().unwrap_or("")
        )));
    }
    Ok(Credentials::new(client_id, host_id, tokens))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> AuthorizeParams<'static> {
        AuthorizeParams {
            client_id: DYNAMIC_CLIENT,
            agent_name_hint: Some("FigCraft"),
            host_id: "urn:uuid:11111111-2222-4333-8444-555555555555",
            redirect_uri: "http://127.0.0.1:1455/auth/callback",
            state: "st",
            nonce: "no",
            code_challenge: "chal",
        }
    }

    #[test]
    fn the_authorize_url_carries_every_documented_parameter() {
        let url = authorize_url(&params());
        assert!(url.starts_with("https://auth.openai.com/api/accounts/authorize?"));
        let q = percent::parse_query(url.split_once('?').unwrap().1);
        let get = |k: &str| q.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
        assert_eq!(get("client_id"), Some("dynamic_agent_client"));
        assert_eq!(get("agent_name_hint"), Some("FigCraft"));
        assert_eq!(get("ext_agent_host_id"), Some("urn:uuid:11111111-2222-4333-8444-555555555555"));
        assert_eq!(get("response_type"), Some("code"));
        assert_eq!(get("redirect_uri"), Some("http://127.0.0.1:1455/auth/callback"));
        assert_eq!(get("scope"), Some("openid profile email offline_access resource.invoke chatgpt.tokens.use.direct"));
        assert_eq!(get("resource"), Some("https://api.openai.com/v1"));
        assert_eq!((get("code_challenge_method"), get("code_challenge")), (Some("S256"), Some("chal")));
        // The name is for the first sign-in only.
        let again = authorize_url(&AuthorizeParams { client_id: "oaiapp_x", agent_name_hint: None, ..params() });
        assert!(!again.contains("agent_name_hint") && again.contains("client_id=oaiapp_x"));
    }

    #[test]
    fn a_callback_is_checked_against_the_state() {
        let ok = parse_callback("/auth/callback?code=c%2F1&state=st&client_id=oaiapp_9", "st").unwrap().unwrap();
        assert_eq!(ok, Callback { code: "c/1".into(), client_id: Some("oaiapp_9".into()) });
        assert!(matches!(parse_callback("/auth/callback?code=c&state=other", "st"), Some(Err(AiError::Auth(_)))));
        assert!(
            matches!(parse_callback("/auth/callback?error=access_denied&error_description=No", "st"), Some(Err(AiError::Auth(m))) if m.contains("access_denied"))
        );
        assert!(matches!(parse_callback("/auth/callback?state=st", "st"), Some(Err(_))));
        assert!(parse_callback("/favicon.ico", "st").is_none());
    }

    #[test]
    fn the_loopback_answers_the_browser_and_hands_over_the_code() {
        let lb = Loopback::bind().unwrap();
        assert!(lb.redirect_uri().starts_with("http://127.0.0.1:") && lb.redirect_uri().ends_with("/auth/callback"));
        let port = lb.port;
        let browser = std::thread::spawn(move || {
            let get = |target: &str| {
                let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
                s.write_all(format!("GET {target} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").as_bytes()).unwrap();
                let mut reply = String::new();
                let _ = s.read_to_string(&mut reply);
                reply
            };
            // A stray request first: it is answered and ignored.
            assert!(get("/favicon.ico").starts_with("HTTP/1.1 404"));
            get("/auth/callback?code=abc&state=st&client_id=oaiapp_1")
        });
        let cb = lb.wait("st", Duration::from_secs(10)).unwrap();
        assert_eq!(cb.code, "abc");
        assert_eq!(cb.client_id.as_deref(), Some("oaiapp_1"));
        let page = browser.join().unwrap();
        assert!(page.starts_with("HTTP/1.1 200") && page.contains("You can close this tab"));
    }

    #[test]
    fn waiting_gives_up_when_no_browser_comes() {
        let lb = Loopback::bind().unwrap();
        assert!(matches!(lb.wait("st", Duration::from_millis(150)), Err(AiError::Timeout)));
    }

    #[test]
    fn tokens_parse_and_never_print() {
        let t: Tokens = serde_json::from_str(r#"{"access_token":"SECRET-A","refresh_token":"SECRET-R","token_type":"Bearer","expires_in":3600,"scope":"openid chatgpt.tokens.use.direct","earliest_refresh_at":1}"#).unwrap();
        assert!(t.can_use_api());
        let shown = format!("{t:?}");
        assert!(!shown.contains("SECRET"), "{shown}");
        let identity_only: Tokens = serde_json::from_str(r#"{"access_token":"a","scope":"openid profile"}"#).unwrap();
        assert!(!identity_only.can_use_api());
    }
}
