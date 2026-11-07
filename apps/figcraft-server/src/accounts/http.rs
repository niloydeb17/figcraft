//! The accounts' web pages and API, on their own port (`accounts_listen`) behind the same reverse
//! proxy as the live sessions:
//!
//! | Request | What it does |
//! |---|---|
//! | `GET /login` | the sign-in page, or the signed-in account (`?next=/path` goes there instead) |
//! | `POST /login` | `email`, `password`, `next`: signs in, or makes the account when there is none |
//! | `POST /logout` | signs this browser out |
//! | `GET /auth/me` | `{"signedIn": true, "user": {id, email, name, hasPicture}}`, or 401 |
//! | `GET /auth/avatar` | the picture Google gave for this account (fetched by the server, kept a day), or 404 |
//! | `POST /auth/profile` | JSON `{name?, role?, roleOther?, experience?, plan?, createFirst?, tips?, onboarded?}`: the onboarding answers |
//! | `POST /auth/invite` | JSON `{emails: [...]}`: people to invite; `{invited, link, emailed: false}` (no email is sent yet) |
//! | `GET /auth/recents` | `{files: [{target, name, kind, opened, picture?}]}`: the account's recent files, newest first |
//! | `POST /auth/recents` | JSON `{open: {target, name, kind}}` or `{forget: target}`; the list after |
//! | `POST /auth/recents/picture` | JSON `{target, png}` (base64): the picture of a file in the list; the list after |
//! | `GET /auth/recents/picture?p=<token>` | that picture (`image/png`), or 404 |
//! | `GET /auth/folders` | `{folders: [{id, name, starred, updated, files: [{target, name, kind}]}]}`: the account's folders, newest first |
//! | `POST /auth/folders` | JSON, one of `{create: {id, name, access?, role?}}`, `{rename: {id, name}}`, `{delete: id}`, `{star: {id, on}}`, `{add: {id, file: {target, name, kind}}}`, `{remove: target}`; the folders after |
//! | `GET /auth/check` | 204 when signed in, else 401 (the proxy asks before serving the app) |
//! | `GET /auth/google` | starts signing in with Google (when it is set up) |
//! | `GET /auth/google/callback` | where Google sends the browser back |
//! | `POST /auth/admin` | the operator's `users` commands (this machine only, with the server key) |
//!
//! A tiny HTTP/1.1 server: one request per connection, small requests only.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{IpAddr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use base64::Engine as _;
use figcraft_collab::same_secret;
use figcraft_collab_net::token;
use serde::{Deserialize, Serialize};

use super::pages::{self, Form, Site};
use super::{Accounts, Lookup, User, UserRow, google, password};

/// The session cookie.
pub const COOKIE: &str = "figcraft_session";
/// Ties a Google sign-in to the browser that started it.
const GOOGLE_COOKIE: &str = "figcraft_google";

/// Longest request head and body.
const MAX_HEAD: usize = 16 << 10;
const MAX_BODY: usize = 16 << 10;
/// Longest body of `POST /auth/recents/picture`: a picture in base64, and its target.
const MAX_PICTURE_BODY: usize = (super::recents::MAX_PICTURE / 3 + 1) * 4 + (8 << 10);
/// Most connections at once.
const MAX_CONNECTIONS: usize = 64;
/// How long a request may take to arrive.
const READ_TIMEOUT: Duration = Duration::from_secs(10);
/// Google sign-ins on their way, and how long one may take.
const MAX_PENDING: usize = 10_000;
const PENDING_FOR: Duration = Duration::from_secs(600);

/// How the pages are served.
#[derive(Clone, Debug, Default)]
pub struct Settings {
    /// The site's origin (`https://figcraft.example.com`) when known: forms posted from anywhere
    /// else are refused. Unknown: the request's own `Host`.
    pub origin: Option<String>,
    /// Cookies only over HTTPS.
    pub secure: bool,
    /// Seconds a session lasts unused (the cookie's lifetime).
    pub idle: u64,
    pub google: Option<google::Client>,
    pub site: Site,
    /// The server key (the operator's requests carry it).
    pub key: String,
}

/// A Google sign-in on its way.
struct Pending {
    verifier: String,
    next: String,
    at: Instant,
}

/// A picture fetched from Google: when, the bytes and their media type.
type Avatar = (Instant, Vec<u8>, String);

/// What every request shares.
pub struct Ctx {
    pub accounts: Arc<Mutex<Accounts>>,
    pub settings: Settings,
    pending: Mutex<HashMap<String, Pending>>,
    /// Pictures fetched from Google, by account: when, the bytes and their media type.
    avatars: Mutex<HashMap<String, Avatar>>,
}

impl Ctx {
    pub fn new(accounts: Arc<Mutex<Accounts>>, settings: Settings) -> Ctx {
        Ctx { accounts, settings, pending: Mutex::new(HashMap::new()), avatars: Mutex::new(HashMap::new()) }
    }

    fn accounts(&self) -> MutexGuard<'_, Accounts> {
        self.accounts.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// A request.
#[derive(Clone, Debug, Default)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub query: String,
    /// Lowercase names.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub peer: Option<IpAddr>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
    }

    pub fn cookie(&self, name: &str) -> Option<String> {
        self.headers
            .iter()
            .filter(|(n, _)| n == "cookie")
            .flat_map(|(_, v)| v.split(';'))
            .filter_map(|c| c.trim().split_once('='))
            .find(|(n, _)| *n == name)
            .map(|(_, v)| v.trim().to_string())
    }

    fn query_value(&self, name: &str) -> Option<String> {
        form_urlencoded::parse(self.query.as_bytes()).find(|(k, _)| k == name).map(|(_, v)| v.into_owned())
    }

    fn form(&self) -> HashMap<String, String> {
        form_urlencoded::parse(&self.body).map(|(k, v)| (k.into_owned(), v.into_owned())).collect()
    }
}

/// A response.
#[derive(Clone, Debug, Default)]
pub struct Response {
    pub status: u16,
    pub headers: Vec<(&'static str, String)>,
    pub body: Vec<u8>,
}

impl Response {
    fn html(status: u16, body: String) -> Response {
        let mut r = Response { status, headers: vec![("Content-Type", "text/html; charset=utf-8".into())], body: body.into_bytes() };
        r.headers.push((
            "Content-Security-Policy",
            "default-src 'none'; style-src 'self'; img-src 'self' data:; form-action 'self'; frame-ancestors 'none'; base-uri 'none'".into(),
        ));
        r.headers.push(("Cache-Control", "no-store".into()));
        r
    }

    fn json<T: Serialize>(status: u16, value: &T) -> Response {
        let body = serde_json::to_vec(value).unwrap_or_default();
        Response { status, headers: vec![("Content-Type", "application/json".into()), ("Cache-Control", "no-store".into())], body }
    }

    fn redirect(to: &str) -> Response {
        Response { status: 303, headers: vec![("Location", to.to_string()), ("Cache-Control", "no-store".into())], body: Vec::new() }
    }

    fn asset(kind: &str, body: &str) -> Response {
        Response {
            status: 200,
            headers: vec![("Content-Type", kind.to_string()), ("Cache-Control", "public, max-age=3600".into())],
            body: body.as_bytes().to_vec(),
        }
    }

    fn cookie(mut self, c: String) -> Response {
        self.headers.push(("Set-Cookie", c));
        self
    }

    /// The bytes on the wire.
    pub fn encode(&self) -> Vec<u8> {
        let reason = match self.status {
            200 => "OK",
            204 => "No Content",
            303 => "See Other",
            400 => "Bad Request",
            401 => "Unauthorized",
            403 => "Forbidden",
            404 => "Not Found",
            405 => "Method Not Allowed",
            413 => "Payload Too Large",
            429 => "Too Many Requests",
            500 => "Internal Server Error",
            503 => "Service Unavailable",
            _ => "",
        };
        let mut out = format!("HTTP/1.1 {} {reason}\r\n", self.status);
        for (n, v) in &self.headers {
            out.push_str(&format!("{n}: {v}\r\n"));
        }
        out.push_str("X-Content-Type-Options: nosniff\r\nReferrer-Policy: same-origin\r\nX-Frame-Options: DENY\r\n");
        out.push_str(&format!("Content-Length: {}\r\nConnection: close\r\n\r\n", self.body.len()));
        let mut bytes = out.into_bytes();
        bytes.extend_from_slice(&self.body);
        bytes
    }
}

/// `next` when it is a path on this site (never another site, so a link can't send people away
/// after they sign in); otherwise empty.
pub fn safe_next(next: &str) -> String {
    let n = next.trim();
    let ok = n.starts_with('/')
        && !n.starts_with("//")
        && !n.starts_with("/\\")
        && n.len() <= 1024
        && n.chars().all(|c| !c.is_control() && !c.is_whitespace() && c != '\\');
    if ok { n.to_string() } else { String::new() }
}

fn session_cookie(s: &Settings, token: &str) -> String {
    format!("{COOKIE}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={}{}", s.idle, if s.secure { "; Secure" } else { "" })
}

fn clear_cookie(s: &Settings, name: &str, path: &str) -> String {
    format!("{name}=; Path={path}; HttpOnly; SameSite=Lax; Max-Age=0{}", if s.secure { "; Secure" } else { "" })
}

/// Whether a form post comes from this site.
fn same_site(ctx: &Ctx, req: &Request) -> bool {
    let Some(origin) = req.header("origin") else {
        // Browsers send Origin with every POST; a tool that doesn't isn't a cross-site form.
        return true;
    };
    match &ctx.settings.origin {
        Some(o) => origin.trim_end_matches('/') == o.trim_end_matches('/'),
        None => origin.split_once("://").is_some_and(|(_, host)| Some(host) == req.header("host")),
    }
}

/// Who this request is signed in as.
fn current(ctx: &Ctx, req: &Request) -> Option<User> {
    let token = req.cookie(COOKIE)?;
    ctx.accounts().session(&token)
}

fn form_page(ctx: &Ctx, status: u16, error: &str, email: &str, next: &str) -> Response {
    let signup = ctx.accounts().signup_open();
    let form = Form { error, email, next, google: ctx.settings.google.is_some(), signup };
    Response::html(status, pages::login(&form, &ctx.settings.site))
}

/// Signed in as `user`: the cookie, then on to `next` (or the app).
fn signed_in(ctx: &Ctx, user: &str, next: &str) -> Response {
    let token = match ctx.accounts().start_session(user) {
        Ok(t) => t,
        Err(e) => {
            log::warn!("accounts: can't start a session: {e}");
            return Response::html(
                503,
                pages::message("Can't sign you in", "Something went wrong on the server. Try again in a minute.", &ctx.settings.site),
            );
        }
    };
    Response::redirect(if next.is_empty() { "/" } else { next }).cookie(session_cookie(&ctx.settings, &token))
}

/// Answer one request.
pub fn handle(ctx: &Ctx, req: &Request) -> Response {
    match (req.method.as_str(), req.path.as_str()) {
        ("GET" | "HEAD", "/login.css") => Response::asset("text/css; charset=utf-8", pages::CSS),
        ("GET" | "HEAD", "/login-art.svg") => Response::asset("image/svg+xml", pages::ART),
        ("GET" | "HEAD", "/login") => {
            let next = safe_next(&req.query_value("next").unwrap_or_default());
            match current(ctx, req) {
                Some(_) if !next.is_empty() && next != "/login" => Response::redirect(&next),
                Some(u) => Response::html(200, pages::account(&u, "/", &ctx.settings.site)),
                None => form_page(ctx, 200, "", "", &next),
            }
        }
        ("POST", "/login") => login(ctx, req),
        ("POST", "/logout") => {
            if !same_site(ctx, req) {
                return Response::html(403, pages::message("Can't log out", "That request came from another site.", &ctx.settings.site));
            }
            if let Some(token) = req.cookie(COOKIE)
                && let Err(e) = ctx.accounts().end_session(&token)
            {
                log::warn!("accounts: can't end a session: {e}");
            }
            Response::redirect("/login").cookie(clear_cookie(&ctx.settings, COOKIE, "/"))
        }
        // Asked by the reverse proxy for every request to the app: only a signed-in browser gets it.
        ("GET" | "HEAD", "/auth/check") => {
            let status = if current(ctx, req).is_some() { 204 } else { 401 };
            Response { status, headers: vec![("Cache-Control", "no-store".into())], body: Vec::new() }
        }
        ("GET", "/auth/me") => match current(ctx, req) {
            Some(u) => Response::json(200, &me(ctx, &u)),
            None => Response::json(401, &serde_json::json!({"signedIn": false})),
        },
        ("POST", "/auth/profile") => signed_in_json(ctx, req, |ctx, u| {
            let update: super::ProfileUpdate = serde_json::from_slice(&req.body).map_err(|_| "not a profile change".to_string())?;
            let user = ctx.accounts().update_profile(&u.id, &update)?;
            Ok(me(ctx, &user))
        }),
        ("POST", "/auth/invite") => signed_in_json(ctx, req, |ctx, u| {
            #[derive(Deserialize)]
            struct Body {
                emails: Vec<String>,
            }
            let body: Body = serde_json::from_slice(&req.body).map_err(|_| "not a list of emails".to_string())?;
            let invited = ctx.accounts().invite(&u.id, &body.emails)?;
            Ok(serde_json::json!({"invited": invited, "link": invite_link(ctx), "emailed": false}))
        }),
        ("GET", "/auth/recents") => match current(ctx, req) {
            Some(u) => Response::json(200, &serde_json::json!({"files": ctx.accounts().recents(&u.id)})),
            None => Response::json(401, &serde_json::json!({"error": "not signed in"})),
        },
        ("POST", "/auth/recents") => signed_in_json(ctx, req, |ctx, u| {
            let change: super::recents::Change =
                serde_json::from_slice(&req.body).map_err(|_| "give `open` {target, name, kind} or `forget` (a target)".to_string())?;
            let files = ctx.accounts().change_recents(&u.id, change)?;
            Ok(serde_json::json!({"files": files}))
        }),
        ("GET", "/auth/folders") => match current(ctx, req) {
            Some(u) => Response::json(200, &serde_json::json!({"folders": ctx.accounts().folders(&u.id)})),
            None => Response::json(401, &serde_json::json!({"error": "not signed in"})),
        },
        ("POST", "/auth/folders") => signed_in_json(ctx, req, |ctx, u| {
            let change: super::folders::Change = serde_json::from_slice(&req.body).map_err(|_| super::folders::BAD_CHANGE.to_string())?;
            let folders = ctx.accounts().change_folders(&u.id, change)?;
            Ok(serde_json::json!({"folders": folders}))
        }),
        ("POST", "/auth/recents/picture") => signed_in_json(ctx, req, |ctx, u| {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Body {
                target: String,
                png: String,
            }
            const BAD: &str = "give `target` (a file in the list) and `png` (the picture, base64)";
            let body: Body = serde_json::from_slice(&req.body).map_err(|_| BAD.to_string())?;
            let png = base64::engine::general_purpose::STANDARD.decode(body.png.trim()).map_err(|_| BAD.to_string())?;
            let files = ctx.accounts().set_recent_picture(&u.id, &body.target, &png)?;
            Ok(serde_json::json!({"files": files}))
        }),
        ("GET", "/auth/recents/picture") => {
            let picture = current(ctx, req).zip(req.query_value("p")).and_then(|(u, token)| ctx.accounts().recent_picture(&u.id, &token));
            match picture {
                // The token changes with the picture, so the browser may keep it.
                Some(png) => Response {
                    status: 200,
                    headers: vec![("Content-Type", "image/png".into()), ("Cache-Control", "private, max-age=31536000, immutable".into())],
                    body: png,
                },
                None => Response { status: 404, headers: vec![("Cache-Control", "no-store".into())], body: Vec::new() },
            }
        }
        ("GET", "/auth/avatar") => match current(ctx, req) {
            Some(u) => avatar(ctx, &u),
            None => Response { status: 401, headers: vec![("Cache-Control", "no-store".into())], body: Vec::new() },
        },
        ("GET", "/auth/google") => google_start(ctx, req),
        ("GET", "/auth/google/callback") => google_back(ctx, req),
        ("POST", "/auth/admin") => admin(ctx, req),
        (
            _,
            "/login"
            | "/logout"
            | "/auth/me"
            | "/auth/avatar"
            | "/auth/check"
            | "/auth/admin"
            | "/auth/profile"
            | "/auth/invite"
            | "/auth/recents"
            | "/auth/folders"
            | "/auth/recents/picture",
        ) => Response::html(405, pages::message("Not here", "That page doesn't take this kind of request.", &ctx.settings.site)),
        _ => Response::html(404, pages::message("Page not found", "There's nothing at this address.", &ctx.settings.site)),
    }
}

/// How long a fetched picture is kept, and how many are (one per account that asked).
const AVATAR_FOR: Duration = Duration::from_secs(24 * 3600);
const AVATARS: usize = 4096;

/// `u`'s picture, from Google the first time (and once a day), from memory otherwise.
fn avatar(ctx: &Ctx, u: &User) -> Response {
    let cached = ctx.avatars.lock().unwrap_or_else(|e| e.into_inner()).get(&u.id).filter(|(at, ..)| at.elapsed() < AVATAR_FOR).cloned();
    let (bytes, kind) = match cached {
        Some((_, bytes, kind)) => (bytes, kind),
        None => {
            // Asks Google over the network: no lock is held.
            let (bytes, kind) = match google::fetch_picture(&u.picture) {
                Ok(p) => p,
                Err(e) => {
                    log::info!("accounts: no picture for an account: {e}");
                    return Response { status: 404, headers: vec![("Cache-Control", "no-store".into())], body: Vec::new() };
                }
            };
            let mut kept = ctx.avatars.lock().unwrap_or_else(|e| e.into_inner());
            if kept.len() >= AVATARS {
                kept.retain(|_, (at, ..)| at.elapsed() < AVATAR_FOR);
            }
            if kept.len() < AVATARS {
                kept.insert(u.id.clone(), (Instant::now(), bytes.clone(), kind.clone()));
            }
            (bytes, kind)
        }
    };
    Response { status: 200, headers: vec![("Content-Type", kind), ("Cache-Control", "private, max-age=3600".into())], body: bytes }
}

/// `/auth/me`'s answer for a signed-in account.
fn me(ctx: &Ctx, u: &User) -> serde_json::Value {
    let invited = ctx.accounts().invites_by(&u.id);
    serde_json::json!({
        "signedIn": true,
        "user": {"id": u.id, "email": u.email, "name": u.name, "hasPicture": !u.picture.is_empty(), "profile": u.profile, "invited": invited},
        "inviteLink": invite_link(ctx),
    })
}

/// The address to send people so they can sign up.
fn invite_link(ctx: &Ctx) -> String {
    format!("{}/login", ctx.settings.origin.as_deref().unwrap_or_default())
}

/// A JSON request from a signed-in browser of this site: `f`'s answer, or why not.
fn signed_in_json(ctx: &Ctx, req: &Request, f: impl FnOnce(&Ctx, &User) -> Result<serde_json::Value, String>) -> Response {
    if !same_site(ctx, req) {
        return Response::json(403, &serde_json::json!({"error": "that request came from another site"}));
    }
    let Some(u) = current(ctx, req) else { return Response::json(401, &serde_json::json!({"error": "not signed in"})) };
    match f(ctx, &u) {
        Ok(v) => Response::json(200, &v),
        Err(e) => Response::json(400, &serde_json::json!({"error": e})),
    }
}

fn login(ctx: &Ctx, req: &Request) -> Response {
    if !same_site(ctx, req) {
        return form_page(ctx, 403, "That form came from another site. Log in here instead.", "", "");
    }
    let form = req.form();
    let field = |k: &str| form.get(k).map(String::as_str).unwrap_or_default();
    let next = safe_next(field("next"));
    let typed = field("email").trim();
    let password = field("password");
    let Some(email) = super::clean_email(typed) else { return form_page(ctx, 400, super::BAD_EMAIL, typed, &next) };
    let found = ctx.accounts().lookup(&email);
    match found {
        Lookup::Missing => {
            if !ctx.accounts().signup_open() {
                return form_page(ctx, 403, super::SIGNUP_CLOSED, &email, &next);
            }
            if let Err(e) = super::check_password(password) {
                return form_page(ctx, 400, e, &email, &next);
            }
            // Hashing takes a moment: not while holding the accounts.
            let hash = match password::hash(password) {
                Ok(h) => h,
                Err(e) => {
                    log::warn!("accounts: {e}");
                    return form_page(ctx, 500, "Something went wrong on the server. Try again.", &email, &next);
                }
            };
            let made = ctx.accounts().create(&email, "", Some(hash), None);
            match made {
                Ok(u) => {
                    log::info!("accounts: new account {}", u.id);
                    signed_in(ctx, &u.id, &next)
                }
                Err(e) => form_page(ctx, 400, &e, &email, &next),
            }
        }
        Lookup::Password { user, hash } => {
            if password::verify(password, &hash) {
                signed_in(ctx, &user, &next)
            } else {
                ctx.accounts().failed(&user);
                form_page(ctx, 401, super::WRONG_PASSWORD, &email, &next)
            }
        }
        Lookup::GoogleOnly => form_page(ctx, 400, super::GOOGLE_ONLY, &email, &next),
        Lookup::Disabled => form_page(ctx, 403, super::DISABLED, &email, &next),
        Lookup::Paused => form_page(ctx, 429, super::PAUSED, &email, &next),
    }
}

fn pending(ctx: &Ctx) -> MutexGuard<'_, HashMap<String, Pending>> {
    ctx.pending.lock().unwrap_or_else(|e| e.into_inner())
}

fn google_start(ctx: &Ctx, req: &Request) -> Response {
    let Some(client) = &ctx.settings.google else {
        return Response::html(
            404,
            pages::message("Google sign-in is off", "This server doesn't sign in with Google yet. Use your email instead.", &ctx.settings.site),
        );
    };
    let started = match google::start(client) {
        Ok(s) => s,
        Err(e) => {
            log::warn!("accounts: {e}");
            return Response::html(503, pages::message("Can't sign you in", "Something went wrong on the server. Try again.", &ctx.settings.site));
        }
    };
    {
        let mut p = pending(ctx);
        p.retain(|_, x| x.at.elapsed() < PENDING_FOR);
        if p.len() >= MAX_PENDING {
            return Response::html(
                503,
                pages::message("Can't sign you in", "Too many sign-ins at once. Try again in a few minutes.", &ctx.settings.site),
            );
        }
        let next = safe_next(&req.query_value("next").unwrap_or_default());
        p.insert(started.state.clone(), Pending { verifier: started.verifier.clone(), next, at: Instant::now() });
    }
    let cookie = format!(
        "{GOOGLE_COOKIE}={}; Path=/auth/google; HttpOnly; SameSite=Lax; Max-Age={}{}",
        started.state,
        PENDING_FOR.as_secs(),
        if ctx.settings.secure { "; Secure" } else { "" }
    );
    Response::redirect(&started.url).cookie(cookie)
}

fn google_back(ctx: &Ctx, req: &Request) -> Response {
    let fail = |text: &str| {
        Response::html(400, pages::message("Can't sign you in", text, &ctx.settings.site)).cookie(clear_cookie(
            &ctx.settings,
            GOOGLE_COOKIE,
            "/auth/google",
        ))
    };
    let Some(client) = &ctx.settings.google else { return fail("This server doesn't sign in with Google.") };
    if req.query_value("error").is_some() {
        return fail("Google sign-in was cancelled.");
    }
    let (Some(state), Some(code)) = (req.query_value("state"), req.query_value("code")) else {
        return fail("Google sent back an incomplete answer. Try again.");
    };
    if !req.cookie(GOOGLE_COOKIE).is_some_and(|c| same_secret(&c, &state)) {
        return fail("This sign-in was started in another browser. Start again here.");
    }
    let Some(p) = pending(ctx).remove(&state).filter(|p| p.at.elapsed() < PENDING_FOR) else { return fail("The sign-in took too long. Try again.") };
    // Asks Google over the network: not while holding the accounts.
    let who = match google::finish(client, &code, &p.verifier) {
        Ok(w) => w,
        Err(e) => return fail(&e),
    };
    let user = ctx.accounts().google_user(&who.sub, &who.email, &who.name, &who.picture);
    match user {
        Ok(u) => signed_in(ctx, &u.id, &p.next).cookie(clear_cookie(&ctx.settings, GOOGLE_COOKIE, "/auth/google")),
        Err(e) => fail(&e),
    }
}

/// An operator's request (`figcraft-server users …`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AdminReq {
    pub key: String,
    /// `list`, `add`, `reset-password`, `disable`, `enable` or `remove`.
    pub action: String,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub name: String,
}

/// Its answer.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AdminReply {
    #[serde(default)]
    pub error: String,
    #[serde(default)]
    pub users: Vec<UserRow>,
    /// A password made for the account (`add`, `reset-password`).
    #[serde(default)]
    pub password: String,
}

fn admin(ctx: &Ctx, req: &Request) -> Response {
    if !req.peer.is_some_and(|p| p.is_loopback()) {
        return Response::json(403, &AdminReply { error: "only from this machine".into(), ..Default::default() });
    }
    let Ok(r) = serde_json::from_slice::<AdminReq>(&req.body) else {
        return Response::json(400, &AdminReply { error: "not a request".into(), ..Default::default() });
    };
    if ctx.settings.key.is_empty() || !same_secret(&r.key, &ctx.settings.key) {
        return Response::json(403, &AdminReply { error: "wrong server key".into(), ..Default::default() });
    }
    match operate(&mut ctx.accounts(), &r) {
        Ok(reply) => Response::json(200, &reply),
        Err(e) => Response::json(400, &AdminReply { error: e, ..Default::default() }),
    }
}

/// Do an operator's request on the accounts (also used directly when no server is running).
pub fn operate(accounts: &mut Accounts, r: &AdminReq) -> Result<AdminReply, String> {
    let made = || -> Result<(String, String), String> {
        let p = format!("{}{}", token::secret()?, token::secret()?).chars().take(20).collect::<String>();
        Ok((password::hash(&p)?, p))
    };
    let mut reply = AdminReply::default();
    match r.action.as_str() {
        "list" => reply.users = accounts.list(),
        "add" => {
            let (hash, p) = made()?;
            accounts.add_by_operator(&r.email, &r.name, Some(hash))?;
            reply.password = p;
        }
        "reset-password" => {
            let (hash, p) = made()?;
            accounts.set_password(&r.email, hash)?;
            reply.password = p;
        }
        "disable" => accounts.set_disabled(&r.email, true)?,
        "enable" => accounts.set_disabled(&r.email, false)?,
        "remove" => accounts.remove(&r.email)?,
        other => return Err(format!("unknown action {other}")),
    }
    Ok(reply)
}

/// Read one request from a connection.
pub fn read_request(stream: &mut impl Read) -> Result<Request, u16> {
    let mut buf = Vec::with_capacity(2048);
    let mut chunk = [0u8; 4096];
    let head_len = loop {
        let n = stream.read(&mut chunk).map_err(|_| 400u16)?;
        if n == 0 {
            return Err(400);
        }
        buf.extend_from_slice(chunk.get(..n).unwrap_or_default());
        if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break end + 4;
        }
        if buf.len() > MAX_HEAD {
            return Err(413);
        }
    };
    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut parsed = httparse::Request::new(&mut headers);
    match parsed.parse(buf.get(..head_len).unwrap_or_default()) {
        Ok(httparse::Status::Complete(_)) => {}
        _ => return Err(400),
    }
    let method = parsed.method.unwrap_or_default().to_string();
    let target = parsed.path.unwrap_or_default();
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let headers: Vec<(String, String)> =
        parsed.headers.iter().map(|h| (h.name.to_ascii_lowercase(), String::from_utf8_lossy(h.value).into_owned())).collect();
    let length = match headers.iter().find(|(n, _)| n == "content-length") {
        Some((_, v)) => v.trim().parse::<usize>().map_err(|_| 400u16)?,
        None => 0,
    };
    if headers.iter().any(|(n, _)| n == "transfer-encoding") {
        return Err(400);
    }
    if length > if path == "/auth/recents/picture" { MAX_PICTURE_BODY } else { MAX_BODY } {
        return Err(413);
    }
    let mut body = buf.get(head_len..).unwrap_or_default().to_vec();
    while body.len() < length {
        let n = stream.read(&mut chunk).map_err(|_| 400u16)?;
        if n == 0 {
            return Err(400);
        }
        body.extend_from_slice(chunk.get(..n).unwrap_or_default());
    }
    body.truncate(length);
    Ok(Request { method, path: path.to_string(), query: query.to_string(), headers, body, peer: None })
}

fn serve_one(ctx: &Ctx, mut stream: TcpStream) {
    let _ = stream.set_read_timeout(Some(READ_TIMEOUT));
    let _ = stream.set_write_timeout(Some(READ_TIMEOUT));
    let peer = stream.peer_addr().ok().map(|a| a.ip());
    let reply = match read_request(&mut stream) {
        Ok(mut req) => {
            req.peer = peer;
            let mut r = handle(ctx, &req);
            if req.method == "HEAD" {
                r.body.clear();
            }
            r
        }
        Err(status) => Response { status, headers: vec![("Content-Type", "text/plain".into())], body: b"bad request\n".to_vec() },
    };
    let _ = stream.write_all(&reply.encode());
    let _ = stream.flush();
    let _ = stream.shutdown(Shutdown::Write);
}

/// The accounts' pages, served on a thread of their own until stopped (or dropped).
pub struct AuthServer {
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl AuthServer {
    pub fn start(listener: TcpListener, ctx: Ctx) -> Result<AuthServer, String> {
        let addr = listener.local_addr().map_err(|e| e.to_string())?;
        let stop = Arc::new(AtomicBool::new(false));
        let ctx = Arc::new(ctx);
        let open = Arc::new(AtomicUsize::new(0));
        let flag = stop.clone();
        let thread = std::thread::Builder::new()
            .name("accounts".into())
            .spawn(move || {
                for stream in listener.incoming() {
                    if flag.load(Ordering::SeqCst) {
                        break;
                    }
                    let Ok(stream) = stream else {
                        std::thread::sleep(Duration::from_millis(50));
                        continue;
                    };
                    if open.load(Ordering::SeqCst) >= MAX_CONNECTIONS {
                        let _ = (&stream).write_all(&Response { status: 503, ..Default::default() }.encode());
                        continue;
                    }
                    open.fetch_add(1, Ordering::SeqCst);
                    let (ctx, open) = (ctx.clone(), open.clone());
                    let spawned = std::thread::Builder::new().name("accounts-conn".into()).spawn(move || {
                        serve_one(&ctx, stream);
                        open.fetch_sub(1, Ordering::SeqCst);
                    });
                    if let Err(e) = spawned {
                        log::warn!("accounts: can't start a connection thread: {e}");
                    }
                }
            })
            .map_err(|e| format!("can't start the accounts thread: {e}"))?;
        Ok(AuthServer { addr, stop, thread: Some(thread) })
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Wake the accept loop.
        let _ = TcpStream::connect_timeout(&crate::cli::reachable(self.addr), Duration::from_secs(1));
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for AuthServer {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Send an operator's request to the running server's accounts port.
pub fn ask(addr: SocketAddr, req: &AdminReq) -> Result<AdminReply, String> {
    let body = serde_json::to_vec(req).map_err(|e| e.to_string())?;
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(5)).map_err(|e| format!("can't reach the server's accounts at {addr}: {e}"))?;
    let _ = s.set_read_timeout(Some(Duration::from_secs(30)));
    let head = format!(
        "POST /auth/admin HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    s.write_all(head.as_bytes()).and_then(|()| s.write_all(&body)).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    s.take(4 << 20).read_to_end(&mut out).map_err(|e| e.to_string())?;
    let at = out.windows(4).position(|w| w == b"\r\n\r\n").ok_or("the server's answer can't be read")?;
    let reply: AdminReply =
        serde_json::from_slice(out.get(at + 4..).unwrap_or_default()).map_err(|_| "the server's answer can't be read".to_string())?;
    if reply.error.is_empty() { Ok(reply) } else { Err(reply.error) }
}
