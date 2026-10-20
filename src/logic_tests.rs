use crossterm::event::{KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::Rect;

use crate::action::Action;
use crate::app::{LogTab, Modal, NavItem, Pane, Screen};
use crate::test_support::*;
use crate::ui::layout::{LayoutMode, split};
use crate::ui::nav::{badge, narrow_notifs_label};
use crate::ui::statusline::{breadcrumb, left, right};

const FOLLOWING: Screen = Screen::Log(LogTab::Following);
const LATEST: Screen = Screen::Log(LogTab::Latest);
const TRENDING: Screen = Screen::Log(LogTab::Trending);

// 1. quit

#[test]
fn t01_shift_q_quits_with_or_without_shift_modifier() {
    for modifiers in [KeyModifiers::NONE, KeyModifiers::SHIFT] {
        let mut app = signed_in(120, 40);
        let effects = send(
            &mut app,
            key_event(KeyCode::Char('Q'), modifiers, KeyEventKind::Press),
        );
        assert!(quit_requested(&effects), "modifiers: {modifiers:?}");
    }
}

#[test]
fn t01_ctrl_c_quits() {
    let mut app = signed_in(120, 40);
    assert!(quit_requested(&ctrl(&mut app, 'c')));
}

#[test]
fn t01_lowercase_q_plain_c_and_root_esc_do_not_quit() {
    let mut app = signed_in(120, 40);
    assert!(!quit_requested(&type_str(&mut app, "q")));
    assert!(!quit_requested(&type_str(&mut app, "c")));
    assert!(!quit_requested(&press(&mut app, KeyCode::Esc)));
}

#[test]
fn t01_release_and_repeat_of_shift_q_are_ignored() {
    for kind in [KeyEventKind::Release, KeyEventKind::Repeat] {
        let mut app = signed_in(120, 40);
        let effects = send(
            &mut app,
            key_event(KeyCode::Char('Q'), KeyModifiers::SHIFT, kind),
        );
        assert!(!quit_requested(&effects), "kind: {kind:?}");
    }
}

// 2. g moves when signed in

#[test]
fn t02_g_prefix_opens_each_screen_when_signed_in() {
    let cases = [
        ('n', Screen::Notifications),
        ('v', Screen::Saved),
        ('s', Screen::Settings),
        ('p', Screen::Profile),
        ('l', FOLLOWING),
        ('e', TRENDING),
    ];
    for (second, expected) in cases {
        let mut app = signed_in(120, 40);
        // Start away from Log so that `g l` is observable.
        type_str(&mut app, "gs");
        type_str(&mut app, "g");
        assert!(app.g_pending(), "after g");
        type_str(&mut app, &second.to_string());
        assert_eq!(app.screen(), expected, "g {second}");
        assert!(!app.g_pending(), "after g {second}");
    }
}

// 3. g prefix cancel

#[test]
fn t03_unknown_key_after_g_cancels_and_next_key_is_plain() {
    let mut app = signed_in(120, 40);
    type_str(&mut app, "g");
    assert!(app.g_pending());
    type_str(&mut app, "x");
    assert_eq!(app.screen(), FOLLOWING);
    assert!(!app.g_pending());
    type_str(&mut app, "n");
    assert_eq!(app.screen(), FOLLOWING);
}

#[test]
fn t03_esc_after_g_cancels_prefix_without_going_back() {
    let mut app = signed_in(120, 40);
    app.update(Action::Open(Screen::Notifications));
    type_str(&mut app, "g");
    press(&mut app, KeyCode::Esc);
    assert!(!app.g_pending());
    assert_eq!(app.screen(), Screen::Notifications);
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.screen(), FOLLOWING);
}

#[test]
fn t03_gg_keeps_screen_and_cancels_prefix() {
    let mut app = signed_in(120, 40);
    app.update(Action::Open(Screen::Notifications));
    type_str(&mut app, "gg");
    assert_eq!(app.screen(), Screen::Notifications);
    assert!(!app.g_pending());
}

// 4. history

#[test]
fn t04_esc_returns_to_previous_screen_after_open() {
    let mut app = signed_in(120, 40);
    app.update(Action::Open(Screen::Notifications));
    assert_eq!(app.screen(), Screen::Notifications);
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.screen(), FOLLOWING);
}

#[test]
fn t04_esc_at_root_changes_nothing() {
    let mut app = signed_in(120, 40);
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.screen(), FOLLOWING);
}

#[test]
fn t04_g_move_after_open_clears_history() {
    let mut app = signed_in(120, 40);
    app.update(Action::Open(Screen::Settings));
    type_str(&mut app, "ge");
    assert_eq!(app.screen(), TRENDING);
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.screen(), TRENDING);
}

#[test]
fn t04_nav_enter_after_open_clears_history() {
    let mut app = signed_in(120, 40);
    app.update(Action::Open(Screen::Settings));
    press(&mut app, KeyCode::Tab);
    type_str(&mut app, "k");
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.screen(), Screen::Saved);
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.screen(), Screen::Saved);
}

// 5. state restore

#[test]
fn t05_back_restores_log_tab_of_origin_screen() {
    let mut app = signed_in(120, 40);
    type_str(&mut app, "2");
    assert_eq!(app.screen(), LATEST);
    app.update(Action::Open(TRENDING));
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.screen(), LATEST);
}

// 6. focus

#[test]
fn t06_tab_and_backtab_toggle_focus_in_every_layout_mode() {
    for (w, h) in [(120, 40), (100, 30), (60, 24)] {
        let mut app = signed_in(w, h);
        assert_eq!(app.focus(), Pane::Main, "initial at {w}x{h}");
        press(&mut app, KeyCode::Tab);
        assert_eq!(app.focus(), Pane::Nav, "Tab at {w}x{h}");
        press(&mut app, KeyCode::Tab);
        assert_eq!(app.focus(), Pane::Main, "Tab again at {w}x{h}");
        press(&mut app, KeyCode::BackTab);
        assert_eq!(app.focus(), Pane::Nav, "BackTab at {w}x{h}");
        press(&mut app, KeyCode::BackTab);
        assert_eq!(app.focus(), Pane::Main, "BackTab again at {w}x{h}");
    }
}

#[test]
fn t06_resize_keeps_focus_and_nav_cursor() {
    let mut app = signed_in(120, 40);
    press(&mut app, KeyCode::Tab);
    type_str(&mut app, "jj");
    assert_eq!(app.nav_cursor(), NavItem::Profile);
    for (w, h) in [(60, 24), (100, 30), (120, 40)] {
        app.update(Action::Resize(w, h));
        assert_eq!(app.focus(), Pane::Nav, "focus at {w}x{h}");
        assert_eq!(app.nav_cursor(), NavItem::Profile, "cursor at {w}x{h}");
    }
}

// 6b. nav cursor

#[test]
fn t06b_cursor_is_on_notifications_after_g_n_then_tab() {
    let mut app = signed_in(120, 40);
    type_str(&mut app, "gn");
    press(&mut app, KeyCode::Tab);
    assert_eq!(app.focus(), Pane::Nav);
    assert_eq!(app.nav_cursor(), NavItem::Notifications);
}

#[test]
fn t06b_cursor_returns_to_current_screen_when_nav_regains_focus() {
    let mut app = signed_in(120, 40);
    press(&mut app, KeyCode::Tab);
    type_str(&mut app, "jj");
    assert_eq!(app.nav_cursor(), NavItem::Profile);
    press(&mut app, KeyCode::Tab);
    press(&mut app, KeyCode::Tab);
    assert_eq!(app.focus(), Pane::Nav);
    assert_eq!(app.nav_cursor(), NavItem::Log);
}

#[test]
fn t06b_cursor_follows_current_screen_across_open_and_esc() {
    let mut app = signed_in(120, 40);
    press(&mut app, KeyCode::Tab);
    type_str(&mut app, "jj");
    app.update(Action::Open(Screen::Notifications));
    assert_eq!(app.nav_cursor(), NavItem::Notifications);
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.nav_cursor(), NavItem::Log);
}

// 6c. priority

#[test]
fn t06c_quit_keys_work_while_g_is_pending() {
    let mut app = signed_in(120, 40);
    type_str(&mut app, "g");
    assert!(quit_requested(&ctrl(&mut app, 'c')));
    let mut app = signed_in(120, 40);
    type_str(&mut app, "g");
    assert!(quit_requested(&type_str(&mut app, "Q")));
}

#[test]
fn t06c_question_mark_after_g_cancels_without_opening_help() {
    let mut app = signed_in(120, 40);
    type_str(&mut app, "g");
    assert!(app.g_pending());
    type_str(&mut app, "?");
    assert_eq!(app.modal(), None);
    assert!(!app.g_pending());
}

#[test]
fn t06c_narrow_nav_focus_g_l_opens_log_instead_of_moving_right() {
    let mut app = signed_in(60, 24);
    type_str(&mut app, "gn");
    press(&mut app, KeyCode::Tab);
    assert_eq!(app.nav_cursor(), NavItem::Notifications);
    type_str(&mut app, "gl");
    assert_eq!(app.screen(), FOLLOWING);
    assert_eq!(app.focus(), Pane::Nav);
    assert_eq!(app.nav_cursor(), NavItem::Log);
}

// 7. nav operation

#[test]
fn t07_j_and_k_move_and_clamp_at_both_ends_for_signed_in_and_guest() {
    let all = NavItem::ALL;
    let mut apps = vec![signed_in(120, 40), {
        let mut g = guest(120, 40);
        press(&mut g, KeyCode::Esc);
        g
    }];
    for app in &mut apps {
        press(app, KeyCode::Tab);
        type_str(app, "k");
        assert_eq!(app.nav_cursor(), all[0]);
        for expected in &all[1..] {
            type_str(app, "j");
            assert_eq!(app.nav_cursor(), *expected);
        }
        type_str(app, "j");
        assert_eq!(app.nav_cursor(), NavItem::Settings);
        type_str(app, "k");
        assert_eq!(app.nav_cursor(), NavItem::Saved);
    }
}

#[test]
fn t07_h_and_l_move_only_in_narrow_mode() {
    let mut app = signed_in(60, 24);
    press(&mut app, KeyCode::Tab);
    type_str(&mut app, "l");
    assert_eq!(app.nav_cursor(), NavItem::Notifications);
    type_str(&mut app, "h");
    assert_eq!(app.nav_cursor(), NavItem::Log);
    type_str(&mut app, "h");
    assert_eq!(app.nav_cursor(), NavItem::Log);

    let mut wide = signed_in(120, 40);
    press(&mut wide, KeyCode::Tab);
    type_str(&mut wide, "l");
    assert_eq!(wide.nav_cursor(), NavItem::Log);
    type_str(&mut wide, "j");
    type_str(&mut wide, "h");
    assert_eq!(wide.nav_cursor(), NavItem::Notifications);
}

#[test]
fn t07_enter_opens_cursor_screen_and_moves_focus_to_main() {
    let mut app = signed_in(120, 40);
    press(&mut app, KeyCode::Tab);
    type_str(&mut app, "j");
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.screen(), Screen::Notifications);
    assert_eq!(app.focus(), Pane::Main);
}

// 8. help

#[test]
fn t08_question_mark_opens_help_and_esc_or_question_mark_closes_it() {
    let mut app = signed_in(120, 40);
    type_str(&mut app, "?");
    assert_eq!(app.modal(), Some(Modal::Help));
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.modal(), None);
    type_str(&mut app, "?");
    type_str(&mut app, "?");
    assert_eq!(app.modal(), None);
}

#[test]
fn t08_g_n_is_ignored_while_help_is_open() {
    let mut app = signed_in(120, 40);
    type_str(&mut app, "?gn");
    assert_eq!(app.modal(), Some(Modal::Help));
    assert_eq!(app.screen(), FOLLOWING);
}

#[test]
fn t08_quit_keys_work_while_help_is_open() {
    let mut app = signed_in(120, 40);
    type_str(&mut app, "?");
    assert!(quit_requested(&type_str(&mut app, "Q")));
    let mut app = signed_in(120, 40);
    type_str(&mut app, "?");
    assert!(quit_requested(&ctrl(&mut app, 'c')));
}

// 9. sign-in dialog

#[test]
fn t09_q_question_mark_and_g_n_do_nothing_inside_sign_in_dialog() {
    let mut app = guest(120, 40);
    assert_eq!(app.modal(), Some(Modal::SignIn));
    assert!(!quit_requested(&type_str(&mut app, "Q")));
    type_str(&mut app, "?");
    type_str(&mut app, "gn");
    assert_eq!(app.modal(), Some(Modal::SignIn));
    assert_eq!(app.screen(), LATEST);
}

#[test]
fn t09_ctrl_c_quits_and_esc_closes_sign_in_dialog() {
    let mut app = guest(120, 40);
    assert!(quit_requested(&ctrl(&mut app, 'c')));
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.modal(), None);
}

// 10. tabs

#[test]
fn t10_number_keys_switch_log_tab_when_main_has_focus() {
    let mut app = signed_in(120, 40);
    type_str(&mut app, "2");
    assert_eq!(app.screen(), LATEST);
    type_str(&mut app, "3");
    assert_eq!(app.screen(), TRENDING);
    type_str(&mut app, "1");
    assert_eq!(app.screen(), FOLLOWING);
}

#[test]
fn t10_number_keys_do_nothing_when_nav_has_focus() {
    let mut app = signed_in(120, 40);
    press(&mut app, KeyCode::Tab);
    assert_eq!(app.focus(), Pane::Nav);
    type_str(&mut app, "3");
    assert_eq!(app.screen(), FOLLOWING);
}

// 11. layout mode

#[test]
fn t11_layout_mode_is_decided_by_width_and_height() {
    let cases = [
        ((120, 20), LayoutMode::Wide),
        ((119, 20), LayoutMode::Medium),
        ((80, 20), LayoutMode::Medium),
        ((79, 20), LayoutMode::Narrow),
        ((120, 19), LayoutMode::TooSmall),
        ((60, 19), LayoutMode::TooSmall),
    ];
    for ((w, h), expected) in cases {
        assert_eq!(LayoutMode::from_size(w, h), expected, "{w}x{h}");
        assert_eq!(signed_in(w, h).layout_mode(), expected, "app {w}x{h}");
    }
}

// 12. layout split

#[test]
fn t12_wide_split_has_nav_20_aside_32_and_main_the_rest() {
    let panes = split(Rect::new(0, 0, 120, 40), LayoutMode::Wide);
    assert_eq!(panes.nav, Rect::new(0, 0, 20, 39));
    assert_eq!(panes.main, Rect::new(20, 0, 68, 39));
    assert_eq!(panes.aside, Some(Rect::new(88, 0, 32, 39)));
    assert_eq!(panes.status, Rect::new(0, 39, 120, 1));
}

#[test]
fn t12_medium_split_has_nav_4_and_no_aside() {
    let panes = split(Rect::new(0, 0, 80, 24), LayoutMode::Medium);
    assert_eq!(panes.nav, Rect::new(0, 0, 4, 23));
    assert_eq!(panes.main, Rect::new(4, 0, 76, 23));
    assert_eq!(panes.aside, None);
    assert_eq!(panes.status, Rect::new(0, 23, 80, 1));
}

#[test]
fn t12_narrow_split_has_3_row_navbar_on_top() {
    let panes = split(Rect::new(0, 0, 60, 24), LayoutMode::Narrow);
    assert_eq!(panes.nav, Rect::new(0, 0, 60, 3));
    assert_eq!(panes.main, Rect::new(0, 3, 60, 20));
    assert_eq!(panes.aside, None);
    assert_eq!(panes.status, Rect::new(0, 23, 60, 1));
}

// 13. badge

#[test]
fn t13_badge_hides_zero_and_caps_at_99_plus() {
    let cases = [
        (0, None),
        (1, Some("1")),
        (12, Some("12")),
        (99, Some("99")),
        (100, Some("99+")),
        (1000, Some("99+")),
    ];
    for (count, expected) in cases {
        assert_eq!(badge(count).as_deref(), expected, "count {count}");
    }
}

#[test]
fn t13_narrow_label_appends_badge_in_parentheses() {
    assert_eq!(narrow_notifs_label(12), "Notifs(12)");
    assert_eq!(narrow_notifs_label(100), "Notifs(99+)");
    assert_eq!(narrow_notifs_label(0), "Notifs");
}

// 14. statusline strings

#[test]
fn t14_statusline_left_and_right_per_layout_mode() {
    let bc = "Log > Following";
    assert_eq!(
        left(LayoutMode::Wide, bc),
        "Log > Following  j/k move  ↵ open  c commit  Tab pane  ? help"
    );
    assert_eq!(right(LayoutMode::Wide), "API -/-  ○ connecting");
    assert_eq!(
        left(LayoutMode::Medium, bc),
        "Log > Following  j/k move  ↵ open  c commit  ? help"
    );
    assert_eq!(right(LayoutMode::Medium), "-/-");
    assert_eq!(left(LayoutMode::Narrow, bc), "j/k ↵ c ?");
    assert_eq!(right(LayoutMode::Narrow), "-/-");
}

#[test]
fn t14_breadcrumb_follows_screen_and_session() {
    let mut app = signed_in(120, 40);
    assert_eq!(breadcrumb(&app), "Log > Following");
    type_str(&mut app, "gn");
    assert_eq!(breadcrumb(&app), "Notifications");
    type_str(&mut app, "gs");
    assert_eq!(breadcrumb(&app), "Settings");
    let guest_app = guest(120, 40);
    assert_eq!(breadcrumb(&guest_app), "Log > Latest");
}

// 15. guest

fn guest_without_dialog(w: u16, h: u16) -> crate::app::App {
    let mut app = guest(w, h);
    press(&mut app, KeyCode::Esc);
    app
}

#[test]
fn t15_guest_starts_with_dialog_over_latest_and_esc_closes_it() {
    let mut app = guest(120, 40);
    assert_eq!(app.modal(), Some(Modal::SignIn));
    assert_eq!(app.screen(), LATEST);
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.modal(), None);
    assert_eq!(app.screen(), LATEST);
}

#[test]
fn t15_guest_g_moves_needing_sign_in_open_dialog_and_keep_screen() {
    for second in ['n', 'p', 'v', 's'] {
        let mut app = guest_without_dialog(120, 40);
        type_str(&mut app, &format!("g{second}"));
        assert_eq!(app.modal(), Some(Modal::SignIn), "g {second}");
        assert_eq!(app.screen(), LATEST, "g {second}");
    }
}

#[test]
fn t15_guest_nav_enter_on_protected_items_opens_dialog_and_keeps_focus_on_nav() {
    for presses in 1..=4 {
        let mut app = guest_without_dialog(120, 40);
        press(&mut app, KeyCode::Tab);
        type_str(&mut app, &"j".repeat(presses));
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.modal(), Some(Modal::SignIn), "j x{presses}");
        assert_eq!(app.screen(), LATEST, "j x{presses}");
        assert_eq!(app.focus(), Pane::Nav, "j x{presses}");
    }
}

#[test]
fn t15_guest_following_tab_and_commit_open_dialog_without_changing_tab() {
    let mut app = guest_without_dialog(120, 40);
    type_str(&mut app, "1");
    assert_eq!(app.modal(), Some(Modal::SignIn));
    assert_eq!(app.screen(), LATEST);
    let mut app = guest_without_dialog(120, 40);
    type_str(&mut app, "c");
    assert_eq!(app.modal(), Some(Modal::SignIn));
    assert_eq!(app.screen(), LATEST);
}

#[test]
fn t15_guest_can_open_latest_and_trending_and_the_dialog_by_key() {
    let mut app = guest_without_dialog(120, 40);
    type_str(&mut app, "ge");
    assert_eq!(app.screen(), TRENDING);
    type_str(&mut app, "gl");
    assert_eq!(app.screen(), LATEST);
    type_str(&mut app, "3");
    assert_eq!(app.screen(), TRENDING);
    type_str(&mut app, "2");
    assert_eq!(app.screen(), LATEST);
    assert_eq!(app.modal(), None);
    type_str(&mut app, "L");
    assert_eq!(app.modal(), Some(Modal::SignIn));
}

#[test]
fn t15_medium_guest_nav_shows_l_instead_of_plus() {
    let app = guest_without_dialog(100, 30);
    let terminal = render(&app);
    let buffer = terminal.backend().buffer();
    // Nav inner column 1..=2; the button row is the 8th inner row.
    assert_eq!(buffer[(2, 8)].symbol(), "L");
}

#[test]
fn t15_narrow_guest_navbar_shows_sign_in_hint() {
    let app = guest_without_dialog(60, 24);
    let terminal = render(&app);
    let buffer = terminal.backend().buffer();
    assert!(row_text(buffer, 1).contains("[L] Sign in"));
}

// 16. signed-in start

#[test]
fn t16_signed_in_starts_on_following_without_modal_and_l_does_nothing() {
    let mut app = signed_in(120, 40);
    assert_eq!(app.screen(), FOLLOWING);
    assert_eq!(app.modal(), None);
    type_str(&mut app, "L");
    assert_eq!(app.modal(), None);
}

// 17. tiny sizes

#[test]
fn t17_draw_does_not_panic_at_tiny_sizes() {
    for (w, h) in [(0, 0), (1, 1), (10, 20), (60, 19)] {
        let _ = render(&signed_in(w, h));
        let _ = render(&guest(w, h));
        let mut help = signed_in(w, h);
        type_str(&mut help, "?");
        let _ = render(&help);
    }
}

#[test]
fn t17_keys_are_handled_normally_when_terminal_is_too_small() {
    let mut app = signed_in(60, 19);
    type_str(&mut app, "gn");
    assert_eq!(app.screen(), Screen::Notifications);
    assert!(quit_requested(&type_str(&mut app, "Q")));
}
