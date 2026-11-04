//! Each account's recent files: the files on this server the account made or opened in the
//! browser, newest first, so Home ▸ Recently viewed lists them after a reload and on every
//! computer the account signs in on. Kept one account to a file, `accounts/recents/<id>.json`
//! (private, written atomically), read when asked for.
//!
//! An entry's `target` is the file's join code or link: it lets whoever has it into the file, so
//! it is only ever handed back to the account that kept it.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{read_json, write_json};
use crate::store::now;

/// The folder in `accounts/`.
pub const DIR: &str = "recents";
/// Most files an account's list keeps (the oldest go first), and the longest target and name.
pub const MAX: usize = 100;
pub const MAX_TARGET: usize = 4096;
pub const MAX_NAME: usize = 128;

/// A file in an account's list.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Recent {
    pub target: String,
    pub name: String,
    /// "design" or "board".
    pub kind: String,
    /// When it was last made, opened or renamed (seconds since 1970).
    pub opened: u64,
}

/// What a browser asks of the list (`POST /auth/recents`).
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Change {
    /// Put this file first: {target, name, kind}.
    #[serde(default)]
    pub open: Option<Opened>,
    /// Take the file with this target off the list.
    #[serde(default)]
    pub forget: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Opened {
    pub target: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub kind: String,
}

fn clean_target(t: &str) -> Result<String, String> {
    let t = t.trim();
    if t.is_empty() || t.chars().count() > MAX_TARGET || t.chars().any(char::is_control) {
        return Err(format!("a file's `target` is its join code or link (1 to {MAX_TARGET} characters)"));
    }
    Ok(t.to_string())
}

/// The file holding account `user`'s list; `None` for an id that can't name a file (ids are hex).
fn path(accounts_dir: &Path, user: &str) -> Option<PathBuf> {
    let ok = !user.is_empty() && user.len() <= 64 && user.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    ok.then(|| accounts_dir.join(DIR).join(format!("{user}.json")))
}

/// Account `user`'s list, newest first (empty when it has none; a damaged one reads as empty and
/// is left on disk).
pub fn list(accounts_dir: &Path, user: &str) -> Vec<Recent> {
    let Some(p) = path(accounts_dir, user) else { return Vec::new() };
    match read_json::<Vec<Recent>>(&p) {
        Ok(list) => list.unwrap_or_default(),
        Err(e) => {
            log::warn!("accounts: an account's recent files can't be read: {e}");
            Vec::new()
        }
    }
}

/// Apply `change` to account `user`'s list and keep it; the list after.
pub fn apply(accounts_dir: &Path, user: &str, change: Change) -> Result<Vec<Recent>, String> {
    let p = path(accounts_dir, user).ok_or("no such account")?;
    if change.open.is_none() && change.forget.is_none() {
        return Err("give `open` {target, name, kind} or `forget` (a target)".into());
    }
    let mut list = list(accounts_dir, user);
    let before = list.clone();
    if let Some(t) = change.forget {
        let t = clean_target(&t)?;
        list.retain(|r| r.target != t);
    }
    if let Some(o) = change.open {
        let target = clean_target(&o.target)?;
        let name: String = o.name.trim().chars().filter(|c| !c.is_control()).take(MAX_NAME).collect();
        let name = if name.is_empty() { "Untitled".to_string() } else { name };
        let kind = if o.kind == "board" { "board" } else { "design" }.to_string();
        list.retain(|r| r.target != target);
        list.insert(0, Recent { target, name, kind, opened: now() });
        list.truncate(MAX);
    }
    if list != before {
        let dir = accounts_dir.join(DIR);
        let mut dirs = std::fs::DirBuilder::new();
        dirs.recursive(true);
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut dirs, 0o700);
        dirs.create(&dir).map_err(|e| format!("can't create {}: {e}", dir.display()))?;
        write_json(&p, &list)?;
    }
    Ok(list)
}
