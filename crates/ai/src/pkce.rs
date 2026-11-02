//! PKCE (RFC 7636) with the S256 method, and the random values a sign-in needs.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest, Sha256};

use crate::AiError;

/// A code verifier and the challenge sent with the authorization request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

/// `bytes` random bytes as base64url without padding.
pub fn random_token(bytes: usize) -> Result<String, AiError> {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).map_err(|e| AiError::Random(e.to_string()))?;
    Ok(URL_SAFE_NO_PAD.encode(buf))
}

/// `bytes` random bytes as lowercase hex.
pub fn random_hex(bytes: usize) -> Result<String, AiError> {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).map_err(|e| AiError::Random(e.to_string()))?;
    Ok(buf.iter().map(|b| format!("{b:02x}")).collect())
}

/// A random (version 4) UUID in its hyphenated form.
pub fn uuid_v4() -> Result<String, AiError> {
    let mut b = [0u8; 16];
    getrandom::fill(&mut b).map_err(|e| AiError::Random(e.to_string()))?;
    // Version 4 in the high nibble of byte 6, the RFC 4122 variant in the top bits of byte 8.
    if let Some(v) = b.get_mut(6) {
        *v = (*v & 0x0f) | 0x40;
    }
    if let Some(v) = b.get_mut(8) {
        *v = (*v & 0x3f) | 0x80;
    }
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    Ok(format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32]))
}

/// The S256 challenge of `verifier`: base64url of its SHA-256, without padding.
pub fn challenge_for(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// A fresh verifier (43 characters, 256 bits) and its challenge.
pub fn generate() -> Result<Pkce, AiError> {
    let verifier = random_token(32)?;
    let challenge = challenge_for(&verifier);
    Ok(Pkce { verifier, challenge })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The worked example of RFC 7636, appendix B.
    #[test]
    fn the_challenge_matches_the_rfc_example() {
        assert_eq!(challenge_for("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"), "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    }

    #[test]
    fn a_generated_pair_is_valid_and_fresh() {
        let (a, b) = (generate().unwrap(), generate().unwrap());
        assert_eq!(a.verifier.len(), 43);
        assert!(a.verifier.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_'));
        assert_eq!(a.challenge, challenge_for(&a.verifier));
        assert_ne!(a.verifier, b.verifier);
        assert_eq!(random_hex(8).unwrap().len(), 16);
    }

    #[test]
    fn a_uuid_is_version_4_and_hyphenated() {
        let u = uuid_v4().unwrap();
        let parts: Vec<&str> = u.split('-').collect();
        assert_eq!(parts.iter().map(|p| p.len()).collect::<Vec<_>>(), [8, 4, 4, 4, 12]);
        assert!(parts[2].starts_with('4'), "{u}");
        assert!(matches!(parts[3].as_bytes()[0], b'8' | b'9' | b'a' | b'b'), "{u}");
        assert_ne!(u, uuid_v4().unwrap());
    }
}
