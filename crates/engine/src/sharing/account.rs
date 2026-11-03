//! The signed-in account and the first-run onboarding (milestone M9).
//!
//! The app's platform reports who is signed in (`account.set`: the browser build asks its server's
//! `/auth/me`); the answers to the onboarding questions and the people invited are kept here and
//! queued in [`AccountState::outbox`] for the platform to send to the server (`/auth/profile`,
//! `/auth/invite`). Headless sessions and the desktop app have no account: the answers stay local.
//! The onboarding screens themselves are the interface's; these commands only keep what they say.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::cmd::CommandSpec;
use crate::params::{boolean, get, string};
use crate::{EngineError, Result, Session, bad, spec};

pub const ROLES: &[&str] = &[
    "design",
    "research",
    "ux-writing",
    "student",
    "data-analytics",
    "software-development",
    "marketing",
    "product-management",
    "educator",
    "other",
];
pub const EXPERIENCE: &[&str] = &["many", "few", "first"];
pub const PLANS: &[&str] = &["starter", "professional", "organization"];
pub const CREATE_FIRST: &[&str] = &["app", "website", "graphic", "animation", "presentation", "whiteboard"];

/// Most addresses in one invite, as the server takes them.
pub const INVITES_AT_ONCE: usize = 20;

/// What the person said about themselves (the server's `profile`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Profile {
    pub role: String,
    pub role_other: String,
    pub experience: String,
    pub plan: String,
    pub create_first: String,
    pub tips: bool,
    pub onboarded: bool,
}

/// Who is signed in.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Account {
    pub email: String,
    pub name: String,
    /// The address that lets others sign up on this server.
    pub invite_link: String,
    /// Who this account has invited.
    pub invited: Vec<String>,
    pub profile: Profile,
}

/// What to send to the account's server.
#[derive(Clone, Debug, PartialEq)]
pub enum Outgoing {
    /// `/auth/profile`: the fields that changed.
    Profile(Value),
    /// `/auth/invite`: addresses to invite.
    Invite(Vec<String>),
}

/// The account, the local answers and what waits to be sent.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AccountState {
    pub account: Option<Account>,
    /// The answers given in this app (the account's when signed in).
    pub profile: Profile,
    /// For the platform to send and clear ([`AccountState::take_outgoing`]).
    pub outbox: Vec<Outgoing>,
}

impl AccountState {
    /// Signed in and not yet through onboarding: the interface shows it.
    pub fn needs_onboarding(&self) -> bool {
        self.account.as_ref().is_some_and(|a| !a.profile.onboarded)
    }

    /// What the platform should send now.
    pub fn take_outgoing(&mut self) -> Vec<Outgoing> {
        std::mem::take(&mut self.outbox)
    }

    fn queue(&mut self, o: Outgoing) {
        if self.account.is_some() {
            self.outbox.push(o);
        }
    }
}

/// An email address as the server takes it (trimmed, lowercase), or `None`.
pub fn clean_email(s: &str) -> Option<String> {
    let e = s.trim().to_lowercase();
    let (local, domain) = e.split_once('@')?;
    let ok = (3..=254).contains(&e.len())
        && !local.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !domain.contains('@')
        && e.chars().all(|c| !c.is_whitespace() && !c.is_control() && !matches!(c, '<' | '>' | '"' | '\'' | '\\' | ',' | ';'));
    ok.then_some(e)
}

/// A file's name for what was chosen to create first: "Ada's App".
pub fn first_file_name(name: &str, create: &str) -> String {
    let kind = match create {
        "app" => "App",
        "website" => "Website",
        "graphic" => "Graphic",
        "animation" => "Animation",
        "presentation" => "Presentation",
        "whiteboard" => "Whiteboard",
        _ => "New File",
    };
    let name = name.trim();
    if name.is_empty() { kind.to_string() } else { format!("{name}'s {kind}") }
}

/// The starting frame for a design file made from a choice: name and size.
fn first_frame(create: &str) -> Option<(&'static str, f64, f64)> {
    match create {
        "app" => Some(("Phone", 402.0, 874.0)),
        "website" => Some(("Desktop", 1440.0, 1024.0)),
        "graphic" => Some(("Post", 1080.0, 1080.0)),
        "animation" => Some(("Scene", 1920.0, 1080.0)),
        _ => None,
    }
}

fn choice(cmd: &str, p: &Value, key: &str, allowed: &[&str]) -> Result<Option<String>> {
    let Some(v) = get(p, key) else { return Ok(None) };
    let v = v.as_str().ok_or_else(|| bad(cmd, format!("`{key}` is text")))?.trim();
    if !v.is_empty() && !allowed.contains(&v) {
        return Err(bad(cmd, format!("`{key}` is one of {} (or empty)", allowed.join(", "))));
    }
    Ok(Some(v.to_string()))
}

fn account_json(s: &Session) -> Value {
    let st = &s.account;
    match &st.account {
        Some(a) => json!({
            "signedIn": true,
            "email": a.email,
            "name": a.name,
            "inviteLink": a.invite_link,
            "invited": a.invited,
            "profile": st.profile,
            "needsOnboarding": st.needs_onboarding(),
        }),
        None => json!({"signedIn": false, "profile": st.profile}),
    }
}

fn set(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "account.set";
    if !boolean(p, "signedIn").unwrap_or(false) {
        s.account.account = None;
        s.account.outbox.clear();
        return Ok(account_json(s));
    }
    let email = string(p, "email").and_then(|e| clean_email(&e)).ok_or_else(|| bad(CMD, "a signed-in account has an `email`"))?;
    let name: String = string(p, "name").unwrap_or_default().trim().chars().filter(|c| !c.is_control()).take(128).collect();
    let invite_link = string(p, "inviteLink").unwrap_or_default().chars().take(500).collect();
    let invited = match get(p, "invited") {
        Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).filter_map(clean_email).take(200).collect(),
        _ => Vec::new(),
    };
    let profile: Profile = match get(p, "profile") {
        Some(v) => serde_json::from_value(v.clone()).map_err(|e| bad(CMD, format!("`profile`: {e}")))?,
        None => Profile::default(),
    };
    // The name others see in comments and live sessions, unless one was chosen here already.
    if !name.is_empty() && s.prefs.user_name == crate::cmd::comment::DEFAULT_AUTHOR {
        s.prefs.user_name = name.clone();
        s.prefs_unsaved = true;
    }
    s.account.profile = profile.clone();
    s.account.account = Some(Account { email, name, invite_link, invited, profile });
    Ok(account_json(s))
}

fn answer(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "onboarding.answer";
    let mut sent = serde_json::Map::new();
    if let Some(name) = string(p, "name") {
        let name: String = name.trim().chars().filter(|c| !c.is_control()).collect();
        if name.is_empty() || name.chars().count() > 128 {
            return Err(bad(CMD, "`name` is 1 to 128 characters"));
        }
        s.prefs.user_name = name.clone();
        s.prefs_unsaved = true;
        if let Some(a) = &mut s.account.account {
            a.name = name.clone();
        }
        sent.insert("name".into(), json!(name));
    }
    let mut next = s.account.profile.clone();
    if let Some(v) = choice(CMD, p, "role", ROLES)? {
        next.role = v;
        sent.insert("role".into(), json!(next.role));
    }
    if let Some(v) = string(p, "roleOther") {
        let v: String = v.trim().chars().filter(|c| !c.is_control()).collect();
        if v.chars().count() > 60 {
            return Err(bad(CMD, "`roleOther` is at most 60 characters"));
        }
        next.role_other = v;
        sent.insert("roleOther".into(), json!(next.role_other));
    }
    if let Some(v) = choice(CMD, p, "experience", EXPERIENCE)? {
        next.experience = v;
        sent.insert("experience".into(), json!(next.experience));
    }
    if let Some(v) = choice(CMD, p, "plan", PLANS)? {
        next.plan = v;
        sent.insert("plan".into(), json!(next.plan));
    }
    if let Some(v) = choice(CMD, p, "createFirst", CREATE_FIRST)? {
        next.create_first = v;
        sent.insert("createFirst".into(), json!(next.create_first));
    }
    if let Some(v) = boolean(p, "tips") {
        next.tips = v;
        sent.insert("tips".into(), json!(v));
    }
    s.account.profile = next.clone();
    if let Some(a) = &mut s.account.account {
        a.profile = next;
    }
    if !sent.is_empty() {
        s.account.queue(Outgoing::Profile(Value::Object(sent)));
    }
    Ok(account_json(s))
}

fn invite(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "onboarding.invite";
    let Some(Value::Array(list)) = get(p, "emails") else { return Err(bad(CMD, "`emails` is a list of email addresses")) };
    if list.len() > INVITES_AT_ONCE {
        return Err(bad(CMD, format!("at most {INVITES_AT_ONCE} addresses at once")));
    }
    let mut emails: Vec<String> = Vec::new();
    for v in list {
        let t = v.as_str().ok_or_else(|| bad(CMD, "`emails` is a list of email addresses"))?;
        if t.trim().is_empty() {
            continue;
        }
        let e = clean_email(t).ok_or_else(|| bad(CMD, format!("{} isn't an email address", t.trim())))?;
        if !emails.contains(&e) {
            emails.push(e);
        }
    }
    let Some(a) = &mut s.account.account else {
        return Err(EngineError::Other("sign in to invite people".into()));
    };
    for e in &emails {
        if !a.invited.contains(e) {
            a.invited.push(e.clone());
        }
    }
    let link = a.invite_link.clone();
    if !emails.is_empty() {
        s.account.queue(Outgoing::Invite(emails.clone()));
    }
    Ok(json!({"queued": emails, "link": link}))
}

fn finish(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "onboarding.finish";
    let create = match choice(CMD, p, "createFirst", CREATE_FIRST)? {
        Some(c) => c,
        None => s.account.profile.create_first.clone(),
    };
    s.account.profile.onboarded = true;
    s.account.profile.create_first = create.clone();
    if let Some(a) = &mut s.account.account {
        a.profile = s.account.profile.clone();
    }
    s.account.queue(Outgoing::Profile(json!({"onboarded": true, "createFirst": create})));
    if create.is_empty() {
        return Ok(json!({"onboarded": true, "file": null}));
    }
    let name = first_file_name(&s.prefs.user_name, &create);
    let made = match create.as_str() {
        "whiteboard" => s.execute("file.newBoard", &json!({"name": name}))?,
        "presentation" => {
            let r = s.execute("file.newFromTemplate", &json!({"template": "slides"}))?;
            if let Ok(ds) = s.ds_mut() {
                std::sync::Arc::make_mut(&mut ds.doc).name = name.clone();
            }
            r
        }
        _ => s.execute("file.new", &json!({"name": name}))?,
    };
    let mut frame = Value::Null;
    if let Some((label, w, h)) = first_frame(&create) {
        let r = s.execute("node.create", &json!({"type": "frame", "name": label, "x": 0, "y": 0, "width": w, "height": h}))?;
        if let Some(id) = r.get("id").cloned() {
            let _ = s.execute("select.set", &json!({"ids": [id.clone()]}));
            let _ = s.execute("view.zoomFit", &json!({}));
            frame = id;
        }
    }
    Ok(json!({"onboarded": true, "file": {"name": name, "index": made.get("index"), "frame": frame}}))
}

fn restart(s: &mut Session) -> Value {
    s.account.profile.onboarded = false;
    if let Some(a) = &mut s.account.account {
        a.profile.onboarded = false;
    }
    s.account.queue(Outgoing::Profile(json!({"onboarded": false})));
    account_json(s)
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        spec!(
            "account.set",
            "Set the signed-in account",
            "{signedIn, email?, name?, inviteLink?, invited? (addresses), profile? {role, roleOther, experience, plan, createFirst, tips, onboarded}}  what the platform knows about who is signed in (the browser build reports its server's /auth/me); signedIn false signs out here",
            false,
            set
        ),
        spec!("account.get", "Account", "{}  who is signed in, the onboarding answers and whether onboarding is still to do", false, |s, _| Ok(
            account_json(s)
        )),
        spec!(
            "onboarding.answer",
            "Answer onboarding",
            "{name?, tips?, role? (design|research|ux-writing|student|data-analytics|software-development|marketing|product-management|educator|other), roleOther?, experience? (many|few|first), plan? (starter|professional|organization), createFirst? (app|website|graphic|animation|presentation|whiteboard)}  keeps the answers (sent to the account's server when signed in); the name is also the one comments and live sessions show",
            false,
            answer
        ),
        spec!(
            "onboarding.invite",
            "Invite people",
            "{emails: [address]}  people to invite to this server (at most 20 at once; signed in only); returns {queued, link}: the server keeps them, and no email is sent yet",
            false,
            invite
        ),
        spec!(
            "onboarding.finish",
            "Finish onboarding",
            "{createFirst? (app|website|graphic|animation|presentation|whiteboard; default the answer given)}  marks onboarding done and opens a new file for what was chosen: a board for whiteboard, slides for presentation, else a design file with a starting frame",
            false,
            finish
        ),
        spec!("onboarding.restart", "Show onboarding again", "{}  the onboarding questions show again (signed in)", false, |s, _| Ok(restart(s))),
        spec!("help.tour", "Take the tour", "{}  shows the editor tour (interface only; nothing happens from scripts)", false, |s, _| {
            s.request_ui("dialog.tour", json!({}));
            Ok(json!({}))
        }),
    ]
}
