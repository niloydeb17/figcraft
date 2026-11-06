//! The signed-in account in the browser build: asks the page's server who is signed in
//! (`GET /auth/me`, with the session cookie) when the page opens, and sends what the onboarding
//! keeps (`POST /auth/profile`, `/auth/invite`) as the engine queues it. Its recent files come
//! from `GET /auth/recents` once it is signed in, and each file kept or forgotten here goes back
//! with `POST /auth/recents`, so Home lists them after a reload and on other computers.

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
    /// The picture's bytes, when they arrived since the last frame.
    picture: Rc<RefCell<Option<Vec<u8>>>>,
    /// The account's recent files (`account.recents` parameters), when they arrived.
    recents: Rc<RefCell<Option<Value>>>,
    /// Pictures of recent files the server kept (target, PNG bytes), as they arrive.
    pictures: Rc<RefCell<Vec<Picture>>>,
}

/// A recent file's target and the PNG the server kept of it.
type Picture = (String, Vec<u8>);

/// Most recent files whose pictures are fetched when the page opens (Home shows a dozen at first).
const PICTURES_FETCHED: usize = 24;

/// The (target, picture token) of each file in `/auth/recents`'s answer that has a picture.
fn picture_tokens(list: &Value) -> Vec<(String, String)> {
    let files = list.get("files").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
    files
        .iter()
        .filter_map(|f| Some((f.get("target")?.as_str()?.to_string(), f.get("picture")?.as_str()?.to_string())))
        .take(PICTURES_FETCHED)
        .collect()
}

impl Account {
    pub fn tick(&mut self, fig: &mut FigApp, ctx: &egui::Context) {
        if !self.asked {
            self.asked = true;
            let (arrived, picture, recents, ctx) = (self.arrived.clone(), self.picture.clone(), self.recents.clone(), ctx.clone());
            let pictures = self.pictures.clone();
            spawn_local(async move {
                let Some((status, body)) = fetch("GET", "/auth/me", None).await else { return };
                if let Some(p) = crate::account_me::params(status, &body) {
                    arrived.borrow_mut().push(p);
                    ctx.request_repaint();
                    // Signed in: the files this account made or opened, on any computer.
                    match fetch("GET", "/auth/recents", None).await {
                        Some((200, text)) => match serde_json::from_str::<Value>(&text) {
                            Ok(v) => {
                                let tokens = picture_tokens(&v);
                                *recents.borrow_mut() = Some(v);
                                ctx.request_repaint();
                                // Each card's picture, newest files first.
                                for (target, token) in tokens {
                                    let path = format!("/auth/recents/picture?p={}", String::from(js_sys::encode_uri_component(&token)));
                                    if let Some(bytes) = fetch_bytes(&path).await {
                                        pictures.borrow_mut().push((target, bytes));
                                        ctx.request_repaint();
                                    }
                                }
                            }
                            Err(e) => log::warn!("/auth/recents: not a list: {e}"),
                        },
                        Some((status, text)) => log::warn!("/auth/recents: the server answered {status}: {text}"),
                        None => log::warn!("/auth/recents: the server can't be reached"),
                    }
                }
                // The server keeps Google's picture for the account and hands it over itself.
                if crate::account_me::has_picture(status, &body)
                    && let Some(bytes) = fetch_bytes("/auth/avatar").await
                {
                    *picture.borrow_mut() = Some(bytes);
                    ctx.request_repaint();
                }
            });
        }
        if let Some(bytes) = self.picture.borrow_mut().take() {
            fig.set_account_picture(ctx, &bytes);
        }
        let arrived: Vec<Value> = std::mem::take(&mut *self.arrived.borrow_mut());
        for p in arrived {
            fig.run("account.set", p);
        }
        // After `account.set` (which starts the list afresh) has run.
        if let Some(list) = self.recents.borrow_mut().take() {
            fig.run("account.recents", list);
        }
        for (target, bytes) in std::mem::take(&mut *self.pictures.borrow_mut()) {
            // One drawn in this page already is newer than the server's.
            if !fig.ui.home_view.recents.has_cloud_picture(&target) && !fig.set_recent_picture(ctx, &target, &bytes) {
                log::warn!("a recent file's picture from the server isn't a picture");
            }
        }
        for o in fig.session.account.take_outgoing() {
            let (path, body) = match o {
                Outgoing::Profile(v) => ("/auth/profile", v),
                Outgoing::Invite(emails) => ("/auth/invite", serde_json::json!({"emails": emails})),
                Outgoing::RecentOpened(file) => ("/auth/recents", serde_json::json!({"open": file})),
                Outgoing::RecentForget(target) => ("/auth/recents", serde_json::json!({"forget": target})),
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
pub(crate) async fn fetch(method: &str, path: &str, body: Option<String>) -> Option<(u16, String)> {
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

/// One same-origin GET's body when it is a 200.
async fn fetch_bytes(path: &str) -> Option<Vec<u8>> {
    let window = web_sys::window()?;
    let resp = JsFuture::from(window.fetch_with_str(path)).await.ok()?;
    let resp: web_sys::Response = resp.dyn_into().ok()?;
    if resp.status() != 200 {
        return None;
    }
    let buffer = JsFuture::from(resp.array_buffer().ok()?).await.ok()?;
    Some(js_sys::Uint8Array::new(&buffer).to_vec())
}
