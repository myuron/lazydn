//! The Appearance tab of Settings: moving over its five rows, changing a value, and the effect that saves it.
//!
//! - `j` / `k` (and the arrows) move over Theme, Collapse nav, Lane dimming, Reply previews and ASCII mode.
//! - `Left` / `Right` / `h` / `l` change the value of the focused row; `Enter` and `Space` do too.
//! - A change shows at once in `prefs()` and asks for exactly one `Effect::SaveConfig` that carries all the prefs.
//! - Reply previews walk 1, 3, 5, 10, 15, 20, 25: the arrows stop at the ends, `Enter` and `Space` wrap round.
//! - A guest can change these values, because they never touch the account.

use crossterm::event::KeyCode;

use crate::action::{Action, Effect};
use crate::app::{App, Prefs, Theme};
use crate::settings::AppearanceRow;
use crate::settings_tests::settings_app;
use crate::test_support::*;
use crate::write_tests::{key, mapped};

const LIMITS: [usize; 7] = [1, 3, 5, 10, 15, 20, 25];
const ROWS: [AppearanceRow; 5] = [
    AppearanceRow::Theme,
    AppearanceRow::CollapseNav,
    AppearanceRow::LaneDimming,
    AppearanceRow::ReplyPreview,
    AppearanceRow::Ascii,
];

/// Signed in on the Appearance tab, the first row focused.
fn appearance_app() -> App {
    let mut app = settings_app(120, 40);
    key(&mut app, '2');
    app
}

fn focus(row: AppearanceRow) -> App {
    let mut app = appearance_app();
    for _ in 0..ROWS.iter().position(|r| *r == row).unwrap_or(0) {
        key(&mut app, 'j');
    }
    assert_eq!(app.settings().appearance_row, row, "the focus did not move");
    app
}

fn saves(effects: &[Effect]) -> Vec<Prefs> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::SaveConfig(prefs) => Some(prefs.clone()),
            _ => None,
        })
        .collect()
}

/// Presses `code` and returns what it saved.
fn change(app: &mut App, code: KeyCode) -> Vec<Prefs> {
    saves(&press(app, code))
}

const CHANGE_KEYS: [KeyCode; 6] = [
    KeyCode::Char('l'),
    KeyCode::Char('h'),
    KeyCode::Right,
    KeyCode::Left,
    KeyCode::Enter,
    KeyCode::Char(' '),
];

// --- the focus ----------------------------------------------------------------------------------

#[test]
fn the_tab_opens_on_the_theme_row() {
    assert_eq!(
        appearance_app().settings().appearance_row,
        AppearanceRow::Theme
    );
}

#[test]
fn j_and_k_walk_the_five_rows_and_stop_at_the_ends() {
    let mut app = appearance_app();
    key(&mut app, 'k');
    assert_eq!(app.settings().appearance_row, AppearanceRow::Theme);
    let mut seen = Vec::new();
    for _ in 0..6 {
        key(&mut app, 'j');
        seen.push(app.settings().appearance_row);
    }
    assert_eq!(
        seen,
        vec![
            AppearanceRow::CollapseNav,
            AppearanceRow::LaneDimming,
            AppearanceRow::ReplyPreview,
            AppearanceRow::Ascii,
            AppearanceRow::Ascii,
            AppearanceRow::Ascii,
        ]
    );
    key(&mut app, 'k');
    assert_eq!(app.settings().appearance_row, AppearanceRow::ReplyPreview);
}

#[test]
fn the_arrow_keys_up_and_down_move_the_focus_too() {
    let mut app = appearance_app();
    press(&mut app, KeyCode::Down);
    assert_eq!(app.settings().appearance_row, AppearanceRow::CollapseNav);
    press(&mut app, KeyCode::Up);
    assert_eq!(app.settings().appearance_row, AppearanceRow::Theme);
}

#[test]
fn the_rows_of_the_other_tabs_do_not_move_the_appearance_focus() {
    let mut app = appearance_app();
    key(&mut app, '1');
    key(&mut app, 'j');
    key(&mut app, '3');
    key(&mut app, 'j');
    assert_eq!(app.settings().appearance_row, AppearanceRow::Theme);
}

#[test]
fn j_moves_the_nav_not_the_rows_while_the_nav_has_the_focus() {
    let mut app = appearance_app();
    press(&mut app, KeyCode::Tab);
    key(&mut app, 'j');
    assert_eq!(app.settings().appearance_row, AppearanceRow::Theme);
}

// --- the keys -----------------------------------------------------------------------------------

#[test]
fn the_keys_of_the_tab_map_to_the_actions_of_the_other_tabs() {
    let app = appearance_app();
    assert_eq!(mapped(&app, 'j'), Some(Action::MoveSelection(1)));
    assert_eq!(mapped(&app, 'k'), Some(Action::MoveSelection(-1)));
    assert_eq!(mapped(&app, 'l'), Some(Action::SettingsStep(1)));
    assert_eq!(mapped(&app, 'h'), Some(Action::SettingsStep(-1)));
    assert_eq!(mapped(&app, ' '), Some(Action::SettingsToggle));
}

#[test]
fn the_profile_and_safety_keys_do_nothing_on_the_appearance_tab() {
    let mut app = appearance_app();
    let before = app.prefs().clone();
    for c in ['S', 'D', 'u', 'G'] {
        let effects = key(&mut app, c);
        assert!(effects.is_empty(), "{c}: {effects:?}");
        assert_eq!(app.modal(), None, "{c}");
    }
    assert_eq!(app.prefs(), &before);
}

// --- theme --------------------------------------------------------------------------------------

#[test]
fn every_change_key_flips_the_theme_and_asks_to_save_it() {
    for code in CHANGE_KEYS {
        let mut app = focus(AppearanceRow::Theme);
        let saved = change(&mut app, code);
        assert_eq!(app.prefs().theme, Theme::Light, "{code:?}");
        assert_eq!(saved, vec![app.prefs().clone()], "{code:?}");
        let saved = change(&mut app, code);
        assert_eq!(app.prefs().theme, Theme::Dark, "{code:?}");
        assert_eq!(saved, vec![app.prefs().clone()], "{code:?}");
    }
}

// --- the three switches -------------------------------------------------------------------------

fn switch_of(prefs: &Prefs, row: AppearanceRow) -> bool {
    match row {
        AppearanceRow::CollapseNav => prefs.collapse_nav,
        AppearanceRow::LaneDimming => prefs.lane_dimming,
        AppearanceRow::Ascii => prefs.ascii,
        other => panic!("{other:?} is not a switch"),
    }
}

#[test]
fn every_change_key_flips_each_switch_and_asks_to_save_it() {
    for row in [
        AppearanceRow::CollapseNav,
        AppearanceRow::LaneDimming,
        AppearanceRow::Ascii,
    ] {
        for code in CHANGE_KEYS {
            let mut app = focus(row);
            assert!(!switch_of(app.prefs(), row));
            let saved = change(&mut app, code);
            assert!(switch_of(app.prefs(), row), "{row:?} {code:?}");
            assert_eq!(saved, vec![app.prefs().clone()], "{row:?} {code:?}");
            let saved = change(&mut app, code);
            assert!(!switch_of(app.prefs(), row), "{row:?} {code:?}");
            assert_eq!(saved, vec![app.prefs().clone()], "{row:?} {code:?}");
        }
    }
}

#[test]
fn a_switch_changes_nothing_else() {
    let mut app = focus(AppearanceRow::LaneDimming);
    let before = app.prefs().clone();
    key(&mut app, 'l');
    assert_eq!(
        app.prefs(),
        &Prefs {
            lane_dimming: true,
            ..before
        }
    );
}

#[test]
fn the_saved_prefs_keep_the_truecolor_of_the_terminal() {
    let mut app = appearance_app();
    app.set_prefs(Prefs {
        truecolor: false,
        ..Prefs::default()
    });
    let saved = change(&mut app, KeyCode::Char('l'));
    assert_eq!(saved.len(), 1);
    assert!(!saved[0].truecolor);
    assert_eq!(saved[0].theme, Theme::Light);
}

// --- reply previews -----------------------------------------------------------------------------

#[test]
fn the_arrows_walk_the_reply_limits_and_stop_at_the_ends() {
    let mut app = focus(AppearanceRow::ReplyPreview);
    assert_eq!(app.prefs().reply_preview_limit, 3);
    let mut up = Vec::new();
    for _ in 0..5 {
        change(&mut app, KeyCode::Char('l'));
        up.push(app.prefs().reply_preview_limit);
    }
    assert_eq!(up, vec![5, 10, 15, 20, 25]);
    assert!(change(&mut app, KeyCode::Char('l')).is_empty());
    assert_eq!(app.prefs().reply_preview_limit, 25);

    let mut down = Vec::new();
    for _ in 0..6 {
        change(&mut app, KeyCode::Char('h'));
        down.push(app.prefs().reply_preview_limit);
    }
    assert_eq!(down, vec![20, 15, 10, 5, 3, 1]);
    assert!(change(&mut app, KeyCode::Char('h')).is_empty());
    assert_eq!(app.prefs().reply_preview_limit, 1);
}

#[test]
fn right_and_left_work_like_l_and_h_on_the_reply_limit() {
    let mut app = focus(AppearanceRow::ReplyPreview);
    change(&mut app, KeyCode::Right);
    assert_eq!(app.prefs().reply_preview_limit, 5);
    change(&mut app, KeyCode::Left);
    change(&mut app, KeyCode::Left);
    assert_eq!(app.prefs().reply_preview_limit, 1);
}

#[test]
fn every_step_of_the_reply_limit_saves_the_new_value_once() {
    let mut app = focus(AppearanceRow::ReplyPreview);
    let saved = change(&mut app, KeyCode::Char('l'));
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].reply_preview_limit, 5);
}

#[test]
fn enter_and_space_step_forward_and_wrap_from_the_last_limit_to_the_first() {
    for code in [KeyCode::Enter, KeyCode::Char(' ')] {
        let mut app = focus(AppearanceRow::ReplyPreview);
        let mut seen = Vec::new();
        for _ in 0..LIMITS.len() {
            let saved = change(&mut app, code);
            assert_eq!(saved.len(), 1, "{code:?}");
            seen.push(app.prefs().reply_preview_limit);
        }
        assert_eq!(seen, vec![5, 10, 15, 20, 25, 1, 3], "{code:?}");
    }
}

// --- who may change it, and where ---------------------------------------------------------------

#[test]
fn a_guest_changes_the_values_and_gets_no_sign_in_dialog() {
    let mut app = guest(120, 40);
    press(&mut app, KeyCode::Esc);
    type_str(&mut app, "gs");
    type_str(&mut app, "2");
    key(&mut app, 'j');
    key(&mut app, 'j');

    let saved = change(&mut app, KeyCode::Char('l'));

    assert!(app.prefs().lane_dimming);
    assert_eq!(saved, vec![app.prefs().clone()]);
    assert_eq!(app.modal(), None);
}

#[test]
fn the_change_keys_save_nothing_on_the_profile_and_safety_tabs() {
    for tab in ['1', '3'] {
        let mut app = settings_app(120, 40);
        key(&mut app, tab);
        let before = app.prefs().clone();
        for code in [KeyCode::Char('l'), KeyCode::Char('h'), KeyCode::Char(' ')] {
            assert!(change(&mut app, code).is_empty(), "{tab} {code:?}");
        }
        assert_eq!(app.prefs(), &before, "{tab}");
    }
}

#[test]
fn the_change_keys_do_nothing_while_the_nav_has_the_focus() {
    let mut app = settings_app(60, 30);
    key(&mut app, '2');
    press(&mut app, KeyCode::Tab);
    let before = app.prefs().clone();

    assert!(change(&mut app, KeyCode::Char('l')).is_empty());
    assert!(change(&mut app, KeyCode::Char(' ')).is_empty());
    assert!(change(&mut app, KeyCode::Enter).is_empty());

    assert_eq!(app.prefs(), &before);
}

#[test]
fn a_change_shows_no_toast() {
    let mut app = focus(AppearanceRow::Ascii);
    key(&mut app, 'l');
    assert!(app.toast().is_none());
}
