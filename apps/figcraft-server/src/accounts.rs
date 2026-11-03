//! Accounts: people sign in with an email and password (or Google) on pages the server serves
//! ([`http`]), and get a session cookie. Kept in the data directory's `accounts/` folder:
//! `users.json` (passwords as argon2id hashes) and `sessions.json` (only hashes of session tokens).
//! Design: `docs/accounts-design.md`.

pub mod google;
pub mod http;
pub mod pages;
pub mod password;

use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};

use figcraft_collab_net::token;
use serde::{Deserialize, Serialize};

use crate::disk;
use crate::store::{hash, now};

/// The folder in the data directory.
pub const DIR: &str = "accounts";
const USERS: &str = "users.json";
const SESSIONS: &str = "sessions.json";
const INVITES: &str = "invites.json";

/// Most accounts kept, sessions per account (the oldest goes first) and sessions in all.
pub const MAX_USERS: usize = 100_000;
pub const MAX_SESSIONS_PER_USER: usize = 50;
pub const MAX_SESSIONS: usize = 500_000;

/// Wrong passwords an account takes in [`FAILURE_WINDOW`] seconds before sign-in is paused for it.
pub const MAX_FAILURES: usize = 10;
pub const FAILURE_WINDOW: u64 = 15 * 60;

/// New accounts the whole server makes in an hour (a script can't fill the disk).
pub const SIGNUPS_PER_HOUR: usize = 120;

/// Shortest and longest password.
pub const MIN_PASSWORD: usize = 8;
pub const MAX_PASSWORD: usize = 256;

/// A session's last use is written down again only once it is this much newer.
const USED_GRANULARITY: u64 = 60 * 60;

pub const BAD_EMAIL: &str = "Enter a valid email address.";
pub const SHORT_PASSWORD: &str = "Use a password of at least 8 characters.";
pub const LONG_PASSWORD: &str = "That password is too long (256 characters at most).";
pub const WRONG_PASSWORD: &str = "That email and password don't match.";
pub const PAUSED: &str = "Too many wrong passwords for this account. Wait 15 minutes and try again.";
pub const GOOGLE_ONLY: &str = "This account signs in with Google.";
pub const DISABLED: &str = "This account is turned off. Ask the server's operator.";
pub const SIGNUP_CLOSED: &str = "Sign-up is closed on this server. Ask its operator for an account.";
pub const SIGNUP_BUSY: &str = "Too many new accounts right now. Try again in a few minutes.";
pub const FULL: &str = "This server has as many accounts as it keeps.";

/// Someone who can sign in.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    /// Lowercase; the login.
    pub email: String,
    pub name: String,
    /// The argon2id hash (PHC string); none for an account that only signs in with Google.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    /// The Google account (`sub`) it signs in with.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub google: Option<String>,
    /// The profile picture Google gave at the last sign-in (an address on its image hosts).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub picture: String,
    pub created: u64,
    #[serde(default)]
    pub last_login: u64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub disabled: bool,
    /// What the person said about themselves when they first signed in (the app's onboarding).
    #[serde(default)]
    pub profile: Profile,
}

/// The onboarding answers. Every value is one of the listed choices or empty (not answered).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Profile {
    /// [`ROLES`].
    pub role: String,
    /// What "other" means, typed (at most 60 characters).
    pub role_other: String,
    /// [`EXPERIENCE`].
    pub experience: String,
    /// [`PLANS`] (nothing is billed: the choice is kept for later).
    pub plan: String,
    /// [`CREATE_FIRST`].
    pub create_first: String,
    /// Wants tips and updates by email.
    pub tips: bool,
    /// Finished (or skipped) onboarding: the app doesn't show it again.
    pub onboarded: bool,
}

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

/// Most invites an account keeps, and per request.
pub const MAX_INVITES: usize = 200;
pub const INVITES_AT_ONCE: usize = 20;

/// A change to the profile: only the fields given change.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ProfileUpdate {
    pub name: Option<String>,
    pub role: Option<String>,
    pub role_other: Option<String>,
    pub experience: Option<String>,
    pub plan: Option<String>,
    pub create_first: Option<String>,
    pub tips: Option<bool>,
    pub onboarded: Option<bool>,
}

/// Someone an account asked to join.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Invite {
    pub by: String,
    pub email: String,
    pub at: u64,
}

fn choice(field: &str, v: &str, allowed: &[&str]) -> Result<String, String> {
    let v = v.trim();
    if v.is_empty() || allowed.contains(&v) { Ok(v.to_string()) } else { Err(format!("`{field}` is one of {} (or empty)", allowed.join(", "))) }
}

/// A signed-in browser: only the token's hash is kept.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Session {
    hash: String,
    user: String,
    created: u64,
    used: u64,
}

/// What signing in with a password finds.
#[derive(Clone, Debug, PartialEq)]
pub enum Lookup {
    /// No account with that email.
    Missing,
    /// Check the password against this hash; the account's id.
    Password {
        user: String,
        hash: String,
    },
    /// The account has no password (it signs in with Google).
    GoogleOnly,
    Disabled,
    /// Too many wrong passwords lately.
    Paused,
}

/// An account as the operator's `users list` shows it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UserRow {
    pub email: String,
    pub name: String,
    pub created: u64,
    pub last_login: u64,
    pub sessions: usize,
    pub password: bool,
    pub google: bool,
    pub disabled: bool,
}

/// The accounts and sessions, in memory and in `accounts/`.
pub struct Accounts {
    dir: PathBuf,
    users: Vec<User>,
    sessions: HashMap<String, Session>,
    /// Sign-up open to anyone.
    signup: bool,
    /// Seconds a session lasts without being used.
    idle: u64,
    /// Recent wrong passwords per account id.
    failures: HashMap<String, VecDeque<u64>>,
    /// When the last accounts were made.
    signups: VecDeque<u64>,
    invites: Vec<Invite>,
}

/// An email address as accounts keep it (trimmed, lowercase), or `None` when it isn't one.
pub fn clean_email(s: &str) -> Option<String> {
    let e = s.trim().to_lowercase();
    let (local, domain) = e.split_once('@')?;
    let ok = (3..=254).contains(&e.len())
        && !local.is_empty()
        && local.len() <= 64
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !domain.contains('@')
        && e.chars().all(|c| !c.is_whitespace() && !c.is_control() && !matches!(c, '<' | '>' | '"' | '\'' | '\\' | ',' | ';'));
    ok.then_some(e)
}

/// Why a new password can't be used, if it can't.
pub fn check_password(p: &str) -> Result<(), &'static str> {
    let n = p.chars().count();
    if n < MIN_PASSWORD {
        return Err(SHORT_PASSWORD);
    }
    if n > MAX_PASSWORD {
        return Err(LONG_PASSWORD);
    }
    Ok(())
}

/// A display name: the given one cleaned, else the email's local part.
fn display_name(name: &str, email: &str) -> String {
    let n: String = name.trim().chars().filter(|c| !c.is_control()).take(128).collect();
    if !n.is_empty() {
        return n;
    }
    email.split('@').next().unwrap_or_default().chars().take(128).collect()
}

impl Accounts {
    /// Read `accounts/` in the data directory `root` (made when missing).
    pub fn open(root: &Path, signup: bool, idle_days: f64) -> Result<Accounts, String> {
        let dir = root.join(DIR);
        let mut dirs = fs::DirBuilder::new();
        dirs.recursive(true);
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut dirs, 0o700);
        dirs.create(&dir).map_err(|e| format!("can't create {}: {e}", dir.display()))?;
        let users: Vec<User> = read_json(&dir.join(USERS))?.unwrap_or_default();
        let sessions: Vec<Session> = read_json(&dir.join(SESSIONS))?.unwrap_or_default();
        let invites: Vec<Invite> = read_json(&dir.join(INVITES))?.unwrap_or_default();
        let idle = if idle_days.is_finite() && idle_days > 0.0 { (idle_days * 86_400.0).min(3650.0 * 86_400.0) as u64 } else { 21 * 86_400 };
        let t = now();
        let sessions = sessions.into_iter().filter(|s| t.saturating_sub(s.used) < idle).map(|s| (s.hash.clone(), s)).collect();
        Ok(Accounts { dir, users, sessions, signup, idle, failures: HashMap::new(), signups: VecDeque::new(), invites })
    }

    /// Whether anyone may sign up.
    pub fn signup_open(&self) -> bool {
        self.signup
    }

    pub fn len(&self) -> usize {
        self.users.len()
    }

    pub fn is_empty(&self) -> bool {
        self.users.is_empty()
    }

    fn index(&self, email: &str) -> Option<usize> {
        let e = clean_email(email)?;
        self.users.iter().position(|u| u.email == e)
    }

    pub fn user(&self, id: &str) -> Option<&User> {
        self.users.iter().find(|u| u.id == id)
    }

    pub fn by_email(&self, email: &str) -> Option<&User> {
        self.index(email).and_then(|i| self.users.get(i))
    }

    /// What signing in as `email` with a password needs.
    pub fn lookup(&mut self, email: &str) -> Lookup {
        let Some(u) = self.by_email(email).cloned() else { return Lookup::Missing };
        if u.disabled {
            return Lookup::Disabled;
        }
        if self.paused(&u.id) {
            return Lookup::Paused;
        }
        match u.password {
            Some(hash) => Lookup::Password { user: u.id, hash },
            None => Lookup::GoogleOnly,
        }
    }

    fn paused(&mut self, user: &str) -> bool {
        let t = now();
        let Some(f) = self.failures.get_mut(user) else { return false };
        while f.front().is_some_and(|&at| t.saturating_sub(at) >= FAILURE_WINDOW) {
            f.pop_front();
        }
        f.len() >= MAX_FAILURES
    }

    /// A wrong password for `user`.
    pub fn failed(&mut self, user: &str) {
        if self.failures.len() > 10_000 {
            let t = now();
            self.failures.retain(|_, f| f.back().is_some_and(|&at| t.saturating_sub(at) < FAILURE_WINDOW));
        }
        let f = self.failures.entry(user.to_string()).or_default();
        f.push_back(now());
        while f.len() > MAX_FAILURES {
            f.pop_front();
        }
    }

    /// Make an account (sign-up). `password` is a hash already; `google` the Google account.
    pub fn create(&mut self, email: &str, name: &str, password: Option<String>, google: Option<String>) -> Result<User, String> {
        let email = clean_email(email).ok_or(BAD_EMAIL)?;
        if self.index(&email).is_some() {
            return Err("There is already an account with that email.".into());
        }
        if !self.signup {
            return Err(SIGNUP_CLOSED.into());
        }
        self.add(email, name, password, google)
    }

    /// Make an account whether or not sign-up is open (the operator's `users add`).
    pub fn add_by_operator(&mut self, email: &str, name: &str, password: Option<String>) -> Result<User, String> {
        let email = clean_email(email).ok_or(BAD_EMAIL)?;
        if self.index(&email).is_some() {
            return Err(format!("{email} already has an account"));
        }
        self.add(email, name, password, None)
    }

    fn add(&mut self, email: String, name: &str, password: Option<String>, google: Option<String>) -> Result<User, String> {
        if self.users.len() >= MAX_USERS {
            return Err(FULL.into());
        }
        let t = now();
        while self.signups.front().is_some_and(|&at| t.saturating_sub(at) >= 3600) {
            self.signups.pop_front();
        }
        if self.signups.len() >= SIGNUPS_PER_HOUR {
            return Err(SIGNUP_BUSY.into());
        }
        let user = User {
            id: token::file_id()?,
            name: display_name(name, &email),
            email,
            password,
            google,
            picture: String::new(),
            created: t,
            last_login: 0,
            disabled: false,
            profile: Profile::default(),
        };
        self.users.push(user.clone());
        if let Err(e) = self.save_users() {
            self.users.pop();
            return Err(e);
        }
        self.signups.push_back(t);
        Ok(user)
    }

    /// The account signing in with Google account `sub` and verified `email`: the one linked to
    /// it, else the one with that email (now linked; any password it had is dropped and its
    /// sessions end, since an email nobody verified may have been taken by someone else), else a
    /// new one when sign-up is open.
    pub fn google_user(&mut self, sub: &str, email: &str, name: &str, picture: &str) -> Result<User, String> {
        let user = self.google_account(sub, email, name)?;
        self.keep_picture(&user.id, picture)
    }

    fn google_account(&mut self, sub: &str, email: &str, name: &str) -> Result<User, String> {
        if let Some(u) = self.users.iter().find(|u| u.google.as_deref() == Some(sub)) {
            return if u.disabled { Err(DISABLED.into()) } else { Ok(u.clone()) };
        }
        let Some(i) = self.index(email) else { return self.create(email, name, None, Some(sub.to_string())) };
        let Some(u) = self.users.get_mut(i) else { return Err(BAD_EMAIL.into()) };
        if u.disabled {
            return Err(DISABLED.into());
        }
        u.google = Some(sub.to_string());
        u.password = None;
        let user = u.clone();
        self.sessions.retain(|_, s| s.user != user.id);
        self.save()?;
        Ok(user)
    }

    /// Remember `picture` as `user`'s (what Google says now); the account as it is then.
    fn keep_picture(&mut self, user: &str, picture: &str) -> Result<User, String> {
        let Some(u) = self.users.iter_mut().find(|u| u.id == user) else { return Err("no such account".into()) };
        if u.picture != picture {
            u.picture = picture.to_string();
            let user = u.clone();
            self.save_users()?;
            return Ok(user);
        }
        Ok(u.clone())
    }

    /// Sign `user` in: a new session token (only its hash is kept).
    pub fn start_session(&mut self, user: &str) -> Result<String, String> {
        let t = now();
        let Some(u) = self.users.iter_mut().find(|u| u.id == user) else { return Err("no such account".into()) };
        u.last_login = t;
        self.failures.remove(user);
        let token = format!("{}{}", token::secret()?, token::secret()?);
        let mine: Vec<(u64, String)> = self.sessions.values().filter(|s| s.user == user).map(|s| (s.used, s.hash.clone())).collect();
        if mine.len() >= MAX_SESSIONS_PER_USER {
            let mut mine = mine;
            mine.sort();
            for (_, h) in mine.iter().take(mine.len() + 1 - MAX_SESSIONS_PER_USER) {
                self.sessions.remove(h);
            }
        }
        if self.sessions.len() >= MAX_SESSIONS {
            self.prune();
            if self.sessions.len() >= MAX_SESSIONS {
                return Err("This server has too many sessions open. Try again later.".into());
            }
        }
        let h = hash(&token);
        self.sessions.insert(h.clone(), Session { hash: h, user: user.to_string(), created: t, used: t });
        self.save()?;
        Ok(token)
    }

    /// Whose session `token` is (it counts as used now); `None` when it ended or expired.
    pub fn session(&mut self, token: &str) -> Option<User> {
        if token.is_empty() || token.len() > 128 {
            return None;
        }
        let t = now();
        let h = hash(token);
        let s = self.sessions.get_mut(&h)?;
        if t.saturating_sub(s.used) >= self.idle {
            self.sessions.remove(&h);
            let _ = self.save_sessions();
            return None;
        }
        let stale = t.saturating_sub(s.used) >= USED_GRANULARITY;
        if stale {
            s.used = t;
        }
        let user = s.user.clone();
        if stale {
            let _ = self.save_sessions();
        }
        self.user(&user).filter(|u| !u.disabled).cloned()
    }

    /// Sign out the session `token`.
    pub fn end_session(&mut self, token: &str) -> Result<(), String> {
        if self.sessions.remove(&hash(token)).is_some() {
            self.save_sessions()?;
        }
        Ok(())
    }

    /// Drop expired sessions.
    pub fn prune(&mut self) {
        let t = now();
        let idle = self.idle;
        self.sessions.retain(|_, s| t.saturating_sub(s.used) < idle);
    }

    /// Set the password of `email` (a hash) and sign it out everywhere.
    pub fn set_password(&mut self, email: &str, hash: String) -> Result<(), String> {
        let i = self.index(email).ok_or_else(|| format!("no account for {email}"))?;
        let Some(u) = self.users.get_mut(i) else { return Err(format!("no account for {email}")) };
        u.password = Some(hash);
        let id = u.id.clone();
        self.sessions.retain(|_, s| s.user != id);
        self.failures.remove(&id);
        self.save()
    }

    /// Turn an account off (it is signed out) or on again.
    pub fn set_disabled(&mut self, email: &str, disabled: bool) -> Result<(), String> {
        let i = self.index(email).ok_or_else(|| format!("no account for {email}"))?;
        let Some(u) = self.users.get_mut(i) else { return Err(format!("no account for {email}")) };
        u.disabled = disabled;
        let id = u.id.clone();
        if disabled {
            self.sessions.retain(|_, s| s.user != id);
        }
        self.save()
    }

    /// Delete an account and its sessions.
    pub fn remove(&mut self, email: &str) -> Result<(), String> {
        let i = self.index(email).ok_or_else(|| format!("no account for {email}"))?;
        let u = self.users.remove(i);
        self.sessions.retain(|_, s| s.user != u.id);
        self.invites.retain(|x| x.by != u.id);
        write_json(&self.dir.join(INVITES), &self.invites)?;
        self.save()
    }

    /// Change `user`'s name and onboarding answers; the account as it is now.
    pub fn update_profile(&mut self, user: &str, u: &ProfileUpdate) -> Result<User, String> {
        let Some(i) = self.users.iter().position(|x| x.id == user) else { return Err("no such account".into()) };
        let mut next = self.users.get(i).cloned().ok_or("no such account")?;
        if let Some(n) = &u.name {
            let n: String = n.trim().chars().filter(|c| !c.is_control()).collect();
            if n.is_empty() || n.chars().count() > 128 {
                return Err("`name` is 1 to 128 characters".into());
            }
            next.name = n;
        }
        let p = &mut next.profile;
        if let Some(v) = &u.role {
            p.role = choice("role", v, ROLES)?;
        }
        if let Some(v) = &u.role_other {
            let v: String = v.trim().chars().filter(|c| !c.is_control()).collect();
            if v.chars().count() > 60 {
                return Err("`roleOther` is at most 60 characters".into());
            }
            p.role_other = v;
        }
        if let Some(v) = &u.experience {
            p.experience = choice("experience", v, EXPERIENCE)?;
        }
        if let Some(v) = &u.plan {
            p.plan = choice("plan", v, PLANS)?;
        }
        if let Some(v) = &u.create_first {
            p.create_first = choice("createFirst", v, CREATE_FIRST)?;
        }
        if let Some(v) = u.tips {
            p.tips = v;
        }
        if let Some(v) = u.onboarded {
            p.onboarded = v;
        }
        if let Some(slot) = self.users.get_mut(i) {
            *slot = next.clone();
        }
        self.save_users()?;
        Ok(next)
    }

    /// Keep `user`'s invites to `emails` (cleaned; ones already asked are skipped). The addresses
    /// kept, or why none could be.
    pub fn invite(&mut self, user: &str, emails: &[String]) -> Result<Vec<String>, String> {
        if emails.len() > INVITES_AT_ONCE {
            return Err(format!("at most {INVITES_AT_ONCE} invites at once"));
        }
        let me = self.user(user).map(|u| u.email.clone()).ok_or("no such account")?;
        let mut clean = Vec::new();
        for e in emails {
            if e.trim().is_empty() {
                continue;
            }
            let c = clean_email(e).ok_or_else(|| format!("{} isn't an email address", e.trim()))?;
            if c != me && !clean.contains(&c) {
                clean.push(c);
            }
        }
        clean.retain(|c| !self.invites.iter().any(|i| i.by == user && &i.email == c));
        if self.invites.iter().filter(|i| i.by == user).count() + clean.len() > MAX_INVITES {
            return Err(format!("an account keeps at most {MAX_INVITES} invites"));
        }
        let t = now();
        self.invites.extend(clean.iter().map(|e| Invite { by: user.to_string(), email: e.clone(), at: t }));
        write_json(&self.dir.join(INVITES), &self.invites)?;
        Ok(clean)
    }

    /// Who `user` has invited.
    pub fn invites_by(&self, user: &str) -> Vec<String> {
        self.invites.iter().filter(|i| i.by == user).map(|i| i.email.clone()).collect()
    }

    /// Every account, oldest first.
    pub fn list(&self) -> Vec<UserRow> {
        let mut counts: HashMap<&str, usize> = HashMap::new();
        for s in self.sessions.values() {
            *counts.entry(s.user.as_str()).or_default() += 1;
        }
        let mut rows: Vec<UserRow> = self
            .users
            .iter()
            .map(|u| UserRow {
                email: u.email.clone(),
                name: u.name.clone(),
                created: u.created,
                last_login: u.last_login,
                sessions: counts.get(u.id.as_str()).copied().unwrap_or(0),
                password: u.password.is_some(),
                google: u.google.is_some(),
                disabled: u.disabled,
            })
            .collect();
        rows.sort_by(|a, b| a.created.cmp(&b.created).then_with(|| a.email.cmp(&b.email)));
        rows
    }

    fn save(&mut self) -> Result<(), String> {
        self.save_users()?;
        self.save_sessions()
    }

    fn save_users(&self) -> Result<(), String> {
        write_json(&self.dir.join(USERS), &self.users)
    }

    fn save_sessions(&mut self) -> Result<(), String> {
        self.prune();
        let mut list: Vec<&Session> = self.sessions.values().collect();
        list.sort_by(|a, b| a.created.cmp(&b.created).then_with(|| a.hash.cmp(&b.hash)));
        write_json(&self.dir.join(SESSIONS), &list)
    }
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<Option<T>, String> {
    match disk::read_capped(path, 512 << 20) {
        Ok(bytes) => serde_json::from_slice(&bytes).map(Some).map_err(|e| format!("{}: {e}", path.display())),
        Err(_) if !path.exists() => Ok(None),
        Err(e) => Err(e),
    }
}

fn write_json<T: Serialize + ?Sized>(path: &Path, value: &T) -> Result<(), String> {
    let text = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    disk::write_atomic_private(path, &text).map_err(|e| format!("can't write {}: {e}", path.display()))
}
