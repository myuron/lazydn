use crate::action::{Action, Failure};
use crate::api::client::RateLimit;
use crate::app::{App, ConnState};
use crate::feed_tests::{FOLLOWING, deliver, first_req, started};
use crate::layout::LayoutMode;
use crate::test_support::*;
use crate::toast_tests::log_page;
use crate::ui::statusline::right_with;

macro_rules! snapshot {
    ($name:literal, $app:expr) => {{
        let terminal = render(&$app);
        insta::assert_snapshot!($name, terminal.backend());
    }};
}

const RATE: Option<RateLimit> = Some(RateLimit {
    limit: 120,
    remaining: 118,
});

fn offline(w: u16, h: u16) -> App {
    let mut app = signed_in(w, h);
    app.update(Action::ApiStatus {
        rate: None,
        online: false,
    });
    app
}

/// A Following list with items whose refresh just failed with a rate limit.
fn rate_limited(w: u16, h: u16) -> App {
    let (mut app, effects) = started(w, h);
    deliver(&mut app, FOLLOWING, &effects, log_page());
    let refresh = type_str(&mut app, "R");
    app.update(Action::FeedFailed {
        key: FOLLOWING,
        req: first_req(&refresh),
        failure: Failure {
            code: "rate_limited".into(),
            retry_after: Some(12),
            message: None,
        },
    });
    app
}

fn last_row(app: &App) -> String {
    let terminal = render(app);
    let buffer = terminal.backend().buffer();
    row_text(buffer, buffer.area.height - 1)
}

// --- 6. the right end of the status line, per layout mode ---------------------------

#[test]
fn wide_shows_the_remaining_calls_and_the_connection_state() {
    let wide = |rate, conn| right_with(LayoutMode::Wide, rate, conn, None);
    assert_eq!(wide(None, ConnState::Connecting), "API -/-  ○ connecting");
    assert_eq!(wide(RATE, ConnState::Online), "API 118/120  ● online");
    assert_eq!(wide(None, ConnState::Online), "API -/-  ● online");
    assert_eq!(wide(None, ConnState::Offline), "API -/-  ○ offline");
    assert_eq!(wide(RATE, ConnState::Offline), "API 118/120  ○ offline");
}

#[test]
fn medium_and_narrow_show_the_counts_and_mark_only_offline() {
    for mode in [LayoutMode::Medium, LayoutMode::Narrow] {
        let right = |rate, conn| right_with(mode, rate, conn, None);
        assert_eq!(right(None, ConnState::Connecting), "-/-", "{mode:?}");
        assert_eq!(right(RATE, ConnState::Online), "118/120", "{mode:?}");
        assert_eq!(right(None, ConnState::Offline), "○ -/-", "{mode:?}");
        assert_eq!(right(RATE, ConnState::Offline), "○ 118/120", "{mode:?}");
    }
}

#[test]
fn a_non_default_host_is_appended_at_the_right_end() {
    let host = Some("127.0.0.1:8080");
    assert_eq!(
        right_with(LayoutMode::Wide, RATE, ConnState::Online, host),
        "API 118/120  ● online  127.0.0.1:8080"
    );
    assert_eq!(
        right_with(LayoutMode::Medium, None, ConnState::Connecting, host),
        "-/-  127.0.0.1:8080"
    );
    assert_eq!(
        right_with(LayoutMode::Narrow, None, ConnState::Offline, host),
        "○ -/-  127.0.0.1:8080"
    );
}

#[test]
fn the_status_line_of_a_too_small_terminal_stays_empty() {
    assert_eq!(
        right_with(LayoutMode::TooSmall, RATE, ConnState::Offline, Some("h")),
        ""
    );
}

#[test]
fn the_drawn_status_line_follows_the_app_state() {
    let mut app = signed_in(120, 40);
    assert!(last_row(&app).ends_with("API -/-  ○ connecting "));

    app.update(Action::ApiStatus {
        rate: RATE,
        online: true,
    });
    assert!(last_row(&app).ends_with("API 118/120  ● online "));

    app.update(Action::ApiStatus {
        rate: None,
        online: false,
    });
    assert!(last_row(&app).ends_with("API 118/120  ○ offline "));
}

#[test]
fn the_drawn_status_line_shows_the_host_only_when_one_is_set() {
    let mut app = signed_in(120, 40);
    app.update(Action::ApiStatus {
        rate: RATE,
        online: true,
    });
    app.set_host_label(Some("127.0.0.1:8080".into()));
    assert!(last_row(&app).ends_with("API 118/120  ● online  127.0.0.1:8080 "));
}

// --- 8. snapshots ----------------------------------------------------------------

#[test]
fn snapshot_offline_status_line_wide() {
    snapshot!("status_offline_wide", offline(120, 40));
}

#[test]
fn snapshot_offline_status_line_medium() {
    snapshot!("status_offline_medium", offline(80, 24));
}

#[test]
fn snapshot_offline_status_line_narrow() {
    snapshot!("status_offline_narrow", offline(60, 24));
}

#[test]
fn snapshot_toast_over_the_wide_log() {
    snapshot!("toast_wide", rate_limited(120, 40));
}

#[test]
fn snapshot_toast_over_the_narrow_log() {
    snapshot!("toast_narrow", rate_limited(60, 24));
}

// --- 4. the toast box -----------------------------------------------------------------

#[test]
fn the_toast_sits_right_above_the_status_line_with_a_one_column_margin() {
    let app = rate_limited(120, 40);
    let terminal = render(&app);
    let buffer = terminal.backend().buffer();
    let box_width = "✗ rate_limited: retry in 12s".chars().count() + 4;
    let margin = " ";
    let top: String = row_text(buffer, 36).chars().skip(119 - box_width).collect();
    assert_eq!(top, format!("┌{}┐{margin}", "─".repeat(box_width - 2)));
    assert!(row_text(buffer, 37).contains("│ ✗ rate_limited: retry in 12s │"));
    assert!(row_text(buffer, 38).contains(&format!("└{}┘", "─".repeat(box_width - 2))));
}

#[test]
fn a_long_toast_is_limited_to_the_screen_width_minus_two() {
    let (mut app, effects) = started(60, 24);
    deliver(&mut app, FOLLOWING, &effects, log_page());
    let refresh = type_str(&mut app, "R");
    app.update(Action::FeedFailed {
        key: FOLLOWING,
        req: first_req(&refresh),
        failure: Failure::new("x".repeat(100)),
    });
    let terminal = render(&app);
    let buffer = terminal.backend().buffer();
    // Columns 0 and 59 belong to the Main pane's border, so only the box itself is compared.
    let row: String = row_text(buffer, 20).chars().skip(1).take(58).collect();
    assert_eq!(row, format!("┌{}┐", "─".repeat(56)));
}

#[test]
fn no_toast_box_is_drawn_without_a_toast() {
    let app = signed_in(120, 40);
    let terminal = render(&app);
    let buffer = terminal.backend().buffer();
    for y in 36..39 {
        assert!(!row_text(buffer, y).contains('✗'), "row {y}");
    }
}

#[test]
fn the_cross_of_an_error_toast_is_red() {
    let app = rate_limited(120, 40);
    let terminal = render(&app);
    let cell = &terminal.backend().buffer()[(89, 37)];
    assert_eq!(cell.symbol(), "✗");
    assert_eq!(cell.fg, ratatui::style::Color::Red);
}
