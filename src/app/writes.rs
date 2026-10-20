use super::copies::CommitCopy;
use super::{App, Screen, Session, ToastKind, toast};
use crate::action::{Effect, Failure};
use crate::feed::ReqId;
use crate::model::{Commit, PastelResponse};

/// What a request in flight does; at most one of each kind is out per commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum WriteKind {
    Star,
    Bookmark,
    Pastel,
}

impl WriteKind {
    /// The flag a press flips at once; a pastel has none, because it waits for the server.
    fn flag(self, c: &Commit) -> Option<bool> {
        match self {
            WriteKind::Star => c.starred,
            WriteKind::Bookmark => c.is_saved,
            WriteKind::Pastel => None,
        }
    }

    /// Sets the flag, and the star count that goes with it.
    fn set(self, c: &mut Commit, on: bool) {
        match self {
            WriteKind::Star => {
                c.starred = Some(on);
                c.stars_count = c.stars_count.map(|n| {
                    if on {
                        n.saturating_add(1)
                    } else {
                        n.saturating_sub(1)
                    }
                });
            }
            WriteKind::Bookmark => c.is_saved = Some(on),
            WriteKind::Pastel => {}
        }
    }

    fn effect(self, id: String, on: bool, req: ReqId) -> Effect {
        match self {
            WriteKind::Star => Effect::StarCommit { id, on, req },
            WriteKind::Bookmark => Effect::BookmarkCommit { id, on, req },
            WriteKind::Pastel => Effect::GivePastel { id, req },
        }
    }
}

/// Replaces a copy by the commit the server answered with. The answer has no `reply_preview` and no `parent`,
/// so the copy keeps its own.
pub(super) fn apply_server_commit(copy: &mut Commit, server: &Commit) {
    let reply_preview = copy.reply_preview.take();
    let parent = copy.parent.take();
    *copy = server.clone();
    copy.reply_preview = reply_preview;
    copy.parent = parent;
}

/// Words for the time left, cut to minutes under an hour and to whole hours above.
fn resets_in(secs: i64) -> Option<String> {
    if secs <= 0 {
        return None;
    }
    let minutes = secs / 60;
    Some(if minutes < 60 {
        format!("{}m", minutes.max(1))
    } else {
        format!("{}h", minutes / 60)
    })
}

fn write_failure_text(failure: &Failure) -> String {
    match (failure.code.as_str(), failure.retry_after, &failure.message) {
        ("rate_limited", Some(_), _) => toast::failure_text(failure),
        ("commit_unavailable", _, _) => "This commit isn't available.".into(),
        (_, _, Some(message)) => toast::clean(message),
        _ => toast::failure_text(failure),
    }
}

/// Starring, saving and giving pastéis, and the links of a card.
impl App {
    /// The card to write to, or the early answer when the session or the selection allows no write.
    fn write_target(&mut self) -> Result<Commit, Vec<Effect>> {
        if let Some(early) = self.session_gate() {
            return Err(early);
        }
        self.selected_commit().ok_or_else(Vec::new)
    }

    fn begin_write(&mut self, kind: WriteKind, id: &str) -> Option<ReqId> {
        let slot = (kind, id.to_string());
        if self.writes.contains_key(&slot) {
            return None;
        }
        let req = self.new_req();
        self.writes.insert(slot, req);
        Some(req)
    }

    /// Takes the request out of the table when `req` is the one that is out; a stale answer finds nothing.
    fn end_write(&mut self, kind: WriteKind, id: &str, req: ReqId) -> bool {
        let slot = (kind, id.to_string());
        if self.writes.get(&slot) != Some(&req) {
            return false;
        }
        self.writes.remove(&slot);
        true
    }

    fn not_own(&self, card: &Commit) -> bool {
        self.account_id() != Some(card.author.id.as_str())
    }

    /// Flips the flag of the selected card in every copy, and asks the server to do the same.
    fn toggle(&mut self, kind: WriteKind) -> Vec<Effect> {
        let card = match self.write_target() {
            Ok(card) => card,
            Err(early) => return early,
        };
        if kind == WriteKind::Star && !self.not_own(&card) {
            return Vec::new();
        }
        let on = !kind.flag(&card).unwrap_or(false);
        let Some(req) = self.begin_write(kind, &card.id) else {
            return Vec::new();
        };
        self.for_each_copy(&card.id, |copy| {
            if let CommitCopy::Commit(c) = copy {
                kind.set(c, on);
            }
        });
        vec![kind.effect(card.id, on, req)]
    }

    pub(super) fn toggle_star(&mut self) -> Vec<Effect> {
        self.toggle(WriteKind::Star)
    }

    pub(super) fn toggle_save(&mut self) -> Vec<Effect> {
        self.toggle(WriteKind::Bookmark)
    }

    pub(super) fn give_pastel(&mut self) -> Vec<Effect> {
        let card = match self.write_target() {
            Ok(card) => card,
            Err(early) => return early,
        };
        let mode = matches!(&self.session, Session::SignedIn(me) if me.pasteis_mode);
        if !mode || !self.not_own(&card) {
            return Vec::new();
        }
        match self.begin_write(WriteKind::Pastel, &card.id) {
            Some(req) => vec![WriteKind::Pastel.effect(card.id, true, req)],
            None => Vec::new(),
        }
    }

    pub(super) fn copy_link(&mut self) -> Vec<Effect> {
        match self.selected_link() {
            Some(url) => vec![Effect::CopyLink(url)],
            None => Vec::new(),
        }
    }

    pub(super) fn open_in_browser(&mut self) -> Vec<Effect> {
        if let Screen::Profile(name) = &self.screen
            && self.selected_commit().is_none()
        {
            let url = self.profile(name).and_then(|state| {
                let user = state.user.as_ref().and_then(|u| u.data.html_url.clone());
                user.or_else(|| state.blocked_user.as_ref().and_then(|u| u.html_url.clone()))
            });
            return url.map(Effect::OpenUrl).into_iter().collect();
        }
        self.selected_link()
            .map(Effect::OpenUrl)
            .into_iter()
            .collect()
    }

    fn selected_link(&self) -> Option<String> {
        self.selected_commit().and_then(|c| c.html_url)
    }

    /// Puts the answer of a star or a save in every copy.
    pub(super) fn flag_done(&mut self, kind: WriteKind, id: &str, req: ReqId, commit: &Commit) {
        if self.end_write(kind, id, req) {
            self.apply_answer(id, commit);
        }
    }

    /// Replaces every copy by the answer, except for what another request of the same commit has set and not
    /// yet had answered: that value is the newer one, and its own answer or refusal settles it.
    fn apply_answer(&mut self, id: &str, commit: &Commit) {
        let star_out = self.writes.contains_key(&(WriteKind::Star, id.to_string()));
        let save_out = self
            .writes
            .contains_key(&(WriteKind::Bookmark, id.to_string()));
        self.for_each_copy(id, |copy| {
            let CommitCopy::Commit(c) = copy else { return };
            let (starred, count, saved) = (c.starred, c.stars_count, c.is_saved);
            apply_server_commit(c, commit);
            if star_out {
                c.starred = starred;
                c.stars_count = count;
            }
            if save_out {
                c.is_saved = saved;
            }
        });
    }

    /// Puts back only the copies that still hold what the press set. A copy is told from a reloaded one by that
    /// value alone, so a reload that brought the same value is put back as well.
    pub(super) fn flag_failed(
        &mut self,
        kind: WriteKind,
        id: &str,
        on: bool,
        req: ReqId,
        failure: &Failure,
    ) {
        if !self.end_write(kind, id, req) {
            return;
        }
        self.for_each_copy(id, |copy| {
            if let CommitCopy::Commit(c) = copy
                && kind.flag(c) == Some(on)
            {
                kind.set(c, !on);
            }
        });
        self.show_toast(ToastKind::Error, write_failure_text(failure));
    }

    pub(super) fn pastel_done(&mut self, id: &str, req: ReqId, response: &PastelResponse) {
        if !self.end_write(WriteKind::Pastel, id, req) {
            return;
        }
        self.apply_answer(id, &response.data);
        if let Session::SignedIn(me) = &mut self.session {
            me.pasteis_left = response.pasteis_remaining;
            me.pasteis_resets_at = response.pasteis_resets_at;
        }
        let author = response.data.author.username.to_ascii_lowercase();
        if let Some(total) = response.author_pasteis_received
            && let Some(user) = self
                .profiles
                .get_mut(&author)
                .and_then(|state| state.user.as_mut())
        {
            user.pasteis_received = Some(total);
        }
        let left = response.pasteis_remaining;
        self.show_toast(
            ToastKind::Success,
            format!("Gave a pastel ({left} left today)"),
        );
    }

    pub(super) fn pastel_failed(&mut self, id: &str, req: ReqId, failure: &Failure) {
        if !self.end_write(WriteKind::Pastel, id, req) {
            return;
        }
        if failure.code != "pasteis_exhausted" {
            self.show_toast(ToastKind::Error, write_failure_text(failure));
            return;
        }
        let mut text = "No pastéis left today".to_string();
        if let Session::SignedIn(me) = &mut self.session {
            me.pasteis_left = 0;
            let left = (me.pasteis_resets_at - self.now).whole_seconds();
            if let Some(left) = resets_in(left) {
                text.push_str(&format!(" (resets in {left})"));
            }
        }
        self.show_toast(ToastKind::Error, text);
    }
}
