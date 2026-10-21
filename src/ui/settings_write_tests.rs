//! How the Profile and Safety tabs show their focus and the account deletion dialog.
//!
//! - The focused row of the Profile tab starts with `▌` in the first column of its line (the Sync line, the
//!   `Lane color` heading, the `Pasteis mode` line and the `[D] Delete account` line).
//! - On the Lane color row the cursor is a `▌` in the gap in front of its choice, while the saved choice keeps
//!   `(•)`. The cursor is not drawn while another row has the focus. The `custom` choice shows the digits typed
//!   so far, padded with `_` to six; the preview takes the color of the choice under the cursor.
//! - On the Safety tab the selected person starts with `▌`, has the cursor background, and the body starts at the
//!   scroll row. Each row ends with `[u] Unmute` or `[u] Unblock`. An empty list reads `No muted users.` or
//!   `No blocked users.`.
//! - The deletion dialog is 50 columns wide with the title `Delete account`; the `[Enter] Delete` hint is shown
//!   only while the typed username matches.
//! - A guest sees `Sign in to change these settings. [L] Sign in` on Profile and Safety, and the Appearance rows.
//!
//! The snapshots `settings_profile_focus`, `settings_lane_custom`, `delete_account_confirm`,
//! `delete_account_ready`, `settings_safety_scrolled` and `settings_guest_profile` were written by hand.

use ratatui::style::Color;

use crate::app::{App, Me};
use crate::model::{Lane, Rgb};
use crate::settings_tests::*;
use crate::settings_write_tests::safety_app;
use crate::test_support::*;
use crate::ui::settings_tests::*;
use crate::ui::theme;
use crossterm::event::KeyCode;

macro_rules! snapshot {
    ($name:literal, $app:expr) => {{
        let terminal = render(&$app);
        insta::assert_snapshot!($name, terminal.backend());
    }};
}

// --- fixtures ---------------------------------------------------------------

fn me_with(preferred: Option<Lane>) -> Me {
    let mut me = fixture_me();
    me.preferred_lane = preferred;
    if let Some(lane) = preferred {
        me.lane = lane;
    }
    me
}

/// Settings on the Profile tab of an account with the saved lane, after the keys.
fn lane_app(preferred: Option<Lane>, w: u16, h: u16, keys: &str) -> App {
    let mut app = settings_app_for(me_with(preferred), w, h);
    type_str(&mut app, keys);
    app
}

/// Custom is chosen, the field is open and `typed` is in it.
fn hex_app(preferred: Option<Lane>, w: u16, h: u16, typed: &str) -> App {
    let mut app = lane_app(preferred, w, h, "j");
    let from = match preferred {
        None => 0,
        Some(Lane::Index(i)) => usize::from(i) + 1,
        Some(Lane::Custom(_)) => 7,
    };
    type_str(&mut app, &"l".repeat(7 - from));
    press(&mut app, KeyCode::Enter);
    type_str(&mut app, typed);
    app
}

fn screen_text(app: &App) -> String {
    let terminal = render(app);
    let buffer = terminal.backend().buffer();
    (0..buffer.area.height)
        .map(|y| row_text(buffer, y))
        .collect::<Vec<_>>()
        .join("\n")
}

fn marked_rows(app: &App) -> usize {
    body_of(app, 18).iter().filter(|r| r.contains('▌')).count()
}

fn guest_settings(w: u16, h: u16) -> App {
    let mut app = guest(w, h);
    press(&mut app, KeyCode::Esc);
    type_str(&mut app, "gs");
    app
}

// --- the focus of the Profile tab ---------------------------------------------

#[test]
fn each_focus_row_carries_the_marker_on_its_first_line() {
    let cases = [
        (
            "",
            0,
            "▌Profile mirrored from GitHub   (synced 2h ago)   [S] Sync".to_string(),
        ),
        ("j", 2, "▌Lane color".to_string()),
        ("jj", 7, right_aligned("▌Pasteis mode", "[x] on", W - 1)),
        ("jjj", 11, "▌ [D] Delete account".to_string()),
    ];
    for (keys, row, want) in cases {
        let app = on_tab(settings_app(120, 40), keys);
        assert_eq!(body_of(&app, 12)[row], want, "keys {keys:?}");
        let expected_marked = if keys == "j" { 2 } else { 1 };
        assert_eq!(marked_rows(&app), expected_marked, "keys {keys:?}");
    }
}

#[test]
fn the_lane_cursor_marks_its_choice_and_the_saved_choice_keeps_its_dot() {
    let app = lane_app(Some(Lane::Index(0)), 120, 40, "jll");
    let got = body_of(&app, 5);
    assert_eq!(
        got[3],
        "  ( ) Auto (green)  (•) ● orange  ( ) ● green ▌( ) ● violet"
    );
    assert_eq!(
        got[4],
        "  ( ) ● teal  ( ) ● rose  ( ) ● ochre  ( ) custom #______"
    );
}

#[test]
fn the_cursor_starts_on_the_saved_choice_when_the_row_gets_the_focus() {
    let app = lane_app(Some(Lane::Index(0)), 120, 40, "j");
    assert_eq!(
        body_of(&app, 4)[3],
        "  ( ) Auto (green) ▌(•) ● orange  ( ) ● green  ( ) ● violet"
    );
}

#[test]
fn the_cursor_marker_sits_in_front_of_a_choice_that_starts_a_line() {
    let app = lane_app(Some(Lane::Index(0)), 120, 40, "jlll");
    assert_eq!(
        body_of(&app, 5)[4],
        " ▌( ) ● teal  ( ) ● rose  ( ) ● ochre  ( ) custom #______"
    );
    let auto = lane_app(Some(Lane::Index(0)), 120, 40, "jh");
    assert_eq!(
        body_of(&auto, 4)[3],
        " ▌( ) Auto (green)  (•) ● orange  ( ) ● green  ( ) ● violet"
    );
    let custom = lane_app(Some(Lane::Index(0)), 120, 40, "jllllll");
    assert_eq!(
        body_of(&custom, 5)[4],
        "  ( ) ● teal  ( ) ● rose  ( ) ● ochre ▌( ) custom #______"
    );
}

#[test]
fn the_cursor_is_not_drawn_while_another_row_has_the_focus() {
    let mut app = lane_app(Some(Lane::Index(0)), 120, 40, "jll");
    assert!(
        body_of(&app, 5)[3].contains('▌'),
        "the cursor is drawn on the row"
    );
    type_str(&mut app, "j");
    let got = body_of(&app, 5);
    assert_eq!(
        got[3],
        "  ( ) Auto (green)  (•) ● orange  ( ) ● green  ( ) ● violet"
    );
    assert!(!got[4].contains('▌'), "{}", got[4]);
}

// --- the custom color -----------------------------------------------------------

#[test]
fn the_custom_choice_shows_the_typed_digits_padded_with_underscores() {
    for (typed, want) in [
        ("", "#______"),
        ("3", "#3_____"),
        ("3a7b", "#3a7b__"),
        ("3A7bd5", "#3a7bd5"),
    ] {
        let app = hex_app(None, 120, 40, typed);
        let line = &body_of(&app, 5)[4];
        assert!(
            line.contains(&format!("custom {want}")),
            "{typed:?}: {line}"
        );
    }
}

#[test]
fn closing_the_field_shows_the_saved_color_again() {
    let mut app = hex_app(None, 120, 40, "ff");
    press(&mut app, KeyCode::Esc);
    assert!(body_of(&app, 5)[4].contains("custom #______"));

    let saved = Some(Lane::Custom(Rgb(0x3a, 0x7b, 0xd5)));
    let mut app = hex_app(saved, 120, 40, "ff");
    assert!(body_of(&app, 5)[4].contains("(•) custom #ff____"));
    press(&mut app, KeyCode::Esc);
    assert!(body_of(&app, 5)[4].contains("(•) custom #3a7bd5"));
}

/// The foreground of the three glyphs of the preview line.
fn preview_colors(app: &App) -> Vec<Color> {
    let inner = inner_rect(app);
    let y = inner.y + 3 + 5;
    let terminal = render(app);
    let buffer = terminal.backend().buffer();
    (inner.x..inner.right())
        .filter(|x| matches!(buffer[(*x, y)].symbol(), "◉" | "●"))
        .map(|x| buffer[(x, y)].fg)
        .collect()
}

#[test]
fn the_preview_takes_the_color_of_the_choice_under_the_cursor() {
    let color = |lane| theme::lane_color(lane, true);
    let cases = [
        (
            "saved stays",
            lane_app(Some(Lane::Index(0)), 120, 40, "j"),
            color(Lane::Index(0)),
        ),
        (
            "a color",
            lane_app(Some(Lane::Index(0)), 120, 40, "jll"),
            color(Lane::Index(2)),
        ),
        (
            "auto",
            lane_app(Some(Lane::Index(0)), 120, 40, "jh"),
            color(Lane::Index(1)),
        ),
        (
            "typed color",
            hex_app(Some(Lane::Index(0)), 120, 40, "3a7bd5"),
            color(Lane::Custom(Rgb(0x3a, 0x7b, 0xd5))),
        ),
        (
            "unfinished color",
            hex_app(Some(Lane::Index(0)), 120, 40, "3a7"),
            color(Lane::Index(0)),
        ),
    ];
    for (name, app, want) in cases {
        let got = preview_colors(&app);
        assert_eq!(got.len(), 3, "{name}");
        assert!(got.iter().all(|c| *c == want), "{name}: {got:?}");
    }
}

// --- the Safety tab -------------------------------------------------------------

#[test]
fn the_body_of_the_safety_tab_starts_at_the_scroll_row() {
    let mut app = safety_app(
        &[
            "amber", "bella", "chloe", "diego", "elena", "felix", "grace", "hiram", "irene",
            "jonas", "karin", "lucas",
        ],
        &[
            "nadia", "oscar", "petra", "quinn", "ravi", "sofia", "tomas", "ursula",
        ],
        80,
        24,
    );
    assert_eq!(body_of(&app, 1)[0], " Muted");
    type_str(&mut app, "G");
    let got = body_of(&app, 18);
    assert_eq!(got[0], right_aligned("  ◉ Elena @elena", "[u] Unmute", 73));
    assert_eq!(got[8], "");
    assert_eq!(got[9], " Blocked");
    assert_eq!(
        got[17],
        right_aligned("▌ ◉ Ursula @ursula", "[u] Unblock", 73)
    );
}

// --- the deletion dialog --------------------------------------------------------

#[test]
fn the_dialog_shows_the_delete_hint_only_while_the_username_matches() {
    let mut app = on_tab(settings_app(120, 40), "D");
    type_str(&mut app, "myur");
    let text = screen_text(&app);
    for want in [
        " Delete account ",
        "Delete your account and every device?",
        "Type myuron to confirm:",
        "> myur",
        "[Esc] Cancel",
    ] {
        assert!(text.contains(want), "missing {want:?}");
    }
    assert!(!text.contains("[Enter] Delete"));

    type_str(&mut app, "on");
    assert!(screen_text(&app).contains("[Enter] Delete   [Esc] Cancel"));

    press(&mut app, KeyCode::Enter);
    let sending = screen_text(&app);
    assert!(sending.contains("deleting…   [Esc] Close"), "{sending}");
    assert!(!sending.contains("[Enter] Delete"));
}

// --- guests ---------------------------------------------------------------------

#[test]
fn a_guest_sees_the_sign_in_hint_on_the_profile_and_safety_tabs() {
    for key in ["1", "3"] {
        let app = on_tab(guest_settings(120, 40), key);
        let got = body_of(&app, 6);
        assert_eq!(
            got[0], " Sign in to change these settings. [L] Sign in",
            "tab {key}"
        );
        assert!(got[1..].iter().all(String::is_empty), "tab {key}: {got:?}");
    }
}

#[test]
fn a_guest_still_sees_the_appearance_rows() {
    let app = on_tab(guest_settings(120, 40), "2");
    assert_eq!(
        body_of(&app, 5),
        vec![
            right_aligned("▌Theme", "dark  (not applied yet)", W - 1),
            right_aligned(" Collapse nav", "[ ] off  (not applied yet)", W - 1),
            right_aligned(" Lane dimming", "[ ] off  (not applied yet)", W - 1),
            right_aligned(" Reply previews", "3", W - 1),
            right_aligned(" ASCII mode", "[ ] off", W - 1),
        ]
    );
}

// --- snapshots ------------------------------------------------------------------

#[test]
fn st1_settings_profile_focus_at_120x40() {
    snapshot!(
        "settings_profile_focus",
        lane_app(Some(Lane::Index(0)), 120, 40, "jll")
    );
}

#[test]
fn st2_settings_lane_custom_at_80x24() {
    snapshot!(
        "settings_lane_custom",
        hex_app(Some(Lane::Index(1)), 80, 24, "3a7b")
    );
}

#[test]
fn st3_delete_account_confirm_at_120x40() {
    let mut app = on_tab(settings_app(120, 40), "D");
    type_str(&mut app, "myur");
    snapshot!("delete_account_confirm", app);
}

#[test]
fn st4_delete_account_ready_at_120x40() {
    let mut app = on_tab(settings_app(120, 40), "D");
    type_str(&mut app, "myuron");
    snapshot!("delete_account_ready", app);
}

#[test]
fn sf1_settings_safety_scrolled_at_80x24() {
    let mut app = safety_app(
        &[
            "amber", "bella", "chloe", "diego", "elena", "felix", "grace", "hiram", "irene",
            "jonas", "karin", "lucas",
        ],
        &[
            "nadia", "oscar", "petra", "quinn", "ravi", "sofia", "tomas", "ursula",
        ],
        80,
        24,
    );
    type_str(&mut app, "G");
    snapshot!("settings_safety_scrolled", app);
}

#[test]
fn sg1_settings_guest_profile_at_80x24() {
    snapshot!("settings_guest_profile", guest_settings(80, 24));
}

// --- the answers reach the other places that show the account ---------------------------------------

#[test]
fn the_lane_answer_recolors_the_dot_of_the_aside() {
    let mut app = settings_app(120, 40);
    let dot_color = |app: &App| {
        let terminal = render(app);
        let buffer = terminal.backend().buffer();
        // The aside starts at column 88; its second row reads " ● @myuron".
        buffer[(90, 2)].fg
    };
    assert_eq!(
        dot_color(&app),
        theme::lane_color(Lane::Index(1), true),
        "the dot is drawn in the lane color"
    );
    type_str(&mut app, "jllll");
    let (lane, req) = match press(&mut app, KeyCode::Enter).as_slice() {
        [crate::action::Effect::SetLane { lane, req }] => (*lane, *req),
        other => panic!("no lane write: {other:?}"),
    };
    assert_eq!(lane, crate::model::LaneSetting::Palette(3));
    let response = serde_json::from_value(crate::notifications_tests::me_json(
        0,
        serde_json::json!({"lane": 3, "preferred_lane": 3}),
    ))
    .unwrap();
    app.update(crate::action::Action::MeWriteDone {
        kind: crate::action::MeWrite::Lane,
        req,
        response: Box::new(response),
    });
    assert_eq!(dot_color(&app), theme::lane_color(Lane::Index(3), true));
}

#[test]
fn the_pasteis_answer_removes_the_pastel_count_from_the_action_row() {
    let mut app =
        crate::write_tests::log_app_of(fixture_me(), vec![crate::write_tests::card("c0", "alice")]);
    assert!(screen_text(&app).contains("◓ 0"), "the count is shown");
    type_str(&mut app, "gsjj");
    let req = match press(&mut app, KeyCode::Enter).as_slice() {
        [crate::action::Effect::SetPasteisMode { req, .. }] => *req,
        other => panic!("no mode write: {other:?}"),
    };
    let response = serde_json::from_value(crate::notifications_tests::me_json(
        0,
        serde_json::json!({"pasteis_mode": false}),
    ))
    .unwrap();
    app.update(crate::action::Action::MeWriteDone {
        kind: crate::action::MeWrite::PasteisMode,
        req,
        response: Box::new(response),
    });
    type_str(&mut app, "gl");
    assert!(!screen_text(&app).contains("◓ 0"), "the count is gone");
}

#[test]
fn the_last_person_released_leaves_the_empty_text_under_the_heading() {
    let mut app = safety_app(&["amber"], &[], 120, 40);
    let effects = type_str(&mut app, "u");
    let (name, req) = match effects.as_slice() {
        [crate::action::Effect::MuteUser { username, req, .. }] => (username.clone(), *req),
        other => panic!("no unmute: {other:?}"),
    };
    app.update(crate::action::Action::MuteDone {
        username: name.clone(),
        on: false,
        req,
        response: Box::new(crate::model::MuteResponse {
            data: person(&name),
            is_muting: false,
        }),
    });
    assert_eq!(body_of(&app, 2), vec![" Muted", "  No muted users."]);
}

#[test]
fn st5_settings_appearance_focus_at_80x24() {
    let mut app = settings_app_for(me_with(None), 80, 24);
    type_str(&mut app, "2jjjl");
    snapshot!("settings_appearance_focus", app);
}
