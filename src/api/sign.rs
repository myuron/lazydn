use std::fmt;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use ssh_key::{HashAlg, LineEnding, PrivateKey};

#[derive(Debug, Clone, PartialEq, Eq)]
/// A signer failure; the text must not contain key material.
pub struct SignError(pub String);

impl fmt::Display for SignError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for SignError {}

/// Produces SSHSIG signatures for request signing; implemented by the device key and by test doubles.
pub trait Signer: Send + Sync {
    /// Returns the binary SSHSIG blob (sha512), not the armored text.
    fn sign(&self, namespace: &str, message: &[u8]) -> Result<Vec<u8>, SignError>;
}

/// ssh-key only exposes the armored form, so the binary blob is recovered from its body.
pub(super) fn sshsig_bytes(
    key: &PrivateKey,
    namespace: &str,
    message: &[u8],
) -> Result<Vec<u8>, SignError> {
    let err = |e: ssh_key::Error| SignError(e.to_string());
    let armored = key
        .sign(namespace, HashAlg::Sha512, message)
        .map_err(err)?
        .to_pem(LineEnding::LF)
        .map_err(err)?;
    let body: String = armored
        .lines()
        .filter(|l| !l.starts_with("-----"))
        .collect();
    STANDARD.decode(body).map_err(|e| SignError(e.to_string()))
}
