use time::Duration;

use super::{App, Modal, Pane, Screen, ScreenKind, Session, ToastKind, toast};
use crate::action::{Effect, Failure};
use crate::body::{MAX_BODY_LEN, body_len};
use crate::compose::{Compose, InFlight, Surface};
use crate::drafts::{DraftKey, Drafts, is_valid_account_id};
use crate::editor::Editor;
use crate::feed::{FeedKey, ReqId};
use crate::keymap::ComposerKey;
use crate::model::{Commit, Status, ThreadItem};
use crate::thread::item_id;
use crate::ui::compose::{compose_text_width, view_top_for};

/// Seconds after the last edit at which the drafts are written.
const SAVE_DELAY_SECS: i64 = 2;
const STILL_SENDING: &str = "a commit is still being sent";
const PASTE_CUT_TEXT: &str = "pasted text was cut at 8000 characters";

/// The composer and the drafts behind it.
impl App {
    /// The open composer, if any.
    pub fn compose(&self) -> Option<&Compose> {
        self.compose.as_ref()
    }

    /// The commit that is out, whether or not its composer is still open.
    pub fn in_flight(&self) -> Option<&InFlight> {
        self.in_flight.as_ref()
    }

    /// Every draft of the signed-in account, including the one being edited.
    pub fn drafts(&self) -> &Drafts {
        &self.drafts
    }

    /// The id of the signed-in account; `None` for a guest or while the session is being restored.
    pub fn account_id(&self) -> Option<&str> {
        match &self.session {
            Session::SignedIn(me) => Some(me.id.as_str()),
            Session::Guest | Session::Restoring => None,
        }
    }

    /// The account whose drafts may be written: an id that cannot name a file has none.
    fn draft_account(&self) -> Option<String> {
        self.account_id()
            .filter(|id| is_valid_account_id(id))
            .map(String::from)
    }

    /// Quits, writing first whatever is unsaved.
    pub(super) fn quit_effects(&mut self) -> Vec<Effect> {
        let mut effects = self.save_effect();
        effects.push(Effect::Quit);
        effects
    }

    /// Writes the drafts on the spot if they changed since the last write.
    pub(super) fn save_effect(&mut self) -> Vec<Effect> {
        let edited = self.drafts_dirty_at.take().is_some();
        if !(std::mem::take(&mut self.drafts_unsaved) || edited) {
            return Vec::new();
        }
        match self.draft_account() {
            Some(account) => vec![Effect::SaveDrafts {
                account,
                drafts: self.drafts.clone(),
            }],
            None => Vec::new(),
        }
    }

    /// The write that is due once the user has stopped editing for a while.
    pub(super) fn due_save(&mut self) -> Vec<Effect> {
        let due = self
            .drafts_dirty_at
            .is_some_and(|t| self.now - t >= Duration::seconds(SAVE_DELAY_SECS));
        if due { self.save_effect() } else { Vec::new() }
    }

    pub(super) fn load_drafts_effect(&self) -> Vec<Effect> {
        self.draft_account()
            .map(|account| Effect::LoadDrafts { account })
            .into_iter()
            .collect()
    }

    /// Drops the composer and everything that belongs to the account.
    pub(super) fn forget_composer_state(&mut self) {
        self.compose = None;
        self.drafts = Drafts::default();
        self.in_flight = None;
        self.drafts_dirty_at = None;
        self.drafts_unsaved = false;
        if self.modal == Some(Modal::Composer) {
            self.modal = None;
        }
    }

    /// `c`: opens the composer inline on Log and as a dialog elsewhere; a guest is asked to sign in.
    pub(super) fn open_composer(&mut self) -> Vec<Effect> {
        if let Some(early) = self.session_gate() {
            return early;
        }
        let surface = if matches!(self.screen, Screen::Log(_)) {
            Surface::Inline
        } else {
            Surface::Modal
        };
        self.open_compose(DraftKey::New, surface, None)
    }

    /// `r`: opens the reply box under the selected card.
    pub(super) fn open_reply(&mut self) -> Vec<Effect> {
        self.open_on_selected(false)
    }

    /// `q`: opens the quote dialog for the selected card.
    pub(super) fn open_quote(&mut self) -> Vec<Effect> {
        self.open_on_selected(true)
    }

    /// `Some` when the session decides alone: a guest is asked to sign in, a restoring session waits.
    pub(super) fn session_gate(&mut self) -> Option<Vec<Effect>> {
        match self.session {
            Session::Guest => Some(self.open_sign_in()),
            Session::Restoring => Some(Vec::new()),
            Session::SignedIn(_) => None,
        }
    }

    fn open_on_selected(&mut self, quote: bool) -> Vec<Effect> {
        if let Some(early) = self.session_gate() {
            return early;
        }
        let Some(commit) = self.selected_commit().filter(|c| !self.is_deleting(&c.id)) else {
            return Vec::new();
        };
        let (key, surface) = if quote {
            (DraftKey::Quote(commit.id.clone()), Surface::Modal)
        } else {
            (DraftKey::Reply(commit.id.clone()), Surface::ReplyBox)
        };
        self.open_compose(key, surface, Some(Box::new(commit)))
    }

    /// Replaces the open composer, whose draft stays; help and the sign-in dialog are not interrupted.
    fn open_compose(
        &mut self,
        key: DraftKey,
        surface: Surface,
        target: Option<Box<Commit>>,
    ) -> Vec<Effect> {
        if matches!(self.modal, Some(Modal::Help | Modal::SignIn)) {
            return Vec::new();
        }
        if self.compose.as_ref().is_some_and(|c| c.key == key) {
            return Vec::new();
        }
        self.compose = None;
        self.modal = if surface == Surface::Modal {
            Some(Modal::Composer)
        } else {
            self.focus = Pane::Main;
            None
        };
        let sending = self.in_flight.as_ref().is_some_and(|f| f.key == key);
        self.compose = Some(Compose {
            key,
            surface,
            submitting: sending,
            error: None,
            view_top: 0,
            target,
        });
        Vec::new()
    }

    pub(super) fn selected_commit(&self) -> Option<Commit> {
        match self.kind() {
            ScreenKind::Feed(key) => {
                let feed = self.feeds.get(&key)?;
                feed.items.get(feed.selected).cloned()
            }
            ScreenKind::Profile(_) => self.profile_selected_commit(),
            ScreenKind::Thread(id) => self.thread_selected_commit(&id),
            ScreenKind::Connections(..) | ScreenKind::Notifications | ScreenKind::Settings => None,
        }
    }

    /// Whether the list on screen still holds the commit.
    fn screen_holds(&self, id: &str) -> bool {
        match self.kind() {
            ScreenKind::Feed(key) => self
                .feeds
                .get(&key)
                .is_some_and(|f| f.items.iter().any(|c| c.id == id)),
            ScreenKind::Profile(name) => self
                .profile(&name)
                .filter(|s| s.shows_list())
                .is_some_and(|s| s.cards(self.signed_in()).iter().any(|c| c.id == id)),
            ScreenKind::Thread(thread) => self.threads.get(&thread).is_some_and(|s| {
                s.main.as_ref().is_some_and(|m| item_id(m) == id)
                    || s.ancestors.iter().any(|i| item_id(i) == id)
                    || s.children
                        .values()
                        .any(|l| l.items.iter().any(|i| item_id(i) == id))
            }),
            ScreenKind::Connections(..) | ScreenKind::Notifications | ScreenKind::Settings => false,
        }
    }

    pub(super) fn on_compose_key(&mut self, key: ComposerKey) -> Vec<Effect> {
        let Some(compose) = &self.compose else {
            return Vec::new();
        };
        let (surface, submitting) = (compose.surface, compose.submitting);
        let width = compose_text_width(self, surface);
        match key {
            ComposerKey::Quit => return self.quit_effects(),
            ComposerKey::Close => return self.close_composer(),
            ComposerKey::Submit => return self.submit(),
            _ if submitting => {}
            ComposerKey::Insert(c) => {
                self.edit_draft(|e| e.insert_char(c));
            }
            ComposerKey::Newline => {
                self.edit_draft(Editor::newline);
            }
            ComposerKey::Tab => {
                self.edit_draft(Editor::insert_tab);
            }
            ComposerKey::Backspace => {
                self.edit_draft(Editor::backspace);
            }
            ComposerKey::Delete => {
                self.edit_draft(Editor::delete);
            }
            ComposerKey::Left => {
                self.edit_draft(Editor::left);
            }
            ComposerKey::Right => {
                self.edit_draft(Editor::right);
            }
            ComposerKey::Up => {
                self.edit_draft(|e| e.up(width));
            }
            ComposerKey::Down => {
                self.edit_draft(|e| e.down(width));
            }
            ComposerKey::Home => {
                self.edit_draft(Editor::home);
            }
            ComposerKey::End => {
                self.edit_draft(Editor::end);
            }
            ComposerKey::KillToLineStart => {
                self.edit_draft(Editor::kill_to_line_start);
            }
            ComposerKey::DeleteWordBack => {
                self.edit_draft(Editor::delete_word_back);
            }
        }
        Vec::new()
    }

    /// Applies `edit` to the draft being written; the drafts count as changed only when the text did.
    fn edit_draft<R>(&mut self, edit: impl FnOnce(&mut Editor) -> R) -> Option<R> {
        let key = self.compose.as_ref()?.key.clone();
        let mut editor = self.drafts.get(&key).cloned().unwrap_or_default();
        let before = editor.text();
        let result = edit(&mut editor);
        if editor.text() != before {
            self.drafts_dirty_at = Some(self.now);
        }
        self.drafts.set(key, editor);
        Some(result)
    }

    pub(super) fn paste_into_composer(&mut self, text: &str) {
        if self.compose.as_ref().is_none_or(|c| c.submitting) {
            return;
        }
        if self.edit_draft(|e| e.paste(text)) == Some(true) {
            self.show_toast(ToastKind::Info, PASTE_CUT_TEXT.into());
        }
    }

    fn close_composer(&mut self) -> Vec<Effect> {
        self.compose = None;
        if self.modal == Some(Modal::Composer) {
            self.modal = None;
        }
        self.save_effect()
    }

    fn submit(&mut self) -> Vec<Effect> {
        let Some(compose) = &self.compose else {
            return Vec::new();
        };
        if compose.submitting {
            return Vec::new();
        }
        let key = compose.key.clone();
        let Some(body) = self.drafts.text(&key) else {
            return Vec::new();
        };
        let len = body_len(&body);
        if len == 0 || len > MAX_BODY_LEN {
            return Vec::new();
        }
        // The answer of the commit that is out decides what the next one may do, so it cannot overtake it.
        if let Some(out) = &self.in_flight {
            if out.key != key {
                self.show_toast(ToastKind::Info, STILL_SENDING.into());
            }
            return Vec::new();
        }
        self.next_req += 1;
        let req = ReqId(self.next_req);
        self.in_flight = Some(InFlight {
            req,
            key: key.clone(),
            sent_text: body.clone(),
        });
        if let Some(compose) = self.compose.as_mut() {
            compose.submitting = true;
            compose.error = None;
        }
        let (parent_id, quoted_commit_id) = match key {
            DraftKey::New => (None, None),
            DraftKey::Reply(id) => (Some(id), None),
            DraftKey::Quote(id) => (None, Some(id)),
        };
        vec![Effect::CreateCommit {
            body,
            parent_id,
            quoted_commit_id,
            req,
        }]
    }

    /// The server made the commit, so it is shown whatever became of the composer that sent it.
    pub(super) fn commit_created(&mut self, req: ReqId, commit: Commit) -> Vec<Effect> {
        let hash = toast::clean(&commit.short_hash);
        let review = if commit.status == Some(Status::Pending) {
            " (in review)"
        } else {
            ""
        };
        let word = if commit.parent_id.is_some() {
            "Replied"
        } else if commit.quoted_commit_id.is_some() {
            "Quoted"
        } else {
            "Committed"
        };
        self.show_toast(ToastKind::Success, format!("{word} {hash}{review}"));
        let signed_in = matches!(self.session, Session::SignedIn(_));
        let before_sign_in = self.sign_in_boundary.is_some_and(|b| req.0 <= b.0);
        if !signed_in || before_sign_in {
            return Vec::new();
        }
        match commit.parent_id.clone() {
            // The log lists hold top-level commits only, so a reply is counted where its parent is shown.
            Some(parent) => self.apply_reply(&parent, &commit),
            None => self.insert_at_heads(commit),
        }
        // An answer for another request leaves the composer to the one that is out.
        if let Some(sent) = self.in_flight.clone().filter(|f| f.req == req) {
            self.in_flight = None;
            self.finish_send(&sent.key, &sent.sent_text);
        }
        self.save_effect()
    }

    /// Clears the draft if it is still what was sent, and closes its composer.
    fn finish_send(&mut self, key: &DraftKey, sent_text: &str) {
        let unchanged = self.drafts.text(key).as_deref() == Some(sent_text);
        if unchanged {
            self.drafts.remove(key);
            self.drafts_dirty_at = Some(self.now);
        }
        if self.compose.as_ref().is_some_and(|c| &c.key == key) {
            self.compose = None;
            if self.modal == Some(Modal::Composer) {
                self.modal = None;
            }
        }
    }

    /// A list that was never loaded, or is being replaced, will get the commit from the server.
    fn insert_at_heads(&mut self, commit: Commit) {
        let open_parent = self
            .compose
            .as_ref()
            .filter(|c| c.surface == Surface::ReplyBox)
            .and_then(|c| c.key.reply_to().map(String::from));
        for tab in [super::LogTab::Following, super::LogTab::Latest] {
            let Some(feed) = self.feeds.get_mut(&FeedKey::Log(tab)) else {
                continue;
            };
            let box_parent_selected = open_parent
                .as_deref()
                .is_some_and(|id| feed.items.get(feed.selected).is_some_and(|c| c.id == id));
            let replacing = feed.pending.as_ref().is_some_and(|p| !p.append);
            if !feed.loaded_once || replacing || feed.items.iter().any(|c| c.id == commit.id) {
                continue;
            }
            feed.items.insert(0, commit.clone());
            // The card under an open reply box stays selected, whatever its place.
            if feed.selected > 0 || box_parent_selected {
                feed.selected += 1;
            }
        }
    }

    /// Raises the reply count of `parent` in every copy, and adds the reply to a thread that holds all replies.
    fn apply_reply(&mut self, parent: &str, reply: &Commit) {
        self.for_each_copy(parent, |mut copy| {
            if let Some(n) = copy.replies_count() {
                *n += 1;
            }
        });
        let threads: Vec<String> = self.threads.keys().cloned().collect();
        for thread in threads {
            let kept = self.selected_entry(&thread);
            if let Some(list) = self
                .threads
                .get_mut(&thread)
                .and_then(|state| state.children.get_mut(parent))
                // A list that failed to load would look loaded with the reply as its only item.
                .filter(|l| l.next_cursor.is_none() && l.pending.is_none() && l.failed().is_none())
                && !list.items.iter().any(|i| item_id(i) == reply.id)
            {
                list.items.push(ThreadItem::Commit(Box::new(reply.clone())));
            }
            self.restore_selection(&thread, kept);
        }
    }

    pub(super) fn commit_failed(&mut self, req: ReqId, failure: &Failure) {
        let Some(sent) = self.in_flight.take_if(|f| f.req == req) else {
            return;
        };
        let text = toast::commit_failure_text(failure);
        if let Some(compose) = self.compose.as_mut().filter(|c| c.key == sent.key) {
            compose.submitting = false;
            compose.error = Some(text.clone());
        }
        self.show_toast(ToastKind::Error, text);
    }

    pub(super) fn drafts_loaded(&mut self, account: &str, drafts: Drafts) {
        if self.draft_account().as_deref() == Some(account) {
            self.drafts.fill_missing(drafts);
        }
    }

    /// Keeps the cursor row of the text in view.
    pub(super) fn reconcile_compose(&mut self) {
        // A thread reloaded with `R` loses its replies at once, which closes a box under one of them. Keys cannot
        // reach that, since the box takes them all; only an action can.
        let orphaned = self.compose.as_ref().is_some_and(|c| {
            c.surface == Surface::ReplyBox
                && c.key.reply_to().is_some_and(|id| !self.screen_holds(id))
        });
        if orphaned {
            self.compose = None;
        }
        let Some(compose) = &self.compose else {
            return;
        };
        let top = view_top_for(self, compose);
        if let Some(compose) = self.compose.as_mut() {
            compose.view_top = top;
        }
    }
}
