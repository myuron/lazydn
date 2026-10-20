use serde::Deserialize;
use time::OffsetDateTime;

use super::commit::Commit;
use super::user::{Rgb, User};

/// The commit after a pastel was given, with the giver's allowance; show the remaining count and the author's total.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct PastelResponse {
    /// The commit with its new `pasteis_count`.
    pub data: Commit,
    /// Pastéis the viewer can still give today.
    pub pasteis_remaining: u32,
    /// When the daily allowance is restored.
    #[serde(with = "time::serde::rfc3339")]
    pub pasteis_resets_at: OffsetDateTime,
    /// Pastéis the author has received in all; absent when the author is gone.
    pub author_pasteis_received: Option<u32>,
}

/// The user after a mute or unmute.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct MuteResponse {
    /// The user acted on.
    pub data: User,
    /// Whether the viewer now mutes them.
    pub is_muting: bool,
}

/// The user after a block or unblock.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct BlockResponse {
    /// The user acted on.
    pub data: User,
    /// Whether the viewer now blocks them.
    pub is_blocking: bool,
}

/// What to put in `PUT /me/lane`; the three forms are mutually exclusive on the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaneSetting {
    /// Let the server derive the lane from the user id.
    Automatic,
    /// A palette lane, `0..=5`.
    Palette(u8),
    /// A custom color.
    Color(Rgb),
}

/// Why a commit is reported; the wire value is the lowercase name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportReason {
    /// Spam or misleading links.
    Spam,
    /// Harassment or hate.
    Harassment,
    /// Harmful or private information.
    Unsafe,
    /// Something else.
    Other,
}

impl ReportReason {
    /// The value the server expects in `reason`.
    pub fn as_str(self) -> &'static str {
        match self {
            ReportReason::Spam => "spam",
            ReportReason::Harassment => "harassment",
            ReportReason::Unsafe => "unsafe",
            ReportReason::Other => "other",
        }
    }
}
