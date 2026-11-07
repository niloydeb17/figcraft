//! Each account's folders: named groups of the files on this server, so Home ▸ All folders shows
//! them after a reload and on every computer the account signs in on. Kept one account to a file,
//! `accounts/folders/<id>.json` (private, written atomically), read when asked for.
//!
//! A browser sends one change at a time (`POST /auth/folders`): make, rename, star or delete a
//! folder, put a file in one (out of any other: a file is in one folder at a time), or take it
//! out. A file's `target` is its join code or link: it lets whoever has it into the file, so it is
//! only ever handed back to the account that kept it.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{read_json, write_json};
use crate::store::now;

/// The folder in `accounts/`.
pub const DIR: &str = "folders";
/// Most folders an account keeps, most files in one, the longest folder id, name and target.
pub const MAX: usize = 100;
pub const MAX_FILES: usize = 200;
pub const MAX_ID: usize = 32;
pub const MAX_NAME: usize = 128;
pub const MAX_TARGET: usize = super::recents::MAX_TARGET;

/// A file in a folder.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct File {
    pub target: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub kind: String,
}

/// One of an account's folders.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Folder {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub files: Vec<File>,
    #[serde(default)]
    pub starred: bool,
    /// When it was made, renamed or last had a file put in (seconds since 1970).
    #[serde(default)]
    pub updated: u64,
    /// Who it is meant for ("team" or "private") and what they may do ("edit" or "view"), as
    /// chosen when it was made.
    #[serde(default)]
    pub access: String,
    #[serde(default)]
    pub role: String,
}

/// What a browser asks of the folders (`POST /auth/folders`): one of these.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Change {
    #[serde(default)]
    pub create: Option<Named>,
    #[serde(default)]
    pub rename: Option<Named>,
    #[serde(default)]
    pub delete: Option<String>,
    #[serde(default)]
    pub star: Option<Star>,
    #[serde(default)]
    pub add: Option<Add>,
    /// Take the file with this target out of its folder.
    #[serde(default)]
    pub remove: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Named {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub access: String,
    #[serde(default)]
    pub role: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Star {
    pub id: String,
    pub on: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Add {
    pub id: String,
    pub file: File,
}

/// What `POST /auth/folders` takes, said when it gets something else.
pub const BAD_CHANGE: &str = "give one of `create` {id, name, access?, role?}, `rename` {id, name}, `delete` (an id), `star` {id, on}, `add` {id, file: {target, name, kind}} or `remove` (a target)";

fn clean_id(id: &str) -> Result<String, String> {
    let id = id.trim();
    let ok = !id.is_empty() && id.len() <= MAX_ID && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    if ok { Ok(id.to_string()) } else { Err(format!("a folder's `id` is 1 to {MAX_ID} letters, digits, - or _")) }
}

fn clean_name(n: &str) -> String {
    let n: String = n.trim().chars().filter(|c| !c.is_control()).take(MAX_NAME).collect();
    if n.is_empty() { "Untitled".into() } else { n }
}

fn clean_target(t: &str) -> Result<String, String> {
    let t = t.trim();
    if t.is_empty() || t.chars().count() > MAX_TARGET || t.chars().any(char::is_control) {
        return Err(format!("a file's `target` is its join code or link (1 to {MAX_TARGET} characters)"));
    }
    Ok(t.to_string())
}

/// The file holding account `user`'s folders; `None` for an id that can't name a file (ids are hex).
fn path(accounts_dir: &Path, user: &str) -> Option<PathBuf> {
    let ok = !user.is_empty() && user.len() <= 64 && user.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    ok.then(|| accounts_dir.join(DIR).join(format!("{user}.json")))
}

/// Account `user`'s folders, newest first (none when it has none; a damaged list reads as none and
/// is left on disk).
pub fn list(accounts_dir: &Path, user: &str) -> Vec<Folder> {
    let Some(p) = path(accounts_dir, user) else { return Vec::new() };
    match read_json::<Vec<Folder>>(&p) {
        Ok(list) => list.unwrap_or_default(),
        Err(e) => {
            log::warn!("accounts: an account's folders can't be read: {e}");
            Vec::new()
        }
    }
}

/// Apply `change` to account `user`'s folders and keep them; the folders after.
pub fn apply(accounts_dir: &Path, user: &str, change: Change) -> Result<Vec<Folder>, String> {
    let p = path(accounts_dir, user).ok_or("no such account")?;
    let asked = [
        change.create.is_some(),
        change.rename.is_some(),
        change.delete.is_some(),
        change.star.is_some(),
        change.add.is_some(),
        change.remove.is_some(),
    ];
    if asked.iter().filter(|a| **a).count() != 1 {
        return Err(BAD_CHANGE.into());
    }
    let mut list = list(accounts_dir, user);
    let before = list.clone();
    let find = |list: &mut Vec<Folder>, id: &str| -> Result<usize, String> {
        let id = clean_id(id)?;
        list.iter().position(|f| f.id == id).ok_or_else(|| "no folder has that id".to_string())
    };
    if let Some(c) = change.create {
        let id = clean_id(&c.id)?;
        if list.iter().any(|f| f.id == id) {
            return Err("a folder already has that id".into());
        }
        if list.len() >= MAX {
            return Err(format!("an account keeps at most {MAX} folders"));
        }
        let access = if c.access == "private" { "private" } else { "team" }.to_string();
        let role = if c.role == "view" { "view" } else { "edit" }.to_string();
        list.insert(0, Folder { id, name: clean_name(&c.name), files: Vec::new(), starred: false, updated: now(), access, role });
    }
    if let Some(r) = change.rename {
        let i = find(&mut list, &r.id)?;
        if let Some(f) = list.get_mut(i) {
            f.name = clean_name(&r.name);
            f.updated = now();
        }
    }
    if let Some(id) = change.delete {
        let i = find(&mut list, &id)?;
        list.remove(i);
    }
    if let Some(st) = change.star {
        let i = find(&mut list, &st.id)?;
        if let Some(f) = list.get_mut(i) {
            f.starred = st.on;
        }
    }
    if let Some(a) = change.add {
        let i = find(&mut list, &a.id)?;
        let target = clean_target(&a.file.target)?;
        let file =
            File { target: target.clone(), name: clean_name(&a.file.name), kind: if a.file.kind == "board" { "board" } else { "design" }.into() };
        let full = list.get(i).is_some_and(|f| f.files.len() >= MAX_FILES && !f.files.iter().any(|x| x.target == target));
        if full {
            return Err(format!("a folder holds at most {MAX_FILES} files"));
        }
        for f in &mut list {
            f.files.retain(|x| x.target != target);
        }
        if let Some(f) = list.get_mut(i) {
            f.files.insert(0, file);
            f.updated = now();
        }
    }
    if let Some(t) = change.remove {
        let t = clean_target(&t)?;
        for f in &mut list {
            f.files.retain(|x| x.target != t);
        }
    }
    if list != before {
        super::recents::make_private_dir(&accounts_dir.join(DIR))?;
        write_json(&p, &list)?;
    }
    Ok(list)
}
