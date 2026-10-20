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
    /// Counts the pages that replaced `users`, so that a person taken off the list can tell if it was replaced.
    pub generation: u32,
    /// Set when the server changed the list, so that the next visit fetches it again.
    pub stale: bool,
}

/// Auto, the six colors and custom.
pub const LANE_CHOICES: usize = 8;
/// The index of the custom choice.
pub const CUSTOM_CHOICE: usize = LANE_CHOICES - 1;
/// Digits of a custom color.
pub const HEX_DIGITS: usize = 6;

/// The choice that the saved lane stands for.
pub fn saved_lane_choice(preferred: Option<crate::model::Lane>) -> usize {
    match preferred {
        None => 0,
        Some(crate::model::Lane::Index(i)) => usize::from(i) + 1,
        Some(crate::model::Lane::Custom(_)) => CUSTOM_CHOICE,
    }
}

/// `rrggbb` as a color; anything else is none.
pub fn parse_hex_color(digits: &str) -> Option<crate::model::Rgb> {
    if digits.len() != HEX_DIGITS || !digits.is_ascii() {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&digits[i..i + 2], 16).ok();
    Some(crate::model::Rgb(byte(0)?, byte(2)?, byte(4)?))
}

/// The numbers of reply previews that Appearance walks through.
pub const REPLY_LIMITS: [usize; 7] = [1, 3, 5, 10, 15, 20, 25];

/// The rows of the Appearance tab, top to bottom.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AppearanceRow {
    #[default]
    Theme,
    CollapseNav,
    LaneDimming,
    ReplyPreview,
    Ascii,
}

/// The rows of the Profile tab that take the focus, top to bottom.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ProfileRow {
    #[default]
    Sync,
    Lane,
    Pasteis,
    Delete,
}

#[derive(Debug, Clone, Default)]
pub struct SettingsState {
    pub tab: SettingsTab,
    pub muted: UserList,
    pub blocked: UserList,
    pub profile_row: ProfileRow,
    pub appearance_row: AppearanceRow,
    /// The lane choice under the cursor: 0 is Auto, 1 to 6 the palette and 7 custom. `None` is the saved one.
    pub lane_cursor: Option<usize>,
    /// The hex digits typed so far while the custom color is being entered.
    pub hex_input: Option<String>,
    /// Over the muted users, then the blocked ones.
    pub safety_selected: usize,
    /// Rows of the Safety body that are scrolled off the top.
    pub safety_scroll: u16,
}

/// Where the rows of the Safety body fall, shared by the drawing and the scrolling.
pub struct SafetyLayout {
    /// The row of each selectable person, muted ones first.
    pub user_rows: Vec<usize>,
    /// Whether the person is the first of their list, whose heading sits right above.
    pub first_of_list: Vec<bool>,
    pub total: usize,
}

pub fn safety_layout(state: &SettingsState) -> SafetyLayout {
    let mut row = 0;
    let mut user_rows = Vec::new();
    let mut first_of_list = Vec::new();
    for (i, list) in [&state.muted, &state.blocked].into_iter().enumerate() {
        if i == 1 {
            row += 1;
        }
        row += 1;
        if list.users.is_empty() {
            let note = list.pending.is_some()
                || matches!(list.load, crate::feed::LoadState::Failed { .. })
                || list.loaded_once;
            row += usize::from(note);
            continue;
        }
        for k in 0..list.users.len() {
            user_rows.push(row);
            first_of_list.push(k == 0);
            row += 1;
        }
        row += usize::from(list.next_cursor.is_some());
    }
    SafetyLayout {
        user_rows,
        first_of_list,
        total: row,
    }
}
