use ratatui::text::Line;
use serde_json::{Value, json};
use unicode_width::UnicodeWidthStr;

use crate::action::{Action, Failure};
use crate::app::App;
use crate::feed_tests::list_width;
use crate::notifications::{Filter, Filters};
use crate::notifications_tests::*;
use crate::test_support::*;
use crate::thread_tests::{merge, user_json};
use crate::ui::feed::CardSpan;
use crate::ui::notifications::{filter_rows, rows};

/// Inner width of Main at 120 columns (nav 20 + aside 32 + 2 borders).
const W: usize = 66;
/// The rightmost column stays free, as it does for cards.
const LIMIT: usize = W - 1;

// --- helpers ----------------------------------------------------------------

fn text(line: &Line) -> String {
    line.spans.iter().map(|s| s.content.as_ref()).collect()
}

fn texts(app: &App) -> Vec<String> {
    rows(app, list_width(app))
        .lines
        .iter()
        .map(|l| text(l).trim_end().to_string())
        .collect()
}

fn from_row(app: &App, from: usize) -> Vec<String> {
    texts(app)
        .get(from..)
        .map(<[String]>::to_vec)
        .unwrap_or_default()
}

fn app_with(items: Vec<Value>) -> App {
    loaded(120, 40, items, None)
}

/// `left` with `time` flush against the free column.
fn row(left: &str, time: &str) -> String {
    format!("{left:<w$}{time}", w = LIMIT - time.chars().count())
}

/// The first notification row, after the date rule.
fn headline(item: Value) -> String {
    texts(&app_with(vec![item]))
        .get(1)
        .cloned()
        .unwrap_or_default()
}

/// Everything under the first notification row.
fn below(items: Vec<Value>) -> Vec<String> {
    from_row(&app_with(items), 2)
}

fn position(lines: &[String], needle: &str) -> usize {
    lines
        .iter()
        .position(|l| l.contains(needle))
        .unwrap_or_else(|| panic!("no row contains {needle:?} in {lines:#?}"))
}

fn boxed(inner: &str) -> Vec<String> {
    vec![
        format!(" │    ┌{}┐", "─".repeat(57)),
        format!(" │    │ {inner:<55} │"),
        format!(" │    └{}┘", "─".repeat(57)),
    ]
}

/// A moderation result; the actor is the account itself.
fn moderated(kind: &str, secs_ago: i64) -> Value {
    seen(merge(
        notif_json("n0", kind, secs_ago),
        json!({ "actor": user_json("myuron"), "actors": [user_json("myuron")] }),
    ))
}

fn starred() -> Value {
    notif_json("n0", "starred", 120)
}

fn grouped(kind: &str, event_count: u32) -> Value {
    merge(
        notif_json("n0", kind, 120),
        json!({ "event_count": event_count, "actors_count": event_count }),
    )
}

fn named(username: &str, name: &str) -> Value {
    merge(user_json(username), json!({ "name": name }))
}

// --- 14. wording ------------------------------------------------------------

#[test]
fn each_kind_gets_its_node_symbol_and_wording() {
    let cases: Vec<(Value, &str)> = vec![
        (starred(), " ◉ ★  Bob @bob starred your commit"),
        (
            notif_json("n0", "followed", 120),
            " ◉ +  Bob @bob followed you",
        ),
        (
            notif_json("n0", "replied", 120),
            " ◉ ↩  Bob @bob replied to your commit",
        ),
        (
            notif_json("n0", "quoted", 120),
            " ◉ ❝  Bob @bob quoted your commit",
        ),
        (
            notif_json("n0", "mentioned", 120),
            " ◉ @  Bob @bob mentioned you",
        ),
        (
            moderated("approved", 120),
            " ○ ✓  Your commit was approved.",
        ),
        (
            moderated("rejected", 120),
            " ○ ✗  Your commit was rejected.",
        ),
    ];
    for (item, left) in cases {
        assert_eq!(headline(item), row(left, "2m"));
    }
}

#[test]
fn stars_and_follows_name_one_actor_and_count_the_others() {
    let cases = [
        ("starred", 0, " ◉ ★  Bob @bob starred your commit"),
        ("starred", 1, " ◉ ★  Bob @bob starred your commit"),
        ("starred", 2, " ◉ ★  Bob and 1 other starred your commit"),
        ("starred", 5, " ◉ ★  Bob and 4 others starred your commit"),
        ("followed", 0, " ◉ +  Bob @bob followed you"),
        ("followed", 1, " ◉ +  Bob @bob followed you"),
        ("followed", 2, " ◉ +  Bob and 1 other followed you"),
        ("followed", 3, " ◉ +  Bob and 2 others followed you"),
    ];
    for (kind, count, left) in cases {
        let item = merge(
            notif_json("n0", kind, 120),
            json!({ "actors_count": count, "event_count": count }),
        );
        // The list sends at most three actors, so a larger count must not depend on the list length.
        assert_eq!(headline(item), row(left, "2m"), "{kind} {count}");
    }
}

#[test]
fn replies_name_only_the_latest_actor_whatever_the_count() {
    let item = merge(
        notif_json("n0", "replied", 120),
        json!({ "actors_count": 4, "event_count": 4 }),
    );
    assert_eq!(
        headline(item),
        row(" ◉ ↩  Bob @bob replied to your commit", "2m")
    );
}

#[test]
fn the_wording_has_no_name_when_there_is_no_actor() {
    let cases = [
        ("starred", " ◉ ★  Starred your commit"),
        ("replied", " ◉ ↩  Replied to your commit"),
    ];
    for (kind, left) in cases {
        let item = merge(notif_json("n0", kind, 120), json!({ "actor": null }));
        assert_eq!(headline(item), row(left, "2m"), "{kind}");
    }
}

#[test]
fn control_characters_are_dropped_from_names() {
    let item = merge(
        starred(),
        json!({ "actor": named("bob", "B\u{7}o\u{1b}b") }),
    );
    assert_eq!(
        headline(item),
        row(" ◉ ★  Bob @bob starred your commit", "2m")
    );
}

#[test]
fn the_username_stands_in_for_a_missing_name() {
    let item = merge(
        notif_json("n0", "replied", 120),
        json!({ "actor": merge(user_json("bob"), json!({"name": null})) }),
    );
    assert_eq!(
        headline(item),
        row(" ◉ ↩  bob @bob replied to your commit", "2m")
    );
}

// --- time column and truncation --------------------------------------------

#[test]
fn the_relative_time_sits_in_the_last_usable_column() {
    let item = merge(starred(), json!({ "actors_count": 5, "event_count": 5 }));
    assert_eq!(
        headline(item),
        format!("{:<63}2m", " ◉ ★  Bob and 4 others starred your commit")
    );
}

#[test]
fn the_time_follows_the_age_of_the_notification() {
    let cases = [
        (30, "now"),
        (120, "2m"),
        (3 * 3600, "3h"),
        (2 * 86400, "2d"),
    ];
    for (secs, time) in cases {
        let item = notif_json("n0", "starred", secs);
        assert_eq!(
            headline(item),
            row(" ◉ ★  Bob @bob starred your commit", time)
        );
    }
}

#[test]
fn the_time_column_is_kept_on_a_narrow_main_pane() {
    let app = loaded(60, 30, vec![starred()], None);
    assert_eq!(
        texts(&app).get(1).cloned().unwrap_or_default(),
        format!("{:<55}2m", " ◉ ★  Bob @bob starred your commit")
    );
}

#[test]
fn a_long_wording_is_cut_with_an_ellipsis_and_keeps_the_time() {
    let item = merge(
        notif_json("n0", "replied", 120),
        json!({ "actor": named("bob", &"W".repeat(80)) }),
    );
    let line = headline(item);
    assert_eq!(line.width(), LIMIT);
    assert!(line.starts_with(" ◉ ↩  WWW"), "{line}");
    assert!(line.ends_with("… 2m"), "{line}");
}

// --- 14b. second lines ------------------------------------------------------

#[test]
fn a_star_shows_the_excerpt_of_the_commit_in_a_box() {
    let item = with_commit(starred(), "c1", "myuron", "shipped a terminal client");
    assert_eq!(below(vec![item]), boxed("shipped a terminal client"));
}

#[test]
fn a_long_excerpt_is_cut_with_an_ellipsis_to_one_row() {
    let body = "x".repeat(80);
    let item = with_commit(starred(), "c1", "myuron", &body);
    let expected = format!("{}…", "x".repeat(54));
    assert_eq!(below(vec![item]), boxed(&expected));
}

#[test]
fn a_multi_line_excerpt_is_joined_into_one_row() {
    let item = with_commit(starred(), "c1", "myuron", "first line\nsecond line");
    assert_eq!(below(vec![item]), boxed("first line second line"));
}

#[test]
fn a_box_has_no_control_characters() {
    let item = with_commit(starred(), "c1", "myuron", "a\u{1b}b\u{7}c");
    assert_eq!(below(vec![item]), boxed("abc"));
}

#[test]
fn a_star_without_a_commit_has_no_second_line() {
    let lines = texts(&app_with(vec![starred()]));
    assert_eq!(lines.len(), 2);
    assert!(lines[1].contains("Bob @bob starred your commit"));
}

#[test]
fn a_reply_shows_the_body_of_the_reply() {
    let item = with_commit(
        notif_json("n0", "replied", 120),
        "c1",
        "bob",
        "sounds great, ship it",
    );
    assert_eq!(below(vec![item]), vec![" │    sounds great, ship it"]);
}

#[test]
fn a_reply_body_is_limited_to_two_rows() {
    let item = with_commit(
        notif_json("n0", "mentioned", 120),
        "c1",
        "bob",
        "one\ntwo\nthree\nfour",
    );
    assert_eq!(below(vec![item]), vec![" │    one", " │    two…"]);
}

#[test]
fn a_quote_shows_the_body_of_the_quoting_commit() {
    let item = with_commit(
        notif_json("n0", "quoted", 120),
        "c9",
        "bob",
        "worth reading\nby everyone",
    );
    assert_eq!(
        below(vec![item]),
        vec![" │    worth reading", " │    by everyone"]
    );
}

#[test]
fn a_long_reply_row_is_cut_at_the_free_column() {
    let item = with_commit(
        notif_json("n0", "replied", 120),
        "c1",
        "bob",
        &"y".repeat(100),
    );
    let lines = below(vec![item]);
    assert_eq!(lines.len(), 1);
    assert_eq!(
        lines.first().cloned().unwrap_or_default(),
        format!(" │    {}…", "y".repeat(58))
    );
}

#[test]
fn a_reply_without_a_commit_has_no_second_line() {
    let lines = texts(&app_with(vec![notif_json("n0", "replied", 120)]));
    assert_eq!(lines.len(), 2);
    assert!(lines[1].contains("replied to your commit"));
}

#[test]
fn approval_shows_the_excerpt_only() {
    let item = with_commit(
        moderated("approved", 120),
        "c1",
        "myuron",
        "my first commit",
    );
    assert_eq!(below(vec![item]), boxed("my first commit"));
}

#[test]
fn a_rejection_shows_the_excerpt_the_reason_and_who_can_see_it() {
    let item = with_commit(
        merge(moderated("rejected", 120), json!({ "reason": "spam" })),
        "c1",
        "myuron",
        "buy cheap watches",
    );
    let mut expected = boxed("buy cheap watches");
    expected.push(" │    reason: spam".into());
    expected.push(" │    Only you can see this update and your rejected commit.".into());
    assert_eq!(below(vec![item]), expected);
}

#[test]
fn a_rejection_without_a_reason_keeps_the_visibility_note() {
    let item = moderated("rejected", 120);
    assert_eq!(
        below(vec![item]),
        vec![" │    Only you can see this update and your rejected commit."]
    );
}

// --- 10b. groups ------------------------------------------------------------

#[test]
fn a_closed_group_shows_how_many_events_it_holds() {
    let cases = [
        ("starred", 5, " │    ⌄ 5 stars"),
        ("followed", 2, " │    ⌄ 2 followers"),
        ("replied", 3, " │    ⌄ 3 updates"),
        ("mentioned", 2, " │    ⌄ 2 updates"),
    ];
    for (kind, count, line) in cases {
        assert_eq!(below(vec![grouped(kind, count)]), vec![line], "{kind}");
    }
}

#[test]
fn the_group_row_comes_after_the_excerpt() {
    let item = with_commit(grouped("starred", 5), "c1", "myuron", "shipped");
    let mut expected = boxed("shipped");
    expected.push(" │    ⌄ 5 stars".into());
    assert_eq!(below(vec![item]), expected);
}

#[test]
fn a_single_event_has_no_group_row() {
    let lines = texts(&app_with(vec![grouped("starred", 1)]));
    assert_eq!(lines.len(), 2);
    assert!(lines[1].contains("starred your commit"));
}

fn opened_group(count: u32) -> (App, Vec<crate::action::Effect>) {
    let mut app = app_with(vec![grouped("starred", count)]);
    let effects = type_str(&mut app, "o");
    (app, effects)
}

fn child_json(id: &str, actor: &str, secs_ago: i64) -> Value {
    let mut value = merge(
        notif_json(id, "starred", secs_ago),
        json!({ "group_id": "g-n0", "actor": user_json(actor) }),
    );
    value.as_object_mut().unwrap().remove("actors");
    value
}

#[test]
fn an_open_group_shows_a_loading_row_until_its_notifications_arrive() {
    let (app, _) = opened_group(5);
    assert_eq!(
        from_row(&app, 2),
        [" │    ⌃ 5 stars", " │    ┊ (loading...)"]
    );
}

#[test]
fn an_open_group_lists_its_notifications_with_the_actor_and_the_time() {
    let (mut app, effects) = opened_group(5);
    app.update(Action::GroupLoaded {
        id: "n0".into(),
        req: group_req(&effects),
        page: gpage(
            vec![
                child_json("k0", "bob", 60),
                child_json("k1", "carol", 2400),
                child_json("k2", "dave", 3600),
            ],
            Some("more"),
        ),
    });
    assert_eq!(
        from_row(&app, 2),
        [
            " │    ⌃ 5 stars".to_string(),
            row(" │    ├─ Bob", "1m"),
            row(" │    ├─ Carol", "40m"),
            row(" │    └─ Dave", "1h"),
            " │    ┊ 2 more not shown".to_string(),
        ]
    );
}

#[test]
fn the_more_row_is_left_out_when_every_event_is_listed() {
    let (mut app, effects) = opened_group(2);
    app.update(Action::GroupLoaded {
        id: "n0".into(),
        req: group_req(&effects),
        page: gpage(
            vec![child_json("k0", "bob", 60), child_json("k1", "carol", 120)],
            None,
        ),
    });
    assert_eq!(
        from_row(&app, 2),
        [
            " │    ⌃ 2 stars".to_string(),
            row(" │    ├─ Bob", "1m"),
            row(" │    └─ Carol", "2m"),
        ]
    );
}

#[test]
fn a_failed_group_shows_what_could_not_be_loaded() {
    let cases = [
        ("starred", "stars"),
        ("followed", "followers"),
        ("replied", "updates"),
    ];
    for (kind, noun) in cases {
        let mut app = app_with(vec![grouped(kind, 3)]);
        let effects = type_str(&mut app, "o");
        app.update(Action::GroupFailed {
            id: "n0".into(),
            req: group_req(&effects),
            failure: Failure::new("http_500"),
        });
        assert_eq!(
            texts(&app).last().map(String::as_str),
            Some(format!(" │    fatal: could not load {noun}").as_str()),
            "{kind}"
        );
    }
}

#[test]
fn closing_a_group_brings_back_the_count_row() {
    let (mut app, effects) = opened_group(3);
    app.update(Action::GroupLoaded {
        id: "n0".into(),
        req: group_req(&effects),
        page: gpage(vec![child_json("k0", "bob", 60)], None),
    });
    type_str(&mut app, "o");
    assert_eq!(from_row(&app, 2), [" │    ⌄ 3 stars"]);
}

// --- 15. new and seen -------------------------------------------------------

fn glyph_of(lines: &[String], needle: &str) -> char {
    lines[position(lines, needle)].chars().nth(1).unwrap()
}

#[test]
fn a_notification_read_at_the_response_time_or_later_or_never_is_new() {
    let items = vec![
        merge(
            notif_json("a", "replied", 100),
            json!({"read_at": "2026-10-20T11:59:50Z", "actor": user_json("amy")}),
        ),
        merge(
            notif_json("b", "replied", 200),
            json!({"read_at": "2026-10-20T11:59:49Z", "actor": user_json("ben")}),
        ),
        merge(
            notif_json("c", "replied", 300),
            json!({"read_at": null, "actor": user_json("cat")}),
        ),
        merge(
            notif_json("d", "replied", 400),
            json!({"read_at": "2026-10-20T12:00:00Z", "actor": user_json("dan")}),
        ),
    ];
    let lines = texts(&app_with(items));
    assert_eq!(glyph_of(&lines, "Amy @amy"), '◉');
    assert_eq!(glyph_of(&lines, "Ben @ben"), '○');
    assert_eq!(glyph_of(&lines, "Cat @cat"), '◉');
    assert_eq!(glyph_of(&lines, "Dan @dan"), '◉');
}

/// Main starts at column 20 at 120 columns; the Nav and the Aside also draw these symbols.
fn main_cell_fg(app: &App, symbol: &str) -> Option<ratatui::style::Color> {
    let terminal = render(app);
    let buffer = terminal.backend().buffer();
    let (_, h) = app.size();
    (0..h)
        .flat_map(|y| (21..87).map(move |x| (x, y)))
        .find(|&(x, y)| buffer[(x, y)].symbol() == symbol)
        .map(|(x, y)| buffer[(x, y)].fg)
}

#[test]
fn the_news_use_the_primary_color_and_the_seen_do_not() {
    let app = app_with(vec![
        notif_json("n0", "starred", 100),
        seen(notif_json("n1", "starred", 200)),
    ]);
    assert_eq!(main_cell_fg(&app, "◉"), Some(crate::ui::theme::PRIMARY));
    let seen_fg = main_cell_fg(&app, "○");
    assert!(seen_fg.is_some(), "no seen node is drawn");
    assert_ne!(seen_fg, Some(crate::ui::theme::PRIMARY));
}

fn head_row() -> String {
    let head = " ╌╌ HEAD@{last visit} ";
    format!("{head}{}", "╌".repeat(LIMIT - head.chars().count()))
}

#[test]
fn a_marker_separates_the_news_from_the_seen() {
    let items = vec![
        merge(
            notif_json("n0", "replied", 100),
            json!({"actor": user_json("amy")}),
        ),
        merge(
            notif_json("n1", "replied", 200),
            json!({"actor": user_json("ben")}),
        ),
        seen(merge(
            notif_json("n2", "replied", 300),
            json!({"actor": user_json("cat")}),
        )),
        seen(merge(
            notif_json("n3", "replied", 400),
            json!({"actor": user_json("dan")}),
        )),
    ];
    let lines = texts(&app_with(items));
    let heads: Vec<usize> = (0..lines.len())
        .filter(|&i| lines[i] == head_row())
        .collect();
    assert_eq!(heads.len(), 1, "{lines:#?}");
    assert!(position(&lines, "Ben @ben") < heads[0]);
    assert_eq!(heads[0] + 1, position(&lines, "Cat @cat"));
}

#[test]
fn no_marker_is_drawn_when_everything_is_new_or_everything_is_seen() {
    let all_new = texts(&app_with(vec![
        notif_json("n0", "starred", 100),
        notif_json("n1", "starred", 200),
    ]));
    let all_seen = texts(&app_with(vec![
        seen(notif_json("n0", "starred", 100)),
        seen(notif_json("n1", "starred", 200)),
    ]));
    for lines in [all_new, all_seen] {
        assert_eq!(
            lines.iter().filter(|l| l.contains("starred your")).count(),
            2
        );
        assert!(!lines.contains(&head_row()));
    }
}

#[test]
fn the_marker_comes_before_the_date_rule_of_the_day_it_opens() {
    let items = vec![
        notif_json("n0", "starred", 100),
        seen(notif_json("n1", "starred", 26 * 3600)),
    ];
    let lines = texts(&app_with(items));
    let head = position(&lines, "HEAD@{last visit}");
    let yesterday = position(&lines, "Yesterday");
    assert_eq!(head + 1, yesterday, "{lines:#?}");
}

// --- date rules -------------------------------------------------------------

#[test]
fn a_date_rule_opens_the_list_and_each_new_day() {
    let items = vec![
        notif_json("n0", "starred", 100),
        notif_json("n1", "starred", 200),
        notif_json("n2", "starred", 26 * 3600),
    ];
    let lines = texts(&app_with(items));
    let rule = |label: &str| {
        let head = format!(" ╎ ── {label} ");
        format!("{head}{}", "─".repeat(LIMIT - head.chars().count()))
    };
    assert_eq!(lines.first().cloned().unwrap_or_default(), rule("Today"));
    assert_eq!(lines.iter().filter(|l| l.contains("╎ ──")).count(), 2);
    assert_eq!(lines[position(&lines, "Yesterday")], rule("Yesterday"));
}

// --- 17. banner, empty, tail ------------------------------------------------

#[test]
fn the_banner_is_centered_and_counts_the_new_notifications() {
    let mut app = app_with(replies_for_banner());
    refresh_me(&mut app, 2);
    let lines = texts(&app);
    assert_eq!(
        lines.first().cloned().unwrap_or_default(),
        format!("{}↑ 2 new notifications", " ".repeat(22))
    );
}

#[test]
fn the_banner_is_singular_for_one_notification() {
    let mut app = app_with(replies_for_banner());
    refresh_me(&mut app, 1);
    let lines = texts(&app);
    assert_eq!(
        lines.first().cloned().unwrap_or_default(),
        format!("{}↑ 1 new notification", " ".repeat(23))
    );
}

#[test]
fn the_banner_is_drawn_in_the_primary_color() {
    let mut app = app_with(replies_for_banner());
    refresh_me(&mut app, 2);
    assert_eq!(main_cell_fg(&app, "↑"), Some(crate::ui::theme::PRIMARY));
}

#[test]
fn no_banner_is_drawn_without_news() {
    let lines = texts(&app_with(replies_for_banner()));
    assert!(lines.iter().any(|l| l.contains("replied to your commit")));
    assert!(!lines.iter().any(|l| l.contains("new notification")));
}

fn replies_for_banner() -> Vec<Value> {
    vec![notif_json("n0", "replied", 100)]
}

fn empty_with(keys: &[&str]) -> Vec<String> {
    let (mut app, mut effects) = on_notifications(120, 40);
    for key in keys {
        effects = type_str(&mut app, key);
    }
    deliver_notifs(&mut app, &effects, npage(vec![], None, Some(READ_AT)));
    texts(&app)
}

#[test]
fn an_empty_list_with_every_filter_on_shows_the_bare_command() {
    assert_eq!(empty_with(&[]), vec![" $ cat /dev/notifications"]);
}

#[test]
fn an_empty_list_names_the_filters_that_are_on() {
    assert_eq!(
        empty_with(&["2", "4", "5", "6"]),
        vec![" $ cat /dev/notifications | grep -E 'replies|stars'"]
    );
}

#[test]
fn an_empty_list_with_every_filter_off_greps_for_nothing() {
    assert_eq!(
        empty_with(&["a"]),
        vec![" $ cat /dev/notifications | grep -E '^$'"]
    );
}

#[test]
fn the_first_load_shows_a_loading_row() {
    let (app, _) = on_notifications(120, 40);
    assert_eq!(texts(&app), vec![" ┊ (loading...)"]);
}

#[test]
fn a_failed_first_load_shows_the_retry_hint() {
    let (mut app, effects) = on_notifications(120, 40);
    app.update(Action::NotificationsFailed {
        req: notif_req(&effects),
        failure: Failure::new("http_500"),
    });
    assert_eq!(texts(&app), vec![" error: http_500 — press R to retry"]);
}

#[test]
fn a_page_being_appended_shows_a_loading_more_row() {
    let items: Vec<Value> = (0..6)
        .map(|i| notif_json(&format!("n{i}"), "starred", 100 + i))
        .collect();
    let mut app = loaded(120, 40, items, Some("next"));
    type_str(&mut app, "G");
    assert_eq!(
        texts(&app).last().map(String::as_str),
        Some(" ┊ (loading more...)")
    );
}

#[test]
fn a_failed_append_shows_the_retry_hint_under_the_list() {
    let items: Vec<Value> = (0..6)
        .map(|i| notif_json(&format!("n{i}"), "starred", 100 + i))
        .collect();
    let mut app = loaded(120, 40, items, Some("next"));
    let effects = type_str(&mut app, "G");
    app.update(Action::NotificationsFailed {
        req: notif_req(&effects),
        failure: Failure::new("http_500"),
    });
    assert_eq!(
        texts(&app).last().map(String::as_str),
        Some(" error: http_500 — press R to retry")
    );
}

#[test]
fn the_end_of_the_list_adds_no_closing_row() {
    let lines = texts(&app_with(vec![
        notif_json("n0", "starred", 100),
        notif_json("n1", "followed", 200),
    ]));
    assert_eq!(
        lines.last().map(String::as_str),
        Some(row(" ◉ +  Bob @bob followed you", "3m").as_str())
    );
}

// --- cards ------------------------------------------------------------------

#[test]
fn each_notification_is_one_card_starting_at_its_headline() {
    let items = vec![
        with_commit(grouped("starred", 5), "c1", "myuron", "shipped"),
        with_commit(notif_json("n1", "replied", 300), "c2", "bob", "nice"),
        notif_json("n2", "followed", 400),
    ];
    let app = app_with(items);
    let view = rows(&app, list_width(&app));
    assert_eq!(
        view.cards,
        vec![
            CardSpan {
                start: 1,
                height: 5
            },
            CardSpan {
                start: 6,
                height: 2
            },
            CardSpan {
                start: 8,
                height: 1
            },
        ]
    );
}

// --- 16. filter rows --------------------------------------------------------

fn filter_texts(filters: &Filters, width: u16) -> Vec<String> {
    filter_rows(filters, width).iter().map(text).collect()
}

const HINT: &str = "(1-6 toggle, a all/none)";

#[test]
fn the_filter_rows_show_each_toggle_and_the_hint() {
    let mut filters = Filters::default();
    filters.toggle(Filter::Mentions);
    assert_eq!(
        filter_texts(&filters, 66),
        vec![
            " [x] Replies  [x] Quotes  [x] Stars  [ ] Mentions  [x] Follows".to_string(),
            format!(" [x] Moderation{HINT:>50}"),
        ]
    );
}

#[test]
fn every_toggle_off_is_drawn_with_an_empty_box() {
    let mut filters = Filters::default();
    for filter in Filter::ALL {
        filters.toggle(filter);
    }
    let joined = filter_texts(&filters, 66).join("\n");
    assert!(!joined.contains("[x]"), "{joined}");
    assert_eq!(joined.matches("[ ]").count(), 6);
}

#[test]
fn the_filter_rows_wrap_by_item_on_a_narrow_pane() {
    let mut filters = Filters::default();
    filters.toggle(Filter::Mentions);
    assert_eq!(
        filter_texts(&filters, 58),
        vec![
            " [x] Replies  [x] Quotes  [x] Stars  [ ] Mentions".to_string(),
            format!(" [x] Follows  [x] Moderation{HINT:>29}"),
        ]
    );
}

#[test]
fn the_filter_rows_never_exceed_the_usable_width() {
    let filters = Filters::default();
    for width in [30u16, 40, 58, 66, 74] {
        let rows = filter_texts(&filters, width);
        assert!(!rows.is_empty());
        for line in &rows {
            assert!(line.width() < usize::from(width), "{width}: {line:?}");
        }
        let joined = rows.join(" ");
        let mut at = 0;
        for filter in Filter::ALL {
            let found = joined[at..].find(filter.label());
            assert!(found.is_some(), "{width}: {} in {joined:?}", filter.label());
            at += found.unwrap();
        }
        assert!(joined.contains(HINT), "{width}: {joined:?}");
    }
}

// --- 18. widths -------------------------------------------------------------

fn crowded() -> Vec<Value> {
    let long = "山田".repeat(60);
    let mut items = vec![
        with_commit(
            merge(
                notif_json("n0", "starred", 100),
                json!({"event_count": 5, "actors_count": 5, "actor": named("bob", &long)}),
            ),
            "c1",
            "myuron",
            &long,
        ),
        with_commit(
            merge(
                notif_json("n1", "replied", 200),
                json!({"event_count": 2, "actor": named("bob", &long)}),
            ),
            "c2",
            "bob",
            &format!("{long}\n{long}\n{long}"),
        ),
        seen(with_commit(
            merge(moderated("rejected", 300), json!({"reason": long})),
            "c3",
            "myuron",
            "short",
        )),
        merge(notif_json("n3", "poked", 400), json!({"actor": null})),
        notif_json("n4", "followed", 40 * 3600),
    ];
    items.push(moderated("approved", 50 * 3600));
    items
}

#[test]
fn every_row_stays_within_the_usable_width_at_every_size() {
    for (w, h) in [(120, 40), (80, 24), (60, 30), (40, 24)] {
        let mut app = loaded(w, h, crowded(), None);
        let effects = type_str(&mut app, "o");
        app.update(Action::GroupLoaded {
            id: "n0".into(),
            req: group_req(&effects),
            page: gpage(
                vec![merge(
                    notif_json("k0", "starred", 60),
                    json!({"group_id": "g-n0", "actor": named("bob", &"山".repeat(70))}),
                )],
                Some("more"),
            ),
        });
        refresh_me(&mut app, 3);
        let width = usize::from(list_width(&app));
        let view = rows(&app, list_width(&app));
        assert!(view.lines.len() > 10, "{w}x{h}");
        for line in &view.lines {
            assert!(
                line.width() < width,
                "{w}x{h}: {} columns in {:?}",
                line.width(),
                text(line)
            );
        }
    }
}

#[test]
fn a_hostile_name_does_not_put_an_escape_into_any_row() {
    let item = merge(
        starred(),
        json!({"actor": named("bob", "\u{1b}[31mBob\u{202e}")}),
    );
    let lines = texts(&app_with(vec![item]));
    assert!(lines.iter().any(|l| l.contains("starred your commit")));
    for line in lines {
        assert!(
            !line.contains('\u{1b}') && !line.contains('\u{202e}'),
            "{line:?}"
        );
    }
}

// --- snapshots --------------------------------------------------------------

macro_rules! snapshot {
    ($name:literal, $app:expr) => {{
        let terminal = render(&$app);
        insta::assert_snapshot!($name, terminal.backend());
    }};
}

/// The Notifications mockup of docs/screen-design.md, with Mentions switched off like there.
fn mockup_items() -> Vec<Value> {
    vec![
        with_commit(
            merge(
                notif_json("n1", "starred", 120),
                json!({
                    "event_count": 5,
                    "actors_count": 5,
                    "actors": [user_json("bob"), user_json("carol"), user_json("dave")],
                }),
            ),
            "c1",
            "myuron",
            "shipped a terminal client for /dev/null",
        ),
        with_commit(
            merge(
                notif_json("n2", "replied", 300),
                json!({"actor": user_json("dave"), "actors": [user_json("dave")]}),
            ),
            "c2",
            "dave",
            "sounds great, ship it",
        ),
        merge(
            notif_json("n3", "followed", 3600),
            json!({
                "actor": user_json("erin"),
                "actors": [user_json("erin"), user_json("frank")],
                "event_count": 2,
                "actors_count": 2,
            }),
        ),
        seen(with_commit(
            merge(
                notif_json("n4", "approved", 3 * 3600),
                json!({"actor": user_json("myuron")}),
            ),
            "c3",
            "myuron",
            "my first commit to /dev/null",
        )),
        seen(with_commit(
            merge(
                notif_json("n5", "rejected", 11 * 3600),
                json!({"actor": user_json("myuron"), "reason": "spam"}),
            ),
            "c4",
            "myuron",
            "buy cheap watches at my site",
        )),
    ]
}

fn mockup(w: u16, h: u16) -> App {
    let (mut app, effects) = on_notifications(w, h);
    deliver_notifs(
        &mut app,
        &effects,
        npage(mockup_items(), None, Some(READ_AT)),
    );
    let effects = type_str(&mut app, "4");
    deliver_notifs(
        &mut app,
        &effects,
        npage(mockup_items(), None, Some(READ_AT)),
    );
    refresh_me(&mut app, 2);
    app
}

#[test]
fn n1_notifications_wide() {
    snapshot!("notifications_wide", mockup(120, 40));
}

#[test]
fn n2_notifications_expanded() {
    let items = vec![
        with_commit(
            merge(
                notif_json("n1", "starred", 120),
                json!({"event_count": 5, "actors_count": 5}),
            ),
            "c1",
            "myuron",
            "shipped a terminal client for /dev/null",
        ),
        with_commit(
            merge(
                notif_json("n2", "replied", 300),
                json!({"actor": user_json("dave")}),
            ),
            "c2",
            "dave",
            "sounds great, ship it",
        ),
        seen(with_commit(
            merge(
                notif_json("n4", "approved", 3 * 3600),
                json!({"actor": user_json("myuron")}),
            ),
            "c3",
            "myuron",
            "my first commit to /dev/null",
        )),
    ];
    let mut app = loaded(120, 30, items, None);
    let effects = type_str(&mut app, "o");
    app.update(Action::GroupLoaded {
        id: "n1".into(),
        req: group_req(&effects),
        page: gpage(
            vec![
                merge(
                    notif_json("k0", "starred", 120),
                    json!({"group_id": "g-n1", "actor": user_json("bob")}),
                ),
                merge(
                    notif_json("k1", "starred", 2400),
                    json!({"group_id": "g-n1", "actor": user_json("carol")}),
                ),
                merge(
                    notif_json("k2", "starred", 3600),
                    json!({"group_id": "g-n1", "actor": user_json("dave")}),
                ),
            ],
            Some("more"),
        ),
    });
    snapshot!("notifications_expanded", app);
}

#[test]
fn n3_notifications_empty() {
    let app = loaded(120, 24, vec![], None);
    snapshot!("notifications_empty", app);
}

#[test]
fn n4_notifications_narrow() {
    snapshot!("notifications_narrow", mockup(60, 30));
}

#[test]
fn a1_aside_and_nav_badge_after_me_loaded() {
    let (mut app, init) = crate::feed_tests::started(120, 24);
    let response = serde_json::from_value(me_json(
        5,
        json!({
            "id": "user-mira",
            "username": "mira",
            "name": "Mira Lee",
            "bio": "Terminal tinkerer",
            "commits_count": 120,
            "following_count": 3,
            "followers_count": 45,
            "pasteis_remaining": 4,
        }),
    ))
    .unwrap();
    app.update(Action::MeLoaded {
        req: me_req(&init),
        response: Box::new(response),
    });
    snapshot!("aside_after_me_loaded", app);
}

// --- rev.4: Unknown, scroll ------------------------------------------------

fn unknown_from(username: &str) -> Value {
    with_commit(
        merge(
            notif_json("nu", "poked", 90),
            json!({ "actor": user_json(username), "actors": [user_json(username)] }),
        ),
        "cu",
        username,
        "from an unknown kind",
    )
}

#[test]
fn a_notification_of_an_unknown_kind_is_not_shown() {
    let known = with_commit(notif_json("n0", "replied", 120), "c1", "bob", "hello");
    let lines = texts(&app_with(vec![unknown_from("zed"), known]));
    assert!(
        lines
            .iter()
            .any(|l| l.contains("Bob @bob replied to your commit"))
    );
    assert!(
        !lines.iter().any(|l| l.contains("zed") || l.contains("Zed")),
        "{lines:#?}"
    );
    assert!(
        !lines.iter().any(|l| l.contains("from an unknown kind")),
        "{lines:#?}"
    );
    assert_eq!(lines.len(), 3, "{lines:#?}");
}

fn many(count: usize) -> Vec<Value> {
    (0..count)
        .map(|i| {
            with_commit(
                notif_json(&format!("n{i}"), "replied", 60 * (i as i64 + 1)),
                &format!("c{i}"),
                "bob",
                &format!("body {i}"),
            )
        })
        .collect()
}

/// 24 rows: 21 inside the frame, 2 of them taken by the filters.
const VIEW: usize = 19;

fn main_text(app: &App) -> String {
    let terminal = render(app);
    let buffer = terminal.backend().buffer();
    let (_, h) = app.size();
    (0..h)
        .map(|y| row_text(buffer, y))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn shift_g_scrolls_the_last_card_into_view_under_the_filter_rows() {
    let mut app = loaded(120, 24, many(30), None);
    type_str(&mut app, "G");
    assert_eq!(app.notifications().selected, 29);
    // One date rule and two rows per card, ending at the last row.
    assert_eq!(usize::from(app.notifications().scroll), 1 + 60 - VIEW);
    assert!(main_text(&app).contains("body 29"));
    assert!(!main_text(&app).contains("body 0 "));
}

#[test]
fn moving_down_scrolls_just_enough_to_keep_the_card_in_view() {
    let mut app = loaded(120, 24, many(30), None);
    type_str(&mut app, "jjjjjjjjjjjj");
    assert_eq!(app.notifications().selected, 12);
    // Card 12 occupies rows 25..27 of the list.
    assert_eq!(app.notifications().scroll, 8);
}

#[test]
fn gg_scrolls_back_to_the_top() {
    let mut app = loaded(120, 24, many(30), None);
    type_str(&mut app, "G");
    assert!(app.notifications().scroll > 0);
    type_str(&mut app, "gg");
    assert_eq!(app.notifications().selected, 0);
    assert_eq!(app.notifications().scroll, 0);
}

#[test]
fn a_refresh_puts_the_selection_and_the_scroll_back_on_top() {
    let mut app = loaded(120, 24, many(30), None);
    type_str(&mut app, "G");
    assert!(app.notifications().scroll > 0);
    let effects = type_str(&mut app, "R");
    deliver_notifs(&mut app, &effects, npage(many(30), None, Some(READ_AT)));
    assert_eq!(app.notifications().selected, 0);
    assert_eq!(app.notifications().scroll, 0);
}

#[test]
fn loading_the_news_with_k_puts_the_selection_and_the_scroll_back_on_top() {
    let mut app = loaded(120, 24, many(30), None);
    refresh_me(&mut app, 2);
    type_str(&mut app, "G");
    type_str(&mut app, "gg");
    let effects = type_str(&mut app, "k");
    assert_eq!(notif_fetches(&effects).len(), 1);
    deliver_notifs(&mut app, &effects, npage(many(31), None, Some(READ_AT)));
    assert_eq!(app.notifications().selected, 0);
    assert_eq!(app.notifications().scroll, 0);
    assert_eq!(app.notifications().items.len(), 31);
}

// --- review fixes ------------------------------------------------------------

#[test]
fn no_empty_message_is_shown_while_another_page_is_still_to_come() {
    let (mut app, effects) = on_notifications(120, 40);
    // The first page held nothing visible; the follow-up request is pending.
    let hidden = npage(
        vec![notif_json("u0", "poked", 60)],
        Some("c2"),
        Some(READ_AT),
    );
    app.update(Action::NotificationsLoaded {
        req: notif_req(&effects),
        page: Box::new(hidden),
    });
    let lines = texts(&app);
    assert!(!lines.iter().any(|l| l.contains("cat /dev/notifications")));
    assert_eq!(lines, vec![" ┊ (loading...)"]);
}

#[test]
fn the_selected_notification_is_marked_with_a_bar_and_a_background() {
    let app = app_with(vec![
        notif_json("n0", "starred", 100),
        notif_json("n1", "starred", 200),
    ]);
    let terminal = render(&app);
    let buffer = terminal.backend().buffer();
    // Main's inner area starts at column 21; two filter rows and a date rule come first.
    assert_eq!(buffer[(21, 4)].symbol(), "▌");
    assert_eq!(buffer[(21, 4)].bg, crate::ui::theme::cursor_bg(true));
    assert_eq!(buffer[(21, 5)].symbol(), " ");
    assert_ne!(buffer[(21, 5)].bg, crate::ui::theme::cursor_bg(true));
}

#[test]
fn the_bar_follows_the_selection_over_every_row_of_the_card() {
    let item = with_commit(starred(), "c1", "myuron", "shipped");
    let mut app = app_with(vec![item, notif_json("n1", "starred", 200)]);
    type_str(&mut app, "j");
    let terminal = render(&app);
    let buffer = terminal.backend().buffer();
    for y in 4..8 {
        assert_eq!(buffer[(21, y)].symbol(), " ", "row {y}");
    }
    assert_eq!(buffer[(21, 8)].symbol(), "▌");
}
