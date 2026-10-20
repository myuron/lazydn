use crossterm::event::KeyCode;

use crate::app::Screen;
use crate::test_support::*;
use crate::ui::theme::PRIMARY;

macro_rules! snapshot {
    ($name:literal, $app:expr) => {{
        let terminal = render(&$app);
        insta::assert_snapshot!($name, terminal.backend());
    }};
}

#[test]
fn s01_wide_log_signed_in_with_main_focused() {
    snapshot!("wide_log_signed_in", signed_in(120, 40));
}

#[test]
fn s02_medium_log_shows_icon_nav_and_short_statusline() {
    snapshot!("medium_log", signed_in(80, 24));
}

#[test]
fn s03_narrow_log_shows_top_navbar_with_notifs_badge() {
    snapshot!("narrow_log", signed_in(60, 24));
}

#[test]
fn s04_too_small_shows_only_the_height_message() {
    snapshot!("too_small", signed_in(100, 19));
}

#[test]
fn s05_help_modal_with_double_border_over_log() {
    let mut app = signed_in(120, 40);
    type_str(&mut app, "?");
    snapshot!("help_modal", app);
}

#[test]
fn s06_nav_focused_draws_nav_thick_and_main_thin() {
    let mut app = signed_in(120, 40);
    press(&mut app, KeyCode::Tab);
    snapshot!("nav_focused", app);
}

#[test]
fn s07_guest_start_shows_sign_in_dialog_in_unfilled_state() {
    snapshot!("guest_signin_dialog", guest(120, 40));
}

#[test]
fn s08_guest_log_after_closing_dialog_prompts_to_sign_in() {
    let mut app = guest(120, 40);
    press(&mut app, KeyCode::Esc);
    snapshot!("guest_log", app);
}

#[test]
fn s09_notifications_placeholder_in_wide_layout() {
    let mut app = signed_in(120, 40);
    type_str(&mut app, "gn");
    assert_eq!(app.screen(), Screen::Notifications);
    snapshot!("notifications_placeholder", app);
}

#[test]
fn s10_narrow_notifications_marks_notifs_in_navbar() {
    let mut app = signed_in(60, 24);
    type_str(&mut app, "gn");
    snapshot!("narrow_notifications", app);
}

// Style assertions: Wide 120x40, Nav inner area starts at (1, 1); Log is its 4th row.

#[test]
fn style_current_nav_item_marker_and_label_use_primary_color() {
    let terminal = render(&signed_in(120, 40));
    let buffer = terminal.backend().buffer();
    assert_eq!(buffer[(1, 4)].symbol(), "▌");
    assert_eq!(buffer[(1, 4)].fg, PRIMARY);
    assert_eq!(buffer[(4, 4)].symbol(), "L");
    assert_eq!(buffer[(4, 4)].fg, PRIMARY);
    assert_ne!(buffer[(4, 5)].fg, PRIMARY);
}

#[test]
fn style_nav_cursor_row_has_distinct_background_only_while_nav_is_focused() {
    let unfocused = render(&signed_in(120, 40));
    let buffer = unfocused.backend().buffer();
    assert_eq!(buffer[(6, 4)].bg, buffer[(6, 5)].bg);

    let mut app = signed_in(120, 40);
    press(&mut app, KeyCode::Tab);
    let focused = render(&app);
    let buffer = focused.backend().buffer();
    assert_ne!(buffer[(6, 4)].bg, buffer[(6, 5)].bg);

    type_str(&mut app, "j");
    let moved = render(&app);
    let buffer = moved.backend().buffer();
    // Row 6 is the unread badge line, so Profile is at row 7.
    assert_ne!(buffer[(6, 5)].bg, buffer[(6, 7)].bg);
    assert_eq!(buffer[(6, 4)].bg, buffer[(6, 7)].bg);
}

#[test]
fn style_narrow_navbar_current_marker_uses_primary_color() {
    let terminal = render(&signed_in(60, 24));
    let buffer = terminal.backend().buffer();
    assert_eq!(buffer[(1, 1)].symbol(), "▌");
    assert_eq!(buffer[(1, 1)].fg, PRIMARY);
}
