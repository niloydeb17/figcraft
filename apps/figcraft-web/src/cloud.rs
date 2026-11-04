//! Files in the cloud, as in Figma's web app: a file you make or open is kept on the page's own
//! server (shared with a join code), the account's recent files list it (`account.recentOpened`:
//! Home ▸ Recently viewed, after a reload and on other computers), and the page's address follows
//! the open file (`#file=<code or link>`), so a reload or a bookmark opens that file again without
//! asking. An automatic save that fails is tried again, a little later each time.

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
    /// Automatic saves that failed: when each file may be tried again, and how many tries so far.
    retry: BTreeMap<u64, (f64, u32)>,
    /// The name and kind the account's recent files last heard for each named file.
    told: BTreeMap<u64, (String, &'static str)>,
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
        self.tell_recents(fig);
        let now = js_sys::Date::now();
        let failing = self.watch_failures(fig, now);
        if self.keep(fig, now) || failing {
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
        // Every file made or opened here goes (a new one too, as Figma's do), so the account's
        // recent files can open it again.
        if self.sent.contains(&uid) || d.branch_of.is_some() {
            return false;
        }
        if self.retry.get(&uid).is_some_and(|(at, _)| now < *at) {
            return true;
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
        if fig.run("collab.share", json!({"quiet": true})).is_none() {
            self.failed(uid, now);
            return true;
        }
        false
    }

    /// An automatic save of `uid` failed at `now`: try again later (5 s, doubling to a minute).
    fn failed(&mut self, uid: u64, now: f64) {
        self.sent.remove(&uid);
        let tries = self.retry.get(&uid).map_or(0, |(_, n)| *n) + 1;
        let wait = (5_000.0 * 2f64.powi(tries.min(4) as i32 - 1)).min(60_000.0);
        self.retry.insert(uid, (now + wait, tries));
        log::warn!("the automatic save didn't go through (try {tries}); trying again in {} s", wait / 1000.0);
    }

    /// Automatic saves whose upload failed before the file got on the server: once their wait is
    /// over, the failed session is cleared and the file goes again. True while one waits.
    fn watch_failures(&mut self, fig: &mut FigApp, now: f64) -> bool {
        let failed: Vec<(u64, u64)> = fig
            .session
            .live
            .iter()
            .filter(|l| l.quiet && l.published.is_none() && matches!(l.status, figcraft_engine::live::Status::Failed { .. }))
            .filter_map(|l| l.doc_uid.or(l.tab).map(|uid| (uid, l.id)))
            .collect();
        let mut waiting = false;
        for (uid, session) in failed {
            match self.retry.get(&uid) {
                // Seen failing just now: wait (the toolbar says the save didn't go through).
                None => {
                    self.failed(uid, now);
                    waiting = true;
                }
                Some((at, _)) if now < *at => waiting = true,
                Some(_) => {
                    fig.run("collab.leave", json!({"session": session}));
                    self.sent.remove(&uid);
                }
            }
        }
        // A file that got there is done with retrying.
        let live = &fig.session.live;
        self.retry.retain(|uid, _| !live.iter().any(|l| (l.doc_uid == Some(*uid) || l.tab == Some(*uid)) && l.published.is_some()));
        waiting
    }

    /// Keep each named file first in the account's recent files, again when its name changes.
    fn tell_recents(&mut self, fig: &mut FigApp) {
        let mut tell = Vec::new();
        for (uid, wanted) in &self.named {
            let Some(d) = fig.session.docs.iter().find(|d| d.uid == *uid) else { continue };
            let now = (d.doc.name.clone(), d.doc.kind.id());
            if self.told.get(uid) != Some(&now) {
                let Some(target) = file_target(wanted) else { continue };
                tell.push((*uid, target, now));
            }
        }
        for (uid, target, (name, kind)) in tell {
            fig.run("account.recentOpened", json!({"target": target, "name": name, "kind": kind, "tab": uid}));
            self.told.insert(uid, (name, kind));
        }
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
                // Opened from the account's recent files: it keeps the address it is listed by.
                let listed = s.account.recent_for_session(live.id).map(|r| r.target.clone());
                let wanted = if let Some(target) = listed {
                    format!("#file={}", String::from(js_sys::encode_uri_component(&target)))
                } else if let Some(code) = live.share.as_ref().and_then(|sh| sh.code()) {
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
