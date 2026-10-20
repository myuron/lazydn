use super::App;
use crate::model::{Commit, ThreadItem, Tombstone};

/// One copy of a commit that the app holds, which a change must reach like all the others.
pub(super) enum CommitCopy<'a> {
    Commit(&'a mut Commit),
    Tombstone(&'a mut Tombstone),
}

impl CommitCopy<'_> {
    pub(super) fn replies_count(&mut self) -> &mut Option<u32> {
        match self {
            Self::Commit(c) => &mut c.replies_count,
            Self::Tombstone(t) => &mut t.replies_count,
        }
    }
}

fn copy_of(item: &mut ThreadItem) -> CommitCopy<'_> {
    match item {
        ThreadItem::Commit(c) => CommitCopy::Commit(c),
        ThreadItem::Tombstone(t) => CommitCopy::Tombstone(t),
    }
}

impl App {
    /// Calls `f` on every copy of the commit `id`: in the log lists, their suggestions and Saved, in the profiles, and in the threads.
    pub(super) fn for_each_copy(&mut self, id: &str, mut f: impl FnMut(CommitCopy<'_>)) {
        for feed in self.feeds.values_mut() {
            for c in feed
                .items
                .iter_mut()
                .chain(feed.suggestions.iter_mut())
                .filter(|c| c.id == id)
            {
                f(CommitCopy::Commit(c));
            }
        }
        for state in self.profiles.values_mut() {
            let pinned = state.user.as_mut().and_then(|u| u.pinned_commit.as_mut());
            for c in state
                .commits
                .items
                .iter_mut()
                .chain(state.replies.items.iter_mut())
                .chain(pinned)
                .filter(|c| c.id == id)
            {
                f(CommitCopy::Commit(c));
            }
        }
        for state in self.threads.values_mut() {
            if let Some(main) = state.main.as_mut().filter(|m| m.id == id) {
                f(CommitCopy::Commit(main));
            }
            let listed = state.children.values_mut().flat_map(|l| l.items.iter_mut());
            for item in state.ancestors.iter_mut().chain(listed) {
                if crate::thread::item_id(item) == id {
                    f(copy_of(item));
                }
            }
        }
    }
}
