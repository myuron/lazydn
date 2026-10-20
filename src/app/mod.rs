use std::collections::HashMap;

use ratatui::layout::Rect;
use time::{OffsetDateTime, UtcOffset};

mod compose;
mod copies;
mod key_status;
mod last_visit;
mod menu;
mod notifications;
mod people;
mod profile;
mod removal;
mod report;
mod session;
mod settings;
mod settle;
mod signin;
mod status;
mod thread;
mod toast;
mod writes;

pub use key_status::KeyStatus;
pub use signin::{SignInState, SignInStep};
pub use status::ConnState;
pub use toast::{Toast, ToastKind};

use crate::action::{Action, Effect, Failure};
use crate::api::client::RateLimit;
use crate::feed::{Feed, FeedKey, LoadState, ReqId, box_span, ensure_visible, settle_card_scroll};
use crate::keyboard::KeyboardMode;
use crate::layout::LayoutMode;
use crate::model::Lane;
use crate::notifications::NotificationsState;
use crate::profile::{ConnectionsState, ProfileState};
use crate::settings::SettingsState;
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
    /// A device key exists and the server has not yet confirmed it; the user is not known.
    Restoring,
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum SettingsTab {
    #[default]
    Profile,
    Appearance,
    Safety,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Theme {
    #[default]
    Dark,
    Light,
}

/// Per-screen dispatch key, so each operation matches once instead of probing screens one by one.
/// Matching `Screen` directly would not do: Log and Saved must share one `Feed(FeedKey)` arm, and the
/// arms call `&mut self` methods, which needs an owned value detached from the borrow of `self.screen`.
enum ScreenKind {
    Feed(FeedKey),
    Thread(String),
    Profile(String),
    Connections(String, ConnTab),
    Notifications,
    Settings,
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
    /// The composer, over every screen but Log.
    Composer,
    /// Waiting for `y` or `n` before a delete.
    ConfirmDelete,
    Report,
    /// The list of everything that can be done to the selected card.
    Menu,
    /// Waiting for `y` or `n` before a block.
    ConfirmBlock,
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
    pub theme: Theme,
    pub collapse_nav: bool,
    pub lane_dimming: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            ascii: false,
            truecolor: true,
            reply_preview_limit: 3,
            theme: Theme::Dark,
            collapse_nav: false,
            lane_dimming: false,
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
    settings: SettingsState,
    /// The request whose answer may still arrive; none are stacked.
    me_pending: Option<ReqId>,
    last_me_fetch: Option<OffsetDateTime>,
    /// The latest FetchNotifications, prefetches included, since each one marks everything read.
    last_notif_req: Option<ReqId>,
    now: OffsetDateTime,
    local_offset: UtcOffset,
    prefs: Prefs,
    last_visits: HashMap<LogTab, String>,
    next_req: u64,
    toast: Option<Toast>,
    conn: ConnState,
    rate: Option<RateLimit>,
    host_label: Option<String>,
    key_status: KeyStatus,
    sign_in: SignInState,
    /// The newest request id at the last sign-in; a rejection of an older one says nothing about the new device.
    sign_in_boundary: Option<ReqId>,
    key_path: std::path::PathBuf,
    keyboard: KeyboardMode,
    /// The open composer, if any.
    compose: Option<crate::compose::Compose>,
    /// Every draft of the signed-in account.
    drafts: crate::drafts::Drafts,
    /// The commit that is out, kept after the composer closes so that it cannot be sent twice.
    in_flight: Option<crate::compose::InFlight>,
    /// Consecutive failed attempts to confirm the session; sets the retry delay while restoring.
    restore_failures: u32,
    /// When the drafts last changed without being written.
    drafts_dirty_at: Option<OffsetDateTime>,
    /// A write failed, so the drafts are not on disk; they are written when the user leaves the composer or quits, not on a timer.
    drafts_unsaved: bool,
    /// The star, save and pastel requests that are out, by commit.
    writes: HashMap<(writes::WriteKind, String), ReqId>,
    /// The delete waiting for `y`.
    confirm: Option<crate::dialogs::DeleteConfirm>,
    /// The open report dialog.
    report: Option<crate::dialogs::ReportState>,
    /// The open more menu.
    menu: Option<crate::dialogs::MoreMenu>,
    /// The deletes that are out, by commit, with the hash the toast names.
    deleting: HashMap<String, (ReqId, String)>,
    /// What became of every commit deleted in this session; loads that were already out are settled by it.
    deleted: HashMap<String, settle::Fate>,
    /// The reports that are out, by commit, with the hash the toast names.
    reports: HashMap<String, (ReqId, String)>,
    /// The follow, mute and block requests that are out, by kind and lowercased username.
    user_writes: HashMap<(people::UserWriteKind, String), ReqId>,
    /// The person the block waits for `y` about.
    block_confirm: Option<String>,
}

impl App {
    pub fn new(session: Session, now: OffsetDateTime, local_offset: UtcOffset) -> Self {
        // A restoring session exists only because a key was found.
        let key_status = if matches!(session, Session::Restoring) {
            KeyStatus::Present
        } else {
            KeyStatus::Absent
        };
        Self::with_key_status(
            session,
            now,
            local_offset,
            key_status,
            std::path::PathBuf::new(),
        )
    }

    /// Starts as `new` does, and tells the app what main found when it looked for the device key.
    pub fn with_key_status(
        session: Session,
        now: OffsetDateTime,
        local_offset: UtcOffset,
        key_status: KeyStatus,
        key_path: std::path::PathBuf,
    ) -> Self {
        let guest = matches!(session, Session::Guest);
        let mut app = Self {
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
            settings: SettingsState::default(),
            me_pending: None,
            last_me_fetch: None,
            last_notif_req: None,
            now,
            local_offset,
            prefs: Prefs::default(),
            last_visits: HashMap::new(),
            next_req: 0,
            toast: None,
            conn: ConnState::default(),
            rate: None,
            host_label: None,
            key_status,
            sign_in: SignInState::default(),
            sign_in_boundary: None,
            key_path,
            keyboard: KeyboardMode::default(),
            restore_failures: 0,
            compose: None,
            drafts: crate::drafts::Drafts::default(),
            in_flight: None,
            drafts_dirty_at: None,
            drafts_unsaved: false,
            writes: HashMap::new(),
            confirm: None,
            report: None,
            menu: None,
            deleting: HashMap::new(),
            deleted: HashMap::new(),
            reports: HashMap::new(),
            user_writes: HashMap::new(),
            block_confirm: None,
        };
        if let KeyStatus::Unreadable { error, path } = &app.key_status {
            let text = toast::key_error_text(error, path);
            app.show_toast(ToastKind::Error, text);
        }
        app
    }

    pub fn update(&mut self, action: Action) -> Vec<Effect> {
        if let Some(effects) = self.handle_rejection(&action) {
            return effects;
        }
        let mut failure_toast: Option<String> = None;
        let mut effects = Vec::new();
        match action {
            Action::Quit => return self.quit_effects(),
            Action::Resize(w, h) => self.size = (w, h),
            Action::Init => {
                if matches!(self.session, Session::Restoring) {
                    effects.push(Effect::UseSignedClient);
                }
                if self.modal == Some(Modal::SignIn) {
                    effects.extend(self.open_sign_in());
                }
                effects.extend(self.request_me());
                effects.extend(self.entry_fetch());
            }
            Action::Open(screen) => effects.extend(self.open(screen)),
            Action::Goto(screen) => {
                self.g_pending = false;
                if let Err(opened) = self.goto(screen) {
                    effects.extend(opened);
                }
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
                match self.goto(target) {
                    Ok(()) => self.focus = Pane::Main,
                    Err(opened) => effects.extend(opened),
                }
                effects.extend(self.enter_fetch());
            }
            Action::SwitchTab(tab) => {
                if matches!(self.screen, Screen::Log(_)) {
                    effects.extend(self.goto_tab(tab));
                    effects.extend(self.entry_fetch());
                }
            }
            Action::OpenHelp => self.modal = Some(Modal::Help),
            Action::OpenSignIn => {
                if matches!(self.session, Session::Guest) {
                    effects.extend(self.open_sign_in());
                }
            }

            Action::CloseModal => self.close_modal(),
            Action::Commit => effects.extend(self.open_composer()),
            Action::Reply => effects.extend(self.open_reply()),
            Action::Quote => effects.extend(self.open_quote()),
            Action::Tick(now) => {
                self.now = now;
                self.expire_toast();
                effects.extend(self.poll_me());
                effects.extend(self.poll());
                effects.extend(self.due_save());
            }
            Action::FeedLoaded { key, req, mut page } => {
                self.settle_page(&mut page);
                let now = self.now;
                self.feeds
                    .entry(key)
                    .or_default()
                    .apply_loaded(req, page, now);
            }
            Action::FeedFailed { key, req, failure } => {
                let feed = self.feeds.entry(key).or_default();
                if feed.apply_failed(req, &failure.code) && !feed.items.is_empty() {
                    failure_toast = Some(toast::failure_text(&failure));
                }
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
            Action::OpenSelected => effects.extend(self.open_selected()),
            Action::ToggleReplies => effects.extend(self.toggle_replies()),
            Action::OpenAuthor => effects.extend(self.open_author()),
            Action::ToggleFilter(filter) => effects.extend(self.notif_toggle_filter(filter)),
            Action::ToggleAllFilters => effects.extend(self.notif_toggle_all_filters()),
            Action::SwitchSettingsTab(tab) => effects.extend(self.settings_switch_tab(tab)),
            Action::MutedLoaded { req, page } => self.apply_muted_loaded(req, page),
            Action::MutedFailed { req, failure } => {
                if self.apply_muted_failed(req, failure.code.clone())
                    && !self.settings.muted.users.is_empty()
                {
                    failure_toast = Some(toast::failure_text(&failure));
                }
            }

            Action::BlockedLoaded { req, page } => self.apply_blocked_loaded(req, page),
            Action::BlockedFailed { req, failure } => {
                if self.apply_blocked_failed(req, failure.code.clone())
                    && !self.settings.blocked.users.is_empty()
                {
                    failure_toast = Some(toast::failure_text(&failure));
                }
            }

            Action::MeLoaded { req, response } => {
                if matches!(self.session, Session::Restoring) {
                    effects.extend(self.finish_restoring(req, &response));
                } else {
                    self.apply_me_loaded(req, &response);
                }
            }
            Action::MeFailed { req, .. } => {
                self.restore_failed(req);
                if self.me_pending == Some(req) {
                    self.me_pending = None;
                }
            }

            Action::NotificationsLoaded { req, page } => {
                effects.extend(self.apply_notifications_loaded(req, *page));
            }
            Action::NotificationsFailed { req, failure } => {
                if self.apply_notifications_failed(req, failure.code.clone())
                    && !self.notifications.items.is_empty()
                {
                    failure_toast = Some(toast::failure_text(&failure));
                }
            }

            Action::GroupLoaded { id, req, page } => self.apply_group_loaded(&id, req, page),
            Action::GroupFailed {
                id,
                req,
                failure: Failure { code, .. },
            } => self.apply_group_failed(&id, req, code),
            Action::UserLoaded {
                username,
                req,
                response,
            } => {
                let mut response = *response;
                self.settle_user(&mut response);
                self.apply_user_loaded(&username, req, response)
            }
            Action::UserFailed {
                username,
                req,
                failure: Failure { code, .. },
                blocked_user,
            } => self.apply_user_failed(&username, req, code, blocked_user.map(|u| *u)),
            Action::ProfileFeedLoaded {
                username,
                tab,
                req,
                mut page,
            } => {
                self.settle_page(&mut page);
                self.apply_feed_loaded(&username, tab, req, page)
            }
            Action::ProfileFeedFailed {
                username,
                tab,
                req,
                failure,
            } => {
                if self.apply_feed_failed(&username, tab, req, &failure.code) {
                    let has_items = self
                        .profile(&username)
                        .is_some_and(|s| !s.feed(tab).items.is_empty());
                    if has_items {
                        failure_toast = Some(toast::failure_text(&failure));
                    }
                }
            }

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
                failure,
            } => {
                if self.apply_conn_failed(&username, tab, req, failure.code.clone()) {
                    let has_items = self
                        .connections(&username, tab)
                        .is_some_and(|s| !s.users.is_empty());
                    if has_items {
                        failure_toast = Some(toast::failure_text(&failure));
                    }
                }
            }

            Action::OpenConnections(tab) => effects.extend(self.open_connections(tab)),
            Action::SwitchProfileTab(tab) => effects.extend(self.profile_switch_tab(tab)),
            Action::SwitchConnTab(tab) => effects.extend(self.conn_switch_tab(tab)),
            Action::ApiStatus { rate, online } => self.apply_api_status(rate, online),
            Action::SetLastVisit { tab, id } => self.set_last_visit(tab, id),
            Action::Paste(text) if self.composer_focused() => self.paste_into_composer(&text),
            Action::Paste(text) if self.report_input_focused() => self.paste_into_report(&text),
            sign_in_action @ (Action::Paste(_)
            | Action::SignInInput(_)
            | Action::SignInBackspace
            | Action::SignInClear
            | Action::SignInSubmit
            | Action::SignInRefresh
            | Action::SignInOpenBrowser
            | Action::SignInCopyUrl
            | Action::SignInPrepared { .. }
            | Action::SignInKeyFailed { .. }
            | Action::SignInKeyMissing
            | Action::SignInPrepareFailed { .. }
            | Action::SignInFailed { .. }
            | Action::DeviceRegistered { .. }
            | Action::Notice { .. }) => effects.extend(self.on_sign_in_action(sign_in_action)),
            Action::Compose(key) => effects.extend(self.on_compose_key(key)),
            Action::CommitCreated { req, commit } => {
                effects.extend(self.commit_created(req, *commit))
            }
            Action::CommitFailed { req, failure } => self.commit_failed(req, &failure),
            Action::DraftsLoaded { account, drafts } => self.drafts_loaded(&account, drafts),
            Action::DraftsSaveFailed => self.drafts_unsaved = true,
            Action::ToggleFollow => effects.extend(self.toggle_follow()),
            Action::ToggleMute => effects.extend(self.toggle_mute()),
            Action::Block => effects.extend(self.block_key()),
            Action::ConfirmBlock => effects.extend(self.confirm_block()),
            Action::FollowDone { username, req, .. } => self.follow_done(&username, req),
            Action::FollowFailed {
                username,
                on,
                req,
                failure,
            } => self.follow_failed(&username, on, req, &failure),
            Action::MuteDone {
                username,
                req,
                response,
                ..
            } => self.mute_done(&username, req, response.is_muting),
            Action::MuteFailed {
                username,
                on,
                req,
                failure,
            } => self.mute_failed(&username, on, req, &failure),
            Action::BlockDone {
                username,
                on,
                req,
                response,
            } => effects.extend(self.block_done(
                &username,
                on,
                req,
                response.is_blocking,
                response.data,
            )),
            Action::BlockFailed {
                username,
                req,
                failure,
                ..
            } => self.block_failed(&username, req, &failure),
            Action::OpenMenu => effects.extend(self.open_menu()),
            Action::MenuKey(key) => effects.extend(self.on_menu_key(key)),
            Action::Delete => effects.extend(self.open_delete()),
            Action::ConfirmDelete => effects.extend(self.confirm_delete()),
            Action::Report => effects.extend(self.open_report()),
            Action::ReportKey(key) => effects.extend(self.on_report_key(key)),
            Action::DeleteDone { id, req } => effects.extend(self.delete_done(&id, req)),
            Action::DeleteFailed { id, req, failure } => {
                effects.extend(self.delete_failed(&id, req, &failure))
            }
            Action::ReportDone { id, req } => self.report_done(&id, req),
            Action::ReportFailed { id, req, failure } => self.report_failed(&id, req, &failure),
            Action::ToggleStar => effects.extend(self.toggle_star()),
            Action::ToggleSave => effects.extend(self.toggle_save()),
            Action::GivePastel => effects.extend(self.give_pastel()),
            Action::CopyLink => effects.extend(self.copy_link()),
            Action::OpenInBrowser => effects.extend(self.open_in_browser()),
            Action::StarDone { id, req, commit } => {
                self.flag_done(writes::WriteKind::Star, &id, req, &commit)
            }
            Action::StarFailed {
                id,
                on,
                req,
                failure,
            } => self.flag_failed(writes::WriteKind::Star, &id, on, req, &failure),
            Action::BookmarkDone { id, req, commit } => {
                self.flag_done(writes::WriteKind::Bookmark, &id, req, &commit)
            }
            Action::BookmarkFailed {
                id,
                on,
                req,
                failure,
            } => self.flag_failed(writes::WriteKind::Bookmark, &id, on, req, &failure),
            Action::PastelDone { id, req, response } => self.pastel_done(&id, req, &response),
            Action::PastelFailed { id, req, failure } => self.pastel_failed(&id, req, &failure),
            Action::Refresh => effects.extend(self.refresh()),
            Action::ThreadLoaded { id, req, response } => {
                if self
                    .threads
                    .get(&id)
                    .is_some_and(|s| s.main_pending == Some(req))
                {
                    let (main, ancestors) = self.settle_thread_response(*response);
                    let Some(state) = self.threads.get_mut(&id) else {
                        return effects;
                    };
                    state.main = Some(main);
                    state.ancestors = ancestors;
                    state.main_pending = None;
                    state.main_load = LoadState::Idle;
                    state.selected = state.ancestors.len();
                    state.scroll = 0;
                }
            }
            Action::ThreadFailed {
                id,
                req,
                failure: Failure { code, .. },
            } => {
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
                let data = self.settle_replies(page.data);
                if let Some(list) = self.reply_list_mut(&thread, &parent) {
                    list.apply_loaded(req, data, page.meta.next_cursor);
                }
                self.restore_selection(&thread, kept);
            }
            Action::RepliesFailed {
                thread,
                parent,
                req,
                failure,
            } => {
                let kept = self.selected_entry(&thread);
                if let Some(list) = self.reply_list_mut(&thread, &parent)
                    && list.apply_failed(req, &failure.code)
                    && !list.items.is_empty()
                {
                    failure_toast = Some(toast::failure_text(&failure));
                }

                self.restore_selection(&thread, kept);
            }
        }
        if let Some(text) = failure_toast {
            self.show_toast(ToastKind::Error, text);
        }
        self.reconcile_compose();
        self.reconcile_scroll();
        effects
    }

    pub fn home_tab(&self) -> LogTab {
        match self.session {
            Session::Guest => LogTab::Latest,
            Session::Restoring => LogTab::Following,
            Session::SignedIn(_) => LogTab::Following,
        }
    }

    fn needs_sign_in(&self, screen: &Screen) -> bool {
        if matches!(self.session, Session::SignedIn(_)) {
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

    /// `Err` carries the effects of opening the sign-in dialog when a guest targets a protected screen.
    fn guard_sign_in(&mut self, screen: &Screen) -> Result<(), Vec<Effect>> {
        if self.needs_sign_in(screen) {
            // The dialog would ask for a key that is already being checked.
            if matches!(self.session, Session::Guest) {
                return Err(self.open_sign_in());
            }
            return Err(Vec::new());
        }
        Ok(())
    }

    /// Replaces the screen and clears the history; `Err` when the sign-in dialog opened instead.
    pub(super) fn goto(&mut self, screen: Screen) -> Result<(), Vec<Effect>> {
        self.guard_sign_in(&screen)?;
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
        Ok(())
    }

    fn goto_tab(&mut self, tab: LogTab) -> Vec<Effect> {
        let screen = Screen::Log(tab);
        match self.guard_sign_in(&screen) {
            Ok(()) => {
                self.screen = screen;
                Vec::new()
            }
            Err(opened) => opened,
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

    pub fn settings(&self) -> &SettingsState {
        &self.settings
    }

    pub fn set_prefs(&mut self, prefs: Prefs) {
        self.prefs = prefs;
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

    pub fn own_handle(&self) -> String {
        match &self.session {
            Session::SignedIn(me) => me.handle.clone(),
            Session::Guest | Session::Restoring => String::new(),
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

    /// Whether keys go to a composer's editor.
    pub fn composer_focused(&self) -> bool {
        self.compose.is_some() && self.modal != Some(Modal::SignIn)
    }

    pub fn keyboard(&self) -> KeyboardMode {
        self.keyboard
    }

    pub fn set_keyboard(&mut self, keyboard: KeyboardMode) {
        self.keyboard = keyboard;
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
        if let Err(opened) = self.guard_sign_in(&screen) {
            return opened;
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

    fn kind(&self) -> ScreenKind {
        match &self.screen {
            Screen::Log(tab) => ScreenKind::Feed(FeedKey::Log(*tab)),
            Screen::Saved => ScreenKind::Feed(FeedKey::Saved),
            Screen::Thread(id) => ScreenKind::Thread(id.clone()),
            Screen::Profile(name) => ScreenKind::Profile(name.clone()),
            Screen::Connections(name, tab) => ScreenKind::Connections(name.clone(), *tab),
            Screen::Notifications => ScreenKind::Notifications,
            Screen::Settings => ScreenKind::Settings,
        }
    }

    fn open_selected(&mut self) -> Vec<Effect> {
        match self.kind() {
            ScreenKind::Notifications => self.notif_open_selected(),
            ScreenKind::Thread(_) => self.thread_open_selected(),
            ScreenKind::Profile(_) => self.profile_open_selected(),
            ScreenKind::Connections(..) => self.conn_open_selected(),
            ScreenKind::Settings => Vec::new(),
            ScreenKind::Feed(_) => {
                let id = self
                    .current_feed()
                    .and_then(|f| f.items.get(f.selected))
                    .map(|c| c.id.clone());
                match id {
                    Some(id) => self.open(Screen::Thread(id)),
                    None => Vec::new(),
                }
            }
        }
    }

    fn toggle_replies(&mut self) -> Vec<Effect> {
        match self.kind() {
            ScreenKind::Notifications => self.notif_toggle_group(),
            ScreenKind::Thread(_) => self.thread_toggle(),
            ScreenKind::Profile(_) => {
                self.profile_toggle();
                Vec::new()
            }
            ScreenKind::Feed(key) => {
                self.feeds.entry(key).or_default().toggle_replies();
                Vec::new()
            }
            ScreenKind::Connections(..) | ScreenKind::Settings => Vec::new(),
        }
    }

    fn open_author(&mut self) -> Vec<Effect> {
        match self.kind() {
            ScreenKind::Notifications => self.notif_open_author(),
            ScreenKind::Feed(_)
            | ScreenKind::Thread(_)
            | ScreenKind::Profile(_)
            | ScreenKind::Connections(..)
            | ScreenKind::Settings => self.feed_open_author(),
        }
    }

    fn entry_fetch(&mut self) -> Vec<Effect> {
        match self.kind() {
            ScreenKind::Settings => self.settings_entry(),
            ScreenKind::Profile(name) => {
                if self.profile(&name).is_some() {
                    return Vec::new();
                }
                self.profile_requests(&name)
            }
            ScreenKind::Connections(name, tab) => self.conn_entry(&name, tab),
            ScreenKind::Thread(id) => {
                if self.threads.contains_key(&id) {
                    return Vec::new();
                }
                self.thread_requests(&id)
            }
            ScreenKind::Notifications => Vec::new(),
            ScreenKind::Feed(_) => match self.fetchable_key() {
                Some(key) if self.feeds[&key].needs_entry_fetch() => {
                    vec![self.request(key, None, false)]
                }
                _ => Vec::new(),
            },
        }
    }

    fn refresh(&mut self) -> Vec<Effect> {
        match self.kind() {
            ScreenKind::Settings => self.settings_refresh(),
            ScreenKind::Profile(name) => self.profile_refresh(&name),
            ScreenKind::Connections(name, tab) => self.conn_refresh(&name, tab),
            ScreenKind::Thread(id) => {
                if let Some(state) = self.threads.get_mut(&id) {
                    state.children.clear();
                    state.expanded.clear();
                }
                self.thread_requests(&id)
            }
            ScreenKind::Notifications => vec![self.request_notifications(None, false)],
            ScreenKind::Feed(_) => {
                let Some(key) = self.fetchable_key() else {
                    return Vec::new();
                };
                let (cursor, append) = self.feeds[&key].refresh_target();
                vec![self.request(key, cursor, append)]
            }
        }
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
        match self.kind() {
            ScreenKind::Notifications => self.notif_select(index),
            ScreenKind::Profile(name) => self.profile_select(&name, index),
            ScreenKind::Connections(name, tab) => self.conn_select(&name, tab, index),
            ScreenKind::Thread(id) => self.thread_select(&id, index),
            ScreenKind::Settings => Vec::new(),
            ScreenKind::Feed(key) => {
                self.feeds.entry(key).or_default().select(index);
                self.prefetch()
            }
        }
    }

    fn move_selection(&mut self, step: i32) -> Vec<Effect> {
        match self.kind() {
            ScreenKind::Notifications => self.notif_move(step),
            ScreenKind::Profile(name) => {
                let current = self.profile(&name).map_or(0, |s| s.selected());
                self.profile_select(&name, current.saturating_add_signed(step as isize))
            }
            ScreenKind::Connections(name, tab) => {
                let current = self.connections(&name, tab).map_or(0, |s| s.selected);
                self.conn_select(&name, tab, current.saturating_add_signed(step as isize))
            }
            ScreenKind::Thread(id) => {
                let current = self.threads.get(&id).map_or(0, |t| t.selected);
                self.thread_select(&id, current.saturating_add_signed(step as isize))
            }
            ScreenKind::Settings => Vec::new(),
            ScreenKind::Feed(key) => {
                let feed = &self.feeds[&key];
                // A failed append must not block this: the user asked for the newest items.
                if step < 0
                    && feed.selected == 0
                    && feed.new_commits > 0
                    && self.fetchable_key().is_some()
                {
                    return vec![self.request(key, None, false)];
                }
                let target = feed.selected.saturating_add_signed(step as isize);
                self.select(target)
            }
        }
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
        let key = match self.kind() {
            ScreenKind::Notifications => return self.reconcile_notifications_scroll(),
            ScreenKind::Profile(name) => return self.reconcile_profile_scroll(&name),
            ScreenKind::Connections(name, tab) => return self.reconcile_conn_scroll(&name, tab),
            ScreenKind::Thread(id) => return self.reconcile_thread_scroll(&id),
            ScreenKind::Settings => return,
            ScreenKind::Feed(key) => key,
        };
        let Some(main) = self.main_area() else {
            return;
        };
        let tabs = if matches!(key, FeedKey::Log(_)) { 2 } else { 0 };
        let view = main.height.saturating_sub(2 + tabs) as usize;
        if matches!(key, FeedKey::Log(_))
            && let Some(height) = crate::ui::compose::inline_height(self)
        {
            let scroll = ensure_visible(0, height, self.feeds[&key].scroll as usize, view);
            self.feeds.entry(key).or_default().scroll = scroll.min(u16::MAX as usize) as u16;
            return;
        }
        let feed = &self.feeds[&key];
        let boxed = self
            .compose
            .as_ref()
            .is_some_and(|c| c.surface == crate::compose::Surface::ReplyBox);
        let scroll = if feed.selected == 0 && !boxed {
            0
        } else {
            let rows = crate::ui::feed::rows(feed, key, self, main.width.saturating_sub(2));
            let total = rows.lines.len();
            let scroll = feed.scroll as usize;
            let boxed = box_span(rows.cursor, rows.box_end);
            match crate::ui::feed::card_extent(&rows.cards, feed.selected, total) {
                Some(extent) => settle_card_scroll(extent, boxed, total, scroll, view),
                None => scroll.min(total.saturating_sub(view)),
            }
        };
        self.feeds.entry(key).or_default().scroll = scroll.min(u16::MAX as usize) as u16;
    }
}
