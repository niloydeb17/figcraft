//! The account's recent files: the files on the page's server this account made or opened, newest
//! first, as Home ▸ Recently viewed lists them on every computer it signs in on.
//!
//! The browser build keeps each file it puts on its server, or opens there, here
//! (`account.recentOpened`) and queues it for the account's server (`/auth/recents`); after a
//! reload the platform hands the server's list back (`account.recents {files}`). A file's `target`
//! (its join code or link) lets whoever has it into the file, so it goes only to the account's own
//! server and is never shown.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::Outgoing;
use crate::cmd::CommandSpec;
use crate::live::SessionId;
use crate::params::{get, string};
use crate::{Result, Session, bad, spec};

/// Most files the list keeps (the server keeps as many).
pub const MAX_RECENTS: usize = 100;
/// The longest `target` (a join code or a file's link) and name taken.
pub const MAX_TARGET: usize = 4096;
pub const MAX_NAME: usize = 128;

/// A file in the account's recent list.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentFile {
    /// What opens it again: its join code or link (`collab.join` takes either).
    pub target: String,
    pub name: String,
    /// "design" or "board".
    pub kind: String,
    /// When it was last made, opened or renamed here (seconds since 1970).
    pub opened: u64,
    /// The tab showing it in this app now (its `DocState::uid`), if one does.
    #[serde(skip)]
    pub tab: Option<u64>,
    /// The join started for it from the list (`account.openRecent`), until its tab opens.
    #[serde(skip)]
    pub session: Option<SessionId>,
}

/// A target as the list keeps it: trimmed, without control characters, not too long.
fn clean_target(t: &str) -> Option<String> {
    let t = t.trim();
    (!t.is_empty() && t.chars().count() <= MAX_TARGET && !t.chars().any(char::is_control)).then(|| t.to_string())
}

fn clean_name(n: &str) -> String {
    let n: String = n.trim().chars().filter(|c| !c.is_control()).take(MAX_NAME).collect();
    if n.is_empty() { "Untitled".into() } else { n }
}

fn clean_kind(k: &str) -> String {
    if k == "board" { "board".into() } else { "design".into() }
}

impl super::AccountState {
    /// The recent file whose join `session` is, if one was opened from the list.
    pub fn recent_for_session(&self, session: SessionId) -> Option<&RecentFile> {
        self.recents.iter().find(|r| r.session == Some(session))
    }

    /// The recent file shown in tab `uid`, if one is.
    pub fn recent_for_tab(&self, uid: u64) -> Option<&RecentFile> {
        self.recents.iter().find(|r| r.tab == Some(uid))
    }

    /// Put `file` first (replacing the one with its target), keeping what this app knows about it.
    fn put_first(&mut self, mut file: RecentFile) {
        if let Some(i) = self.recents.iter().position(|r| r.target == file.target) {
            let old = self.recents.remove(i);
            file.tab = file.tab.or(old.tab);
            file.session = file.session.or(old.session);
        }
        // A tab shows one file.
        if let Some(tab) = file.tab {
            for r in &mut self.recents {
                if r.tab == Some(tab) {
                    r.tab = None;
                }
            }
        }
        self.recents.insert(0, file);
        self.recents.truncate(MAX_RECENTS);
    }
}

fn file_json(r: &RecentFile) -> Value {
    json!({"target": r.target, "name": r.name, "kind": r.kind, "opened": r.opened, "tab": r.tab})
}

fn list_json(s: &Session) -> Value {
    json!({"files": s.account.recents.iter().map(file_json).collect::<Vec<_>>()})
}

/// `account.recents`: the list, after taking the server's (`files`) when given.
fn recents(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "account.recents";
    let Some(given) = get(p, "files") else { return Ok(list_json(s)) };
    let Value::Array(given) = given else { return Err(bad(CMD, "`files` is a list of {target, name, kind, opened}")) };
    let mut next: Vec<RecentFile> = Vec::new();
    for v in given.iter().take(MAX_RECENTS * 2) {
        // The server's word, and anything else that hands a list: a bad entry is skipped.
        let Some(target) = v.get("target").and_then(Value::as_str).and_then(clean_target) else { continue };
        if next.iter().any(|r| r.target == target) {
            continue;
        }
        let name = clean_name(v.get("name").and_then(Value::as_str).unwrap_or_default());
        let kind = clean_kind(v.get("kind").and_then(Value::as_str).unwrap_or_default());
        let opened = v.get("opened").and_then(Value::as_u64).unwrap_or(0);
        next.push(RecentFile { target, name, kind, opened, tab: None, session: None });
    }
    // What this app knows of the same files (their tabs) stays; files kept here since the list was
    // asked for (on their way to the server) stay too.
    let local = std::mem::take(&mut s.account.recents);
    for l in &local {
        match next.iter_mut().find(|r| r.target == l.target) {
            Some(r) => {
                r.tab = l.tab;
                r.session = l.session;
                if l.opened > r.opened {
                    (r.name, r.kind, r.opened) = (l.name.clone(), l.kind.clone(), l.opened);
                }
            }
            None => {
                // Kept before the account was known (a file opened from the page's address while
                // it was asking): the server hears of it now.
                s.account.queue(Outgoing::RecentOpened(json!({"target": l.target, "name": l.name, "kind": l.kind})));
                next.push(l.clone());
            }
        }
    }
    next.sort_by_key(|r| std::cmp::Reverse(r.opened));
    next.truncate(MAX_RECENTS);
    s.account.recents = next;
    Ok(list_json(s))
}

/// `account.recentOpened`: a file on the server was made, opened or renamed here.
fn opened(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "account.recentOpened";
    let target = string(p, "target").and_then(|t| clean_target(&t)).ok_or_else(|| bad(CMD, "`target` is the file's join code or link"))?;
    let name = clean_name(&string(p, "name").unwrap_or_default());
    let kind = clean_kind(&string(p, "kind").unwrap_or_default());
    let tab = get(p, "tab").and_then(Value::as_u64);
    let file = RecentFile { target, name, kind, opened: crate::cmd::comment::now_secs(), tab, session: None };
    s.account.queue(Outgoing::RecentOpened(json!({"target": file.target, "name": file.name, "kind": file.kind})));
    s.account.put_first(file);
    Ok(list_json(s))
}

/// `account.recentForget`: take a file off the list (it stays on the server).
fn forget(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "account.recentForget";
    let target = string(p, "target").and_then(|t| clean_target(&t)).ok_or_else(|| bad(CMD, "`target` is the file's join code or link"))?;
    let before = s.account.recents.len();
    s.account.recents.retain(|r| r.target != target);
    if s.account.recents.len() != before {
        s.account.queue(Outgoing::RecentForget(target));
    }
    Ok(list_json(s))
}

/// `account.openRecent`: join a file from the list; the join's summary (`collab.join`).
fn open(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "account.openRecent";
    let target = string(p, "target").and_then(|t| clean_target(&t)).ok_or_else(|| bad(CMD, "`target` is the file's join code or link"))?;
    let Some(file) = s.account.recents.iter().find(|r| r.target == target).cloned() else { return Err(bad(CMD, "that file isn't in the list")) };
    let joined = s.execute("collab.join", &json!({"link": target}))?;
    let session = joined.get("session").and_then(Value::as_u64);
    s.account.put_first(RecentFile { opened: crate::cmd::comment::now_secs(), session, tab: None, ..file });
    Ok(joined)
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        spec!(
            "account.recents",
            "Recent files",
            "{files?: [{target, name, kind (design|board), opened (seconds since 1970)}]}  the account's recent files on its server, newest first (at most 100): with `files`, the list the account's server keeps (the browser build hands it over after /auth/recents) replaces this one, keeping the files this app knows are open; each entry's `target` (join code or link) lets whoever has it into the file",
            false,
            recents
        ),
        spec!(
            "account.recentOpened",
            "Keep a recent file",
            "{target (join code or link), name?, kind? (design|board), tab? (the open tab's uid)}  a file on the server was made, opened or renamed here: it goes first in the account's recent files (and to its server when signed in)",
            false,
            opened
        ),
        spec!(
            "account.recentForget",
            "Remove from recents",
            "{target}  takes a file off the account's recent files (and its server's list); the file itself stays on the server",
            false,
            forget
        ),
        spec!(
            "account.openRecent",
            "Open a recent file",
            "{target}  joins a file in the account's recent files (as collab.join {link: target}) and moves it first; returns the join's summary",
            false,
            open
        ),
    ]
}
