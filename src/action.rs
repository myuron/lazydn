use time::OffsetDateTime;

use crate::app::{ConnTab, LogTab, ProfileTab, Screen};
use crate::feed::{FeedKey, FeedPage, ReqId};
use crate::model::{Page, ThreadItem, ThreadResponse, User, UserResponse};

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
    /// `code` is an API error code or `http_<status>`.
    FeedFailed {
        key: FeedKey,
        req: ReqId,
        code: String,
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
    /// `code` is an API error code or `http_<status>`.
    ThreadFailed {
        id: String,
        req: ReqId,
        code: String,
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
        code: String,
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
        code: String,
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
        code: String,
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
        code: String,
    },
    /// `@`: opens the profile of the selected card's author.
    OpenAuthor,
    /// `3` / `4` on a profile; pushes onto the history.
    OpenConnections(ConnTab),
    /// `1` / `2` on a profile.
    SwitchProfileTab(ProfileTab),
    /// `1` / `2` on Followers / Following; replaces the screen without a history entry.
    SwitchConnTab(ConnTab),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    Quit,
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
}
