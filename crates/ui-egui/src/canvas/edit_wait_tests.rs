//! Dragging a frame or section left a trail: the canvas polled the render thread straight after
//! asking it for the edit, so the layers showed a frame or more behind their selection outline and
//! label. A quick edit's render is now waited for, briefly, and shows in the frame that made it.

use std::sync::Arc;
use std::time::Duration;

use figcraft_doc::{Document, NodeId};
use figcraft_engine::Session;
use figcraft_engine::params::id_of;
use figcraft_render::View;
use serde_json::json;
use web_time::Instant;

use super::edit_wait;
use crate::render_worker::{Job, Worker};

fn job(doc: &Arc<Document>, page: NodeId, uid: u64) -> Job {
    Job {
        doc: doc.clone(),
        page,
        view: View { origin: (-50.0, -50.0), scale: 1.0, width: 800, height: 600 },
        outline: false,
        outline_extras: Default::default(),
        outlined: vec![],
        skip: vec![],
        solo: None,
        lifted: vec![],
        proof: false,
        dots: None,
        uid,
        zoom_preview: true,
        at: Instant::now(),
    }
}

#[test]
fn a_dragged_frames_render_comes_with_the_edit() {
    let mut s = Session::new();
    let made = s.execute("node.create", &json!({"type": "section", "x": 0, "y": 0, "width": 400, "height": 300})).unwrap();
    let section = id_of(&made["id"]).unwrap();
    s.execute("node.create", &json!({"type": "frame", "x": 450, "y": 0, "width": 200, "height": 200})).unwrap();
    let mut w = Worker::new(egui::Context::default());
    let ds = s.ds().unwrap();
    w.submit(job(&ds.doc, ds.page, ds.uid));
    assert!(w.poll_within(Duration::from_secs(30)).is_some_and(|d| d.seq == 1 && !d.coarse));
    // Each step of the drag: the image polled in the same frame is the edit's own.
    for step in 1..=5 {
        s.execute("transform.move", &json!({"ids": [section], "dx": 10, "dy": 5})).unwrap();
        let ds = s.ds().unwrap();
        w.submit(job(&ds.doc, ds.page, ds.uid));
        let d = w.poll_within(Duration::from_secs(30)).unwrap();
        assert_eq!((d.seq, d.coarse), (step + 1, false), "step {step}");
    }
}

#[test]
fn only_quick_renders_are_waited_for() {
    assert_eq!(edit_wait(2.0), Duration::from_millis(4));
    assert_eq!(edit_wait(0.0), Duration::from_millis(1));
    assert_eq!(edit_wait(6.0), Duration::from_millis(8));
    assert_eq!(edit_wait(9.0), Duration::ZERO);
    assert_eq!(edit_wait(f64::NAN), Duration::ZERO);
    assert_eq!(edit_wait(-1.0), Duration::ZERO);
}
