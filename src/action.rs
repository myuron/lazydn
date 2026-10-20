use time::OffsetDateTime;

use crate::api::client::RateLimit;
use crate::api::keys::KeyError;
use crate::api::pkce::Pkce;
use crate::app::{ConnTab, LogTab, ProfileTab, Screen, SettingsTab, ToastKind};
use crate::drafts::Drafts;
use crate::feed::{FeedKey, FeedPage, ReqId};
use crate::model::{
    Commit, GroupPage, MeResponse, NotificationsPage, Page, ThreadItem, ThreadResponse, User,
    UserResponse,
};
use crate::notifications::Filter;

/// A value that `Debug` never prints, so that logs and panic messages cannot leak it.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    /// The hidden text, for the one place that sends it.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl From<String> for Secret {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for Secret {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(..)")
    }
}

/// Why a request failed, as the executor reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    /// An API error code, or `http_<status>`, `network`, `decode`, `sign` and so on.
    pub code: String,
    /// Seconds from `Retry-After`, when the server sent it.
    pub retry_after: Option<u64>,
    /// The API `message` of an HTTP error, as the server wrote it; not yet cleaned for display.
    pub message: Option<String>,
}

impl Failure {
    /// A failure that carries no `Retry-After`.
    pub fn new(code: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            retry_after: None,
            message: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Quit,
    /// Sent once at startup so that the first screen requests its data.
    Init,
    Tick(OffsetDateTime),
    Resize(u16, u16),
    /// Pushes onto the history so that Esc can return.
    Open(Screen),
    /// Replaces the screen and clears the history (g keys, Nav).
    Goto(Screen),
    Back,
    StartGPrefix,
    CancelGPrefix,
    FocusNext,
    FocusPrev,
    NavStep(i8),
    NavEnter,
    SwitchTab(LogTab),
    OpenHelp,
    OpenSignIn,
    CloseModal,
    Commit,
    FeedLoaded {
        key: FeedKey,
        req: ReqId,
        page: FeedPage,
    },
    FeedFailed {
        key: FeedKey,
        req: ReqId,
        failure: Failure,
    },
    NewCommits {
        key: FeedKey,
        since: String,
        count: u32,
    },
    MoveSelection(i32),
    SelectFirst,
    SelectLast,
    OpenSelected,
    ToggleReplies,
    Refresh,
    ThreadLoaded {
        id: String,
        req: ReqId,
        response: Box<ThreadResponse>,
    },
    ThreadFailed {
        id: String,
        req: ReqId,
        failure: Failure,
    },
    /// `parent == thread` for the first level of replies.
    RepliesLoaded {
        thread: String,
        parent: String,
        req: ReqId,
        page: Page<ThreadItem>,
    },
    RepliesFailed {
        thread: String,
        parent: String,
        req: ReqId,
        failure: Failure,
    },
    UserLoaded {
        username: String,
        req: ReqId,
        response: Box<UserResponse>,
    },
    /// `blocked_user` is the `user` field of a `user_blocked` error.
    UserFailed {
        username: String,
        req: ReqId,
        failure: Failure,
        blocked_user: Option<Box<User>>,
    },
    ProfileFeedLoaded {
        username: String,
        tab: ProfileTab,
        req: ReqId,
        page: FeedPage,
    },
    ProfileFeedFailed {
        username: String,
        tab: ProfileTab,
        req: ReqId,
        failure: Failure,
    },
    ConnectionsLoaded {
        username: String,
        tab: ConnTab,
        req: ReqId,
        page: Page<User>,
    },
    ConnectionsFailed {
        username: String,
        tab: ConnTab,
        req: ReqId,
        failure: Failure,
    },
    MeLoaded {
        req: ReqId,
        response: Box<MeResponse>,
    },
    MeFailed {
        req: ReqId,
        failure: Failure,
    },
    NotificationsLoaded {
        req: ReqId,
        page: Box<NotificationsPage>,
    },
    NotificationsFailed {
        req: ReqId,
        failure: Failure,
    },
    /// `id` is the id of the notification that was asked, not a `group_id`.
    GroupLoaded {
        id: String,
        req: ReqId,
        page: GroupPage,
    },
    GroupFailed {
        id: String,
        req: ReqId,
        failure: Failure,
    },
    /// `1`..`6` on Notifications.
    ToggleFilter(Filter),
    /// `a` on Notifications.
    ToggleAllFilters,
    /// `@`: opens the profile of the selected card's author.
    OpenAuthor,
    /// `3` / `4` on a profile; pushes onto the history.
    OpenConnections(ConnTab),
    /// `1` / `2` on a profile.
    SwitchProfileTab(ProfileTab),
    /// `1`..`3` on Settings.
    SwitchSettingsTab(SettingsTab),
    MutedLoaded {
        req: ReqId,
        page: Page<User>,
    },
    MutedFailed {
        req: ReqId,
        failure: Failure,
    },
    BlockedLoaded {
        req: ReqId,
        page: Page<User>,
    },
    BlockedFailed {
        req: ReqId,
        failure: Failure,
    },
    /// `1` / `2` on Followers / Following; replaces the screen without a history entry.
    SwitchConnTab(ConnTab),
    /// Sent by the executor after each response. `rate: None` keeps the previous value.
    ApiStatus {
        rate: Option<RateLimit>,
        online: bool,
    },
    /// The newest commit id the last session saw on a tab; main sends it once at startup.
    SetLastVisit {
        tab: LogTab,
        id: String,
    },
    /// A key typed while a composer has the focus.
    Compose(crate::keymap::ComposerKey),
    /// A bracketed paste. Only the sign-in dialog takes it; elsewhere it is dropped.
    Paste(String),
    /// A character typed into the Code field of the sign-in dialog.
    SignInInput(char),
    SignInBackspace,
    /// `Ctrl+U` in the dialog: empties the Code field.
    SignInClear,
    /// `Enter` in the dialog; what it does depends on the step.
    SignInSubmit,
    /// `Ctrl+R` in the dialog: reads the key again and makes a new approval URL.
    SignInRefresh,
    /// `Ctrl+O` in the dialog.
    SignInOpenBrowser,
    /// `Ctrl+Y` in the dialog.
    SignInCopyUrl,
    /// The executor read or created the key and built the approval URL.
    SignInPrepared {
        fingerprint: String,
        url: String,
        pkce: Pkce,
        state: String,
        /// Whether this call made the key file.
        created: bool,
    },
    /// The key could not be read or created.
    SignInKeyFailed {
        error: KeyError,
    },
    /// `prepare(false)` found no key file, so the dialog goes back to offering to create one.
    SignInKeyMissing,
    /// The approval URL could not be built, so the dialog can only offer to try again.
    SignInPrepareFailed {
        failure: Failure,
    },
    /// Registering the device failed: the server refused the code, or the request did not get through.
    SignInFailed {
        failure: Failure,
    },
    DeviceRegistered {
        response: Box<MeResponse>,
    },
    /// A message from the executor for the toast, such as a browser that would not start.
    Notice {
        kind: ToastKind,
        text: String,
    },
    /// The server created the commit that `Effect::CreateCommit` sent; applied whatever the age of `req`, since
    /// the commit exists either way.
    CommitCreated {
        req: ReqId,
        commit: Box<Commit>,
    },
    CommitFailed {
        req: ReqId,
        failure: Failure,
    },
    /// The write of the drafts failed, so they are still unsaved.
    DraftsSaveFailed,
    /// The drafts of `account` read from disk; they fill only the slots that hold no text yet.
    DraftsLoaded {
        account: String,
        drafts: Drafts,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    Quit,
    /// Makes later requests unsigned. The runtime applies it before it spawns the effects after it.
    UseGuestClient,
    /// Makes later requests signed with the device key. Applied in order like `UseGuestClient`.
    UseSignedClient,
    FetchLog {
        tab: LogTab,
        cursor: Option<String>,
        req: ReqId,
    },
    FetchSaved {
        cursor: Option<String>,
        req: ReqId,
    },
    PollLog {
        tab: LogTab,
        since: String,
    },
    FetchThread {
        id: String,
        req: ReqId,
    },
    /// `parent == thread` asks for the first level of replies.
    FetchReplies {
        thread: String,
        parent: String,
        cursor: Option<String>,
        req: ReqId,
    },
    FetchUser {
        username: String,
        req: ReqId,
    },
    FetchUserCommits {
        username: String,
        replies: bool,
        cursor: Option<String>,
        req: ReqId,
    },
    FetchConnections {
        username: String,
        tab: ConnTab,
        cursor: Option<String>,
        req: ReqId,
    },
    FetchMe {
        req: ReqId,
    },
    FetchMuted {
        cursor: Option<String>,
        req: ReqId,
    },
    FetchBlocked {
        cursor: Option<String>,
        req: ReqId,
    },
    /// Marks every notification read on the server, so it must only follow an explicit user step.
    /// `types` is `all`, `none` or a comma list.
    FetchNotifications {
        types: String,
        cursor: Option<String>,
        req: ReqId,
    },
    /// `id` is the id of one notification in the group; this call does not mark anything read.
    FetchNotificationGroup {
        id: String,
        types: String,
        cursor: Option<String>,
        req: ReqId,
    },
    /// Reads the device key, or makes it when `create` is set, then builds the approval URL.
    PrepareSignIn {
        create: bool,
    },
    /// Starts the browser; only ever follows an explicit key press.
    OpenUrl(String),
    /// Writes the text to the terminal clipboard (OSC 52).
    CopyToClipboard(String),
    /// Redeems the approval code.
    RegisterDevice {
        code: String,
        verifier: Secret,
    },
    /// Posts a commit; `parent_id` and `quoted_commit_id` are never both set.
    CreateCommit {
        body: String,
        parent_id: Option<String>,
        quoted_commit_id: Option<String>,
        req: ReqId,
    },
    /// Writes the drafts of `account` to disk. The runtime does it on the spot, not on a task.
    SaveDrafts {
        account: String,
        drafts: Drafts,
    },
    /// Reads the drafts of `account`. The runtime does it on the spot and answers with `Action::DraftsLoaded`.
    LoadDrafts {
        account: String,
    },
}
