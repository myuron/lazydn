//! The key hints of the status line and of the Keys box of the Aside; docs/screen-design.md has the tables.

use unicode_width::UnicodeWidthStr;

use crate::app::{App, Modal, Screen, SettingsTab};
use crate::layout::LayoutMode;

type Item = (&'static str, &'static str);

const TAB: Item = ("Tab", "pane");
const HELP: Item = ("?", "help");
const ALL_KEYS: Item = ("?", "all keys");
const MOVE: Item = ("j/k", "move");
const OPEN: Item = ("↵", "open");
const BACK: Item = ("Esc", "back");
const CLOSE: Item = ("Esc", "close");
const TAB3: Item = ("1-3", "tab");
const CANCEL: Item = ("n/Esc", "cancel");

#[derive(Clone, Copy, PartialEq, Eq)]
enum Ctx {
    Log,
    Saved,
    Thread,
    ProfileOther,
    ProfileOwn,
    ProfileBlocked,
    Connections,
    Notifications,
    SettingsProfile,
    SettingsAppearance,
    SettingsSafety,
    SettingsGuest,
    Help,
    SignIn,
    Composer,
    ConfirmDelete,
    ConfirmBlock,
    ConfirmAccount,
    Report,
    Menu,
    Hex,
}

fn ctx(app: &App) -> Ctx {
    if app.modal() == Some(Modal::SignIn) {
        return Ctx::SignIn;
    }
    if app.composer_focused() {
        return Ctx::Composer;
    }
    match app.modal() {
        Some(Modal::Help) => return Ctx::Help,
        Some(Modal::Composer) => return Ctx::Composer,
        Some(Modal::ConfirmDelete) => return Ctx::ConfirmDelete,
        Some(Modal::ConfirmBlock) => return Ctx::ConfirmBlock,
        Some(Modal::ConfirmAccountDelete) => return Ctx::ConfirmAccount,
        Some(Modal::Report) => return Ctx::Report,
        Some(Modal::Menu) => return Ctx::Menu,
        Some(Modal::SignIn) | None => {}
    }
    if app.settings_typing() {
        return Ctx::Hex;
    }
    match app.screen() {
        Screen::Log(_) => Ctx::Log,
        Screen::Saved => Ctx::Saved,
        Screen::Thread(_) => Ctx::Thread,
        Screen::Profile(name) => {
            if app
                .profile(&name)
                .is_some_and(|s| s.error.as_deref() == Some("user_blocked"))
            {
                Ctx::ProfileBlocked
            } else if app.is_own(&name) {
                Ctx::ProfileOwn
            } else {
                Ctx::ProfileOther
            }
        }
        Screen::Connections(..) => Ctx::Connections,
        Screen::Notifications => Ctx::Notifications,
        Screen::Settings => match app.settings().tab {
            SettingsTab::Appearance => Ctx::SettingsAppearance,
            _ if app.is_guest() => Ctx::SettingsGuest,
            SettingsTab::Profile => Ctx::SettingsProfile,
            SettingsTab::Safety => Ctx::SettingsSafety,
        },
    }
}

/// The hints of the status line, most important first; `Tab pane` and `? help` close the list.
fn status_items(ctx: Ctx) -> &'static [Item] {
    match ctx {
        Ctx::Log => &[MOVE, OPEN, ("c", "commit"), TAB, HELP],
        Ctx::Saved => &[MOVE, OPEN, ("b", "unsave"), TAB, HELP],
        Ctx::Thread => &[MOVE, OPEN, ("r", "reply"), BACK, TAB, HELP],
        Ctx::ProfileOther => &[MOVE, OPEN, ("1-4", "tab"), ("f", "follow"), BACK, TAB, HELP],
        Ctx::ProfileOwn => &[MOVE, OPEN, ("1-4", "tab"), BACK, TAB, HELP],
        Ctx::ProfileBlocked => &[("B", "unblock"), BACK, TAB, HELP],
        Ctx::Connections => &[MOVE, OPEN, ("1-2", "tab"), ("f", "follow"), BACK, TAB, HELP],
        Ctx::Notifications => &[MOVE, OPEN, ("1-6", "filter"), ("a", "all"), TAB, HELP],
        Ctx::SettingsProfile => &[
            ("j/k", "row"),
            ("h/l", "choose"),
            ("S", "sync"),
            ("D", "delete"),
            TAB3,
            TAB,
            HELP,
        ],
        Ctx::SettingsAppearance => &[("j/k", "row"), ("h/l", "change"), TAB3, TAB, HELP],
        Ctx::SettingsSafety => &[MOVE, ("u", "release"), ("R", "reload"), TAB3, TAB, HELP],
        Ctx::SettingsGuest => &[("L", "sign in"), TAB3, TAB, HELP],
        Ctx::Help => &[CLOSE, ("Q", "quit")],
        Ctx::SignIn => &[
            ("↵", "submit"),
            CLOSE,
            ("^O", "browser"),
            ("^Y", "copy"),
            ("^U", "clear"),
            ("^R", "refresh"),
        ],
        Ctx::Composer => &[("^S", "send"), ("↵", "newline"), CLOSE],
        Ctx::ConfirmDelete => &[("y", "delete"), CANCEL],
        Ctx::ConfirmBlock => &[("y", "block"), CANCEL],
        Ctx::ConfirmAccount => &[("↵", "delete"), CLOSE],
        Ctx::Report => &[("j/k", "reason"), ("Tab", "details"), ("↵", "send"), CLOSE],
        Ctx::Menu => &[MOVE, ("↵", "run"), CLOSE],
        Ctx::Hex => &[("0-9a-f", "type"), ("↵", "set"), ("Esc", "cancel")],
    }
}

/// The pairs of the Keys box in reading order; none where the Aside shows no such box.
fn keys_items(ctx: Ctx) -> Option<&'static [Item]> {
    Some(match ctx {
        Ctx::Log => &[
            MOVE,
            OPEN,
            ("c", "commit"),
            ("r", "reply"),
            ("s", "star"),
            ("o", "replies"),
            ALL_KEYS,
        ],
        Ctx::Saved => &[
            MOVE,
            OPEN,
            ("b", "unsave"),
            ("r", "reply"),
            ("s", "star"),
            ("o", "replies"),
            ALL_KEYS,
        ],
        Ctx::Thread => &[
            MOVE,
            OPEN,
            ("r", "reply"),
            ("q", "quote"),
            ("s", "star"),
            ("o", "replies"),
            BACK,
            ALL_KEYS,
        ],
        Ctx::ProfileOther => &[
            MOVE,
            OPEN,
            ("1-4", "tab"),
            ("f", "follow"),
            ("m", "mute"),
            ("B", "block"),
            BACK,
            ALL_KEYS,
        ],
        Ctx::ProfileOwn => &[MOVE, OPEN, ("1-4", "tab"), ("s", "star"), BACK, ALL_KEYS],
        Ctx::ProfileBlocked => &[("B", "unblock"), BACK, ALL_KEYS],
        Ctx::Connections => &[MOVE, OPEN, ("1-2", "tab"), ("f", "follow"), BACK, ALL_KEYS],
        Ctx::Notifications => &[MOVE, OPEN, ("1-6", "filter"), ("a", "all"), ALL_KEYS],
        Ctx::SettingsProfile => &[
            ("j/k", "row"),
            ("h/l", "choose"),
            ("S", "sync"),
            ("D", "delete"),
            TAB3,
            ALL_KEYS,
        ],
        Ctx::SettingsAppearance => &[("j/k", "row"), ("h/l", "change"), TAB3, ALL_KEYS],
        Ctx::SettingsSafety => &[MOVE, ("u", "release"), ("R", "reload"), TAB3, ALL_KEYS],
        Ctx::SettingsGuest | Ctx::SignIn => return None,
        other => status_items(other),
    })
}

fn key_text(key: &str, ascii: bool) -> String {
    if ascii {
        key.replace('↵', "Enter")
    } else {
        key.to_string()
    }
}

/// The hints after the breadcrumb: `{breadcrumb}  {key} {label}  …`, or the keys alone on a narrow terminal.
/// `right` is the width of the right end, which keeps two blank columns from the hints.
pub fn status_left(app: &App, mode: LayoutMode, breadcrumb: &str, right: usize) -> String {
    let ascii = app.prefs().ascii;
    let mut items: Vec<(String, &str)> = status_items(ctx(app))
        .iter()
        .filter(|item| **item != TAB || mode == LayoutMode::Wide)
        .map(|(k, l)| (key_text(k, ascii), *l))
        .collect();
    match mode {
        LayoutMode::TooSmall => return String::new(),
        LayoutMode::Narrow => {
            let keys: Vec<&str> = items.iter().map(|(k, _)| k.as_str()).collect();
            return format!(" {}", keys.join(" "));
        }
        _ => {}
    }
    let width = usize::from(app.size().0);
    let text = |items: &[(String, &str)]| {
        let body: Vec<String> = items.iter().map(|(k, l)| format!("{k} {l}")).collect();
        format!(" {breadcrumb}  {}", body.join("  "))
    };
    let help = items.last().is_some_and(|(k, _)| k == "?");
    while items.len() > 1 && text(&items).width() + right + 3 > width {
        items.remove(items.len() - if help { 2 } else { 1 });
    }
    text(&items)
}

/// The content rows of the Keys box: two pairs a row, four rows at most.
pub fn keys_rows(app: &App) -> Option<Vec<String>> {
    let ascii = app.prefs().ascii;
    let pairs: Vec<(String, &str)> = keys_items(ctx(app))?
        .iter()
        .map(|(k, l)| (key_text(k, ascii), *l))
        .collect();
    let column = |c: usize| pairs.iter().skip(c).step_by(2);
    let full = pairs.len() / 2 * 2;
    let widest_key = |c: usize| column(c).map(|(k, _)| k.width()).max().unwrap_or(0);
    let widest_label = pairs[..full]
        .iter()
        .step_by(2)
        .map(|(_, l)| l.width())
        .max()
        .unwrap_or(0);
    let (key1, label1, key2) = (widest_key(0) + 2, widest_label + 3, widest_key(1) + 2);
    let pad = |s: &str, n: usize| format!("{s}{}", " ".repeat(n.saturating_sub(s.width())));
    Some(
        pairs
            .chunks(2)
            .map(|row| match row {
                [(k1, l1), (k2, l2)] => {
                    format!(" {}{}{}{l2}", pad(k1, key1), pad(l1, label1), pad(k2, key2))
                }
                [(k1, l1)] => format!(" {}{l1}", pad(k1, key1)),
                _ => String::new(),
            })
            .collect(),
    )
}
