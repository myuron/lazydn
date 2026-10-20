use std::path::{Path, PathBuf};
use std::sync::Arc;

use time::{OffsetDateTime, UtcOffset};

use crate::action::{Action, Effect};
use crate::api::keys::KeyPair;
use crate::app::{App, KeyStatus, Session, ToastKind};
use crate::config::{LastVisits, LoadedConfig};

/// Looks at the device key once, before the app starts.
/// A readable key means the session is to be restored, and the key is handed on to sign requests;
/// no key or an unusable one means a guest.
pub fn session_for_key(key_path: &Path) -> (Session, KeyStatus, Option<Arc<KeyPair>>) {
    match KeyPair::load(key_path) {
        Ok(Some(key)) => (Session::Restoring, KeyStatus::Present, Some(Arc::new(key))),
        Ok(None) => (Session::Guest, KeyStatus::Absent, None),
        Err(error) => (
            Session::Guest,
            KeyStatus::Unreadable {
                error,
                path: key_path.to_path_buf(),
            },
            None,
        ),
    }
}

/// Everything main has found out before the app exists.
pub struct Startup {
    pub session: Session,
    pub key_status: KeyStatus,
    pub key_path: PathBuf,
    pub config: LoadedConfig,
    /// From `COLORTERM`; the config file has no say.
    pub truecolor: bool,
    pub host_label: Option<String>,
    pub last_visits: LastVisits,
    pub now: OffsetDateTime,
    pub offset: UtcOffset,
    /// Terminal columns and rows, so that the first fetches are planned for the real screen.
    pub size: (u16, u16),
}

/// Builds the app and returns the effects of its first `Init`.
pub fn build_app(startup: Startup) -> (App, Vec<Effect>) {
    let mut app = App::with_key_status(
        startup.session,
        startup.now,
        startup.offset,
        startup.key_status,
        startup.key_path,
    );
    app.set_prefs(crate::app::Prefs {
        truecolor: startup.truecolor,
        ..startup.config.prefs
    });
    app.set_host_label(startup.host_label);
    for (tab, id) in startup.last_visits {
        app.update(Action::SetLastVisit { tab, id });
    }
    // A key that cannot be used is the more pressing message, and only one toast is kept.
    if let Some(text) = startup.config.warning
        && app.toast().is_none()
    {
        app.update(Action::Notice {
            kind: ToastKind::Error,
            text,
        });
    }
    app.update(Action::Resize(startup.size.0, startup.size.1));
    let effects = app.update(Action::Init);
    (app, effects)
}
