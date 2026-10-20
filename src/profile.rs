use crate::app::ProfileTab;
use crate::feed::{Feed, LoadState, Pending, ReqId};
use crate::model::{Commit, User, UserResponse};

/// One profile screen; the two tabs keep separate lists so that switching back restores each.
#[derive(Debug, Clone, Default)]
pub struct ProfileState {
    pub user: Option<UserResponse>,
    pub user_pending: Option<ReqId>,
    /// API error code of the user request.
    pub error: Option<String>,
    /// The `user` field of a `user_blocked` error; it carries no counts.
    pub blocked_user: Option<User>,
    pub tab: ProfileTab,
    pub commits: Feed,
    pub replies: Feed,
}

impl ProfileState {
    pub fn feed(&self, tab: ProfileTab) -> &Feed {
        match tab {
            ProfileTab::Commits => &self.commits,
            ProfileTab::Replies => &self.replies,
        }
    }

    pub fn feed_mut(&mut self, tab: ProfileTab) -> &mut Feed {
        match tab {
            ProfileTab::Commits => &mut self.commits,
            ProfileTab::Replies => &mut self.replies,
        }
    }

    /// Only the signed Commits list leaves the pinned commit out, so only there does it come separately.
    pub fn pinned(&self, signed_in: bool) -> Option<&Commit> {
        if signed_in && self.tab == ProfileTab::Commits {
            self.user.as_ref().and_then(|u| u.pinned_commit.as_ref())
        } else {
            None
        }
    }

    /// The selectable cards: the pinned commit first, then the list without a copy of it.
    pub fn cards(&self, signed_in: bool) -> Vec<&Commit> {
        let pinned = self.pinned(signed_in);
        pinned
            .into_iter()
            .chain(
                self.feed(self.tab)
                    .items
                    .iter()
                    .filter(|c| pinned.is_none_or(|p| p.id != c.id)),
            )
            .collect()
    }

    /// The list is drawn only once the user is known without an error, so nothing else may act on it.
    pub fn shows_list(&self) -> bool {
        self.user.is_some() && self.error.is_none()
    }

    /// Index into the selectable cards, where a pinned commit counts as index 0.
    pub fn selected(&self) -> usize {
        self.feed(self.tab).selected
    }

    /// Rows scrolled off the top of the whole screen, header included.
    pub fn scroll(&self) -> u16 {
        self.feed(self.tab).scroll
    }
}

#[derive(Debug, Clone, Default)]
pub struct ConnectionsState {
    pub users: Vec<User>,
    pub next_cursor: Option<String>,
    pub load: LoadState,
    pub pending: Option<Pending>,
    pub loaded_once: bool,
    pub selected: usize,
    pub scroll: u16,
}
