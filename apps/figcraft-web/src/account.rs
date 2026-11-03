//! The signed-in account in the browser build: asks the page's server who is signed in
//! (`GET /auth/me`, with the session cookie) when the page opens, and sends what the onboarding
//! keeps (`POST /auth/profile`, `/auth/invite`) as the engine queues it.

use std::cell::RefCell;
use std::rc::Rc;

use figcraft_engine::account::Outgoing;
use figcraft_ui_egui::FigApp;
use serde_json::Value;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::{JsFuture, spawn_local};

#[derive(Default)]
pub struct Account {
    asked: bool,
    /// `account.set` parameters that arrived since the last frame.
    arrived: Rc<RefCell<Vec<Value>>>,
}

impl Account {
    pub fn tick(&mut self, fig: &mut FigApp, ctx: &egui::Context) {
        if !self.asked {
            self.asked = true;
            let (arrived, ctx) = (self.arrived.clone(), ctx.clone());
            spawn_local(async move {
                if let Some((status, body)) = fetch("GET", "/auth/me", None).await
                    && let Some(p) = crate::account_me::params(status, &body)
                {
                    arrived.borrow_mut().push(p);
                    ctx.request_repaint();
                }
            });
        }
        let arrived: Vec<Value> = std::mem::take(&mut *self.arrived.borrow_mut());
        for p in arrived {
            fig.run("account.set", p);
        }
        for o in fig.session.account.take_outgoing() {
            let (path, body) = match o {
                Outgoing::Profile(v) => ("/auth/profile", v),
                Outgoing::Invite(emails) => ("/auth/invite", serde_json::json!({"emails": emails})),
            };
            spawn_local(async move {
                match fetch("POST", path, Some(body.to_string())).await {
                    Some((200, _)) => {}
                    Some((status, text)) => log::warn!("{path}: the server answered {status}: {text}"),
                    None => log::warn!("{path}: the server can't be reached"),
                }
            });
        }
    }
}

/// One same-origin request (the session cookie goes with it): its status and text.
async fn fetch(method: &str, path: &str, body: Option<String>) -> Option<(u16, String)> {
    let window = web_sys::window()?;
    let init = web_sys::RequestInit::new();
    init.set_method(method);
    if let Some(b) = body {
        let headers = web_sys::Headers::new().ok()?;
        headers.set("Content-Type", "application/json").ok()?;
        init.set_headers(&headers);
        init.set_body(&wasm_bindgen::JsValue::from_str(&b));
    }
    let resp = JsFuture::from(window.fetch_with_str_and_init(path, &init)).await.ok()?;
    let resp: web_sys::Response = resp.dyn_into().ok()?;
    let status = resp.status();
    let text = JsFuture::from(resp.text().ok()?).await.ok()?.as_string().unwrap_or_default();
    Some((status, text))
}
