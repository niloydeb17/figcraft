//! The Share dialog in the window, as the incumbent's: its two cards near the top; Copy link (a file
//! not on the server goes there first) and Copy Dev Mode link; what anyone with the link may do
//! (lowering it makes new links, no access revokes them); people invited by email, each with a link
//! of their own, changed or removed on their own, and handed to the account's server when signed
//! in; More (the local network and its code, removing the file from the server); joining from the
//! home screen and the Join dialog; the server preference; and an idle dialog asking for no frames.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use egui::{Event, Key, Shape};
use figcraft_engine::figcraft_collab::code::{self, Local};
use figcraft_engine::figcraft_collab::message::{Admin, AdminReply, ClientMsg, LinkInfo, NewLink};
use figcraft_engine::live::{
    Access, DEFAULT_SERVER, Event as LiveEvent, HostOptions, LiveLink, NetReply, NetRequest, Presence, PublishOptions, Published, Role, SessionInfo,
    Shared as SharedLinks, SharedLink, Status,
};
use serde_json::json;

use crate::live_tests::{Harness, has_text, text_center, texts};

/// What the link was asked to do, and the events it hands out next (as the session started last).
#[derive(Default)]
struct Script {
    hosted: Vec<HostOptions>,
    published: Vec<PublishOptions>,
    joined: Vec<String>,
    left: usize,
    events: Vec<LiveEvent>,
    session: u64,
    copied: Vec<String>,
    /// Link ids the server was asked to revoke, how often it was asked for new links, the files
    /// it was asked to remove, and its answers on their way.
    revoked: Vec<String>,
    shares: usize,
    removed: Vec<String>,
    answers: Vec<LiveEvent>,
    /// The file's links made by owner requests (invites), and how many were made.
    links: Vec<LinkInfo>,
    made: usize,
}

/// The links a server makes for others the `n`th time a file is shared, with their codes.
fn links(n: usize, codes: Option<(&str, &str)>) -> SharedLinks {
    let link = |what: &str, code: Option<&str>| SharedLink {
        id: format!("{what}{n}"),
        link: format!("figcraft://join?server=wss://share.example:443/&file=f1&token=Share{what}Share{what}0{n}"),
        code: code.map(str::to_string),
    };
    SharedLinks { edit: link("edit", codes.map(|c| c.0)), view: Some(link("view", codes.map(|c| c.1))), comment: None }
}

type Shared = Arc<Mutex<Script>>;

struct Link(Shared);

/// Where the scripted host listens, and its session's secret.
const HOST: Local = Local { ip: std::net::Ipv4Addr::new(192, 168, 1, 20), port: 47_900, secret: 0x123_4567_89AB };
/// Where the scripted server serves its browser build.
const WEB: &str = "https://figcraft.example/";

/// A link without a code in it (an address a code can't carry).
const PLAIN: &str = "figcraft://join?host=[fd00::1]:47900&file=f&token=AbCdEfGhIjKlMnOpQrStUv";

fn info(role: Role, link: &str) -> SessionInfo {
    SessionInfo {
        role,
        me: 1,
        name: "Ada".into(),
        color: "#14AE5C".into(),
        file_name: "Untitled".into(),
        pages: vec![],
        link: link.into(),
        url: String::new(),
        address: "192.168.1.20:47900".into(),
        network: true,
        access: Access::Edit,
    }
}

impl LiveLink for Link {
    fn host(&mut self, session: u64, opts: HostOptions) -> Result<SessionInfo, String> {
        let mut s = self.0.lock().unwrap();
        s.session = session;
        let link = if opts.port == Some(6) { PLAIN.to_string() } else { HOST.link() };
        s.hosted.push(opts);
        Ok(info(Role::Host, &link))
    }
    fn join(&mut self, session: u64, link: &str, _: &str) -> Result<(), String> {
        let mut s = self.0.lock().unwrap();
        s.session = session;
        s.joined.push(link.to_string());
        Ok(())
    }
    fn publish(&mut self, session: u64, opts: PublishOptions) -> Result<(), String> {
        let mut s = self.0.lock().unwrap();
        s.session = session;
        s.published.push(opts);
        Ok(())
    }
    fn share(&mut self, _: &str, _: &str, _: &str, revoke: &[String], create: bool) -> Result<Option<SharedLinks>, String> {
        let mut s = self.0.lock().unwrap();
        s.revoked.extend(revoke.iter().cloned());
        s.shares += usize::from(create);
        let n = s.shares;
        Ok(create.then(|| links(n, Some((&format!("NEWED1T0{n}"), &format!("NEWV1EW0{n}"))))))
    }
    fn hosting(
        &mut self,
        _: &str,
        file: &str,
        _: &str,
        req: Option<figcraft_engine::figcraft_collab::message::Admin>,
    ) -> Result<(figcraft_engine::live::HostInfo, Option<AdminReply>), String> {
        let mut s = self.0.lock().unwrap();
        let reply = match req {
            Some(Admin::Remove { .. }) => {
                s.removed.push(file.to_string());
                AdminReply::Done
            }
            Some(Admin::Links { .. }) => AdminReply::Links(s.links.clone()),
            Some(Admin::Web) => AdminReply::Web { web: WEB.into() },
            _ => AdminReply::Done,
        };
        Ok((Default::default(), Some(reply)))
    }
    fn admin(&mut self, _: &str, file: &str, _: &str, reqs: Vec<Admin>) -> Result<Vec<AdminReply>, String> {
        let mut s = self.0.lock().unwrap();
        Ok(reqs
            .into_iter()
            .map(|r| match r {
                Admin::CreateLink { access, label, .. } => {
                    s.made += 1;
                    let n = s.made;
                    let link = LinkInfo { id: format!("p{n}"), file: file.to_string(), access, label, created: n as u64 };
                    s.links.push(link.clone());
                    AdminReply::Link(NewLink {
                        link,
                        token: format!("figcraft://join?server=wss://share.example:443/&file={file}&token=Person{n:0>16}"),
                    })
                }
                Admin::Revoke { link, .. } => {
                    s.links.retain(|l| l.id != link);
                    AdminReply::Done
                }
                _ => AdminReply::Done,
            })
            .collect())
    }
    fn background(&mut self, job: u64, req: NetRequest) -> bool {
        // Answered at the next poll, as the network would.
        let reply: NetReply = self.request(req);
        self.0.lock().unwrap().answers.push(LiveEvent::Net { job, reply: Box::new(reply) });
        true
    }
    fn leave(&mut self, _: u64) {
        self.0.lock().unwrap().left += 1;
    }
    fn presence(&mut self, _: u64, _: &Presence) {}
    fn send(&mut self, _: u64, _: ClientMsg) {}
    fn poll(&mut self) -> Vec<(u64, LiveEvent)> {
        let mut s = self.0.lock().unwrap();
        let session = s.session;
        let answers: Vec<(u64, LiveEvent)> = std::mem::take(&mut s.answers).into_iter().map(|e| (0, e)).collect();
        answers.into_iter().chain(std::mem::take(&mut s.events).into_iter().map(|e| (session, e))).collect()
    }
}

/// The clipboard, kept where the test can read it.
struct Clipboard(Shared);

impl crate::Services for Clipboard {
    fn clipboard_set_text(&mut self, s: &str) {
        self.0.lock().unwrap().copied.push(s.to_string());
    }
}

/// A window on a file, with the scripted link and a server of the test's own in Preferences.
fn harness() -> (Harness, Shared) {
    let (mut h, _) = Harness::new();
    let script = Shared::default();
    h.app.session.live.install(Box::new(Link(script.clone())));
    h.app.services = Box::new(Clipboard(script.clone()));
    h.app.session.execute("pref.publishServer", &json!({"url": "wss://share.example/"})).unwrap();
    h.frames(3);
    (h, script)
}

/// Open the Share dialog.
fn open(h: &mut Harness) -> Vec<Shape> {
    let shapes = h.frames(1);
    h.click(text_center(&shapes, "Share"));
    h.frames(2)
}

fn click(h: &mut Harness, shapes: &[Shape], text: &str) -> Vec<Shape> {
    h.click(text_center(shapes, text));
    h.frames(2)
}

fn key(h: &mut Harness, key: Key) {
    for pressed in [true, false] {
        h.frame(vec![Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Default::default() }]);
    }
}

/// Type into the field with the keyboard.
fn type_text(h: &mut Harness, text: &str) {
    h.frame(vec![Event::Text(text.into())]);
}

/// The file reached the server and the session is live, with these codes (if the server has any).
fn on_the_server(h: &mut Harness, script: &Shared, codes: Option<(&str, &str)>) -> Vec<Shape> {
    shared_on_the_server(h, script, links(0, codes))
}

/// [`on_the_server`] with the server's links for others as given.
fn shared_on_the_server(h: &mut Harness, script: &Shared, shared: SharedLinks) -> Vec<Shape> {
    let link = |token: &str| format!("figcraft://join?server=wss://share.example:443/&file=f1&token={token}");
    let published = Published {
        server: "wss://share.example:443/".into(),
        file: "f1".into(),
        name: "Untitled".into(),
        edit: link("EditEditEditEditEdit00"),
        view: link("ViewViewViewViewView00"),
        owner: Some("OwnerOwnerOwnerOwner00".into()),
        expires: None,
    };
    let welcome = LiveEvent::Welcome { info: info(Role::Guest, &published.edit), peers: vec![] };
    script.lock().unwrap().events.extend([LiveEvent::Published(published), LiveEvent::Shared(Ok(shared)), welcome]);
    h.frames(3)
}

/// The session's document arrives (the host's welcome): it opens in a tab of its own.
fn document_arrives(h: &mut Harness, script: &Shared) {
    use figcraft_engine::figcraft_collab::message::ServerMsg;
    use figcraft_engine::figcraft_collab::{Client, Limits, Outgoing, Server};
    let mut server = Server::new(h.app.session.doc().unwrap().clone(), "", Limits::default());
    let ClientMsg::Hello(me) = Client::hello("f", "", "Ada", "") else { panic!("no hello") };
    let (_, outs) = server.join(me).unwrap();
    let w = outs.into_iter().find_map(|o| if let Outgoing::To(_, w @ ServerMsg::Welcome { .. }) = o { Some(w) } else { None }).unwrap();
    script.lock().unwrap().events.push(LiveEvent::Sync(w));
    h.frames(3);
}

/// The middle of the `n`th text `s` on screen (top to bottom as drawn).
fn nth_center(shapes: &[Shape], s: &str, n: usize) -> egui::Pos2 {
    let all: Vec<_> = texts(shapes).into_iter().filter(|(t, _, _)| t == s).collect();
    let (_, p, size) = all.get(n).cloned().unwrap_or_else(|| panic!("no text {s:?} #{n}"));
    p + size / 2.0
}

/// The open file shared on the server, with these links, and the dialog open on it.
fn shared(h: &mut Harness, script: &Shared) -> Vec<Shape> {
    h.app.session.execute("collab.share", &json!({})).unwrap();
    on_the_server(h, script, Some(("K7M2QX9TD", "4FHJ8R2WN")));
    open(h)
}

/// Choose `item` from More.
fn more(h: &mut Harness, shapes: &[Shape], item: &str) -> Vec<Shape> {
    let shapes = click(h, shapes, "More");
    click(h, &shapes, item)
}

/// Frames enough for a background request or two to be answered.
fn settle(h: &mut Harness) -> Vec<Shape> {
    h.frames(8)
}

#[test]
fn the_dialog_is_the_kits_near_the_top_of_the_window() {
    let (mut h, script) = harness();
    let shapes = open(&mut h);
    for t in
        ["Share this file", "Copy link", "Add comma separated emails to invite", "Invite", "Who has access", "Anyone", "no access", "owner", "More"]
    {
        assert!(has_text(&shapes, t), "the dialog shows {t:?}");
    }
    for t in ["Online", "Local network", "Start sharing", "Join with a code", "Advanced", "Your name", "Copy Dev Mode link"] {
        assert!(!has_text(&shapes, t), "not {t:?}");
    }
    assert!(text_center(&shapes, "Share this file").y < 160.0, "near the top, as the kit's");
    // Invite does nothing until there is an address.
    click(&mut h, &shapes, "Invite");
    assert!(script.lock().unwrap().published.is_empty());
    // Escape closes it.
    key(&mut h, Key::Escape);
    assert!(!h.app.ui.live.share.open);
}

#[test]
fn copy_link_puts_the_file_online_then_copies_its_link() {
    let (mut h, script) = harness();
    let shapes = open(&mut h);
    click(&mut h, &shapes, "Copy link");
    assert_eq!(script.lock().unwrap().published.len(), 1, "the file goes to the server first");
    assert!(script.lock().unwrap().copied.is_empty(), "nothing to copy yet");
    on_the_server(&mut h, &script, Some(("K7M2QX9TD", "4FHJ8R2WN")));
    let shapes = settle(&mut h);
    // As in the browser: the file's address in the server's browser build, which opens anywhere.
    let edit = links(0, None).edit.link;
    let web = figcraft_engine::team::file_link(WEB, &edit);
    assert_eq!(script.lock().unwrap().copied, std::slice::from_ref(&web), "then its link is copied");
    assert_eq!(figcraft_engine::live::join_target("", &web).as_deref(), Ok(edit.as_str()), "and it joins the file");
    assert!(has_text(&shapes, "can edit") && has_text(&shapes, "Copy Dev Mode link"));
    click(&mut h, &shapes, "Copy Dev Mode link");
    assert_eq!(script.lock().unwrap().copied.last(), Some(&figcraft_engine::team::file_link(WEB, &format!("{edit}&dev=1"))));
}

#[test]
fn lowering_link_access_makes_new_links_and_no_access_revokes_them() {
    let (mut h, script) = harness();
    let shapes = shared(&mut h, &script);
    let shapes = click(&mut h, &shapes, "can edit");
    for t in ["can comment", "can view", "No access"] {
        assert!(has_text(&shapes, t), "the menu offers {t:?}");
    }
    click(&mut h, &shapes, "can view");
    let shapes = settle(&mut h);
    {
        let s = script.lock().unwrap();
        assert_eq!((s.revoked.as_slice(), s.shares), (["edit0".to_string(), "view0".to_string()].as_slice(), 1), "the old links go, new ones come");
    }
    let share = h.app.session.live().share.clone().unwrap();
    assert_eq!((share.access, share.link()), (Access::View, Some(links(1, None).view.unwrap().link.as_str())));
    assert!(has_text(&shapes, "can view"));
    // No access: every link others got is revoked, and there is none to copy.
    let shapes = click(&mut h, &shapes, "can view");
    click(&mut h, &shapes, "No access");
    let shapes = settle(&mut h);
    assert!(script.lock().unwrap().revoked.ends_with(&["edit1".to_string(), "view1".to_string()]));
    assert!(h.app.session.live().share.as_ref().is_some_and(|s| s.links.is_none()));
    assert!(has_text(&shapes, "no access") && !has_text(&shapes, "Copy link"));
}

#[test]
fn invited_people_get_links_of_their_own() {
    let (mut h, script) = harness();
    let shapes = shared(&mut h, &script);
    click(&mut h, &shapes, "Add comma separated emails to invite");
    type_text(&mut h, "Ada@Example.com, bo@example.com");
    key(&mut h, Key::Enter);
    let shapes = settle(&mut h);
    let labels: Vec<String> = script.lock().unwrap().links.iter().map(|l| l.label.clone()).collect();
    assert_eq!(labels, ["Invited: ada@example.com", "Invited: bo@example.com"]);
    assert!(has_text(&shapes, "ada@example.com") && has_text(&shapes, "bo@example.com"));
    assert!(h.app.ui.live.share.emails.is_empty(), "the field is cleared");
    // Signed out: their links are copied, to send them.
    let copied = script.lock().unwrap().copied.last().cloned().unwrap_or_default();
    assert!(copied.contains(&format!("ada@example.com: {WEB}#file=figcraft%3A%2F%2Fjoin")) && copied.contains("Person"), "{copied}");
    // Bo can view: a new link for Bo alone.
    h.click(nth_center(&shapes, "can edit", 2));
    let shapes = h.frames(2);
    click(&mut h, &shapes, "can view");
    let shapes = settle(&mut h);
    let now: Vec<(String, Access)> = script.lock().unwrap().links.iter().map(|l| (l.label.clone(), l.access)).collect();
    assert_eq!(now, [("Invited: ada@example.com".to_string(), Access::Edit), ("Invited: bo@example.com".to_string(), Access::View)]);
    assert!(!script.lock().unwrap().links.iter().any(|l| l.id == "p2"), "Bo's first link is revoked");
    // Ada removed: her link goes, and her row.
    h.click(nth_center(&shapes, "can edit", 1));
    let shapes = h.frames(2);
    click(&mut h, &shapes, "Remove");
    let shapes = settle(&mut h);
    assert_eq!(script.lock().unwrap().links.len(), 1);
    assert!(!has_text(&shapes, "ada@example.com") && has_text(&shapes, "bo@example.com"));
}

#[test]
fn signed_in_an_invite_goes_to_the_accounts_server() {
    let (mut h, script) = harness();
    h.app.session.account.account = Some(figcraft_engine::account::Account {
        email: "me@example.com".into(),
        name: "Me".into(),
        invite_link: String::new(),
        invited: vec![],
        profile: figcraft_engine::account::Profile { onboarded: true, ..Default::default() },
    });
    h.app.session.account.profile.onboarded = true;
    let shapes = shared(&mut h, &script);
    click(&mut h, &shapes, "Add comma separated emails to invite");
    type_text(&mut h, "cy@example.com");
    key(&mut h, Key::Enter);
    settle(&mut h);
    let out = h.app.session.account.take_outgoing();
    let shares: Vec<_> =
        out.iter().filter_map(|o| if let figcraft_engine::account::Outgoing::Share(v) = o { Some(v.clone()) } else { None }).collect();
    assert_eq!(shares.len(), 1);
    assert_eq!(shares[0]["emails"], json!(["cy@example.com"]));
    assert!(shares[0]["target"].as_str().is_some_and(|t| t.starts_with("figcraft://join?") && t.contains("Person")));
    assert_eq!(h.app.ui.toast.as_ref().map(|t| t.0.as_str()), Some("Invited 1 person: the file is on their Home"));
}

#[test]
fn more_shares_on_the_local_network_and_copies_its_code() {
    let (mut h, script) = harness();
    let shapes = open(&mut h);
    let shapes = more(&mut h, &shapes, "Share on the local network");
    assert_eq!(script.lock().unwrap().hosted.len(), 1);
    assert!(has_text(&shapes, "Anyone on your network with the link") && has_text(&shapes, "can edit"));
    more(&mut h, &shapes, "Copy join code");
    assert_eq!(script.lock().unwrap().copied.last(), Some(&code::grouped(&HOST.code())));
    let shapes = h.frames(2);
    click(&mut h, &shapes, "Copy link");
    assert_eq!(script.lock().unwrap().copied.last(), Some(&HOST.link()));
    let shapes = h.frames(2);
    more(&mut h, &shapes, "Stop sharing on the local network");
    assert_eq!(script.lock().unwrap().left, 1);
}

#[test]
fn a_file_on_the_server_is_removed_after_asking() {
    let (mut h, script) = harness();
    let shapes = shared(&mut h, &script);
    let shapes = more(&mut h, &shapes, "Remove from server…");
    assert!(has_text(&shapes, "Remove this file from the server? It can't be shared or opened from there again. Your copy stays open here."));
    click(&mut h, &shapes, "Remove");
    settle(&mut h);
    assert_eq!(script.lock().unwrap().removed, ["f1"]);
}

#[test]
fn a_dev_mode_link_opens_the_file_in_dev_mode() {
    let (mut h, script) = harness();
    h.app.session.execute("collab.join", &json!({"link": format!("{PLAIN}&dev=1")})).unwrap();
    assert!(h.app.session.live.iter().any(|l| l.dev));
    script.lock().unwrap().events.push(LiveEvent::Welcome { info: info(Role::Guest, PLAIN), peers: vec![] });
    h.frames(2);
    document_arrives(&mut h, &script);
    assert_eq!(h.app.ui.right_tab, crate::RightTab::Inspect, "Dev Mode");
    // Only once: you may leave it.
    h.app.ui.right_tab = crate::RightTab::Design;
    h.frames(2);
    assert_eq!(h.app.ui.right_tab, crate::RightTab::Design);
}

#[test]
fn joining_from_the_home_screen_opens_the_file() {
    let (mut h, script) = harness();
    h.app.ui.home = true;
    let shapes = h.frames(2);
    assert!(!has_text(&shapes, "Join with a code"), "the home screen has no join row: the Join dialog does it");
    crate::menus::invoke(&mut h.app, "collab.join");
    h.frames(2);
    type_text(&mut h, "K7M-2QX-9TD");
    key(&mut h, Key::Enter);
    assert_eq!(script.lock().unwrap().joined, ["figcraft://join?server=wss://share.example/&code=K7M2QX9TD"]);
    let shapes = h.frames(2);
    assert!(has_text(&shapes, "Joining live session…") && h.app.ui.home, "the Join dialog shows the join");
    // It failed: the dialog says why, and the home screen stays.
    script.lock().unwrap().events.push(LiveEvent::Status(Status::Failed { reason: "That code has expired or its sharing was stopped".into() }));
    let shapes = h.frames(3);
    assert!(has_text(&shapes, "That code has expired or its sharing was stopped") && h.app.ui.home);
    // Again, and it works: the shared file's tab comes to the front.
    click(&mut h, &shapes, "Try again");
    assert_eq!(script.lock().unwrap().joined.len(), 2);
    script.lock().unwrap().events.push(LiveEvent::Welcome { info: info(Role::Guest, PLAIN), peers: vec![] });
    h.frames(2);
    document_arrives(&mut h, &script);
    assert!(!h.app.ui.home, "the joined file shows");
    assert_eq!(h.app.session.docs.len(), 2);
}

/// Asserts the join dialog uses the same shared join field, focuses it, and shows the join once
/// it starts.
#[test]
fn the_join_dialog_is_the_same_field() {
    let (mut h, script) = harness();
    crate::menus::invoke(&mut h.app, "collab.join");
    let shapes = h.frames(2);
    assert!(has_text(&shapes, "Type the join code you were given, or paste an invite link.") && has_text(&shapes, "Code or invite link"));
    assert!(h.ctx.memory(|m| m.has_focus(egui::Id::unique("live_join_link"))), "the field has the keyboard");
    type_text(&mut h, "4fh j8r 2wn");
    key(&mut h, Key::Enter);
    h.frames(1);
    assert_eq!(script.lock().unwrap().joined, ["figcraft://join?server=wss://share.example/&code=4FHJ8R2WN"]);
    assert!(h.app.ui.live.join.is_none(), "its field goes once the join started");
    assert!(has_text(&h.frames(1), "Joining live session…"), "and it shows the join");
}

#[test]
fn the_server_preference_resets_and_turns_online_sharing_off() {
    let (mut h, script) = harness();
    h.app.ui.preferences = true;
    let shapes = h.frames(2);
    assert!(has_text(&shapes, "Server for online sharing") && has_text(&shapes, "Reset"));
    click(&mut h, &shapes, "Reset");
    assert_eq!(h.app.session.prefs.publish_server, DEFAULT_SERVER);
    // Off: Preferences say what that means, and the dialog offers the local network under More.
    h.app.session.execute("pref.publishServer", &json!({"url": ""})).unwrap();
    let shapes = h.frames(2);
    assert!(has_text(&shapes, "Online sharing is off: files are shared on the local network only."));
    h.app.ui.preferences = false;
    h.frames(2);
    let shapes = open(&mut h);
    assert!(has_text(&shapes, "Online sharing is off: no server is set in Preferences. More has the local network and joining a file."));
    assert!(!has_text(&shapes, "Copy link"));
    more(&mut h, &shapes, "Share on the local network");
    let s = script.lock().unwrap();
    assert!(s.published.is_empty() && s.hosted.len() == 1, "shared on the local network");
}

#[test]
fn an_open_dialog_asks_for_no_frames() {
    let (mut h, script) = harness();
    open(&mut h);
    assert!(h.goes_quiet(), "not shared");
    h.app.session.execute("collab.share", &json!({})).unwrap();
    on_the_server(&mut h, &script, Some(("K7M2QX9TD", "4FHJ8R2WN")));
    assert!(h.goes_quiet(), "shared, the dialog showing who has access");
    let (shapes, delay) = h.frame(vec![]);
    assert!(has_text(&shapes, "can edit") && delay >= Duration::from_secs(1));
}
