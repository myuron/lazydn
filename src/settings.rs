use crate::app::SettingsTab;
use crate::feed::{LoadState, Pending};
use crate::model::User;

/// One Safety list; only its first page is ever shown, so there is no selection or append.
#[derive(Debug, Clone, Default)]
pub struct UserList {
    pub users: Vec<User>,
    pub next_cursor: Option<String>,
    pub load: LoadState,
    pub pending: Option<Pending>,
    pub loaded_once: bool,
}

#[derive(Debug, Clone, Default)]
pub struct SettingsState {
    pub tab: SettingsTab,
    pub muted: UserList,
    pub blocked: UserList,
}
