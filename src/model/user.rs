use serde::{Deserialize, Deserializer};
use time::OffsetDateTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lane {
    Index(u8),
    Custom(Rgb),
}

/// derive では範囲外の値で User 全体のパースが失敗するため、使わない。
pub(crate) fn lane_opt<'de, D>(d: D) -> Result<Option<Lane>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Raw {
        Int(i64),
        Str(String),
        Other(serde::de::IgnoredAny),
    }
    Ok(match Raw::deserialize(d)? {
        Raw::Int(n) => u8::try_from(n).ok().filter(|n| *n < 6).map(Lane::Index),
        Raw::Str(s) => parse_hex(&s).map(Lane::Custom),
        Raw::Other(_) => None,
    })
}

fn parse_hex(s: &str) -> Option<Rgb> {
    let hex = s.strip_prefix('#')?;
    let valid = hex.len() == 6
        && hex
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    if !valid {
        return None;
    }
    let c = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    Some(Rgb(c(0)?, c(2)?, c(4)?))
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct User {
    pub id: String,
    pub username: String,
    pub name: Option<String>,
    pub avatar_url: Option<String>,
    pub bio: Option<String>,
    pub company: Option<String>,
    pub location: Option<String>,
    pub website: Option<String>,
    pub github_url: Option<String>,
    pub html_url: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    pub commits_count: Option<u64>,
    pub followers_count: Option<u64>,
    pub following_count: Option<u64>,
    #[serde(default, deserialize_with = "lane_opt")]
    pub lane: Option<Lane>,
    pub is_followed: Option<bool>,
    pub follows_you: Option<bool>,
}

impl User {
    pub fn effective_lane(&self) -> Lane {
        self.lane
            .unwrap_or_else(|| Lane::Index(crate::lane::auto_lane(&self.id)))
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct CurrentUser {
    #[serde(flatten)]
    pub user: User,
    pub pasteis_mode: bool,
    pub pasteis_remaining: u32,
    #[serde(with = "time::serde::rfc3339")]
    pub pasteis_resets_at: OffsetDateTime,
    pub pasteis_received: u32,
    #[serde(default, deserialize_with = "lane_opt")]
    pub preferred_lane: Option<Lane>,
    pub automatic_lane: u8,
    pub sponsors_url: Option<String>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub github_synced_at: Option<OffsetDateTime>,
}
