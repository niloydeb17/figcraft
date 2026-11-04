//! Files in the cloud, as in Figma's web app: a file you change is kept on the page's own server
//! (shared with a join code, so the server's team list shows it on every computer: Home ▸ Online),
//! and the page's address follows the open file (`#file=<code or link>`), so a reload or a
//! bookmark opens that file again without asking.

use std::collections::BTreeMap;

use figcraft_ui_egui::FigApp;
use serde_json::json;

/// How long a file must stay still (no new edit, no gesture or text edit going on) before it goes
/// to the server, in milliseconds: publishing makes the tab read-only for a moment.
const SETTLE_MS: f64 = 1_500.0;

/// The page's own files on its server, and the address that names the open one.
#[derive(Default)]
pub struct Cloud {
    /// The address (`#file=…`) of each file that has one, by its `DocState::uid`. A file sent to the
    /// server (or tried) is in `sent`, so each file goes once.
    named: BTreeMap<u64, String>,
    sent: std::collections::BTreeSet<u64>,
    /// The open file's uid and revision, and when that revision was first seen.
    still: Option<(u64, u64, f64)>,
    /// The page opened a file from its address: the first file joined keeps that address.
    adopt: bool,
    /// Home must stop naming the editor file, without erasing a startup deep link mid-join.
    navigation: crate::navigation::Navigation,
}

impl Cloud {
    /// The page was opened with `#file=…`: join that file at once (it's this page's own address,
    /// not a link from elsewhere, so there's nothing to confirm).
    pub fn open_from_address(&mut self, fig: &mut FigApp, fragment: &str) {
        let Some(target) = file_target(fragment) else { return };
        self.adopt = true;
        fig.run("collab.join", json!({"link": target}));
    }

    /// Once a frame. egui draws only when something happens: while a file waits to go to the
    /// server, ask for another frame so it goes even if the pointer stays still.
    pub fn tick(&mut self, fig: &mut FigApp, ctx: &egui::Context) {
        self.follow_address(fig);
        if self.keep(fig, js_sys::Date::now()) {
            ctx.request_repaint_after(std::time::Duration::from_millis(300));
        }
    }

    /// Put the open file on the server once it has changes and is still for a moment. True while
    /// it waits for that moment.
    fn keep(&mut self, fig: &mut FigApp, now: f64) -> bool {
        let s = &fig.session;
        if fig.ui.home || s.prefs.publish_server.trim().is_empty() || !s.live.available() {
            return false;
        }
        let Some(d) = s.docs.get(s.active) else { return false };
        let (uid, revision) = (d.uid, d.revision);
        if self.sent.contains(&uid) || d.branch_of.is_some() || !d.is_dirty() {
            return false;
        }
        // In a session already (one you joined, or one on its way): it's on a server.
        if s.live.iter().any(|l| l.doc_uid == Some(uid) || l.tab == Some(uid)) {
            self.sent.insert(uid);
            return false;
        }
        let busy = d.interaction.is_some() || d.text_edit.is_some();
        match self.still {
            Some((u, r, since)) if u == uid && r == revision && !busy => {
                if now - since < SETTLE_MS {
                    return true;
                }
            }
            _ => {
                self.still = Some((uid, revision, now));
                return true;
            }
        }
        self.sent.insert(uid);
        self.still = None;
        fig.run("collab.share", json!({}));
        false
    }

    /// Point the page's address at the open file once it is on a server.
    fn follow_address(&mut self, fig: &FigApp) {
        let s = &fig.session;
        let Some(window) = web_sys::window() else { return };
        let current = window.location().hash().unwrap_or_default();
        // With no files the UI also shows Home, even when the explicit Home toggle is false.
        let home = fig.ui.home || !fig.has_files();
        if let Some(fragment) = self.navigation.follow(home, &current)
            && let Err(e) = window.location().set_hash(fragment)
        {
            log::warn!("couldn't return to Home in the address: {e:?}");
        }
        if home {
            return;
        }
        let Some(uid) = s.docs.get(s.active).map(|d| d.uid) else { return };
        let wanted = match self.named.get(&uid) {
            Some(w) => w.clone(),
            None => {
                let Some(live) = s.live.iter().find(|l| l.doc_uid == Some(uid) && l.status.active()) else { return };
                let wanted = if let Some(code) = live.share.as_ref().and_then(|sh| sh.code()) {
                    format!("#file={code}")
                } else if live.share.is_none() {
                    let Some(link) = live.info.as_ref().map(|i| i.link.clone()).filter(|l| !l.is_empty()) else { return };
                    if self.adopt && current.starts_with("#file=") {
                        current.clone()
                    } else {
                        format!("#file={}", String::from(js_sys::encode_uri_component(&link)))
                    }
                } else {
                    // Yours on the server, its code still on its way.
                    return;
                };
                self.adopt = false;
                self.named.insert(uid, wanted.clone());
                wanted
            }
        };
        if current != wanted
            && let Err(e) = window.location().set_hash(&wanted)
        {
            log::warn!("couldn't name the file in the address: {e:?}");
        }
    }
}

/// The file in `#file=<code or link>` (`fragment` is `location.hash`), unescaped.
fn file_target(fragment: &str) -> Option<String> {
    const MAX: usize = 3 * 2048;
    let f = fragment.strip_prefix('#').unwrap_or(fragment);
    let raw = f.strip_prefix("file=").filter(|r| !r.is_empty() && r.len() <= MAX)?;
    let decoded = js_sys::decode_uri_component(raw).ok().map(String::from).unwrap_or_else(|| raw.to_string());
    Some(decoded.trim().to_string()).filter(|t| !t.is_empty())
}
