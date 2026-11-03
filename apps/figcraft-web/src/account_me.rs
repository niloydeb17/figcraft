//! The page's server's `/auth/me` answer as `account.set`'s parameters.

use serde_json::{Value, json};

/// `account.set`'s parameters for an `/auth/me` answer (`None` when it isn't one: a server
/// without accounts, or an error page).
pub fn params(status: u16, body: &str) -> Option<Value> {
    if status == 401 {
        return Some(json!({"signedIn": false}));
    }
    if status != 200 {
        return None;
    }
    let me: Value = serde_json::from_str(body).ok()?;
    if me.get("signedIn").and_then(Value::as_bool) != Some(true) {
        return Some(json!({"signedIn": false}));
    }
    let user = me.get("user")?;
    Some(json!({
        "signedIn": true,
        "email": user.get("email")?.as_str()?,
        "name": user.get("name").and_then(Value::as_str).unwrap_or_default(),
        "inviteLink": me.get("inviteLink").and_then(Value::as_str).unwrap_or_default(),
        "invited": user.get("invited").cloned().unwrap_or_else(|| json!([])),
        "profile": user.get("profile").cloned().unwrap_or_else(|| json!({})),
    }))
}

/// Whether an `/auth/me` answer says the account has a picture to fetch (`GET /auth/avatar`).
pub fn has_picture(status: u16, body: &str) -> bool {
    status == 200 && serde_json::from_str::<Value>(body).ok().and_then(|me| me.get("user")?.get("hasPicture")?.as_bool()).unwrap_or(false)
}
