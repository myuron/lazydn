use super::{App, Me, Screen, Session};
use crate::action::Effect;
use crate::feed::{LoadState, PREFETCH_MARGIN, Pending, ReqId, near_end, settle_scroll};
use crate::model::{
    GroupPage, Lane, MeResponse, Notification, NotificationType, NotificationsPage, ThreadItem,
};
use crate::notifications::{Filter, GroupList};

/// Seconds between two requests for the current user.
pub(super) const ME_INTERVAL_SECS: i64 = 30;

impl From<&MeResponse> for Me {
    fn from(response: &MeResponse) -> Self {
        let current = &response.data;
        let user = &current.user;
        let count = |n: Option<u64>| n.map_or(0, |n| u32::try_from(n).unwrap_or(u32::MAX));
        Self {
            id: user.id.clone(),
            name: user.name.clone().unwrap_or_else(|| user.username.clone()),
            handle: user.username.clone(),
            commits: count(user.commits_count),
            following: count(user.following_count),
            followers: count(user.followers_count),
            pasteis_left: current.pasteis_remaining,
            // The API does not report the daily allowance.
            pasteis_max: 10,
            bio: user.bio.clone().unwrap_or_default(),
            unread: response.unread_notifications_count,
            pasteis_mode: current.pasteis_mode,
            lane: user.lane.unwrap_or(Lane::Index(current.automatic_lane)),
            preferred_lane: current.preferred_lane,
            automatic_lane: current.automatic_lane,
            pasteis_resets_at: current.pasteis_resets_at,
            pasteis_received: current.pasteis_received,
            github_synced_at: current.github_synced_at,
        }
    }
}

/// Current user and Notifications screen: requests, selection and scrolling.
impl App {
    pub(super) fn on_notifications(&self) -> bool {
        matches!(self.screen, Screen::Notifications)
    }

    /// Notifications is fetched on every entry, which is the call that marks everything read.
    pub(super) fn enter_fetch(&mut self) -> Vec<Effect> {
        if self.on_notifications() {
            return vec![self.request_notifications(None, false)];
        }
        self.entry_fetch()
    }

    // --- current user -------------------------------------------------------

    pub(super) fn request_me(&mut self) -> Option<Effect> {
        if matches!(self.session, Session::Guest) || self.me_pending.is_some() {
            return None;
        }
        let req = self.new_req();
        self.me_pending = Some(req);
        self.last_me_fetch = Some(self.now);
        Some(Effect::FetchMe { req })
    }

    pub(super) fn poll_me(&mut self) -> Option<Effect> {
        let wait = self.me_wait_secs();
        let due = self
            .last_me_fetch
            .is_some_and(|t| (self.now - t).whole_seconds() >= wait);
        if due { self.request_me() } else { None }
    }

    pub(super) fn apply_me_loaded(&mut self, req: ReqId, response: &MeResponse) {
        if self.me_pending != Some(req) {
            return;
        }
        self.me_pending = None;
        // Everything was marked read after this request went out, so its count is out of date.
        let outdated = self.last_notif_req.is_some_and(|last| req.0 < last.0);
        let Session::SignedIn(me) = &mut self.session else {
            return;
        };
        let unread = me.unread;
        *me = Me::from(response);
        if outdated {
            me.unread = unread;
            return;
        }
        let count = response.unread_notifications_count;
        if count >= 1 && matches!(self.screen, Screen::Notifications) {
            self.notifications.new_count = count;
        }
    }

    // --- notifications ------------------------------------------------------

    pub(super) fn request_notifications(&mut self, cursor: Option<String>, append: bool) -> Effect {
        let req = self.new_req();
        self.last_notif_req = Some(req);
        let state = &mut self.notifications;
        state.pending = Some(Pending { req, append });
        state.load = LoadState::Idle;
        Effect::FetchNotifications {
            types: state.filters.types(),
            cursor,
            req,
        }
    }

    pub(super) fn apply_notifications_loaded(
        &mut self,
        req: ReqId,
        page: NotificationsPage,
    ) -> Vec<Effect> {
        let append = match &self.notifications.pending {
            Some(p) if p.req == req => p.append,
            _ => return Vec::new(),
        };
        let state = &mut self.notifications;
        let received = page.data.len();
        let shown: Vec<Notification> = page
            .data
            .into_iter()
            .filter(|n| n.kind != NotificationType::Unknown)
            .collect();
        let dropped = shown.len() < received;
        if append {
            for n in shown {
                match state.items.iter().position(|i| i.group_id == n.group_id) {
                    Some(at) => {
                        // A group that was asked for under the old id would never be answered.
                        if state.items[at].id != n.id {
                            state.groups.remove(&n.group_id);
                            state.expanded.remove(&n.group_id);
                        }
                        state.items[at] = n;
                    }
                    None => state.items.push(n),
                }
            }
        } else {
            state.items = shown;
            state.selected = 0;
            state.scroll = 0;
            state.new_count = 0;
            state.groups.clear();
            state.expanded.clear();
            state.read_at = page.read_at;
        }
        state.next_cursor = page.meta.next_cursor;
        state.pending = None;
        state.load = LoadState::Idle;
        state.loaded_once = true;
        if let Session::SignedIn(me) = &mut self.session {
            me.unread = 0;
        }
        // Nothing to move the selection over, so a page of hidden kinds would otherwise end the list.
        let starved = dropped && self.notifications.items.len() <= PREFETCH_MARGIN;
        if starved {
            self.notif_prefetch()
        } else {
            Vec::new()
        }
    }

    pub(super) fn apply_notifications_failed(&mut self, req: ReqId, code: String) -> bool {
        let append = match &self.notifications.pending {
            Some(p) if p.req == req => p.append,
            _ => return false,
        };
        self.notifications.pending = None;
        self.notifications.load = LoadState::Failed { code, append };
        true
    }

    // --- filters ------------------------------------------------------------

    pub(super) fn notif_toggle_filter(&mut self, filter: Filter) -> Vec<Effect> {
        if !self.on_notifications() {
            return Vec::new();
        }
        self.notifications.filters.toggle(filter);
        vec![self.request_notifications(None, false)]
    }

    pub(super) fn notif_toggle_all_filters(&mut self) -> Vec<Effect> {
        if !self.on_notifications() {
            return Vec::new();
        }
        let filters = &mut self.notifications.filters;
        let all_on = filters.all_on();
        filters.set_all(!all_on);
        vec![self.request_notifications(None, false)]
    }

    // --- selection ----------------------------------------------------------

    fn selected_notification(&self) -> Option<Notification> {
        self.notifications
            .items
            .get(self.notifications.selected)
            .cloned()
    }

    fn notif_prefetch(&mut self) -> Vec<Effect> {
        let state = &self.notifications;
        let near = state.items.is_empty() || near_end(state.selected, state.items.len());
        let append_failed = matches!(state.load, LoadState::Failed { append: true, .. });
        match state.next_cursor.clone() {
            Some(cursor) if near && state.pending.is_none() && !append_failed => {
                vec![self.request_notifications(Some(cursor), true)]
            }
            _ => Vec::new(),
        }
    }

    pub(super) fn notif_select(&mut self, index: usize) -> Vec<Effect> {
        let state = &mut self.notifications;
        if !state.items.is_empty() {
            state.selected = index.min(state.items.len() - 1);
        }
        self.notif_prefetch()
    }

    pub(super) fn notif_move(&mut self, step: i32) -> Vec<Effect> {
        let state = &self.notifications;
        if step < 0 && state.selected == 0 && state.new_count > 0 {
            // The news are already on their way; asking again would only supersede that request.
            if state.pending.as_ref().is_some_and(|p| !p.append) {
                return Vec::new();
            }
            // A failed append must not block this: the user asked for the newest items.
            return vec![self.request_notifications(None, false)];
        }
        let target = state.selected.saturating_add_signed(step as isize);
        self.notif_select(target)
    }

    pub(super) fn notif_open_selected(&mut self) -> Vec<Effect> {
        let Some(n) = self.selected_notification() else {
            return Vec::new();
        };
        let target = match n.kind {
            NotificationType::Followed => n.actor.map(|a| Screen::Profile(a.username)),
            _ => match n.commit {
                Some(ThreadItem::Commit(c)) => Some(Screen::Thread(c.id)),
                _ => None,
            },
        };
        target.map_or_else(Vec::new, |screen| self.open(screen))
    }

    pub(super) fn notif_open_author(&mut self) -> Vec<Effect> {
        let Some(n) = self.selected_notification() else {
            return Vec::new();
        };
        // The actor of a moderation result is the account itself.
        if matches!(
            n.kind,
            NotificationType::Approved | NotificationType::Rejected
        ) {
            return Vec::new();
        }
        n.actor
            .map_or_else(Vec::new, |a| self.open(Screen::Profile(a.username)))
    }

    // --- groups -------------------------------------------------------------

    pub(super) fn notif_toggle_group(&mut self) -> Vec<Effect> {
        let Some(n) = self.selected_notification() else {
            return Vec::new();
        };
        if n.event_count <= 1 {
            return Vec::new();
        }
        let state = &mut self.notifications;
        if state.expanded.remove(&n.group_id) {
            return Vec::new();
        }
        state.expanded.insert(n.group_id.clone());
        let cached = state
            .groups
            .get(&n.group_id)
            .is_some_and(|g| g.pending.is_some() || matches!(g.load, LoadState::Idle));
        if cached {
            return Vec::new();
        }
        let types = state.filters.types();
        let req = self.new_req();
        let group = self.notifications.groups.entry(n.group_id).or_default();
        group.pending = Some(req);
        group.load = LoadState::Idle;
        vec![Effect::FetchNotificationGroup {
            id: n.id,
            types,
            cursor: None,
            req,
        }]
    }

    /// The group key is looked up from the asked notification, since the response only knows its id.
    fn group_mut(&mut self, id: &str, req: ReqId) -> Option<&mut GroupList> {
        let group_id = self
            .notifications
            .items
            .iter()
            .find(|n| n.id == id)?
            .group_id
            .clone();
        self.notifications
            .groups
            .get_mut(&group_id)
            .filter(|g| g.pending == Some(req))
    }

    pub(super) fn apply_group_loaded(&mut self, id: &str, req: ReqId, page: GroupPage) {
        if let Some(group) = self.group_mut(id, req) {
            group.items = page.data;
            group.next_cursor = page.meta.next_cursor;
            group.pending = None;
            group.load = LoadState::Idle;
        }
    }

    pub(super) fn apply_group_failed(&mut self, id: &str, req: ReqId, code: String) {
        if let Some(group) = self.group_mut(id, req) {
            group.pending = None;
            group.load = LoadState::Failed {
                code,
                append: false,
            };
        }
    }

    // --- scrolling ----------------------------------------------------------

    /// Leaves the scroll alone when the size is unknown, since no row can be placed then.
    pub(super) fn reconcile_notifications_scroll(&mut self) {
        let Some(main) = self.main_area() else {
            return;
        };
        let width = main.width.saturating_sub(2);
        let fixed = crate::ui::notifications::filter_rows(&self.notifications.filters, width).len();
        let view = usize::from(main.height.saturating_sub(2)).saturating_sub(fixed);
        let state = &self.notifications;
        let scroll = if state.selected == 0 {
            0
        } else {
            let rows = crate::ui::notifications::rows(self, width);
            let total = rows.lines.len();
            let current = usize::from(state.scroll);
            match crate::ui::feed::card_extent(&rows.cards, state.selected, total) {
                Some(extent) => settle_scroll(extent, total, current, view),
                None => current.min(total.saturating_sub(view)),
            }
        };
        self.notifications.scroll = scroll.min(usize::from(u16::MAX)) as u16;
    }
}
