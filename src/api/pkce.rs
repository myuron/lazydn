use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest, Sha256};
use ssh_key::rand_core::{OsRng, RngCore};

const UNRESERVED: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789._~-";
const VERIFIER_LEN: usize = 64;

#[derive(Clone, PartialEq, Eq)]
/// A PKCE verifier and its challenge; make one per sign-in attempt and keep the verifier until `register_device`.
pub struct Pkce {
    /// The secret sent when redeeming the code.
    pub verifier: String,
    /// Sent in the authorize URL.
    pub challenge: String,
}

impl std::fmt::Debug for Pkce {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pkce")
            .field("verifier", &"<hidden>")
            .field("challenge", &self.challenge)
            .finish()
    }
}

impl Pkce {
    /// Makes a fresh pair from the OS random source.
    pub fn generate() -> Self {
        let verifier = random_verifier();
        let challenge = Self::challenge_for(&verifier);
        Pkce {
            verifier,
            challenge,
        }
    }

    /// base64url (no padding) of sha256(verifier).
    pub fn challenge_for(verifier: &str) -> String {
        URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
    }
}

/// Bytes at or above the largest multiple of the alphabet size are dropped so no character is favoured.
fn random_verifier() -> String {
    let limit = 256 - (256 % UNRESERVED.len());
    let mut out = String::with_capacity(VERIFIER_LEN);
    let mut buf = [0u8; 32];
    while out.len() < VERIFIER_LEN {
        OsRng.fill_bytes(&mut buf);
        for &b in buf.iter().filter(|b| (**b as usize) < limit) {
            if out.len() < VERIFIER_LEN {
                out.push(UNRESERVED[b as usize % UNRESERVED.len()] as char);
            }
        }
    }
    out
}

/// 32 lowercase hex characters for the authorize `state`.
pub fn random_state() -> String {
    super::nonce::generate()
}
