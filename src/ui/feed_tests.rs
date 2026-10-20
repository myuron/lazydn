use serde_json::{Value, json};
use time::{Duration, OffsetDateTime};

use crate::action::{Action, Failure};
use crate::app::{App, LogTab};
use crate::feed::{FeedKey, FeedPage};
use crate::feed_tests::{
    FOLLOWING, LATEST, TRENDING, deliver, down, first_req, list_width, started,
};
use crate::model::{Commit, Lane};
use crate::test_support::*;
use crate::ui::feed::{FeedRows, rows};
use crate::ui::theme::{self, PRIMARY};

const SAVED: FeedKey = FeedKey::Saved;
/// Inner width of Main at 120 columns (nav 20 + aside 32 + 2 borders).
const W: usize = 66;

// --- fixtures ---------------------------------------------------------------

fn user_json(handle: &str) -> Value {
    let (name, lane) = match handle {
        "alice" => ("Alice", 1),
        "bob" => ("Bob", 2),
        "carol" => ("Carol", 3),
        "dave" => ("Dave", 4),
        _ => (handle, 0),
    };
    json!({
        "id": format!("user-{handle}"),
        "username": handle,
        "name": name,
        "created_at": "2026-10-01T00:00:00Z",
        "lane": lane,
    })
}

fn commit_json(i: usize, author: &str, created: OffsetDateTime) -> Value {
    json!({
        "id": format!("c{i}"),
        "short_hash": format!("h{i:06}"),
        "body": format!("commit {i}"),
        "parent_id": null,
        "html_url": null,
        "created_at": created.format(&time::format_description::well_known::Rfc3339).unwrap(),
        "stars_count": 1,
        "replies_count": 0,
        "link_preview": null,
        "images": [],
        "author": user_json(author),
        "body_segments": [{"type": "text", "text": format!("commit {i}")}],
        "preview_segments": null,
        "mentions": [],
        "status": "approved",
        "is_pinned": false,
        "starred": false,
        "is_saved": false,
        "pasteis_count": 0,
        "quoted_commit_id": null,
    })
}

fn build(value: Value) -> Commit {
    serde_json::from_value(value).unwrap()
}

fn at(i: usize, author: &str, created: OffsetDateTime) -> Commit {
    build(commit_json(i, author, created))
}

/// A commit `minutes` old at the fixed clock.
fn ago(i: usize, author: &str, minutes: i64) -> Commit {
    at(i, author, NOW - Duration::minutes(minutes))
}

fn pending(i: usize, author: &str) -> Commit {
    let mut value = commit_json(i, author, NOW - Duration::minutes(3));
    value["status"] = json!("pending");
    build(value)
}

fn page_of(items: Vec<Commit>, cursor: Option<&str>) -> FeedPage {
    FeedPage {
        items,
        next_cursor: cursor.map(String::from),
        is_ranked: false,
        suggestions: Vec::new(),
    }
}

fn ranked_of(items: Vec<Commit>) -> FeedPage {
    FeedPage {
        is_ranked: true,
        ..page_of(items, None)
    }
}

/// Alternating authors so that no card is drawn as a same-author follow-up.
fn alternating(n: usize) -> Vec<Commit> {
    (0..n)
        .map(|i| {
            ago(
                i,
                if i % 2 == 0 { "alice" } else { "bob" },
                3 + 2 * i as i64,
            )
        })
        .collect()
}

fn following(w: u16, h: u16, page: FeedPage) -> App {
    let (mut app, effects) = started(w, h);
    deliver(&mut app, FOLLOWING, &effects, page);
    app
}

fn trending(w: u16, h: u16, page: FeedPage) -> App {
    let mut app = signed_in(w, h);
    let effects = type_str(&mut app, "3");
    deliver(&mut app, TRENDING, &effects, page);
    app
}

fn saved(w: u16, h: u16, page: FeedPage) -> App {
    let mut app = signed_in(w, h);
    let effects = type_str(&mut app, "gv");
    deliver(&mut app, SAVED, &effects, page);
    app
}

fn new_commits(app: &mut App, key: FeedKey, count: u32) {
    let since = app.feed(key).items[0].id.clone();
    app.update(Action::NewCommits { key, since, count });
}

fn view(app: &App, key: FeedKey) -> FeedRows {
    rows(app.feed(key), key, app, list_width(app))
}

/// Row texts without trailing blanks, so that padding does not matter.
fn texts(rows: &FeedRows) -> Vec<String> {
    rows.lines
        .iter()
        .map(|line| {
            let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
            text.trim_end().to_string()
        })
        .collect()
}

fn lines_of(app: &App, key: FeedKey) -> Vec<String> {
    texts(&view(app, key))
}

/// Out-of-range reads yield a marker string so that a missing row fails an assertion, not an index panic.
fn row(t: &[String], i: usize) -> String {
    t.get(i).cloned().unwrap_or_else(|| "<missing>".into())
}

fn above(t: &[String], start: usize, n: usize) -> String {
    start
        .checked_sub(n)
        .map_or_else(|| "<missing>".into(), |i| row(t, i))
}

fn top(t: &[String], n: usize) -> Vec<String> {
    t.iter().take(n).cloned().collect()
}

/// Inner column of the composer hint: `min(38, W - 1 - 18)`.
const HINT_COL: usize = 38;

/// Display width, not char count, because the fixed texts hold wide glyphs.
fn assert_fits(t: &[String], width: usize) {
    use unicode_width::UnicodeWidthStr;
    for line in t {
        assert!(line.width() <= width, "{line:?} is {} wide", line.width());
    }
}

fn composer() -> Vec<String> {
    vec![
        " ┆".to_string(),
        " ◌  What did you ship?".to_string(),
        format!(" ┆{}[c] Write a commit", " ".repeat(HINT_COL - 2)),
        " ┆".to_string(),
    ]
}

fn separator(label: &str) -> String {
    let head = format!(" ╎ ── {label} ");
    format!("{head}{}", "─".repeat(W - 1 - head.chars().count()))
}

fn head_marker() -> String {
    let head = " ╌╌ HEAD@{last visit} ";
    format!("{head}{}", "╌".repeat(W - 1 - head.chars().count()))
}

fn centered(text: &str) -> String {
    format!("{}{text}", " ".repeat((W - text.chars().count()) / 2))
}

fn banner(text: &str) -> String {
    centered(text)
}

const RAIL: &str = " │";
const LOADING_MORE: &str = " ┊ (loading more...)";
const ROOT: &str = " ○ That's the root commit. There's nothing before it.";

/// Node glyph of the card header (the character after the marker column).
fn node_of(lines: &[String], start: usize) -> char {
    lines[start].chars().nth(1).unwrap()
}

/// Computes visibility from the card spans, the scroll offset and the 19 list rows of a 120x24 Log.
fn assert_visible_at_120x24(app: &App, key: FeedKey) {
    const VIEW: usize = 19;
    let feed = app.feed(key);
    let card = view(app, key).cards[feed.selected];
    let scroll = feed.scroll as usize;
    assert!(
        scroll <= card.start && card.start + card.height <= scroll + VIEW,
        "card {card:?} not visible: scroll={scroll}"
    );
}

// --- composer and banner ----------------------------------------------------

#[test]
fn signed_in_log_starts_with_the_inline_composer() {
    let app = following(120, 40, page_of(vec![ago(0, "alice", 3)], None));
    assert_eq!(top(&lines_of(&app, FOLLOWING), 4), composer());
}

#[test]
fn composer_is_shown_on_every_signed_in_tab_and_never_to_guests() {
    let app = trending(120, 40, ranked_of(vec![ago(0, "alice", 3)]));
    assert_eq!(top(&lines_of(&app, TRENDING), 4), composer());

    let mut latest = signed_in(120, 40);
    let effects = type_str(&mut latest, "2");
    deliver(
        &mut latest,
        LATEST,
        &effects,
        page_of(vec![ago(0, "alice", 3)], None),
    );
    assert_eq!(top(&lines_of(&latest, LATEST), 4), composer());

    let mut guest_app = guest(120, 40);
    guest_app.update(Action::CloseModal);
    assert_eq!(lines_of(&guest_app, LATEST), Vec::<String>::new());
    assert_eq!(lines_of(&signed_in(120, 40), FOLLOWING), composer());
}

#[test]
fn composer_hint_is_dropped_when_it_does_not_fit() {
    // At 20 columns the hint column would fall before column 3.
    let app = signed_in(120, 40);
    let narrow = rows(app.feed(FOLLOWING), FOLLOWING, &app, 20);
    let t = texts(&narrow);
    assert_eq!(row(&t, 2), " ┆");
    assert_fits(&t, 20);
}

#[test]
fn new_commits_banner_sits_between_composer_and_cards_centered() {
    let mut app = following(120, 40, page_of(vec![ago(0, "alice", 3)], None));
    new_commits(&mut app, FOLLOWING, 3);
    let t = lines_of(&app, FOLLOWING);
    assert_eq!(top(&t, 4), composer());
    assert_eq!(row(&t, 4), banner("↑ 3 new commits"));
    assert_eq!(row(&t, 5), separator("Today"));
}

#[test]
fn new_commits_banner_uses_the_singular_for_one_commit() {
    let mut app = following(120, 40, page_of(vec![ago(0, "alice", 3)], None));
    new_commits(&mut app, FOLLOWING, 1);
    assert_eq!(row(&lines_of(&app, FOLLOWING), 4), banner("↑ 1 new commit"));
}

#[test]
fn new_commits_banner_is_hidden_for_zero_and_for_ranked_feeds() {
    let mut zero = following(120, 40, page_of(vec![ago(0, "alice", 3)], None));
    new_commits(&mut zero, FOLLOWING, 0);
    assert_eq!(row(&lines_of(&zero, FOLLOWING), 4), separator("Today"));

    let mut ranked = trending(120, 40, ranked_of(vec![ago(0, "alice", 3)]));
    new_commits(&mut ranked, TRENDING, 3);
    assert_eq!(ranked.feed(TRENDING).new_commits, 3);
    let t = lines_of(&ranked, TRENDING);
    assert!(t.iter().all(|l| !l.contains("new commit")), "{t:?}");
}

#[test]
fn selected_card_stays_visible_after_the_banner_pushes_cards_down() {
    let mut app = following(120, 24, page_of(alternating(6), None));
    down(&mut app, 3);
    assert_visible_at_120x24(&app, FOLLOWING);

    new_commits(&mut app, FOLLOWING, 3);

    assert_eq!(
        row(&lines_of(&app, FOLLOWING), 4),
        banner("↑ 3 new commits")
    );
    assert_visible_at_120x24(&app, FOLLOWING);
}

// --- date separators ----------------------------------------------------------

#[test]
fn day_separator_precedes_the_first_card_even_when_it_is_from_today() {
    let app = following(120, 40, page_of(vec![ago(0, "alice", 3)], None));
    let t = lines_of(&app, FOLLOWING);
    assert_eq!(row(&t, 4), separator("Today"));
    assert_eq!(row(&t, 5), RAIL);
}

#[test]
fn day_separators_are_inserted_only_where_the_day_changes() {
    let items = vec![
        at(0, "alice", NOW - Duration::hours(1)),
        at(1, "bob", NOW - Duration::hours(2)),
        at(2, "alice", NOW - Duration::hours(25)),
        at(3, "bob", NOW - Duration::hours(26)),
        at(4, "alice", NOW - Duration::days(8)),
    ];
    let app = following(120, 40, page_of(items, None));
    let v = view(&app, FOLLOWING);
    let t = texts(&v);
    let before = |i: usize| above(&t, v.cards[i].start, 2).clone();
    assert_eq!(before(0), separator("Today"));
    assert_ne!(before(1), separator("Today"));
    assert_eq!(before(2), separator("Yesterday"));
    assert!(!before(3).starts_with(" ╎"), "{:?}", before(3));
    assert_eq!(before(4), separator("Mon, Oct 12"));
    assert_eq!(t.iter().filter(|l| l.starts_with(" ╎")).count(), 3);
}

#[test]
fn ranked_feed_has_no_separator_head_marker_or_banner() {
    let mut app = trending(
        120,
        40,
        ranked_of(vec![
            ago(0, "alice", 3),
            ago(1, "bob", 5),
            ago(2, "carol", 7),
        ]),
    );
    visit(&mut app, LogTab::Trending, "c1");
    new_commits(&mut app, TRENDING, 3);
    let t = lines_of(&app, TRENDING);
    assert_eq!(top(&t, 4), composer());
    assert_eq!(row(&t, 4), RAIL);
    for text in &t {
        assert!(
            !text.contains('╎') && !text.contains("HEAD@") && !text.contains("new commit"),
            "{text:?}"
        );
    }
}

// --- rails and nodes ----------------------------------------------------------

#[test]
fn every_card_is_preceded_by_a_rail_row() {
    let items = vec![
        ago(0, "alice", 3),
        ago(1, "bob", 5),
        at(2, "carol", NOW - Duration::hours(30)),
    ];
    let mut app = following(120, 40, page_of(items, None));
    visit(&mut app, LogTab::Following, "c1");
    let v = view(&app, FOLLOWING);
    let t = texts(&v);
    for card in &v.cards {
        assert_eq!(above(&t, card.start, 1), RAIL, "{card:?}");
    }
}

#[test]
fn consecutive_posts_by_one_author_use_the_same_author_node() {
    let items = vec![
        ago(0, "alice", 3),
        ago(1, "alice", 4),
        ago(2, "bob", 5),
        ago(3, "alice", 6),
    ];
    let app = following(120, 40, page_of(items, None));
    let v = view(&app, FOLLOWING);
    let t = texts(&v);
    let nodes: Vec<char> = v.cards.iter().map(|c| node_of(&t, c.start)).collect();
    // The first card is selected, so its marker occupies column 0 and the node stays in column 1.
    assert_eq!(nodes, vec!['◉', '•', '◉', '◉']);
}

#[test]
fn same_author_after_a_day_change_keeps_the_normal_node() {
    let items = vec![
        at(0, "alice", NOW - Duration::hours(1)),
        at(1, "alice", NOW - Duration::hours(25)),
    ];
    let app = following(120, 40, page_of(items, None));
    let v = view(&app, FOLLOWING);
    let t = texts(&v);
    assert_eq!(above(&t, v.cards[1].start, 2), separator("Yesterday"));
    assert_eq!(node_of(&t, v.cards[1].start), '◉');
}

#[test]
fn same_author_right_after_the_head_marker_keeps_the_normal_node() {
    let mut app = following(
        120,
        40,
        page_of(vec![ago(0, "alice", 3), ago(1, "alice", 4)], None),
    );
    visit(&mut app, LogTab::Following, "c1");
    let v = view(&app, FOLLOWING);
    let t = texts(&v);
    assert_eq!(above(&t, v.cards[1].start, 2), head_marker());
    assert_eq!(node_of(&t, v.cards[1].start), '◉');
}

#[test]
fn rail_row_takes_the_color_of_the_card_it_leads_into() {
    let app = following(
        120,
        40,
        page_of(vec![ago(0, "alice", 3), ago(1, "bob", 5)], None),
    );
    let v = view(&app, FOLLOWING);
    let terminal = render(&app);
    let buffer = terminal.backend().buffer();
    // Main inner starts at x=21 and the list at y=3; the rail glyph sits in the second column.
    let rail_cell = |card: usize| {
        let y = 3 + (v.cards[card].start - 1) as u16;
        buffer[(22, y)].clone()
    };
    assert_eq!(rail_cell(0).symbol(), "│");
    assert_eq!(rail_cell(0).fg, theme::lane_color(Lane::Index(1), true));
    assert_eq!(rail_cell(1).symbol(), "│");
    assert_eq!(rail_cell(1).fg, theme::lane_color(Lane::Index(2), true));
}

// --- head marker -----------------------------------------------------------------

#[test]
fn head_marker_precedes_the_last_visit_commit() {
    let mut app = following(
        120,
        40,
        page_of(
            vec![ago(0, "alice", 3), ago(1, "bob", 5), ago(2, "carol", 7)],
            None,
        ),
    );
    visit(&mut app, LogTab::Following, "c1");
    let v = view(&app, FOLLOWING);
    let t = texts(&v);
    assert_eq!(above(&t, v.cards[1].start, 2), head_marker());
    assert_eq!(above(&t, v.cards[1].start, 1), RAIL);
    assert_eq!(t.iter().filter(|l| l.contains("HEAD@")).count(), 1);
}

#[test]
fn head_marker_is_not_drawn_before_the_first_card() {
    let mut app = following(
        120,
        40,
        page_of(vec![ago(0, "alice", 3), ago(1, "bob", 5)], None),
    );
    visit(&mut app, LogTab::Following, "c0");
    let t = lines_of(&app, FOLLOWING);
    assert!(t.iter().all(|l| !l.contains("HEAD@")), "{t:?}");
    assert_eq!(row(&t, 4), separator("Today"));
}

#[test]
fn head_marker_is_not_drawn_without_a_matching_commit() {
    let mut app = following(
        120,
        40,
        page_of(vec![ago(0, "alice", 3), ago(1, "bob", 5)], None),
    );
    visit(&mut app, LogTab::Following, "gone");
    let t = lines_of(&app, FOLLOWING);
    assert!(t.iter().all(|l| !l.contains("HEAD@")), "{t:?}");
    assert_eq!(row(&t, 4), separator("Today"));
}

// --- Saved list -------------------------------------------------------------------

#[test]
fn saved_list_has_no_composer_separator_head_marker_or_banner() {
    let mut app = saved(
        120,
        40,
        page_of(
            vec![ago(0, "alice", 3), ago(1, "bob", 5), ago(2, "carol", 7)],
            None,
        ),
    );
    visit(&mut app, LogTab::Following, "c1");
    new_commits(&mut app, SAVED, 3);
    let v = view(&app, SAVED);
    let t = texts(&v);
    assert_eq!(row(&t, 0), RAIL);
    assert_eq!(v.cards[0].start, 1);
    assert_eq!(above(&t, v.cards[1].start, 1), RAIL);
    assert!(
        above(&t, v.cards[1].start, 2).contains('⚑'),
        "card directly follows the previous one"
    );
    for text in &t {
        assert!(
            !text.contains('╎') && !text.contains("HEAD@") && !text.contains("new commit"),
            "{text:?}"
        );
        assert!(!text.contains("What did you ship?"), "{text:?}");
    }
    assert_eq!(t.last().map(String::as_str), Some(ROOT));
}

#[test]
fn empty_saved_list_shows_a_centered_message_once_loaded() {
    assert_eq!(lines_of(&signed_in(120, 40), SAVED), Vec::<String>::new());

    let loaded = saved(120, 40, page_of(Vec::new(), None));
    assert_eq!(
        lines_of(&loaded, SAVED),
        vec![
            String::new(),
            String::new(),
            centered("fatal: no saved commits yet"),
            String::new(),
            centered("press b on any commit to save it"),
        ]
    );
}

// --- selection ----------------------------------------------------------------------

#[test]
fn only_card_rows_of_the_selected_commit_carry_the_marker() {
    let mut app = following(120, 40, page_of(alternating(3), None));
    down(&mut app, 1);
    let v = view(&app, FOLLOWING);
    let t = texts(&v);
    for (i, card) in v.cards.iter().enumerate() {
        for line in &t[card.start..card.start + card.height] {
            let marker = line.chars().next().unwrap();
            assert_eq!(marker, if i == 1 { '▌' } else { ' ' }, "card {i}: {line:?}");
        }
        assert_eq!(above(&t, card.start, 1).chars().next(), Some(' '));
    }
}

#[test]
fn own_pending_commit_shows_the_in_review_badge_and_others_do_not() {
    let app = following(
        120,
        40,
        page_of(vec![pending(0, "myuron"), pending(1, "alice")], None),
    );
    let v = view(&app, FOLLOWING);
    let t = texts(&v);
    assert_eq!(above(&t, v.cards[0].start, 1), RAIL);
    assert!(
        t[v.cards[0].start].contains("[in review]"),
        "{:?}",
        t[v.cards[0].start]
    );
    assert!(
        !t[v.cards[1].start].contains("[in review]"),
        "{:?}",
        t[v.cards[1].start]
    );
}

// --- end of list ---------------------------------------------------------------------

#[test]
fn loading_more_is_shown_at_the_end_while_the_next_page_is_pending() {
    let mut app = following(120, 40, page_of(alternating(3), Some("next")));
    type_str(&mut app, "gg");
    assert_eq!(
        app.feed(FOLLOWING).pending.as_ref().map(|p| p.append),
        Some(true)
    );
    let t = lines_of(&app, FOLLOWING);
    assert_eq!(t.last().map(String::as_str), Some(LOADING_MORE));
    assert!(!t.iter().any(|l| l == ROOT));
}

#[test]
fn idle_list_with_a_next_cursor_ends_with_the_last_card() {
    let app = following(120, 40, page_of(alternating(3), Some("next")));
    let v = view(&app, FOLLOWING);
    let last = v.cards.last().unwrap();
    assert_eq!(top(&texts(&v), 4), composer());
    assert_eq!(v.lines.len(), last.start + last.height);
    let t = texts(&v);
    assert!(!t.iter().any(|l| l == ROOT || l == LOADING_MORE));
}

#[test]
fn root_commit_line_closes_a_list_without_a_next_cursor() {
    let app = following(120, 40, page_of(alternating(3), None));
    let t = lines_of(&app, FOLLOWING);
    assert_eq!(t.last().map(String::as_str), Some(ROOT));
    assert!(
        t[t.len() - 2].contains('⚑'),
        "root line follows the last card directly"
    );
}

#[test]
fn failed_append_shows_the_retry_hint_after_the_last_card() {
    let mut app = following(120, 40, page_of(alternating(3), Some("next")));
    let effects = type_str(&mut app, "gg");
    app.update(Action::FeedFailed {
        key: FOLLOWING,
        req: first_req(&effects),
        failure: Failure::new("rate_limited"),
    });
    let t = lines_of(&app, FOLLOWING);
    assert_eq!(
        t.last().map(String::as_str),
        Some(" error: rate_limited — press R to retry")
    );
    assert!(!t.iter().any(|l| l == LOADING_MORE || l == ROOT));
}

#[test]
fn failed_refresh_keeps_the_list_without_an_error_line() {
    let mut app = following(120, 40, page_of(alternating(2), None));
    let effects = type_str(&mut app, "R");
    app.update(Action::FeedFailed {
        key: FOLLOWING,
        req: first_req(&effects),
        failure: Failure::new("http_500"),
    });
    let t = lines_of(&app, FOLLOWING);
    assert!(t.iter().all(|l| !l.contains("error:")), "{t:?}");
    assert_eq!(t.last().map(String::as_str), Some(ROOT));
}

#[test]
fn refresh_in_flight_over_a_list_shows_no_loading_line() {
    let mut app = following(120, 40, page_of(alternating(2), Some("next")));
    type_str(&mut app, "R");
    let t = lines_of(&app, FOLLOWING);
    assert_eq!(top(&t, 4), composer());
    assert!(t.iter().all(|l| !l.contains("(loading")), "{t:?}");
}

// --- empty and error states -----------------------------------------------------------

#[test]
fn first_load_in_flight_shows_loading_below_the_composer() {
    let (app, _) = started(120, 40);
    let mut expected = composer();
    expected.push(" ┊ (loading...)".to_string());
    assert_eq!(lines_of(&app, FOLLOWING), expected);
}

#[test]
fn feed_not_requested_yet_shows_only_the_composer() {
    assert_eq!(lines_of(&signed_in(120, 40), FOLLOWING), composer());
}

#[test]
fn first_load_failure_shows_the_error_with_its_code() {
    let (mut app, effects) = started(120, 40);
    app.update(Action::FeedFailed {
        key: FOLLOWING,
        req: first_req(&effects),
        failure: Failure::new("http_500"),
    });
    let mut expected = composer();
    expected.push(" error: http_500 — press R to retry".to_string());
    assert_eq!(lines_of(&app, FOLLOWING), expected);
}

#[test]
fn empty_following_with_suggestions_lists_them_under_the_who_to_follow_heading() {
    let page = FeedPage {
        suggestions: vec![ago(10, "carol", 60), ago(11, "dave", 120)],
        ..page_of(Vec::new(), None)
    };
    let app = following(120, 40, page);
    let t = lines_of(&app, FOLLOWING);
    assert_eq!(top(&t, 4), composer());
    assert_eq!(row(&t, 4), " $ who --should-i-follow");
    assert_eq!(row(&t, 5), RAIL);
    assert!(
        row(&t, 6).starts_with(" ◉ Carol @carol · 1h"),
        "{:?}",
        row(&t, 6)
    );
    assert_eq!(row(&t, 9), RAIL);
    assert!(
        row(&t, 10).starts_with(" ◉ Dave @dave · 2h"),
        "{:?}",
        row(&t, 10)
    );
    assert_eq!(t.len(), 13);
    assert!(
        t.iter()
            .all(|l| !l.starts_with('▌') && !l.contains("fatal:"))
    );
}

#[test]
fn empty_following_without_suggestions_is_fatal() {
    let app = following(120, 40, page_of(Vec::new(), None));
    let mut expected = composer();
    expected.push(" fatal: your log does not have any commits yet".to_string());
    assert_eq!(lines_of(&app, FOLLOWING), expected);
}

#[test]
fn empty_latest_and_trending_are_fatal_even_with_suggestions() {
    let with_suggestions = |ranked: bool| FeedPage {
        is_ranked: ranked,
        suggestions: vec![ago(10, "carol", 60)],
        ..page_of(Vec::new(), None)
    };
    let mut expected = composer();
    expected.push(" fatal: your log does not have any commits yet".to_string());

    let mut latest = signed_in(120, 40);
    let effects = type_str(&mut latest, "2");
    deliver(&mut latest, LATEST, &effects, with_suggestions(false));
    assert_eq!(lines_of(&latest, LATEST), expected);

    let ranked = trending(120, 40, with_suggestions(true));
    assert_eq!(lines_of(&ranked, TRENDING), expected);
}

#[test]
fn empty_list_while_reloading_shows_loading_instead_of_fatal() {
    let mut app = following(120, 40, page_of(Vec::new(), None));
    type_str(&mut app, "R");
    let t = lines_of(&app, FOLLOWING);
    assert_eq!(row(&t, 4), " ┊ (loading...)");
    assert!(t.iter().all(|l| !l.contains("fatal:")));
}

// --- style ----------------------------------------------------------------------------

#[test]
fn new_commits_banner_is_drawn_in_the_primary_color() {
    let mut app = following(120, 40, page_of(vec![ago(0, "alice", 3)], None));
    new_commits(&mut app, FOLLOWING, 3);
    let terminal = render(&app);
    let buffer = terminal.backend().buffer();
    // Banner is the fifth list line: y = 3 + 4; its text starts at x = 21 + 25.
    assert_eq!(buffer[(46, 7)].symbol(), "↑");
    for x in 46..61 {
        if buffer[(x, 7)].symbol() != " " {
            assert_eq!(buffer[(x, 7)].fg, PRIMARY, "x={x}");
        }
    }
}

// --- snapshots --------------------------------------------------------------------------

macro_rules! snapshot {
    ($name:literal, $app:expr) => {{
        let terminal = render(&$app);
        insta::assert_snapshot!($name, terminal.backend());
    }};
}

/// Three cards with a same-author follow-up, the banner, the HEAD marker and a page in flight.
fn feed_fixture(w: u16, h: u16) -> App {
    let items = vec![ago(0, "alice", 3), ago(1, "alice", 5), ago(2, "bob", 25)];
    let mut app = following(w, h, page_of(items, Some("next")));
    visit(&mut app, LogTab::Following, "c2");
    new_commits(&mut app, FOLLOWING, 3);
    type_str(&mut app, "gg");
    app
}

#[test]
fn l1_wide_feed_with_composer_banner_separator_head_and_loading_more() {
    snapshot!("log_wide_feed", feed_fixture(120, 40));
}

#[test]
fn l2_medium_feed_with_the_same_data() {
    snapshot!("log_medium_feed", feed_fixture(80, 30));
}

#[test]
fn l3_narrow_feed_with_the_same_data() {
    snapshot!("log_narrow_feed", feed_fixture(60, 30));
}

#[test]
fn l4_trending_without_separators_banner_or_head_marker() {
    let mut app = trending(
        120,
        30,
        FeedPage {
            next_cursor: Some("next".into()),
            ..ranked_of(vec![
                ago(0, "alice", 3),
                ago(1, "bob", 25),
                ago(2, "carol", 60),
            ])
        },
    );
    visit(&mut app, LogTab::Trending, "c1");
    new_commits(&mut app, TRENDING, 3);
    snapshot!("log_trending", app);
}

#[test]
fn l5_list_ends_with_the_root_commit_line() {
    let app = following(
        120,
        30,
        page_of(vec![ago(0, "alice", 3), ago(1, "bob", 25)], None),
    );
    snapshot!("log_root_end", app);
}

#[test]
fn l6_empty_following_lists_suggestions() {
    let page = FeedPage {
        suggestions: vec![ago(10, "carol", 60), ago(11, "dave", 120)],
        ..page_of(Vec::new(), None)
    };
    snapshot!("log_empty_suggestions", following(120, 30, page));
}

#[test]
fn l7_empty_following_is_fatal() {
    snapshot!(
        "log_empty_fatal",
        following(120, 24, page_of(Vec::new(), None))
    );
}

#[test]
fn l8_first_load_failure_shows_the_retry_hint() {
    let (mut app, effects) = started(120, 24);
    app.update(Action::FeedFailed {
        key: FOLLOWING,
        req: first_req(&effects),
        failure: Failure::new("rate_limited"),
    });
    snapshot!("log_first_error", app);
}

#[test]
fn l9_selection_near_the_bottom_scrolls_the_list_under_fixed_tabs() {
    let mut app = following(120, 24, page_of(alternating(8), None));
    down(&mut app, 7);
    assert_eq!(app.feed(FOLLOWING).selected, 7);
    snapshot!("log_scrolled", app);
}

#[test]
fn s1_saved_list_has_no_composer_or_separators() {
    let app = saved(
        120,
        30,
        page_of(
            vec![ago(0, "alice", 3), ago(1, "bob", 25), ago(2, "carol", 60)],
            None,
        ),
    );
    snapshot!("saved_list", app);
}

#[test]
fn s2_empty_saved_shows_the_centered_message() {
    snapshot!("saved_empty", saved(120, 24, page_of(Vec::new(), None)));
}

// --- review follow-ups (rev.5) ------------------------------------------------

#[test]
fn ranked_feed_never_uses_the_same_author_node() {
    let app = trending(
        120,
        40,
        ranked_of(vec![ago(0, "alice", 3), ago(1, "alice", 4)]),
    );
    let v = view(&app, TRENDING);
    let t = texts(&v);
    assert_eq!(top(&t, 4), composer());
    assert_eq!(node_of(&t, v.cards[1].start), '◉');
}

#[test]
fn saved_list_never_uses_the_same_author_node() {
    let app = saved(
        120,
        40,
        page_of(vec![ago(0, "alice", 3), ago(1, "alice", 4)], None),
    );
    let v = view(&app, SAVED);
    let t = texts(&v);
    assert_eq!(row(&t, 0), RAIL);
    assert_eq!(node_of(&t, v.cards[1].start), '◉');
}

#[test]
fn separator_and_head_marker_stack_before_the_same_card() {
    let items = vec![
        at(0, "alice", NOW - Duration::hours(1)),
        at(1, "bob", NOW - Duration::hours(25)),
    ];
    let mut app = following(120, 40, page_of(items, None));
    visit(&mut app, LogTab::Following, "c1");
    let v = view(&app, FOLLOWING);
    let t = texts(&v);
    let start = v.cards[1].start;
    assert_eq!(above(&t, start, 3), separator("Yesterday"));
    assert_eq!(above(&t, start, 2), head_marker());
    assert_eq!(above(&t, start, 1), RAIL);
}

#[test]
fn separator_and_head_lines_never_exceed_an_extremely_narrow_width() {
    let mut app = following(
        120,
        40,
        page_of(vec![ago(0, "alice", 3), ago(1, "bob", 4)], None),
    );
    visit(&mut app, LogTab::Following, "c1");
    let narrow = rows(app.feed(FOLLOWING), FOLLOWING, &app, 10);
    let t = texts(&narrow);
    let marks: Vec<&String> = t
        .iter()
        .filter(|l| l.starts_with(" ╎") || l.starts_with(" ╌"))
        .collect();
    assert_eq!(marks.len(), 2, "{t:?}");
    assert!(marks.iter().all(|l| l.chars().count() <= 10), "{marks:?}");
}

#[test]
fn every_state_keeps_all_rows_within_a_20_column_list() {
    const NARROW: usize = 20;
    let rows_at = |app: &App, key: FeedKey| texts(&rows(app.feed(key), key, app, NARROW as u16));

    let (loading, _) = started(120, 40);
    assert_eq!(row(&rows_at(&loading, FOLLOWING), 0), " ┆");
    assert_fits(&rows_at(&loading, FOLLOWING), NARROW);

    let (mut failed, effects) = started(120, 40);
    failed.update(Action::FeedFailed {
        key: FOLLOWING,
        req: first_req(&effects),
        failure: Failure::new("rate_limited"),
    });
    assert_fits(&rows_at(&failed, FOLLOWING), NARROW);

    let fatal = following(120, 40, page_of(Vec::new(), None));
    assert_fits(&rows_at(&fatal, FOLLOWING), NARROW);

    let suggestions = following(
        120,
        40,
        FeedPage {
            suggestions: vec![ago(10, "carol", 60)],
            ..page_of(Vec::new(), None)
        },
    );
    assert_fits(&rows_at(&suggestions, FOLLOWING), NARROW);

    let mut listed = following(120, 40, page_of(alternating(3), Some("next")));
    new_commits(&mut listed, FOLLOWING, 3);
    let effects = type_str(&mut listed, "gg");
    assert_fits(&rows_at(&listed, FOLLOWING), NARROW);
    listed.update(Action::FeedFailed {
        key: FOLLOWING,
        req: first_req(&effects),
        failure: Failure::new("rate_limited"),
    });
    assert_fits(&rows_at(&listed, FOLLOWING), NARROW);

    let root = following(120, 40, page_of(alternating(2), None));
    assert_fits(&rows_at(&root, FOLLOWING), NARROW);

    let saved_empty = saved(120, 40, page_of(Vec::new(), None));
    assert_fits(&rows_at(&saved_empty, SAVED), NARROW);
}

// --- closing rows and local days ------------------------------------------------

/// Last row of the list must be inside the 19 list rows of a 120x24 Log.
fn assert_last_row_in_view_at_120x24(app: &App) {
    let total = view(app, FOLLOWING).lines.len();
    let scroll = app.feed(FOLLOWING).scroll as usize;
    assert!(scroll + 19 >= total, "scroll={scroll} total={total}");
}

#[test]
fn root_line_scrolls_into_view_when_jumping_to_the_end_of_a_long_list() {
    let mut app = following(120, 24, page_of(alternating(30), None));
    type_str(&mut app, "G");
    assert_eq!(
        lines_of(&app, FOLLOWING).last().map(String::as_str),
        Some(ROOT)
    );
    assert_last_row_in_view_at_120x24(&app);
}

#[test]
fn append_failure_line_scrolls_into_view_at_the_end_of_a_long_list() {
    let mut app = following(120, 24, page_of(alternating(30), Some("next")));
    let effects = type_str(&mut app, "G");
    app.update(Action::FeedFailed {
        key: FOLLOWING,
        req: first_req(&effects),
        failure: Failure::new("rate_limited"),
    });
    assert_eq!(
        lines_of(&app, FOLLOWING).last().map(String::as_str),
        Some(" error: rate_limited — press R to retry")
    );
    assert_last_row_in_view_at_120x24(&app);
}

fn following_at_offset(offset: time::UtcOffset, items: Vec<Commit>) -> App {
    let mut app = App::new(crate::app::Session::SignedIn(fixture_me()), NOW, offset);
    let effects = app.update(Action::Init);
    app.update(Action::Resize(120, 40));
    deliver(&mut app, FOLLOWING, &effects, page_of(items, None));
    app
}

#[test]
fn days_are_split_by_the_local_offset_not_by_utc() {
    let jst = time::UtcOffset::from_hms(9, 0, 0).unwrap();

    // Same UTC day, different day in +09:00.
    let split_in_jst = vec![
        at(0, "alice", time::macros::datetime!(2026-10-20 16:00 UTC)),
        at(1, "bob", time::macros::datetime!(2026-10-20 14:00 UTC)),
    ];
    let app = following_at_offset(jst, split_in_jst.clone());
    let t = lines_of(&app, FOLLOWING);
    assert_eq!(t.iter().filter(|l| l.starts_with(" ╎")).count(), 2, "{t:?}");
    let utc = following_at_offset(time::UtcOffset::UTC, split_in_jst);
    let t = lines_of(&utc, FOLLOWING);
    assert_eq!(t.iter().filter(|l| l.starts_with(" ╎")).count(), 1, "{t:?}");

    // Different UTC days, same day in +09:00.
    let same_in_jst = vec![
        at(0, "alice", time::macros::datetime!(2026-10-20 02:00 UTC)),
        at(1, "bob", time::macros::datetime!(2026-10-19 20:00 UTC)),
    ];
    let app = following_at_offset(jst, same_in_jst);
    let t = lines_of(&app, FOLLOWING);
    assert_eq!(t.iter().filter(|l| l.starts_with(" ╎")).count(), 1, "{t:?}");
}

// --- failed refresh over a list (M5a): the toast carries the failure -------------------

#[test]
fn failed_refresh_over_a_list_with_a_next_cursor_shows_neither_root_nor_error_line() {
    let mut app = following(120, 40, page_of(alternating(2), Some("next")));
    let effects = type_str(&mut app, "R");
    app.update(Action::FeedFailed {
        key: FOLLOWING,
        req: first_req(&effects),
        failure: Failure::new("http_500"),
    });
    let t = lines_of(&app, FOLLOWING);
    assert!(
        t.iter()
            .all(|l| !l.contains("error:") && l != ROOT && l != LOADING_MORE),
        "{t:?}"
    );
    assert_eq!(app.toast().map(|t| t.text.as_str()), Some("http_500"));
}

#[test]
fn failed_refresh_over_the_last_page_keeps_the_root_line_and_shows_no_error_line() {
    let mut app = following(120, 40, page_of(alternating(2), None));
    let effects = type_str(&mut app, "R");
    app.update(Action::FeedFailed {
        key: FOLLOWING,
        req: first_req(&effects),
        failure: Failure::new("http_500"),
    });
    let t = lines_of(&app, FOLLOWING);
    assert!(t.iter().all(|l| !l.contains("error:")), "{t:?}");
    assert_eq!(t.last().map(String::as_str), Some(ROOT));
    assert_eq!(app.toast().map(|t| t.text.as_str()), Some("http_500"));
}

fn visit(app: &mut App, tab: LogTab, id: &str) {
    app.update(Action::SetLastVisit { tab, id: id.into() });
}

// --- ASCII mode ----------------------------------------------------------------------------

#[test]
fn as1_log_ascii_at_80x24() {
    snapshot!("log_ascii", with_ascii(feed_fixture(80, 24)));
}

#[test]
fn the_log_list_holds_only_ascii_in_ascii_mode() {
    let terminal = render(&with_ascii(feed_fixture(80, 24)));
    assert_eq!(
        non_ascii_cells(&terminal, (5, 79), (3, 22)),
        Vec::<String>::new()
    );
}

#[test]
fn the_saved_list_holds_only_ascii_in_ascii_mode() {
    let app = with_ascii(saved(
        120,
        30,
        page_of(
            vec![ago(0, "alice", 3), ago(1, "bob", 25), ago(2, "carol", 60)],
            None,
        ),
    ));
    let terminal = render(&app);
    assert_eq!(
        non_ascii_cells(&terminal, (21, 87), (1, 28)),
        Vec::<String>::new()
    );
}

#[test]
fn the_loading_row_and_the_root_row_are_ascii_in_ascii_mode() {
    let mut app = with_ascii(following(
        80,
        24,
        page_of(vec![ago(0, "alice", 3)], Some("next")),
    ));
    let loading = render(&app);
    assert_eq!(
        non_ascii_cells(&loading, (5, 79), (3, 22)),
        Vec::<String>::new()
    );
    app = with_ascii(following(80, 24, page_of(vec![ago(0, "alice", 3)], None)));
    let root = render(&app);
    assert_eq!(
        non_ascii_cells(&root, (5, 79), (3, 22)),
        Vec::<String>::new()
    );
}

#[test]
fn the_hints_of_the_status_line_are_ascii_in_ascii_mode() {
    let terminal = render(&with_ascii(feed_fixture(80, 24)));
    let row = row_text(terminal.backend().buffer(), 23);
    assert!(
        row.starts_with(" Log > Following  j/k move  Enter open  c commit  ? help"),
        "{row:?}"
    );
}

#[test]
fn the_retry_hint_of_a_failed_list_is_ascii_in_ascii_mode() {
    let (mut app, effects) = started(80, 24);
    app.update(Action::FeedFailed {
        key: FOLLOWING,
        req: first_req(&effects),
        failure: Failure::new("rate_limited"),
    });
    let app = with_ascii(app);
    let terminal = render(&app);
    assert_eq!(
        non_ascii_cells(&terminal, (5, 79), (3, 22)),
        Vec::<String>::new()
    );
    let buffer = terminal.backend().buffer();
    let text: Vec<String> = (3..22).map(|y| row_text(buffer, y)).collect();
    assert!(
        text.iter()
            .any(|r| r.contains("error: rate_limited - press R to retry")),
        "{text:#?}"
    );
}

#[test]
fn what_the_author_wrote_is_not_touched_in_ascii_mode() {
    let body = "wait… a │ b ◉ c ╎ d ─ e";
    let mut value = commit_json(0, "alice", NOW - Duration::minutes(3));
    value["body"] = json!(body);
    value["body_segments"] = json!([{"type": "text", "text": body}]);
    let commit = build(value);
    let shown = |ascii: bool| {
        let app = following(120, 24, page_of(vec![commit.clone()], None));
        let app = if ascii { with_ascii(app) } else { app };
        let terminal = render(&app);
        let buffer = terminal.backend().buffer();
        (0..24)
            .map(|y| row_text(buffer, y))
            .find(|r| r.contains("wait"))
            .unwrap_or_default()
    };
    assert!(shown(false).contains(body), "{}", shown(false));
    assert!(shown(true).contains(body), "{}", shown(true));
}
