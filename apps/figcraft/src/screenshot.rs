//! `--screenshot <file.png>`: draw about [`FRAMES`] frames, save the window through the control
//! channel's own `ui.screenshot`, and quit. With no file to open, the window shows a small motion
//! demo (a frame whose layers slide, scale, spin and fade in) with the timeline open.

use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

use figcraft_engine::Session;
use figcraft_ui_egui::{ControlReply, ControlRequest};
use serde_json::{Value, json};

/// Frames drawn before the picture is taken.
pub const FRAMES: u64 = 120;
/// Frames to wait for the picture after asking for it.
const PATIENCE: u64 = 900;

pub struct Shot {
    path: String,
    tx: Sender<ControlReply>,
    rx: Receiver<ControlReply>,
    asked_at: Option<u64>,
}

impl Shot {
    pub fn new(path: &str) -> Self {
        let path = std::path::absolute(path).map_or_else(|_| path.to_string(), |p| p.to_string_lossy().into_owned());
        let (tx, rx) = channel();
        Shot { path, tx, rx, asked_at: None }
    }

    /// Every frame: keep frames coming, ask for the picture once enough were drawn, and quit when
    /// it's written (exit code 1 when it couldn't be).
    pub fn tick(&mut self, ctx: &egui::Context, queue: &Arc<Mutex<Vec<ControlRequest>>>) {
        let frame = ctx.cumulative_frame_nr();
        ctx.request_repaint();
        if frame < FRAMES {
            return;
        }
        let Some(at) = self.asked_at else {
            if let Some(parent) = std::path::Path::new(&self.path).parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let req = ControlRequest {
                id: json!("screenshot"),
                method: "ui.screenshot".into(),
                params: json!({"path": self.path}),
                reply: self.tx.clone(),
                state: figcraft_ui_egui::control::RequestGate::new(),
            };
            queue.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(req);
            self.asked_at = Some(frame);
            return;
        };
        match self.rx.try_recv() {
            Ok(reply) => match reply.result {
                Ok(_) => std::process::exit(0),
                Err(e) => {
                    eprintln!("FigCraft: screenshot failed: {e}");
                    std::process::exit(1);
                }
            },
            Err(_) if frame > at + PATIENCE => {
                eprintln!("FigCraft: the screenshot wasn't delivered");
                std::process::exit(1);
            }
            Err(_) => {}
        }
    }
}

fn ex(s: &mut Session, id: &str, p: Value) -> Option<Value> {
    s.execute(id, &p).map_err(|e| log::warn!("screenshot demo: {id}: {e}")).ok()
}

fn id_of(v: Option<Value>) -> Value {
    v.map(|v| v.get("id").cloned().or_else(|| v.get("ids").and_then(|i| i.get(0)).cloned()).unwrap_or(Value::Null)).unwrap_or(Value::Null)
}

/// A new file with a small animated screen, the timeline open and the playhead part way.
pub fn demo(s: &mut Session) {
    ex(s, "file.new", json!({}));
    let frame = id_of(ex(s, "frame.create", json!({"x": 0, "y": 0, "width": 480, "height": 320, "name": "Welcome", "props": {"fill": "#F4F1FF"}})));
    let title = id_of(ex(
        s,
        "text.create",
        json!({"text": "Hello, motion", "x": 32, "y": 32, "parent": frame, "props": {"fontSize": 28, "fill": "#1E1B2E"}}),
    ));
    let card = id_of(ex(
        s,
        "shape.rectangle",
        json!({"x": 32, "y": 96, "width": 260, "height": 150, "parent": frame, "name": "Card", "props": {"fill": "#7B61FF", "cornerRadius": 16}}),
    ));
    let dot = id_of(ex(
        s,
        "shape.ellipse",
        json!({"x": 340, "y": 110, "width": 96, "height": 96, "parent": frame, "name": "Badge", "props": {"fill": "#FFB547"}}),
    ));
    let button = id_of(ex(
        s,
        "shape.rectangle",
        json!({"x": 32, "y": 262, "width": 140, "height": 36, "parent": frame, "name": "Button", "props": {"fill": "#1E1B2E", "cornerRadius": 18}}),
    ));
    ex(s, "motion.timeline.duration", json!({"id": frame, "seconds": 2}));
    ex(s, "motion.preset", json!({"ids": [title], "preset": "opacity", "start": 0, "duration": 0.4}));
    ex(s, "motion.preset", json!({"ids": [card], "preset": "position", "direction": "bottom", "amount": 120, "start": 0.2, "duration": 0.6}));
    ex(s, "motion.preset", json!({"ids": [card], "preset": "opacity", "start": 0.2, "duration": 0.6}));
    ex(s, "motion.preset", json!({"ids": [dot], "preset": "scale", "amount": 50, "start": 0.5, "duration": 0.5, "easing": "bouncy"}));
    ex(s, "motion.preset", json!({"ids": [dot], "preset": "rotation", "start": 0.5, "duration": 0.9, "easing": "easeInAndOut"}));
    ex(s, "motion.keyframe.add", json!({"ids": [button], "field": "fills/0", "time": 1.0, "value": "#1E1B2E", "easing": "linear"}));
    ex(s, "motion.keyframe.add", json!({"ids": [button], "field": "fills/0", "time": 1.6, "value": "#14AE5C", "easing": "easeOut"}));
    ex(s, "motion.keyframe.add", json!({"ids": [button], "field": "WIDTH", "time": 1.0, "value": 140, "easing": "linear"}));
    ex(s, "motion.keyframe.add", json!({"ids": [button], "field": "WIDTH", "time": 1.6, "value": 200, "easing": "gentle"}));
    ex(s, "select.set", json!({"ids": [card]}));
    ex(s, "view.motionTimeline", json!({"show": true}));
    ex(s, "motion.seek", json!({"time": 0.45}));
}
