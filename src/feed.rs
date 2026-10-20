use std::collections::HashSet;

use time::OffsetDateTime;

use crate::app::LogTab;
use crate::model::{Commit, LogPage, Page};

/// Identifies one list so that its selection and scroll survive navigation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FeedKey {
    Log(LogTab),
    Saved,
}

impl FeedKey {
    pub const ALL: [FeedKey; 4] = [
        FeedKey::Log(LogTab::Following),
        FeedKey::Log(LogTab::Latest),
        FeedKey::Log(LogTab::Trending),
        FeedKey::Saved,
    ];
}

/// Tags a fetch so that a response to a superseded request can be dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReqId(pub u64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pending {
    pub req: ReqId,
    pub append: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum LoadState {
    #[default]
    Idle,
    Failed {
        code: String,
        append: bool,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct FeedPage {
    pub items: Vec<Commit>,
    pub next_cursor: Option<String>,
    pub is_ranked: bool,
    pub suggestions: Vec<Commit>,
}

impl From<LogPage> for FeedPage {
    fn from(page: LogPage) -> Self {
        Self {
            items: page.data,
            next_cursor: page.meta.next_cursor,
            is_ranked: page.is_ranked,
            suggestions: page.suggestions,
        }
    }
}

impl From<Page<Commit>> for FeedPage {
    fn from(page: Page<Commit>) -> Self {
        Self {
            items: page.data,
            next_cursor: page.meta.next_cursor,
            is_ranked: false,
            suggestions: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Feed {
    pub items: Vec<Commit>,
    pub selected: usize,
    pub scroll: u16,
    pub next_cursor: Option<String>,
    pub load: LoadState,
    pub pending: Option<Pending>,
    pub loaded_once: bool,
    pub is_ranked: bool,
    pub new_commits: u32,
    pub suggestions: Vec<Commit>,
    pub expanded: HashSet<String>,
    pub last_poll: Option<OffsetDateTime>,
}

/// Seconds between two polls of the same feed.
const POLL_INTERVAL_SECS: i64 = 30;
/// Items before the end at which the next page is requested.
pub const PREFETCH_MARGIN: usize = 5;

impl Feed {
    /// Loading is derived from `pending` so that the two can never disagree.
    pub fn is_loading(&self) -> bool {
        self.pending.is_some()
    }

    /// Supersedes any earlier request, so that its late response is dropped.
    pub fn begin(&mut self, req: ReqId, append: bool) {
        self.pending = Some(Pending { req, append });
        self.load = LoadState::Idle;
    }

    /// True when the first page was never fetched, or the only attempt so far failed.
    pub fn needs_entry_fetch(&self) -> bool {
        let failed_first = self.items.is_empty() && matches!(self.load, LoadState::Failed { .. });
        self.pending.is_none() && (!self.loaded_once || failed_first)
    }

    /// Returns the cursor and append flag that a refresh should request.
    ///
    /// After a failed append only that page is retried, so the list the user is reading stays.
    pub fn refresh_target(&self) -> (Option<String>, bool) {
        match self.load {
            LoadState::Failed { append: true, .. } => (self.next_cursor.clone(), true),
            _ => (None, false),
        }
    }

    pub fn prefetch_cursor(&self) -> Option<String> {
        let near_end = !self.items.is_empty() && near_end(self.selected, self.items.len());
        let append_failed = matches!(self.load, LoadState::Failed { append: true, .. });
        if near_end && self.pending.is_none() && !append_failed {
            self.next_cursor.clone()
        } else {
            None
        }
    }

    /// Returns the id to poll from once 30 seconds have passed since the last poll or load.
    pub fn poll_since(&self, now: OffsetDateTime) -> Option<String> {
        let due = self
            .last_poll
            .is_some_and(|t| (now - t).whole_seconds() >= POLL_INTERVAL_SECS);
        self.items.first().filter(|_| due).map(|c| c.id.clone())
    }

    /// Returns false when `req` is not the pending request, in which case nothing changes.
    pub fn apply_loaded(&mut self, req: ReqId, page: FeedPage, now: OffsetDateTime) -> bool {
        let append = match &self.pending {
            Some(p) if p.req == req => p.append,
            _ => return false,
        };
        if append {
            for item in page.items {
                if !self.items.iter().any(|c| c.id == item.id) {
                    self.items.push(item);
                }
            }
        } else {
            self.items = page.items;
            self.selected = 0;
            self.scroll = 0;
            self.new_commits = 0;
            self.expanded.clear();
            self.is_ranked = page.is_ranked;
            self.suggestions = page.suggestions;
            self.last_poll = Some(now);
        }
        self.next_cursor = page.next_cursor;
        self.pending = None;
        self.load = LoadState::Idle;
        self.loaded_once = true;
        true
    }

    /// Returns false when `req` is not the pending request, in which case nothing changes.
    pub fn apply_failed(&mut self, req: ReqId, code: &str) -> bool {
        let append = match &self.pending {
            Some(p) if p.req == req => p.append,
            _ => return false,
        };
        self.pending = None;
        self.load = LoadState::Failed {
            code: code.to_string(),
            append,
        };
        true
    }

    pub fn accept_new_commits(&mut self, since: &str, count: u32) {
        if self.items.first().is_some_and(|c| c.id == since) {
            self.new_commits = count;
        }
    }

    /// Does nothing on an empty list.
    pub fn select(&mut self, index: usize) {
        if !self.items.is_empty() {
            self.selected = index.min(self.items.len() - 1);
        }
    }

    pub fn toggle_replies(&mut self) {
        if let Some(id) = self.items.get(self.selected).map(|c| c.id.clone())
            && !self.expanded.remove(&id)
        {
            self.expanded.insert(id);
        }
    }
}

/// True when `position` is within the prefetch margin of the last of `len` items.
pub fn near_end(position: usize, len: usize) -> bool {
    position + PREFETCH_MARGIN >= len.saturating_sub(1)
}

/// Scrolls just enough to show `extent` (start, height), without leaving blank rows below the list.
pub fn settle_scroll(extent: (usize, usize), lines: usize, scroll: usize, view: usize) -> usize {
    ensure_visible(extent.0, extent.1, scroll, view).min(lines.saturating_sub(view))
}

/// Returns the scroll offset that shows the card at `top..top + height` in a view of `view` rows.
///
/// A card taller than the view is aligned to its top so that its header stays visible.
pub fn ensure_visible(top: usize, height: usize, scroll: usize, view: usize) -> usize {
    if top < scroll {
        top
    } else if top + height > scroll + view {
        (top + height).saturating_sub(view).min(top)
    } else {
        scroll
    }
}
