//! Each account's recent files: the files on this server the account made or opened in the
//! browser, newest first, so Home ▸ Recently viewed lists them after a reload and on every
//! computer the account signs in on. Kept one account to a file, `accounts/recents/<id>.json`
//! (private, written atomically), read when asked for.
//!
//! An entry's `target` is the file's join code or link: it lets whoever has it into the file, so
//! it is only ever handed back to the account that kept it.
//!
//! Each file may have a picture of itself (a PNG the browser drew of the open file, as Home's card
//! shows it), kept beside the list in `accounts/recents/<id>/<key>.png` and named in the list by a
//! token (`<key>-<version>`, where `key` is a hash of the target, never the target itself), so a
//! reload or another computer shows the card's picture without opening the file.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use sha2::{Digest, Sha256};

use super::{read_json, write_json};
use crate::store::now;

/// The folder in `accounts/`.
pub const DIR: &str = "recents";
/// Most files an account's list keeps (the oldest go first), and the longest target and name.
pub const MAX: usize = 100;
pub const MAX_TARGET: usize = 4096;
pub const MAX_NAME: usize = 128;
/// The largest picture kept (PNG bytes), and its widest or tallest side (pixels).
pub const MAX_PICTURE: usize = 384 << 10;
pub const MAX_PICTURE_SIDE: u32 = 2048;

/// A file in an account's list.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Recent {
    pub target: String,
    pub name: String,
    /// "design" or "board".
    pub kind: String,
    /// When it was last made, opened or renamed (seconds since 1970).
    pub opened: u64,
    /// Its picture's token (`GET /auth/recents/picture?p=<token>`), once a browser sent one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub picture: Option<String>,
    /// Someone else shared it with this account (the Share dialog's invite): their name, as Home
    /// shows it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by: Option<String>,
    /// The account that shared it (never sent to a browser): sharing the same file again with
    /// another link takes this entry's place.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "byId")]
    pub by_id: Option<String>,
}

/// A recent file as its account's browser gets it: without who shared it by account id.
pub fn for_browser(mut list: Vec<Recent>) -> Vec<Recent> {
    for r in &mut list {
        r.by_id = None;
    }
    list
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

pub(super) fn clean_target(t: &str) -> Result<String, String> {
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

/// The folder holding account `user`'s pictures.
fn pictures(accounts_dir: &Path, user: &str) -> Option<PathBuf> {
    path(accounts_dir, user).map(|p| p.with_extension(""))
}

/// The name a target's picture is kept by: a hash of it, so the target never shows in a file name
/// or an address.
fn picture_key(target: &str) -> String {
    Sha256::digest(target.as_bytes()).iter().take(16).map(|b| format!("{b:02x}")).collect()
}

/// A folder only the server's user can read (the files in it name nobody's files, but are theirs).
pub(super) fn make_private_dir(dir: &Path) -> Result<(), String> {
    let mut dirs = std::fs::DirBuilder::new();
    dirs.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut dirs, 0o700);
    dirs.create(dir).map_err(|e| format!("can't create {}: {e}", dir.display()))
}

/// The width and height a PNG says it has; `None` for anything that isn't a PNG.
fn png_size(bytes: &[u8]) -> Option<(u32, u32)> {
    const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    if bytes.get(..8)? != SIGNATURE || bytes.get(12..16)? != b"IHDR" {
        return None;
    }
    let be = |at: usize| bytes.get(at..at + 4).and_then(|b| <[u8; 4]>::try_from(b).ok()).map(u32::from_be_bytes);
    Some((be(16)?, be(20)?))
}

/// Forget the pictures of the files `before` had and `after` hasn't.
fn drop_pictures(accounts_dir: &Path, user: &str, before: &[Recent], after: &[Recent]) {
    let Some(dir) = pictures(accounts_dir, user) else { return };
    for gone in before.iter().filter(|b| b.picture.is_some() && !after.iter().any(|a| a.target == b.target)) {
        let file = dir.join(format!("{}.png", picture_key(&gone.target)));
        if let Err(e) = std::fs::remove_file(&file)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            log::warn!("accounts: a recent file's picture can't be removed: {e}");
        }
    }
}

/// Put a file someone shared first in account `user`'s list: `target` (the link of the user's own
/// to it), named `name`, shared by account `by_id` called `by`. An entry the same account shared
/// for the same file (an older link of theirs to it) goes. The list after.
pub fn shared(accounts_dir: &Path, user: &str, file: &Shared) -> Result<Vec<Recent>, String> {
    let p = path(accounts_dir, user).ok_or("no such account")?;
    let target = clean_target(&file.target)?;
    let mut list = list(accounts_dir, user);
    let before = list.clone();
    let same = file_id(&target);
    list.retain(|r| r.target != target && !(r.by_id.as_deref() == Some(file.by_id.as_str()) && same.is_some() && file_id(&r.target) == same));
    list.insert(
        0,
        Recent {
            target,
            name: clean_name(&file.name),
            kind: if file.kind == "board" { "board" } else { "design" }.to_string(),
            opened: file.at,
            picture: None,
            by: Some(clean_name(&file.by)).filter(|b| !b.is_empty() && b != "Untitled"),
            by_id: Some(file.by_id.clone()),
        },
    );
    list.truncate(MAX);
    make_private_dir(&accounts_dir.join(DIR))?;
    write_json(&p, &list)?;
    drop_pictures(accounts_dir, user, &before, &list);
    Ok(list)
}

/// A file shared with an account, as [`shared`] keeps it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Shared {
    pub target: String,
    pub name: String,
    pub kind: String,
    /// Who shared it: their name, and their account id.
    pub by: String,
    #[serde(rename = "byId")]
    pub by_id: String,
    /// When (seconds since 1970).
    pub at: u64,
}

/// A file's name as a list keeps it: no control characters, at most [`MAX_NAME`], "Untitled" when
/// empty.
fn clean_name(name: &str) -> String {
    let name: String = name.trim().chars().filter(|c| !c.is_control()).take(MAX_NAME).collect();
    if name.is_empty() { "Untitled".to_string() } else { name }
}

/// The file a join link names (its `file=` part), if it names one.
pub(super) fn file_id(target: &str) -> Option<&str> {
    let (_, query) = target.split_once('?')?;
    query.split('&').find_map(|kv| kv.strip_prefix("file=")).filter(|f| !f.is_empty())
}

/// Keep `png` as the picture of the file with `target` in account `user`'s list; the list after.
/// Only a listed file keeps a picture, and only a PNG within [`MAX_PICTURE`] and
/// [`MAX_PICTURE_SIDE`].
pub fn set_picture(accounts_dir: &Path, user: &str, target: &str, png: &[u8]) -> Result<Vec<Recent>, String> {
    let (p, dir) = path(accounts_dir, user).zip(pictures(accounts_dir, user)).ok_or("no such account")?;
    let target = clean_target(target)?;
    if png.len() > MAX_PICTURE {
        return Err(format!("a picture is at most {} KB", MAX_PICTURE >> 10));
    }
    match png_size(png) {
        Some((w, h)) if (1..=MAX_PICTURE_SIDE).contains(&w) && (1..=MAX_PICTURE_SIDE).contains(&h) => {}
        _ => return Err(format!("a picture is a PNG at most {MAX_PICTURE_SIDE} pixels a side")),
    }
    let mut list = list(accounts_dir, user);
    let entry = list.iter_mut().find(|r| r.target == target).ok_or("that file isn't in the list")?;
    let key = picture_key(&target);
    make_private_dir(&dir)?;
    crate::disk::write_atomic_private(&dir.join(format!("{key}.png")), png).map_err(|e| format!("can't keep the picture: {e}"))?;
    // A new token for new bytes, so a browser never shows an old picture it kept.
    entry.picture = Some(format!("{key}-{:08x}", crc32fast::hash(png)));
    write_json(&p, &list)?;
    Ok(list)
}

/// The picture named by `token` in account `user`'s list (`None` for one not in it).
pub fn picture(accounts_dir: &Path, user: &str, token: &str) -> Option<Vec<u8>> {
    let list = list(accounts_dir, user);
    let entry = list.iter().find(|r| r.picture.as_deref() == Some(token))?;
    let file = pictures(accounts_dir, user)?.join(format!("{}.png", picture_key(&entry.target)));
    crate::disk::read_capped(&file, MAX_PICTURE as u64).ok()
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
        // Its picture, and who shared it, stay until a new one comes.
        let was = list.iter().find(|r| r.target == target);
        let (picture, by, by_id) = was.map(|r| (r.picture.clone(), r.by.clone(), r.by_id.clone())).unwrap_or_default();
        list.retain(|r| r.target != target);
        list.insert(0, Recent { target, name, kind, opened: now(), picture, by, by_id });
        list.truncate(MAX);
    }
    if list != before {
        make_private_dir(&accounts_dir.join(DIR))?;
        write_json(&p, &list)?;
        drop_pictures(accounts_dir, user, &before, &list);
    }
    Ok(list)
}
