use base64::Engine;
use base64::engine::general_purpose::STANDARD;

use super::clock::Clock;
use super::error::{ApiError, ErrorKind};
use super::nonce;
use super::sign::Signer;

/// The SSHSIG namespace the server verifies.
pub const NAMESPACE: &str = "devnull-api";

#[derive(Debug, Clone, PartialEq, Eq)]
/// The three header values of a signed request.
pub struct SignedHeaders {
    /// Value of `X-Devnull-Timestamp`.
    pub timestamp: String,
    /// Value of `X-Devnull-Nonce`.
    pub nonce: String,
    /// Value of `X-SSH-Signature`.
    pub signature: String,
}

/// `{ts}\n{nonce}\n{METHOD}\n{path_and_query}\n{body}`
pub fn signing_message(
    ts: i64,
    nonce: &str,
    method: &str,
    path_and_query: &str,
    body: &[u8],
) -> Vec<u8> {
    let mut msg = format!("{ts}\n{nonce}\n{method}\n{path_and_query}\n").into_bytes();
    msg.extend_from_slice(body);
    msg
}

/// Standard base64 of the SSHSIG bytes on a single line.
pub fn encode_signature(sshsig: &[u8]) -> String {
    STANDARD.encode(sshsig)
}

/// Builds the headers for one request; `path_and_query` and `body` must be exactly what is sent. Each call uses a new nonce.
pub fn sign_request(
    signer: &dyn Signer,
    clock: &dyn Clock,
    method: &str,
    path_and_query: &str,
    body: &[u8],
) -> Result<SignedHeaders, ApiError> {
    let ts = clock.now_unix();
    let nonce = nonce::generate();
    let msg = signing_message(ts, &nonce, method, path_and_query, body);
    let sig = signer.sign(NAMESPACE, &msg).map_err(|e| ApiError {
        kind: ErrorKind::Sign(e.0),
    })?;
    Ok(SignedHeaders {
        timestamp: ts.to_string(),
        nonce,
        signature: encode_signature(&sig),
    })
}
