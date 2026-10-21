use std::collections::HashMap;

use time::OffsetDateTime;

use super::App;
use crate::feed::FeedPage;
use crate::model::{Commit, ThreadItem, ThreadResponse, Tombstone, UserResponse};
use crate::thread::item_id;

/// What became of a deleted commit, judged once when the server answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Fate {
    /// Nothing leans on it, so it leaves every list.
    Gone,
    /// Replies or loaded replies lean on it, so threads keep a tombstone.
    Tombstone,
}

/// Turns a commit into the tombstone that stands where it was; a tombstone stays as it is.
pub(super) fn bury(item: &mut ThreadItem, now: OffsetDateTime) {
    if let ThreadItem::Commit(c) = item {
        let tombstone = Tombstone {
            id: std::mem::take(&mut c.id),
            parent_id: c.parent_id.take(),
            deleted_at: now,
            replies_count: c.replies_count,
        };
        *item = ThreadItem::Tombstone(tombstone);
    }
}

fn buried(mut item: ThreadItem, now: OffsetDateTime) -> ThreadItem {
    bury(&mut item, now);
    item
}

/// Decides what a loaded item becomes. A load that shows replies beats the record, since the commit is then
/// known to stay.
pub(super) fn settle_item(
    deleted: &mut HashMap<String, Fate>,
    item: ThreadItem,
    now: OffsetDateTime,
) -> Option<ThreadItem> {
    let Some(fate) = deleted.get(item_id(&item)).copied() else {
        return Some(settle_previews(deleted, item, now));
    };
    match &item {
        ThreadItem::Tombstone(_) => Some(item),
        ThreadItem::Commit(c) if c.replies_count.unwrap_or(0) >= 1 => {
            deleted.insert(c.id.clone(), Fate::Tombstone);
            Some(buried(item, now))
        }
        ThreadItem::Commit(_) => match fate {
            Fate::Gone => None,
            Fate::Tombstone => Some(buried(item, now)),
        },
    }
}

pub(super) fn settle_previews(
    deleted: &mut HashMap<String, Fate>,
    mut item: ThreadItem,
    now: OffsetDateTime,
) -> ThreadItem {
    if let ThreadItem::Commit(c) = &mut item {
        settle_commit(deleted, c, now);
    }
    item
}

pub(super) fn settle_commit(
    deleted: &mut HashMap<String, Fate>,
    commit: &mut Commit,
    now: OffsetDateTime,
) {
    if let Some(preview) = &mut commit.reply_preview {
        let data = std::mem::take(&mut preview.data);
        preview.data = data
            .into_iter()
            .filter_map(|i| settle_item(deleted, i, now))
            .collect();
    }
}

/// Loads that were already out when a delete was answered.
impl App {
    fn with_deleted<R>(
        &mut self,
        f: impl FnOnce(&mut HashMap<String, Fate>, OffsetDateTime) -> R,
    ) -> R {
        let mut deleted = std::mem::take(&mut self.deleted);
        let out = f(&mut deleted, self.now);
        self.deleted = deleted;
        out
    }

    pub(super) fn settle_page(&mut self, page: &mut FeedPage) {
        if self.deleted.is_empty() {
            return;
        }
        self.with_deleted(|deleted, now| {
            page.items.retain(|c| !deleted.contains_key(&c.id));
            page.suggestions.retain(|c| !deleted.contains_key(&c.id));
            for c in page.items.iter_mut().chain(page.suggestions.iter_mut()) {
                settle_commit(deleted, c, now);
            }
        });
    }

    pub(super) fn settle_user(&mut self, response: &mut UserResponse) {
        if self.deleted.is_empty() {
            return;
        }
        self.with_deleted(|deleted, now| {
            if response
                .pinned_commit
                .as_ref()
                .is_some_and(|c| deleted.contains_key(&c.id))
            {
                response.pinned_commit = None;
            }
            if let Some(c) = &mut response.pinned_commit {
                settle_commit(deleted, c, now);
            }
        });
    }

    pub(super) fn settle_thread_response(
        &mut self,
        response: ThreadResponse,
    ) -> (ThreadItem, Vec<ThreadItem>) {
        let main = ThreadItem::Commit(Box::new(response.data));
        if self.deleted.is_empty() {
            return (main, response.ancestors);
        }
        self.with_deleted(|deleted, now| {
            let settle_always = |deleted: &mut HashMap<String, Fate>, item: ThreadItem| {
                if deleted.contains_key(item_id(&item)) {
                    buried(item, now)
                } else {
                    settle_previews(deleted, item, now)
                }
            };
            let main = settle_always(deleted, main);
            let ancestors = response
                .ancestors
                .into_iter()
                .map(|a| settle_always(deleted, a))
                .collect();
            (main, ancestors)
        })
    }

    pub(super) fn settle_replies(&mut self, items: Vec<ThreadItem>) -> Vec<ThreadItem> {
        if self.deleted.is_empty() {
            return items;
        }
        self.with_deleted(|deleted, now| {
            items
                .into_iter()
                .filter_map(|i| settle_item(deleted, i, now))
                .collect()
        })
    }
}
