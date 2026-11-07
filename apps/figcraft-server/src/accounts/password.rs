//! Password hashes: argon2id with the argon2 crate's defaults (19 MiB, two passes, one lane: the
//! OWASP recommendation), stored as PHC strings. At most [`AT_ONCE`] are computed at a time, so a
//! burst of sign-ins can't take all the memory.

use std::sync::{Condvar, Mutex};

use argon2::Argon2;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};

/// Hashes computed at once.
pub const AT_ONCE: usize = 4;

static BUSY: Mutex<usize> = Mutex::new(0);
static FREED: Condvar = Condvar::new();

/// Holds one of the [`AT_ONCE`] places while it lives.
struct Place;

impl Place {
    fn take() -> Place {
        let mut busy = BUSY.lock().unwrap_or_else(|e| e.into_inner());
        while *busy >= AT_ONCE {
            busy = FREED.wait(busy).unwrap_or_else(|e| e.into_inner());
        }
        *busy += 1;
        Place
    }
}

impl Drop for Place {
    fn drop(&mut self) {
        let mut busy = BUSY.lock().unwrap_or_else(|e| e.into_inner());
        *busy = busy.saturating_sub(1);
        FREED.notify_one();
    }
}

/// The hash of a new password.
pub fn hash(password: &str) -> Result<String, String> {
    let mut salt = [0u8; 16];
    getrandom::fill(&mut salt).map_err(|e| format!("no secure random numbers: {e}"))?;
    let salt = SaltString::encode_b64(&salt).map_err(|e| e.to_string())?;
    let _place = Place::take();
    Argon2::default().hash_password(password.as_bytes(), &salt).map(|h| h.to_string()).map_err(|e| format!("can't hash the password: {e}"))
}

/// Whether `password` matches `hash` (a hash that can't be read matches nothing).
/// Check `password` against a hash nobody's password matches, taking as long as [`verify`] does: a
/// sign-in that fails for another reason answers as slowly as a wrong password.
pub fn waste(password: &str) {
    static STAND_IN: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    if let Some(h) = STAND_IN.get_or_init(|| hash("a stand-in nobody signs in with").ok()) {
        let _ = verify(password, h);
    }
}

pub fn verify(password: &str, hash: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(hash) else { return false };
    let _place = Place::take();
    Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok()
}
