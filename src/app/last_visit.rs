use std::collections::HashMap;

use super::{App, LogTab};
use crate::feed::FeedKey;

/// The head of each chronological tab as the last session left it, kept apart per tab.
impl App {
    /// The id the last session saw at the top of `tab`, if it was recorded.
    pub fn last_visit_of(&self, tab: LogTab) -> Option<&str> {
        self.last_visits.get(&tab).map(String::as_str)
    }

    /// The first commit of every chronological tab that loaded in this session.
    /// A tab that never loaded is absent, so that its stored value survives.
    pub fn heads_to_save(&self) -> HashMap<LogTab, String> {
        [LogTab::Following, LogTab::Latest]
            .into_iter()
            .filter_map(|tab| {
                let head = self.feeds[&FeedKey::Log(tab)].items.first()?;
                Some((tab, head.id.clone()))
            })
            .collect()
    }

    pub(super) fn set_last_visit(&mut self, tab: LogTab, id: String) {
        self.last_visits.insert(tab, id);
        self.reconcile_scroll();
    }
}
