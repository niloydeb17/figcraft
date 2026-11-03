//! The signed-in account and the onboarding answers, as the interface and the platform use them.

use serde_json::json;

use crate::Session;
use crate::account::Outgoing;

fn signed_in(s: &mut Session, onboarded: bool) {
    s.execute(
        "account.set",
        &json!({"signedIn": true, "email": "Ada@Example.com", "name": "Ada", "inviteLink": "https://f.example/login", "profile": {"onboarded": onboarded}}),
    )
    .unwrap();
}

#[test]
fn a_new_account_needs_onboarding_and_answers_are_queued_for_its_server() {
    let mut s = Session::new();
    assert!(!s.account.needs_onboarding(), "nobody signed in");
    signed_in(&mut s, false);
    assert!(s.account.needs_onboarding());
    assert_eq!(s.prefs.user_name, "Ada", "the account's name signs comments");
    s.execute("onboarding.answer", &json!({"name": "Ada L", "tips": true})).unwrap();
    s.execute("onboarding.answer", &json!({"role": "other", "roleOther": "Art direction"})).unwrap();
    s.execute("onboarding.answer", &json!({"experience": "first", "plan": "professional"})).unwrap();
    assert_eq!(s.prefs.user_name, "Ada L");
    let sent = s.account.take_outgoing();
    assert_eq!(sent.len(), 3);
    assert_eq!(sent[0], Outgoing::Profile(json!({"name": "Ada L", "tips": true})));
    assert_eq!(sent[1], Outgoing::Profile(json!({"role": "other", "roleOther": "Art direction"})));
    assert!(s.account.take_outgoing().is_empty(), "taken once");
    let me = s.execute("account.get", &json!({})).unwrap();
    assert_eq!(me["profile"]["plan"], "professional");
    assert_eq!(me["email"], "ada@example.com");
}

#[test]
fn wrong_answers_are_refused() {
    let mut s = Session::new();
    signed_in(&mut s, false);
    for bad in [
        json!({"role": "wizard"}),
        json!({"experience": "lots"}),
        json!({"plan": "gold"}),
        json!({"createFirst": "film"}),
        json!({"name": "  "}),
        json!({"role": 3}),
    ] {
        assert!(s.execute("onboarding.answer", &bad).is_err(), "{bad}");
    }
    assert!(s.execute("onboarding.answer", &json!({"roleOther": "x".repeat(61)})).is_err());
    assert!(s.account.take_outgoing().is_empty());
}

#[test]
fn invites_are_cleaned_and_need_an_account() {
    let mut s = Session::new();
    assert!(s.execute("onboarding.invite", &json!({"emails": ["a@example.com"]})).is_err(), "signed out");
    signed_in(&mut s, false);
    let r = s.execute("onboarding.invite", &json!({"emails": ["Sneha@Example.com", "", "sneha@example.com", "oscar@example.com"]})).unwrap();
    assert_eq!(r["queued"], json!(["sneha@example.com", "oscar@example.com"]));
    assert_eq!(r["link"], "https://f.example/login");
    assert_eq!(s.account.take_outgoing(), vec![Outgoing::Invite(vec!["sneha@example.com".into(), "oscar@example.com".into()])]);
    assert!(s.execute("onboarding.invite", &json!({"emails": ["not an email"]})).is_err());
    let many: Vec<String> = (0..21).map(|i| format!("p{i}@example.com")).collect();
    assert!(s.execute("onboarding.invite", &json!({"emails": many})).is_err());
}

#[test]
fn finishing_opens_the_first_file() {
    let mut s = Session::new();
    signed_in(&mut s, false);
    let files = s.docs.len();
    let r = s.execute("onboarding.finish", &json!({"createFirst": "app"})).unwrap();
    assert_eq!(r["file"]["name"], "Ada's App");
    assert_eq!(s.docs.len(), files + 1);
    assert_eq!(s.ds().unwrap().doc.name, "Ada's App");
    assert_eq!(s.ds().unwrap().selection.len(), 1, "the starting frame is selected");
    assert!(!s.account.needs_onboarding());
    assert!(s.account.take_outgoing().contains(&Outgoing::Profile(json!({"onboarded": true, "createFirst": "app"}))));

    let r = s.execute("onboarding.finish", &json!({"createFirst": "whiteboard"})).unwrap();
    assert_eq!(r["file"]["name"], "Ada's Whiteboard");
    assert_eq!(s.ds().unwrap().doc.kind, figcraft_doc::FileKind::Board);
    // Skipped: done, no file.
    let before = s.docs.len();
    let r = s.execute("onboarding.finish", &json!({"createFirst": ""})).unwrap();
    assert!(r["file"].is_null());
    assert_eq!(s.docs.len(), before);
    s.execute("onboarding.restart", &json!({})).unwrap();
    assert!(s.account.needs_onboarding());
}

#[test]
fn signing_out_drops_the_account_and_what_waits() {
    let mut s = Session::new();
    signed_in(&mut s, false);
    s.execute("onboarding.answer", &json!({"role": "design"})).unwrap();
    s.execute("account.set", &json!({"signedIn": false})).unwrap();
    assert!(s.account.account.is_none() && s.account.take_outgoing().is_empty());
    // Signed out, answers stay local and nothing is queued.
    s.execute("onboarding.answer", &json!({"role": "student"})).unwrap();
    assert!(s.account.take_outgoing().is_empty());
    assert!(s.execute("account.set", &json!({"signedIn": true})).is_err(), "a signed-in account has an email");
    assert!(s.execute("help.tour", &json!({})).is_ok());
}
