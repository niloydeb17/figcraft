//! Live sessions are per document in the window: the Share popover, the people at the top of the
//! right panel, the cursors on the canvas and `ui.inspect`'s `live` are the session of the tab in
//! front. Another tab shows its own session, or the way to start one; a session behind keeps
//! running without drawing anything or asking for frames.

use std::sync::{Arc, Mutex};

use figcraft_engine::figcraft_collab::message::ClientMsg;
use figcraft_engine::live::{Access, Event as LiveEvent, HostOptions, LiveLink, Peer, Presence, Role, SessionInfo};
use serde_json::{Value, json};

use crate::live_tests::{Harness, has_text, text_center};

type Queue = Arc<Mutex<Vec<(u64, LiveEvent)>>>;

/// A link that hosts any number of sessions and hands out scripted events for each.
struct Hosting(Queue);

impl LiveLink for Hosting {
    fn host(&mut self, session: u64, _: HostOptions) -> Result<SessionInfo, String> {
        let address = format!("127.0.0.1:{}", 47_899 + session);
        Ok(SessionInfo {
            role: Role::Host,
            me: 1,
            name: "Ada Lovelace".into(),
            color: "#14AE5C".into(),
            file_name: "Shared".into(),
            pages: vec![],
            link: format!("figcraft://join?host={address}&file=f&token=t"),
            url: format!("ws://{address}/?file=f&token=t"),
            address,
            network: false,
            access: Access::Edit,
        })
    }
    fn join(&mut self, _: u64, _: &str, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn leave(&mut self, _: u64) {}
    fn presence(&mut self, _: u64, _: &Presence) {}
    fn send(&mut self, _: u64, _: ClientMsg) {}
    fn poll(&mut self) -> Vec<(u64, LiveEvent)> {
        std::mem::take(&mut *self.0.lock().unwrap())
    }
}

fn harness() -> (Harness, Queue) {
    let (mut h, _) = Harness::new();
    let queue = Queue::default();
    h.app.session.live.install(Box::new(Hosting(queue.clone())));
    h.frames(3);
    (h, queue)
}

/// Share the file in front on the local network from its Share dialog (which is open): More ▸
/// Share on the local network.
fn start_sharing(h: &mut Harness, shapes: &[egui::Shape]) -> Vec<egui::Shape> {
    more(h, shapes, "Share on the local network")
}

/// Choose `item` from the Share dialog's More menu.
fn more(h: &mut Harness, shapes: &[egui::Shape], item: &str) -> Vec<egui::Shape> {
    h.click(text_center(shapes, "More"));
    let shapes = h.frames(2);
    h.click(text_center(&shapes, item));
    h.frames(2)
}

/// What the Share dialog shows while the file is shared on the local network.
const ON_THE_NETWORK: &str = "Anyone on your network with the link";

fn live(h: &Harness) -> Value {
    h.app.inspect()["live"].clone()
}

fn run(h: &mut Harness, cmd: &str, params: Value) {
    h.app.session.execute(cmd, &params).unwrap();
    h.frames(3);
}

fn hana() -> Peer {
    Peer { client_id: 2, name: "Hana".into(), color: "#9747FF".into() }
}

#[test]
fn the_share_popover_is_the_session_of_the_tab_in_front() {
    let (mut h, queue) = harness();
    // Start a session for the first file from its Share dialog.
    let shapes = h.frames(1);
    h.click(text_center(&shapes, "Share"));
    let shapes = h.frames(2);
    assert!(has_text(&shapes, "Share this file"));
    let shapes = start_sharing(&mut h, &shapes);
    for t in [ON_THE_NETWORK, "Copy link", "can edit"] {
        assert!(has_text(&shapes, t), "hosting shows {t:?}");
    }
    queue.lock().unwrap().push((1, LiveEvent::PeerJoined(hana())));
    h.frames(2);
    assert_eq!(
        (&live(&h)["status"], &live(&h)["session"], &live(&h)["peers"], &live(&h)["sessions"]),
        (&json!("live"), &json!(1), &json!(1), &json!(1))
    );
    // A new file: its tab isn't in that session and offers to start one.
    run(&mut h, "file.new", json!({}));
    let l = live(&h);
    assert_eq!(
        (&l["status"], &l["session"], &l["role"], &l["peers"], &l["sessions"]),
        (&json!("off"), &Value::Null, &Value::Null, &json!(0), &json!(1))
    );
    let shapes = h.frames(1);
    assert!(has_text(&shapes, "Share this file") && has_text(&shapes, "More"), "the new file's dialog");
    assert!(!has_text(&shapes, ON_THE_NETWORK), "nothing of the other file's session");
    // Start one here too: both run, each tab shows its own.
    let shapes = start_sharing(&mut h, &shapes);
    assert!(has_text(&shapes, ON_THE_NETWORK));
    assert_eq!((&live(&h)["session"], &live(&h)["peers"], &live(&h)["sessions"]), (&json!(2), &json!(0), &json!(2)));
    assert_eq!(h.app.session.live().info.as_ref().unwrap().address, "127.0.0.1:47901");
    run(&mut h, "doc.switch", json!({"index": 0}));
    assert_eq!((&live(&h)["session"], &live(&h)["peers"]), (&json!(1), &json!(1)));
    assert_eq!(h.app.session.live().info.as_ref().unwrap().address, "127.0.0.1:47900");
    // Stopping the first leaves the second running.
    let shapes = h.frames(1);
    let shapes = more(&mut h, &shapes, "Stop sharing on the local network");
    assert!(!has_text(&shapes, ON_THE_NETWORK));
    assert_eq!((&live(&h)["status"], &live(&h)["sessions"]), (&json!("off"), &json!(1)));
    run(&mut h, "doc.switch", json!({"index": 1}));
    assert_eq!((&live(&h)["status"], &live(&h)["session"]), (&json!("live"), &json!(2)));
}

#[test]
fn a_session_behind_draws_nothing_and_asks_for_no_frames() {
    let (mut h, queue) = harness();
    run(&mut h, "collab.host", json!({}));
    let page = h.page();
    let cursor = |x: f64, y: f64| Presence { cursor: Some((page, x, y)), ..Presence::default() };
    queue.lock().unwrap().extend([(1, LiveEvent::PeerJoined(hana())), (1, LiveEvent::Presence(2, cursor(40.0, 40.0)))]);
    let shapes = h.frames(3);
    assert!(has_text(&shapes, "Hana"), "their name tag by their cursor");
    assert_eq!(live(&h)["cursorsShown"], json!([2]));
    // Your chat box is this session's.
    crate::live::open_chat(&mut h.app);
    h.frames(2);
    assert_eq!(live(&h)["chatBox"]["editing"], true);
    // Another file in front: none of it shows there, and the window goes quiet.
    run(&mut h, "file.new", json!({}));
    let shapes = h.frames(2);
    assert!(!has_text(&shapes, "Hana"));
    let l = live(&h);
    assert_eq!((&l["cursorsShown"], &l["chatBox"], &l["status"]), (&json!([]), &Value::Null, &json!("off")));
    assert!(h.goes_quiet(), "a session behind doesn't keep the window drawing");
    // What happens there arrives all the same, without showing here.
    queue.lock().unwrap().push((1, LiveEvent::Presence(2, cursor(90.0, 10.0))));
    let shapes = h.frames(2);
    assert!(!has_text(&shapes, "Hana"));
    let first = h.app.session.docs[0].uid;
    assert_eq!(h.app.session.live_of(first).presence[&2].cursor, Some((page, 90.0, 10.0)));
    assert!(h.goes_quiet());
    // Back on its tab the cursor is where it was last reported.
    run(&mut h, "doc.switch", json!({"index": 0}));
    assert_eq!(crate::live::shown_at(&h.app), vec![(2, (90.0, 10.0))]);
    assert_eq!(live(&h)["chatBox"], Value::Null, "the chat box closed with the switch");
}
