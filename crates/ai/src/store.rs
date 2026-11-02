//! Where the sign-in is kept. Desktop only: a JSON file in the app's folder, readable by the user
//! alone (mode 0600 on Unix). Anyone who can read it can use the plan, so it is never written next
//! to a document and never printed.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::AiError;
use crate::chatgpt::Tokens;

/// The sign-in on this machine.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Credentials {
    /// The client OpenAI registered for this app.
    pub client_id: String,
    /// Stands for this installation (`ext_agent_host_id`).
    pub host_id: String,
    pub tokens: Tokens,
    /// When `tokens` were obtained, in Unix seconds.
    pub saved_at: u64,
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credentials").field("client_id", &self.client_id).field("tokens", &self.tokens).finish()
    }
}

/// Seconds since the Unix epoch.
pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

impl Credentials {
    pub fn new(client_id: String, host_id: String, tokens: Tokens) -> Self {
        Self { client_id, host_id, tokens, saved_at: now() }
    }

    /// Whether the access token is still good at `at` (Unix seconds), with a minute to spare.
    pub fn access_valid_at(&self, at: u64) -> bool {
        let life = self.tokens.expires_in.unwrap_or(0);
        at.saturating_add(60) < self.saved_at.saturating_add(life)
    }

    /// Take the tokens of a refresh: the access token, and the replacement refresh token (the old
    /// one is spent; a reply without one keeps what there was).
    pub fn renewed(&self, mut tokens: Tokens) -> Self {
        if tokens.refresh_token.is_none() {
            tokens.refresh_token = self.tokens.refresh_token.clone();
        }
        if tokens.scope.is_none() {
            tokens.scope = self.tokens.scope.clone();
        }
        Self { client_id: self.client_id.clone(), host_id: self.host_id.clone(), tokens, saved_at: now() }
    }
}

/// The folder FigCraft keeps its AI sign-in in: `$FIGCRAFT_AI_DIR` when set, else the user's
/// application-data folder.
pub fn dir() -> Result<PathBuf, AiError> {
    if let Some(d) = std::env::var_os("FIGCRAFT_AI_DIR").filter(|d| !d.is_empty()) {
        return Ok(PathBuf::from(d));
    }
    let var = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    let base = if cfg!(target_os = "macos") {
        var("HOME").map(|h| h.join("Library").join("Application Support"))
    } else if cfg!(windows) {
        var("APPDATA")
    } else {
        var("XDG_CONFIG_HOME").or_else(|| var("HOME").map(|h| h.join(".config")))
    };
    base.map(|b| b.join("FigCraft")).ok_or_else(|| AiError::Store("no home folder to keep the sign-in in".into()))
}

fn file_in(dir: &Path) -> PathBuf {
    dir.join("chatgpt-sign-in.json")
}

/// This installation's host id (`ext_agent_host_id`): `urn:uuid:` and a random UUID, made once and
/// kept before the first sign-in, so a sign-in that fails (or a second one) reuses it. It identifies
/// the machine's install and is not a credential.
pub fn host_id_in(dir: &Path) -> Result<String, AiError> {
    let path = dir.join("host-id");
    if let Ok(text) = std::fs::read_to_string(&path) {
        let id = text.trim();
        if id.starts_with("urn:uuid:") {
            return Ok(id.to_string());
        }
    }
    let id = format!("urn:uuid:{}", crate::pkce::uuid_v4()?);
    std::fs::create_dir_all(dir).map_err(|e| AiError::Store(e.to_string()))?;
    std::fs::write(&path, &id).map_err(|e| AiError::Store(e.to_string()))?;
    Ok(id)
}

/// The stored sign-in in `dir`, if any.
pub fn load_from(dir: &Path) -> Result<Option<Credentials>, AiError> {
    let text = match std::fs::read_to_string(file_in(dir)) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(AiError::Store(e.to_string())),
    };
    serde_json::from_str(&text).map(Some).map_err(|e| AiError::Store(format!("not readable ({e}): sign in again")))
}

/// Keep `c` in `dir`: written to a sibling file readable by the user alone, then moved into place.
pub fn save_to(dir: &Path, c: &Credentials) -> Result<(), AiError> {
    std::fs::create_dir_all(dir).map_err(|e| AiError::Store(e.to_string()))?;
    let text = serde_json::to_string_pretty(c).map_err(|e| AiError::Store(e.to_string()))?;
    let (tmp, target) = (dir.join("chatgpt-sign-in.json.tmp"), file_in(dir));
    write_private(&tmp, &text).map_err(|e| AiError::Store(e.to_string()))?;
    std::fs::rename(&tmp, &target).map_err(|e| AiError::Store(e.to_string()))
}

#[cfg(unix)]
fn write_private(path: &Path, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(path)?;
    f.write_all(text.as_bytes())
}

#[cfg(not(unix))]
fn write_private(path: &Path, text: &str) -> std::io::Result<()> {
    std::fs::write(path, text)
}

/// Forget the sign-in in `dir`. True when there was one.
pub fn clear_in(dir: &Path) -> Result<bool, AiError> {
    match std::fs::remove_file(file_in(dir)) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(AiError::Store(e.to_string())),
    }
}

pub fn host_id() -> Result<String, AiError> {
    host_id_in(&dir()?)
}

pub fn load() -> Result<Option<Credentials>, AiError> {
    load_from(&dir()?)
}

pub fn save(c: &Credentials) -> Result<(), AiError> {
    save_to(&dir()?, c)
}

pub fn clear() -> Result<bool, AiError> {
    clear_in(&dir()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(expires: u64) -> Tokens {
        serde_json::from_value(
            serde_json::json!({"access_token": "A", "refresh_token": "R1", "expires_in": expires, "scope": "openid chatgpt.tokens.use.direct"}),
        )
        .unwrap()
    }

    fn temp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("figcraft-ai-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn credentials_round_trip_and_clear() {
        let dir = temp_dir("rt");
        assert_eq!(load_from(&dir).unwrap(), None);
        let c = Credentials::new("oaiapp_1".into(), "figcraft-x".into(), tokens(3600));
        save_to(&dir, &c).unwrap();
        assert_eq!(load_from(&dir).unwrap(), Some(c));
        assert!(clear_in(&dir).unwrap());
        assert!(!clear_in(&dir).unwrap());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn the_file_is_readable_by_the_user_alone() {
        use std::os::unix::fs::PermissionsExt;
        let dir = temp_dir("mode");
        save_to(&dir, &Credentials::new("c".into(), "h".into(), tokens(10))).unwrap();
        let mode = std::fs::metadata(file_in(&dir)).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_broken_file_asks_to_sign_in_again() {
        let dir = temp_dir("bad");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(file_in(&dir), "{nope").unwrap();
        assert!(matches!(load_from(&dir), Err(AiError::Store(m)) if m.contains("sign in again")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_access_token_is_good_for_its_hour_less_a_minute() {
        let mut c = Credentials::new("c".into(), "h".into(), tokens(3600));
        c.saved_at = 1_000;
        assert!(c.access_valid_at(1_000 + 3_500));
        assert!(!c.access_valid_at(1_000 + 3_541));
        assert!(!c.access_valid_at(1_000 + 4_000));
    }

    #[test]
    fn a_refresh_keeps_the_new_refresh_token_or_else_the_old_one() {
        let c = Credentials::new("c".into(), "h".into(), tokens(3600));
        let mut fresh = tokens(3600);
        fresh.refresh_token = Some("R2".into());
        assert_eq!(c.renewed(fresh).tokens.refresh_token.as_deref(), Some("R2"));
        let mut none = tokens(3600);
        none.refresh_token = None;
        none.scope = None;
        let kept = c.renewed(none);
        assert_eq!(kept.tokens.refresh_token.as_deref(), Some("R1"));
        assert_eq!(kept.tokens.scope.as_deref(), Some("openid chatgpt.tokens.use.direct"));
    }

    #[test]
    fn the_host_id_is_made_once_and_kept() {
        let dir = temp_dir("host");
        let first = host_id_in(&dir).unwrap();
        assert!(first.starts_with("urn:uuid:") && first.len() == "urn:uuid:".len() + 36, "{first}");
        assert_eq!(host_id_in(&dir).unwrap(), first);
        // Clearing the sign-in does not change which machine this is.
        let _ = clear_in(&dir);
        assert_eq!(host_id_in(&dir).unwrap(), first);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn debug_output_has_no_secrets() {
        let c = Credentials::new("c".into(), "h".into(), tokens(10));
        assert!(!format!("{c:?}").contains("R1"));
    }
}
