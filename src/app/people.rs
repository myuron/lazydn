use super::removal::remove_from_feed;
use super::writes::write_failure_text;
use super::{App, ConnTab, Modal, Screen, Session, ToastKind};
use crate::action::{Effect, Failure};
use crate::feed::ReqId;
use crate::model::User;

/// What a request about a person does; at most one of each kind is out per person.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum UserWriteKind {
    Follow,
    Mute,
    Block,
}

const BLOCKED: &str = "user_blocked";

/// Following, muting and blocking people.
impl App {
    /// The username in the confirmation that waits for `y`.
    pub fn block_confirm(&self) -> Option<&str> {
        self.block_confirm.as_deref()
    }

    pub(super) fn close_block_confirm(&mut self) {
        self.block_confirm = None;
    }

    /// Forgets what was out; the answers that follow find nothing.
    pub(super) fn forget_user_writes(&mut self) {
        self.user_writes.clear();
        self.block_confirm = None;
        if self.modal == Some(Modal::ConfirmBlock) {
            self.modal = None;
        }
    }

    fn user_write_out(&self, kind: UserWriteKind, username: &str) -> bool {
        self.user_writes
            .contains_key(&(kind, username.to_ascii_lowercase()))
    }

    /// A block that is out holds back the follow and the mute of the same person.
    pub(super) fn begin_user_write(
        &mut self,
        kind: UserWriteKind,
        username: &str,
    ) -> Option<ReqId> {
        let busy = self.user_write_out(kind, username)
            || (kind != UserWriteKind::Block
                && self.user_write_out(UserWriteKind::Block, username));
        if busy {
            return None;
        }
        let req = self.new_req();
        self.user_writes
            .insert((kind, username.to_ascii_lowercase()), req);
        Some(req)
    }

    fn end_user_write(&mut self, kind: UserWriteKind, username: &str, req: ReqId) -> bool {
        let slot = (kind, username.to_ascii_lowercase());
        if self.user_writes.get(&slot) != Some(&req) {
            return false;
        }
        self.user_writes.remove(&slot);
        true
    }

    // --- keys ---------------------------------------------------------------------------------

    pub(super) fn toggle_follow(&mut self) -> Vec<Effect> {
        if let Some(early) = self.session_gate() {
            return early;
        }
        match self.screen.clone() {
            Screen::Profile(name) => self.follow_profile(&name),
            Screen::Connections(name, tab) => self.follow_row(&name, tab),
            _ => Vec::new(),
        }
    }

    /// The loaded profile of someone else, as `(username, is_following, is_muting)`.
    fn other_profile(&self, name: &str) -> Option<(String, Option<bool>, Option<bool>)> {
        if self.is_own(name) {
            return None;
        }
        let state = self.profile(name).filter(|s| s.shows_list())?;
        let resp = state.user.as_ref()?;
        Some((
            resp.data.username.clone(),
            resp.is_following,
            resp.is_muting,
        ))
    }

    fn follow_profile(&mut self, name: &str) -> Vec<Effect> {
        let Some((username, following, _)) = self.other_profile(name) else {
            return Vec::new();
        };
        let on = !following.unwrap_or(false);
        let Some(req) = self.begin_user_write(UserWriteKind::Follow, &username) else {
            return Vec::new();
        };
        self.show_follow(&username, on, false);
        vec![Effect::FollowUser { username, on, req }]
    }

    fn follow_row(&mut self, name: &str, tab: ConnTab) -> Vec<Effect> {
        let row = self
            .connections(name, tab)
            .and_then(|s| s.users.get(s.selected))
            .map(|u| (u.username.clone(), u.is_followed));
        let Some((username, Some(followed))) = row else {
            return Vec::new();
        };
        if self.is_own(&username) {
            return Vec::new();
        }
        let Some(req) = self.begin_user_write(UserWriteKind::Follow, &username) else {
            return Vec::new();
        };
        self.show_follow(&username, !followed, false);
        vec![Effect::FollowUser {
            username,
            on: !followed,
            req,
        }]
    }

    pub(super) fn toggle_mute(&mut self) -> Vec<Effect> {
        if let Some(early) = self.session_gate() {
            return early;
        }
        let Screen::Profile(name) = self.screen.clone() else {
            return Vec::new();
        };
        let Some((username, _, muting)) = self.other_profile(&name) else {
            return Vec::new();
        };
        let on = !muting.unwrap_or(false);
        let Some(req) = self.begin_user_write(UserWriteKind::Mute, &username) else {
            return Vec::new();
        };
        self.show_mute(&username, on, false);
        vec![Effect::MuteUser { username, on, req }]
    }

    pub(super) fn block_key(&mut self) -> Vec<Effect> {
        if let Some(early) = self.session_gate() {
            return early;
        }
        let Screen::Profile(name) = self.screen.clone() else {
            return Vec::new();
        };
        if self.is_own(&name) {
            return Vec::new();
        }
        let Some(state) = self.profile(&name) else {
            return Vec::new();
        };
        match state.error.as_deref() {
            Some(BLOCKED) if state.user_pending.is_some() => Vec::new(),
            Some(BLOCKED) => {
                let username = state
                    .blocked_user
                    .as_ref()
                    .map_or(name, |u| u.username.clone());
                self.send_block(username, false)
            }
            Some(_) => Vec::new(),
            None => {
                let Some(user) = state.user.as_ref() else {
                    return Vec::new();
                };
                let username = user.data.username.clone();
                if self.user_write_out(UserWriteKind::Block, &username) {
                    return Vec::new();
                }
                self.block_confirm = Some(username);
                self.modal = Some(Modal::ConfirmBlock);
                Vec::new()
            }
        }
    }

    pub(super) fn confirm_block(&mut self) -> Vec<Effect> {
        let Some(username) = self.block_confirm.take() else {
            return Vec::new();
        };
        if self.modal == Some(Modal::ConfirmBlock) {
            self.modal = None;
        }
        self.send_block(username, true)
    }

    fn send_block(&mut self, username: String, on: bool) -> Vec<Effect> {
        match self.begin_user_write(UserWriteKind::Block, &username) {
            Some(req) => vec![Effect::BlockUser { username, on, req }],
            None => Vec::new(),
        }
    }

    // --- copies -------------------------------------------------------------------------------

    /// Shows the follow of `username` in every copy. With `undo`, `on` is what a press set and is taken back, in
    /// the copies that still hold it; the counts of you go back whatever the copies say.
    fn show_follow(&mut self, username: &str, on: bool, undo: bool) {
        let lc = username.to_ascii_lowercase();
        let target = if undo { !on } else { on };
        let step = |n: &mut Option<u64>| {
            if let Some(c) = n {
                *c = if target {
                    c.saturating_add(1)
                } else {
                    c.saturating_sub(1)
                };
            }
        };
        if let Some(resp) = self.profiles.get_mut(&lc).and_then(|s| s.user.as_mut())
            && if undo {
                resp.is_following == Some(on)
            } else {
                resp.is_following.unwrap_or(false) != on
            }
        {
            resp.is_following = Some(target);
            step(&mut resp.data.followers_count);
        }
        for state in self.connections.values_mut() {
            for u in state
                .users
                .iter_mut()
                .filter(|u| u.username.eq_ignore_ascii_case(&lc))
            {
                let held = if undo { on } else { !on };
                if u.is_followed == Some(held) {
                    u.is_followed = Some(target);
                }
            }
        }
        self.step_following(target);
    }

    /// Your following count, here and on your own profile, moves one step.
    fn step_following(&mut self, up: bool) {
        let Session::SignedIn(me) = &mut self.session else {
            return;
        };
        me.following = if up {
            me.following.saturating_add(1)
        } else {
            me.following.saturating_sub(1)
        };
        let handle = me.handle.to_ascii_lowercase();
        if let Some(n) = self
            .profiles
            .get_mut(&handle)
            .and_then(|s| s.user.as_mut())
            .and_then(|u| u.data.following_count.as_mut())
        {
            *n = if up {
                n.saturating_add(1)
            } else {
                n.saturating_sub(1)
            };
        }
    }

    fn count_followers_down(&mut self) {
        let Session::SignedIn(me) = &mut self.session else {
            return;
        };
        me.followers = me.followers.saturating_sub(1);
        let handle = me.handle.to_ascii_lowercase();
        if let Some(n) = self
            .profiles
            .get_mut(&handle)
            .and_then(|s| s.user.as_mut())
            .and_then(|u| u.data.followers_count.as_mut())
        {
            *n = n.saturating_sub(1);
        }
    }

    fn show_mute(&mut self, username: &str, on: bool, undo: bool) {
        let lc = username.to_ascii_lowercase();
        if let Some(resp) = self.profiles.get_mut(&lc).and_then(|s| s.user.as_mut())
            && (!undo || resp.is_muting == Some(on))
        {
            resp.is_muting = Some(if undo { !on } else { on });
        }
    }

    // --- answers ------------------------------------------------------------------------------

    pub(super) fn follow_done(&mut self, username: &str, req: ReqId) {
        self.end_user_write(UserWriteKind::Follow, username, req);
    }

    pub(super) fn follow_failed(
        &mut self,
        username: &str,
        on: bool,
        req: ReqId,
        failure: &Failure,
    ) {
        if self.end_user_write(UserWriteKind::Follow, username, req) {
            self.show_follow(username, on, true);
            self.show_toast(ToastKind::Error, write_failure_text(failure));
        }
    }

    pub(super) fn mute_done(&mut self, username: &str, req: ReqId, is_muting: bool) {
        if !self.end_user_write(UserWriteKind::Mute, username, req) {
            return;
        }
        self.drop_release(UserWriteKind::Mute, username);
        let lc = username.to_ascii_lowercase();
        if let Some(resp) = self.profiles.get_mut(&lc).and_then(|s| s.user.as_mut()) {
            resp.is_muting = Some(is_muting);
        }
        self.settings.muted.stale = true;
    }

    pub(super) fn mute_failed(&mut self, username: &str, on: bool, req: ReqId, failure: &Failure) {
        if self.end_user_write(UserWriteKind::Mute, username, req) {
            self.restore_release(UserWriteKind::Mute, username);
            self.show_mute(username, on, true);
            self.show_toast(ToastKind::Error, write_failure_text(failure));
        }
    }

    pub(super) fn block_done(
        &mut self,
        username: &str,
        on: bool,
        req: ReqId,
        is_blocking: bool,
        data: User,
    ) -> Vec<Effect> {
        if !self.end_user_write(UserWriteKind::Block, username, req) || is_blocking != on {
            return Vec::new();
        }
        self.drop_release(UserWriteKind::Block, username);
        self.settings.blocked.stale = true;
        if on {
            self.apply_block(username, data);
            return Vec::new();
        }
        if self.profile(username).is_some() {
            return self.profile_requests(username);
        }
        Vec::new()
    }

    pub(super) fn block_failed(&mut self, username: &str, req: ReqId, failure: &Failure) {
        if self.end_user_write(UserWriteKind::Block, username, req) {
            self.restore_release(UserWriteKind::Block, username);
            self.show_toast(ToastKind::Error, write_failure_text(failure));
        }
    }

    /// The follow goes in both directions, the person's commits leave the Log tabs and Saved, and the profile
    /// shows the block. Other screens show them until they are loaded again.
    fn apply_block(&mut self, username: &str, data: User) {
        let lc = username.to_ascii_lowercase();
        let flags = self
            .profiles
            .get(&lc)
            .and_then(|s| s.user.as_ref())
            .map(|r| (r.is_following == Some(true), r.follows_you == Some(true)));
        if let Some((following, follows_you)) = flags {
            if following {
                self.step_following(false);
            }
            if follows_you {
                self.count_followers_down();
            }
        }
        for state in self.connections.values_mut() {
            for u in state
                .users
                .iter_mut()
                .filter(|u| u.username.eq_ignore_ascii_case(&lc) && u.is_followed == Some(true))
            {
                u.is_followed = Some(false);
            }
        }
        if self
            .user_writes
            .contains_key(&(UserWriteKind::Mute, lc.clone()))
        {
            self.settings.muted.stale = true;
        }
        self.user_writes
            .retain(|(kind, name), _| *name != lc || matches!(kind, UserWriteKind::Block));
        for feed in self.feeds.values_mut() {
            let ids: Vec<String> = feed
                .items
                .iter()
                .chain(&feed.suggestions)
                .filter(|c| c.author.id == data.id)
                .map(|c| c.id.clone())
                .collect();
            for id in ids {
                remove_from_feed(feed, &id);
            }
        }
        if let Some(state) = self.profiles.get_mut(&lc) {
            state.user_pending = None;
            state.error = Some(BLOCKED.into());
            state.blocked_user = Some(data);
        }
    }
}
