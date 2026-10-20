use time::Duration;

use super::{App, Modal, Pane, Screen, Session, ToastKind, toast};
use crate::action::{Effect, Failure};
use crate::body::{MAX_BODY_LEN, body_len};
use crate::compose::{Compose, InFlight, Surface};
use crate::drafts::{DraftKey, Drafts, is_valid_account_id};
use crate::editor::Editor;
use crate::feed::{FeedKey, ReqId};
use crate::keymap::ComposerKey;
use crate::model::{Commit, Status};
use crate::ui::compose::{compose_text_width, view_top_for};

/// Seconds after the last edit at which the drafts are written.
const SAVE_DELAY_SECS: i64 = 2;
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
        match self.session {
            Session::Guest => return self.open_sign_in(),
            Session::Restoring => return Vec::new(),
            Session::SignedIn(_) => {}
        }
        if self.compose.is_some() || self.modal.is_some() {
            return Vec::new();
        }
        let surface = if matches!(self.screen, Screen::Log(_)) {
            self.focus = Pane::Main;
            Surface::Inline
        } else {
            self.modal = Some(Modal::Composer);
            Surface::Modal
        };
        let sending = self.in_flight.as_ref().filter(|f| f.key == DraftKey::New);
        self.compose = Some(Compose {
            key: DraftKey::New,
            surface,
            submitting: sending.is_some(),
            error: None,
            view_top: 0,
        });
        Vec::new()
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
        if compose.submitting || self.in_flight.is_some() {
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
        self.show_toast(ToastKind::Success, format!("Committed {hash}{review}"));
        let signed_in = matches!(self.session, Session::SignedIn(_));
        let before_sign_in = self.sign_in_boundary.is_some_and(|b| req.0 <= b.0);
        if !signed_in || before_sign_in {
            return Vec::new();
        }
        self.insert_at_heads(commit);
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
        for tab in [super::LogTab::Following, super::LogTab::Latest] {
            let Some(feed) = self.feeds.get_mut(&FeedKey::Log(tab)) else {
                continue;
            };
            let replacing = feed.pending.as_ref().is_some_and(|p| !p.append);
            if !feed.loaded_once || replacing || feed.items.iter().any(|c| c.id == commit.id) {
                continue;
            }
            feed.items.insert(0, commit.clone());
            if feed.selected > 0 {
                feed.selected += 1;
            }
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
        let Some(compose) = &self.compose else {
            return;
        };
        let top = view_top_for(self, compose);
        if let Some(compose) = self.compose.as_mut() {
            compose.view_top = top;
        }
    }
}
