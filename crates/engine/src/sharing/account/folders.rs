//! The account's folders: named groups of the files on the page's server, as Home ▸ All folders
//! shows them on every computer the account signs in on. A file is in one folder at a time (adding
//! it to another moves it), and a folder can be starred to list it under Starred in the sidebar.
//!
//! Each change here (`folder.create`, `folder.rename`, `folder.delete`, `folder.star`,
//! `folder.addFile`, `folder.removeFile`) is queued for the account's server (`/auth/folders`); after
//! a reload the platform hands the server's list back (`account.folders {folders}`). Like a recent
//! file's, a file's `target` lets whoever has it into the file, so it goes only to the account's own
//! server and is never shown.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::Outgoing;
use super::recents::{MAX_NAME, MAX_TARGET};
use crate::cmd::CommandSpec;
use crate::params::{boolean, get, string};
use crate::{Result, Session, bad, spec};

/// Most folders an account keeps, and most files in one (the server keeps as many).
pub const MAX_FOLDERS: usize = 100;
pub const MAX_FOLDER_FILES: usize = 200;
/// The longest folder id taken (ids are made here: letters, digits, `-` and `_`).
pub const MAX_ID: usize = 32;

/// A file in a folder: what opens it, and its name and kind as last heard.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FolderFile {
    pub target: String,
    pub name: String,
    /// "design" or "board".
    pub kind: String,
}

/// One of the account's folders.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Folder {
    pub id: String,
    pub name: String,
    /// Its files, the one added last first.
    pub files: Vec<FolderFile>,
    pub starred: bool,
    /// When it was made, renamed or last had a file added (seconds since 1970).
    pub updated: u64,
    /// Who it is meant for: "team" (the workspace) or "private" (only you), and what they may do:
    /// "edit" or "view". Kept as chosen; folders aren't shared with other accounts yet.
    pub access: String,
    pub role: String,
}

fn clean_id(id: &str) -> Option<String> {
    let id = id.trim();
    (!id.is_empty() && id.len() <= MAX_ID && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')).then(|| id.to_string())
}

fn clean_name(n: &str) -> String {
    let n: String = n.trim().chars().filter(|c| !c.is_control()).take(MAX_NAME).collect();
    if n.is_empty() { "Untitled".into() } else { n }
}

fn clean_target(t: &str) -> Option<String> {
    let t = t.trim();
    (!t.is_empty() && t.chars().count() <= MAX_TARGET && !t.chars().any(char::is_control)).then(|| t.to_string())
}

fn clean_access(a: &str) -> String {
    if a == "private" { "private".into() } else { "team".into() }
}

fn clean_role(r: &str) -> String {
    if r == "view" { "view".into() } else { "edit".into() }
}

fn clean_kind(k: &str) -> String {
    if k == "board" { "board".into() } else { "design".into() }
}

/// A file entry from JSON (`{target, name?, kind?}`), or `None` for one without a usable target.
fn file_from(v: &Value) -> Option<FolderFile> {
    let target = v.get("target").and_then(Value::as_str).and_then(clean_target)?;
    let name = clean_name(v.get("name").and_then(Value::as_str).unwrap_or_default());
    let kind = clean_kind(v.get("kind").and_then(Value::as_str).unwrap_or_default());
    Some(FolderFile { target, name, kind })
}

fn folder_json(f: &Folder) -> Value {
    json!({"id": f.id, "name": f.name, "starred": f.starred, "updated": f.updated, "access": f.access, "role": f.role, "files": f.files})
}

fn list_json(s: &Session) -> Value {
    json!({"folders": s.account.folders.iter().map(folder_json).collect::<Vec<_>>()})
}

impl super::AccountState {
    /// The folder with `id`.
    pub fn folder(&self, id: &str) -> Option<&Folder> {
        self.folders.iter().find(|f| f.id == id)
    }

    /// The folder `target`'s file is in, if any.
    pub fn folder_of(&self, target: &str) -> Option<&Folder> {
        self.folders.iter().find(|f| f.files.iter().any(|x| x.target == target))
    }

    /// A new folder id: unlike every other one.
    fn new_folder_id(&self, now: u64) -> String {
        (0u64..).map(|n| format!("f{now:x}{n:x}")).find(|id| self.folder(id).is_none()).unwrap_or_else(|| format!("f{now:x}"))
    }

    /// The folder with `id`, to change (with the command's error when there is none).
    fn folder_mut(&mut self, cmd: &str, id: &str) -> Result<&mut Folder> {
        self.folders.iter_mut().find(|f| f.id == id).ok_or_else(|| bad(cmd, "no folder has that id"))
    }
}

/// The `id` parameter as an existing folder's id.
fn id_param(cmd: &str, p: &Value) -> Result<String> {
    string(p, "id").and_then(|id| clean_id(&id)).ok_or_else(|| bad(cmd, "`id` is a folder's id (account.folders lists them)"))
}

/// `account.folders`: the list, after taking the server's (`folders`) when given.
fn folders(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "account.folders";
    let Some(given) = get(p, "folders") else { return Ok(list_json(s)) };
    let Value::Array(given) = given else { return Err(bad(CMD, "`folders` is a list of {id, name, files, starred, updated}")) };
    let mut next: Vec<Folder> = Vec::new();
    for v in given.iter().take(MAX_FOLDERS * 2) {
        // The server's word: a bad entry is skipped, a file listed twice stays in the first folder.
        let Some(id) = v.get("id").and_then(Value::as_str).and_then(clean_id) else { continue };
        if next.iter().any(|f| f.id == id) || next.len() >= MAX_FOLDERS {
            continue;
        }
        let mut files: Vec<FolderFile> = Vec::new();
        for f in v.get("files").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default().iter().filter_map(file_from) {
            let taken = files.iter().any(|x| x.target == f.target) || next.iter().any(|n| n.files.iter().any(|x| x.target == f.target));
            if !taken && files.len() < MAX_FOLDER_FILES {
                files.push(f);
            }
        }
        next.push(Folder {
            id,
            name: clean_name(v.get("name").and_then(Value::as_str).unwrap_or_default()),
            files,
            starred: v.get("starred").and_then(Value::as_bool).unwrap_or(false),
            updated: v.get("updated").and_then(Value::as_u64).unwrap_or(0),
            access: clean_access(v.get("access").and_then(Value::as_str).unwrap_or_default()),
            role: clean_role(v.get("role").and_then(Value::as_str).unwrap_or_default()),
        });
    }
    s.account.folders = next;
    Ok(list_json(s))
}

/// `folder.create`: a new, empty folder; the folder.
fn create(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "folder.create";
    if s.account.folders.len() >= MAX_FOLDERS {
        return Err(bad(CMD, format!("an account keeps at most {MAX_FOLDERS} folders")));
    }
    let now = crate::cmd::comment::now_secs();
    let id = s.account.new_folder_id(now);
    let folder = Folder {
        id: id.clone(),
        name: clean_name(&string(p, "name").unwrap_or_default()),
        files: Vec::new(),
        starred: false,
        updated: now,
        access: clean_access(&string(p, "access").unwrap_or_default()),
        role: clean_role(&string(p, "role").unwrap_or_default()),
    };
    s.account.queue(Outgoing::Folder(json!({"create": {"id": id, "name": folder.name, "access": folder.access, "role": folder.role}})));
    let out = folder_json(&folder);
    s.account.folders.insert(0, folder);
    Ok(out)
}

/// `folder.rename`.
fn rename(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "folder.rename";
    let id = id_param(CMD, p)?;
    let name = clean_name(&string(p, "name").ok_or_else(|| bad(CMD, "`name` is the folder's new name"))?);
    let f = s.account.folder_mut(CMD, &id)?;
    if f.name != name {
        f.name = name.clone();
        f.updated = crate::cmd::comment::now_secs();
        s.account.queue(Outgoing::Folder(json!({"rename": {"id": id, "name": name}})));
    }
    Ok(list_json(s))
}

/// `folder.delete`: the folder goes; its files stay on the server and in the recent files.
fn delete(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "folder.delete";
    let id = id_param(CMD, p)?;
    s.account.folder_mut(CMD, &id)?;
    s.account.folders.retain(|f| f.id != id);
    s.account.queue(Outgoing::Folder(json!({"delete": id})));
    Ok(list_json(s))
}

/// `folder.star`: list the folder under Starred (or not).
fn star(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "folder.star";
    let id = id_param(CMD, p)?;
    let f = s.account.folder_mut(CMD, &id)?;
    let on = boolean(p, "on").unwrap_or(!f.starred);
    if f.starred != on {
        f.starred = on;
        s.account.queue(Outgoing::Folder(json!({"star": {"id": id, "on": on}})));
    }
    Ok(list_json(s))
}

/// `folder.addFile`: put a file on the server in the folder (out of any other one).
fn add_file(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "folder.addFile";
    let id = id_param(CMD, p)?;
    let file = file_from(p).ok_or_else(|| bad(CMD, "`target` is the file's join code or link"))?;
    if s.account.folder(&id).is_none() {
        return Err(bad(CMD, "no folder has that id"));
    }
    if s.account.folder(&id).is_some_and(|f| f.files.len() >= MAX_FOLDER_FILES && !f.files.iter().any(|x| x.target == file.target)) {
        return Err(bad(CMD, format!("a folder holds at most {MAX_FOLDER_FILES} files")));
    }
    for f in &mut s.account.folders {
        f.files.retain(|x| x.target != file.target);
    }
    let f = s.account.folder_mut(CMD, &id)?;
    f.files.insert(0, file.clone());
    f.updated = crate::cmd::comment::now_secs();
    s.account.queue(Outgoing::Folder(json!({"add": {"id": id, "file": file}})));
    Ok(list_json(s))
}

/// `folder.removeFile`: take a file out of its folder (it stays on the server).
fn remove_file(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "folder.removeFile";
    let target = string(p, "target").and_then(|t| clean_target(&t)).ok_or_else(|| bad(CMD, "`target` is the file's join code or link"))?;
    let mut changed = false;
    for f in &mut s.account.folders {
        let before = f.files.len();
        f.files.retain(|x| x.target != target);
        changed |= f.files.len() != before;
    }
    if changed {
        s.account.queue(Outgoing::Folder(json!({"remove": target})));
    }
    Ok(list_json(s))
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        spec!(
            "account.folders",
            "Folders",
            "{folders?: [{id, name, starred, updated, files: [{target, name, kind}]}]}  the account's folders (at most 100, each with at most 200 files on its server): with `folders`, the list the account's server keeps (the browser build hands it over after /auth/folders) replaces this one",
            false,
            folders
        ),
        spec!(
            "folder.create",
            "New folder",
            "{name?, access? (team|private, default team), role? (edit|view, default edit)}  a new, empty folder in the account's folders (sent to its server when signed in); returns the folder",
            false,
            create
        ),
        spec!("folder.rename", "Rename folder", "{id, name}", false, rename),
        spec!("folder.delete", "Delete folder", "{id}  the folder goes; its files stay on the server and in the recent files", false, delete),
        spec!(
            "folder.star",
            "Star folder",
            "{id, on? (default: the other way)}  starred folders are listed under Starred in Home's sidebar",
            false,
            star
        ),
        spec!(
            "folder.addFile",
            "Move file to folder",
            "{id, target (the file's join code or link), name?, kind? (design|board)}  puts a file on the server in the folder, out of any other one",
            false,
            add_file
        ),
        spec!("folder.removeFile", "Remove file from folder", "{target}  takes a file out of its folder; it stays on the server", false, remove_file),
    ]
}
