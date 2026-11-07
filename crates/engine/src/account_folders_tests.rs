//! The account's folders: made, renamed, starred and deleted; files moved in and out (one folder at
//! a time); each change queued for the account's server, and the server's list taken back.

use serde_json::{Value, json};

use crate::Session;
use crate::account::Outgoing;
use crate::account::folders::{MAX_FOLDER_FILES, MAX_FOLDERS};

fn signed_in() -> Session {
    let mut s = Session::new();
    s.execute("account.set", &json!({"signedIn": true, "email": "ada@example.com", "name": "Ada", "profile": {"onboarded": true}})).unwrap();
    s
}

fn ids(s: &Session) -> Vec<String> {
    s.account.folders.iter().map(|f| f.id.clone()).collect()
}

fn files(s: &Session, id: &str) -> Vec<String> {
    s.account.folder(id).map(|f| f.files.iter().map(|x| x.target.clone()).collect()).unwrap_or_default()
}

#[test]
fn folders_are_made_renamed_starred_and_deleted_and_each_change_goes_to_the_server() {
    let mut s = signed_in();
    let made = s.execute("folder.create", &json!({"name": "  Brand  "})).unwrap();
    let id = made["id"].as_str().unwrap().to_string();
    assert_eq!(made["name"], "Brand");
    let other = s.execute("folder.create", &json!({"access": "private", "role": "view"})).unwrap()["id"].as_str().unwrap().to_string();
    assert_eq!((s.account.folder(&other).unwrap().access.as_str(), s.account.folder(&other).unwrap().role.as_str()), ("private", "view"));
    assert_ne!(id, other, "ids are unique");
    assert_eq!(s.account.folder(&other).unwrap().name, "Untitled");
    assert_eq!(ids(&s), vec![other.clone(), id.clone()], "newest first");
    s.execute("folder.rename", &json!({"id": id, "name": "Brand kit"})).unwrap();
    s.execute("folder.star", &json!({"id": id})).unwrap();
    assert!(s.account.folder(&id).unwrap().starred);
    s.execute("folder.star", &json!({"id": id, "on": true})).unwrap();
    s.execute("folder.delete", &json!({"id": other})).unwrap();
    assert_eq!(ids(&s), vec![id.clone()]);
    let sent = s.account.take_outgoing();
    assert_eq!(
        sent,
        vec![
            Outgoing::Folder(json!({"create": {"id": id, "name": "Brand", "access": "team", "role": "edit"}})),
            Outgoing::Folder(json!({"create": {"id": other, "name": "Untitled", "access": "private", "role": "view"}})),
            Outgoing::Folder(json!({"rename": {"id": id, "name": "Brand kit"}})),
            Outgoing::Folder(json!({"star": {"id": id, "on": true}})),
            Outgoing::Folder(json!({"delete": other})),
        ],
        "starring a starred folder again sends nothing"
    );
    for (cmd, p) in
        [("folder.rename", json!({"id": "nope", "name": "x"})), ("folder.delete", json!({"id": "nope"})), ("folder.star", json!({"id": "bad id!"}))]
    {
        assert!(s.execute(cmd, &p).is_err(), "{cmd} {p}");
    }
}

#[test]
fn a_file_is_in_one_folder_at_a_time() {
    let mut s = signed_in();
    let a = s.execute("folder.create", &json!({"name": "A"})).unwrap()["id"].as_str().unwrap().to_string();
    let b = s.execute("folder.create", &json!({"name": "B"})).unwrap()["id"].as_str().unwrap().to_string();
    s.execute("folder.addFile", &json!({"id": a, "target": "K7M-2QX-9TD", "name": "Landing", "kind": "board"})).unwrap();
    s.execute("folder.addFile", &json!({"id": a, "target": "AAA-BBB-CCC"})).unwrap();
    assert_eq!(files(&s, &a), vec!["AAA-BBB-CCC", "K7M-2QX-9TD"], "the one added last first");
    assert_eq!(s.account.folder(&a).unwrap().files[1].kind, "board");
    // Into B: out of A.
    s.execute("folder.addFile", &json!({"id": b, "target": "K7M-2QX-9TD", "name": "Landing"})).unwrap();
    assert_eq!(files(&s, &a), vec!["AAA-BBB-CCC"]);
    assert_eq!(s.account.folder_of("K7M-2QX-9TD").map(|f| f.id.clone()), Some(b.clone()));
    s.execute("folder.removeFile", &json!({"target": "K7M-2QX-9TD"})).unwrap();
    assert!(s.account.folder_of("K7M-2QX-9TD").is_none());
    s.account.take_outgoing();
    s.execute("folder.removeFile", &json!({"target": "K7M-2QX-9TD"})).unwrap();
    assert!(s.account.take_outgoing().is_empty(), "nothing to take out: nothing sent");
    assert!(s.execute("folder.addFile", &json!({"id": a})).is_err(), "no target");
    assert!(s.execute("folder.addFile", &json!({"id": "nope", "target": "X"})).is_err(), "no such folder");
}

#[test]
fn the_servers_list_replaces_this_one_and_bad_entries_are_skipped() {
    let mut s = signed_in();
    s.execute("folder.create", &json!({"name": "local"})).unwrap();
    let list = s
        .execute(
            "account.folders",
            &json!({"folders": [
                {"id": "f1", "name": "One", "starred": true, "updated": 5, "files": [{"target": "T1", "name": "File"}, {"target": "T1"}, {"name": "no target"}]},
                {"id": "f1", "name": "Again"},
                {"id": "bad id", "name": "Bad"},
                {"name": "No id"},
                {"id": "f2", "name": "", "files": [{"target": "T1"}, {"target": "T2"}]},
                "junk",
            ]}),
        )
        .unwrap();
    assert_eq!(ids(&s), vec!["f1", "f2"]);
    assert_eq!(files(&s, "f1"), vec!["T1"]);
    assert_eq!(files(&s, "f2"), vec!["T2"], "a file in two folders stays in the first");
    assert_eq!(s.account.folder("f2").unwrap().name, "Untitled");
    assert!(s.account.folder("f1").unwrap().starred);
    assert_eq!(list["folders"].as_array().map(Vec::len), Some(2));
    assert!(s.execute("account.folders", &json!({"folders": "nope"})).is_err());
    assert_eq!(s.execute("account.folders", &json!({})).unwrap(), list, "without folders: the list");
    // Signing out forgets them.
    s.execute("account.set", &json!({"signedIn": false})).unwrap();
    assert!(s.account.folders.is_empty());
}

#[test]
fn without_an_account_folders_stay_here_and_the_limits_hold() {
    let mut s = Session::new();
    for i in 0..MAX_FOLDERS {
        s.execute("folder.create", &json!({"name": format!("F{i}")})).unwrap();
    }
    assert!(s.execute("folder.create", &json!({})).is_err(), "at most {MAX_FOLDERS}");
    assert!(s.account.take_outgoing().is_empty(), "no account: nothing to send");
    let id = s.account.folders[0].id.clone();
    for i in 0..MAX_FOLDER_FILES {
        s.execute("folder.addFile", &json!({"id": id, "target": format!("T{i}")})).unwrap();
    }
    assert!(s.execute("folder.addFile", &json!({"id": id, "target": "one more"})).is_err());
    // One already in it can be added again (it goes first).
    s.execute("folder.addFile", &json!({"id": id, "target": "T0"})).unwrap();
    assert_eq!(files(&s, &id).first().map(String::as_str), Some("T0"));
    let long = "x".repeat(10_000);
    let out: Value = s.execute("folder.rename", &json!({"id": id, "name": long})).unwrap();
    assert!(out["folders"][0]["name"].as_str().unwrap().chars().count() <= 128);
}
