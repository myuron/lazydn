use serde::Deserialize;
use time::OffsetDateTime;

use super::commit::ThreadItem;
use super::page::{Meta, Page};
use super::user::User;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NotificationType {
    Followed,
    Replied,
    Mentioned,
    Approved,
    Rejected,
    Quoted,
    Starred,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Notification {
    pub id: String,
    pub group_id: String,
    #[serde(rename = "type")]
    pub kind: NotificationType,
    pub subject_id: Option<String>,
    pub reason: Option<String>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub read_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    pub actor: Option<User>,
    /// Absent from the group endpoint, and at most three users in the list.
    #[serde(default)]
    pub actors: Vec<User>,
    pub event_count: u32,
    pub actors_count: u32,
    pub unread_count: u32,
    pub commit: Option<ThreadItem>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct NotificationFilters {
    #[serde(rename = "type")]
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct NotificationsPage {
    pub data: Vec<Notification>,
    #[serde(default)]
    pub meta: Meta,
    /// When the server marked everything read, as of this response.
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub read_at: Option<OffsetDateTime>,
    #[serde(default)]
    pub filters: Option<NotificationFilters>,
}

/// The group endpoint returns a plain cursor page.
pub type GroupPage = Page<Notification>;
