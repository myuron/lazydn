use super::{App, Screen, SettingsTab};
use crate::action::Effect;
use crate::feed::{LoadState, Pending, ReqId};
use crate::model::{Page, User};
use crate::settings::UserList;

#[derive(Clone, Copy)]
enum Which {
    Muted,
    Blocked,
}

impl UserList {
    /// True when the first page was never fetched, or the only attempt so far failed.
    fn needs_entry_fetch(&self) -> bool {
        let failed_first = self.users.is_empty() && matches!(self.load, LoadState::Failed { .. });
        self.pending.is_none() && (!self.loaded_once || failed_first)
    }

    fn apply_loaded(&mut self, req: ReqId, page: Page<User>) {
        if self.pending.as_ref().is_none_or(|p| p.req != req) {
            return;
        }
        self.users = page.data;
        self.next_cursor = page.meta.next_cursor;
        self.pending = None;
        self.load = LoadState::Idle;
        self.loaded_once = true;
    }

    fn apply_failed(&mut self, req: ReqId, code: String) {
        if self.pending.as_ref().is_none_or(|p| p.req != req) {
            return;
        }
        self.pending = None;
        self.load = LoadState::Failed {
            code,
            append: false,
        };
    }
}

impl App {
    fn list_mut(&mut self, which: Which) -> &mut UserList {
        match which {
            Which::Muted => &mut self.settings.muted,
            Which::Blocked => &mut self.settings.blocked,
        }
    }

    fn settings_request(&mut self, which: Which) -> Effect {
        let req = self.new_req();
        let list = self.list_mut(which);
        list.pending = Some(Pending { req, append: false });
        list.load = LoadState::Idle;
        match which {
            Which::Muted => Effect::FetchMuted { cursor: None, req },
            Which::Blocked => Effect::FetchBlocked { cursor: None, req },
        }
    }

    pub(super) fn on_settings(&self) -> bool {
        matches!(self.screen, Screen::Settings)
    }

    /// Only the Safety tab has data to fetch.
    pub(super) fn settings_entry(&mut self) -> Vec<Effect> {
        if self.settings.tab != SettingsTab::Safety {
            return Vec::new();
        }
        let mut effects = Vec::new();
        for which in [Which::Muted, Which::Blocked] {
            if self.list_mut(which).needs_entry_fetch() {
                effects.push(self.settings_request(which));
            }
        }
        effects
    }

    pub(super) fn settings_switch_tab(&mut self, tab: SettingsTab) -> Vec<Effect> {
        if !self.on_settings() {
            return Vec::new();
        }
        self.settings.tab = tab;
        self.settings_entry()
    }

    pub(super) fn settings_refresh(&mut self) -> Vec<Effect> {
        if self.settings.tab != SettingsTab::Safety {
            return Vec::new();
        }
        vec![
            self.settings_request(Which::Muted),
            self.settings_request(Which::Blocked),
        ]
    }

    pub(super) fn apply_muted_loaded(&mut self, req: ReqId, page: Page<User>) {
        self.settings.muted.apply_loaded(req, page);
    }

    pub(super) fn apply_blocked_loaded(&mut self, req: ReqId, page: Page<User>) {
        self.settings.blocked.apply_loaded(req, page);
    }

    pub(super) fn apply_muted_failed(&mut self, req: ReqId, code: String) {
        self.settings.muted.apply_failed(req, code);
    }

    pub(super) fn apply_blocked_failed(&mut self, req: ReqId, code: String) {
        self.settings.blocked.apply_failed(req, code);
    }
}
