use serde_json::json;

use crate::account_me::params;

#[test]
fn me_becomes_the_account() {
    let body = r#"{"signedIn":true,"user":{"id":"a1","email":"ada@example.com","name":"Ada","profile":{"role":"design","onboarded":false},"invited":["b@example.com"]},"inviteLink":"https://f.example/login"}"#;
    let p = params(200, body).unwrap();
    assert_eq!(p["email"], "ada@example.com");
    assert_eq!(p["profile"]["role"], "design");
    assert_eq!(p["invited"], json!(["b@example.com"]));
    assert_eq!(p["inviteLink"], "https://f.example/login");
    assert_eq!(params(401, ""), Some(json!({"signedIn": false})));
    // A server without accounts (the page itself, a 404) or junk: nothing to say.
    assert_eq!(params(404, "<html>"), None);
    assert_eq!(params(200, "<!doctype html>"), None);
    assert_eq!(params(200, r#"{"signedIn":true,"user":{}}"#), None);
}
