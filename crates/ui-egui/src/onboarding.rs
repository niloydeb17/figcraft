//! The first-run onboarding: full-window steps shown once to a newly signed-in account (your name,
//! your role, who joins you, how well you know design tools, a plan, and what to make first), as
//! in UI3. The answers go through `onboarding.*` commands, which keep them on the account; the
//! right half previews a file with your cursor and name in it. The pictures are drawn here from
//! plain shapes.

use egui::{Align2, Color32, Context, CornerRadius, FontId, Id, Order, Pos2, Rect, Response, Sense, Shape, Stroke, StrokeKind, Ui, pos2, vec2};
use serde_json::json;

use crate::FigApp;
use crate::i18n::{tr, tr_noop};
use crate::icons::{self, Icon};
use crate::theme::{Tokens, regular, semibold};

/// The steps, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Name,
    Role,
    Invite,
    Experience,
    Plan,
    Create,
}

const STEPS: [Step; 6] = [Step::Name, Step::Role, Step::Invite, Step::Experience, Step::Plan, Step::Create];

/// The role chips: (answer, label).
const ROLES: [(&str, &str); 10] = [
    ("research", tr_noop("Research")),
    ("design", tr_noop("Design")),
    ("ux-writing", tr_noop("UX Writing")),
    ("student", tr_noop("Student")),
    ("data-analytics", tr_noop("Data analytics")),
    ("software-development", tr_noop("Software development")),
    ("marketing", tr_noop("Marketing")),
    ("product-management", tr_noop("Product management")),
    ("educator", tr_noop("Educator")),
    ("other", tr_noop("Other")),
];

const EXPERIENCE: [(&str, &str); 3] =
    [("many", tr_noop("Yes, many times")), ("few", tr_noop("Yes, a few times")), ("first", tr_noop("No, it's my first time"))];

/// What to make first: (answer, label, icon).
const CREATE: [(&str, &str, Icon); 6] = [
    ("app", tr_noop("App"), Icon::Frame),
    ("website", tr_noop("Website"), Icon::Page),
    ("graphic", tr_noop("Graphic"), Icon::Image),
    ("animation", tr_noop("Animation"), Icon::Play),
    ("presentation", tr_noop("Presentation"), Icon::Templates),
    ("whiteboard", tr_noop("Whiteboard"), Icon::Board),
];

/// A plan card: answer, name, who it is for, price line, and what it includes. Nothing is billed
/// yet: every plan is free while FigCraft is in beta.
struct Plan {
    id: &'static str,
    name: &'static str,
    badge: &'static str,
    about: &'static str,
    price: &'static str,
    heading: &'static str,
    features: &'static [&'static str],
}

const PLANS: [Plan; 3] = [
    Plan {
        id: "starter",
        name: tr_noop("Starter"),
        badge: "",
        about: tr_noop("For anyone trying FigCraft on their own"),
        price: tr_noop("Free"),
        heading: tr_noop("Everything you need to start:"),
        features: &[
            tr_noop("Unlimited files on your computer"),
            tr_noop("Design, prototyping and Dev Mode"),
            tr_noop("Live sessions with share links"),
        ],
    },
    Plan {
        id: "professional",
        name: tr_noop("Professional"),
        badge: tr_noop("Most popular"),
        about: tr_noop("For small teams who create and collaborate"),
        price: tr_noop("Free during the beta"),
        heading: tr_noop("Everything in Starter, plus:"),
        features: &[
            tr_noop("Team files and folders on your server"),
            tr_noop("A team-wide library of components, styles and variables"),
            tr_noop("Version history and branching"),
            tr_noop("Plugins and code connect for your team"),
        ],
    },
    Plan {
        id: "organization",
        name: tr_noop("Organization"),
        badge: "",
        about: tr_noop("For several teams who share libraries and need easy admin"),
        price: tr_noop("Free during the beta"),
        heading: tr_noop("Everything in Professional, plus:"),
        features: &[
            tr_noop("Teams under one admin"),
            tr_noop("Libraries shared across teams"),
            tr_noop("Sign-in with your company's accounts"),
            tr_noop("Usage and activity reports"),
        ],
    },
];

/// The answers while the steps are shown.
pub struct Flow {
    step: Step,
    name: String,
    tips: bool,
    role: String,
    role_other: String,
    invites: [String; 3],
    experience: String,
    plan: String,
    create: String,
    /// The invite link was copied this frame (the app says so).
    copied: bool,
}

impl Flow {
    fn new(app: &FigApp) -> Flow {
        let st = &app.session.account;
        let account_name = st.account.as_ref().map(|a| a.name.clone()).unwrap_or_default();
        let p = &st.profile;
        Flow {
            step: Step::Name,
            name: account_name,
            tips: p.tips,
            role: p.role.clone(),
            role_other: p.role_other.clone(),
            invites: Default::default(),
            experience: p.experience.clone(),
            plan: p.plan.clone(),
            create: p.create_first.clone(),
            copied: false,
        }
    }

    fn index(&self) -> usize {
        STEPS.iter().position(|s| *s == self.step).unwrap_or(0)
    }

    fn go(&mut self, by: isize) {
        let i = self.index().saturating_add_signed(by).min(STEPS.len() - 1);
        if let Some(s) = STEPS.get(i) {
            self.step = *s;
        }
    }

    fn emails(&self) -> Vec<String> {
        self.invites.iter().map(|e| e.trim().to_string()).filter(|e| !e.is_empty()).collect()
    }

    /// Continue may be pressed.
    fn ready(&self) -> bool {
        match self.step {
            Step::Name => !self.name.trim().is_empty(),
            Step::Role => !self.role.is_empty() && (self.role != "other" || !self.role_other.trim().is_empty()),
            Step::Invite => self.emails().iter().all(|e| figcraft_engine::account::clean_email(e).is_some()),
            Step::Experience => !self.experience.is_empty(),
            Step::Plan => !self.plan.is_empty(),
            Step::Create => !self.create.is_empty(),
        }
    }
}

/// What the person did this frame.
enum Act {
    None,
    Back,
    Next,
    Skip,
}

/// Draw the onboarding over everything when the signed-in account hasn't been through it.
pub fn show(app: &mut FigApp, ctx: &Context) {
    if !app.session.account.needs_onboarding() {
        app.ui.onboarding = None;
        return;
    }
    let mut flow = match app.ui.onboarding.take() {
        Some(f) => f,
        None => Flow::new(app),
    };
    let screen = ctx.content_rect();
    let mut act = Act::None;
    egui::Area::new(Id::unique("onboarding")).order(Order::Foreground).fixed_pos(screen.min).show(ctx, |ui| {
        ui.set_min_size(screen.size());
        // Nothing underneath takes the pointer.
        ui.interact(screen, Id::unique("onboarding_backdrop"), Sense::click_and_drag());
        act = draw(app, ui, &mut flow, screen);
    });
    if std::mem::take(&mut flow.copied) {
        app.toast(tr("Link copied: anyone with it can sign up on this server"));
    }
    let enter = ctx.input(|i| i.key_pressed(egui::Key::Enter));
    if enter && flow.ready() && matches!(act, Act::None) {
        act = Act::Next;
    }
    match act {
        Act::None => {}
        Act::Back => flow.go(-1),
        Act::Next => {
            if !submit(app, &mut flow, ctx) {
                app.ui.onboarding = Some(flow);
                return;
            }
            if flow.step == Step::Create {
                finish(app, &flow.create);
                return;
            }
            flow.go(1);
        }
        Act::Skip => {
            if flow.step == Step::Create {
                finish(app, "");
                return;
            }
            flow.go(1);
        }
    }
    app.ui.onboarding = Some(flow);
}

/// Keep this step's answer; false when it was refused (the app says why).
fn submit(app: &mut FigApp, flow: &mut Flow, ctx: &Context) -> bool {
    let params = match flow.step {
        Step::Name => json!({"name": flow.name.trim(), "tips": flow.tips}),
        Step::Role => json!({"role": flow.role, "roleOther": if flow.role == "other" { flow.role_other.trim() } else { "" }}),
        Step::Invite => {
            let emails = flow.emails();
            if emails.is_empty() {
                return true;
            }
            let Some(r) = app.run("onboarding.invite", json!({"emails": emails})) else { return false };
            if let Some(link) = r.get("link").and_then(|l| l.as_str()).filter(|l| !l.is_empty()) {
                ctx.copy_text(link.to_string());
            }
            app.toast(tr("Invites saved. Email isn't set up on this server yet, so the sign-up link was copied: send it to them."));
            return true;
        }
        Step::Experience => json!({"experience": flow.experience}),
        Step::Plan => json!({"plan": flow.plan}),
        Step::Create => json!({"createFirst": flow.create}),
    };
    app.run("onboarding.answer", params).is_some()
}

fn finish(app: &mut FigApp, create: &str) {
    if app.run("onboarding.finish", json!({"createFirst": create})).is_none() {
        return;
    }
    app.ui.onboarding = None;
    if !create.is_empty() {
        app.ui.home = false;
        if !matches!(create, "whiteboard" | "presentation") {
            app.ui.tour = Some(0);
        }
    }
}

/// The design's light-blue preview side and its colours in the dark theme.
fn side_bg(t: &Tokens) -> Color32 {
    t.selected_row
}

fn draw(app: &FigApp, ui: &mut Ui, flow: &mut Flow, screen: Rect) -> Act {
    let t = Tokens::get(ui.ctx());
    let p = ui.painter().clone();
    p.rect_filled(screen, CornerRadius::ZERO, t.panel);
    let wide = flow.step == Step::Plan;
    let split = !wide && screen.width() >= 900.0;
    let left = if split { Rect::from_min_max(screen.min, pos2(screen.min.x + (screen.width() * 0.382).max(640.0), screen.max.y)) } else { screen };
    if split {
        let right = Rect::from_min_max(pos2(left.max.x, screen.min.y), screen.max);
        p.rect_filled(right, CornerRadius::ZERO, side_bg(&t));
        preview(&ui.painter_at(right), flow, right);
    }
    let column = if wide { 906.0_f32.min(screen.width() - 48.0) } else { 512.0_f32.min(left.width() - 48.0) };
    let x = left.center().x - column / 2.0;
    let mut act = Act::None;

    // Top bar: the app's mark on the first step, then a way back.
    let top = Rect::from_min_size(pos2(left.min.x + 24.0, screen.min.y + 20.0), vec2(24.0, 24.0));
    if flow.step != Step::Name {
        let back = ui.interact(top.expand(4.0), Id::unique("onboarding_back"), Sense::click());
        crate::a11y::button(&back, tr("Back"));
        if back.hovered() {
            p.rect_filled(top.expand(4.0), CornerRadius::same(5), t.hover);
        }
        icons::paint(&p, top.shrink(2.0), Icon::ChevronLeft, t.text);
        if back.clicked() {
            act = Act::Back;
        }
    }

    let mut y = screen.min.y + if wide { 80.0 } else { (screen.height() * 0.21).max(96.0) };
    let (title, sub) = match flow.step {
        Step::Name => (tr("What's your name?"), tr("This is what others see when you join files, send invites, and more.")),
        Step::Role => (tr("Which role best describes you?"), tr("If you're multi-talented, pick what you do most often.")),
        Step::Invite => (tr("Will anyone else be joining you?"), tr("You can invite others to design, prototype and build things with you.")),
        Step::Experience => (tr("Have you used FigCraft before?"), ""),
        Step::Plan => (tr("Which plan would you like?"), ""),
        Step::Create => (tr("What do you want to create first?"), ""),
    };
    let heading = p.layout(title.to_string(), regular(32.0), t.text, column);
    let hx = if wide { screen.center().x - heading.size().x / 2.0 } else { x };
    p.galley(pos2(hx, y), heading.clone(), t.text);
    y += heading.size().y + 16.0;
    if !sub.is_empty() {
        let g = p.layout(sub.to_string(), regular(16.0), t.text_secondary, column);
        p.galley(pos2(x, y), g.clone(), t.text_secondary);
        y += g.size().y;
    }
    y += 40.0;

    match flow.step {
        Step::Name => name_step(ui, &t, flow, x, y, column),
        Step::Role => role_step(ui, &t, flow, x, y, column),
        Step::Invite => invite_step(app, ui, &t, flow, x, y, column),
        Step::Experience => experience_step(ui, &t, flow, x, y),
        Step::Plan => plans(ui, &t, flow, Rect::from_min_size(pos2(x, y), vec2(column, 440.0))),
        Step::Create => create_step(ui, &t, flow, x, y, column),
    }

    // Progress and the buttons.
    let bottom = screen.max.y - 64.0 - 48.0;
    let bar = Rect::from_min_size(pos2(x, bottom + 21.0), vec2(128.0, 6.0));
    p.rect_filled(bar, CornerRadius::same(3), t.border);
    let done = (flow.index() + 1) as f32 / (STEPS.len() + 1) as f32;
    p.rect_filled(Rect::from_min_size(bar.min, vec2(128.0 * done, 6.0)), CornerRadius::same(3), t.accent);
    let ready = flow.ready();
    let label = if flow.step == Step::Invite && !flow.emails().is_empty() { tr("Send invites") } else { tr("Continue") };
    let cw = p.layout_no_wrap(label.to_string(), semibold(15.0), t.text).size().x + 48.0;
    let cr = Rect::from_min_size(pos2(x + column - cw, bottom), vec2(cw, 48.0));
    if big_button(ui, &t, cr, label, true, ready, "onboarding_continue").clicked() {
        act = Act::Next;
    }
    if matches!(flow.step, Step::Invite | Step::Create) {
        let sw = p.layout_no_wrap(tr("Skip").to_string(), semibold(13.0), t.text).size().x + 32.0;
        let sr = Rect::from_min_size(pos2(cr.min.x - sw - 8.0, bottom), vec2(sw, 48.0));
        if big_button(ui, &t, sr, tr("Skip"), false, true, "onboarding_skip").clicked() {
            act = Act::Skip;
        }
    }
    act
}

fn name_step(ui: &mut Ui, t: &Tokens, flow: &mut Flow, x: f32, mut y: f32, column: f32) {
    let p = ui.painter().clone();
    text_box(ui, t, Rect::from_min_size(pos2(x, y), vec2(column, 50.0)), &mut flow.name, tr("Your name"), "onboarding_name", true);
    y += 50.0 + 48.0;
    let box_r = Rect::from_min_size(pos2(x, y + 4.0), vec2(24.0, 24.0));
    let label = tr("Subscribe to FigCraft tips and updates");
    let row = Rect::from_min_size(pos2(x, y), vec2(column.min(360.0), 32.0));
    let resp = ui.interact(row, Id::unique("onboarding_tips"), Sense::click());
    crate::a11y::checkbox(&resp, label, flow.tips);
    if resp.clicked() {
        flow.tips = !flow.tips;
    }
    if flow.tips {
        p.rect_filled(box_r, CornerRadius::same(6), t.accent);
        icons::paint(&p, box_r.shrink(4.0), Icon::Check, t.accent_on);
    } else {
        p.rect_stroke(box_r, CornerRadius::same(6), Stroke::new(2.0, if resp.hovered() { t.border_strong } else { t.border }), StrokeKind::Inside);
    }
    p.text(pos2(x + 36.0, y + 16.0), Align2::LEFT_CENTER, label, semibold(13.0), t.text);
    let note = p.layout(
        tr("By opting in, you agree to receive news about FigCraft: new features, tips and events. You can unsubscribe at any time.").to_string(),
        regular(11.0),
        t.text_secondary,
        column.min(400.0),
    );
    p.galley(pos2(x, y + 40.0), note, t.text_secondary);
}

fn role_step(ui: &mut Ui, t: &Tokens, flow: &mut Flow, x: f32, mut y: f32, column: f32) {
    let p = ui.painter().clone();
    let mut cx = x;
    for (id, label) in ROLES {
        let w = chip_width(&p, tr(label));
        if cx + w > x + column + 0.5 {
            cx = x;
            y += 40.0;
        }
        let r = Rect::from_min_size(pos2(cx, y), vec2(w, 32.0));
        if chip(ui, t, r, tr(label), flow.role == id, ("role", id)).clicked() {
            flow.role = id.to_string();
        }
        cx += w + 8.0;
    }
    y += 56.0;
    if flow.role == "other" {
        text_box(
            ui,
            t,
            Rect::from_min_size(pos2(x, y), vec2(column, 50.0)),
            &mut flow.role_other,
            tr("Ex: Art direction"),
            "onboarding_role_other",
            true,
        );
        flow.role_other = flow.role_other.chars().take(60).collect();
    }
}

fn invite_step(app: &FigApp, ui: &mut Ui, t: &Tokens, flow: &mut Flow, x: f32, mut y: f32, column: f32) {
    let p = ui.painter().clone();
    let hints = ["sneha@example.com", "oscar@example.com", "molly@example.com"];
    for (i, hint) in hints.iter().enumerate() {
        if let Some(e) = flow.invites.get_mut(i) {
            text_box(ui, t, Rect::from_min_size(pos2(x, y), vec2(column, 48.0)), e, hint, ("onboarding_invite", i), i == 0);
        }
        y += 56.0;
    }
    let bad = flow.emails().into_iter().find(|e| figcraft_engine::account::clean_email(e).is_none());
    if let Some(b) = bad {
        p.text(pos2(x, y), Align2::LEFT_TOP, format!("{b}: {}", tr("that isn't an email address")), regular(12.0), t.danger);
        y += 20.0;
    }
    let link = app.session.account.account.as_ref().map(|a| a.invite_link.clone()).unwrap_or_default();
    let label = tr("Copy link to invite");
    let g = p.layout_no_wrap(label.to_string(), semibold(13.0), t.accent_text);
    let lr = Rect::from_center_size(pos2(x + column / 2.0 + 10.0, y + 14.0), vec2(g.size().x + 20.0, 24.0));
    let resp = ui.interact(lr, Id::unique("onboarding_copy_link"), Sense::click());
    crate::a11y::button(&resp, label);
    icons::paint(&p, Rect::from_center_size(pos2(lr.min.x + 6.0, lr.center().y), vec2(14.0, 14.0)), Icon::Link, t.accent_text);
    p.galley(pos2(lr.min.x + 18.0, lr.center().y - g.size().y / 2.0), g, t.accent_text);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if resp.clicked() && !link.is_empty() {
        ui.ctx().copy_text(link);
        flow.copied = true;
    }
}

fn experience_step(ui: &mut Ui, t: &Tokens, flow: &mut Flow, x: f32, y: f32) {
    let p = ui.painter().clone();
    let mut cx = x;
    for (id, label) in EXPERIENCE {
        let w = chip_width(&p, tr(label)) + 8.0;
        let r = Rect::from_min_size(pos2(cx, y), vec2(w, 40.0));
        if chip(ui, t, r, tr(label), flow.experience == id, ("experience", id)).clicked() {
            flow.experience = id.to_string();
        }
        cx += w + 8.0;
    }
}

fn create_step(ui: &mut Ui, t: &Tokens, flow: &mut Flow, x: f32, y: f32, column: f32) {
    let p = ui.painter().clone();
    let gap = 8.0;
    let w = (column - 2.0 * gap) / 3.0;
    for (i, (id, label, icon)) in CREATE.iter().enumerate() {
        let r = Rect::from_min_size(pos2(x + (i % 3) as f32 * (w + gap), y + (i / 3) as f32 * (98.0 + gap)), vec2(w, 98.0));
        let resp = ui.interact(r, Id::unique(("onboarding_create", *id)), Sense::click());
        crate::a11y::radio(&resp, tr(label), flow.create == *id);
        let on = flow.create == *id;
        p.rect_filled(
            r,
            CornerRadius::same(8),
            if on {
                t.selected_row
            } else if resp.hovered() {
                t.hover
            } else {
                t.panel
            },
        );
        p.rect_stroke(r, CornerRadius::same(8), Stroke::new(1.0, if on { t.accent } else { t.border }), StrokeKind::Inside);
        icons::paint(&p, Rect::from_center_size(r.center() - vec2(0.0, 12.0), vec2(16.0, 16.0)), *icon, t.text);
        p.text(r.center() + vec2(0.0, 14.0), Align2::CENTER_CENTER, tr(label), semibold(11.0), t.text);
        if resp.clicked() {
            flow.create = id.to_string();
        }
    }
}

fn chip_width(p: &egui::Painter, label: &str) -> f32 {
    p.layout_no_wrap(label.to_string(), regular(13.0), Color32::WHITE).size().x + 16.0
}

fn chip(ui: &mut Ui, t: &Tokens, r: Rect, label: &str, on: bool, id: impl std::hash::Hash + std::fmt::Debug) -> Response {
    let resp = ui.interact(r, Id::unique(("onboarding_chip", id)), Sense::click());
    crate::a11y::radio(&resp, label, on);
    let p = ui.painter();
    p.rect_filled(
        r,
        CornerRadius::same(5),
        if on {
            t.selected_row
        } else if resp.hovered() {
            t.hover
        } else {
            t.panel
        },
    );
    p.rect_stroke(r, CornerRadius::same(5), Stroke::new(1.0, if on { t.accent } else { t.border }), StrokeKind::Inside);
    p.text(r.center(), Align2::CENTER_CENTER, label, regular(13.0), if on { t.accent_text } else { t.text });
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

fn big_button(ui: &mut Ui, t: &Tokens, r: Rect, label: &str, primary: bool, enabled: bool, id: &str) -> Response {
    let resp = ui.interact(r, Id::unique(id), if enabled { Sense::click() } else { Sense::hover() });
    crate::a11y::button(&resp, label);
    let p = ui.painter();
    let (fill, text) = match (primary, enabled, resp.hovered()) {
        (true, false, _) => (t.field, t.text_tertiary),
        (true, true, false) => (t.accent, t.accent_on),
        (true, true, true) => (t.accent_hover, t.accent_on),
        (false, _, true) => (t.hover, t.text),
        (false, _, false) => (Color32::TRANSPARENT, t.text),
    };
    p.rect_filled(r, CornerRadius::same(5), fill);
    p.text(r.center(), Align2::CENTER_CENTER, label, semibold(if primary { 15.0 } else { 13.0 }), text);
    resp
}

/// A large text field (16 px), with a border that turns blue while typing.
fn text_box(ui: &mut Ui, t: &Tokens, r: Rect, text: &mut String, hint: &str, id: impl std::hash::Hash + std::fmt::Debug, focus_first: bool) {
    let id = Id::unique(("onboarding_field", id));
    let edit = egui::TextEdit::singleline(text)
        .id(id)
        .font(FontId::proportional(16.0))
        .frame(egui::Frame::NONE)
        .hint_text(egui::RichText::new(hint).size(16.0).color(t.text_tertiary))
        .text_color(t.text)
        .desired_width(r.width() - 32.0)
        .margin(egui::Margin::ZERO)
        .char_limit(254);
    let line = ui.painter().layout_no_wrap("Ag".into(), FontId::proportional(16.0), t.text).size().y;
    let inner = Rect::from_center_size(r.center(), vec2(r.width() - 32.0, line));
    let resp = ui.put(inner, edit);
    if focus_first && !ui.ctx().memory(|m| m.focused().is_some()) {
        resp.request_focus();
    }
    let border = if resp.has_focus() { t.accent } else { t.border };
    ui.painter().rect_stroke(r, CornerRadius::same(6), Stroke::new(1.0, border), StrokeKind::Inside);
}

fn plans(ui: &mut Ui, t: &Tokens, flow: &mut Flow, area: Rect) {
    let p = ui.painter().clone();
    p.rect_stroke(area, CornerRadius::same(8), Stroke::new(1.0, t.border), StrokeKind::Inside);
    let w = area.width() / PLANS.len() as f32;
    for (i, plan) in PLANS.iter().enumerate() {
        let r = Rect::from_min_size(pos2(area.min.x + i as f32 * w, area.min.y), vec2(w, area.height()));
        if i > 0 {
            p.line_segment([r.left_top(), r.left_bottom()], Stroke::new(1.0, t.border));
        }
        let resp = ui.interact(r, Id::unique(("onboarding_plan", plan.id)), Sense::click());
        let on = flow.plan == plan.id;
        crate::a11y::radio(&resp, tr(plan.name), on);
        if on || resp.hovered() {
            p.rect_filled(r.shrink(1.0), CornerRadius::same(7), if on { t.selected_row } else { t.hover });
        }
        if resp.clicked() {
            flow.plan = plan.id.to_string();
        }
        let x = r.min.x + 12.0;
        let inner = w - 24.0;
        let mut y = r.min.y + 16.0;
        let name = p.text(pos2(x, y), Align2::LEFT_TOP, tr(plan.name), semibold(13.0), t.text);
        if !plan.badge.is_empty() {
            let g = p.layout_no_wrap(tr(plan.badge).to_string(), regular(10.0), t.accent_text);
            let b = Rect::from_min_size(pos2(name.max.x + 6.0, name.center().y - 8.0), vec2(g.size().x + 8.0, 16.0));
            p.rect_filled(b, CornerRadius::same(3), t.selected_row);
            p.galley(pos2(b.min.x + 4.0, b.center().y - g.size().y / 2.0), g, t.accent_text);
        }
        let dot = pos2(r.max.x - 18.0, name.center().y);
        p.circle_stroke(dot, 7.0, Stroke::new(1.0, if on { t.accent } else { t.border_strong }));
        if on {
            p.circle_filled(dot, 4.0, t.accent);
        }
        y = name.max.y + 6.0;
        let about = p.layout(tr(plan.about).to_string(), regular(11.0), t.text_secondary, inner - 24.0);
        p.galley(pos2(x, y), about, t.text_secondary);
        // Two lines' room, so the prices line up across the cards.
        y += 32.0 + 24.0;
        p.text(pos2(x, y), Align2::LEFT_TOP, tr(plan.price), semibold(16.0), t.text);
        y += 40.0;
        p.line_segment([pos2(x, y), pos2(x + inner, y)], Stroke::new(1.0, t.border));
        y += 12.0;
        p.text(pos2(x, y), Align2::LEFT_TOP, tr(plan.heading), semibold(11.0), t.text);
        y += 22.0;
        for f in plan.features {
            icons::paint(&p, Rect::from_min_size(pos2(x, y), vec2(12.0, 12.0)), Icon::Check, t.text_secondary);
            let g = p.layout(tr(f).to_string(), regular(11.0), t.text, inner - 20.0);
            let h = g.size().y;
            p.galley(pos2(x + 18.0, y - 1.0), g, t.text);
            y += h + 8.0;
        }
    }
}

// The preview on the right: a file with your cursor in it, and a picture for your role.

const CURSOR_RED: Color32 = Color32::from_rgb(0xf2, 0x48, 0x22);
const OTHERS: [Color32; 3] = [Color32::from_rgb(0x14, 0xae, 0x5c), Color32::from_rgb(0x97, 0x47, 0xff), Color32::from_rgb(0xff, 0x24, 0xbd)];
const SHAPE_GREY: Color32 = Color32::from_rgb(0xe6, 0xe6, 0xe6);

fn preview(p: &egui::Painter, flow: &Flow, right: Rect) {
    let pad = (right.width() * 0.093).max(48.0);
    let w = right.width() - 2.0 * pad;
    let h = (w * 0.65).min(right.height() * 0.62);
    let card = Rect::from_min_size(pos2(right.min.x + pad, right.center().y - h / 2.0 + right.height() * 0.04), vec2(w, h));
    let s = w / 1034.0;
    for (grow, alpha) in [(12.0, 4), (8.0, 6), (4.0, 10), (1.0, 22)] {
        p.rect_filled(card.expand(grow).translate(vec2(0.0, grow * 0.4)), CornerRadius::same((12.0 + grow) as u8), Color32::from_black_alpha(alpha));
    }
    let white = Color32::WHITE;
    p.rect_filled(card, CornerRadius::same(12), white);
    // Title bar: the app's mark, the file's name, your avatar.
    let name = flow.name.trim();
    let who = if name.is_empty() { tr("You") } else { name };
    let file = match flow.create.as_str() {
        "" => format!("{who}'s New File"),
        c => figcraft_engine::account::first_file_name(who, c),
    };
    icons::paint(p, Rect::from_min_size(card.min + vec2(12.0, 14.0), vec2(20.0, 20.0)), Icon::Logo, Color32::from_gray(60));
    p.text(pos2(card.center().x, card.min.y + 24.0), Align2::CENTER_CENTER, file, regular(14.0 * s.max(0.8)), Color32::from_black_alpha(230));
    let avatar = pos2(card.max.x - 24.0, card.min.y + 24.0);
    p.circle_filled(avatar, 12.0, OTHERS[1]);
    let initial: String = who.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
    p.text(avatar, Align2::CENTER_CENTER, initial, regular(13.0), white);

    let body = ui_body(card);
    let inside = p.with_clip_rect(body.intersect(p.clip_rect()));
    if flow.step == Step::Role {
        role_picture(&inside, &flow.role, body, s);
    }
    if flow.step == Step::Invite {
        let names: Vec<String> = flow
            .invites
            .iter()
            .zip(["sneha", "oscar", "molly"])
            .map(|(e, hint)| {
                let local = e.trim().split('@').next().unwrap_or_default().to_string();
                if local.is_empty() { hint.to_string() } else { local }
            })
            .collect();
        let spots = [(0.14, 0.42), (0.55, 0.57), (0.33, 0.73)];
        for (i, n) in names.iter().enumerate() {
            let typed = flow.invites.get(i).is_some_and(|e| !e.trim().is_empty());
            let (fx, fy) = spots.get(i).copied().unwrap_or((0.5, 0.5));
            let c = OTHERS.get(i).copied().unwrap_or(OTHERS[0]);
            cursor(&inside, pos2(card.min.x + w * fx, card.min.y + h * fy), n, if typed { c } else { c.gamma_multiply(0.55) }, s);
        }
    }
    cursor(&inside, pos2(card.min.x + w * 0.39, card.min.y + h * 0.25), who, CURSOR_RED, s);
}

/// The file's canvas below its title bar.
fn ui_body(card: Rect) -> Rect {
    Rect::from_min_max(pos2(card.min.x, card.min.y + 48.0), card.max)
}

/// A cursor arrow and its name tag, at the arrow's tip.
fn cursor(p: &egui::Painter, at: Pos2, name: &str, color: Color32, s: f32) {
    let k = s.max(0.55);
    let pt = |x: f32, y: f32| at + vec2(x * k, y * k);
    // Tip, tail, notch and wing, from the tip.
    let (tip, tail, notch, wing) = (pt(0.0, 0.0), pt(5.6, 29.25), pt(12.4, 18.0), pt(24.75, 14.6));
    let edge = Stroke::new(2.25 * k, Color32::WHITE);
    let shadow = |q: Pos2| q + vec2(0.0, 1.0 * k);
    for (a, b, c) in [(tip, tail, notch), (tip, notch, wing)] {
        p.add(Shape::convex_polygon(vec![shadow(a), shadow(b), shadow(c)], Color32::from_black_alpha(45), Stroke::NONE));
    }
    p.add(Shape::closed_line(vec![tip, tail, notch, wing], edge));
    for (a, b, c) in [(tip, tail, notch), (tip, notch, wing)] {
        p.add(Shape::convex_polygon(vec![a, b, c], color, Stroke::NONE));
    }
    let font = FontId::proportional(28.0 * k);
    let g = p.layout_no_wrap(name.to_string(), font, Color32::WHITE);
    let tag = Rect::from_min_size(pt(24.0, 38.0), vec2(g.size().x + 18.0 * k, (46.0 * k).max(g.size().y + 6.0 * k)));
    p.rect_filled(tag.translate(vec2(0.0, 2.25 * k)), CornerRadius::ZERO, Color32::from_black_alpha(38));
    p.rect_filled(tag, CornerRadius::ZERO, color);
    p.galley(pos2(tag.min.x + 9.0 * k, tag.center().y - g.size().y / 2.0), g, Color32::WHITE);
}

/// A picture of the kind of work a role does, from plain grey shapes.
fn role_picture(p: &egui::Painter, role: &str, body: Rect, s: f32) {
    let o = body.min;
    let at = |x: f32, y: f32| o + vec2(x * s, y * s);
    let size = |w: f32, h: f32| vec2(w * s, h * s);
    let line = Stroke::new(3.0 * s.max(0.5), SHAPE_GREY);
    let r = |x: f32, y: f32, w: f32, h: f32| Rect::from_min_size(at(x, y), size(w, h));
    match role {
        "design" => {
            for (i, big) in [96.0, 64.0, 44.0].iter().enumerate() {
                p.text(at(40.0, 40.0 + i as f32 * 110.0), Align2::LEFT_TOP, "Aa", FontId::proportional(big * s), SHAPE_GREY);
            }
            let card = r(330.0, 220.0, 200.0, 220.0);
            p.extend(Shape::dashed_line(
                &[card.left_top(), card.right_top(), card.right_bottom(), card.left_bottom(), card.left_top()],
                line,
                10.0 * s,
                6.0 * s,
            ));
            for (i, on) in [true, false].iter().enumerate() {
                let tr_ = r(360.0, 260.0 + i as f32 * 80.0, 140.0, 48.0);
                p.rect_stroke(tr_, CornerRadius::same((24.0 * s) as u8), line, StrokeKind::Inside);
                let knob = if *on { tr_.right_center() - vec2(24.0 * s, 0.0) } else { tr_.left_center() + vec2(24.0 * s, 0.0) };
                p.circle_filled(knob, 16.0 * s, SHAPE_GREY);
            }
            let frame = r(720.0, 30.0, 330.0, 300.0);
            p.rect_stroke(frame, CornerRadius::ZERO, line, StrokeKind::Inside);
            for c in [frame.left_top(), frame.left_bottom()] {
                p.rect_filled(Rect::from_center_size(c, size(16.0, 16.0)), CornerRadius::ZERO, Color32::WHITE);
                p.rect_stroke(Rect::from_center_size(c, size(16.0, 16.0)), CornerRadius::ZERO, Stroke::new(2.0, SHAPE_GREY), StrokeKind::Inside);
            }
            for (cx, cy) in [(800.0, 120.0), (900.0, 120.0), (800.0, 220.0), (900.0, 220.0)] {
                p.circle_filled(at(cx, cy), 36.0 * s, SHAPE_GREY);
            }
        }
        "research" | "ux-writing" => {
            for i in 0..6 {
                let (cx, cy) = ((i % 3) as f32, (i / 3) as f32);
                p.rect_filled(r(80.0 + cx * 170.0, 60.0 + cy * 170.0, 140.0, 140.0), CornerRadius::same(4), SHAPE_GREY);
            }
            for (i, len) in [360.0, 300.0, 340.0, 220.0, 280.0].iter().enumerate() {
                p.rect_filled(r(640.0, 80.0 + i as f32 * 48.0, *len, 18.0), CornerRadius::same(9), SHAPE_GREY);
            }
        }
        "student" | "educator" => {
            p.circle_stroke(at(200.0, 220.0), 110.0 * s, line);
            p.add(Shape::closed_line(vec![at(480.0, 330.0), at(580.0, 120.0), at(680.0, 330.0)], line));
            p.rect_stroke(r(760.0, 120.0, 210.0, 210.0), CornerRadius::ZERO, line, StrokeKind::Inside);
            for i in 0..12 {
                let x = 80.0 + i as f32 * 74.0;
                p.line_segment([at(x, 430.0), at(x, if i % 2 == 0 { 470.0 } else { 452.0 })], line);
            }
        }
        "data-analytics" => {
            for (i, hgt) in [140.0, 240.0, 190.0, 320.0, 260.0, 380.0].iter().enumerate() {
                p.rect_filled(r(90.0 + i as f32 * 90.0, 470.0 - hgt, 56.0, *hgt), CornerRadius::same(4), SHAPE_GREY);
            }
            let pts: Vec<Pos2> =
                [(660.0, 380.0), (740.0, 300.0), (820.0, 330.0), (900.0, 200.0), (980.0, 150.0)].iter().map(|(x, y)| at(*x, *y)).collect();
            p.add(Shape::line(pts.clone(), line));
            for q in pts {
                p.circle_filled(q, 9.0 * s, SHAPE_GREY);
            }
        }
        "software-development" => {
            p.text(at(70.0, 60.0), Align2::LEFT_TOP, "</>", FontId::monospace(110.0 * s), SHAPE_GREY);
            for (i, (indent, len)) in [(0.0, 300.0), (40.0, 360.0), (80.0, 240.0), (80.0, 320.0), (40.0, 200.0), (0.0, 120.0)].iter().enumerate() {
                p.rect_filled(r(460.0 + indent, 70.0 + i as f32 * 56.0, *len, 22.0), CornerRadius::same(11), SHAPE_GREY);
            }
        }
        "marketing" | "product-management" => {
            for col in 0..3 {
                let x = 80.0 + col as f32 * 310.0;
                p.rect_filled(r(x, 50.0, 260.0, 26.0), CornerRadius::same(13), SHAPE_GREY);
                for row in 0..(3 - col % 2) {
                    p.rect_stroke(r(x, 100.0 + row as f32 * 120.0, 260.0, 100.0), CornerRadius::same(8), line, StrokeKind::Inside);
                }
            }
        }
        "other" => {
            for i in 0..14 {
                for j in 0..7 {
                    p.circle_filled(at(70.0 + i as f32 * 70.0, 50.0 + j as f32 * 70.0), 5.0 * s, SHAPE_GREY);
                }
            }
        }
        _ => {}
    }
}
