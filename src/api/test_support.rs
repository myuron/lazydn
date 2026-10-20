use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use ssh_key::{HashAlg, PrivateKey, PublicKey, SshSig};

use super::request::NAMESPACE;
use super::sign::{SignError, Signer};

pub const GOLDEN_PUB: &str = include_str!("../../tests/fixtures/golden/key.pub");
pub const GOLDEN_SIG1: &str = include_str!("../../tests/fixtures/golden/msg1.sig");
pub const GOLDEN_SIG2: &str = include_str!("../../tests/fixtures/golden/msg2.sig");
/// Seed of the throwaway golden key; `key.pub` must be its public half.
pub const GOLDEN_SEED_HEX: &str =
    "912e99f354091833a1e47229147339bde78771b8506d66decbb53483a40da76c";
const RSA_BODY: &str = include_str!("../../tests/fixtures/golden/rsa_key.b64");
const ENCRYPTED_BODY: &str = include_str!("../../tests/fixtures/golden/enc_key.b64");

/// From `ssh-keygen -lf key.pub`, computed outside this repo.
pub const GOLDEN_FINGERPRINT: &str = "SHA256:KhVovNmj/HaIYLFS2Thqiu7ov5FlDlXpnntLqnfTL0I";

pub const GOLDEN_MSG1: &[u8] = b"1700000000\n0123456789abcdef0123456789abcdef\nGET\n/api/v1/me\n";
pub const GOLDEN_MSG2: &[u8] = b"1700000000\n0123456789abcdef0123456789abcdef\nPOST\n/api/v1/devices\n{\"code\":\"abc\",\"code_verifier\":\"xyz\"}";

pub fn write_private_key(path: &Path, content: &str) {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .expect("create key file in tempdir");
    f.write_all(content.as_bytes()).expect("write key file");
}

pub fn golden_seed_bytes() -> [u8; 32] {
    let seed: Vec<u8> = (0..32)
        .map(|i| u8::from_str_radix(&GOLDEN_SEED_HEX[i * 2..i * 2 + 2], 16).expect("hex"))
        .collect();
    seed.try_into().expect("32 bytes")
}

pub fn golden_private_key() -> PrivateKey {
    let seed = golden_seed_bytes();
    let pair = ssh_key::private::Ed25519Keypair::from_seed(&seed);
    PrivateKey::new(
        ssh_key::private::KeypairData::Ed25519(pair),
        "lazydn-test-only",
    )
    .expect("key builds")
}

pub fn golden_key_openssh() -> String {
    golden_private_key()
        .to_openssh(ssh_key::LineEnding::LF)
        .expect("encodes")
        .to_string()
}

fn framed(body: &str) -> String {
    format!(
        concat!(
            "-----BEGIN ",
            "OPENSSH PRIVATE KEY",
            "-----\n{}",
            "-----END ",
            "OPENSSH PRIVATE KEY",
            "-----\n"
        ),
        body
    )
}

pub fn rsa_key_pem() -> String {
    framed(RSA_BODY)
}

pub fn encrypted_key_pem() -> String {
    framed(ENCRYPTED_BODY)
}

pub fn golden_key_in(dir: &Path) -> PathBuf {
    let path = dir.join("id_ed25519");
    write_private_key(&path, &golden_key_openssh());
    path
}

pub fn golden_public_key() -> PublicKey {
    PublicKey::from_openssh(GOLDEN_PUB).expect("golden public key parses")
}

/// The binary SSHSIG inside an armored block.
pub fn armored_to_bytes(armored: &str) -> Vec<u8> {
    let body: String = armored
        .lines()
        .filter(|l| !l.starts_with("-----"))
        .collect();
    STANDARD.decode(body).expect("armored body is base64")
}

pub fn bytes_to_sshsig(bytes: &[u8]) -> Result<SshSig, String> {
    let b64 = STANDARD.encode(bytes);
    let mut pem = String::from("-----BEGIN SSH SIGNATURE-----\n");
    for chunk in b64.as_bytes().chunks(70) {
        pem.push_str(std::str::from_utf8(chunk).expect("ascii"));
        pem.push('\n');
    }
    pem.push_str("-----END SSH SIGNATURE-----\n");
    SshSig::from_pem(pem).map_err(|e| e.to_string())
}

/// Decodes an `X-SSH-Signature` header the way the server does: strip the
/// markers and whitespace, decode strict base64, then expect the SSHSIG magic.
pub fn parse_signature_header(header: &str) -> Result<SshSig, String> {
    let stripped: String = header
        .replace("-----BEGIN SSH SIGNATURE-----", "")
        .replace("-----END SSH SIGNATURE-----", "")
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    let bytes = STANDARD
        .decode(stripped)
        .map_err(|e| format!("not strict base64: {e}"))?;
    if !bytes.starts_with(b"SSHSIG") {
        return Err("missing SSHSIG magic".into());
    }
    bytes_to_sshsig(&bytes)
}

/// Signs with ssh-key directly so client tests do not depend on `KeyPair`.
pub struct TestSigner(PrivateKey);

impl TestSigner {
    pub fn golden() -> Self {
        TestSigner(golden_private_key())
    }
}

impl Signer for TestSigner {
    fn sign(&self, namespace: &str, message: &[u8]) -> Result<Vec<u8>, SignError> {
        let sig = self
            .0
            .sign(namespace, HashAlg::Sha512, message)
            .map_err(|e| SignError(e.to_string()))?;
        let pem = sig
            .to_pem(ssh_key::LineEnding::LF)
            .map_err(|e| SignError(e.to_string()))?;
        Ok(armored_to_bytes(&pem))
    }
}

pub struct FailingSigner;

impl Signer for FailingSigner {
    fn sign(&self, _namespace: &str, _message: &[u8]) -> Result<Vec<u8>, SignError> {
        Err(SignError("hardware key unavailable".into()))
    }
}

pub fn is_lower_hex32(s: &str) -> bool {
    s.len() == 32
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Rebuilds the signed message from what the mock server received and checks
/// the signature against the golden public key.
pub fn verify_received(req: &wiremock::Request) -> Result<(), String> {
    let header = |name: &str| -> Result<String, String> {
        req.headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
            .ok_or_else(|| format!("missing header {name}"))
    };
    let ts = header("X-Devnull-Timestamp")?;
    let nonce = header("X-Devnull-Nonce")?;
    let sig = parse_signature_header(&header("X-SSH-Signature")?)?;
    // wiremock only exposes a re-parsed Url, so this is the query as the Url
    // serializes it, not necessarily the exact bytes on the wire.
    let mut uri = req.url.path().to_string();
    if let Some(q) = req.url.query() {
        uri.push('?');
        uri.push_str(q);
    }
    let mut msg = format!("{ts}\n{nonce}\n{}\n{uri}\n", req.method).into_bytes();
    msg.extend_from_slice(&req.body);
    golden_public_key()
        .verify(NAMESPACE, &msg, &sig)
        .map_err(|e| format!("signature does not verify: {e}"))
}

#[test]
fn golden_seed_yields_the_public_key_ssh_keygen_made() {
    let derived = golden_private_key().public_key().to_openssh().unwrap();

    let two = |s: &str| s.split_whitespace().take(2).collect::<Vec<_>>().join(" ");
    assert_eq!(two(&derived), two(GOLDEN_PUB));
}
