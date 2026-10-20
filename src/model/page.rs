use serde::Deserialize;

use super::commit::{Commit, ThreadItem};
use super::user::{CurrentUser, User};

#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
pub struct Meta {
    #[serde(default)]
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Page<T> {
    pub data: Vec<T>,
    #[serde(default)]
    pub meta: Meta,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct LogPage {
    pub data: Vec<Commit>,
    #[serde(default)]
    pub meta: Meta,
    pub log: String,
    pub is_ranked: bool,
    pub new_commits_count: Option<u32>,
    #[serde(default)]
    pub suggestions: Vec<Commit>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ThreadResponse {
    pub data: Commit,
    #[serde(default)]
    pub ancestors: Vec<ThreadItem>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct MeResponse {
    pub data: CurrentUser,
    pub unread_notifications_count: u32,
}

/// The extra fields come only with a signed request, so a guest gets `data` alone.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct UserResponse {
    pub data: User,
    pub is_following: Option<bool>,
    pub follows_you: Option<bool>,
    pub is_muting: Option<bool>,
    pub pasteis_received: Option<u32>,
    pub pinned_commit: Option<Commit>,
}
