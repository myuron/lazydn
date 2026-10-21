use std::collections::{HashMap, HashSet};

use time::OffsetDateTime;

use crate::feed::{LoadState, Pending, ReqId};
use crate::model::Notification;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Filter {
    Replies,
    Quotes,
    Stars,
    Mentions,
    Follows,
    Moderation,
}

impl Filter {
    /// The order of the `1`..`6` keys and of the `type` query.
    pub const ALL: [Filter; 6] = [
        Filter::Replies,
        Filter::Quotes,
        Filter::Stars,
        Filter::Mentions,
        Filter::Follows,
        Filter::Moderation,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Filter::Replies => "Replies",
            Filter::Quotes => "Quotes",
            Filter::Stars => "Stars",
            Filter::Mentions => "Mentions",
            Filter::Follows => "Follows",
            Filter::Moderation => "Moderation",
        }
    }

    pub fn index(self) -> usize {
        self as usize
    }

    /// The word the `type` query uses.
    pub fn name(self) -> &'static str {
        match self {
            Filter::Replies => "replies",
            Filter::Quotes => "quotes",
            Filter::Stars => "stars",
            Filter::Mentions => "mentions",
            Filter::Follows => "follows",
            Filter::Moderation => "moderation",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Filters([bool; 6]);

impl Default for Filters {
    fn default() -> Self {
        Self([true; 6])
    }
}

impl Filters {
    pub fn is_on(&self, filter: Filter) -> bool {
        self.0[filter.index()]
    }

    pub fn toggle(&mut self, filter: Filter) {
        self.0[filter.index()] ^= true;
    }

    pub fn all_on(&self) -> bool {
        self.0.iter().all(|on| *on)
    }

    pub fn set_all(&mut self, on: bool) {
        self.0 = [on; 6];
    }

    /// The `type` query value: `all`, `none`, or the enabled names in `Filter::ALL` order.
    pub fn types(&self) -> String {
        if self.all_on() {
            return "all".to_string();
        }
        let names: Vec<&str> = Filter::ALL
            .into_iter()
            .filter(|f| self.is_on(*f))
            .map(Filter::name)
            .collect();
        if names.is_empty() {
            "none".to_string()
        } else {
            names.join(",")
        }
    }
}

/// The individual notifications behind one grouped row.
#[derive(Debug, Clone, Default)]
pub struct GroupList {
    pub items: Vec<Notification>,
    pub next_cursor: Option<String>,
    pub pending: Option<ReqId>,
    pub load: LoadState,
}

#[derive(Debug, Clone, Default)]
pub struct NotificationsState {
    pub items: Vec<Notification>,
    pub next_cursor: Option<String>,
    pub pending: Option<Pending>,
    pub load: LoadState,
    pub loaded_once: bool,
    pub selected: usize,
    pub scroll: u16,
    pub filters: Filters,
    /// Top-level `read_at` of the latest first-page response.
    pub read_at: Option<OffsetDateTime>,
    pub new_count: u32,
    /// Keyed by `group_id`.
    pub groups: HashMap<String, GroupList>,
    /// `group_id`s of the rows that are open.
    pub expanded: HashSet<String>,
}
