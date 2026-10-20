use std::collections::HashMap;

use ratatui::layout::Rect;
use time::{OffsetDateTime, UtcOffset};

mod notifications;
mod profile;
mod thread;

use crate::action::{Action, Effect};
use crate::feed::{Feed, FeedKey, LoadState, ReqId, settle_scroll};
use crate::layout::LayoutMode;
use crate::model::Lane;
use crate::notifications::NotificationsState;
use crate::profile::{ConnectionsState, ProfileState};
use crate::thread::ThreadState;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Me {
    pub id: String,
    pub name: String,
    pub handle: String,
    pub commits: u32,
    pub following: u32,
    pub followers: u32,
    pub pasteis_left: u32,
    pub pasteis_max: u32,
    pub bio: String,
    pub unread: u32,
    pub pasteis_mode: bool,
    /// The lane in effect, which is the automatic one when none is chosen.
    pub lane: Lane,
    pub preferred_lane: Option<Lane>,
    pub automatic_lane: u8,
    pub pasteis_resets_at: OffsetDateTime,
    pub pasteis_received: u32,
    pub github_synced_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Session {
    Guest,
    SignedIn(Me),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LogTab {
    Following,
    Latest,
    Trending,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum ProfileTab {
    #[default]
    Commits,
    Replies,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum ConnTab {
    #[default]
    Followers,
    Following,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Screen {
    Log(LogTab),
    Notifications,
    /// Username of the profile owner.
    Profile(String),
    Saved,
    Settings,
    /// Commit id.
    Thread(String),
    /// Username of the profile owner, as typed; state is looked up without regard to case.
    Connections(String, ConnTab),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Nav,
    Main,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modal {
    Help,
    SignIn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavItem {
    Log,
    Notifications,
    Profile,
    Saved,
    Settings,
}

impl NavItem {
    pub fn of_screen(screen: &Screen) -> Option<NavItem> {
        match screen {
            Screen::Log(_) => Some(NavItem::Log),
            Screen::Notifications => Some(NavItem::Notifications),
            Screen::Profile(_) => Some(NavItem::Profile),
            Screen::Saved => Some(NavItem::Saved),
            Screen::Settings => Some(NavItem::Settings),
            Screen::Thread(_) | Screen::Connections(..) => None,
        }
    }

    fn index(self) -> usize {
        self as usize
    }

    pub const ALL: [NavItem; 5] = [
        NavItem::Log,
        NavItem::Notifications,
        NavItem::Profile,
        NavItem::Saved,
        NavItem::Settings,
    ];
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prefs {
    pub ascii: bool,
    pub truecolor: bool,
    pub reply_preview_limit: usize,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            ascii: false,
            truecolor: true,
            reply_preview_limit: 3,
        }
    }
}

#[derive(Debug, Clone)]
pub struct App {
    session: Session,
    screen: Screen,
    history: Vec<Screen>,
    focus: Pane,
    modal: Option<Modal>,
    nav_cursor: NavItem,
    g_pending: bool,
    size: (u16, u16),
    feeds: HashMap<FeedKey, Feed>,
    threads: HashMap<String, ThreadState>,
    /// Keyed by the lowercased username.
    profiles: HashMap<String, ProfileState>,
    connections: HashMap<(String, ConnTab), ConnectionsState>,
    notifications: NotificationsState,
    /// The request whose answer may still arrive; none are stacked.
    me_pending: Option<ReqId>,
    last_me_fetch: Option<OffsetDateTime>,
    /// The latest FetchNotifications, prefetches included, since each one marks everything read.
    last_notif_req: Option<ReqId>,
    now: OffsetDateTime,
    local_offset: UtcOffset,
    prefs: Prefs,
    last_visit: Option<String>,
    next_req: u64,
}

impl App {
    pub fn new(session: Session, now: OffsetDateTime, local_offset: UtcOffset) -> Self {
        let guest = matches!(session, Session::Guest);
        Self {
            screen: Screen::Log(if guest {
                LogTab::Latest
            } else {
                LogTab::Following
            }),
            modal: guest.then_some(Modal::SignIn),
            session,
            history: Vec::new(),
            focus: Pane::Main,
            nav_cursor: NavItem::Log,
            g_pending: false,
            size: (0, 0),
            feeds: FeedKey::ALL
                .into_iter()
                .map(|k| (k, Feed::default()))
                .collect(),
            threads: HashMap::new(),
            profiles: HashMap::new(),
            connections: HashMap::new(),
            notifications: NotificationsState::default(),
            me_pending: None,
            last_me_fetch: None,
            last_notif_req: None,
            now,
            local_offset,
            prefs: Prefs::default(),
            last_visit: None,
            next_req: 0,
        }
    }

    pub fn update(&mut self, action: Action) -> Vec<Effect> {
        let mut effects = Vec::new();
        match action {
            Action::Quit => return vec![Effect::Quit],
            Action::Resize(w, h) => self.size = (w, h),
            Action::Init => {
                effects.extend(self.request_me());
                effects.extend(self.entry_fetch());
            }
            Action::Open(screen) => effects.extend(self.open(screen)),
            Action::Goto(screen) => {
                self.g_pending = false;
                self.goto(screen);
                effects.extend(self.enter_fetch());
            }
            Action::Back => {
                if let Some(previous) = self.history.pop() {
                    self.show(previous);
                    self.drop_unreachable_threads();
                    self.drop_unreachable_profiles();
                    effects.extend(self.entry_fetch());
                }
            }
            Action::StartGPrefix => self.g_pending = true,
            Action::CancelGPrefix => self.g_pending = false,
            Action::FocusNext | Action::FocusPrev => {
                self.focus = match self.focus {
                    Pane::Nav => Pane::Main,
                    Pane::Main => {
                        self.sync_cursor();
                        Pane::Nav
                    }
                };
            }
            Action::NavStep(step) => {
                let last = NavItem::ALL.len() - 1;
                let next = self
                    .nav_cursor
                    .index()
                    .saturating_add_signed(step as isize)
                    .min(last);
                self.nav_cursor = NavItem::ALL[next];
            }
            Action::NavEnter => {
                let target = match self.nav_cursor {
                    NavItem::Log => match self.screen {
                        Screen::Log(_) => self.screen.clone(),
                        _ => Screen::Log(self.home_tab()),
                    },
                    NavItem::Notifications => Screen::Notifications,
                    NavItem::Profile => Screen::Profile(self.own_handle()),
                    NavItem::Saved => Screen::Saved,
                    NavItem::Settings => Screen::Settings,
                };
                if self.goto(target) {
                    self.focus = Pane::Main;
                }
                effects.extend(self.enter_fetch());
            }
            Action::SwitchTab(tab) => {
                if matches!(self.screen, Screen::Log(_)) {
                    self.goto_tab(tab);
                    effects.extend(self.entry_fetch());
                }
            }
            Action::OpenHelp => self.modal = Some(Modal::Help),
            Action::OpenSignIn => self.modal = Some(Modal::SignIn),
            Action::CloseModal => self.modal = None,
            Action::Commit => {
                if matches!(self.session, Session::Guest) {
                    self.modal = Some(Modal::SignIn);
                }
            }
            Action::Tick(now) => {
                self.now = now;
                effects.extend(self.poll_me());
                effects.extend(self.poll());
            }
            Action::FeedLoaded { key, req, page } => {
                let now = self.now;
                self.feeds
                    .entry(key)
                    .or_default()
                    .apply_loaded(req, page, now);
            }
            Action::FeedFailed { key, req, code } => {
                self.feeds.entry(key).or_default().apply_failed(req, &code);
            }
            Action::NewCommits { key, since, count } => {
                self.feeds
                    .entry(key)
                    .or_default()
                    .accept_new_commits(&since, count);
            }
            Action::MoveSelection(step) => effects.extend(self.move_selection(step)),
            Action::SelectFirst => {
                self.g_pending = false;
                effects.extend(self.select(0));
            }
            Action::SelectLast => effects.extend(self.select(usize::MAX)),
            Action::OpenSelected if self.on_notifications() => {
                effects.extend(self.notif_open_selected());
            }
            Action::ToggleReplies if self.on_notifications() => {
                effects.extend(self.notif_toggle_group());
            }
            Action::OpenAuthor if self.on_notifications() => {
                effects.extend(self.notif_open_author());
            }
            Action::ToggleFilter(filter) => effects.extend(self.notif_toggle_filter(filter)),
            Action::ToggleAllFilters => effects.extend(self.notif_toggle_all_filters()),
            Action::MeLoaded { req, response } => self.apply_me_loaded(req, &response),
            Action::MeFailed { req, .. } => {
                if self.me_pending == Some(req) {
                    self.me_pending = None;
                }
            }
            Action::NotificationsLoaded { req, page } => {
                effects.extend(self.apply_notifications_loaded(req, *page));
            }
            Action::NotificationsFailed { req, code } => {
                self.apply_notifications_failed(req, code);
            }
            Action::GroupLoaded { id, req, page } => self.apply_group_loaded(&id, req, page),
            Action::GroupFailed { id, req, code } => self.apply_group_failed(&id, req, code),
            Action::OpenSelected if self.thread_id().is_some() => {
                effects.extend(self.thread_open_selected());
            }
            Action::ToggleReplies if self.thread_id().is_some() => {
                effects.extend(self.thread_toggle());
            }
            Action::OpenSelected if self.profile_name().is_some() => {
                effects.extend(self.profile_open_selected());
            }
            Action::OpenSelected if self.conn_screen().is_some() => {
                effects.extend(self.conn_open_selected());
            }
            Action::ToggleReplies if self.profile_name().is_some() => self.profile_toggle(),
            Action::OpenSelected => {
                let id = self
                    .current_feed()
                    .and_then(|f| f.items.get(f.selected))
                    .map(|c| c.id.clone());
                if let Some(id) = id {
                    effects.extend(self.open(Screen::Thread(id)));
                }
            }
            Action::UserLoaded {
                username,
                req,
                response,
            } => self.apply_user_loaded(&username, req, *response),
            Action::UserFailed {
                username,
                req,
                code,
                blocked_user,
            } => self.apply_user_failed(&username, req, code, blocked_user.map(|u| *u)),
            Action::ProfileFeedLoaded {
                username,
                tab,
                req,
                page,
            } => self.apply_feed_loaded(&username, tab, req, page),
            Action::ProfileFeedFailed {
                username,
                tab,
                req,
                code,
            } => self.apply_feed_failed(&username, tab, req, &code),
            Action::ConnectionsLoaded {
                username,
                tab,
                req,
                page,
            } => self.apply_conn_loaded(&username, tab, req, page),
            Action::ConnectionsFailed {
                username,
                tab,
                req,
                code,
            } => self.apply_conn_failed(&username, tab, req, code),
            Action::OpenAuthor => effects.extend(self.open_author()),
            Action::OpenConnections(tab) => effects.extend(self.open_connections(tab)),
            Action::SwitchProfileTab(tab) => effects.extend(self.profile_switch_tab(tab)),
            Action::SwitchConnTab(tab) => effects.extend(self.conn_switch_tab(tab)),
            Action::ToggleReplies => {
                if let Some(key) = self.current_key() {
                    self.feeds.entry(key).or_default().toggle_replies();
                }
            }
            Action::Refresh => effects.extend(self.refresh()),
            Action::ThreadLoaded { id, req, response } => {
                if let Some(state) = self.threads.get_mut(&id)
                    && state.main_pending == Some(req)
                {
                    let response = *response;
                    state.main = Some(response.data);
                    state.ancestors = response.ancestors;
                    state.main_pending = None;
                    state.main_load = LoadState::Idle;
                    state.selected = state.ancestors.len();
                    state.scroll = 0;
                }
            }
            Action::ThreadFailed { id, req, code } => {
                if let Some(state) = self.threads.get_mut(&id)
                    && state.main_pending == Some(req)
                {
                    state.main_pending = None;
                    state.main_load = LoadState::Failed {
                        code,
                        append: false,
                    };
                }
            }
            Action::RepliesLoaded {
                thread,
                parent,
                req,
                page,
            } => {
                let kept = self.selected_entry(&thread);
                if let Some(list) = self.reply_list_mut(&thread, &parent) {
                    list.apply_loaded(req, page.data, page.meta.next_cursor);
                }
                self.restore_selection(&thread, kept);
            }
            Action::RepliesFailed {
                thread,
                parent,
                req,
                code,
            } => {
                let kept = self.selected_entry(&thread);
                if let Some(list) = self.reply_list_mut(&thread, &parent) {
                    list.apply_failed(req, &code);
                }
                self.restore_selection(&thread, kept);
            }
        }
        self.reconcile_scroll();
        effects
    }

    pub fn home_tab(&self) -> LogTab {
        match self.session {
            Session::Guest => LogTab::Latest,
            Session::SignedIn(_) => LogTab::Following,
        }
    }

    fn needs_sign_in(&self, screen: &Screen) -> bool {
        if !matches!(self.session, Session::Guest) {
            return false;
        }
        // Relies on own_handle() being empty for a guest, which is what makes g p and the Nav target Profile(""); other profiles are public.
        matches!(
            screen,
            Screen::Log(LogTab::Following)
                | Screen::Notifications
                | Screen::Saved
                | Screen::Settings
        ) || matches!(screen, Screen::Profile(u) if u.is_empty())
    }

    /// Returns false after opening the sign-in dialog when a guest targets a protected screen.
    fn guard_sign_in(&mut self, screen: &Screen) -> bool {
        if self.needs_sign_in(screen) {
            self.modal = Some(Modal::SignIn);
            return false;
        }
        true
    }

    /// Replaces the screen and clears the history; returns false when the sign-in dialog opened instead.
    fn goto(&mut self, screen: Screen) -> bool {
        if !self.guard_sign_in(&screen) {
            return false;
        }
        self.history.clear();
        // No history entry can lead back to a thread now, so its state would only go stale.
        self.threads.clear();
        self.profiles.clear();
        self.connections.clear();
        self.show(screen);
        if let Some(feed) = self.current_key().map(|k| self.feeds.entry(k).or_default()) {
            feed.selected = 0;
            feed.scroll = 0;
        }
        true
    }

    fn goto_tab(&mut self, tab: LogTab) {
        let screen = Screen::Log(tab);
        if self.guard_sign_in(&screen) {
            self.screen = screen;
        }
    }

    fn show(&mut self, screen: Screen) {
        self.screen = screen;
        self.sync_cursor();
    }

    /// None while the terminal is too small, since no row can be placed then.
    fn main_area(&self) -> Option<Rect> {
        if self.layout_mode() == LayoutMode::TooSmall {
            return None;
        }
        let (w, h) = self.size;
        Some(crate::ui::layout::split(Rect::new(0, 0, w, h), self.layout_mode()).main)
    }

    fn sync_cursor(&mut self) {
        if let Some(item) = self.nav_item() {
            self.nav_cursor = item;
        }
    }

    pub fn session(&self) -> &Session {
        &self.session
    }

    pub fn screen(&self) -> Screen {
        self.screen.clone()
    }

    pub fn feed(&self, key: FeedKey) -> &Feed {
        &self.feeds[&key]
    }

    pub fn thread(&self, id: &str) -> Option<&ThreadState> {
        self.threads.get(id)
    }

    pub fn profile(&self, username: &str) -> Option<&ProfileState> {
        self.profiles.get(&username.to_ascii_lowercase())
    }

    pub fn connections(&self, username: &str, tab: ConnTab) -> Option<&ConnectionsState> {
        self.connections.get(&(username.to_ascii_lowercase(), tab))
    }

    pub fn notifications(&self) -> &NotificationsState {
        &self.notifications
    }

    pub fn now(&self) -> OffsetDateTime {
        self.now
    }

    pub fn local_offset(&self) -> UtcOffset {
        self.local_offset
    }

    pub fn prefs(&self) -> &Prefs {
        &self.prefs
    }

    pub fn last_visit(&self) -> Option<&str> {
        self.last_visit.as_deref()
    }

    pub fn set_last_visit(&mut self, id: Option<String>) {
        self.last_visit = id;
    }

    pub fn own_handle(&self) -> String {
        match &self.session {
            Session::SignedIn(me) => me.handle.clone(),
            Session::Guest => String::new(),
        }
    }

    pub fn focus(&self) -> Pane {
        self.focus
    }

    pub fn modal(&self) -> Option<Modal> {
        self.modal
    }

    pub fn nav_cursor(&self) -> NavItem {
        self.nav_cursor
    }

    pub fn g_pending(&self) -> bool {
        self.g_pending
    }

    pub fn size(&self) -> (u16, u16) {
        self.size
    }

    pub fn layout_mode(&self) -> LayoutMode {
        LayoutMode::from_size(self.size.0, self.size.1)
    }
}

impl App {
    fn open(&mut self, screen: Screen) -> Vec<Effect> {
        if !self.guard_sign_in(&screen) {
            return Vec::new();
        }
        self.history.push(self.screen.clone());
        self.show(screen);
        self.enter_fetch()
    }

    fn current_key(&self) -> Option<FeedKey> {
        match self.screen {
            Screen::Log(tab) => Some(FeedKey::Log(tab)),
            Screen::Saved => Some(FeedKey::Saved),
            _ => None,
        }
    }

    fn current_feed(&self) -> Option<&Feed> {
        self.current_key().map(|k| &self.feeds[&k])
    }

    /// Guests never fetch: the log API needs a sign-in.
    fn fetchable_key(&self) -> Option<FeedKey> {
        self.current_key()
            .filter(|_| matches!(self.session, Session::SignedIn(_)))
    }

    fn request(&mut self, key: FeedKey, cursor: Option<String>, append: bool) -> Effect {
        self.next_req += 1;
        let req = ReqId(self.next_req);
        self.feeds.entry(key).or_default().begin(req, append);
        match key {
            FeedKey::Log(tab) => Effect::FetchLog { tab, cursor, req },
            FeedKey::Saved => Effect::FetchSaved { cursor, req },
        }
    }

    fn entry_fetch(&mut self) -> Vec<Effect> {
        if let Some(name) = self.profile_name() {
            if self.profile(&name).is_some() {
                return Vec::new();
            }
            return self.profile_requests(&name);
        }
        if let Some((name, tab)) = self.conn_screen() {
            return self.conn_entry(&name, tab);
        }
        if let Some(id) = self.thread_id() {
            if self.threads.contains_key(&id) {
                return Vec::new();
            }
            return self.thread_requests(&id);
        }
        match self.fetchable_key() {
            Some(key) if self.feeds[&key].needs_entry_fetch() => {
                vec![self.request(key, None, false)]
            }
            _ => Vec::new(),
        }
    }

    fn refresh(&mut self) -> Vec<Effect> {
        if let Some(name) = self.profile_name() {
            return self.profile_refresh(&name);
        }
        if let Some((name, tab)) = self.conn_screen() {
            return self.conn_refresh(&name, tab);
        }
        if let Some(id) = self.thread_id() {
            if let Some(state) = self.threads.get_mut(&id) {
                state.children.clear();
                state.expanded.clear();
            }
            return self.thread_requests(&id);
        }
        if self.on_notifications() {
            return vec![self.request_notifications(None, false)];
        }
        let Some(key) = self.fetchable_key() else {
            return Vec::new();
        };
        let (cursor, append) = self.feeds[&key].refresh_target();
        vec![self.request(key, cursor, append)]
    }

    fn prefetch(&mut self) -> Vec<Effect> {
        let Some(key) = self.fetchable_key() else {
            return Vec::new();
        };
        match self.feeds[&key].prefetch_cursor() {
            Some(cursor) => vec![self.request(key, Some(cursor), true)],
            None => Vec::new(),
        }
    }

    fn select(&mut self, index: usize) -> Vec<Effect> {
        if self.on_notifications() {
            return self.notif_select(index);
        }
        if let Some(name) = self.profile_name() {
            return self.profile_select(&name, index);
        }
        if let Some((name, tab)) = self.conn_screen() {
            return self.conn_select(&name, tab, index);
        }
        if let Some(id) = self.thread_id() {
            return self.thread_select(&id, index);
        }
        let Some(key) = self.current_key() else {
            return Vec::new();
        };
        self.feeds.entry(key).or_default().select(index);
        self.prefetch()
    }

    fn move_selection(&mut self, step: i32) -> Vec<Effect> {
        if self.on_notifications() {
            return self.notif_move(step);
        }
        if let Some(name) = self.profile_name() {
            let current = self.profile(&name).map_or(0, |s| s.selected());
            return self.profile_select(&name, current.saturating_add_signed(step as isize));
        }
        if let Some((name, tab)) = self.conn_screen() {
            let current = self.connections(&name, tab).map_or(0, |s| s.selected);
            return self.conn_select(&name, tab, current.saturating_add_signed(step as isize));
        }
        if let Some(id) = self.thread_id() {
            let current = self.threads.get(&id).map_or(0, |t| t.selected);
            return self.thread_select(&id, current.saturating_add_signed(step as isize));
        }
        let Some(key) = self.current_key() else {
            return Vec::new();
        };
        let feed = &self.feeds[&key];
        // A failed append must not block this: the user asked for the newest items.
        if step < 0 && feed.selected == 0 && feed.new_commits > 0 && self.fetchable_key().is_some()
        {
            return vec![self.request(key, None, false)];
        }
        let target = feed.selected.saturating_add_signed(step as isize);
        self.select(target)
    }

    fn poll(&mut self) -> Vec<Effect> {
        let Some(key @ FeedKey::Log(tab)) = self.fetchable_key() else {
            return Vec::new();
        };
        if tab == LogTab::Trending {
            return Vec::new();
        }
        let Some(since) = self.feeds[&key].poll_since(self.now) else {
            return Vec::new();
        };
        self.feeds.entry(key).or_default().last_poll = Some(self.now);
        vec![Effect::PollLog { tab, since }]
    }

    /// Leaves the scroll alone when the size is unknown, since no row can be placed then.
    fn reconcile_scroll(&mut self) {
        if self.on_notifications() {
            self.reconcile_notifications_scroll();
            return;
        }
        if let Some(name) = self.profile_name() {
            self.reconcile_profile_scroll(&name);
            return;
        }
        if let Some((name, tab)) = self.conn_screen() {
            self.reconcile_conn_scroll(&name, tab);
            return;
        }
        if let Some(id) = self.thread_id() {
            self.reconcile_thread_scroll(&id);
            return;
        }
        let Some(key) = self.current_key() else {
            return;
        };
        let Some(main) = self.main_area() else {
            return;
        };
        let tabs = if matches!(key, FeedKey::Log(_)) { 2 } else { 0 };
        let view = main.height.saturating_sub(2 + tabs) as usize;
        let feed = &self.feeds[&key];
        let scroll = if feed.selected == 0 {
            0
        } else {
            let rows = crate::ui::feed::rows(feed, key, self, main.width.saturating_sub(2));
            let total = rows.lines.len();
            match crate::ui::feed::card_extent(&rows.cards, feed.selected, total) {
                Some(extent) => settle_scroll(extent, total, feed.scroll as usize, view),
                None => (feed.scroll as usize).min(total.saturating_sub(view)),
            }
        };
        self.feeds.entry(key).or_default().scroll = scroll.min(u16::MAX as usize) as u16;
    }
}
