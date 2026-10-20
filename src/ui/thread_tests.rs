use crossterm::event::KeyCode;
use serde_json::{Value, json};
use unicode_width::UnicodeWidthStr;

use crate::action::{Action, Effect, Failure};
use crate::app::{App, Screen};
use crate::feed::{FeedKey, FeedPage};
use crate::feed_tests::{FOLLOWING, deliver, list_width, started};
use crate::model::{Commit, ThreadItem, ThreadResponse};
use crate::test_support::*;
use crate::thread_tests::*;
use crate::ui::feed::rows as feed_rows;
use crate::ui::statusline::breadcrumb;
use crate::ui::thread::{ThreadRows, rows};

/// Long enough that the short hash is the last 7 characters without hyphens.
const LONG: &str = "9f8e7d6c-5b4a-4321-8000-00000a1b2c3d";

// --- rendering helpers ------------------------------------------------------

fn thread_id(app: &App) -> String {
    match app.screen() {
        Screen::Thread(id) => id,
        other => panic!("not a thread screen: {other:?}"),
    }
}

fn view(app: &App) -> ThreadRows {
    rows(app, &thread_id(app), list_width(app))
}

fn text_of(line: &ratatui::text::Line) -> String {
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    text.trim_end().to_string()
}

/// Row texts without trailing blanks, so that padding does not matter.
fn lines(app: &App) -> Vec<String> {
    view(app).lines.iter().map(text_of).collect()
}

/// Fixture hashes are `00000xx`; the right-aligned hash is dropped so that the tests stay about structure.
fn without_hash(line: &str) -> String {
    match line.rsplit_once(' ') {
        Some((head, last))
            if last.len() == 7 && last.starts_with("00000") && head.ends_with(' ') =>
        {
            head.trim_end().to_string()
        }
        _ => line.to_string(),
    }
}

fn is_action_row(line: &str) -> bool {
    line.contains("↩ Reply")
}

/// The rows below the copy-link header, without action rows, hashes and the selection marker.
fn skeleton(app: &App) -> Vec<String> {
    lines(app)
        .into_iter()
        .skip(1)
        .filter(|l| !is_action_row(l))
        .map(|l| format!(" {}", without_hash(&l).chars().skip(1).collect::<String>()))
        .collect()
}

fn from_line(sk: &[String], needle: &str) -> Vec<String> {
    let at = sk.iter().position(|l| l.contains(needle));
    assert!(at.is_some(), "no row contains {needle:?}: {sk:#?}");
    sk[at.unwrap_or(0)..].to_vec()
}

fn marked(app: &App) -> Vec<String> {
    lines(app)
        .into_iter()
        .filter(|l| l.starts_with('▌') && !is_action_row(l))
        .map(|l| without_hash(&l))
        .collect()
}

fn strings(rows: &[&str]) -> Vec<String> {
    rows.iter().map(|s| s.to_string()).collect()
}

fn j(app: &mut App, n: usize) {
    for _ in 0..n {
        press(app, KeyCode::Char('j'));
    }
}

/// Presses `o` and answers the child request that it makes.
fn expand(app: &mut App, parent: &str, items: Vec<ThreadItem>, cursor: Option<&str>) {
    let effects = press(app, KeyCode::Char('o'));
    deliver_replies(app, &effects, parent, items, cursor);
}

// --- fixtures ---------------------------------------------------------------

fn rc(item: ThreadItem, n: u32) -> ThreadItem {
    with_replies_count(item, n)
}

/// Bob (1 child) and Carol, with Bob's chain opened to a fourth level.
fn nested() -> App {
    let mut app = opened(
        120,
        50,
        vec![],
        2,
        vec![rc(item("r1", "bob"), 1), item("r2", "carol")],
        None,
    );
    j(&mut app, 1);
    expand(&mut app, "r1", vec![rc(child("x1", "dan", "r1"), 1)], None);
    j(&mut app, 1);
    expand(&mut app, "x1", vec![rc(child("y1", "erin", "x1"), 1)], None);
    j(&mut app, 1);
    expand(&mut app, "y1", vec![child("z1", "frank", "y1")], None);
    app
}

const MAIN_CARD: [&str; 3] = [
    " ◉ Alice @alice",
    " │ body of m1",
    " │ Oct 20, 2026 11:00 · commit 00000m1",
];

fn main_block() -> Vec<String> {
    let mut out = strings(&[" │"]);
    out.extend(strings(&MAIN_CARD));
    out
}

// --- 11. layout: nesting ----------------------------------------------------

#[test]
fn replies_nest_by_branch_prefixes_to_three_levels_and_flatten_below() {
    let app = nested();
    let mut want = main_block();
    want.extend(strings(&[
        " │",
        " ├─● Bob @bob · 10m",
        " │ │   body of r1",
        " │ └─● Dan @dan · 5m",
        " │   │   body of x1",
        " │   ├─● Erin @erin · 5m",
        " │   │ │   body of y1",
        " │   └─● Frank @frank · 5m",
        " │         ↳ replying to @erin",
        " │         body of z1",
        " └─● Carol @carol · 10m",
        "       body of r2",
    ]));
    assert_eq!(skeleton(&app), want);
}

#[test]
fn an_unopened_reply_with_children_shows_a_replies_hint_for_commits_and_tombstones() {
    let app = opened(
        120,
        50,
        vec![],
        3,
        vec![
            rc(item("r1", "bob"), 3),
            rc(tomb("t1"), 2),
            item("r2", "carol"),
        ],
        None,
    );
    let mut want = main_block();
    want.extend(strings(&[
        " │",
        " ├─● Bob @bob · 10m",
        " │ │   body of r1",
        " │ ┊ 3 replies (o)",
        " ├─◌ [deleted] This commit was deleted.",
        " │ ┊ 2 replies (o)",
        " └─● Carol @carol · 10m",
        "       body of r2",
    ]));
    assert_eq!(skeleton(&app), want);
}

#[test]
fn opening_a_reply_replaces_its_replies_hint_with_the_children() {
    let mut app = opened(
        120,
        50,
        vec![],
        2,
        vec![rc(item("r1", "bob"), 3), item("r2", "carol")],
        None,
    );
    j(&mut app, 1);
    expand(&mut app, "r1", vec![child("x1", "dan", "r1")], None);
    let sk = skeleton(&app);
    assert!(sk.iter().all(|l| !l.contains("3 replies (o)")), "{sk:#?}");
    assert!(sk.contains(&" │ └─● Dan @dan · 5m".to_string()), "{sk:#?}");
}

#[test]
fn a_child_list_with_a_next_page_ends_with_a_show_more_row() {
    let mut app = opened(
        120,
        50,
        vec![],
        2,
        vec![rc(item("r1", "bob"), 3), item("r2", "carol")],
        None,
    );
    j(&mut app, 1);
    expand(
        &mut app,
        "r1",
        vec![child("x1", "dan", "r1"), child("x2", "erin", "r1")],
        Some("c2"),
    );
    let mut want = main_block();
    want.extend(strings(&[
        " │",
        " ├─● Bob @bob · 10m",
        " │ │   body of r1",
        " │ ├─● Dan @dan · 5m",
        " │ │     body of x1",
        " │ ├─● Erin @erin · 5m",
        " │ │     body of x2",
        " │ ┊ Show more (o)",
        " └─● Carol @carol · 10m",
        "       body of r2",
    ]));
    assert_eq!(skeleton(&app), want);
}

#[test]
fn a_quote_reply_shows_the_marker_before_the_body() {
    let app = opened(120, 50, vec![], 1, vec![quote("q0", "eve")], None);
    let sk = from_line(&skeleton(&app), "Eve");
    assert_eq!(
        sk,
        strings(&[
            " └─● Eve @eve · 8m",
            "       ❝ quoted this commit",
            "       body of q0",
        ])
    );
}

// --- 11. layout: continuation -----------------------------------------------

#[test]
fn the_first_direct_reply_by_the_author_follows_the_main_commit_as_its_continuation() {
    let app = opened(
        120,
        50,
        vec![],
        4,
        vec![
            quote("q0", "alice"),
            item("c0", "alice"),
            item("c1", "alice"),
            item("r1", "bob"),
        ],
        None,
    );
    let want = strings(&[
        " │",
        " ◉ Alice @alice  [1/2]",
        " │ body of m1",
        " │ Oct 20, 2026 11:00 · commit 00000m1",
        " │",
        " • Alice  [2/2]",
        " │ body of c0",
        " │",
        " ├─● Alice @alice · 8m",
        " │     ❝ quoted this commit",
        " │     body of q0",
        " ├─● Alice @alice · 10m",
        " │     body of c1",
        " └─● Bob @bob · 10m",
        "       body of r1",
    ]);
    assert_eq!(skeleton(&app), want);
}

#[test]
fn a_quote_by_the_author_is_not_a_continuation() {
    let app = opened(120, 50, vec![], 1, vec![quote("q0", "alice")], None);
    let sk = skeleton(&app);
    assert!(sk.contains(&" ◉ Alice @alice".to_string()), "{sk:#?}");
    assert!(sk.iter().all(|l| !l.contains("[1/2]")), "{sk:#?}");
    assert!(sk.iter().all(|l| !l.contains('•')), "{sk:#?}");
}

#[test]
fn replies_by_others_leave_the_main_header_without_a_position() {
    let app = opened(120, 50, vec![], 2, others(0, 2), None);
    let sk = skeleton(&app);
    assert!(sk.contains(&" ◉ Alice @alice".to_string()), "{sk:#?}");
    assert!(sk.iter().all(|l| !l.contains("[1/2]")), "{sk:#?}");
    assert!(sk.iter().all(|l| !l.contains('•')), "{sk:#?}");
}

#[test]
fn a_reply_by_the_author_to_another_reply_is_not_a_continuation() {
    let app = opened(
        120,
        50,
        vec![],
        2,
        vec![item("r1", "bob"), child("c0", "alice", "r1")],
        None,
    );
    let sk = skeleton(&app);
    assert!(sk.contains(&" ◉ Alice @alice".to_string()), "{sk:#?}");
    assert!(sk.iter().all(|l| !l.contains("[1/2]")), "{sk:#?}");
}

// --- 11. layout: show more --------------------------------------------------

fn show_more_app(replies_count: u32, cursor: Option<&str>) -> App {
    opened(
        120,
        50,
        vec![],
        replies_count,
        vec![
            item("c0", "alice"),
            item("r1", "bob"),
            item("r2", "carol"),
            item("r3", "bob"),
        ],
        cursor,
    )
}

#[test]
fn show_n_more_counts_the_replies_not_yet_listed_without_the_continuation() {
    let app = show_more_app(6, Some("p2"));
    let sk = skeleton(&app);
    assert_eq!(sk.last(), Some(&" ┊ Show 3 more".to_string()), "{sk:#?}");
    assert!(sk.contains(&" ├─● Bob @bob · 10m".to_string()), "{sk:#?}");
}

#[test]
fn show_more_has_no_count_when_nothing_seems_to_be_missing() {
    let app = show_more_app(3, Some("p2"));
    let sk = skeleton(&app);
    assert_eq!(sk.last(), Some(&" ┊ Show more".to_string()), "{sk:#?}");
}

#[test]
fn no_show_more_row_is_drawn_without_a_next_page() {
    let app = show_more_app(10, None);
    let sk = skeleton(&app);
    assert!(sk.iter().all(|l| !l.contains("Show")), "{sk:#?}");
    assert!(sk.contains(&" └─● Bob @bob · 10m".to_string()), "{sk:#?}");
}

#[test]
fn a_page_in_flight_replaces_the_show_more_row_with_a_loading_row() {
    let mut app = opened(120, 50, vec![], 12, others(0, 3), Some("p2"));
    let effects = press(&mut app, KeyCode::Char('j'));
    assert_eq!(replies_fetches(&effects).len(), 1);
    let sk = skeleton(&app);
    assert_eq!(
        sk.last(),
        Some(&" ┊ (loading more...)".to_string()),
        "{sk:#?}"
    );
    assert!(sk.iter().all(|l| !l.contains("Show")), "{sk:#?}");
}

// --- 11. layout: ancestors --------------------------------------------------

#[test]
fn ancestors_are_drawn_oldest_first_above_the_main_commit() {
    let app = opened(
        120,
        50,
        vec![item_of(commit_json("a0", "zoe", 7200)), ancestor_tomb("a1")],
        0,
        vec![],
        None,
    );
    let mut want = strings(&[
        " │",
        " ◉ Zoe @zoe · 2h",
        " │ body of a0",
        " │",
        " ◌ [deleted] This commit was deleted.",
    ]);
    want.extend(main_block());
    assert_eq!(skeleton(&app), want);
}

#[test]
fn the_copy_link_header_is_right_aligned_with_a_right_margin() {
    let app = full_thread(120, 50);
    let first = lines(&app).first().cloned().unwrap_or_default();
    let margin = usize::from(list_width(&app)) - 1 - "a1b2c3d  y copy link".len();
    assert_eq!(first, format!("{}a1b2c3d  y copy link", " ".repeat(margin)));
}

#[test]
fn the_full_thread_fixture_ends_with_the_main_commit_selected() {
    let app = full_thread(120, 50);
    assert_eq!(selected_of(&app, LONG), 2);
}

// --- selection --------------------------------------------------------------

#[test]
fn the_main_commit_rows_carry_the_selection_marker_initially() {
    let app = opened(120, 50, vec![], 2, others(0, 2), None);
    assert_eq!(
        marked(&app),
        strings(&[
            "▌◉ Alice @alice",
            "▌│ body of m1",
            "▌│ Oct 20, 2026 11:00 · commit 00000m1",
        ])
    );
}

#[test]
fn the_marker_moves_to_the_selected_reply_with_its_prefix() {
    let mut app = opened(120, 50, vec![], 2, others(0, 2), None);
    j(&mut app, 1);
    assert_eq!(
        marked(&app),
        strings(&["▌├─● Bob @bob · 10m", "▌│ │   body of r0"])
    );
}

#[test]
fn a_show_more_row_carries_the_marker_when_selected() {
    let mut app = opened(
        120,
        50,
        vec![],
        2,
        vec![rc(item("r1", "bob"), 3), item("r2", "carol")],
        None,
    );
    j(&mut app, 1);
    expand(
        &mut app,
        "r1",
        vec![child("x1", "dan", "r1"), child("x2", "erin", "r1")],
        Some("c2"),
    );
    // Entries: main 0, r1 1, x1 2, x2 3, Show more 4.
    j(&mut app, 3);
    assert_eq!(selected(&app), 4);
    assert_eq!(marked(&app), strings(&["▌│ ┊ Show more (o)"]));
}

#[test]
fn each_entry_span_starts_at_the_row_that_carries_the_marker() {
    let mut app = nested();
    type_str(&mut app, "gg");
    let count = view(&app).entries.len();
    assert_eq!(count, 6);
    for want in 0..count {
        assert_eq!(selected(&app), want);
        let v = view(&app);
        let start = v.entries[want].start;
        let row = v.lines.get(start).map(text_of).unwrap_or_default();
        assert!(row.starts_with('▌'), "entry {want} at {start}: {row:?}");
        j(&mut app, 1);
    }
}

#[test]
fn every_row_fits_the_pane_width_at_wide_and_narrow_sizes() {
    for (w, h) in [(120, 50), (60, 40)] {
        let mut app = nested();
        app.update(Action::Resize(w, h));
        let width = usize::from(list_width(&app));
        let t = lines(&app);
        assert!(t.len() > 10, "{t:#?}");
        for line in &t {
            assert!(line.width() < width, "{line:?} is {} wide", line.width());
        }
    }
}

// --- states -----------------------------------------------------------------

fn open_long(app: &mut App) -> Vec<Effect> {
    app.update(Action::Open(Screen::Thread(LONG.into())))
}

fn fail_main(app: &mut App, effects: &[Effect], code: &str) {
    app.update(Action::ThreadFailed {
        id: LONG.into(),
        req: thread_req(effects, LONG),
        failure: Failure::new(code),
    });
}

#[test]
fn an_unavailable_commit_is_reported_as_fatal_with_its_short_hash() {
    let mut app = signed_in(120, 24);
    let effects = open_long(&mut app);
    fail_main(&mut app, &effects, "commit_unavailable");
    assert_eq!(
        lines(&app).first().map(String::as_str),
        Some(" fatal: commit a1b2c3d is not available")
    );
}

#[test]
fn a_missing_commit_is_reported_the_same_way() {
    let mut app = signed_in(120, 24);
    let effects = open_long(&mut app);
    fail_main(&mut app, &effects, "http_404");
    assert_eq!(
        lines(&app).first().map(String::as_str),
        Some(" fatal: commit a1b2c3d is not available")
    );
}

#[test]
fn another_failure_of_the_main_commit_offers_a_retry() {
    let mut app = signed_in(120, 24);
    let effects = open_long(&mut app);
    fail_main(&mut app, &effects, "http_500");
    assert_eq!(
        lines(&app).first().map(String::as_str),
        Some(" error: http_500 — press R to retry")
    );
}

#[test]
fn a_failed_main_commit_hides_a_failed_replies_request() {
    let mut app = signed_in(120, 24);
    let effects = open_long(&mut app);
    app.update(Action::RepliesFailed {
        thread: LONG.into(),
        parent: LONG.into(),
        req: replies_req(&effects, LONG),
        failure: Failure::new("http_500"),
    });
    fail_main(&mut app, &effects, "commit_unavailable");
    let t: Vec<String> = lines(&app).into_iter().filter(|l| !l.is_empty()).collect();
    assert_eq!(t, strings(&[" fatal: commit a1b2c3d is not available"]));
}

#[test]
fn a_failed_replies_request_adds_a_retry_row_below_the_main_commit() {
    let mut app = signed_in(120, 24);
    let effects = open_thread(&mut app, MAIN);
    deliver_thread(&mut app, &effects, vec![], 2);
    app.update(Action::RepliesFailed {
        thread: MAIN.into(),
        parent: MAIN.into(),
        req: replies_req(&effects, MAIN),
        failure: Failure::new("http_500"),
    });
    let sk = skeleton(&app);
    assert_eq!(
        sk.last(),
        Some(&" error: http_500 — press R to retry".to_string()),
        "{sk:#?}"
    );
    assert!(sk.contains(&" ◉ Alice @alice".to_string()), "{sk:#?}");
}

#[test]
fn a_thread_that_is_still_loading_shows_a_loading_row_only() {
    let mut app = signed_in(120, 24);
    open_long(&mut app);
    assert_eq!(
        lines(&app)
            .into_iter()
            .filter(|l| !l.is_empty())
            .collect::<Vec<_>>(),
        strings(&[" ┊ (loading...)"])
    );
}

#[test]
fn replies_are_not_drawn_before_the_main_commit_arrives() {
    let mut app = signed_in(120, 24);
    let effects = open_thread(&mut app, MAIN);
    deliver_replies(&mut app, &effects, MAIN, others(0, 2), None);
    assert_eq!(
        lines(&app)
            .into_iter()
            .filter(|l| !l.is_empty())
            .collect::<Vec<_>>(),
        strings(&[" ┊ (loading...)"])
    );
}

#[test]
fn a_guest_sees_a_sign_in_row_instead_of_replies() {
    let mut app = guest(120, 40);
    app.update(Action::CloseModal);
    let effects = open_thread(&mut app, MAIN);
    deliver_thread(&mut app, &effects, vec![], 2);
    let sk = skeleton(&app);
    assert_eq!(
        sk.last(),
        Some(&" ┊ sign in to see replies".to_string()),
        "{sk:#?}"
    );
}

#[test]
fn a_guest_sees_no_sign_in_row_when_the_commit_has_no_replies() {
    let mut app = guest(120, 40);
    app.update(Action::CloseModal);
    let effects = open_thread(&mut app, MAIN);
    deliver_thread(&mut app, &effects, vec![], 0);
    let sk = skeleton(&app);
    assert!(sk.contains(&" ◉ Alice @alice".to_string()), "{sk:#?}");
    assert!(sk.iter().all(|l| !l.contains("sign in")), "{sk:#?}");
}

#[test]
fn the_breadcrumb_names_the_commit_by_its_short_hash() {
    let mut app = signed_in(120, 24);
    open_long(&mut app);
    assert_eq!(breadcrumb(&app), "Commit a1b2c3d");
}

// --- 12. replying_to in lists -----------------------------------------------

fn parent_json(parent: Value) -> Value {
    json!({"parent_id": "p0", "parent": parent})
}

fn feed_lines(app: &App, key: FeedKey) -> Vec<String> {
    feed_rows(app.feed(key), key, app, list_width(app))
        .lines
        .iter()
        .map(text_of)
        .collect()
}

fn list_with(parent: Option<Value>) -> App {
    let extra = parent.map_or(json!({}), parent_json);
    let reply: Commit = commit_of(merge(commit_json("c0", "alice", 180), extra));
    let (mut app, effects) = started(120, 40);
    let page = FeedPage {
        items: vec![reply],
        next_cursor: None,
        is_ranked: false,
        suggestions: Vec::new(),
    };
    deliver(&mut app, FOLLOWING, &effects, page);
    app
}

#[test]
fn a_listed_reply_names_the_author_of_its_parent() {
    let app = list_with(Some(commit_json("p0", "bob", 600)));
    let t = feed_lines(&app, FOLLOWING);
    assert!(
        t.iter().any(|l| l.contains("│ ↳ replying to @bob")),
        "{t:#?}"
    );
}

#[test]
fn a_listed_reply_to_a_deleted_parent_names_nobody() {
    let tombstone = json!({
        "id": "p0", "parent_id": null,
        "deleted_at": "2026-10-20T11:00:00Z", "replies_count": 0,
    });
    let app = list_with(Some(tombstone));
    let t = feed_lines(&app, FOLLOWING);
    assert!(t.iter().all(|l| !l.contains("replying to")), "{t:#?}");
}

#[test]
fn a_listed_commit_without_a_parent_names_nobody() {
    let app = list_with(None);
    let t = feed_lines(&app, FOLLOWING);
    assert!(t.iter().all(|l| !l.contains("replying to")), "{t:#?}");
}

#[test]
fn a_saved_reply_names_the_author_of_its_parent() {
    let reply = commit_of(merge(
        commit_json("c0", "alice", 180),
        parent_json(commit_json("p0", "bob", 600)),
    ));
    let (mut app, _) = started(120, 40);
    let effects = type_str(&mut app, "gv");
    let page = FeedPage {
        items: vec![reply],
        next_cursor: None,
        is_ranked: false,
        suggestions: Vec::new(),
    };
    deliver(&mut app, FeedKey::Saved, &effects, page);
    let t = feed_lines(&app, FeedKey::Saved);
    assert!(
        t.iter().any(|l| l.contains("│ ↳ replying to @bob")),
        "{t:#?}"
    );
}

// --- snapshots --------------------------------------------------------------

macro_rules! snapshot {
    ($name:literal, $app:expr) => {{
        let terminal = render(&$app);
        insta::assert_snapshot!($name, terminal.backend());
    }};
}

fn sj(
    id: &str,
    author: &str,
    secs: i64,
    short: &str,
    body: &str,
    parent: &str,
    extra: Value,
) -> ThreadItem {
    item_of(merge(
        merge(
            commit_json(id, author, secs),
            json!({
                "short_hash": short,
                "body": body,
                "body_segments": [{"type": "text", "text": body}],
                "parent_id": parent,
            }),
        ),
        extra,
    ))
}

fn tomb_of(id: &str, parent: &str, replies: u32) -> ThreadItem {
    serde_json::from_value(json!({
        "id": id,
        "parent_id": parent,
        "deleted_at": "2026-10-20T11:00:00Z",
        "replies_count": replies,
    }))
    .unwrap()
}

fn settle_prefetch(app: &mut App, effects: &[Effect]) {
    for effect in effects {
        if let Effect::FetchReplies {
            thread,
            parent,
            cursor: Some(cursor),
            req,
        } = effect
            && thread == parent
        {
            app.update(Action::RepliesLoaded {
                thread: thread.clone(),
                parent: parent.clone(),
                req: *req,
                page: page(vec![], Some(cursor)),
            });
        }
    }
}

/// Presses a key and answers any first-level prefetch with an empty page, so that no load stays in flight.
fn step(app: &mut App, c: char) -> Vec<Effect> {
    let effects = press(app, KeyCode::Char(c));
    settle_prefetch(app, &effects);
    effects
}

fn open_child(app: &mut App, parent: &str, items: Vec<ThreadItem>, cursor: Option<&str>) {
    let effects = step(app, 'o');
    app.update(Action::RepliesLoaded {
        thread: LONG.into(),
        parent: parent.into(),
        req: replies_req(&effects, parent),
        page: page(items, cursor),
    });
}

/// The Thread mockup of docs/screen-design.md with every kind of row, main commit selected.
fn full_thread(w: u16, h: u16) -> App {
    let mut app = signed_in(w, h);
    let open = open_long(&mut app);
    let main = commit_of(merge(
        commit_json(LONG, "alice", 7080),
        json!({
            "short_hash": "a1b2c3d",
            "body": "shipped a terminal client for /dev/null.\nit mirrors the web layout: lanes, cards, tabs.",
            "body_segments": [{"type": "text", "text": "shipped a terminal client for /dev/null.\nit mirrors the web layout: lanes, cards, tabs."}],
            "parent_id": "anc1",
            "stars_count": 13,
            "pasteis_count": 3,
            "replies_count": 16,
        }),
    ));
    let zoe = sj(
        "anc0",
        "zoe",
        7200,
        "5a6b7c8",
        "what is everyone building this weekend?",
        "",
        json!({"parent_id": null, "stars_count": 8, "replies_count": 1}),
    );
    app.update(Action::ThreadLoaded {
        id: LONG.into(),
        req: thread_req(&open, LONG),
        response: Box::new(ThreadResponse {
            data: main,
            ancestors: vec![zoe, tomb_of("anc1", "anc0", 1)],
        }),
    });
    let first_level = vec![
        sj(
            "r0",
            "alice",
            6000,
            "c0ffee0",
            "source: github.com/myuron/lazydn",
            LONG,
            json!({"stars_count": 4}),
        ),
        sj(
            "q1",
            "eve",
            300,
            "b1b1b1b",
            "this is the way",
            "",
            json!({"parent_id": null, "quoted_commit_id": LONG, "stars_count": 2}),
        ),
        sj(
            "b1",
            "bob",
            180,
            "0b0b0b0",
            "nice! which crate for ssh sigs?",
            LONG,
            json!({"replies_count": 1}),
        ),
        tomb_of("t1", LONG, 3),
        sj(
            "f1",
            "frank",
            60,
            "f1f1f1f",
            "ship it",
            LONG,
            json!({"stars_count": 0}),
        ),
    ];
    app.update(Action::RepliesLoaded {
        thread: LONG.into(),
        parent: LONG.into(),
        req: replies_req(&open, LONG),
        page: page(first_level, Some("page2")),
    });
    // Entries: anc0 0, anc1 1, main 2, r0 3, q1 4, b1 5, c1 6, e1 7, t1 8.
    for _ in 0..3 {
        step(&mut app, 'j');
    }
    open_child(
        &mut app,
        "b1",
        vec![sj(
            "c1",
            "carol",
            30,
            "ca401ca",
            "ssh-key has SshSig",
            "b1",
            json!({"replies_count": 1}),
        )],
        None,
    );
    step(&mut app, 'j');
    open_child(
        &mut app,
        "c1",
        vec![sj(
            "e1",
            "erin",
            20,
            "e1e1e1e",
            "works great here",
            "c1",
            json!({}),
        )],
        None,
    );
    step(&mut app, 'j');
    step(&mut app, 'j');
    open_child(
        &mut app,
        "t1",
        vec![
            sj(
                "d1",
                "dan",
                10,
                "d4d4d4d",
                "...",
                "t1",
                json!({"replies_count": 3}),
            ),
            sj("g1", "gus", 5, "9a9a9a9", "second", "t1", json!({})),
        ],
        Some("t1-next"),
    );
    type_str(&mut app, "gg");
    step(&mut app, 'j');
    step(&mut app, 'j');
    app
}

fn selected_of(app: &App, id: &str) -> usize {
    state(app, id).selected
}

#[test]
fn t1_full_thread_at_120x50() {
    snapshot!("thread_full", full_thread(120, 50));
}

#[test]
fn t2_unavailable_commit_at_120x24() {
    let mut app = signed_in(120, 24);
    let effects = open_long(&mut app);
    fail_main(&mut app, &effects, "commit_unavailable");
    snapshot!("thread_unavailable", app);
}

#[test]
fn t3_loading_thread_at_120x24() {
    let mut app = signed_in(120, 24);
    open_long(&mut app);
    snapshot!("thread_loading", app);
}

#[test]
fn t4_full_thread_at_60x40() {
    snapshot!("thread_narrow", full_thread(60, 40));
}

// --- review follow-ups ------------------------------------------------------

fn two_replies_first_open() -> App {
    let mut app = opened(
        120,
        50,
        vec![],
        2,
        vec![rc(item("r1", "bob"), 2), item("r2", "carol")],
        None,
    );
    j(&mut app, 1);
    app
}

const R1_BLOCK: [&str; 2] = [" ├─● Bob @bob · 10m", " │ │   body of r1"];

fn from_bob(app: &App) -> Vec<String> {
    from_line(&skeleton(app), "Bob")
}

#[test]
fn a_child_list_in_flight_shows_a_loading_row_under_its_parent() {
    let mut app = two_replies_first_open();
    press(&mut app, KeyCode::Char('o'));
    let mut want = strings(&R1_BLOCK);
    want.extend(strings(&[
        " │ ┊ (loading...)",
        " └─● Carol @carol · 10m",
        "       body of r2",
    ]));
    assert_eq!(from_bob(&app), want);
}

#[test]
fn a_failed_child_request_shows_a_retry_row_under_its_parent() {
    let mut app = two_replies_first_open();
    let effects = press(&mut app, KeyCode::Char('o'));
    app.update(Action::RepliesFailed {
        thread: MAIN.into(),
        parent: "r1".into(),
        req: replies_req(&effects, "r1"),
        failure: Failure::new("http_500"),
    });
    let mut want = strings(&R1_BLOCK);
    want.extend(strings(&[
        " │ error: http_500 — press o to retry",
        " └─● Carol @carol · 10m",
        "       body of r2",
    ]));
    assert_eq!(from_bob(&app), want);
}

#[test]
fn a_failed_next_first_level_page_shows_show_more_then_the_retry_row() {
    let mut app = opened(120, 50, vec![], 12, others(0, 3), Some("p2"));
    let effects = press(&mut app, KeyCode::Char('j'));
    app.update(Action::RepliesFailed {
        thread: MAIN.into(),
        parent: MAIN.into(),
        req: replies_req(&effects, MAIN),
        failure: Failure::new("http_500"),
    });
    let sk = skeleton(&app);
    let tail = &sk[sk.len().saturating_sub(2)..];
    assert_eq!(
        tail,
        strings(&[" ┊ Show 9 more", " error: http_500 — press R to retry"])
    );
}

#[test]
fn a_single_hidden_reply_is_hinted_in_the_singular() {
    let app = opened(120, 50, vec![], 1, vec![rc(item("r1", "bob"), 1)], None);
    let sk = from_line(&skeleton(&app), "Bob");
    assert_eq!(
        sk,
        strings(&[
            " └─● Bob @bob · 10m",
            "   │   body of r1",
            "   ┊ 1 reply (o)",
        ])
    );
}

#[test]
fn a_failed_refresh_shows_only_the_error_even_though_the_commit_is_known() {
    let mut app = opened(120, 24, vec![], 2, others(0, 2), None);
    let effects = type_str(&mut app, "R");
    app.update(Action::ThreadFailed {
        id: MAIN.into(),
        req: thread_req(&effects, MAIN),
        failure: Failure::new("commit_unavailable"),
    });
    let t: Vec<String> = lines(&app).into_iter().filter(|l| !l.is_empty()).collect();
    assert_eq!(t, strings(&[" fatal: commit m1 is not available"]));
}
