//! Accounts: signing up and in on the server's pages, sessions, Google's ID tokens, the operator's
//! `users` commands, and junk requests.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::{Arc, Mutex};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;

use crate::accounts::http::{self, AdminReq, Ctx, Request, Response, Settings, handle, read_request, safe_next};
use crate::accounts::{self, Accounts, google, pages};
use crate::cli;
use crate::config::Config;
use crate::test_util::{TempDir, config};

fn ctx(dir: &TempDir, signup: bool) -> Ctx {
    let a = Accounts::open(dir.path(), signup, 21.0).unwrap();
    let settings = Settings {
        origin: Some("https://figcraft.example.com".into()),
        secure: true,
        idle: 21 * 86_400,
        key: "operator-key-123456".into(),
        ..Settings::default()
    };
    Ctx::new(Arc::new(Mutex::new(a)), settings)
}

fn post(path: &str, body: &str, cookie: Option<&str>) -> Request {
    let mut headers = vec![("origin".to_string(), "https://figcraft.example.com".to_string())];
    if let Some(c) = cookie {
        headers.push(("cookie".into(), format!("{}={c}", http::COOKIE)));
    }
    Request { method: "POST".into(), path: path.into(), headers, body: body.as_bytes().to_vec(), ..Request::default() }
}

fn get(path: &str, query: &str, cookie: Option<&str>) -> Request {
    let headers = cookie.map(|c| vec![("cookie".to_string(), format!("other=1; {}={c}", http::COOKIE))]).unwrap_or_default();
    Request { method: "GET".into(), path: path.into(), query: query.into(), headers, ..Request::default() }
}

fn header<'a>(r: &'a Response, name: &str) -> Option<&'a str> {
    r.headers.iter().find(|(n, _)| *n == name).map(|(_, v)| v.as_str())
}

/// The session token a response sets.
fn token(r: &Response) -> String {
    let c = header(r, "Set-Cookie").expect("a cookie");
    let v = c.strip_prefix(&format!("{}=", http::COOKIE)).expect("the session cookie");
    v.split(';').next().unwrap().to_string()
}

fn body(r: &Response) -> String {
    String::from_utf8_lossy(&r.body).into_owned()
}

#[test]
fn signing_up_then_in_and_out() {
    let dir = TempDir::new("acc-flow");
    let c = ctx(&dir, true);
    let page = handle(&c, &get("/login", "", None));
    assert_eq!(page.status, 200);
    assert!(body(&page).contains("Log in or sign up") && body(&page).contains("name=\"password\""));
    assert!(!body(&page).contains("Continue with Google"), "no Google client: no button");

    // A new email makes the account and signs it in.
    let r = handle(&c, &post("/login", "email=Ada%40Example.com&password=correct+horse&next=%2Ffiles", None));
    assert_eq!(r.status, 303, "{}", body(&r));
    assert_eq!(header(&r, "Location"), Some("/files"));
    let cookie = header(&r, "Set-Cookie").unwrap();
    assert!(cookie.contains("HttpOnly") && cookie.contains("SameSite=Lax") && cookie.contains("Secure"), "{cookie}");
    let t = token(&r);
    let me = handle(&c, &get("/auth/me", "", Some(&t)));
    assert_eq!(me.status, 200);
    assert!(body(&me).contains("\"email\":\"ada@example.com\"") && body(&me).contains("\"name\":\"ada\""), "{}", body(&me));
    let acct = handle(&c, &get("/login", "", Some(&t)));
    assert!(body(&acct).contains("You're signed in") && body(&acct).contains("ada@example.com"));
    assert_eq!(header(&handle(&c, &get("/login", "next=/files", Some(&t))), "Location"), Some("/files"));

    // The password is hashed, never kept as typed.
    let users = std::fs::read_to_string(dir.path().join("accounts/users.json")).unwrap();
    assert!(users.contains("$argon2id$") && !users.contains("correct horse"), "{users}");
    let sessions = std::fs::read_to_string(dir.path().join("accounts/sessions.json")).unwrap();
    assert!(!sessions.contains(&t), "only the token's hash is kept");

    // Logging in again: right password, then wrong.
    let again = handle(&c, &post("/login", "email=ada%40example.com&password=correct+horse", None));
    assert_eq!((again.status, header(&again, "Location")), (303, Some("/")), "on to the app");
    let wrong = handle(&c, &post("/login", "email=ada%40example.com&password=battery+staple", None));
    assert_eq!(wrong.status, 401);
    assert!(body(&wrong).contains(accounts::WRONG_PASSWORD.replace('\'', "&#39;").as_str()) && body(&wrong).contains("value=\"ada@example.com\""));

    // The proxy's check before serving the app.
    assert_eq!(handle(&c, &get("/auth/check", "", Some(&t))).status, 204);
    assert_eq!(handle(&c, &get("/auth/check", "", None)).status, 401);
    assert_eq!(handle(&c, &get("/auth/check", "", Some("forged"))).status, 401);

    // Logging out ends that session only.
    let out = handle(&c, &post("/logout", "", Some(&t)));
    assert_eq!((out.status, header(&out, "Location")), (303, Some("/login")));
    assert!(header(&out, "Set-Cookie").unwrap().contains("Max-Age=0"));
    assert_eq!(handle(&c, &get("/auth/me", "", Some(&t))).status, 401);
    assert_eq!(handle(&c, &get("/auth/me", "", Some(&token(&again)))).status, 200);
    assert_eq!(handle(&c, &get("/auth/check", "", Some(&t))).status, 401, "signed out: no app");
}

#[test]
fn accounts_and_sessions_survive_a_restart() {
    let dir = TempDir::new("acc-restart");
    let t = {
        let c = ctx(&dir, true);
        token(&handle(&c, &post("/login", "email=bo%40example.org&password=12345678", None)))
    };
    let c = ctx(&dir, true);
    assert_eq!(handle(&c, &get("/auth/me", "", Some(&t))).status, 200);
    assert_eq!(handle(&c, &post("/login", "email=bo%40example.org&password=12345678", None)).status, 303);
    // A session unused for longer than it lasts is gone.
    let mut a = Accounts::open(dir.path(), true, 0.000_01).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1100));
    assert!(a.session(&t).is_none());
}

#[test]
fn bad_input_is_refused_with_a_reason() {
    let dir = TempDir::new("acc-bad");
    let c = ctx(&dir, true);
    let r = handle(&c, &post("/login", "email=nope&password=12345678", None));
    assert_eq!(r.status, 400);
    assert!(body(&r).contains("valid email"));
    let r = handle(&c, &post("/login", "email=a%40b.co&password=short", None));
    assert_eq!(r.status, 400);
    assert!(body(&r).contains("at least 8"));
    assert!(Accounts::open(dir.path(), true, 21.0).unwrap().is_empty());
    // What people typed comes back escaped.
    let r = handle(&c, &post("/login", "email=%22%3E%3Cscript%3E%40x.co&password=1", None));
    assert!(!body(&r).contains("<script>"), "{}", body(&r));
    // A form from another site.
    let mut r = post("/login", "email=a%40b.co&password=12345678", None);
    r.headers[0].1 = "https://evil.example".into();
    assert_eq!(handle(&c, &r).status, 403);
    assert!(Accounts::open(dir.path(), true, 21.0).unwrap().is_empty());
    let mut out = post("/logout", "", Some("x"));
    out.headers[0].1 = "https://evil.example".into();
    assert_eq!(handle(&c, &out).status, 403);
}

#[test]
fn too_many_wrong_passwords_pause_an_account() {
    let dir = TempDir::new("acc-pause");
    let c = ctx(&dir, true);
    assert_eq!(handle(&c, &post("/login", "email=cy%40example.com&password=right-password", None)).status, 303);
    for _ in 0..accounts::MAX_FAILURES {
        assert_eq!(handle(&c, &post("/login", "email=cy%40example.com&password=wrong-password", None)).status, 401);
    }
    let r = handle(&c, &post("/login", "email=cy%40example.com&password=right-password", None));
    assert_eq!(r.status, 429, "even the right password waits");
}

#[test]
fn closed_sign_up_only_lets_existing_accounts_in() {
    let dir = TempDir::new("acc-closed");
    let mut a = Accounts::open(dir.path(), false, 21.0).unwrap();
    let made =
        http::operate(&mut a, &AdminReq { action: "add".into(), email: "Di@Example.com".into(), name: "Di".into(), ..AdminReq::default() }).unwrap();
    assert_eq!(made.password.len(), 20);
    drop(a);
    let c = ctx(&dir, false);
    let page = body(&handle(&c, &get("/login", "", None)));
    assert!(page.contains(">Log in<") && !page.contains("sign up"));
    let r = handle(&c, &post("/login", "email=new%40example.com&password=12345678", None));
    assert_eq!(r.status, 403);
    assert!(body(&r).contains("Sign-up is closed"));
    let ok = handle(&c, &post("/login", &format!("email=di%40example.com&password={}", made.password), None));
    assert_eq!(ok.status, 303, "{}", body(&ok));
}

#[test]
fn operator_commands() {
    let dir = TempDir::new("acc-ops");
    let c = ctx(&dir, true);
    let t = token(&handle(&c, &post("/login", "email=ed%40example.com&password=first-password", None)));
    let mut a = c.accounts.lock().unwrap();
    let rows = http::operate(&mut a, &AdminReq { action: "list".into(), ..AdminReq::default() }).unwrap().users;
    assert_eq!(rows.len(), 1);
    assert_eq!((rows[0].email.as_str(), rows[0].sessions, rows[0].password), ("ed@example.com", 1, true));
    let reset = http::operate(&mut a, &AdminReq { action: "reset-password".into(), email: "ED@example.com".into(), ..AdminReq::default() }).unwrap();
    assert!(a.session(&t).is_none(), "a reset signs the account out");
    http::operate(&mut a, &AdminReq { action: "disable".into(), email: "ed@example.com".into(), ..AdminReq::default() }).unwrap();
    drop(a);
    let off = handle(&c, &post("/login", &format!("email=ed%40example.com&password={}", reset.password), None));
    assert_eq!(off.status, 403);
    let mut a = c.accounts.lock().unwrap();
    http::operate(&mut a, &AdminReq { action: "enable".into(), email: "ed@example.com".into(), ..AdminReq::default() }).unwrap();
    drop(a);
    assert_eq!(handle(&c, &post("/login", &format!("email=ed%40example.com&password={}", reset.password), None)).status, 303);
    let mut a = c.accounts.lock().unwrap();
    assert!(http::operate(&mut a, &AdminReq { action: "remove".into(), email: "nobody@example.com".into(), ..AdminReq::default() }).is_err());
    http::operate(&mut a, &AdminReq { action: "remove".into(), email: "ed@example.com".into(), ..AdminReq::default() }).unwrap();
    assert!(a.is_empty());
    drop(a);

    // Over HTTP only from this machine, and only with the key.
    let mut req = post("/auth/admin", r#"{"key":"operator-key-123456","action":"list"}"#, None);
    assert_eq!(handle(&c, &req).status, 403, "no peer: not this machine");
    req.peer = Some("127.0.0.1".parse().unwrap());
    assert_eq!(handle(&c, &req).status, 200);
    req.body = br#"{"key":"wrong","action":"list"}"#.to_vec();
    assert_eq!(handle(&c, &req).status, 403);
    req.peer = Some("10.0.0.2".parse().unwrap());
    req.body = br#"{"key":"operator-key-123456","action":"list"}"#.to_vec();
    assert_eq!(handle(&c, &req).status, 403);
}

#[test]
fn next_stays_on_this_site() {
    assert_eq!(safe_next("/files/abc?x=1"), "/files/abc?x=1");
    for bad in ["//evil.example", "https://evil.example/", "/\\evil.example", "evil", "/a b", "/a\nb", "", "/a\\b"] {
        assert_eq!(safe_next(bad), "", "{bad:?}");
    }
    let dir = TempDir::new("acc-next");
    let c = ctx(&dir, true);
    let r = handle(&c, &post("/login", "email=fi%40example.com&password=12345678&next=%2F%2Fevil.example", None));
    assert_eq!(header(&r, "Location"), Some("/"));
}

fn id_token(claims: &serde_json::Value) -> String {
    format!("{}.{}.sig", URL_SAFE_NO_PAD.encode(br#"{"alg":"RS256"}"#), URL_SAFE_NO_PAD.encode(claims.to_string()))
}

#[test]
fn google_id_tokens_are_checked() {
    let good = serde_json::json!({"iss":"https://accounts.google.com","aud":"client-1","sub":"1234","exp":2000,"email":"gi@example.com","email_verified":true,"name":"Gi"});
    let who = google::identity(&id_token(&good), "client-1", 1000).unwrap();
    assert_eq!((who.sub.as_str(), who.email.as_str(), who.name.as_str()), ("1234", "gi@example.com", "Gi"));
    assert!(google::identity(&id_token(&good), "client-2", 1000).is_err(), "another app's token");
    assert!(google::identity(&id_token(&good), "client-1", 2000).is_err(), "expired");
    let mut other = good.clone();
    other["iss"] = "https://evil.example".into();
    assert!(google::identity(&id_token(&other), "client-1", 1000).is_err());
    let mut unverified = good.clone();
    unverified["email_verified"] = false.into();
    assert!(google::identity(&id_token(&unverified), "client-1", 1000).is_err());
    for junk in ["", "a", "a.b", "a.!!!.c", "a.e30.c"] {
        assert!(google::identity(junk, "client-1", 1000).is_err(), "{junk}");
    }
    let s = google::start(&google::Client {
        id: "client-1".into(),
        secret: "s".into(),
        redirect: "https://figcraft.example.com/auth/google/callback".into(),
    })
    .unwrap();
    assert!(s.url.starts_with("https://accounts.google.com/") && s.url.contains("code_challenge_method=S256") && s.url.contains(&s.state));
    assert!(s.verifier.len() >= 43 && !s.url.contains(&s.verifier), "the verifier stays here");
}

#[test]
fn google_takes_over_an_unverified_email() {
    let dir = TempDir::new("acc-google");
    let c = ctx(&dir, true);
    // Someone signs up with an email that isn't theirs…
    let t = token(&handle(&c, &post("/login", "email=gi%40example.com&password=squatter-pass", None)));
    let mut a = c.accounts.lock().unwrap();
    // …then its owner signs in with Google: same account, the squatter's password and session go.
    let u = a.google_user("g-1", "gi@example.com", "Gi").unwrap();
    assert!(u.password.is_none() && u.google.as_deref() == Some("g-1"));
    assert!(a.session(&t).is_none());
    assert_eq!(a.google_user("g-1", "changed@example.com", "Gi").unwrap().id, u.id, "found by Google account");
    assert_eq!(a.lookup("gi@example.com"), accounts::Lookup::GoogleOnly);
    assert_ne!(a.google_user("g-2", "new@example.com", "").unwrap().id, u.id);
    drop(a);
    // Without a Google client: no button, no flow.
    assert_eq!(handle(&c, &get("/auth/google", "", None)).status, 404);
    assert_eq!(handle(&c, &get("/auth/google/callback", "state=a&code=b", None)).status, 400);
}

#[test]
fn google_callback_needs_the_browser_that_started_it() {
    let dir = TempDir::new("acc-gstate");
    let mut c = ctx(&dir, true);
    c.settings.google =
        Some(google::Client { id: "client-1".into(), secret: "s".into(), redirect: "https://figcraft.example.com/auth/google/callback".into() });
    assert!(body(&handle(&c, &get("/login", "next=/x", None))).contains("href=\"/auth/google?next=%2Fx\""));
    let r = handle(&c, &get("/auth/google", "next=/x", None));
    assert_eq!(r.status, 303);
    let loc = header(&r, "Location").unwrap();
    let state = loc.split("state=").nth(1).unwrap().split('&').next().unwrap();
    // No state cookie (another browser), or a wrong one: refused before Google is asked.
    let back = handle(&c, &get("/auth/google/callback", &format!("state={state}&code=c"), None));
    assert_eq!(back.status, 400);
    assert!(body(&back).contains("another browser"));
    let mut wrong = get("/auth/google/callback", &format!("state={state}&code=c"), None);
    wrong.headers.push(("cookie".into(), "figcraft_google=nope".into()));
    assert_eq!(handle(&c, &wrong).status, 400);
    assert_eq!(handle(&c, &get("/auth/google/callback", "error=access_denied", None)).status, 400);
}

#[test]
fn pages_escape_and_assets_serve() {
    assert_eq!(pages::escape("<a href=\"x\">'&"), "&lt;a href=&quot;x&quot;&gt;&#39;&amp;");
    let dir = TempDir::new("acc-assets");
    let c = ctx(&dir, true);
    let css = handle(&c, &get("/login.css", "", None));
    assert_eq!((css.status, header(&css, "Content-Type")), (200, Some("text/css; charset=utf-8")));
    let art = handle(&c, &get("/login-art.svg", "", None));
    assert!(body(&art).starts_with("<svg"));
    assert_eq!(handle(&c, &get("/nothing", "", None)).status, 404);
    assert_eq!(handle(&c, &get("/logout", "", None)).status, 405, "logging out takes a POST");
    let page = handle(&c, &get("/login", "", None));
    assert!(header(&page, "Content-Security-Policy").unwrap().contains("frame-ancestors 'none'"));
    assert!(String::from_utf8_lossy(&page.encode()).starts_with("HTTP/1.1 200 OK\r\n"));
}

#[test]
fn emails_are_cleaned() {
    assert_eq!(accounts::clean_email("  Ada@Example.COM "), Some("ada@example.com".into()));
    for bad in ["", "a", "@x.co", "a@", "a@x", "a@.co", "a@x.", "a b@x.co", "a@b@c.co", "<a>@x.co", "a@x\u{0}y.co"] {
        assert_eq!(accounts::clean_email(bad), None, "{bad:?}");
    }
}

#[test]
fn junk_requests_never_panic() {
    let dir = TempDir::new("acc-junk");
    let c = ctx(&dir, true);
    let mut seed = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let pieces: [&[u8]; 12] = [
        b"GET ",
        b"POST ",
        b"/login",
        b"/auth/me",
        b"/auth/admin",
        b"?next=%2F%2F",
        b" HTTP/1.1\r\n",
        b"Content-Length: 99999999\r\n",
        b"Cookie: figcraft_session=\xff\xfe\r\n",
        b"\r\n",
        b"email=%ZZ&password=\xe2\x82",
        b"Transfer-Encoding: chunked\r\n",
    ];
    for _ in 0..3000 {
        let mut bytes = Vec::new();
        for _ in 0..(next() % 10) {
            bytes.extend_from_slice(pieces[(next() % pieces.len() as u64) as usize]);
            if next() % 5 == 0 {
                bytes.push((next() % 256) as u8);
            }
        }
        if let Ok(mut req) = read_request(&mut bytes.as_slice()) {
            req.peer = Some("127.0.0.1".parse().unwrap());
            let _ = handle(&c, &req).encode();
        }
    }
    // Random fields straight into the handler too.
    for _ in 0..500 {
        let junk: String = (0..(next() % 40)).map(|_| char::from_u32((next() % 0x3000) as u32).unwrap_or('x')).collect();
        for path in ["/login", "/logout", "/auth/admin", "/auth/google", "/auth/google/callback", "/auth/me", "/auth/check"] {
            let mut r = post(path, &junk, Some(&junk));
            r.query = junk.clone();
            let _ = handle(&c, &r);
            r.method = "GET".into();
            let _ = handle(&c, &r);
        }
    }
    assert!(read_request(&mut &b"GET /login HTTP/1.1\r\nContent-Length: 999999\r\n\r\n"[..]).is_err());
}

fn http_req(addr: SocketAddr, request: &str) -> String {
    let mut s = TcpStream::connect(addr).unwrap();
    s.write_all(request.as_bytes()).unwrap();
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    out
}

#[test]
fn the_server_serves_accounts_on_their_own_port() {
    let dir = TempDir::new("acc-server");
    let cfg = Config { accounts_listen: "127.0.0.1:0".into(), ..config(dir.path()) };
    let mut server = crate::start(&cfg).unwrap();
    let at = server.accounts_addr().unwrap();
    let page = http_req(at, "GET /login HTTP/1.1\r\nHost: x\r\n\r\n");
    assert!(page.starts_with("HTTP/1.1 200 OK") && page.contains("Welcome to FigCraft"), "{page}");
    let body = "email=ha%40example.com&password=12345678";
    let r = http_req(
        at,
        &format!(
            "POST /login HTTP/1.1\r\nHost: x\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        ),
    );
    assert!(r.starts_with("HTTP/1.1 303") && r.contains("Set-Cookie: figcraft_session="), "{r}");
    // The operator's commands go through the running server.
    let cli = cli::parse(&["users".into(), "--data".into(), dir.path().display().to_string()]).unwrap();
    let mut cli = cli;
    cli.config.accounts_listen = cfg.accounts_listen.clone();
    let list = cli::run(&cli).unwrap();
    assert!(list.contains("ha@example.com"), "{list}");
    let reset = cli::run(&cli::Cli {
        command: cli::Command::Users { action: "reset-password".into(), email: "ha@example.com".into(), name: String::new() },
        ..cli.clone()
    })
    .unwrap();
    assert!(reset.starts_with("New password for ha@example.com"), "{reset}");
    server.stop();
    // And on the data directory once it has stopped.
    let list = cli::run(&cli).unwrap();
    assert!(list.contains("ha@example.com") && list.contains("password"), "{list}");
}

#[test]
fn accounts_settings_are_checked() {
    let dir = TempDir::new("acc-cfg");
    let ok = Config { accounts_listen: "0.0.0.0:47901".into(), ..config(dir.path()) };
    assert!(ok.check().is_ok());
    assert!(Config { accounts_listen: "nope".into(), ..ok.clone() }.check().is_err());
    assert!(Config { listen: "0.0.0.0:47901".into(), ..ok.clone() }.check().is_err(), "a port of its own");
    assert!(Config { session_days: 0.0, ..ok.clone() }.check().is_err());
    assert!(Config { session_days: f64::NAN, ..ok.clone() }.check().is_err());
    assert!(Config { google_client_id: "abc.apps.googleusercontent.com".into(), ..ok.clone() }.check().is_err(), "needs web_url");
    let g = Config { google_client_id: "abc.apps.googleusercontent.com".into(), web_url: "https://figcraft.example.com/".into(), ..ok };
    assert!(g.check().is_ok());
    let parsed: Config = toml::from_str("accounts_listen = \"0.0.0.0:47901\"\nsignup = false\nsession_days = 7\n").unwrap();
    assert!(!parsed.signup && parsed.session_days == 7.0);
}
