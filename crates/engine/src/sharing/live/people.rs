//! Who has access to a file of yours on a server, as the Share dialog shows it (as in the
//! incumbent): what anyone with the link may do, and the people invited by email.
//!
//! "Anyone with the link" is the share's links ([`Share::links`]): the link copied is the one for
//! its access. Lowering it makes new links (the old ones are revoked), so a link copied earlier
//! never keeps more access than the file gives now; "no access" revokes them all.
//!
//! Each person invited by email gets a link of their own, labelled with their address
//! ([`INVITED`]): taking their access away revokes only that link. Signed in on the page's server,
//! the link also goes to their account (`/auth/share`): the file shows on their Home.

use super::host::{hosting_request, share_answer, share_request, shared_now};
use super::types::Status;
use super::types::{NetReply, NetRequest};
use super::{Access, Admin, AdminReply, EngineError, Result, Session, Value, bad, clean_shared, get, json, string};
use crate::account::{Outgoing, clean_email};
use crate::spec;
use figcraft_collab::message::LinkInfo;

/// The label of an invited person's link: this, then their email.
pub const INVITED: &str = "Invited: ";
/// Most people one file is shared with by email, and per invite.
pub const MAX_INVITED: usize = 50;
pub const INVITES_AT_ONCE: usize = 20;

/// Why a command here can't change the open file.
const NOT_OWNED: &str = "only the owner of a file on a server can change who has access: share the open file online first";

/// The server, file and owner token of the open file, when it is yours on a server and live.
fn owned(s: &Session, cmd: &str) -> Result<(String, String, String)> {
    let live = s.live();
    if live.status != Status::Live {
        return Err(bad(cmd, NOT_OWNED));
    }
    let (server, file) = live.server_file().ok_or_else(|| bad(cmd, NOT_OWNED))?;
    let owner = live.published.as_ref().and_then(|p| p.owner.clone()).or_else(|| s.online_owner(&server, &file)).filter(|o| !o.is_empty());
    Ok((server, file, owner.ok_or_else(|| bad(cmd, NOT_OWNED))?))
}

/// The open file is yours on a server (the Share dialog lets you change who has access).
pub fn owns_open_file(s: &Session) -> bool {
    owned(s, "").is_ok()
}

/// One request about the open file to its server, with its owner token.
fn admin(s: &mut Session, cmd: &str, (server, file, owner): &(String, String, String), req: Admin) -> Result<AdminReply> {
    hosting_request(s, cmd, server, file, owner, Some(req))?.1.ok_or_else(|| EngineError::Other(format!("{cmd}: the server answered nothing")))
}

/// The file's links.
fn links(s: &mut Session, cmd: &str, at: &(String, String, String)) -> Result<Vec<LinkInfo>> {
    match admin(s, cmd, at, Admin::Links { file: at.1.clone() })? {
        AdminReply::Links(l) => Ok(l),
        _ => Err(bad(cmd, "the server answered something else")),
    }
}

/// The email an invited person's link is labelled with.
fn invited(l: &LinkInfo) -> Option<&str> {
    l.label.strip_prefix(INVITED).map(str::trim).filter(|e| !e.is_empty())
}

/// The people invited, one each (their newest link), in the order they were invited.
fn people(links: &[LinkInfo]) -> Vec<(String, Access, String)> {
    let mut out: Vec<(String, Access, String, u64)> = Vec::new();
    for l in links {
        let Some(email) = invited(l) else { continue };
        match out.iter_mut().find(|p| p.0 == email) {
            Some(p) if l.created >= p.3 => *p = (email.to_string(), l.access, l.id.clone(), l.created),
            Some(_) => {}
            None => out.push((email.to_string(), l.access, l.id.clone(), l.created)),
        }
    }
    out.into_iter().map(|(e, a, id, _)| (e, a, id)).collect()
}

/// A level as the dialog says it: `edit`, `comment`, `view` or `none`.
fn level(cmd: &str, p: &Value, extra: &str) -> Result<Option<Access>> {
    match string(p, "access").as_deref().map(str::trim) {
        Some(a) if a == extra => Ok(None),
        Some(a) => Access::parse(a).map(Some).ok_or_else(|| bad(cmd, format!("`access` must be \"edit\", \"comment\", \"view\" or \"{extra}\""))),
        None => Err(bad(cmd, "`access` is missing")),
    }
}

/// How much a level lets people do (no access is 0).
fn rank(a: Option<Access>) -> u8 {
    match a {
        None => 0,
        Some(Access::View) => 1,
        Some(Access::Comment) => 2,
        Some(Access::Edit) => 3,
    }
}

/// What anyone with the link may do now (`None`: nobody gets in with a link).
fn link_level(s: &Session) -> Option<Access> {
    s.live().share.as_ref().filter(|sh| sh.links.is_some()).map(|sh| sh.access)
}

fn access_json(a: Option<Access>) -> &'static str {
    a.map_or("none", Access::id)
}

/// Make each of `emails` a link of their own to the file with `access` (revoking the ones they
/// had in `old`), all on one connection: the links, in order.
fn give(s: &mut Session, cmd: &str, at: &(String, String, String), old: &[LinkInfo], emails: &[String], access: Access) -> Result<Vec<String>> {
    let mut reqs: Vec<Admin> = old
        .iter()
        .filter(|l| invited(l).is_some_and(|e| emails.iter().any(|x| x == e)))
        .map(|l| Admin::Revoke { file: at.1.clone(), link: l.id.clone() })
        .collect();
    let revokes = reqs.len();
    reqs.extend(emails.iter().map(|e| Admin::CreateLink { file: at.1.clone(), access, label: format!("{INVITED}{e}") }));
    let replies = batch(s, cmd, at, reqs)?;
    let mut links = Vec::new();
    for r in replies.into_iter().skip(revokes) {
        match r {
            // The link layer gives the new link whole in place of its token.
            AdminReply::Link(l) => links.push(l.token),
            AdminReply::Failed { reason } => return Err(EngineError::Other(reason)),
            _ => return Err(bad(cmd, "the server answered something else")),
        }
    }
    if links.len() != emails.len() {
        return Err(bad(cmd, "the server made fewer links than asked"));
    }
    Ok(links)
}

/// Requests about the open file, with its owner token, on one connection: the answers in order.
fn batch(s: &mut Session, cmd: &str, (server, file, owner): &(String, String, String), reqs: Vec<Admin>) -> Result<Vec<AdminReply>> {
    let req = NetRequest::Admin { server: server.clone(), file: file.clone(), owner: owner.clone(), reqs };
    match s.net(req)?.0 {
        NetReply::Admin(Ok(r)) => Ok(r),
        NetReply::Admin(Err(why)) => Err(EngineError::Other(why)),
        _ => Err(bad(cmd, "the server answered something else")),
    }
}

/// Queue the links made for the account's server, once every request is answered.
fn deliver(s: &mut Session, given: &[(String, String)]) -> bool {
    if s.account.account.is_none() {
        return false;
    }
    let (name, kind) = s.ds().map(|d| (d.doc.name.clone(), d.doc.kind.id())).unwrap_or_default();
    for (email, link) in given {
        s.account.queue(Outgoing::Share(json!({"emails": [email], "target": link, "name": name, "kind": kind})));
    }
    true
}

fn access(s: &mut Session, _p: &Value) -> Result<Value> {
    const CMD: &str = "collab.access";
    let Ok(at) = owned(s, CMD) else {
        return Ok(json!({"owner": false, "link": access_json(link_level(s)), "people": [], "max": MAX_INVITED}));
    };
    let list = links(s, CMD, &at)?;
    let people: Vec<Value> = people(&list).into_iter().map(|(email, a, id)| json!({"email": email, "access": a.id(), "id": id})).collect();
    Ok(json!({"owner": true, "link": access_json(link_level(s)), "people": people, "max": MAX_INVITED}))
}

fn invite(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "collab.invite";
    let Some(Value::Array(list)) = get(p, "emails") else { return Err(bad(CMD, "`emails` is a list of email addresses")) };
    let access = if get(p, "access").is_some() { level(CMD, p, "edit")?.unwrap_or(Access::Edit) } else { Access::Edit };
    let mut emails: Vec<String> = Vec::new();
    for v in list {
        let t = v.as_str().ok_or_else(|| bad(CMD, "`emails` is a list of email addresses"))?;
        if t.trim().is_empty() {
            continue;
        }
        let e = clean_email(t).ok_or_else(|| bad(CMD, format!("{} isn't an email address", t.trim())))?;
        if !emails.contains(&e) {
            emails.push(e);
        }
    }
    if emails.is_empty() {
        return Err(bad(CMD, "give at least one email address"));
    }
    if emails.len() > INVITES_AT_ONCE {
        return Err(bad(CMD, format!("at most {INVITES_AT_ONCE} people at once")));
    }
    let at = owned(s, CMD)?;
    let old = links(s, CMD, &at)?;
    let had = people(&old);
    let new = emails.iter().filter(|e| !had.iter().any(|h| &h.0 == *e)).count();
    if had.len() + new > MAX_INVITED {
        return Err(bad(CMD, format!("a file is shared with at most {MAX_INVITED} people: remove someone first")));
    }
    let links = give(s, CMD, &at, &old, &emails, access)?;
    let given: Vec<(String, String)> = emails.into_iter().zip(links).collect();
    let delivered = deliver(s, &given);
    let invited: Vec<Value> = given.iter().map(|(e, l)| json!({"email": e, "access": access.id(), "link": l})).collect();
    Ok(json!({"invited": invited, "delivered": delivered}))
}

fn set_access(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "collab.setAccess";
    let email = string(p, "email").and_then(|e| clean_email(&e)).ok_or_else(|| bad(CMD, "`email` is an invited person's address"))?;
    let access = level(CMD, p, "remove")?;
    let at = owned(s, CMD)?;
    let old = links(s, CMD, &at)?;
    if !old.iter().any(|l| invited(l) == Some(email.as_str())) {
        return Err(bad(CMD, format!("{email} isn't invited to this file")));
    }
    let Some(access) = access else {
        let reqs =
            old.iter().filter(|l| invited(l) == Some(email.as_str())).map(|l| Admin::Revoke { file: at.1.clone(), link: l.id.clone() }).collect();
        for r in batch(s, CMD, &at, reqs)? {
            if let AdminReply::Failed { reason } = r {
                return Err(EngineError::Other(reason));
            }
        }
        return Ok(json!({"email": email, "removed": true}));
    };
    let links = give(s, CMD, &at, &old, std::slice::from_ref(&email), access)?;
    let given: Vec<(String, String)> = std::iter::once(email.clone()).zip(links).collect();
    deliver(s, &given);
    Ok(json!({"email": email, "access": access.id()}))
}

fn link_access(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "collab.linkAccess";
    let want = level(CMD, p, "none")?;
    owned(s, CMD)?;
    let now = link_level(s);
    let shared = s.live().share.as_ref().and_then(|sh| sh.links.clone());
    let ids: Vec<String> = shared.iter().flat_map(|l| l.all()).map(|l| l.id.clone()).collect();
    match want {
        // Nobody gets in with a link: every link others got is revoked.
        None if shared.is_some() => {
            let req = share_request(s, ids, false).ok_or_else(|| bad(CMD, NOT_OWNED))?;
            share_answer(s, req)?;
            shared_now(s, None);
        }
        None => {}
        // Links for the first time, or new ones in place of links that gave more.
        Some(_) if shared.is_none() || rank(want) < rank(now) => {
            let revoke = if rank(want) < rank(now) { ids } else { Vec::new() };
            let req = share_request(s, revoke, true).ok_or_else(|| bad(CMD, NOT_OWNED))?;
            let links = share_answer(s, req)?.ok_or_else(|| EngineError::Other("the server made no links".into()))?;
            shared_now(s, Some(clean_shared(links)));
        }
        Some(_) => {}
    }
    if let Some(a) = want {
        let sh = s.live_mut().and_then(|l| l.share.as_mut()).ok_or_else(|| bad(CMD, NOT_OWNED))?;
        if sh.links.as_ref().is_some_and(|l| l.of(a).is_none()) {
            return Err(bad(CMD, "this file's server has no comment links: choose can view or can edit"));
        }
        sh.access = a;
    }
    let link = s.live().share.as_ref().and_then(|sh| sh.link()).filter(|_| want.is_some()).map(str::to_string);
    Ok(json!({"link": access_json(link_level(s)), "copy": link}))
}

fn web_address(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "collab.webAddress";
    let server = match string(p, "server") {
        Some(sv) => sv.trim().to_string(),
        None => s.live().server_file().map(|(sv, _)| sv).unwrap_or_else(|| s.prefs.publish_server.trim().to_string()),
    };
    if server.is_empty() {
        return Err(bad(CMD, "no server: give `server`, or set one in Preferences"));
    }
    // A server older than this request closes the connection: it names no web address.
    let web = match hosting_request(s, CMD, &server, "", "", Some(Admin::Web)) {
        Ok((_, Some(AdminReply::Web { web }))) => crate::team::web_address(&web).map(str::to_string),
        Err(EngineError::Waiting) => return Err(EngineError::Waiting),
        _ => None,
    };
    Ok(json!({"server": server, "web": web}))
}

pub fn specs() -> Vec<crate::cmd::CommandSpec> {
    vec![
        spec!(
            "collab.webAddress",
            "Server's web address",
            "{server? (default: the open file's server, else the one in Preferences)} → {server, web (where the server's browser build is, `https://…/`; null when its operator didn't say or the server is older than this request)}: what Copy link builds a link that opens in any browser with (`<web>#file=<link>`)",
            false,
            web_address
        ),
        spec!(
            "collab.access",
            "Who has access",
            "{} → {owner (the open file is yours on a server: the rest can change), link: edit | comment | view | none (what anyone with the file's link may do), people: [{email, access, id}] (the people invited by email, each with a link of their own), max}",
            false,
            access
        ),
        spec!(
            "collab.linkAccess",
            "Set link access",
            "{access: edit | comment | view | none} → {link, copy (the link to give now, or null)}: what anyone with the open file's link may do. Lowering it revokes the links others got and makes new ones (whoever joined with an old one is disconnected); none revokes them all. People invited by email keep their own links",
            false,
            link_access
        ),
        spec!(
            "collab.invite",
            "Invite people",
            "{emails: [address…] (at most 20), access?: edit (default) | comment | view} → {invited: [{email, access, link}], delivered}: each person gets a link of their own to the open file (yours on a server; at most 50 people a file). Signed in on the page's server, the file also goes on each person's Home (delivered: true; someone without an account gets it when they sign up); otherwise give them their link",
            false,
            invite
        ),
        spec!(
            "collab.setAccess",
            "Change a person's access",
            "{email, access: edit | comment | view | remove} → {email, access} | {email, removed}: an invited person gets a new link with that access (the old one is revoked), or loses access to the open file",
            false,
            set_access
        ),
    ]
}
