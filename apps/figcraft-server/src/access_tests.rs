//! Who has access to a file, against a real server, as the Share dialog changes it (as in the
//! incumbent): what anyone with the link may do (lowering it makes new links, so an old one never
//! keeps more access; no access revokes them all), and people invited by email with a link of
//! their own each, whose access changes or goes on its own.

use figcraft_engine::Session;
use serde_json::{Value, json};

use crate::config::Config;
use crate::test_util::{TempDir, config, live, pump, session};

fn ex(s: &mut Session, cmd: &str, p: Value) -> Value {
    s.execute(cmd, &p).unwrap_or_else(|e| panic!("{cmd}: {e}"))
}

/// Ada's open file, shared online through the server at `url`.
fn shared(url: &str) -> Session {
    let mut a = session("Ada");
    ex(&mut a, "pref.publishServer", json!({"url": url}));
    ex(&mut a, "shape.rectangle", json!({"x": 10, "y": 10, "width": 40, "height": 30}));
    ex(&mut a, "collab.share", json!({}));
    pump(&mut [&mut a], "the share", |s| live(s[0]) && s[0].live().share.as_ref().is_some_and(|sh| sh.links.is_some()));
    a
}

/// Someone joining with `link`: whether they got in (else the join failed).
fn joins(url: &str, link: &str) -> bool {
    use figcraft_engine::live::Status;
    let mut b = session("Bo");
    ex(&mut b, "pref.publishServer", json!({"url": url}));
    if b.execute("collab.join", &json!({"link": link})).is_err() {
        return false;
    }
    pump(&mut [&mut b], "the join to end one way or the other", |s| {
        s[0].live.iter().any(|l| l.status == Status::Live || matches!(l.status, Status::Failed { .. }))
    });
    b.live.iter().any(|l| l.status == Status::Live)
}

#[test]
fn link_access_and_invited_people() {
    let dir = TempDir::new("access");
    // The test joins over and over; the budget of new connections is the point of another test.
    let mut server = crate::start(&Config { open_publish: true, connects_per_minute: 10_000, ..config(dir.path()) }).unwrap();
    let url = format!("ws://{}/", server.addr());
    let mut a = shared(&url);
    let at = ex(&mut a, "collab.access", json!({}));
    assert_eq!((&at["owner"], &at["link"], &at["people"]), (&json!(true), &json!("edit"), &json!([])));

    // Raising or keeping access only changes which link is given.
    let edit = a.live().share.as_ref().and_then(|s| s.link()).unwrap().to_string();
    // Lowering it to view: the edit link given out before no longer gets in.
    let r = ex(&mut a, "collab.linkAccess", json!({"access": "view"}));
    assert_eq!(r["link"], json!("view"));
    let view = r["copy"].as_str().unwrap().to_string();
    assert!(!joins(&url, &edit), "the old edit link was revoked");
    assert!(joins(&url, &view));
    // No access: no link gets in.
    assert_eq!(ex(&mut a, "collab.linkAccess", json!({"access": "none"}))["link"], json!("none"));
    assert!(!joins(&url, &view));
    assert_eq!(ex(&mut a, "collab.access", json!({}))["link"], json!("none"));
    // Back to edit: new links.
    let r = ex(&mut a, "collab.linkAccess", json!({"access": "edit"}));
    assert!(joins(&url, r["copy"].as_str().unwrap()));

    // Invited by email: a link each, listed by address.
    let r = ex(&mut a, "collab.invite", json!({"emails": ["Cy@Example.com", "dee@example.com", "cy@example.com"], "access": "view"}));
    let invited = r["invited"].as_array().unwrap();
    assert_eq!(invited.len(), 2);
    assert_eq!(r["delivered"], json!(false), "not signed in: nothing goes to an account");
    let cy = invited[0]["link"].as_str().unwrap().to_string();
    assert!(cy.starts_with("figcraft://join?") && cy.contains("file="), "{cy}");
    let people = ex(&mut a, "collab.access", json!({}))["people"].clone();
    assert_eq!(
        people.as_array().unwrap().iter().map(|p| (p["email"].as_str().unwrap(), p["access"].as_str().unwrap())).collect::<Vec<_>>(),
        [("cy@example.com", "view"), ("dee@example.com", "view")]
    );
    // Link access going to none leaves invited people in.
    ex(&mut a, "collab.linkAccess", json!({"access": "none"}));
    assert!(joins(&url, &cy));
    // A new level: a new link, the old one revoked.
    let r = ex(&mut a, "collab.setAccess", json!({"email": "cy@example.com", "access": "edit"}));
    assert_eq!(r["access"], json!("edit"));
    assert!(!joins(&url, &cy));
    let people = ex(&mut a, "collab.access", json!({}))["people"].clone();
    let cy_now = people.as_array().unwrap().iter().find(|p| p["email"] == "cy@example.com").unwrap();
    assert_eq!((&cy_now["access"], people.as_array().unwrap().len()), (&json!("edit"), 2));
    // Removed: gone from the list.
    ex(&mut a, "collab.setAccess", json!({"email": "dee@example.com", "access": "remove"}));
    let people = ex(&mut a, "collab.access", json!({}))["people"].clone();
    assert_eq!(people.as_array().unwrap().len(), 1);

    // Refused with a reason: junk, someone not invited, and anyone who isn't the owner.
    for (cmd, p) in [
        ("collab.invite", json!({"emails": ["not an email"]})),
        ("collab.invite", json!({"emails": []})),
        ("collab.invite", json!({"emails": "x@example.com"})),
        ("collab.setAccess", json!({"email": "nobody@example.com", "access": "view"})),
        ("collab.setAccess", json!({"email": "cy@example.com", "access": "owner"})),
        ("collab.linkAccess", json!({"access": "all"})),
        ("collab.linkAccess", json!({})),
    ] {
        assert!(a.execute(cmd, &p).is_err(), "{cmd} {p}");
    }
    let mut lone = session("Eve");
    assert_eq!(ex(&mut lone, "collab.access", json!({}))["owner"], json!(false));
    assert!(lone.execute("collab.invite", &json!({"emails": ["x@example.com"]})).is_err());
    assert!(lone.execute("collab.linkAccess", &json!({"access": "view"})).is_err());
    server.stop();
}

/// Inviting the most people at once takes two connections (the file's links, then one batch), so
/// a server's usual budgets (new connections a minute, changes to its files a minute) never stop
/// it; a file is shared with at most 50 people.
#[test]
fn twenty_invites_fit_the_usual_budgets() {
    let dir = TempDir::new("access-budget");
    let cfg = Config { open_publish: true, ..config(dir.path()) };
    assert!(cfg.connects_per_minute <= 30, "the test means the usual budget");
    let mut server = crate::start(&cfg).unwrap();
    let mut a = shared(&format!("ws://{}/", server.addr()));
    let emails = |r: std::ops::Range<usize>| -> Vec<String> { r.map(|i| format!("p{i}@example.com")).collect() };
    for r in [0..20, 20..40, 40..49] {
        let n = r.len();
        assert_eq!(ex(&mut a, "collab.invite", json!({"emails": emails(r)}))["invited"].as_array().unwrap().len(), n);
    }
    assert_eq!(ex(&mut a, "collab.access", json!({}))["people"].as_array().unwrap().len(), 49);
    let e = a.execute("collab.invite", &json!({"emails": emails(49..51)})).unwrap_err().to_string();
    assert!(e.contains("at most 50 people"), "{e}");
    server.stop();
}

/// The server says where its browser build is (`web_url`), to anyone who asks, so Copy link gives a
/// link that opens in any browser in the desktop app as in the browser; and that link joins the
/// file from the desktop app too.
#[test]
fn the_server_names_its_browser_build_and_its_links_join() {
    let dir = TempDir::new("access-web");
    let cfg = Config { open_publish: true, web_url: "https://figcraft.example/".into(), ..config(dir.path()) };
    let mut server = crate::start(&cfg).unwrap();
    let url = format!("ws://{}/", server.addr());
    let mut a = shared(&url);
    let mut lone = session("Eve");
    assert_eq!(ex(&mut lone, "collab.webAddress", json!({"server": url}))["web"], json!("https://figcraft.example/"), "anyone may ask");
    // The file's link as Copy link gives it, from a browser build: it joins from the desktop app.
    let link = a.live().share.as_ref().and_then(|s| s.link()).unwrap().to_string();
    let web = figcraft_engine::team::file_link("https://figcraft.example/", &link);
    assert!(web.starts_with("https://figcraft.example/#file="));
    assert!(joins(&url, &web), "{web}");
    assert_eq!(ex(&mut a, "collab.webAddress", json!({}))["web"], json!("https://figcraft.example/"), "the open file's server");
    server.stop();
    // A server that names none.
    let dir = TempDir::new("access-noweb");
    let mut server = crate::start(&Config { open_publish: true, ..config(dir.path()) }).unwrap();
    let url = format!("ws://{}/", server.addr());
    assert_eq!(ex(&mut lone, "collab.webAddress", json!({"server": url}))["web"], Value::Null);
    server.stop();
}
