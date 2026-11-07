//! Files shared with people by email, as the Share dialog's invites (as in the incumbent): the
//! owner's app makes each person a link of their own to the file (revoking it takes their access
//! away), then hands it here (`POST /auth/share`). The file goes first in that person's recent
//! files (Home ▸ Recently viewed), marked with who shared it. Someone without an account yet gets
//! it once they sign up with that email: it waits in `accounts/shared.json` (private, written
//! atomically) until their browser first asks for their recent files.
//!
//! Only a FigCraft join link can be handed on (`figcraft://join?…&file=…`), never a web address,
//! so an invite can't put a page from elsewhere on someone's Home.

use std::path::Path;

use serde::Deserialize;

use super::recents::{self, Shared};
use super::{read_json, write_json};

/// The file in `accounts/`.
pub const FILE: &str = "shared.json";
/// Most invites waiting for someone to sign up, in all and from one account.
pub const MAX_WAITING: usize = 5000;
pub const MAX_WAITING_BY: usize = 200;

/// What a browser hands on (`POST /auth/share`).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Share {
    pub emails: Vec<String>,
    /// The link of these people's own to the file.
    pub target: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub kind: String,
}

pub const BAD_SHARE: &str = "give `emails`, `target` (the people's link to the file), `name` and `kind`";

/// A file waiting for someone to sign up.
#[derive(Clone, Debug, PartialEq, serde::Serialize, Deserialize)]
pub struct Waiting {
    pub email: String,
    #[serde(flatten)]
    pub file: Shared,
}

/// `target` when it is a join link to a file, cleaned; why not otherwise.
pub fn join_link(target: &str) -> Result<String, String> {
    let t = recents::clean_target(target)?;
    if !t.starts_with("figcraft://join?") || recents::file_id(&t).is_none() || t.chars().any(char::is_whitespace) {
        return Err("only a FigCraft link to a file (figcraft://join?…&file=…) can be shared".into());
    }
    Ok(t)
}

/// The files waiting (empty when there are none; a damaged list reads as empty and is left on
/// disk).
pub fn waiting(accounts_dir: &Path) -> Vec<Waiting> {
    match read_json::<Vec<Waiting>>(&accounts_dir.join(FILE)) {
        Ok(list) => list.unwrap_or_default(),
        Err(e) => {
            log::warn!("accounts: the files waiting for people to sign up can't be read: {e}");
            Vec::new()
        }
    }
}

/// Keep `file` for `email` until they sign up (the same file from the same account for the same
/// person takes its older entry's place).
pub fn wait(accounts_dir: &Path, email: &str, file: &Shared) -> Result<(), String> {
    let mut list = waiting(accounts_dir);
    let same = recents::file_id(&file.target).map(str::to_string);
    list.retain(|w| !(w.email == email && w.file.by_id == file.by_id && recents::file_id(&w.file.target).map(str::to_string) == same));
    if list.iter().filter(|w| w.file.by_id == file.by_id).count() >= MAX_WAITING_BY {
        return Err(format!("an account has at most {MAX_WAITING_BY} invites waiting for people to sign up"));
    }
    if list.len() >= MAX_WAITING {
        return Err("this server has too many invites waiting for people to sign up: try again later".into());
    }
    list.push(Waiting { email: email.to_string(), file: file.clone() });
    write_json(&accounts_dir.join(FILE), &list)
}

/// Hand account `user` (email `email`) the files waiting for them: each goes on their recent
/// files, oldest first, and off the waiting list. How many there were.
pub fn claim(accounts_dir: &Path, user: &str, email: &str) -> usize {
    let list = waiting(accounts_dir);
    let (mine, rest): (Vec<Waiting>, Vec<Waiting>) = list.into_iter().partition(|w| w.email == email);
    if mine.is_empty() {
        return 0;
    }
    for w in &mine {
        if let Err(e) = recents::shared(accounts_dir, user, &w.file) {
            log::warn!("accounts: a file shared with a new account can't go on its list: {e}");
        }
    }
    if let Err(e) = write_json(&accounts_dir.join(FILE), &rest) {
        log::warn!("accounts: the files waiting for people to sign up can't be written: {e}");
    }
    mine.len()
}
