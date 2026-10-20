use std::path::Path;

use time::OffsetDateTime;

use super::App;
use crate::action::{Action, Failure};
use crate::api::keys::KeyError;
use crate::feed::ReqId;
use crate::text::is_unsafe_char;

/// Picks the symbol and color of a toast.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Success,
    Error,
    Info,
}

/// A short message over the status line; only the newest one is kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Toast {
    pub kind: ToastKind,
    /// Without the kind's symbol, which the renderer adds.
    pub text: String,
    /// The first Tick at or after this time removes the toast.
    pub until: OffsetDateTime,
}

impl App {
    /// The toast on show, if any.
    pub fn toast(&self) -> Option<&Toast> {
        self.toast.as_ref()
    }
}

/// Seconds a toast stays up, counted from the clock of the app.
const TOAST_SECS: i64 = 3;
/// Server strings are cut here so that one cannot fill the screen.
const TOAST_CODE_MAX: usize = 64;

/// Drops control and bidi characters before cutting, so that they cannot eat into the length.
pub(super) fn clean(text: &str) -> String {
    text.chars()
        .filter(|c| !is_unsafe_char(*c))
        .take(TOAST_CODE_MAX)
        .collect()
}

pub(super) fn failure_text(failure: &Failure) -> String {
    match (failure.code.as_str(), failure.retry_after) {
        ("rate_limited", Some(secs)) => format!("rate_limited: retry in {secs}s"),
        ("invalid_signature", _) => "invalid_signature: check your system clock".into(),
        ("device_not_approved", _) => "this device is no longer approved — sign in again".into(),
        ("network", _) => "network error — press R to retry".into(),
        (code, _) => clean(code),
    }
}

/// Why a commit was refused. A timeout is worded apart because the server may have created the commit.
pub(super) fn commit_failure_text(failure: &Failure) -> String {
    match (failure.code.as_str(), failure.retry_after, &failure.message) {
        ("network", _, _) => {
            "network error — check your log before sending again (it may have been posted)".into()
        }
        ("rate_limited", Some(_), _) | (_, _, None) => failure_text(failure),
        (_, _, Some(message)) => clean(message),
    }
}

/// Paths come from the environment, so control characters are dropped like in server strings.
fn clean_path(path: &Path) -> String {
    path.display()
        .to_string()
        .chars()
        .filter(|c| !is_unsafe_char(*c))
        .collect()
}

pub(super) fn key_error_text(error: &KeyError, path: &Path) -> String {
    let p = clean_path(path);
    match error {
        KeyError::InsecurePermissions => {
            format!("key {p} is readable by others — run chmod 600 {p}")
        }
        KeyError::Encrypted => {
            format!("key {p} is encrypted — lazydn needs an unencrypted ed25519 key")
        }
        KeyError::NotEd25519 => format!("key {p} is not an ed25519 key"),
        KeyError::Parse(_) => format!("key {p} could not be parsed"),
        KeyError::NotARegularFile => format!("{p} is not a regular file"),
        KeyError::Io(kind) => format!("key {p} could not be read ({})", clean(kind)),
        KeyError::Generate(_) => format!("key {p} could not be created"),
    }
}

/// The `Failure` inside any `*Failed` action; `None` for every other action.
pub(super) fn failure_in(action: &Action) -> Option<&Failure> {
    match action {
        Action::FeedFailed { failure, .. }
        | Action::ThreadFailed { failure, .. }
        | Action::RepliesFailed { failure, .. }
        | Action::UserFailed { failure, .. }
        | Action::ProfileFeedFailed { failure, .. }
        | Action::ConnectionsFailed { failure, .. }
        | Action::MeFailed { failure, .. }
        | Action::NotificationsFailed { failure, .. }
        | Action::GroupFailed { failure, .. }
        | Action::MutedFailed { failure, .. }
        | Action::BlockedFailed { failure, .. }
        | Action::CommitFailed { failure, .. } => Some(failure),
        _ => None,
    }
}

/// The request a `*Failed` action answers; `None` for every other action.
pub(super) fn req_in(action: &Action) -> Option<ReqId> {
    match action {
        Action::FeedFailed { req, .. }
        | Action::ThreadFailed { req, .. }
        | Action::RepliesFailed { req, .. }
        | Action::UserFailed { req, .. }
        | Action::ProfileFeedFailed { req, .. }
        | Action::ConnectionsFailed { req, .. }
        | Action::MeFailed { req, .. }
        | Action::NotificationsFailed { req, .. }
        | Action::GroupFailed { req, .. }
        | Action::MutedFailed { req, .. }
        | Action::BlockedFailed { req, .. }
        | Action::CommitFailed { req, .. } => Some(*req),
        _ => None,
    }
}

impl App {
    /// Shows the newest message only; the same message again just stays up longer.
    pub(super) fn show_toast(&mut self, kind: ToastKind, text: String) {
        let until = self.now + time::Duration::seconds(TOAST_SECS);
        match &mut self.toast {
            Some(t) if t.kind == kind && t.text == text => t.until = until,
            slot => *slot = Some(Toast { kind, text, until }),
        }
    }

    pub(super) fn expire_toast(&mut self) {
        if self.toast.as_ref().is_some_and(|t| t.until <= self.now) {
            self.toast = None;
        }
    }
}
