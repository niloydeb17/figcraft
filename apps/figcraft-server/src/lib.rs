//! `figcraft-server`: FigCraft's standalone collaboration server (milestone M9, step 5 of
//! `docs/collab-protocol.md`; running it: `docs/server.md`).
//!
//! It keeps many files in a data directory ([`store`]), each with share links that let people
//! edit or only view, and serves their live sessions through the same hub the desktop app hosts
//! with ([`figcraft_collab_net::hub`]). Files persist as a snapshot plus an append-only log of what
//! changed ([`disk`]), compacted into a new snapshot from time to time, so a restart loses nothing
//! that was confirmed and clients resume where they were. There are no user accounts yet: share
//! links are the access, and the server key manages files.

pub mod abuse;
pub mod accounts;
pub mod cli;
pub mod codes;
pub mod config;
pub mod directory;
pub mod disk;
pub mod store;

use std::net::{SocketAddr, TcpListener};
use std::sync::{Arc, Mutex};

use figcraft_collab_net::hub::Hosting;

use crate::accounts::http::{AuthServer, Ctx, Settings};
use crate::accounts::pages::Site;
use crate::accounts::{Accounts, google};
use crate::config::Config;
use crate::store::Store;

/// A running server. Dropping it stops it: open files are saved, everyone is told the server is
/// restarting (their apps reconnect once it is back).
pub struct Running {
    hosting: Hosting,
    accounts: Option<AuthServer>,
}

impl Running {
    /// Where it listens.
    pub fn addr(&self) -> SocketAddr {
        self.hosting.addr()
    }

    /// Where the accounts' pages listen, when they are on.
    pub fn accounts_addr(&self) -> Option<SocketAddr> {
        self.accounts.as_ref().map(AuthServer::addr)
    }

    /// Open connections.
    pub fn connections(&self) -> usize {
        self.hosting.connections()
    }

    /// Bytes waiting to be sent to clients, all connections together.
    pub fn queued_bytes(&self) -> usize {
        self.hosting.queued_bytes()
    }

    /// Blocks of addresses the server keeps counters for (see [`Hosting::blocks_remembered`]).
    pub fn blocks_remembered(&self) -> usize {
        self.hosting.blocks_remembered()
    }

    /// Stop (see the type's docs).
    pub fn stop(&mut self) {
        if let Some(a) = &mut self.accounts {
            a.stop();
        }
        self.hosting.stop();
    }
}

/// Start serving `cfg.data` on `cfg.listen`. Files that can't be read are skipped and logged.
pub fn start(cfg: &Config) -> Result<Running, String> {
    cfg.check()?;
    let store = Store::open(&cfg.data, cfg.store())?;
    for p in store.problems() {
        log::warn!("{p}");
    }
    for w in cfg.warnings() {
        log::warn!("{w}");
    }
    let files = store.len();
    let addr = cfg.addr()?;
    let listener = TcpListener::bind(addr).map_err(|e| format!("can't listen on {addr}: {e}"))?;
    let addr = listener.local_addr().map_err(|e| e.to_string())?;
    let accounts = match cfg.accounts_addr()? {
        Some(at) => Some(start_accounts(cfg, at, store.key())?),
        None => None,
    };
    cli::write_running(&cfg.data, addr, accounts.as_ref().map(AuthServer::addr))?;
    let public = cli::public_endpoint(cfg, addr)?;
    let hosting = Hosting::serve(listener, store, cfg.net()).map_err(|e| format!("can't start serving: {e}"))?;
    log::info!(
        "figcraft-server {}: {files} file{} in {}; listening on {addr}; links name {}",
        env!("CARGO_PKG_VERSION"),
        if files == 1 { "" } else { "s" },
        cfg.data.display(),
        public.url()
    );
    if addr.ip().is_loopback() && cfg.public_url.is_none() {
        log::info!("only this machine can connect: listen on 0.0.0.0:{} (or behind a reverse proxy) to share", addr.port());
    }
    Ok(Running { hosting, accounts })
}

/// `scheme://host[:port]` of a web address.
fn origin(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    let host = rest.split('/').next().filter(|h| !h.is_empty())?;
    Some(format!("{scheme}://{host}"))
}

/// Serve the accounts' sign-in pages on `at`.
fn start_accounts(cfg: &Config, at: SocketAddr, key: &str) -> Result<AuthServer, String> {
    let accounts = Accounts::open(&cfg.data, cfg.signup, cfg.session_days)?;
    let web = cfg.web()?;
    let id = google::client_id(&cfg.google_client_id, &std::env::var(google::ID_ENV).unwrap_or_default());
    let secret = std::env::var(google::SECRET_ENV).unwrap_or_default();
    let google = match id {
        None => None,
        Some(id) if web.is_empty() => {
            log::warn!("accounts: a Google client id ({id}) is set but web_url isn't: Google sign-in stays off");
            None
        }
        Some(id) if secret.trim().is_empty() => {
            log::warn!("accounts: a Google client id ({id}) is set but {} is empty: Google sign-in stays off", google::SECRET_ENV);
            None
        }
        Some(id) => {
            let base = if web.ends_with('/') { web.clone() } else { format!("{web}/") };
            Some(google::Client { id, secret: secret.trim().to_string(), redirect: format!("{base}auth/google/callback") })
        }
    };
    let settings = Settings {
        origin: origin(&web),
        secure: web.starts_with("https://"),
        // `check` keeps the days finite, from 0.01 to 3650.
        idle: (cfg.session_days * 86_400.0).round() as u64,
        google,
        site: Site { contact: cfg.operator_contact.trim().to_string(), terms: cfg.terms_url.trim().to_string() },
        key: key.to_string(),
    };
    let listener = TcpListener::bind(at).map_err(|e| format!("can't listen for the accounts on {at}: {e}"))?;
    let at = listener.local_addr().map_err(|e| e.to_string())?;
    log::info!(
        "accounts: {} account{}; sign-in pages on {at}; sign-up {}; Google sign-in {}",
        accounts.len(),
        if accounts.len() == 1 { "" } else { "s" },
        if cfg.signup { "open" } else { "closed" },
        if settings.google.is_some() { "on" } else { "off" }
    );
    AuthServer::start(listener, Ctx::new(Arc::new(Mutex::new(accounts)), settings))
}

#[cfg(test)]
mod accounts_tests;
#[cfg(test)]
mod admin_budget_tests;
#[cfg(test)]
mod background_tests;
#[cfg(test)]
mod cli_tests;
#[cfg(test)]
mod codes_tests;
#[cfg(test)]
mod comment_link_tests;
#[cfg(test)]
mod directory_tests;
#[cfg(test)]
mod download_burst_tests;
#[cfg(test)]
mod hardening_tests;
#[cfg(test)]
mod idle_io_tests;
#[cfg(test)]
mod library_actions_tests;
#[cfg(test)]
mod library_assets_tests;
#[cfg(test)]
mod library_detach_tests;
#[cfg(test)]
mod library_tests;
#[cfg(test)]
mod owned_tests;
#[cfg(test)]
mod public_tests;
#[cfg(test)]
mod server_tests;
#[cfg(test)]
mod store_tests;
#[cfg(test)]
mod team_tests;
#[cfg(test)]
mod test_util;
