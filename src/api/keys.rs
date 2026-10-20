use std::ffi::OsString;
use std::fmt;
use std::fs::{DirBuilder, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use ssh_key::rand_core::OsRng;
use ssh_key::{Algorithm, HashAlg, LineEnding, PrivateKey};

use super::nonce;
use super::sign::{SignError, Signer, sshsig_bytes};

/// Why a key file could not be loaded or created.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyError {
    /// A filesystem call failed; carries the `io::ErrorKind`, never a path or content.
    Io(String),
    /// The file is not an OpenSSH private key.
    Parse(String),
    /// The key is not ed25519.
    NotEd25519,
    /// The key is passphrase-protected, which this app cannot unlock.
    Encrypted,
    /// Group or others can access the key file.
    InsecurePermissions,
    /// The path is not a regular file (a directory or FIFO, say).
    NotARegularFile,
    /// Generating or encoding a new key failed.
    Generate(String),
}

impl fmt::Display for KeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KeyError::Io(kind) => write!(f, "key file I/O error: {kind}"),
            KeyError::Parse(msg) => write!(f, "unreadable key file: {msg}"),
            KeyError::NotEd25519 => write!(f, "the key is not an ed25519 key"),
            KeyError::Encrypted => write!(f, "the key is encrypted"),
            KeyError::InsecurePermissions => {
                write!(f, "the key file is accessible by group or others")
            }
            KeyError::NotARegularFile => write!(f, "the key path is not a regular file"),
            KeyError::Generate(msg) => write!(f, "could not generate a key: {msg}"),
        }
    }
}

impl std::error::Error for KeyError {}

/// The device key; the private half never leaves this type.
pub struct KeyPair {
    key: PrivateKey,
}

fn io_err(e: io::Error) -> KeyError {
    KeyError::Io(e.kind().to_string())
}

fn pub_path(path: &Path) -> PathBuf {
    let mut name = OsString::from(path.as_os_str());
    name.push(".pub");
    PathBuf::from(name)
}

/// Fills a temp file and hard-links it into place, so a reader never sees a partial
/// key and an existing file is never replaced (the link fails with `AlreadyExists`).
fn write_atomic(path: &Path, mode: u32, content: &str) -> io::Result<()> {
    let mut tmp_name = OsString::from(".");
    tmp_name.push(path.file_name().unwrap_or_default());
    tmp_name.push(format!(".tmp-{}", nonce::generate()));
    let tmp = path.with_file_name(tmp_name);
    // If this open fails, the name may belong to someone else, so it is never removed.
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(&tmp)?;
    let filled = file
        .write_all(content.as_bytes())
        .and_then(|()| file.sync_all());
    let linked = filled.and_then(|()| std::fs::hard_link(&tmp, path));
    let _ = std::fs::remove_file(&tmp);
    linked?;
    // Best effort: the key already exists, only its durability across a crash is at stake.
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        let _ = File::open(dir).and_then(|d| d.sync_all());
    }
    Ok(())
}

impl KeyPair {
    /// Reads the key at `path` without creating anything: `None` when the file is absent.
    /// Call it at startup to tell whether a device key exists.
    pub fn load(path: &Path) -> Result<Option<KeyPair>, KeyError> {
        match std::fs::metadata(path) {
            Ok(meta) => Ok(Some(KeyPair {
                key: Self::read(path, &meta)?,
            })),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(io_err(e)),
        }
    }

    /// Reads the key at `path`, or creates one when the file is absent; never overwrites.
    /// Call it at startup and before sign-in.
    pub fn load_or_create(path: &Path) -> Result<KeyPair, KeyError> {
        let pair = match Self::load(path)? {
            Some(pair) => pair,
            None => KeyPair {
                key: Self::create(path)?,
            },
        };
        // A missing .pub must not make a usable key unusable (read-only directory, say).
        let _ = pair.ensure_public_file(path);
        Ok(pair)
    }

    /// Checks the file kind and mode before reading, so a FIFO cannot block and a
    /// shared key is refused.
    fn read(path: &Path, meta: &std::fs::Metadata) -> Result<PrivateKey, KeyError> {
        if !meta.is_file() {
            return Err(KeyError::NotARegularFile);
        }
        if meta.mode() & 0o077 != 0 {
            return Err(KeyError::InsecurePermissions);
        }
        // This reader zeroizes its buffer; the parser's message is dropped so no
        // key material can reach an error.
        let key = PrivateKey::read_openssh_file(path).map_err(|e| match e {
            ssh_key::Error::Io(kind) => KeyError::Io(kind.to_string()),
            _ => KeyError::Parse("not an OpenSSH private key".into()),
        })?;
        if key.is_encrypted() {
            return Err(KeyError::Encrypted);
        }
        if key.algorithm() != Algorithm::Ed25519 {
            return Err(KeyError::NotEd25519);
        }
        Ok(key)
    }

    fn create(path: &Path) -> Result<PrivateKey, KeyError> {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(dir)
                .map_err(io_err)?;
        }
        let key = PrivateKey::random(&mut OsRng, Algorithm::Ed25519)
            .map_err(|e| KeyError::Generate(e.to_string()))?;
        let pem = key
            .to_openssh(LineEnding::LF)
            .map_err(|e| KeyError::Generate(e.to_string()))?;
        match write_atomic(path, 0o600, &pem) {
            Ok(()) => Ok(key),
            // Another process won the race; use its key rather than ours.
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                Self::read(path, &std::fs::metadata(path).map_err(io_err)?)
            }
            Err(e) => Err(io_err(e)),
        }
    }

    /// An existing `.pub` is left alone even when it disagrees, since the private key is the authority.
    fn ensure_public_file(&self, path: &Path) -> Result<(), KeyError> {
        let line = self
            .key
            .public_key()
            .to_openssh()
            .map_err(|e| KeyError::Generate(e.to_string()))?;
        match write_atomic(&pub_path(path), 0o644, &format!("{line}\n")) {
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(()),
            other => other.map_err(io_err),
        }
    }

    /// `SHA256:` plus 43 base64 characters, the `ssh-keygen -lf` form, for the authorize URL.
    pub fn fingerprint(&self) -> String {
        self.key.fingerprint(HashAlg::Sha256).to_string()
    }

    /// The one-line OpenSSH public key; empty only if encoding fails, which ed25519 cannot do.
    pub fn public_key_openssh(&self) -> String {
        self.key.public_key().to_openssh().unwrap_or_default()
    }
}

impl fmt::Debug for KeyPair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("KeyPair").finish_non_exhaustive()
    }
}

impl Signer for KeyPair {
    fn sign(&self, namespace: &str, message: &[u8]) -> Result<Vec<u8>, SignError> {
        sshsig_bytes(&self.key, namespace, message)
    }
}
