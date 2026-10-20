use std::path::PathBuf;

use super::App;
use crate::api::keys::KeyError;

/// What main found at the device key path before the app started.
#[derive(Debug, Clone, PartialEq)]
pub enum KeyStatus {
    /// No key file; a first sign-in will create one.
    Absent,
    /// A key file was read; the app may try to restore the session with it.
    Present,
    /// The file exists but cannot be used; the app starts as a guest and says why.
    Unreadable { error: KeyError, path: PathBuf },
}

impl App {
    /// What main found at startup; the sign-in dialog reads it to decide whether to offer creating a key.
    pub fn key_status(&self) -> &KeyStatus {
        &self.key_status
    }
}
