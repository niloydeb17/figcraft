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
    // No account here answers as a wrong password does: the page doesn't tell who has one.
    let r = handle(&c, &post("/login", "email=new%40example.com&password=12345678", None));
    assert_eq!(r.status, 401);
    assert!(body(&r).contains(&accounts::WRONG_PASSWORD.replace('\'', "&#39;")));
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
    assert_eq!(off.status, 401, "a turned-off account answers as a wrong password");
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
fn google_pictures_only_come_from_googles_image_hosts() {
    let ok = "https://lh3.googleusercontent.com/a/ACg8ocK=s96-c";
    assert_eq!(google::picture_url(ok), ok);
    for bad in [
        "",
        "http://lh3.googleusercontent.com/a/x",
        "https://evil.example/a.googleusercontent.com",
        "https://lh3.googleusercontent.com@evil.example/a",
        "https://lh3.googleusercontent.com:8080/a",
        "https://googleusercontent.com/a",
        "https://evil.example/?u=lh3.googleusercontent.com",
        "https://lh3.googleusercontent.com/a b",
        "file:///etc/passwd",
    ] {
        assert_eq!(google::picture_url(bad), "", "{bad}");
    }
    assert!(google::fetch_picture("https://evil.example/x").is_err(), "the server won't fetch it");
    let mut claims = serde_json::json!({"iss":"https://accounts.google.com","aud":"client-1","sub":"1234","exp":2000,"email":"gi@example.com","email_verified":true,"name":"Gi","picture":ok});
    assert_eq!(google::identity(&id_token(&claims), "client-1", 1000).unwrap().picture, ok);
    claims["picture"] = "https://evil.example/p.png".into();
    assert_eq!(google::identity(&id_token(&claims), "client-1", 1000).unwrap().picture, "", "another host's picture is dropped");
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
fn the_google_client_id_comes_from_the_setting_or_the_environment() {
    assert_eq!(google::client_id("", " abc.apps.googleusercontent.com\n").as_deref(), Some("abc.apps.googleusercontent.com"));
    assert_eq!(google::client_id("set.apps.googleusercontent.com", "env").as_deref(), Some("set.apps.googleusercontent.com"));
    assert_eq!(google::client_id("", ""), None);
    assert_eq!(google::client_id("", "two words"), None);
    assert_eq!(google::client_id("", &"x".repeat(300)), None);
}

#[test]
fn google_takes_over_an_unverified_email() {
    let dir = TempDir::new("acc-google");
    let c = ctx(&dir, true);
    // Someone signs up with an email that isn't theirs…
    let t = token(&handle(&c, &post("/login", "email=gi%40example.com&password=squatter-pass", None)));
    let mut a = c.accounts.lock().unwrap();
    // …then its owner signs in with Google: same account, the squatter's password and session go.
    let u = a.google_user("g-1", "gi@example.com", "Gi", "").unwrap();
    assert!(u.password.is_none() && u.google.as_deref() == Some("g-1"));
    assert!(a.session(&t).is_none());
    assert_eq!(a.google_user("g-1", "changed@example.com", "Gi", "").unwrap().id, u.id, "found by Google account");
    assert_eq!(a.lookup("gi@example.com"), accounts::Lookup::GoogleOnly);
    assert_ne!(a.google_user("g-2", "new@example.com", "", "").unwrap().id, u.id);
    let pic = "https://lh3.googleusercontent.com/a/x=s96-c";
    assert_eq!(a.google_user("g-1", "gi@example.com", "Gi", pic).unwrap().picture, pic, "kept at sign-in");
    assert_eq!(a.google_user("g-1", "gi@example.com", "Gi", "").unwrap().picture, "", "and gone when Google has none");
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
        for path in [
            "/login",
            "/logout",
            "/auth/admin",
            "/auth/google",
            "/auth/google/callback",
            "/auth/me",
            "/auth/check",
            "/auth/profile",
            "/auth/invite",
            "/auth/recents",
            "/auth/recents/picture",
            "/auth/folders",
        ] {
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

fn post_json(path: &str, body: &str, cookie: &str) -> Request {
    let mut r = post(path, body, Some(cookie));
    r.headers.push(("content-type".into(), "application/json".into()));
    r
}

#[test]
fn onboarding_answers_are_kept_on_the_account() {
    let dir = TempDir::new("acc-profile");
    let c = ctx(&dir, true);
    let t = token(&handle(&c, &post("/login", "email=jo%40example.com&password=12345678", None)));
    let me = body(&handle(&c, &get("/auth/me", "", Some(&t))));
    assert!(me.contains("\"onboarded\":false") && me.contains("\"inviteLink\":\"https://figcraft.example.com/login\""), "{me}");

    let r = handle(
        &c,
        &post_json(
            "/auth/profile",
            r#"{"name":"Jo Park","role":"design","experience":"first","plan":"starter","createFirst":"app","tips":true}"#,
            &t,
        ),
    );
    assert_eq!(r.status, 200, "{}", body(&r));
    assert!(body(&r).contains("\"name\":\"Jo Park\"") && body(&r).contains("\"role\":\"design\"") && body(&r).contains("\"tips\":true"));
    let r = handle(&c, &post_json("/auth/profile", r#"{"role":"other","roleOther":"Art direction","onboarded":true}"#, &t));
    assert!(body(&r).contains("\"roleOther\":\"Art direction\"") && body(&r).contains("\"experience\":\"first\""), "only what's given changes");

    for bad in [
        r#"{"role":"wizard"}"#,
        r#"{"plan":"gold"}"#,
        r#"{"name":"   "}"#,
        r#"{"roleOther":"xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"}"#,
        "nope",
    ] {
        assert_eq!(handle(&c, &post_json("/auth/profile", bad, &t)).status, 400, "{bad}");
    }
    // Kept across a restart.
    let again = Accounts::open(dir.path(), true, 21.0).unwrap();
    let u = again.by_email("jo@example.com").unwrap();
    assert_eq!((u.name.as_str(), u.profile.role.as_str(), u.profile.onboarded), ("Jo Park", "other", true));

    // Signed out, or from another site: refused.
    assert_eq!(handle(&c, &post_json("/auth/profile", r#"{"role":"design"}"#, "forged")).status, 401);
    let mut cross = post_json("/auth/profile", r#"{"role":"design"}"#, &t);
    cross.headers[0].1 = "https://evil.example".into();
    assert_eq!(handle(&c, &cross).status, 403);
}

#[test]
fn invites_are_kept_and_cleaned() {
    let dir = TempDir::new("acc-invite");
    let c = ctx(&dir, true);
    let t = token(&handle(&c, &post("/login", "email=kai%40example.com&password=12345678", None)));
    let r = handle(
        &c,
        &post_json("/auth/invite", r#"{"emails":["Sneha@Example.com"," ","kai@example.com","sneha@example.com","oscar@example.com"]}"#, &t),
    );
    assert_eq!(r.status, 200, "{}", body(&r));
    assert!(body(&r).contains(r#""invited":["sneha@example.com","oscar@example.com"]"#) && body(&r).contains("\"emailed\":false"), "{}", body(&r));
    // Asked again: nothing new.
    assert!(body(&handle(&c, &post_json("/auth/invite", r#"{"emails":["oscar@example.com"]}"#, &t))).contains(r#""invited":[]"#));
    assert!(body(&handle(&c, &get("/auth/me", "", Some(&t)))).contains(r#""invited":["sneha@example.com","oscar@example.com"]"#));
    assert_eq!(handle(&c, &post_json("/auth/invite", r#"{"emails":["not an email"]}"#, &t)).status, 400);
    let many: Vec<String> = (0..21).map(|i| format!("\"p{i}@example.com\"")).collect();
    assert_eq!(handle(&c, &post_json("/auth/invite", &format!("{{\"emails\":[{}]}}", many.join(",")), &t)).status, 400);
    assert_eq!(handle(&c, &post_json("/auth/invite", r#"{"emails":["x@example.com"]}"#, "forged")).status, 401);
}

/// Each account keeps its own recent files: newest first, renamed in place, forgotten on request,
/// kept across a restart, and never handed to another account.
#[test]
fn recent_files_belong_to_their_account() {
    let dir = TempDir::new("acc-recents");
    let c = ctx(&dir, true);
    let ada = token(&handle(&c, &post("/login", "email=ada%40example.com&password=12345678", None)));
    let bo = token(&handle(&c, &post("/login", "email=bo%40example.com&password=12345678", None)));
    assert_eq!(body(&handle(&c, &get("/auth/recents", "", Some(&ada)))), r#"{"files":[]}"#);
    for (target, name) in [("K7M-2QX-9TD", "Landing"), ("B0A-RD1-XYZ", "Ideas"), ("K7M-2QX-9TD", "Landing v2")] {
        let r = handle(&c, &post_json("/auth/recents", &format!(r#"{{"open":{{"target":"{target}","name":"{name}","kind":"design"}}}}"#), &ada));
        assert_eq!(r.status, 200, "{}", body(&r));
    }
    let listed = body(&handle(&c, &get("/auth/recents", "", Some(&ada))));
    let v: serde_json::Value = serde_json::from_str(&listed).unwrap();
    let names: Vec<&str> = v["files"].as_array().unwrap().iter().map(|f| f["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["Landing v2", "Ideas"], "newest first, one entry a file");
    // Someone else's list is their own.
    assert_eq!(body(&handle(&c, &get("/auth/recents", "", Some(&bo)))), r#"{"files":[]}"#);
    // Forgotten, and kept across a restart.
    assert_eq!(handle(&c, &post_json("/auth/recents", r#"{"forget":"B0A-RD1-XYZ"}"#, &ada)).status, 200);
    let again = Accounts::open(dir.path(), true, 21.0).unwrap();
    let id = again.by_email("ada@example.com").unwrap().id.clone();
    let kept = again.recents(&id);
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].name, "Landing v2");
    // Refused: signed out, from another site, junk.
    assert_eq!(handle(&c, &get("/auth/recents", "", Some("forged"))).status, 401);
    assert_eq!(handle(&c, &post_json("/auth/recents", r#"{"forget":"x"}"#, "forged")).status, 401);
    let mut cross = post_json("/auth/recents", r#"{"forget":"x"}"#, &ada);
    cross.headers[0].1 = "https://evil.example".into();
    assert_eq!(handle(&c, &cross).status, 403);
    let long = "x".repeat(accounts::recents::MAX_TARGET + 1);
    for bad in ["nope", "{}", r#"{"open":{"target":"  "}}"#, &format!(r#"{{"forget":"{long}"}}"#), r#"{"open":{"target":"a"},"other":1}"#] {
        assert_eq!(handle(&c, &post_json("/auth/recents", bad, &ada)).status, 400, "{bad}");
    }
    // At most MAX a list.
    for i in 0..accounts::recents::MAX + 5 {
        handle(&c, &post_json("/auth/recents", &format!(r#"{{"open":{{"target":"T-{i}"}}}}"#), &ada));
    }
    assert_eq!(Accounts::open(dir.path(), true, 21.0).unwrap().recents(&id).len(), accounts::recents::MAX);
}

/// A PNG `w` by `h` as far as its header says (enough for the server, which never decodes it).
fn png_header(w: u32, h: u32) -> Vec<u8> {
    let mut b = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13];
    b.extend_from_slice(b"IHDR");
    b.extend_from_slice(&w.to_be_bytes());
    b.extend_from_slice(&h.to_be_bytes());
    b.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);
    b
}

/// A recent file's picture is kept with it: only for a listed file and its own account, handed back
/// by a token that changes with the picture, kept when the file is opened again, and gone with the
/// file when it leaves the list.
#[test]
fn recent_files_keep_their_pictures() {
    use base64::engine::general_purpose::STANDARD;
    let dir = TempDir::new("acc-recent-pictures");
    let c = ctx(&dir, true);
    let ada = token(&handle(&c, &post("/login", "email=ada%40example.com&password=12345678", None)));
    let bo = token(&handle(&c, &post("/login", "email=bo%40example.com&password=12345678", None)));
    let open = |target: &str| post_json("/auth/recents", &format!(r#"{{"open":{{"target":"{target}","name":"Landing"}}}}"#), &ada);
    let send = |target: &str, png: &[u8], who: &str| {
        handle(&c, &post_json("/auth/recents/picture", &format!(r#"{{"target":"{target}","png":"{}"}}"#, STANDARD.encode(png)), who))
    };
    let token_of = |target: &str| -> Option<String> {
        let v: serde_json::Value = serde_json::from_str(&body(&handle(&c, &get("/auth/recents", "", Some(&ada))))).unwrap();
        v["files"].as_array().unwrap().iter().find(|f| f["target"] == target).and_then(|f| f["picture"].as_str().map(String::from))
    };
    // Not in the list: nothing kept.
    let first = png_header(640, 360);
    assert_eq!(send("K7M-2QX-9TD", &first, &ada).status, 400);
    assert_eq!(handle(&c, &open("K7M-2QX-9TD")).status, 200);
    assert_eq!(token_of("K7M-2QX-9TD"), None);
    let r = send("K7M-2QX-9TD", &first, &ada);
    assert_eq!(r.status, 200, "{}", body(&r));
    let t1 = token_of("K7M-2QX-9TD").expect("a token");
    assert!(!t1.contains("K7M"), "the token never names the target");
    let got = handle(&c, &get("/auth/recents/picture", &format!("p={t1}"), Some(&ada)));
    assert_eq!((got.status, got.body.clone()), (200, first.clone()));
    // Someone else, or signed out, gets nothing.
    assert_eq!(handle(&c, &get("/auth/recents/picture", &format!("p={t1}"), Some(&bo))).status, 404);
    assert_eq!(handle(&c, &get("/auth/recents/picture", &format!("p={t1}"), None)).status, 404);
    // A new picture, a new token; the old one is gone.
    let second = png_header(320, 180);
    assert_eq!(send("K7M-2QX-9TD", &second, &ada).status, 200);
    let t2 = token_of("K7M-2QX-9TD").unwrap();
    assert_ne!(t1, t2);
    assert_eq!(handle(&c, &get("/auth/recents/picture", &format!("p={t1}"), Some(&ada))).status, 404);
    assert_eq!(handle(&c, &get("/auth/recents/picture", &format!("p={t2}"), Some(&ada))).body, second);
    // Opened again: the picture stays.
    assert_eq!(handle(&c, &open("K7M-2QX-9TD")).status, 200);
    assert_eq!(token_of("K7M-2QX-9TD").as_deref(), Some(t2.as_str()));
    // Junk is refused: not a PNG, too large a side, too many bytes, not base64.
    for bad in [b"not a png".to_vec(), png_header(0, 10), png_header(4096, 10), {
        let mut big = png_header(10, 10);
        big.resize(accounts::recents::MAX_PICTURE + 1, 0);
        big
    }] {
        assert_eq!(send("K7M-2QX-9TD", &bad, &ada).status, 400);
    }
    assert_eq!(handle(&c, &post_json("/auth/recents/picture", r#"{"target":"K7M-2QX-9TD","png":"%%%"}"#, &ada)).status, 400);
    assert_eq!(send("K7M-2QX-9TD", &first, "forged").status, 401);
    // Forgotten: its picture goes too.
    let pictures = dir.path().join("accounts").join("recents");
    let count = || {
        std::fs::read_dir(&pictures)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.path().is_dir())
            .flat_map(|d| std::fs::read_dir(d.path()).unwrap())
            .count()
    };
    assert_eq!(count(), 1);
    assert_eq!(handle(&c, &post_json("/auth/recents", r#"{"forget":"K7M-2QX-9TD"}"#, &ada)).status, 200);
    assert_eq!(count(), 0);
    assert_eq!(handle(&c, &get("/auth/recents/picture", &format!("p={t2}"), Some(&ada))).status, 404);
}

#[test]
fn folders_belong_to_their_account_and_a_file_is_in_one_at_a_time() {
    let dir = TempDir::new("acc-folders");
    let c = ctx(&dir, true);
    let ada = token(&handle(&c, &post("/login", "email=ada%40example.com&password=12345678", None)));
    let bo = token(&handle(&c, &post("/login", "email=bo%40example.com&password=12345678", None)));
    assert_eq!(body(&handle(&c, &get("/auth/folders", "", Some(&ada)))), r#"{"folders":[]}"#);
    for change in [
        r#"{"create":{"id":"f1","name":"Brand"}}"#,
        r#"{"create":{"id":"f2","name":"  "}}"#,
        r#"{"rename":{"id":"f1","name":"Brand kit"}}"#,
        r#"{"star":{"id":"f1","on":true}}"#,
        r#"{"add":{"id":"f1","file":{"target":"K7M-2QX-9TD","name":"Landing","kind":"board"}}}"#,
        r#"{"add":{"id":"f2","file":{"target":"AAA-BBB-CCC","name":"Ideas"}}}"#,
        r#"{"add":{"id":"f2","file":{"target":"K7M-2QX-9TD","name":"Landing"}}}"#,
    ] {
        let r = handle(&c, &post_json("/auth/folders", change, &ada));
        assert_eq!(r.status, 200, "{change}: {}", body(&r));
    }
    let v: serde_json::Value = serde_json::from_str(&body(&handle(&c, &get("/auth/folders", "", Some(&ada))))).unwrap();
    let folders = v["folders"].as_array().unwrap();
    assert_eq!(folders.iter().map(|f| f["id"].as_str().unwrap()).collect::<Vec<_>>(), ["f2", "f1"], "newest first");
    assert_eq!(folders[0]["name"], "Untitled");
    assert_eq!(folders[1]["name"], "Brand kit");
    assert_eq!(folders[1]["starred"], true);
    let targets =
        |f: &serde_json::Value| f["files"].as_array().unwrap().iter().map(|x| x["target"].as_str().unwrap().to_string()).collect::<Vec<_>>();
    assert_eq!(targets(&folders[0]), ["K7M-2QX-9TD", "AAA-BBB-CCC"], "moved into f2, newest first");
    assert!(targets(&folders[1]).is_empty(), "and out of f1");
    // Someone else's folders are their own.
    assert_eq!(body(&handle(&c, &get("/auth/folders", "", Some(&bo)))), r#"{"folders":[]}"#);
    // Taken out, deleted, and kept across a restart.
    assert_eq!(handle(&c, &post_json("/auth/folders", r#"{"remove":"AAA-BBB-CCC"}"#, &ada)).status, 200);
    assert_eq!(handle(&c, &post_json("/auth/folders", r#"{"delete":"f1"}"#, &ada)).status, 200);
    let again = Accounts::open(dir.path(), true, 21.0).unwrap();
    let id = again.by_email("ada@example.com").unwrap().id.clone();
    let kept = again.folders(&id);
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].files.len(), 1);
    // Refused: signed out, from another site, junk, two changes at once, a missing folder.
    assert_eq!(handle(&c, &get("/auth/folders", "", Some("forged"))).status, 401);
    assert_eq!(handle(&c, &post_json("/auth/folders", r#"{"delete":"f2"}"#, "forged")).status, 401);
    let mut cross = post_json("/auth/folders", r#"{"delete":"f2"}"#, &ada);
    cross.headers[0].1 = "https://evil.example".into();
    assert_eq!(handle(&c, &cross).status, 403);
    let long = "x".repeat(accounts::folders::MAX_TARGET + 1);
    for bad in [
        "nope",
        "{}",
        r#"{"create":{"id":"f2","name":"again"}}"#,
        r#"{"create":{"id":"bad id"}}"#,
        r#"{"delete":"f1","remove":"x"}"#,
        r#"{"rename":{"id":"gone","name":"x"}}"#,
        r#"{"star":{"id":"f2"}}"#,
        r#"{"other":1}"#,
        &format!(r#"{{"add":{{"id":"f2","file":{{"target":"{long}"}}}}}}"#),
    ] {
        assert_eq!(handle(&c, &post_json("/auth/folders", bad, &ada)).status, 400, "{bad}");
    }
    // At most MAX folders.
    for i in 0..accounts::folders::MAX + 5 {
        handle(&c, &post_json("/auth/folders", &format!(r#"{{"create":{{"id":"n{i}"}}}}"#), &ada));
    }
    assert_eq!(Accounts::open(dir.path(), true, 21.0).unwrap().folders(&id).len(), accounts::folders::MAX);
}

/// A change whose write to disk fails leaves the accounts as they were, in memory and on disk
/// (forced by making the accounts folder read-only; skipped where that doesn't stop writes, as for
/// root).
#[cfg(unix)]
#[test]
fn a_change_that_cant_be_written_leaves_the_accounts_as_they_were() {
    use std::os::unix::fs::PermissionsExt;
    let dir = TempDir::new("acc-rollback");
    let mut a = Accounts::open(dir.path(), true, 21.0).unwrap();
    let u = a.create("ada@example.com", "Ada", Some(accounts::password::hash("12345678").unwrap()), None).unwrap();
    let token = a.start_session(&u.id).unwrap();
    a.invite(&u.id, &["bo@example.com".into()]).unwrap();
    let folder = dir.path().join(accounts::DIR);
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o500)).unwrap();
    if std::fs::write(folder.join("probe"), b"x").is_ok() {
        std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o700)).unwrap();
        return;
    }
    assert!(a.set_disabled("ada@example.com", true).is_err());
    assert!(a.session(&token).is_some(), "not disabled, still signed in");
    assert!(a.set_password("ada@example.com", accounts::password::hash("87654321").unwrap()).is_err());
    assert!(a.session(&token).is_some(), "the password change didn't sign it out");
    assert!(a.remove("ada@example.com").is_err());
    assert!(a.by_email("ada@example.com").is_some(), "not removed");
    let renamed = accounts::ProfileUpdate { name: Some("Bo".into()), ..Default::default() };
    assert!(a.update_profile(&u.id, &renamed).is_err());
    assert_eq!(a.by_email("ada@example.com").map(|u| u.name.as_str()), Some("Ada"));
    assert!(a.end_session(&token).is_err());
    assert!(a.session(&token).is_some(), "still signed in after a sign-out that wasn't kept");
    assert!(a.invite(&u.id, &["cy@example.com".into()]).is_err());
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o700)).unwrap();
    // Disk agrees: opened again, nothing changed.
    let mut again = Accounts::open(dir.path(), true, 21.0).unwrap();
    let kept = again.by_email("ada@example.com").cloned().expect("still there");
    assert!(!kept.disabled && kept.name == "Ada");
    assert!(again.session(&token).is_some());
    assert_eq!(again.invite(&u.id, &["bo@example.com".into()]).unwrap(), Vec::<String>::new(), "the first invite was kept, the second wasn't");
}

#[test]
fn a_connection_slot_is_given_back_however_its_connection_ends() {
    let open = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let load = |o: &Arc<std::sync::atomic::AtomicUsize>| o.load(std::sync::atomic::Ordering::SeqCst);
    // Served, then done.
    drop(http::OpenSlot::take(&open));
    assert_eq!(load(&open), 0);
    // A thread that never ran: the closure holding the slot is dropped.
    let slot = http::OpenSlot::take(&open);
    let never = move || drop(slot);
    assert_eq!(load(&open), 1);
    drop(never);
    assert_eq!(load(&open), 0);
    // A panic while serving.
    let slot = http::OpenSlot::take(&open);
    let joined = std::thread::spawn(move || {
        let _slot = slot;
        panic!("serving failed");
    })
    .join();
    assert!(joined.is_err());
    assert_eq!(load(&open), 0);
}

/// Every sign-in that fails answers the same way, so the page can't be used to learn which
/// addresses have accounts, how they sign in or whether they are turned off.
#[test]
fn failed_sign_ins_all_look_the_same() {
    let dir = TempDir::new("acc-same");
    let mut a = Accounts::open(dir.path(), false, 21.0).unwrap();
    let pw = http::operate(&mut a, &AdminReq { action: "add".into(), email: "pw@example.com".into(), ..AdminReq::default() }).unwrap().password;
    http::operate(&mut a, &AdminReq { action: "add".into(), email: "off@example.com".into(), ..AdminReq::default() }).unwrap();
    http::operate(&mut a, &AdminReq { action: "disable".into(), email: "off@example.com".into(), ..AdminReq::default() }).unwrap();
    a.add_by_operator("g@example.com", "G", None).unwrap();
    drop(a);
    let c = ctx(&dir, false);
    let answer = |email: &str, password: &str| {
        let r = handle(&c, &post("/login", &format!("email={}&password={password}", email.replace('@', "%40")), None));
        // The address typed is put back in the form; nothing else may differ.
        (r.status, body(&r).replace(email, "EMAIL"))
    };
    let wrong = answer("pw@example.com", "not-the-password");
    assert_eq!(wrong.0, 401);
    for (email, why) in [("nobody@example.com", "no account"), ("off@example.com", "turned off"), ("g@example.com", "Google only")] {
        assert_eq!(answer(email, "not-the-password"), wrong, "{why}");
    }
    assert_eq!(handle(&c, &post("/login", &format!("email=pw%40example.com&password={pw}"), None)).status, 303, "the right password still works");
}
