use serde::{Deserialize, Deserializer};
use time::OffsetDateTime;

use super::user::User;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Pending,
    Approved,
    Rejected,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Segment {
    Text {
        text: String,
    },
    Code {
        code: String,
    },
    Block {
        language: Option<String>,
        html: String,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Mention {
    pub username: String,
    pub offset: usize,
    pub length: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EmbedKind {
    Video,
    Audio,
    Code,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Embed {
    pub provider: String,
    pub kind: EmbedKind,
    pub src: String,
    pub aspect_ratio: Option<String>,
    pub height: Option<u32>,
    pub poster: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct LinkPreview {
    pub url: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub image_url: Option<String>,
    pub site_name: Option<String>,
    pub is_pending: bool,
    pub embed: Option<Embed>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Image {
    pub id: String,
    pub width: u32,
    pub height: u32,
    pub description: Option<String>,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct PollOption {
    pub id: String,
    pub label: String,
    pub votes_count: Option<u32>,
    pub voted: bool,
    pub percentage: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Poll {
    pub id: String,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub closes_at: Option<OffsetDateTime>,
    pub is_closed: bool,
    pub has_voted: bool,
    pub total_votes: Option<u32>,
    pub options: Vec<PollOption>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Tombstone {
    pub id: String,
    pub parent_id: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub deleted_at: OffsetDateTime,
    pub replies_count: Option<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ThreadItem {
    Commit(Box<Commit>),
    Tombstone(Tombstone),
}

// untagged にすると、壊れた Commit が Tombstone に落ちてエラーが隠れるため、手書きにしている。
impl<'de> Deserialize<'de> for ThreadItem {
    fn deserialize<D>(d: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::Error;
        let value = serde_json::Value::deserialize(d)?;
        let is_tombstone =
            value.get("author").is_none() && value.get("deleted_at").is_some_and(|v| !v.is_null());
        if is_tombstone {
            serde_json::from_value(value)
                .map(ThreadItem::Tombstone)
                .map_err(D::Error::custom)
        } else {
            serde_json::from_value(value)
                .map(|c| ThreadItem::Commit(Box::new(c)))
                .map_err(D::Error::custom)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub enum Quoted {
    #[default]
    NotLoaded,
    Unavailable,
    Item(Box<ThreadItem>),
}

impl<'de> Deserialize<'de> for Quoted {
    fn deserialize<D>(d: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(match Option::<ThreadItem>::deserialize(d)? {
            None => Quoted::Unavailable,
            Some(item) => Quoted::Item(Box::new(item)),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ReplyPreview {
    pub data: Vec<ThreadItem>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Commit {
    pub id: String,
    pub short_hash: String,
    pub body: String,
    pub parent_id: Option<String>,
    pub html_url: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    pub stars_count: Option<u32>,
    pub replies_count: Option<u32>,
    pub link_preview: Option<LinkPreview>,
    #[serde(default)]
    pub images: Vec<Image>,
    pub author: User,
    pub body_segments: Option<Vec<Segment>>,
    pub preview_segments: Option<Vec<Segment>>,
    pub mentions: Option<Vec<Mention>>,
    pub status: Option<Status>,
    pub is_pinned: Option<bool>,
    pub starred: Option<bool>,
    pub is_saved: Option<bool>,
    pub pasteis_count: Option<u32>,
    pub quoted_commit_id: Option<String>,
    #[serde(default)]
    pub quoted_commit: Quoted,
    pub poll: Option<Poll>,
    pub reply_preview: Option<ReplyPreview>,
}
