use std::path::Path;

use super::{App, KeyStatus, LogTab, Me, Modal, Screen, Session, ToastKind, toast};
use crate::action::{Action, Effect, Failure};
use crate::api::keys::KeyError;
use crate::api::pkce::Pkce;
use crate::signin::{CodeError, PASTE_MAX, parse_code, sanitize_paste};

/// Where the sign-in dialog stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignInStep {
    /// Nothing sent yet.
    Idle,
    /// The approval URL is ready and the Code field takes input.
    Ready,
    /// A `RegisterDevice` is in flight.
    Submitting,
    /// The last submit failed; carries the error code.
    Failed(String),
}

/// What the dialog keeps for the whole session, so closing and reopening it loses nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignInState {
    pub step: SignInStep,
    /// A `PrepareSignIn` is in flight; kept apart from `step` so that a failed submit can wait for a new URL.
    pub preparing: bool,
    pub fingerprint: Option<String>,
    pub url: Option<String>,
    pub pkce: Option<Pkce>,
    /// The `state` of the approval URL; a pasted callback must carry the same one.
    pub oauth_state: Option<String>,
    /// The Code field.
    pub code: String,
    /// Whether the key file was made by this sign-in.
    pub created: bool,
    /// The message under the Code field.
    pub error: Option<String>,
}

impl Default for SignInState {
    fn default() -> Self {
        Self {
            step: SignInStep::Idle,
            preparing: false,
            fingerprint: None,
            url: None,
            pkce: None,
            oauth_state: None,
            code: String::new(),
            created: false,
            error: None,
        }
    }
}

impl SignInState {
    /// Drops the approval URL and what belongs to it, but not what the user typed.
    fn forget_url(&mut self) {
        self.fingerprint = None;
        self.url = None;
        self.pkce = None;
        self.oauth_state = None;
    }
}

const INVALID_GRANT_TEXT: &str = "the code was not accepted — approve again and paste the new URL";
const COPY_SENT_TEXT: &str = "Copy sent to the terminal (OSC 52)";

fn failure_message(failure: &Failure) -> String {
    match failure.code.as_str() {
        "invalid_grant" => INVALID_GRANT_TEXT.into(),
        "network" => "network error — press Enter to try again".into(),
        "invalid_signature" => toast::failure_text(failure),
        code => format!("sign-in failed: {}", toast::clean(code)),
    }
}

impl App {
    /// The dialog state; kept while the dialog is closed.
    pub fn sign_in(&self) -> &SignInState {
        &self.sign_in
    }

    /// Where the device key lives, as the dialog shows it.
    pub fn key_path(&self) -> &Path {
        &self.key_path
    }

    /// Why the key cannot be used, in the words the dialog shows; `None` when it can.
    pub fn key_problem(&self) -> Option<String> {
        match &self.key_status {
            KeyStatus::Unreadable { error, path } => Some(toast::key_error_text(error, path)),
            KeyStatus::Absent | KeyStatus::Present => None,
        }
    }

    /// The one way the dialog opens. Returns the effects that prepare the approval URL.
    pub(super) fn open_sign_in(&mut self) -> Vec<Effect> {
        self.modal = Some(Modal::SignIn);
        if self.key_status == KeyStatus::Present
            && !self.sign_in.preparing
            && self.sign_in.url.is_none()
        {
            self.sign_in.preparing = true;
            return vec![Effect::PrepareSignIn { create: false }];
        }
        Vec::new()
    }

    /// Whether the Code field takes input: the URL is there and no answer is awaited.
    fn sign_in_accepts_input(&self) -> bool {
        self.modal == Some(Modal::SignIn)
            && self.key_status == KeyStatus::Present
            && self.sign_in.pkce.is_some()
            && !self.sign_in.preparing
            && self.sign_in.step != SignInStep::Submitting
    }

    fn edit_code(&mut self, edit: impl FnOnce(&mut String)) {
        if self.sign_in_accepts_input() {
            edit(&mut self.sign_in.code);
            self.sign_in.error = None;
        }
    }

    pub(super) fn on_sign_in_action(&mut self, action: Action) -> Vec<Effect> {
        match action {
            Action::SignInInput(c) => self.edit_code(|code| {
                if code.chars().count() < PASTE_MAX {
                    code.push(c);
                }
            }),
            Action::SignInBackspace => self.edit_code(|code| {
                code.pop();
            }),
            Action::SignInClear => self.edit_code(String::clear),
            Action::Paste(text) => self.edit_code(|code| {
                let room = PASTE_MAX.saturating_sub(code.chars().count());
                code.extend(sanitize_paste(&text).chars().take(room));
            }),
            Action::SignInSubmit => return self.sign_in_submit(),
            Action::SignInRefresh => return self.sign_in_refresh(),
            Action::SignInOpenBrowser => {
                if self.modal == Some(Modal::SignIn)
                    && let Some(url) = self.sign_in.url.clone()
                {
                    return vec![Effect::OpenUrl(url)];
                }
            }
            Action::SignInCopyUrl => {
                if self.modal == Some(Modal::SignIn)
                    && let Some(url) = self.sign_in.url.clone()
                {
                    self.show_toast(ToastKind::Info, COPY_SENT_TEXT.into());
                    return vec![Effect::CopyToClipboard(url)];
                }
            }
            Action::SignInPrepared {
                fingerprint,
                url,
                pkce,
                state,
                created,
            } => {
                let sign_in = &mut self.sign_in;
                sign_in.preparing = false;
                sign_in.fingerprint = Some(fingerprint);
                sign_in.url = Some(url);
                sign_in.pkce = Some(pkce);
                sign_in.oauth_state = Some(state);
                sign_in.created = created;
                // Only a failed submit leaves a message worth keeping across the new URL.
                if !matches!(sign_in.step, SignInStep::Failed(_)) {
                    sign_in.step = SignInStep::Ready;
                    sign_in.error = None;
                }
                self.key_status = KeyStatus::Present;
            }
            Action::SignInKeyFailed { error } => {
                self.key_status = KeyStatus::Unreadable {
                    error,
                    path: self.key_path.clone(),
                };
                self.forget_prepared();
            }
            Action::SignInKeyMissing => {
                self.key_status = KeyStatus::Absent;
                self.forget_prepared();
            }
            Action::SignInPrepareFailed { failure } => {
                self.sign_in.preparing = false;
                self.sign_in.error = Some(failure_message(&failure));
            }
            Action::SignInFailed { failure } => return self.sign_in_failed(&failure),
            Action::DeviceRegistered { response } => return self.device_registered(&response),
            Action::Notice { kind, text } => self.show_toast(kind, text),
            _ => {}
        }
        Vec::new()
    }

    /// The key is gone or unusable, so nothing prepared from it is worth keeping.
    fn forget_prepared(&mut self) {
        self.sign_in.preparing = false;
        self.sign_in.step = SignInStep::Idle;
        self.sign_in.forget_url();
    }

    fn sign_in_submit(&mut self) -> Vec<Effect> {
        if self.modal != Some(Modal::SignIn)
            || self.sign_in.preparing
            || self.sign_in.step == SignInStep::Submitting
        {
            return Vec::new();
        }
        let create = match &self.key_status {
            KeyStatus::Absent => true,
            // A key that failed to be made is made again; any other unreadable key is only read again.
            KeyStatus::Unreadable { error, .. } => matches!(error, KeyError::Generate(_)),
            KeyStatus::Present if self.sign_in.pkce.is_none() => false,
            KeyStatus::Present => return self.submit_code(),
        };
        self.sign_in.preparing = true;
        vec![Effect::PrepareSignIn { create }]
    }

    fn submit_code(&mut self) -> Vec<Effect> {
        let (Some(pkce), Some(state)) = (&self.sign_in.pkce, &self.sign_in.oauth_state) else {
            return Vec::new();
        };
        match parse_code(&self.sign_in.code, state) {
            Err(CodeError::Empty) => Vec::new(),
            Err(error) => {
                self.sign_in.error = Some(error.text().into());
                Vec::new()
            }
            Ok(code) => {
                let verifier = pkce.verifier.as_str().into();
                self.sign_in.step = SignInStep::Submitting;
                self.sign_in.error = None;
                vec![Effect::RegisterDevice { code, verifier }]
            }
        }
    }

    fn sign_in_refresh(&mut self) -> Vec<Effect> {
        let usable = matches!(
            self.key_status,
            KeyStatus::Present | KeyStatus::Unreadable { .. }
        );
        if self.modal != Some(Modal::SignIn)
            || !usable
            || self.sign_in.preparing
            || self.sign_in.step == SignInStep::Submitting
        {
            return Vec::new();
        }
        self.sign_in.forget_url();
        self.sign_in.error = None;
        if self.sign_in.step == SignInStep::Ready {
            self.sign_in.step = SignInStep::Idle;
        }
        self.sign_in.preparing = true;
        vec![Effect::PrepareSignIn { create: false }]
    }

    fn sign_in_failed(&mut self, failure: &Failure) -> Vec<Effect> {
        if self.sign_in.step != SignInStep::Submitting {
            return Vec::new();
        }
        self.sign_in.step = SignInStep::Failed(failure.code.clone());
        self.sign_in.error = Some(failure_message(failure));
        if failure.code != "invalid_grant" {
            return Vec::new();
        }
        // The code was spent or wrong, and the verifier goes with it.
        self.sign_in.forget_url();
        self.sign_in.preparing = true;
        vec![Effect::PrepareSignIn { create: false }]
    }

    fn device_registered(&mut self, response: &crate::model::MeResponse) -> Vec<Effect> {
        if self.sign_in.step != SignInStep::Submitting {
            return Vec::new();
        }
        self.sign_in_boundary = Some(crate::feed::ReqId(self.next_req));
        self.session = Session::SignedIn(Me::from(response));
        self.reset_account_data();
        // The response is as fresh as a FetchMe would be.
        self.last_me_fetch = Some(self.now);
        self.sign_in = Default::default();
        self.modal = None;
        let mut effects = vec![Effect::UseSignedClient];
        effects.extend(self.load_drafts_effect());
        // The signed-in session passes the guard, so only the fetch is left to collect.
        let _ = self.goto(Screen::Log(LogTab::Following));
        effects.extend(self.entry_fetch());
        effects
    }
}
