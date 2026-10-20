use std::collections::HashMap;

use super::external::{Clipboard, Launcher, copy_text, open_url};
use crate::action::{Action, Effect};
use crate::app::{App, LogTab};
use crate::drafts::Drafts;

/// Which API client the next request goes out with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientKind {
    Guest,
    Signed,
}

/// Runs an effect in the background and sends its actions back to the loop.
/// Tests give a fake that records what it was asked to run.
pub trait Exec {
    fn spawn(&mut self, client: ClientKind, effect: Effect);
}

/// Keeps what must outlive the process. Tests give a fake.
pub trait Store {
    /// Records the first commit of each tab loaded in this session.
    fn save_last_visits(&mut self, loaded: &HashMap<LogTab, String>) -> Result<(), String>;

    /// Writes the drafts of `account`; an empty set removes the file.
    fn save_drafts(&mut self, account: &str, drafts: &Drafts) -> Result<(), String>;

    /// Reads the drafts of `account`; a missing or corrupt file gives none.
    fn load_drafts(&mut self, account: &str) -> Drafts;
}

/// What the loop does after an action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Control {
    Continue,
    /// `save_error` is set when the state could not be written; main prints it once the terminal is restored.
    Quit {
        save_error: Option<String>,
    },
}

/// The app with everything it acts on, so that the loop in main holds no logic.
pub struct Runtime<E, L, C, S> {
    pub app: App,
    pub client: ClientKind,
    pub exec: E,
    pub launcher: L,
    pub clipboard: C,
    pub store: S,
    pub base_url: String,
}

impl<E: Exec, L: Launcher, C: Clipboard, S: Store> Runtime<E, L, C, S> {
    /// Feeds `action` to the app and carries out the effects it returns.
    pub fn handle(&mut self, action: Action) -> Control {
        let effects = self.app.update(action);
        self.apply(effects)
    }

    /// Carries out `effects` in order, so that a client swap applies to the effects after it only.
    /// Stops at `Effect::Quit`, saving the state first.
    pub fn apply(&mut self, effects: Vec<Effect>) -> Control {
        let mut drafts_error: Option<String> = None;
        for effect in effects {
            let notices = match effect {
                Effect::Quit => {
                    let state_error = self.store.save_last_visits(&self.app.heads_to_save()).err();
                    // Named only when there are two, since one error is clear from the message that main prints.
                    let save_error = match (state_error, drafts_error.take()) {
                        (Some(state), Some(drafts)) => {
                            Some(format!("state: {state}; drafts: {drafts}"))
                        }
                        (state, drafts) => state.or(drafts),
                    };
                    return Control::Quit { save_error };
                }
                Effect::UseGuestClient => {
                    self.client = ClientKind::Guest;
                    continue;
                }
                Effect::UseSignedClient => {
                    self.client = ClientKind::Signed;
                    continue;
                }
                Effect::SaveDrafts { account, drafts } => {
                    match self.store.save_drafts(&account, &drafts) {
                        Ok(()) => continue,
                        Err(error) => {
                            drafts_error = Some(error);
                            vec![Action::DraftsSaveFailed]
                        }
                    }
                }
                Effect::LoadDrafts { account } => {
                    let drafts = self.store.load_drafts(&account);
                    vec![Action::DraftsLoaded { account, drafts }]
                }
                Effect::OpenUrl(url) => open_url(&self.launcher, &self.base_url, &url),
                Effect::CopyToClipboard(text) => copy_text(&self.clipboard, &text),
                other => {
                    self.exec.spawn(self.client, other);
                    continue;
                }
            };
            for notice in notices {
                if let quit @ Control::Quit { .. } = self.handle(notice) {
                    return quit;
                }
            }
        }
        if let Some(error) = drafts_error {
            let text = format!("drafts could not be saved ({error})");
            return self.handle(Action::Notice {
                kind: crate::app::ToastKind::Error,
                text,
            });
        }
        Control::Continue
    }
}

/// Keeps the last visit in `state.toml` under `dir`.
pub struct FileStore {
    pub dir: std::path::PathBuf,
}

impl Store for FileStore {
    fn save_last_visits(&mut self, loaded: &HashMap<LogTab, String>) -> Result<(), String> {
        crate::config::save_last_visits(&self.dir, loaded).map_err(|e| e.to_string())
    }

    fn save_drafts(&mut self, account: &str, drafts: &Drafts) -> Result<(), String> {
        crate::drafts::save_drafts(&self.dir, account, drafts).map_err(|e| e.to_string())
    }

    fn load_drafts(&mut self, account: &str) -> Drafts {
        crate::drafts::load_drafts(&self.dir, account)
    }
}
