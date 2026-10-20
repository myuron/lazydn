use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use tokio::sync::mpsc::UnboundedSender;

use super::executor::execute;
use super::run::{ClientKind, Exec};
use super::sign_in::SignInHost;
use crate::action::{Action, Effect};
use crate::api::client::ApiClient;
use crate::api::clock::Clock;
use crate::api::error::ApiError;
use crate::api::keys::KeyPair;

/// Runs effects as tokio tasks and sends the resulting actions to the loop.
/// The device key read at startup signs the requests of the signed client, so that a restored
/// session needs no sign-in step.
pub struct TokioExec {
    guest: Arc<ApiClient>,
    signed: Arc<RwLock<Option<Arc<ApiClient>>>>,
    host: Arc<SignInHost>,
    tx: UnboundedSender<Action>,
}

impl TokioExec {
    pub fn new(
        base_url: &str,
        key_path: PathBuf,
        device_name: &str,
        key: Option<Arc<KeyPair>>,
        clock: Arc<dyn Clock>,
        tx: UnboundedSender<Action>,
    ) -> Result<Self, ApiError> {
        let guest = Arc::new(ApiClient::new(base_url, None, clock.clone())?);
        let host = Arc::new(SignInHost::new(key_path, base_url, device_name, clock));
        if let Some(key) = key {
            host.set_key(key);
        }
        let signed = host.signed_client().map(Arc::new);
        Ok(Self {
            guest,
            signed: Arc::new(RwLock::new(signed)),
            host,
            tx,
        })
    }

    // Not an error: a signed client is missing only before any key exists, and guest-readable
    // screens still work with the guest client.
    fn client_for(&self, kind: ClientKind) -> Arc<ApiClient> {
        match kind {
            ClientKind::Guest => self.guest.clone(),
            ClientKind::Signed => self
                .signed
                .read()
                .ok()
                .and_then(|slot| slot.clone())
                .unwrap_or_else(|| self.guest.clone()),
        }
    }
}

impl Exec for TokioExec {
    fn spawn(&mut self, client: ClientKind, effect: Effect) {
        let tx = self.tx.clone();
        match effect {
            Effect::PrepareSignIn { create } => {
                let host = self.host.clone();
                let signed = self.signed.clone();
                // The key file is read and written synchronously.
                tokio::task::spawn_blocking(move || {
                    let actions = host.prepare(create);
                    if actions
                        .iter()
                        .any(|a| matches!(a, Action::SignInPrepared { .. }))
                        && let Ok(mut slot) = signed.write()
                    {
                        *slot = host.signed_client().map(Arc::new);
                    }
                    for action in actions {
                        let _ = tx.send(action);
                    }
                });
            }
            Effect::RegisterDevice { code, verifier } => {
                let host = self.host.clone();
                tokio::spawn(async move {
                    for action in host.register(&code, verifier.expose()).await {
                        let _ = tx.send(action);
                    }
                });
            }
            other => {
                let client = self.client_for(client);
                tokio::spawn(async move {
                    for action in execute(&client, other).await {
                        let _ = tx.send(action);
                    }
                });
            }
        }
    }
}
