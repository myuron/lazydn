use super::{App, ConnTab, NavItem, ProfileTab, Screen, Session};
use crate::action::Effect;
use crate::feed::{FeedPage, LoadState, Pending, ReqId, near_end, settle_scroll};
use crate::model::{Page, ThreadItem, User, UserResponse};
use crate::profile::ConnectionsState;
use crate::thread::Entry;

/// Rows above the connection list that do not scroll: tabs, underline and a blank row.
const CONN_FIXED_ROWS: u16 = 3;

/// Profile and Connections screens: requests, selection and scrolling.
impl App {
    pub fn is_own(&self, username: &str) -> bool {
        matches!(&self.session, Session::SignedIn(me) if me.handle.eq_ignore_ascii_case(username))
    }

    /// The Nav item that the current screen belongs to; only the own profile counts as Profile.
    pub fn nav_item(&self) -> Option<NavItem> {
        match &self.screen {
            Screen::Profile(u) if !self.is_own(u) => None,
            other => NavItem::of_screen(other),
        }
    }

    pub(super) fn profile_name(&self) -> Option<String> {
        match &self.screen {
            Screen::Profile(u) => Some(u.clone()),
            _ => None,
        }
    }

    pub(super) fn conn_screen(&self) -> Option<(String, ConnTab)> {
        match &self.screen {
            Screen::Connections(u, tab) => Some((u.clone(), *tab)),
            _ => None,
        }
    }

    fn signed_in(&self) -> bool {
        !self.is_guest()
    }

    // --- profile requests ---------------------------------------------------

    fn feed_request(
        &mut self,
        name: &str,
        tab: ProfileTab,
        cursor: Option<String>,
        append: bool,
    ) -> Effect {
        let req = self.new_req();
        let state = self.profiles.entry(name.to_ascii_lowercase()).or_default();
        state.feed_mut(tab).begin(req, append);
        Effect::FetchUserCommits {
            username: name.to_string(),
            replies: tab == ProfileTab::Replies,
            cursor,
            req,
        }
    }

    pub(super) fn profile_requests(&mut self, name: &str) -> Vec<Effect> {
        let req = self.new_req();
        self.profiles
            .entry(name.to_ascii_lowercase())
            .or_default()
            .user_pending = Some(req);
        vec![
            Effect::FetchUser {
                username: name.to_string(),
                req,
            },
            self.feed_request(name, ProfileTab::Commits, None, false),
        ]
    }

    pub(super) fn profile_refresh(&mut self, name: &str) -> Vec<Effect> {
        let Some(state) = self.profiles.get(&name.to_ascii_lowercase()) else {
            return self.profile_requests(name);
        };
        let tab = state.tab;
        let (cursor, append) = state.feed(tab).refresh_target();
        let req = self.new_req();
        if let Some(state) = self.profiles.get_mut(&name.to_ascii_lowercase()) {
            state.user_pending = Some(req);
        }
        vec![
            Effect::FetchUser {
                username: name.to_string(),
                req,
            },
            self.feed_request(name, tab, cursor, append),
        ]
    }

    pub(super) fn profile_switch_tab(&mut self, tab: ProfileTab) -> Vec<Effect> {
        let Some(name) = self.profile_name() else {
            return Vec::new();
        };
        let Some(state) = self.profiles.get_mut(&name.to_ascii_lowercase()) else {
            return Vec::new();
        };
        state.tab = tab;
        if state.feed(tab).needs_entry_fetch() {
            return vec![self.feed_request(&name, tab, None, false)];
        }
        Vec::new()
    }

    pub(super) fn open_connections(&mut self, tab: ConnTab) -> Vec<Effect> {
        let Some(name) = self.profile_name() else {
            return Vec::new();
        };
        let usable = self
            .profile(&name)
            .is_some_and(|s| s.error.is_none() && s.user.is_some());
        if !usable {
            return Vec::new();
        }
        self.open(Screen::Connections(name, tab))
    }

    // --- profile responses --------------------------------------------------

    pub(super) fn apply_user_loaded(&mut self, name: &str, req: ReqId, response: UserResponse) {
        if let Some(state) = self.profiles.get_mut(&name.to_ascii_lowercase())
            && state.user_pending == Some(req)
        {
            state.user = Some(response);
            state.user_pending = None;
            state.error = None;
            state.blocked_user = None;
        }
    }

    pub(super) fn apply_user_failed(
        &mut self,
        name: &str,
        req: ReqId,
        code: String,
        blocked: Option<User>,
    ) {
        if let Some(state) = self.profiles.get_mut(&name.to_ascii_lowercase())
            && state.user_pending == Some(req)
        {
            state.user_pending = None;
            state.error = Some(code);
            state.blocked_user = blocked;
        }
    }

    pub(super) fn apply_feed_loaded(
        &mut self,
        name: &str,
        tab: ProfileTab,
        req: ReqId,
        page: FeedPage,
    ) {
        let now = self.now;
        if let Some(state) = self.profiles.get_mut(&name.to_ascii_lowercase()) {
            state.feed_mut(tab).apply_loaded(req, page, now);
        }
    }

    pub(super) fn apply_feed_failed(
        &mut self,
        name: &str,
        tab: ProfileTab,
        req: ReqId,
        code: &str,
    ) {
        if let Some(state) = self.profiles.get_mut(&name.to_ascii_lowercase()) {
            state.feed_mut(tab).apply_failed(req, code);
        }
    }

    // --- profile selection --------------------------------------------------

    pub(super) fn profile_select(&mut self, name: &str, index: usize) -> Vec<Effect> {
        let signed = self.signed_in();
        let Some(state) = self
            .profiles
            .get_mut(&name.to_ascii_lowercase())
            .filter(|s| s.shows_list())
        else {
            return Vec::new();
        };
        let count = state.cards(signed).len();
        if count == 0 {
            return Vec::new();
        }
        let offset = usize::from(state.pinned(signed).is_some());
        let tab = state.tab;
        let selected = index.min(count - 1);
        let feed = state.feed_mut(tab);
        feed.selected = selected;
        let relative = selected.saturating_sub(offset);
        let near_end = near_end(relative, feed.items.len());
        let append_failed = matches!(feed.load, LoadState::Failed { append: true, .. });
        if near_end
            && feed.pending.is_none()
            && !append_failed
            && let Some(cursor) = feed.next_cursor.clone()
        {
            return vec![self.feed_request(name, tab, Some(cursor), true)];
        }
        Vec::new()
    }

    pub(super) fn profile_open_selected(&mut self) -> Vec<Effect> {
        let Some(id) = self.profile_selected_commit().map(|c| c.id) else {
            return Vec::new();
        };
        self.open(Screen::Thread(id))
    }

    fn profile_selected_commit(&self) -> Option<crate::model::Commit> {
        let name = self.profile_name()?;
        let state = self.profile(&name).filter(|s| s.shows_list())?;
        state
            .cards(self.signed_in())
            .get(state.selected())
            .map(|c| (*c).clone())
    }

    pub(super) fn profile_toggle(&mut self) {
        let Some(id) = self.profile_selected_commit().map(|c| c.id) else {
            return;
        };
        let Some(name) = self.profile_name() else {
            return;
        };
        if let Some(state) = self.profiles.get_mut(&name.to_ascii_lowercase()) {
            let tab = state.tab;
            let expanded = &mut state.feed_mut(tab).expanded;
            if !expanded.remove(&id) {
                expanded.insert(id);
            }
        }
    }

    // --- author profile -----------------------------------------------------

    fn selected_author(&self) -> Option<String> {
        match &self.screen {
            Screen::Log(_) | Screen::Saved => self
                .current_feed()
                .and_then(|f| f.items.get(f.selected))
                .map(|c| c.author.username.clone()),
            Screen::Profile(_) => self.profile_selected_commit().map(|c| c.author.username),
            Screen::Thread(id) => {
                let state = self.threads.get(id)?;
                let commit = match self.selected_entry(id)? {
                    Entry::Main => state.main.clone(),
                    Entry::Continuation => state.continuation(id).cloned(),
                    Entry::Ancestor(i) => match state.ancestors.get(i) {
                        Some(ThreadItem::Commit(c)) => Some((**c).clone()),
                        _ => None,
                    },
                    Entry::Reply(item, _) => match self.find_item(id, &item) {
                        Some(ThreadItem::Commit(c)) => Some(*c),
                        _ => None,
                    },
                    Entry::ShowMore(_) => None,
                };
                commit.map(|c| c.author.username)
            }
            _ => None,
        }
    }

    pub(super) fn feed_open_author(&mut self) -> Vec<Effect> {
        let Some(author) = self.selected_author() else {
            return Vec::new();
        };
        if let Screen::Profile(current) = &self.screen
            && current.eq_ignore_ascii_case(&author)
        {
            return Vec::new();
        }
        self.open(Screen::Profile(author))
    }

    // --- connections --------------------------------------------------------

    fn conn_request(
        &mut self,
        name: &str,
        tab: ConnTab,
        cursor: Option<String>,
        append: bool,
    ) -> Effect {
        let req = self.new_req();
        let state = self
            .connections
            .entry((name.to_ascii_lowercase(), tab))
            .or_default();
        state.pending = Some(Pending { req, append });
        state.load = LoadState::Idle;
        Effect::FetchConnections {
            username: name.to_string(),
            tab,
            cursor,
            req,
        }
    }

    pub(super) fn conn_entry(&mut self, name: &str, tab: ConnTab) -> Vec<Effect> {
        if self
            .connections
            .contains_key(&(name.to_ascii_lowercase(), tab))
        {
            return Vec::new();
        }
        vec![self.conn_request(name, tab, None, false)]
    }

    pub(super) fn conn_refresh(&mut self, name: &str, tab: ConnTab) -> Vec<Effect> {
        let target = match self.connections.get(&(name.to_ascii_lowercase(), tab)) {
            Some(ConnectionsState {
                load: LoadState::Failed { append: true, .. },
                next_cursor,
                ..
            }) => (next_cursor.clone(), true),
            _ => (None, false),
        };
        vec![self.conn_request(name, tab, target.0, target.1)]
    }

    pub(super) fn conn_switch_tab(&mut self, tab: ConnTab) -> Vec<Effect> {
        let Some((name, current)) = self.conn_screen() else {
            return Vec::new();
        };
        if current == tab {
            return Vec::new();
        }
        self.show(Screen::Connections(name.clone(), tab));
        self.conn_entry(&name, tab)
    }

    pub(super) fn apply_conn_loaded(
        &mut self,
        name: &str,
        tab: ConnTab,
        req: ReqId,
        page: Page<User>,
    ) {
        let Some(state) = self.connections.get_mut(&(name.to_ascii_lowercase(), tab)) else {
            return;
        };
        let append = match &state.pending {
            Some(p) if p.req == req => p.append,
            _ => return,
        };
        if append {
            for user in page.data {
                if !state.users.iter().any(|u| u.id == user.id) {
                    state.users.push(user);
                }
            }
        } else {
            state.users = page.data;
            state.selected = 0;
            state.scroll = 0;
        }
        state.next_cursor = page.meta.next_cursor;
        state.pending = None;
        state.load = LoadState::Idle;
        state.loaded_once = true;
    }

    pub(super) fn apply_conn_failed(&mut self, name: &str, tab: ConnTab, req: ReqId, code: String) {
        let Some(state) = self.connections.get_mut(&(name.to_ascii_lowercase(), tab)) else {
            return;
        };
        if let Some(p) = state.pending.clone().filter(|p| p.req == req) {
            state.pending = None;
            state.load = LoadState::Failed {
                code,
                append: p.append,
            };
        }
    }

    pub(super) fn conn_select(&mut self, name: &str, tab: ConnTab, index: usize) -> Vec<Effect> {
        let Some(state) = self.connections.get_mut(&(name.to_ascii_lowercase(), tab)) else {
            return Vec::new();
        };
        if state.users.is_empty() {
            return Vec::new();
        }
        state.selected = index.min(state.users.len() - 1);
        let near_end = near_end(state.selected, state.users.len());
        let append_failed = matches!(state.load, LoadState::Failed { append: true, .. });
        if near_end
            && state.pending.is_none()
            && !append_failed
            && let Some(cursor) = state.next_cursor.clone()
        {
            return vec![self.conn_request(name, tab, Some(cursor), true)];
        }
        Vec::new()
    }

    pub(super) fn conn_open_selected(&mut self) -> Vec<Effect> {
        let Some((name, tab)) = self.conn_screen() else {
            return Vec::new();
        };
        let target = self
            .connections(&name, tab)
            .and_then(|s| s.users.get(s.selected))
            .map(|u| u.username.clone());
        match target {
            Some(username) => self.open(Screen::Profile(username)),
            None => Vec::new(),
        }
    }

    // --- cleanup ------------------------------------------------------------

    /// Back leaves a screen for good, so only what history can still reach is kept.
    pub(super) fn drop_unreachable_profiles(&mut self) {
        let screens: Vec<&Screen> = self
            .history
            .iter()
            .chain(std::iter::once(&self.screen))
            .collect();
        let profiles: Vec<String> = screens
            .iter()
            .filter_map(|s| match s {
                Screen::Profile(u) => Some(u.to_ascii_lowercase()),
                _ => None,
            })
            .collect();
        let conns: Vec<String> = screens
            .iter()
            .filter_map(|s| match s {
                Screen::Connections(u, _) => Some(u.to_ascii_lowercase()),
                _ => None,
            })
            .collect();
        self.profiles.retain(|u, _| profiles.contains(u));
        self.connections.retain(|(u, _), _| conns.contains(u));
    }

    // --- scrolling ----------------------------------------------------------

    pub(super) fn reconcile_profile_scroll(&mut self, name: &str) {
        let Some(main) = self.main_area() else {
            return;
        };
        let Some(state) = self.profile(name) else {
            return;
        };
        let selected = state.selected();
        let tab = state.tab;
        let scroll = usize::from(state.scroll());
        let view = main.height.saturating_sub(2) as usize;
        let rows = crate::ui::profile::rows(self, name, main.width.saturating_sub(2));
        let total = rows.lines.len();
        let wanted = match crate::ui::feed::card_extent(&rows.cards, selected, total) {
            Some(extent) if selected > 0 => settle_scroll(extent, total, scroll, view),
            _ => 0,
        };
        if let Some(state) = self.profiles.get_mut(&name.to_ascii_lowercase()) {
            state.feed_mut(tab).scroll = wanted.min(usize::from(u16::MAX)) as u16;
        }
    }

    pub(super) fn reconcile_conn_scroll(&mut self, name: &str, tab: ConnTab) {
        let Some(main) = self.main_area() else {
            return;
        };
        let Some(state) = self.connections(name, tab) else {
            return;
        };
        let selected = state.selected;
        let scroll = usize::from(state.scroll);
        let view = main.height.saturating_sub(2 + CONN_FIXED_ROWS) as usize;
        let rows = crate::ui::connections::rows(self, name, tab, main.width.saturating_sub(2));
        let total = rows.lines.len();
        let wanted = match crate::ui::feed::card_extent(&rows.users, selected, total) {
            Some(extent) if selected > 0 => settle_scroll(extent, total, scroll, view),
            _ => 0,
        };
        if let Some(state) = self.connections.get_mut(&(name.to_ascii_lowercase(), tab)) {
            state.scroll = wanted.min(usize::from(u16::MAX)) as u16;
        }
    }
}
