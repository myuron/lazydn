use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use super::executor::{failure_of, status};
use crate::action::{Action, Failure};
use crate::api::authorize::{REDIRECT_URI, authorize_url};
use crate::api::client::ApiClient;
use crate::api::clock::Clock;
use crate::api::error::ErrorKind;
use crate::api::keys::KeyPair;
use crate::api::pkce::{Pkce, random_state};
use crate::api::sign::Signer;
use crate::text::is_unsafe_char;

/// What the runtime keeps between the sign-in effects: where the key is, which host to ask,
/// and the key once it has been read.
pub struct SignInHost {
    key_path: PathBuf,
    base_url: String,
    device_name: String,
    clock: Arc<dyn Clock>,
    key: Mutex<Option<Arc<KeyPair>>>,
}

impl SignInHost {
    /// `base_url` also builds the approval URL, so that a mock host gets a mock approval page.
    pub fn new(
        key_path: PathBuf,
        base_url: &str,
        device_name: &str,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            key_path,
            base_url: base_url.to_string(),
            device_name: device_name.to_string(),
            clock,
            key: Mutex::new(None),
        }
    }

    /// Runs `Effect::PrepareSignIn`: answers `SignInPrepared`, `SignInKeyFailed`, or `SignInKeyMissing`
    /// when `create` is unset and the key file is gone.
    pub fn prepare(&self, create: bool) -> Vec<Action> {
        let (key, created) = match KeyPair::load(&self.key_path) {
            Ok(Some(key)) => (key, false),
            Ok(None) if create => match KeyPair::load_or_create(&self.key_path) {
                Ok(key) => (key, true),
                Err(error) => return vec![Action::SignInKeyFailed { error }],
            },
            Ok(None) => return vec![Action::SignInKeyMissing],
            Err(error) => return vec![Action::SignInKeyFailed { error }],
        };
        let key = Arc::new(key);
        let pkce = Pkce::generate();
        let state = random_state();
        let fingerprint = key.fingerprint();
        let url = authorize_url(
            &self.base_url,
            &fingerprint,
            &self.device_name,
            &pkce.challenge,
            &state,
            REDIRECT_URI,
        );
        let Ok(url) = url else {
            return vec![Action::SignInPrepareFailed {
                failure: Failure::new("invalid_base_url"),
            }];
        };
        if let Ok(mut slot) = self.key.lock() {
            *slot = Some(key);
        }
        vec![Action::SignInPrepared {
            fingerprint,
            url: url.to_string(),
            pkce,
            state,
            created,
        }]
    }

    /// Hands over a key that was read before the runtime started, so that a restored session can sign.
    pub fn set_key(&self, key: Arc<KeyPair>) {
        if let Ok(mut slot) = self.key.lock() {
            *slot = Some(key);
        }
    }

    /// The key that `prepare` read or made.
    pub fn key(&self) -> Option<Arc<KeyPair>> {
        self.key.lock().ok().and_then(|k| k.clone())
    }

    /// The client that `Effect::UseSignedClient` swaps in; `None` before a key is known.
    pub fn signed_client(&self) -> Option<ApiClient> {
        let signer: Arc<dyn Signer> = self.key()?;
        ApiClient::new(&self.base_url, Some(signer), self.clock.clone()).ok()
    }

    /// Runs `Effect::RegisterDevice` with the key signing the request.
    /// Answers `DeviceRegistered` or `SignInFailed`, then `ApiStatus`.
    pub async fn register(&self, code: &str, verifier: &str) -> Vec<Action> {
        let Some(client) = self.signed_client() else {
            return vec![Action::SignInFailed {
                failure: Failure::new("key_missing"),
            }];
        };
        match client.register_device(code, verifier).await {
            Ok(response) => vec![
                Action::DeviceRegistered {
                    response: Box::new(response.data),
                },
                status(response.rate, true),
            ],
            Err(error) => {
                let mut out = vec![Action::SignInFailed {
                    failure: failure_of(&error),
                }];
                match error.kind {
                    ErrorKind::Network(_) => out.push(status(None, false)),
                    ErrorKind::Http { .. } | ErrorKind::Decode(_) => out.push(status(None, true)),
                    ErrorKind::Sign(_)
                    | ErrorKind::SignInRequired
                    | ErrorKind::InvalidArgument(_)
                    | ErrorKind::InvalidBaseUrl => {}
                }
                out
            }
        }
    }
}

const DEVICE_NAME_MAX: usize = 100;
const DEVICE_NAME_PREFIX: &str = "lazydn on ";

/// `lazydn on <hostname>`, or `lazydn` when the host has no usable name.
/// Control characters and bidi marks are dropped and the whole name is cut at 100 characters.
pub fn device_name_from(hostname: Option<&str>) -> String {
    let cleaned: String = hostname
        .unwrap_or_default()
        .chars()
        .filter(|c| !is_unsafe_char(*c))
        .collect();
    let host = cleaned.trim();
    if host.is_empty() {
        return "lazydn".into();
    }
    let room = DEVICE_NAME_MAX - DEVICE_NAME_PREFIX.len();
    format!(
        "{DEVICE_NAME_PREFIX}{}",
        host.chars().take(room).collect::<String>()
    )
}

/// `device_name_from` the name the OS reports.
pub fn device_name() -> String {
    let hostname = std::fs::read_to_string("/proc/sys/kernel/hostname").ok();
    device_name_from(hostname.as_deref())
}
