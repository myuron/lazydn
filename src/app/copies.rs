use super::App;
use crate::model::{Commit, ThreadItem, Tombstone};

/// One copy of a commit that the app holds, which a change must reach like all the others.
pub(super) enum CommitCopy<'a> {
    Commit(&'a mut Commit),
    Tombstone(&'a mut Tombstone),
}

impl CommitCopy<'_> {
    pub(super) fn id(&self) -> &str {
        match self {
            Self::Commit(c) => &c.id,
            Self::Tombstone(t) => &t.id,
        }
    }

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

/// Hands over a commit, and then the commits of its reply preview when `previews` is set.
fn visit(commit: &mut Commit, previews: bool, f: &mut impl FnMut(CommitCopy<'_>)) {
    f(CommitCopy::Commit(commit));
    if previews && let Some(preview) = &mut commit.reply_preview {
        for item in &mut preview.data {
            f(copy_of(item));
        }
    }
}

impl App {
    /// Calls `f` on every copy the app holds: in the log lists, their suggestions and Saved, in the profiles, and
    /// in the threads; with `previews` set, also on those inside the reply previews of the commits.
    pub(super) fn walk_copies(&mut self, previews: bool, mut f: impl FnMut(CommitCopy<'_>)) {
        let f = &mut f;
        for feed in self.feeds.values_mut() {
            for c in feed.items.iter_mut().chain(feed.suggestions.iter_mut()) {
                visit(c, previews, f);
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
            {
                visit(c, previews, f);
            }
        }
        for state in self.threads.values_mut() {
            let listed = state.children.values_mut().flat_map(|l| l.items.iter_mut());
            for item in state
                .main
                .iter_mut()
                .chain(state.ancestors.iter_mut())
                .chain(listed)
            {
                match item {
                    ThreadItem::Commit(c) => visit(c, previews, f),
                    ThreadItem::Tombstone(t) => f(CommitCopy::Tombstone(t)),
                }
            }
        }
    }

    /// Calls `f` on every copy of the commit `id` that is not inside a preview.
    pub(super) fn for_each_copy(&mut self, id: &str, mut f: impl FnMut(CommitCopy<'_>)) {
        self.walk_copies(false, |copy| {
            if copy.id() == id {
                f(copy);
            }
        });
    }

    /// Like `for_each_copy`, and also on the copies inside the reply previews.
    pub(super) fn for_each_copy_deep(&mut self, id: &str, mut f: impl FnMut(CommitCopy<'_>)) {
        self.walk_copies(true, |copy| {
            if copy.id() == id {
                f(copy);
            }
        });
    }

    /// Calls `f` on every commit the app holds, whatever its id.
    pub(super) fn for_each_commit(&mut self, mut f: impl FnMut(&mut Commit)) {
        self.walk_copies(false, |copy| {
            if let CommitCopy::Commit(c) = copy {
                f(c);
            }
        });
    }
}
