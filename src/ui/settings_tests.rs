use ratatui::layout::Rect;
use time::Duration;
use time::macros::datetime;
use unicode_width::UnicodeWidthStr;

use crate::action::{Action, Failure};
use crate::app::{App, Me, Prefs, Theme};
use crate::model::{Lane, Rgb};
use crate::profile_tests::user_of;
use crate::settings_tests::*;
use crate::test_support::*;
use crate::thread_tests::{merge, user_json};
use crate::ui::layout::split;
use crate::ui::theme;

/// Inner width of Main at 120 columns (nav 20 + aside 32 + 2 borders).
const W: usize = 66;

// --- helpers ----------------------------------------------------------------

fn inner_rect(app: &App) -> Rect {
    let (w, h) = app.size();
    let main = split(Rect::new(0, 0, w, h), app.layout_mode()).main;
    Rect::new(
        main.x + 1,
        main.y + 1,
        main.width.saturating_sub(2),
        main.height.saturating_sub(2),
    )
}

/// The cells of Main's inner area, row by row, padding included.
fn raw_rows(app: &App) -> Vec<String> {
    let inner = inner_rect(app);
    let terminal = render(app);
    let buffer = terminal.backend().buffer();
    (inner.y..inner.bottom())
        .map(|y| {
            (inner.x..inner.right())
                .map(|x| buffer[(x, y)].symbol())
                .collect()
        })
        .collect()
}

fn rows(app: &App) -> Vec<String> {
    raw_rows(app)
        .into_iter()
        .map(|r| r.trim_end().to_string())
        .collect()
}

/// The rows below the tab row, its underline and the blank row.
fn body(app: &App) -> Vec<String> {
    rows(app).into_iter().skip(3).collect()
}

fn body_of(app: &App, count: usize) -> Vec<String> {
    body(app).into_iter().take(count).collect()
}

/// `right` ends at column `width`, so a row with the free last column passes `W - 1`.
fn right_aligned(left: &str, right: &str, width: usize) -> String {
    let gap = width.saturating_sub(left.width() + right.width());
    format!("{left}{}{right}", " ".repeat(gap))
}

fn strs(lines: &[&str]) -> Vec<String> {
    lines.iter().map(|l| l.to_string()).collect()
}

fn with_me(extra: impl FnOnce(&mut Me)) -> App {
    let mut me = fixture_me();
    extra(&mut me);
    settings_app_for(me, 120, 30)
}

fn on_tab(mut app: App, keys: &str) -> App {
    type_str(&mut app, keys);
    app
}

macro_rules! snapshot {
    ($name:literal, $app:expr) => {{
        let terminal = render(&$app);
        insta::assert_snapshot!($name, terminal.backend());
    }};
}

/// Safety tab at the given size with two muted users and nobody blocked.
fn safety_two_muted(w: u16, h: u16) -> App {
    let mut app = settings_app(w, h);
    let effects = open_safety(&mut app);
    load_muted(&mut app, &effects, &["alice", "bob"], None);
    load_blocked(&mut app, &effects, &[], None);
    app
}

/// Every text that can be long is long, and the lists have a next page.
fn long_names_app(w: u16, h: u16, tab_key: &str) -> App {
    let long = "x".repeat(70);
    let mut me = fixture_me();
    me.handle = long.clone();
    me.preferred_lane = Some(Lane::Custom(Rgb(0x3a, 0x7b, 0xd5)));
    let mut app = settings_app_for(me, w, h);
    app.set_prefs(custom_prefs());
    let effects = open_safety(&mut app);
    let user = |name: &str| user_of(merge(user_json(name), serde_json::json!({ "name": name })));
    app.update(Action::MutedLoaded {
        req: muted_req(&effects),
        page: crate::profile_tests::upage(vec![user(&long)], Some("c1")),
    });
    app.update(Action::BlockedLoaded {
        req: blocked_req(&effects),
        page: crate::profile_tests::upage(vec![user(&long)], None),
    });
    type_str(&mut app, tab_key);
    app
}

fn custom_prefs() -> Prefs {
    Prefs {
        ascii: true,
        theme: Theme::Light,
        collapse_nav: true,
        lane_dimming: false,
        reply_preview_limit: 10,
        ..Prefs::default()
    }
}

// --- tab row ----------------------------------------------------------------

#[test]
fn the_tab_row_splits_the_width_in_thirds_and_underlines_the_current_tab() {
    let labels = format!(
        "{}Profile{}Appearance{}Safety",
        " ".repeat(7),
        " ".repeat(14),
        " ".repeat(14)
    );
    let cases = [
        ("1", format!("{}{}", " ".repeat(7), "━".repeat(7))),
        ("2", format!("{}{}", " ".repeat(28), "━".repeat(10))),
        ("3", format!("{}{}", " ".repeat(52), "━".repeat(6))),
    ];
    for (key, underline) in cases {
        let app = on_tab(settings_app(120, 30), key);
        let got = rows(&app);
        assert_eq!(got[0], labels, "key {key}");
        assert_eq!(got[1], underline, "key {key}");
        assert_eq!(got[2], "", "key {key}");
    }
}

#[test]
fn the_title_of_the_frame_is_settings() {
    let app = settings_app(120, 30);
    let terminal = render(&app);
    let top = row_text(terminal.backend().buffer(), 0);
    assert!(top.contains("┏ Settings ━"), "{top}");
}

// --- 4. Profile tab ---------------------------------------------------------

#[test]
fn the_profile_tab_follows_the_mockup() {
    let app = with_me(|me| me.preferred_lane = Some(Lane::Index(0)));
    assert_eq!(
        body_of(&app, 12),
        vec![
            " Profile mirrored from GitHub   (synced 2h ago)   [S] Sync".to_string(),
            String::new(),
            " Lane color".to_string(),
            "  ( ) Auto (green)  (•) ● orange  ( ) ● green  ( ) ● violet".to_string(),
            "  ( ) ● teal  ( ) ● rose  ( ) ● ochre  ( ) custom #______".to_string(),
            "  preview:  ◉──●──●  myuron".to_string(),
            String::new(),
            right_aligned(" Pasteis mode", "[x] on", W - 1),
            "  10 per day, resets 00:00 UTC".to_string(),
            String::new(),
            " Danger zone".to_string(),
            "  [D] Delete account".to_string(),
        ]
    );
}

#[test]
fn no_preferred_lane_selects_auto() {
    let app = with_me(|me| me.preferred_lane = None);
    let got = body_of(&app, 5);
    assert_eq!(
        got[3],
        "  (•) Auto (green)  ( ) ● orange  ( ) ● green  ( ) ● violet"
    );
    assert_eq!(
        got[4],
        "  ( ) ● teal  ( ) ● rose  ( ) ● ochre  ( ) custom #______"
    );
}

#[test]
fn an_indexed_preferred_lane_selects_its_radio() {
    let names = ["orange", "green", "violet", "teal", "rose", "ochre"];
    for (i, name) in names.iter().enumerate() {
        let app = with_me(|me| me.preferred_lane = Some(Lane::Index(i as u8)));
        let lines = body_of(&app, 5);
        let selected: Vec<&String> = lines[3..5].iter().filter(|l| l.contains("(•)")).collect();
        assert_eq!(selected.len(), 1, "lane {i}");
        assert!(
            selected[0].contains(&format!("(•) ● {name}")),
            "lane {i}: {}",
            selected[0]
        );
        assert!(
            !lines[3..5].iter().any(|l| l.contains("(•) Auto")),
            "lane {i}"
        );
    }
}

#[test]
fn a_custom_preferred_lane_selects_custom_and_shows_its_hex() {
    let app = with_me(|me| me.preferred_lane = Some(Lane::Custom(Rgb(0x3a, 0x7b, 0xd5))));
    let got = body_of(&app, 5);
    assert_eq!(
        got[3],
        "  ( ) Auto (green)  ( ) ● orange  ( ) ● green  ( ) ● violet"
    );
    assert_eq!(
        got[4],
        "  ( ) ● teal  ( ) ● rose  ( ) ● ochre  (•) custom #3a7bd5"
    );
}

#[test]
fn the_auto_label_names_the_automatic_color() {
    let names = ["orange", "green", "violet", "teal", "rose", "ochre"];
    for (i, name) in names.iter().enumerate() {
        let app = with_me(|me| me.automatic_lane = i as u8);
        let got = body_of(&app, 4);
        assert!(
            got[3].starts_with(&format!("  (•) Auto ({name})")),
            "{}",
            got[3]
        );
    }
}

#[test]
fn the_synced_time_is_relative_to_the_clock() {
    let cases = [
        (Some(NOW - Duration::hours(2)), "(synced 2h ago)"),
        (Some(NOW - Duration::minutes(5)), "(synced 5m ago)"),
        (Some(NOW - Duration::days(3)), "(synced 3d ago)"),
        (Some(NOW - Duration::seconds(30)), "(synced just now)"),
        (Some(NOW - Duration::seconds(59)), "(synced just now)"),
        (Some(NOW - Duration::seconds(60)), "(synced 1m ago)"),
        (Some(NOW - Duration::days(7)), "(synced 7d ago)"),
        (Some(NOW - Duration::days(8)), "(synced Oct 12)"),
        (
            Some(datetime!(2025-10-03 10:00 UTC)),
            "(synced Oct 3, 2025)",
        ),
        (None, "(never synced)"),
    ];
    for (synced, want) in cases {
        let app = with_me(|me| me.github_synced_at = synced);
        assert_eq!(
            body_of(&app, 1)[0],
            format!(" Profile mirrored from GitHub   {want}   [S] Sync")
        );
    }
}

#[test]
fn pasteis_mode_shows_on_or_off_at_the_right_edge() {
    let on = with_me(|me| me.pasteis_mode = true);
    let off = with_me(|me| me.pasteis_mode = false);
    assert_eq!(
        body_of(&on, 8)[7],
        right_aligned(" Pasteis mode", "[x] on", W - 1)
    );
    assert_eq!(
        body_of(&off, 8)[7],
        right_aligned(" Pasteis mode", "[ ] off", W - 1)
    );
}

#[test]
fn the_preview_shows_the_own_handle() {
    let app = with_me(|me| me.handle = "alice".into());
    assert_eq!(body_of(&app, 6)[5], "  preview:  ◉──●──●  alice");
}

#[test]
fn the_preview_is_drawn_in_the_effective_lane_color() {
    let app = with_me(|me| {
        me.lane = Lane::Index(2);
        me.preferred_lane = Some(Lane::Index(2));
    });
    let inner = inner_rect(&app);
    let y = inner.y + 3 + 5;
    let terminal = render(&app);
    let buffer = terminal.backend().buffer();
    let want = theme::lane_color(Lane::Index(2), app.prefs().truecolor);
    let glyphs: Vec<_> = (inner.x..inner.right())
        .filter(|x| matches!(buffer[(*x, y)].symbol(), "◉" | "●"))
        .collect();
    assert_eq!(glyphs.len(), 3);
    for x in glyphs {
        assert_eq!(buffer[(x, y)].fg, want, "column {x}");
    }
}

#[test]
fn each_radio_dot_is_drawn_in_its_own_lane_color() {
    let app = with_me(|me| me.preferred_lane = None);
    let inner = inner_rect(&app);
    let terminal = render(&app);
    let buffer = terminal.backend().buffer();
    let rows = raw_rows(&app);
    let names = ["orange", "green", "violet", "teal", "rose", "ochre"];
    for (i, name) in names.iter().enumerate() {
        let needle = format!("● {name}");
        let (dy, line) = rows
            .iter()
            .enumerate()
            .find(|(_, l)| l.contains(&needle))
            .unwrap_or((0, &rows[0]));
        let x = line
            .find(&needle)
            .map(|byte| line[..byte].chars().count())
            .unwrap_or(0) as u16;
        let cell = &buffer[(inner.x + x, inner.y + dy as u16)];
        assert_eq!(cell.symbol(), "●", "{name}");
        assert_eq!(
            cell.fg,
            theme::lane_color(Lane::Index(i as u8), app.prefs().truecolor),
            "{name}"
        );
    }
}

#[test]
fn a_narrow_screen_wraps_the_radios_by_item_and_the_sync_button_onto_its_own_row() {
    let mut me = fixture_me();
    me.preferred_lane = Some(Lane::Custom(Rgb(0x3a, 0x7b, 0xd5)));
    let app = settings_app_for(me, 60, 30);
    assert_eq!(
        body_of(&app, 14),
        vec![
            " Profile mirrored from GitHub   (synced 2h ago)".to_string(),
            "  [S] Sync".to_string(),
            String::new(),
            " Lane color".to_string(),
            "  ( ) Auto (green)  ( ) ● orange  ( ) ● green".to_string(),
            "  ( ) ● violet  ( ) ● teal  ( ) ● rose  ( ) ● ochre".to_string(),
            "  (•) custom #3a7bd5".to_string(),
            "  preview:  ◉──●──●  myuron".to_string(),
            String::new(),
            right_aligned(" Pasteis mode", "[x] on", 57),
            "  10 per day, resets 00:00 UTC".to_string(),
            String::new(),
            " Danger zone".to_string(),
            "  [D] Delete account".to_string(),
        ]
    );
}

// --- 5. Appearance tab ------------------------------------------------------

#[test]
fn the_appearance_tab_shows_the_default_prefs() {
    let app = on_tab(settings_app(120, 30), "2");
    assert_eq!(
        body_of(&app, 5),
        vec![
            right_aligned(" Theme", "dark", W - 1),
            right_aligned(" Collapse nav", "[ ] off", W - 1),
            right_aligned(" Lane dimming", "[ ] off", W - 1),
            right_aligned(" Reply previews", "3", W - 1),
            right_aligned(" ASCII mode", "[ ] off", W - 1),
        ]
    );
}

#[test]
fn the_appearance_tab_shows_the_values_of_the_prefs() {
    let mut app = settings_app(120, 30);
    app.set_prefs(custom_prefs());
    type_str(&mut app, "2");
    assert_eq!(
        body_of(&app, 5),
        vec![
            right_aligned(" Theme", "light", W - 1),
            right_aligned(" Collapse nav", "[x] on", W - 1),
            right_aligned(" Lane dimming", "[ ] off", W - 1),
            right_aligned(" Reply previews", "10", W - 1),
            right_aligned(" ASCII mode", "[x] on", W - 1),
        ]
    );
}

#[test]
fn lane_dimming_shows_on_when_the_pref_is_on() {
    let mut app = settings_app(120, 30);
    app.set_prefs(Prefs {
        lane_dimming: true,
        ..Prefs::default()
    });
    type_str(&mut app, "2");
    assert_eq!(
        body_of(&app, 3)[2],
        right_aligned(" Lane dimming", "[x] on", W - 1)
    );
}

// --- 6. Safety tab ----------------------------------------------------------

#[test]
fn the_safety_tab_lists_the_muted_users_and_says_nobody_is_blocked() {
    let app = safety_two_muted(120, 30);
    assert_eq!(
        body_of(&app, 6),
        vec![
            " Muted".to_string(),
            right_aligned("  ◉ Alice @alice", "[m] Unmute", W - 1),
            right_aligned("  ◉ Bob @bob", "[m] Unmute", W - 1),
            String::new(),
            " Blocked".to_string(),
            "  nobody blocked".to_string(),
        ]
    );
}

#[test]
fn the_blocked_users_come_with_an_unblock_hint() {
    let mut app = settings_app(120, 30);
    let effects = open_safety(&mut app);
    load_muted(&mut app, &effects, &[], None);
    load_blocked(&mut app, &effects, &["carol", "dave"], None);
    assert_eq!(
        body_of(&app, 6),
        vec![
            " Muted".to_string(),
            "  nobody muted".to_string(),
            String::new(),
            " Blocked".to_string(),
            right_aligned("  ◉ Carol @carol", "[B] Unblock", W - 1),
            right_aligned("  ◉ Dave @dave", "[B] Unblock", W - 1),
        ]
    );
}

#[test]
fn a_user_without_a_name_is_listed_by_the_username() {
    let mut app = settings_app(120, 30);
    let effects = open_safety(&mut app);
    let nameless = user_of(merge(user_json("dora"), serde_json::json!({"name": null})));
    app.update(Action::MutedLoaded {
        req: muted_req(&effects),
        page: crate::profile_tests::upage(vec![nameless], None),
    });
    assert_eq!(
        body_of(&app, 2)[1],
        right_aligned("  ◉ dora @dora", "[m] Unmute", W - 1)
    );
}

#[test]
fn both_lists_show_loading_while_the_requests_are_pending() {
    let mut app = settings_app(120, 30);
    open_safety(&mut app);
    assert_eq!(
        body_of(&app, 5),
        strs(&[
            " Muted",
            " ┊ (loading...)",
            "",
            " Blocked",
            " ┊ (loading...)"
        ])
    );
}

#[test]
fn a_failed_list_shows_the_retry_hint_and_the_other_list_is_kept() {
    let mut app = settings_app(120, 30);
    let effects = open_safety(&mut app);
    app.update(Action::MutedFailed {
        req: muted_req(&effects),
        failure: Failure::new("http_500"),
    });
    load_blocked(&mut app, &effects, &["carol"], None);
    assert_eq!(
        body_of(&app, 5),
        vec![
            " Muted".to_string(),
            " error: http_500 — press R to retry".to_string(),
            String::new(),
            " Blocked".to_string(),
            right_aligned("  ◉ Carol @carol", "[B] Unblock", W - 1),
        ]
    );
}

#[test]
fn both_lists_empty_say_so() {
    let mut app = settings_app(120, 30);
    let effects = open_safety(&mut app);
    load_muted(&mut app, &effects, &[], None);
    load_blocked(&mut app, &effects, &[], None);
    assert_eq!(
        body_of(&app, 5),
        strs(&[
            " Muted",
            "  nobody muted",
            "",
            " Blocked",
            "  nobody blocked"
        ])
    );
}

#[test]
fn a_list_with_a_next_page_says_that_more_is_not_shown() {
    let mut app = settings_app(120, 30);
    let effects = open_safety(&mut app);
    load_muted(&mut app, &effects, &["alice"], Some("c1"));
    load_blocked(&mut app, &effects, &["carol"], Some("c2"));
    assert_eq!(
        body_of(&app, 8),
        vec![
            " Muted".to_string(),
            right_aligned("  ◉ Alice @alice", "[m] Unmute", W - 1),
            " ┊ more not shown".to_string(),
            String::new(),
            " Blocked".to_string(),
            right_aligned("  ◉ Carol @carol", "[B] Unblock", W - 1),
            " ┊ more not shown".to_string(),
            String::new(),
        ]
    );
}

#[test]
fn the_safety_lists_carry_no_selection_marker() {
    let app = safety_two_muted(120, 30);
    assert!(!rows(&app).iter().any(|r| r.contains('▌')));
}

// --- 7. widths --------------------------------------------------------------

#[test]
fn every_row_of_every_tab_leaves_the_last_column_free() {
    for (w, h) in [(120, 30), (100, 24), (80, 24), (60, 30), (40, 24)] {
        for key in ["1", "2", "3"] {
            let app = long_names_app(w, h, key);
            let tab = app.settings().tab;
            let inner_width = usize::from(inner_rect(&app).width);
            let drawn = raw_rows(&app);
            let marker = match key {
                "1" => "preview:",
                "3" => "◉ ",
                _ => "Theme",
            };
            assert!(
                drawn.iter().any(|r| r.contains(marker)),
                "{w}x{h} {tab:?} draws no {marker:?}"
            );
            for (i, row) in raw_rows(&app).iter().enumerate() {
                assert_eq!(row.width(), inner_width, "{w}x{h} {tab:?} row {i}");
                assert!(
                    row.ends_with(' '),
                    "{w}x{h} {tab:?} row {i} reaches the last column: {row:?}"
                );
            }
        }
    }
}

// --- snapshots --------------------------------------------------------------

#[test]
fn st1_settings_profile_at_120x30() {
    let app = with_me(|me| me.preferred_lane = Some(Lane::Index(0)));
    snapshot!("settings_profile", app);
}

#[test]
fn st2_settings_appearance_at_120x30() {
    let mut app = settings_app(120, 30);
    app.set_prefs(Prefs {
        theme: Theme::Light,
        lane_dimming: true,
        reply_preview_limit: 10,
        ..Prefs::default()
    });
    type_str(&mut app, "2");
    snapshot!("settings_appearance", app);
}

#[test]
fn st3_settings_safety_at_120x30() {
    snapshot!("settings_safety", safety_two_muted(120, 30));
}

#[test]
fn st4_settings_narrow_at_60x30() {
    let mut me = fixture_me();
    me.preferred_lane = Some(Lane::Custom(Rgb(0x3a, 0x7b, 0xd5)));
    snapshot!("settings_narrow", settings_app_for(me, 60, 30));
}

// --- right parts survive a short width --------------------------------------

#[test]
fn safety_rows_end_with_their_button_even_for_long_names() {
    for (w, h) in [(120, 30), (80, 24), (60, 30), (40, 24)] {
        let app = long_names_app(w, h, "3");
        let user_rows: Vec<String> = rows(&app)
            .into_iter()
            .filter(|r| r.contains("◉ "))
            .collect();
        assert_eq!(user_rows.len(), 2, "{w}x{h}");
        assert!(
            user_rows[0].ends_with("[m] Unmute"),
            "{w}x{h}: {}",
            user_rows[0]
        );
        assert!(
            user_rows[1].ends_with("[B] Unblock"),
            "{w}x{h}: {}",
            user_rows[1]
        );
    }
}

#[test]
fn appearance_values_stay_visible_at_a_very_narrow_width() {
    let mut app = settings_app(20, 20);
    app.set_prefs(custom_prefs());
    type_str(&mut app, "2");
    let got = body_of(&app, 5);
    let values = ["light", "[x] on", "[ ] off", "10", "[x] on"];
    for (row, value) in got.iter().zip(values) {
        assert!(row.ends_with(value), "{row:?} lacks {value}");
    }
}
