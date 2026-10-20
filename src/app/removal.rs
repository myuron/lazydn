use std::collections::HashMap;

use super::copies::CommitCopy;
use super::settle::{Fate, bury, settle_commit};
use super::writes::write_failure_text;
use super::{App, Modal, ToastKind, toast};
use crate::action::{Effect, Failure};
use crate::dialogs::DeleteConfirm;
use crate::drafts::DraftKey;
use crate::feed::{Feed, ReqId};
use crate::model::{Commit, ThreadItem};
use crate::profile::ProfileState;
use crate::thread::item_id;

const NOT_FOUND: [&str; 2] = ["commit_unavailable", "http_404"];

/// Drops `id` from `items` and moves `selected` so that it still points at the same card.
fn remove_from(items: &mut Vec<Commit>, selected: &mut usize, id: &str) {
    let before = items.iter().take(*selected).filter(|c| c.id == id).count();
    items.retain(|c| c.id != id);
    *selected = (*selected - before).min(items.len().saturating_sub(1));
}

pub(super) fn remove_from_feed(feed: &mut Feed, id: &str) {
    remove_from(&mut feed.items, &mut feed.selected, id);
    feed.suggestions.retain(|c| c.id != id);
    feed.expanded.remove(id);
}

fn card_ids(state: &ProfileState, tab: crate::app::ProfileTab, signed: bool) -> Vec<String> {
    let cards = state.cards_of(tab, signed);
    cards.into_iter().map(|c| c.id.clone()).collect()
}

fn remove_from_profile(state: &mut ProfileState, id: &str, signed: bool) {
    use crate::app::ProfileTab::{Commits, Replies};
    let tabs = [Commits, Replies];
    let before = tabs.map(|t| card_ids(state, t, signed));
    if let Some(user) = &mut state.user
        && user.pinned_commit.as_ref().is_some_and(|c| c.id == id)
    {
        user.pinned_commit = None;
    }
    for tab in tabs {
        let feed = state.feed_mut(tab);
        feed.items.retain(|c| c.id != id);
        feed.expanded.remove(id);
    }
    for (tab, ids) in tabs.into_iter().zip(before) {
        let left = card_ids(state, tab, signed).len();
        let feed = state.feed_mut(tab);
        let gone = ids.iter().take(feed.selected).filter(|i| *i == id).count();
        feed.selected = (feed.selected - gone).min(left.saturating_sub(1));
    }
}

/// Deleting a commit.
impl App {
    /// The delete that waits for an answer to the confirmation.
    pub fn delete_confirm(&self) -> Option<&DeleteConfirm> {
        self.confirm.as_ref()
    }

    /// Whether a delete of the commit is out, so that its card shows `rm … → /dev/null`.
    pub fn is_deleting(&self, id: &str) -> bool {
        self.deleting.contains_key(id)
    }

    pub(super) fn close_modal(&mut self) {
        match self.modal {
            Some(Modal::ConfirmDelete) => self.confirm = None,
            Some(Modal::Report) => self.report = None,
            Some(Modal::Menu) => self.menu = None,
            Some(Modal::ConfirmBlock) => self.close_block_confirm(),
            _ => {}
        }
        self.modal = None;
    }

    /// Forgets the dialogs and everything that was out; the answers that follow find nothing.
    pub(super) fn forget_removals(&mut self) {
        self.confirm = None;
        self.menu = None;
        self.report = None;
        self.deleting.clear();
        self.deleted.clear();
        self.reports.clear();
        if matches!(
            self.modal,
            Some(Modal::ConfirmDelete | Modal::Report | Modal::Menu)
        ) {
            self.modal = None;
        }
    }

    // --- delete ---------------------------------------------------------------------------

    pub(super) fn open_delete(&mut self) -> Vec<Effect> {
        if let Some(early) = self.session_gate() {
            return early;
        }
        match self.selected_commit() {
            Some(card) => self.open_delete_on(card),
            None => Vec::new(),
        }
    }

    pub(super) fn open_delete_on(&mut self, card: Commit) -> Vec<Effect> {
        if self.not_own(&card) || self.is_deleting(&card.id) {
            return Vec::new();
        }
        self.confirm = Some(DeleteConfirm {
            id: card.id,
            short_hash: card.short_hash,
        });
        self.modal = Some(Modal::ConfirmDelete);
        Vec::new()
    }

    pub(super) fn confirm_delete(&mut self) -> Vec<Effect> {
        let Some(confirm) = self.confirm.take() else {
            return Vec::new();
        };
        if self.modal == Some(Modal::ConfirmDelete) {
            self.modal = None;
        }
        let req = self.new_req();
        self.deleting
            .insert(confirm.id.clone(), (req, confirm.short_hash));
        vec![Effect::DeleteCommit {
            id: confirm.id,
            req,
        }]
    }

    pub(super) fn delete_done(&mut self, id: &str, req: ReqId) -> Vec<Effect> {
        if self.deleting.get(id).map(|(r, _)| *r) != Some(req) {
            return Vec::new();
        }
        let Some((_, short_hash)) = self.deleting.remove(id) else {
            return Vec::new();
        };
        let effects = self.apply_deletion(id);
        let text = format!("rm {} → /dev/null", toast::clean(&short_hash));
        self.show_toast(ToastKind::Success, text);
        effects
    }

    pub(super) fn delete_failed(&mut self, id: &str, req: ReqId, failure: &Failure) -> Vec<Effect> {
        if self.deleting.get(id).map(|(r, _)| *r) != Some(req) {
            return Vec::new();
        }
        // The commit was gone already, which is what the user asked for.
        if NOT_FOUND.contains(&failure.code.as_str()) {
            return self.delete_done(id, req);
        }
        self.deleting.remove(id);
        self.show_toast(ToastKind::Error, write_failure_text(failure));
        Vec::new()
    }

    /// Whether anything keeps the commit from leaving: a reply counted in any copy, a place as an ancestor, or
    /// replies loaded under it.
    fn survives_deletion(&mut self, id: &str) -> bool {
        let mut counted = false;
        self.for_each_copy_deep(id, |mut copy| {
            counted |= copy.replies_count().unwrap_or(0) > 0;
        });
        counted
            || self.threads.values().any(|t| {
                t.ancestors.iter().any(|a| item_id(a) == id)
                    || t.children.get(id).is_some_and(|l| !l.items.is_empty())
            })
    }

    fn apply_deletion(&mut self, id: &str) -> Vec<Effect> {
        let (mut parent, mut quoted) = (None, None);
        self.for_each_copy_deep(id, |copy| match copy {
            CommitCopy::Commit(c) => {
                parent = parent.take().or_else(|| c.parent_id.clone());
                quoted = quoted.take().or_else(|| c.quoted_commit_id.clone());
            }
            CommitCopy::Tombstone(t) => {
                parent = parent.take().or_else(|| t.parent_id.clone());
            }
        });
        let fate = if self.survives_deletion(id) {
            Fate::Tombstone
        } else {
            Fate::Gone
        };
        self.deleted.insert(id.to_string(), fate);

        for feed in self.feeds.values_mut() {
            remove_from_feed(feed, id);
        }
        let signed = self.signed_in();
        for state in self.profiles.values_mut() {
            remove_from_profile(state, id, signed);
        }
        self.settle_threads(id, fate);
        let now = self.now;
        let mut previews = HashMap::from([(id.to_string(), fate)]);
        self.for_each_commit(|c| settle_commit(&mut previews, c, now));

        if fate == Fate::Gone {
            for target in [parent, quoted].into_iter().flatten() {
                self.for_each_copy_deep(&target, |mut copy| {
                    if let Some(n) = copy.replies_count() {
                        *n = n.saturating_sub(1);
                    }
                });
            }
        }
        self.count_one_less();
        self.forget_drafts_of(id);
        self.save_effect()
    }

    fn settle_threads(&mut self, id: &str, fate: Fate) {
        let now = self.now;
        let ids: Vec<String> = self.threads.keys().cloned().collect();
        for thread in ids {
            let kept = self.selected_entry(&thread);
            if let Some(state) = self.threads.get_mut(&thread) {
                let swap = |item: &mut ThreadItem| bury(item, now);
                if state.main.as_ref().is_some_and(|m| item_id(m) == id) {
                    state.main.iter_mut().for_each(swap);
                }
                state
                    .ancestors
                    .iter_mut()
                    .filter(|a| item_id(a) == id)
                    .for_each(swap);
                for list in state.children.values_mut() {
                    match fate {
                        Fate::Gone => list.items.retain(|i| item_id(i) != id),
                        Fate::Tombstone => list
                            .items
                            .iter_mut()
                            .filter(|i| item_id(i) == id)
                            .for_each(swap),
                    }
                }
                if fate == Fate::Gone {
                    state.expanded.remove(id);
                    state.children.remove(id);
                }
            }
            self.restore_selection(&thread, kept);
        }
    }

    /// Your commit count is one lower, here and on your own profile.
    fn count_one_less(&mut self) {
        let Some(handle) = (match &mut self.session {
            super::Session::SignedIn(me) => {
                me.commits = me.commits.saturating_sub(1);
                Some(me.handle.to_ascii_lowercase())
            }
            _ => None,
        }) else {
            return;
        };
        if let Some(user) = self.profiles.get_mut(&handle).and_then(|s| s.user.as_mut())
            && let Some(n) = &mut user.data.commits_count
        {
            *n = n.saturating_sub(1);
        }
    }

    fn forget_drafts_of(&mut self, id: &str) {
        let keys = [DraftKey::Reply(id.into()), DraftKey::Quote(id.into())];
        for key in keys {
            if self.drafts.get(&key).is_some() {
                self.drafts.remove(&key);
                self.drafts_dirty_at = Some(self.now);
            }
        }
        // Keys never reach a delete while a composer has them, so this only guards a direct action.
        if self.compose.as_ref().is_some_and(|c| {
            c.key == DraftKey::Reply(id.into()) || c.key == DraftKey::Quote(id.into())
        }) {
            self.compose = None;
            if self.modal == Some(Modal::Composer) {
                self.modal = None;
            }
        }
    }
}
