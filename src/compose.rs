//! The state of an open composer, and the character counter under it.

use crate::drafts::DraftKey;
use crate::feed::ReqId;

/// Where an open composer is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    /// Under the Log tabs.
    Inline,
    /// A dialog over any other screen.
    Modal,
    /// Under a commit card.
    ReplyBox,
}

/// An open composer; the text it edits is the draft of `key`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Compose {
    pub key: DraftKey,
    pub surface: Surface,
    /// Whether the commit of `key` is out; `App::in_flight` holds the rest.
    pub submitting: bool,
    /// The last failure, shown under the text.
    pub error: Option<String>,
    /// The first wrapped row shown, for text taller than the field.
    pub view_top: usize,
}

/// The counter shows from this many characters left.
pub const COUNTER_SHOWN_AT: i64 = 200;
/// The counter turns yellow from this many characters left.
pub const COUNTER_WARN_AT: i64 = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CounterTone {
    Normal,
    /// 20 or fewer left.
    Warn,
    /// 0 or fewer left.
    Over,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Counter {
    /// Negative when the body is too long.
    pub remaining: i64,
    pub tone: CounterTone,
}

/// The counter for a body of `body_len` characters, or `None` while more than 200 are left.
pub fn counter(body_len: usize) -> Option<Counter> {
    let remaining = crate::body::MAX_BODY_LEN as i64 - body_len as i64;
    if remaining > COUNTER_SHOWN_AT {
        return None;
    }
    let tone = if remaining <= 0 {
        CounterTone::Over
    } else if remaining <= COUNTER_WARN_AT {
        CounterTone::Warn
    } else {
        CounterTone::Normal
    };
    Some(Counter { remaining, tone })
}

/// A commit that has been sent and not yet answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InFlight {
    pub req: ReqId,
    pub key: DraftKey,
    pub sent_text: String,
}
