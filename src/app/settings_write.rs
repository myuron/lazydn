use std::collections::HashMap;

use super::people::UserWriteKind;
use super::toast;
use super::writes::write_failure_text;
use super::{App, Me, Modal, Session, SettingsTab, ToastKind};
use crate::action::{Effect, Failure, MeWrite};
use crate::dialogs::AccountDelete;
use crate::feed::ReqId;
use crate::keymap::SettingsText;
use crate::model::{LaneSetting, MeResponse, User};
use crate::settings::{
    CUSTOM_CHOICE, HEX_DIGITS, LANE_CHOICES, ProfileRow, parse_hex_color, safety_layout,
    saved_lane_choice,
};

const ROWS: [ProfileRow; 4] = [
    ProfileRow::Sync,
    ProfileRow::Lane,
    ProfileRow::Pasteis,
    ProfileRow::Delete,
];
const USERNAME_MAX: usize = 64;
const SYNC_RATE_TEXT: &str = "You just synced. Try again in a minute.";
const SYNC_FAILED_TEXT: &str = "Could not sync from GitHub. Try again in a minute.";
const DELETE_UNCONFIRMED_TEXT: &str =
    "could not confirm the deletion — check dev-null.sh before trying again";

/// A person taken off a Safety list while the server is asked to release them.
#[derive(Debug, Clone)]
pub(super) struct Release {
    generation: u32,
    index: usize,
    user: User,
}

pub(super) type Releases = HashMap<(UserWriteKind, String), Release>;

fn sync_failure_text(failure: &Failure) -> String {
    let said = |fixed: &str| {
        failure
            .message
            .as_deref()
            .map(toast::clean)
            .filter(|m| !m.is_empty())
            .unwrap_or_else(|| fixed.to_string())
    };
    match failure.code.as_str() {
        "rate_limited" => said(SYNC_RATE_TEXT),
        "github_sync_failed" => said(SYNC_FAILED_TEXT),
        // Without a body the message is only the reason phrase of the status.
        "http_429" => SYNC_RATE_TEXT.to_string(),
        "http_503" => SYNC_FAILED_TEXT.to_string(),
        _ => write_failure_text(failure),
    }
}

/// Settings: the writes of the Profile tab and the selection of the Safety tab.
impl App {
    fn me(&self) -> Option<&Me> {
        match &self.session {
            Session::SignedIn(me) => Some(me),
            _ => None,
        }
    }

    /// Whether keys go into the hex field.
    pub fn settings_typing(&self) -> bool {
        self.on_settings() && self.settings.hex_input.is_some()
    }

    fn begin_me_write(&mut self, kind: MeWrite) -> Option<ReqId> {
        if self.me_writes.contains_key(&kind) {
            return None;
        }
        let req = self.new_req();
        self.me_writes.insert(kind, req);
        Some(req)
    }

    pub(super) fn forget_settings_writes(&mut self) {
        self.me_writes.clear();
        self.releases.clear();
        self.account_delete = None;
        if self.modal == Some(Modal::ConfirmAccountDelete) {
            self.modal = None;
        }
    }

    /// The Profile tab of a screen that can write; a guest is asked to sign in.
    fn profile_write_gate(&mut self) -> Result<(), Vec<Effect>> {
        if !self.on_settings() || self.settings.tab != SettingsTab::Profile {
            return Err(Vec::new());
        }
        match self.session_gate() {
            Some(early) => Err(early),
            None => Ok(()),
        }
    }

    // --- movement -----------------------------------------------------------------------------

    pub(super) fn settings_move(&mut self, step: i32) {
        match self.settings.tab {
            SettingsTab::Profile => {
                let at = ROWS
                    .iter()
                    .position(|r| *r == self.settings.profile_row)
                    .unwrap_or(0);
                let next = at.saturating_add_signed(step as isize).min(ROWS.len() - 1);
                self.settings.profile_row = ROWS[next];
            }
            SettingsTab::Safety => {
                let at = self.settings.safety_selected;
                self.settings_select(at.saturating_add_signed(step as isize));
            }
            SettingsTab::Appearance => {}
        }
    }

    pub(super) fn settings_select(&mut self, index: usize) {
        if self.settings.tab != SettingsTab::Safety {
            return;
        }
        let total = safety_layout(&self.settings).user_rows.len();
        self.settings.safety_selected = index.min(total.saturating_sub(1));
    }

    /// Keeps the selection inside the lists and in view; the first person of a list brings its heading along.
    pub(super) fn reconcile_settings_scroll(&mut self) {
        let layout = safety_layout(&self.settings);
        let total = layout.user_rows.len();
        self.settings.safety_selected = self.settings.safety_selected.min(total.saturating_sub(1));
        if self.settings.tab != SettingsTab::Safety {
            return;
        }
        let Some(main) = self.main_area() else {
            return;
        };
        // The border and the three rows of the tabs are not part of the body.
        let view = usize::from(main.height.saturating_sub(5));
        let mut scroll = usize::from(self.settings.safety_scroll);
        if let Some(row) = layout.user_rows.get(self.settings.safety_selected) {
            let top = if layout.first_of_list[self.settings.safety_selected] {
                row - 1
            } else {
                *row
            };
            scroll = crate::feed::ensure_visible(top, row - top + 1, scroll, view);
        }
        scroll = scroll.min(layout.total.saturating_sub(view));
        self.settings.safety_scroll = u16::try_from(scroll).unwrap_or(u16::MAX);
    }

    pub(super) fn settings_step(&mut self, step: i8) {
        if !self.on_settings()
            || self.settings.tab != SettingsTab::Profile
            || self.settings.profile_row != ProfileRow::Lane
            || self.settings.hex_input.is_some()
        {
            return;
        }
        let Some(saved) = self.me().map(|me| saved_lane_choice(me.preferred_lane)) else {
            return;
        };
        let at = self.settings.lane_cursor.unwrap_or(saved);
        let next = at
            .saturating_add_signed(isize::from(step))
            .min(LANE_CHOICES - 1);
        self.settings.lane_cursor = Some(next);
    }

    // --- Profile tab ----------------------------------------------------------------------------

    /// `Enter` on Settings: acts on the focused row, or on the selected person of the Safety tab.
    pub(super) fn settings_enter(&mut self) -> Vec<Effect> {
        match self.settings.tab {
            SettingsTab::Profile => match self.settings.profile_row {
                ProfileRow::Sync => self.sync_github(),
                ProfileRow::Lane => self.lane_enter(),
                ProfileRow::Pasteis => self.toggle_pasteis(),
                ProfileRow::Delete => self.open_account_delete(),
            },
            SettingsTab::Safety => self.release_selected(),
            SettingsTab::Appearance => Vec::new(),
        }
    }

    pub(super) fn guarded_settings_toggle(&mut self) -> Vec<Effect> {
        if self.on_settings()
            && self.settings.tab == SettingsTab::Profile
            && self.settings.profile_row == ProfileRow::Pasteis
        {
            return self.toggle_pasteis();
        }
        Vec::new()
    }

    pub(super) fn sync_github(&mut self) -> Vec<Effect> {
        if let Err(early) = self.profile_write_gate() {
            return early;
        }
        match self.begin_me_write(MeWrite::Sync) {
            Some(req) => vec![Effect::SyncGithub { req }],
            None => Vec::new(),
        }
    }

    fn lane_enter(&mut self) -> Vec<Effect> {
        if let Err(early) = self.profile_write_gate() {
            return early;
        }
        let Some(saved) = self.me().map(|me| saved_lane_choice(me.preferred_lane)) else {
            return Vec::new();
        };
        let choice = self.settings.lane_cursor.unwrap_or(saved);
        if choice == CUSTOM_CHOICE {
            if self.me_writes.contains_key(&MeWrite::Lane) {
                return Vec::new();
            }
            self.settings.hex_input = Some(String::new());
            return Vec::new();
        }
        let lane = match choice {
            0 => LaneSetting::Automatic,
            n => LaneSetting::Palette(u8::try_from(n - 1).unwrap_or(0)),
        };
        self.send_lane(lane)
    }

    fn send_lane(&mut self, lane: LaneSetting) -> Vec<Effect> {
        match self.begin_me_write(MeWrite::Lane) {
            Some(req) => vec![Effect::SetLane { lane, req }],
            None => Vec::new(),
        }
    }

    fn toggle_pasteis(&mut self) -> Vec<Effect> {
        if let Err(early) = self.profile_write_gate() {
            return early;
        }
        let Some(enabled) = self.me().map(|me| !me.pasteis_mode) else {
            return Vec::new();
        };
        match self.begin_me_write(MeWrite::PasteisMode) {
            Some(req) => vec![Effect::SetPasteisMode { enabled, req }],
            None => Vec::new(),
        }
    }

    pub(super) fn me_write_done(
        &mut self,
        kind: MeWrite,
        req: ReqId,
        response: &MeResponse,
    ) -> Vec<Effect> {
        if self.me_writes.get(&kind) != Some(&req) {
            return Vec::new();
        }
        self.me_writes.remove(&kind);
        let Session::SignedIn(me) = &mut self.session else {
            return Vec::new();
        };
        let unread = me.unread;
        let previous_handle = me.handle.to_ascii_lowercase();
        *me = Me::from(response);
        me.unread = unread;
        let (id, lane, handle) = (me.id.clone(), me.lane, me.handle.clone());
        match kind {
            MeWrite::Lane => {
                self.settings.lane_cursor = None;
                self.settings.hex_input = None;
                self.for_each_commit(|c| {
                    if c.author.id == id {
                        c.author.lane = Some(lane);
                    }
                });
            }
            MeWrite::Sync => {
                // The name may have changed, and the profile is kept under it.
                if let Some(old) = self.profiles.remove(&previous_handle) {
                    let mut state = old;
                    let req = self.new_req();
                    state.user_pending = Some(req);
                    self.profiles.insert(handle.to_ascii_lowercase(), state);
                    return vec![Effect::FetchUser {
                        username: handle,
                        req,
                    }];
                }
            }
            MeWrite::PasteisMode => {}
        }
        Vec::new()
    }

    pub(super) fn me_write_failed(&mut self, kind: MeWrite, req: ReqId, failure: &Failure) {
        if self.me_writes.get(&kind) != Some(&req) {
            return;
        }
        self.me_writes.remove(&kind);
        let text = match kind {
            MeWrite::Sync => sync_failure_text(failure),
            MeWrite::Lane | MeWrite::PasteisMode => write_failure_text(failure),
        };
        self.show_toast(ToastKind::Error, text);
    }

    // --- text ----------------------------------------------------------------------------------

    pub(super) fn settings_text(&mut self, text: SettingsText) -> Vec<Effect> {
        if self.account_delete.is_some() && self.modal == Some(Modal::ConfirmAccountDelete) {
            return self.delete_text(text);
        }
        if self.on_settings() && self.settings.hex_input.is_some() {
            return self.hex_text(text);
        }
        Vec::new()
    }

    fn hex_text(&mut self, text: SettingsText) -> Vec<Effect> {
        match text {
            SettingsText::Insert(c) => {
                if let Some(input) = self.settings.hex_input.as_mut()
                    && c.is_ascii_hexdigit()
                    && input.len() < HEX_DIGITS
                {
                    input.push(c.to_ascii_lowercase());
                }
            }
            SettingsText::Backspace => {
                if let Some(input) = self.settings.hex_input.as_mut() {
                    input.pop();
                }
            }
            SettingsText::Cancel => self.settings.hex_input = None,
            SettingsText::Submit => {
                let rgb = self.settings.hex_input.as_deref().and_then(parse_hex_color);
                if let Some(rgb) = rgb {
                    return self.send_lane(LaneSetting::Color(rgb));
                }
            }
        }
        Vec::new()
    }

    // --- account deletion -------------------------------------------------------------------------

    pub(super) fn open_account_delete(&mut self) -> Vec<Effect> {
        if let Err(early) = self.profile_write_gate() {
            return early;
        }
        self.account_delete = Some(AccountDelete {
            input: String::new(),
            sending: self.pending_account_deletion.is_some(),
        });
        self.modal = Some(Modal::ConfirmAccountDelete);
        Vec::new()
    }

    fn delete_text(&mut self, text: SettingsText) -> Vec<Effect> {
        match text {
            SettingsText::Cancel => self.close_modal(),
            SettingsText::Insert(c) => {
                if let Some(d) = self.account_delete.as_mut()
                    && !d.sending
                    && !c.is_control()
                    && d.input.chars().count() < USERNAME_MAX
                {
                    d.input.push(c);
                }
            }
            SettingsText::Backspace => {
                if let Some(d) = self.account_delete.as_mut()
                    && !d.sending
                {
                    d.input.pop();
                }
            }
            SettingsText::Submit => {
                // A dialog that outlived its tab or its account must not delete anything.
                if !self.on_settings()
                    || self.settings.tab != SettingsTab::Profile
                    || self.me().is_none()
                {
                    self.close_modal();
                    return Vec::new();
                }
                let handle = self.me().map(|me| me.handle.clone());
                let ready = self
                    .account_delete
                    .as_ref()
                    .is_some_and(|d| !d.sending && handle.as_deref() == Some(d.input.as_str()));
                if !ready {
                    return Vec::new();
                }
                let id = self.account_id().map(String::from).unwrap_or_default();
                let req = self.new_req();
                if let Some(d) = self.account_delete.as_mut() {
                    d.sending = true;
                }
                self.pending_account_deletion = Some((req, id));
                return vec![Effect::DeleteAccount { req }];
            }
        }
        Vec::new()
    }

    /// Whether the answer of a deletion still concerns this session: a guest, or the same account.
    fn deletion_applies(&self, id: &str) -> bool {
        match &self.session {
            Session::Guest => true,
            Session::SignedIn(me) => me.id == id,
            Session::Restoring => false,
        }
    }

    /// Takes the record of the deletion out when this action answers it, whatever else the action does.
    pub(super) fn take_deletion_answer(
        &mut self,
        action: &crate::action::Action,
    ) -> Option<String> {
        use crate::action::Action;
        let (Action::AccountDeleted { req } | Action::AccountDeleteFailed { req, .. }) = action
        else {
            return None;
        };
        match self.pending_account_deletion.take() {
            Some((out, id)) if out == *req => Some(id),
            other => {
                self.pending_account_deletion = other;
                None
            }
        }
    }

    pub(super) fn account_deleted(&mut self, answered: Option<String>) -> Vec<Effect> {
        let Some(id) = answered else {
            return Vec::new();
        };
        if !self.deletion_applies(&id) {
            return Vec::new();
        }
        let effects = if matches!(self.session, Session::Guest) {
            Vec::new()
        } else {
            self.drop_to_guest()
        };
        if self.modal == Some(Modal::SignIn) {
            self.modal = None;
        }
        self.show_toast(ToastKind::Info, "Account deleted".into());
        effects
    }

    pub(super) fn account_delete_failed(&mut self, answered: Option<String>, failure: &Failure) {
        let Some(id) = answered else {
            return;
        };
        if !self.deletion_applies(&id) {
            return;
        }
        let text = if failure.code == "network" {
            DELETE_UNCONFIRMED_TEXT.to_string()
        } else {
            write_failure_text(failure)
        };
        self.show_toast(ToastKind::Error, text);
        if let Some(dialog) = self.account_delete.as_mut() {
            dialog.sending = false;
            dialog.input.clear();
        }
    }

    // --- Safety tab ---------------------------------------------------------------------------------

    pub(super) fn release_selected(&mut self) -> Vec<Effect> {
        if !self.on_settings() || self.settings.tab != SettingsTab::Safety {
            return Vec::new();
        }
        if let Some(early) = self.session_gate() {
            return early;
        }
        let selected = self.settings.safety_selected;
        let muted = self.settings.muted.users.len();
        let (kind, index) = if selected < muted {
            (UserWriteKind::Mute, selected)
        } else {
            (UserWriteKind::Block, selected - muted)
        };
        let list = match kind {
            UserWriteKind::Mute => &self.settings.muted,
            _ => &self.settings.blocked,
        };
        let Some(username) = list.users.get(index).map(|u| u.username.clone()) else {
            return Vec::new();
        };
        let Some(req) = self.begin_user_write(kind, &username) else {
            return Vec::new();
        };
        let list = match kind {
            UserWriteKind::Mute => &mut self.settings.muted,
            _ => &mut self.settings.blocked,
        };
        let generation = list.generation;
        let user = list.users.remove(index);
        self.releases.insert(
            (kind, username.to_ascii_lowercase()),
            Release {
                generation,
                index,
                user,
            },
        );
        match kind {
            UserWriteKind::Mute => vec![Effect::MuteUser {
                username,
                on: false,
                req,
            }],
            _ => vec![Effect::BlockUser {
                username,
                on: false,
                req,
            }],
        }
    }

    /// The server took the release, so there is nothing left to put back.
    pub(super) fn drop_release(&mut self, kind: UserWriteKind, username: &str) {
        self.releases.remove(&(kind, username.to_ascii_lowercase()));
    }

    /// The server refused: the person returns to their place, or to the end if the list was replaced meanwhile.
    pub(super) fn restore_release(&mut self, kind: UserWriteKind, username: &str) {
        let Some(release) = self.releases.remove(&(kind, username.to_ascii_lowercase())) else {
            return;
        };
        let muted_len = self.settings.muted.users.len();
        let list = match kind {
            UserWriteKind::Mute => &mut self.settings.muted,
            _ => &mut self.settings.blocked,
        };
        let at = if list.generation == release.generation {
            release.index.min(list.users.len())
        } else if list
            .users
            .iter()
            .any(|u| u.username.eq_ignore_ascii_case(username))
        {
            return;
        } else {
            list.users.len()
        };
        list.users.insert(at, release.user);
        let base = if kind == UserWriteKind::Mute {
            0
        } else {
            muted_len
        };
        self.settings.safety_selected = base + at;
    }
}
