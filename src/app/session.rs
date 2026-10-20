use super::notifications::ME_INTERVAL_SECS;
use super::{App, LogTab, Me, Modal, Pane, Screen, Session, ToastKind, toast};
use crate::action::{Action, Effect};
use crate::feed::{Feed, FeedKey, ReqId};
use crate::model::MeResponse;
use crate::notifications::NotificationsState;
use crate::settings::SettingsState;

/// Session transitions: restoring a stored key, and falling back to a guest when the server refuses it.
impl App {
    /// Handles the two refusals that end a session. `Some` means the action is fully handled.
    pub(super) fn handle_rejection(&mut self, action: &Action) -> Option<Vec<Effect>> {
        let failure = toast::failure_in(action)?;
        match failure.code.as_str() {
            // The approval was revoked, whichever request noticed it first; a guest has nothing to lose.
            "device_not_approved" if matches!(self.session, Session::Guest) => Some(Vec::new()),
            "device_not_approved" => Some(self.return_to_guest(toast::failure_text(failure))),
            "invalid_signature" if matches!(self.session, Session::Restoring) => {
                Some(self.return_to_guest(toast::failure_text(failure)))
            }
            _ => None,
        }
    }

    /// Drops everything the account had loaded, since the server no longer vouches for it.
    fn return_to_guest(&mut self, toast_text: String) -> Vec<Effect> {
        self.session = Session::Guest;
        self.feeds = FeedKey::ALL
            .into_iter()
            .map(|k| (k, Feed::default()))
            .collect();
        self.threads.clear();
        self.profiles.clear();
        self.connections.clear();
        self.notifications = NotificationsState::default();
        self.settings = SettingsState::default();
        self.me_pending = None;
        self.last_me_fetch = None;
        self.last_notif_req = None;
        self.restore_failures = 0;
        self.history.clear();
        self.g_pending = false;
        self.focus = Pane::Main;
        self.show(Screen::Log(LogTab::Latest));
        self.modal = Some(Modal::SignIn);
        self.show_toast(ToastKind::Error, toast_text);
        self.reconcile_scroll();
        vec![Effect::UseGuestClient]
    }

    /// Fetches for the screen the user is on without moving them, since they may have left the first one.
    pub(super) fn finish_restoring(&mut self, req: ReqId, response: &MeResponse) -> Vec<Effect> {
        if self.me_pending != Some(req) {
            return Vec::new();
        }
        self.me_pending = None;
        self.restore_failures = 0;
        self.session = Session::SignedIn(Me::from(response));
        self.entry_fetch()
    }

    /// Counts a failed attempt and starts the retry delay from now, not from when the request went out.
    pub(super) fn restore_failed(&mut self, req: ReqId) {
        if self.me_pending == Some(req) && matches!(self.session, Session::Restoring) {
            self.restore_failures += 1;
            self.last_me_fetch = Some(self.now);
        }
    }

    /// Seconds to wait before asking again; a session still being confirmed backs off after each failure.
    pub(super) fn me_wait_secs(&self) -> i64 {
        if !matches!(self.session, Session::Restoring) {
            return ME_INTERVAL_SECS;
        }
        match self.restore_failures {
            0 | 1 => 5,
            2 => 10,
            3 => 30,
            _ => 60,
        }
    }
}
