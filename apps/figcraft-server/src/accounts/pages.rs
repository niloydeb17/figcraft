//! The sign-in pages: plain HTML forms with no scripts, filled in here.

use super::User;

pub const HTML: &str = include_str!("login.html");
pub const CSS: &str = include_str!("login.css");
pub const ART: &str = include_str!("login-art.svg");

/// Where the help line and the terms point.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Site {
    /// How to reach the operator: a web address, or an email address (`mailto:`).
    pub contact: String,
    /// The terms of use, when there are some.
    pub terms: String,
}

/// What the sign-in form shows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Form<'a> {
    pub error: &'a str,
    pub email: &'a str,
    /// Where to go once signed in (a path on this site).
    pub next: &'a str,
    pub google: bool,
    pub signup: bool,
}

/// Text made safe to put in HTML (also inside attribute values).
pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

fn help(site: &Site) -> String {
    let c = site.contact.trim();
    if c.starts_with("https://") || c.starts_with("http://") {
        c.to_string()
    } else if super::clean_email(c).is_some() {
        format!("mailto:{c}")
    } else {
        "https://github.com/storytold/figcraft/issues".to_string()
    }
}

fn page(title: &str, card: &str, site: &Site) -> String {
    HTML.replace("{{title}}", &escape(title)).replace("{{help}}", &escape(&help(site))).replace("{{card}}", card)
}

/// The sign-in (and sign-up) page.
pub fn login(form: &Form, site: &Site) -> String {
    let next = escape(form.next);
    let mut card = String::new();
    card.push_str("        <h1>Welcome to FigCraft</h1>\n");
    card.push_str(if form.signup { "        <p class=\"login-sub\">Log in or sign up</p>\n" } else { "        <p class=\"login-sub\">Log in</p>\n" });
    if form.google {
        card.push_str(&format!(
            "        <a class=\"google-button\" href=\"/auth/google?next={}\">Continue with Google</a>\n        <p class=\"login-or\">or</p>\n",
            escape(&form_urlencoded::byte_serialize(form.next.as_bytes()).collect::<String>())
        ));
    }
    card.push_str("        <form action=\"/login\" method=\"post\">\n");
    if !form.error.is_empty() {
        card.push_str(&format!("          <p class=\"login-error\" role=\"alert\">{}</p>\n", escape(form.error)));
    }
    card.push_str(&format!("          <input type=\"hidden\" name=\"next\" value=\"{next}\">\n"));
    card.push_str("          <label for=\"email\">Email</label>\n");
    card.push_str(&format!(
        "          <input id=\"email\" name=\"email\" type=\"email\" placeholder=\"you@example.com\" autocomplete=\"email\" required maxlength=\"254\" value=\"{}\"{}>\n",
        escape(form.email),
        if form.email.is_empty() { " autofocus" } else { "" }
    ));
    card.push_str("          <label for=\"password\">Password</label>\n");
    card.push_str(&format!(
        "          <input id=\"password\" name=\"password\" type=\"password\" placeholder=\"Enter your password\" autocomplete=\"current-password\" required minlength=\"{}\" maxlength=\"{}\"{}>\n",
        super::MIN_PASSWORD,
        super::MAX_PASSWORD,
        if form.email.is_empty() { "" } else { " autofocus" }
    ));
    if form.signup {
        card.push_str("          <p class=\"login-note\">New here? Pick a password of at least 8 characters and we'll make your account.</p>\n");
    }
    card.push_str("          <button type=\"submit\">Continue</button>\n        </form>\n");
    if !site.terms.trim().is_empty() {
        card.push_str(&format!(
            "        <p class=\"login-legal\">By clicking continue, you agree to the <a href=\"{}\" target=\"_blank\" rel=\"noopener noreferrer\">terms of use</a>.</p>\n",
            escape(site.terms.trim())
        ));
    }
    page("Log in to FigCraft", &card, site)
}

/// The page a signed-in person sees: who they are, a way into the app and a way out.
pub fn account(user: &User, open: &str, site: &Site) -> String {
    let card = format!(
        "        <h1>You're signed in</h1>\n        <p class=\"login-sub\">&nbsp;</p>\n        <p class=\"login-account\"><b>{}</b>{}</p>\n        <a class=\"login-open\" href=\"{}\">Open FigCraft</a>\n        <form class=\"login-out\" action=\"/logout\" method=\"post\"><button type=\"submit\">Log out</button></form>\n",
        escape(&user.name),
        escape(&user.email),
        escape(open)
    );
    page("Your FigCraft account", &card, site)
}

/// A page with only a message (a sign-in that couldn't finish).
pub fn message(title: &str, text: &str, site: &Site) -> String {
    let card = format!(
        "        <h1>{}</h1>\n        <p class=\"login-sub\">{}</p>\n        <a class=\"login-open\" href=\"/login\">Back to log in</a>\n",
        escape(title),
        escape(text)
    );
    page(title, &card, site)
}
