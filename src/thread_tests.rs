use crossterm::event::KeyCode;
use serde_json::{Value, json};
use time::Duration;

use crate::action::{Action, Effect, Failure};
use crate::app::{App, LogTab, Screen};
use crate::feed::{LoadState, ReqId};
use crate::feed_tests::list_width;
use crate::model::{Commit, Meta, Page, ThreadItem, ThreadResponse};
use crate::test_support::*;
use crate::thread::{ThreadState, item_id};
use crate::ui::feed::CardSpan;
use crate::ui::thread::rows;

pub(crate) const MAIN: &str = "m1";

// --- fixtures ---------------------------------------------------------------

fn capitalized(username: &str) -> String {
    let mut chars = username.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

pub(crate) fn user_json(username: &str) -> Value {
    json!({
        "id": format!("user-{username}"),
        "username": username,
        "name": capitalized(username),
        "created_at": "2026-10-01T00:00:00Z",
        "lane": 1,
    })
}

pub(crate) fn merge(mut base: Value, extra: Value) -> Value {
    base.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    base
}

/// A commit `secs_ago` old at the fixed clock whose body is `body of <id>`.
pub(crate) fn commit_json(id: &str, author: &str, secs_ago: i64) -> Value {
    let created = NOW - Duration::seconds(secs_ago);
    let body = format!("body of {id}");
    json!({
        "id": id,
        "short_hash": format!("{id:0>7}"),
        "body": body,
        "parent_id": null,
        "html_url": null,
        "created_at": created.format(&time::format_description::well_known::Rfc3339).unwrap(),
        "stars_count": 1,
        "replies_count": 0,
        "link_preview": null,
        "images": [],
        "author": user_json(author),
        "body_segments": [{"type": "text", "text": body}],
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

pub(crate) fn commit_of(value: Value) -> Commit {
    serde_json::from_value(value).unwrap()
}

pub(crate) fn item_of(value: Value) -> ThreadItem {
    ThreadItem::Commit(Box::new(commit_of(value)))
}

/// A direct reply to [`MAIN`].
pub(crate) fn item(id: &str, author: &str) -> ThreadItem {
    item_of(merge(
        commit_json(id, author, 600),
        json!({"parent_id": MAIN}),
    ))
}

/// A reply to the item `parent`.
pub(crate) fn child(id: &str, author: &str, parent: &str) -> ThreadItem {
    item_of(merge(
        commit_json(id, author, 300),
        json!({"parent_id": parent}),
    ))
}

/// A reply that quotes [`MAIN`] without being a direct child of it.
pub(crate) fn quote(id: &str, author: &str) -> ThreadItem {
    item_of(merge(
        commit_json(id, author, 500),
        json!({"quoted_commit_id": MAIN}),
    ))
}

pub(crate) fn with_replies_count(item: ThreadItem, count: u32) -> ThreadItem {
    match item {
        ThreadItem::Commit(mut c) => {
            c.replies_count = Some(count);
            ThreadItem::Commit(c)
        }
        ThreadItem::Tombstone(mut t) => {
            t.replies_count = Some(count);
            ThreadItem::Tombstone(t)
        }
    }
}

pub(crate) fn tomb(id: &str) -> ThreadItem {
    serde_json::from_value(json!({
        "id": id,
        "parent_id": MAIN,
        "deleted_at": "2026-10-20T11:00:00Z",
        "replies_count": 0,
    }))
    .unwrap()
}

pub(crate) fn main_commit(replies_count: u32) -> Commit {
    commit_of(merge(
        commit_json(MAIN, "alice", 3600),
        json!({"replies_count": replies_count, "parent_id": "a1"}),
    ))
}

pub(crate) fn response(ancestors: Vec<ThreadItem>, replies_count: u32) -> ThreadResponse {
    ThreadResponse {
        data: main_commit(replies_count),
        ancestors,
    }
}

pub(crate) fn page(items: Vec<ThreadItem>, cursor: Option<&str>) -> Page<ThreadItem> {
    Page {
        data: items,
        meta: Meta {
            next_cursor: cursor.map(String::from),
        },
    }
}

/// Alternating authors so that no reply is taken for a continuation of the main commit.
pub(crate) fn others(from: usize, to: usize) -> Vec<ThreadItem> {
    (from..to)
        .map(|i| {
            let author = if i % 2 == 0 { "bob" } else { "carol" };
            with_replies_count(item(&format!("r{i}"), author), 1)
        })
        .collect()
}

// --- effects ----------------------------------------------------------------

pub(crate) fn thread_fetches(effects: &[Effect]) -> Vec<String> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::FetchThread { id, .. } => Some(id.clone()),
            _ => None,
        })
        .collect()
}

pub(crate) fn replies_fetches(effects: &[Effect]) -> Vec<(String, String, Option<String>)> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::FetchReplies {
                thread,
                parent,
                cursor,
                ..
            } => Some((thread.clone(), parent.clone(), cursor.clone())),
            _ => None,
        })
        .collect()
}

/// Falls back to a bogus id so that a missing effect fails the assertions, not an unwrap.
pub(crate) fn thread_req(effects: &[Effect], id: &str) -> ReqId {
    effects
        .iter()
        .find_map(|e| match e {
            Effect::FetchThread { id: i, req } if i == id => Some(*req),
            _ => None,
        })
        .unwrap_or(ReqId(u64::MAX))
}

pub(crate) fn replies_req(effects: &[Effect], parent: &str) -> ReqId {
    effects
        .iter()
        .find_map(|e| match e {
            Effect::FetchReplies { parent: p, req, .. } if p == parent => Some(*req),
            _ => None,
        })
        .unwrap_or(ReqId(u64::MAX))
}

fn fetch(id: &str, parent: &str, cursor: Option<&str>) -> (String, String, Option<String>) {
    (id.into(), parent.into(), cursor.map(String::from))
}

// --- scenario helpers -------------------------------------------------------

pub(crate) fn open_thread(app: &mut App, id: &str) -> Vec<Effect> {
    app.update(Action::Open(Screen::Thread(id.into())))
}

pub(crate) fn deliver_thread(
    app: &mut App,
    effects: &[Effect],
    ancestors: Vec<ThreadItem>,
    replies_count: u32,
) {
    app.update(Action::ThreadLoaded {
        id: MAIN.into(),
        req: thread_req(effects, MAIN),
        response: Box::new(response(ancestors, replies_count)),
    });
}

pub(crate) fn deliver_replies(
    app: &mut App,
    effects: &[Effect],
    parent: &str,
    items: Vec<ThreadItem>,
    cursor: Option<&str>,
) {
    app.update(Action::RepliesLoaded {
        thread: MAIN.into(),
        parent: parent.into(),
        req: replies_req(effects, parent),
        page: page(items, cursor),
    });
}

/// Signed-in thread view with the main commit and its first level of replies loaded.
pub(crate) fn opened(
    w: u16,
    h: u16,
    ancestors: Vec<ThreadItem>,
    replies_count: u32,
    replies: Vec<ThreadItem>,
    cursor: Option<&str>,
) -> App {
    let mut app = signed_in(w, h);
    let effects = open_thread(&mut app, MAIN);
    deliver_thread(&mut app, &effects, ancestors, replies_count);
    deliver_replies(&mut app, &effects, MAIN, replies, cursor);
    app
}

pub(crate) fn state(app: &App, id: &str) -> ThreadState {
    assert!(app.thread(id).is_some(), "no state for thread {id}");
    app.thread(id).cloned().unwrap_or_default()
}

pub(crate) fn selected(app: &App) -> usize {
    state(app, MAIN).selected
}

fn reply_ids(app: &App, parent: &str) -> Vec<String> {
    state(app, MAIN)
        .children
        .get(parent)
        .map(|list| list.items.iter().map(|i| item_id(i).to_string()).collect())
        .unwrap_or_default()
}

fn failed_code(load: &LoadState) -> Option<&str> {
    match load {
        LoadState::Failed { code, .. } => Some(code),
        LoadState::Idle => None,
    }
}

fn key(app: &mut App, c: char) -> Vec<Effect> {
    press(app, KeyCode::Char(c))
}

fn names(ids: &[&str]) -> Vec<String> {
    ids.iter().map(|s| s.to_string()).collect()
}

// --- 1. fetching on open ----------------------------------------------------

#[test]
fn open_requests_the_thread_and_its_first_replies_once() {
    let mut app = signed_in(120, 40);
    let effects = open_thread(&mut app, MAIN);
    assert_eq!(app.screen(), Screen::Thread(MAIN.into()));
    assert_eq!(thread_fetches(&effects), names(&[MAIN]));
    assert_eq!(replies_fetches(&effects), vec![fetch(MAIN, MAIN, None)]);
    assert_eq!(effects.len(), 2);
}

#[test]
fn reopening_a_known_thread_requests_nothing() {
    let mut app = opened(120, 40, vec![], 0, others(0, 2), None);
    // Back would drop the thread, so it is left through a deeper thread that keeps it in the history.
    open_thread(&mut app, "other");
    assert!(app.thread(MAIN).is_some());
    assert!(open_thread(&mut app, MAIN).is_empty());
}

#[test]
fn reopening_a_thread_whose_requests_are_in_flight_requests_nothing() {
    let mut app = signed_in(120, 40);
    open_thread(&mut app, MAIN);
    open_thread(&mut app, "other");
    assert!(app.thread(MAIN).is_some());
    assert!(open_thread(&mut app, MAIN).is_empty());
}

// --- 2. selection -----------------------------------------------------------

#[test]
fn the_main_commit_is_selected_after_loading() {
    let app = opened(
        120,
        40,
        vec![ancestor("a0", "zoe"), ancestor_tomb("a1")],
        2,
        others(0, 2),
        None,
    );
    assert_eq!(selected(&app), 2);
}

#[test]
fn the_main_commit_is_selected_when_the_replies_arrive_first() {
    let mut app = signed_in(120, 40);
    let effects = open_thread(&mut app, MAIN);
    deliver_replies(&mut app, &effects, MAIN, others(0, 2), None);
    deliver_thread(
        &mut app,
        &effects,
        vec![ancestor("a0", "zoe"), ancestor_tomb("a1")],
        2,
    );
    assert_eq!(selected(&app), 2);
}

#[test]
fn j_and_k_walk_ancestors_the_main_commit_the_continuation_and_replies() {
    let replies = vec![item("c0", "alice"), item("r1", "bob"), item("r2", "carol")];
    let mut app = opened(
        120,
        40,
        vec![ancestor("a0", "zoe"), ancestor("a1", "dan")],
        3,
        replies,
        None,
    );
    assert_eq!(selected(&app), 2);
    key(&mut app, 'k');
    assert_eq!(selected(&app), 1);
    key(&mut app, 'k');
    assert_eq!(selected(&app), 0);
    key(&mut app, 'k');
    assert_eq!(selected(&app), 0);
    for want in [1, 2, 3, 4, 5] {
        key(&mut app, 'j');
        assert_eq!(selected(&app), want);
    }
    key(&mut app, 'j');
    assert_eq!(selected(&app), 5);
}

#[test]
fn capital_g_selects_the_last_item_and_gg_the_first() {
    let mut app = opened(120, 40, vec![ancestor("a0", "zoe")], 3, others(0, 3), None);
    key(&mut app, 'G');
    assert_eq!(selected(&app), 4);
    type_str(&mut app, "gg");
    assert_eq!(selected(&app), 0);
}

#[test]
fn nothing_is_selectable_before_the_main_commit_arrives() {
    let mut app = signed_in(120, 40);
    let effects = open_thread(&mut app, MAIN);
    deliver_replies(&mut app, &effects, MAIN, others(0, 3), None);
    key(&mut app, 'j');
    key(&mut app, 'G');
    assert_eq!(selected(&app), 0);
}

// --- 3. stale responses -----------------------------------------------------

#[test]
fn a_thread_response_for_a_superseded_request_is_dropped() {
    let mut app = signed_in(120, 40);
    let effects = open_thread(&mut app, MAIN);
    let stale = thread_req(&effects, MAIN);
    let refetch = key_r(&mut app);
    assert_ne!(thread_req(&refetch, MAIN), stale);
    app.update(Action::ThreadLoaded {
        id: MAIN.into(),
        req: stale,
        response: Box::new(response(vec![], 0)),
    });
    assert!(state(&app, MAIN).main.is_none());
    app.update(Action::ThreadLoaded {
        id: MAIN.into(),
        req: thread_req(&refetch, MAIN),
        response: Box::new(response(vec![], 0)),
    });
    assert!(state(&app, MAIN).main.is_some());
}

fn key_r(app: &mut App) -> Vec<Effect> {
    type_str(app, "R")
}

#[test]
fn a_replies_response_for_a_superseded_request_is_dropped() {
    let mut app = signed_in(120, 40);
    let effects = open_thread(&mut app, MAIN);
    let stale = replies_req(&effects, MAIN);
    let refetch = key_r(&mut app);
    app.update(Action::RepliesLoaded {
        thread: MAIN.into(),
        parent: MAIN.into(),
        req: stale,
        page: page(others(0, 2), None),
    });
    assert_eq!(reply_ids(&app, MAIN), Vec::<String>::new());
    deliver_replies(&mut app, &refetch, MAIN, others(0, 2), None);
    assert_eq!(reply_ids(&app, MAIN), names(&["r0", "r1"]));
}

#[test]
fn a_late_thread_failure_does_not_override_a_newer_request() {
    let mut app = signed_in(120, 40);
    let effects = open_thread(&mut app, MAIN);
    let stale = thread_req(&effects, MAIN);
    key_r(&mut app);
    app.update(Action::ThreadFailed {
        id: MAIN.into(),
        req: stale,
        failure: Failure::new("http_500"),
    });
    assert_eq!(failed_code(&state(&app, MAIN).main_load), None);
}

// --- 4. prefetching replies -------------------------------------------------

#[test]
fn replies_are_prefetched_once_when_selection_is_within_five_of_the_end() {
    let mut app = opened(120, 40, vec![], 12, others(0, 10), Some("p2"));
    for _ in 0..4 {
        assert!(replies_fetches(&key(&mut app, 'j')).is_empty());
    }
    let effects = key(&mut app, 'j');
    assert_eq!(
        replies_fetches(&effects),
        vec![fetch(MAIN, MAIN, Some("p2"))]
    );
    assert!(replies_fetches(&key(&mut app, 'j')).is_empty());
}

#[test]
fn no_prefetch_is_requested_without_a_next_cursor() {
    let mut app = opened(120, 40, vec![], 10, others(0, 10), None);
    for _ in 0..10 {
        assert!(replies_fetches(&key(&mut app, 'j')).is_empty());
    }
    assert_eq!(selected(&app), 10);
}

#[test]
fn prefetch_counts_by_the_first_level_reply_that_owns_the_selected_child() {
    let mut replies = others(0, 10);
    replies[2] = with_replies_count(replies[2].clone(), 1);
    let mut app = opened(120, 40, vec![], 12, replies, Some("p2"));
    // Entries: main 0, r0 1, r1 2, r2 3.
    for _ in 0..3 {
        key(&mut app, 'j');
    }
    let effects = key(&mut app, 'o');
    deliver_replies(
        &mut app,
        &effects,
        "r2",
        vec![child("x0", "dan", "r2")],
        None,
    );
    // r2 is a far-from-the-end owner: the child below it must not trigger the prefetch.
    let effects = key(&mut app, 'j');
    assert_eq!(selected(&app), 4);
    assert!(replies_fetches(&effects).is_empty());
    // r4 is within five of the end of the first level.
    let mut got = Vec::new();
    for _ in 0..2 {
        got.extend(replies_fetches(&key(&mut app, 'j')));
    }
    assert_eq!(got, vec![fetch(MAIN, MAIN, Some("p2"))]);
}

#[test]
fn the_prefetch_page_is_appended_without_duplicates() {
    let mut app = opened(120, 40, vec![], 4, others(0, 2), Some("p2"));
    let effects = key(&mut app, 'j');
    deliver_replies(
        &mut app,
        &effects,
        MAIN,
        vec![item("r1", "carol"), item("r2", "bob")],
        None,
    );
    assert_eq!(reply_ids(&app, MAIN), names(&["r0", "r1", "r2"]));
}

// --- 5. Enter and Esc -------------------------------------------------------

fn mixed_replies() -> Vec<ThreadItem> {
    vec![
        item("r1", "bob"),
        with_replies_count(tomb("t1"), 2),
        item("r2", "carol"),
    ]
}

#[test]
fn enter_on_a_reply_opens_that_thread_and_esc_restores_the_selection() {
    let mut app = opened(120, 40, vec![], 3, mixed_replies(), None);
    key(&mut app, 'j');
    let effects = press(&mut app, KeyCode::Enter);
    assert_eq!(app.screen(), Screen::Thread("r1".into()));
    assert_eq!(thread_fetches(&effects), names(&["r1"]));
    assert_eq!(replies_fetches(&effects), vec![fetch("r1", "r1", None)]);
    let back = press(&mut app, KeyCode::Esc);
    assert_eq!(app.screen(), Screen::Thread(MAIN.into()));
    assert!(back.is_empty());
    assert_eq!(selected(&app), 1);
}

#[test]
fn enter_on_the_main_commit_does_nothing() {
    let mut app = opened(120, 40, vec![], 3, mixed_replies(), None);
    assert_eq!(selected(&app), 0);
    let effects = press(&mut app, KeyCode::Enter);
    assert!(effects.is_empty());
    assert_eq!(app.screen(), Screen::Thread(MAIN.into()));
}

#[test]
fn enter_on_a_tombstone_reply_does_nothing() {
    let mut app = opened(120, 40, vec![], 3, mixed_replies(), None);
    type_str(&mut app, "jj");
    assert_eq!(selected(&app), 2);
    let effects = press(&mut app, KeyCode::Enter);
    assert!(effects.is_empty());
    assert_eq!(app.screen(), Screen::Thread(MAIN.into()));
}

#[test]
fn enter_on_an_ancestor_commit_opens_it_but_not_on_an_ancestor_tombstone() {
    let mut app = opened(
        120,
        40,
        vec![ancestor("a0", "zoe"), ancestor_tomb("a1")],
        0,
        vec![],
        None,
    );
    key(&mut app, 'k');
    assert!(press(&mut app, KeyCode::Enter).is_empty());
    assert_eq!(app.screen(), Screen::Thread(MAIN.into()));
    key(&mut app, 'k');
    let effects = press(&mut app, KeyCode::Enter);
    assert_eq!(app.screen(), Screen::Thread("a0".into()));
    assert_eq!(thread_fetches(&effects), names(&["a0"]));
}

// --- 6. refreshing ----------------------------------------------------------

#[test]
fn r_requests_the_thread_and_the_first_replies_again() {
    let mut app = opened(120, 40, vec![], 2, others(0, 2), None);
    let effects = key_r(&mut app);
    assert_eq!(thread_fetches(&effects), names(&[MAIN]));
    assert_eq!(replies_fetches(&effects), vec![fetch(MAIN, MAIN, None)]);
    assert_eq!(effects.len(), 2);
}

#[test]
fn the_main_commit_is_selected_again_after_a_refresh() {
    let mut app = opened(120, 40, vec![ancestor("a0", "zoe")], 3, others(0, 3), None);
    type_str(&mut app, "jj");
    assert_eq!(selected(&app), 3);
    let effects = key_r(&mut app);
    deliver_thread(&mut app, &effects, vec![ancestor("a0", "zoe")], 3);
    deliver_replies(&mut app, &effects, MAIN, others(0, 3), None);
    assert_eq!(selected(&app), 1);
}

#[test]
fn a_refresh_drops_the_loaded_children_and_the_expansion() {
    let mut app = opened(120, 40, vec![], 2, others(0, 2), None);
    key(&mut app, 'j');
    let effects = key(&mut app, 'o');
    deliver_replies(
        &mut app,
        &effects,
        "r0",
        vec![child("x0", "dan", "r0")],
        None,
    );
    assert!(state(&app, MAIN).expanded.contains("r0"));
    let refetch = key_r(&mut app);
    let after = state(&app, MAIN);
    assert!(after.expanded.is_empty());
    assert!(!after.children.contains_key("r0"));
    assert_eq!(replies_fetches(&refetch), vec![fetch(MAIN, MAIN, None)]);
}

#[test]
fn a_child_response_arriving_after_a_refresh_does_not_recreate_the_list() {
    let mut app = opened(120, 40, vec![], 2, others(0, 2), None);
    key(&mut app, 'j');
    let effects = key(&mut app, 'o');
    key_r(&mut app);
    deliver_replies(
        &mut app,
        &effects,
        "r0",
        vec![child("x0", "dan", "r0")],
        None,
    );
    assert!(!state(&app, MAIN).children.contains_key("r0"));
}

#[test]
fn a_first_level_response_for_a_pre_refresh_request_is_dropped() {
    let mut app = signed_in(120, 40);
    let first = open_thread(&mut app, MAIN);
    deliver_thread(&mut app, &first, vec![], 2);
    key_r(&mut app);
    deliver_replies(&mut app, &first, MAIN, others(0, 2), None);
    assert_eq!(reply_ids(&app, MAIN), Vec::<String>::new());
}

// --- 7. errors --------------------------------------------------------------

#[test]
fn a_failed_thread_request_records_the_error_code() {
    let mut app = signed_in(120, 40);
    let effects = open_thread(&mut app, MAIN);
    app.update(Action::ThreadFailed {
        id: MAIN.into(),
        req: thread_req(&effects, MAIN),
        failure: Failure::new("commit_unavailable"),
    });
    let s = state(&app, MAIN);
    assert_eq!(failed_code(&s.main_load), Some("commit_unavailable"));
    assert!(s.main.is_none());
}

#[test]
fn a_failed_replies_request_records_the_error_on_the_list() {
    let mut app = signed_in(120, 40);
    let effects = open_thread(&mut app, MAIN);
    deliver_thread(&mut app, &effects, vec![], 2);
    app.update(Action::RepliesFailed {
        thread: MAIN.into(),
        parent: MAIN.into(),
        req: replies_req(&effects, MAIN),
        failure: Failure::new("http_500"),
    });
    let s = state(&app, MAIN);
    let list = s.children.get(MAIN).cloned().unwrap_or_default();
    assert_eq!(failed_code(&list.load), Some("http_500"));
    assert!(s.main.is_some());
}

// --- 8. Goto ----------------------------------------------------------------

#[test]
fn goto_drops_the_thread_state_and_opening_it_again_refetches() {
    let mut app = opened(120, 40, vec![], 2, others(0, 2), None);
    type_str(&mut app, "gl");
    assert!(app.thread(MAIN).is_none());
    let effects = open_thread(&mut app, MAIN);
    assert_eq!(thread_fetches(&effects), names(&[MAIN]));
}

#[test]
fn goto_drops_threads_that_were_reached_through_the_history() {
    let mut app = opened(120, 40, vec![], 2, others(0, 2), None);
    key(&mut app, 'j');
    press(&mut app, KeyCode::Enter);
    assert!(app.thread(MAIN).is_some());
    assert!(app.thread("r0").is_some());
    type_str(&mut app, "gl");
    assert!(app.thread(MAIN).is_none());
    assert!(app.thread("r0").is_none());
    assert_eq!(app.screen(), Screen::Log(LogTab::Following));
}

#[test]
fn responses_arriving_after_goto_do_not_recreate_the_state() {
    let mut app = signed_in(120, 40);
    let effects = open_thread(&mut app, MAIN);
    assert!(app.thread(MAIN).is_some());
    type_str(&mut app, "gl");
    deliver_thread(&mut app, &effects, vec![], 2);
    deliver_replies(&mut app, &effects, MAIN, others(0, 2), None);
    app.update(Action::RepliesFailed {
        thread: MAIN.into(),
        parent: MAIN.into(),
        req: replies_req(&effects, MAIN),
        failure: Failure::new("http_500"),
    });
    assert!(app.thread(MAIN).is_none());
}

// --- 10. scrolling ----------------------------------------------------------

/// Rows of the scrolling area of a Thread at 120x24: the terminal minus status line and borders.
const VIEW_120X24: usize = 21;

fn assert_selected_visible(app: &App) {
    let s = state(app, MAIN);
    let spans = rows(app, MAIN, list_width(app)).entries;
    let span = spans.get(s.selected).copied();
    assert!(span.is_some(), "entry {} has no span", s.selected);
    let span = span.unwrap_or(CardSpan {
        start: 0,
        height: 0,
    });
    let scroll = s.scroll as usize;
    let top_visible = scroll <= span.start && span.start < scroll + VIEW_120X24;
    let fits = span.start + span.height <= scroll + VIEW_120X24;
    assert!(
        top_visible && (fits || span.height > VIEW_120X24),
        "{span:?} not visible: scroll={scroll}"
    );
}

#[test]
fn the_selected_item_stays_visible_while_walking_a_long_thread() {
    let mut app = opened(120, 24, vec![], 30, others(0, 30), None);
    assert_selected_visible(&app);
    for _ in 0..30 {
        key(&mut app, 'j');
        assert_selected_visible(&app);
    }
    for _ in 0..30 {
        key(&mut app, 'k');
        assert_selected_visible(&app);
    }
}

#[test]
fn a_long_thread_scrolls_down_and_back_up() {
    let mut app = opened(120, 24, vec![], 30, others(0, 30), None);
    key(&mut app, 'G');
    assert!(state(&app, MAIN).scroll > 0);
    assert_selected_visible(&app);
    type_str(&mut app, "gg");
    assert_eq!(state(&app, MAIN).scroll, 0);
}

// --- 11. continuation, children, show more ----------------------------------

#[test]
fn j_walks_into_an_expanded_child_and_enter_opens_it() {
    let mut app = opened(120, 40, vec![], 2, others(0, 2), None);
    key(&mut app, 'j');
    let effects = key(&mut app, 'o');
    assert_eq!(replies_fetches(&effects), vec![fetch(MAIN, "r0", None)]);
    deliver_replies(
        &mut app,
        &effects,
        "r0",
        vec![child("x0", "dan", "r0")],
        None,
    );
    assert_eq!(reply_ids(&app, "r0"), names(&["x0"]));
    assert_eq!(reply_ids(&app, MAIN), names(&["r0", "r1"]));
    key(&mut app, 'j');
    assert_eq!(selected(&app), 2);
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.screen(), Screen::Thread("x0".into()));
    press(&mut app, KeyCode::Esc);
    key(&mut app, 'j');
    assert_eq!(selected(&app), 3);
}

#[test]
fn o_toggles_a_loaded_child_list_without_fetching_again() {
    let mut app = opened(120, 40, vec![], 2, others(0, 2), None);
    key(&mut app, 'j');
    let effects = key(&mut app, 'o');
    deliver_replies(
        &mut app,
        &effects,
        "r0",
        vec![child("x0", "dan", "r0")],
        None,
    );
    assert!(state(&app, MAIN).expanded.contains("r0"));
    assert!(key(&mut app, 'o').is_empty());
    assert!(!state(&app, MAIN).expanded.contains("r0"));
    assert!(key(&mut app, 'o').is_empty());
    assert!(state(&app, MAIN).expanded.contains("r0"));
}

#[test]
fn o_on_a_tombstone_reply_expands_its_children() {
    let mut app = opened(120, 40, vec![], 3, mixed_replies(), None);
    type_str(&mut app, "jj");
    let effects = key(&mut app, 'o');
    assert_eq!(replies_fetches(&effects), vec![fetch(MAIN, "t1", None)]);
    deliver_replies(
        &mut app,
        &effects,
        "t1",
        vec![child("x0", "dan", "t1")],
        None,
    );
    key(&mut app, 'j');
    assert_eq!(selected(&app), 3);
}

#[test]
fn o_does_nothing_on_the_main_commit_ancestors_and_ancestor_tombstones() {
    let mut app = opened(
        120,
        40,
        vec![ancestor("a0", "zoe"), ancestor_tomb("a1")],
        2,
        others(0, 2),
        None,
    );
    for _ in 0..3 {
        assert!(key(&mut app, 'o').is_empty());
        assert!(state(&app, MAIN).expanded.is_empty());
        key(&mut app, 'k');
    }
}

#[test]
fn a_third_level_can_be_opened_from_a_child() {
    let mut app = opened(120, 40, vec![], 2, others(0, 2), None);
    key(&mut app, 'j');
    let e1 = key(&mut app, 'o');
    deliver_replies(
        &mut app,
        &e1,
        "r0",
        vec![with_replies_count(child("x0", "dan", "r0"), 1)],
        None,
    );
    key(&mut app, 'j');
    let e2 = key(&mut app, 'o');
    assert_eq!(replies_fetches(&e2), vec![fetch(MAIN, "x0", None)]);
    deliver_replies(&mut app, &e2, "x0", vec![child("y0", "erin", "x0")], None);
    key(&mut app, 'j');
    assert_eq!(selected(&app), 3);
    assert_eq!(reply_ids(&app, "x0"), names(&["y0"]));
}

#[test]
fn a_child_response_goes_only_to_its_own_thread_and_parent() {
    let mut app = opened(120, 40, vec![], 2, others(0, 2), None);
    key(&mut app, 'j');
    let effects = key(&mut app, 'o');
    let req = replies_req(&effects, "r0");
    app.update(Action::RepliesLoaded {
        thread: "other".into(),
        parent: "r0".into(),
        req,
        page: page(vec![child("x0", "dan", "r0")], None),
    });
    app.update(Action::RepliesLoaded {
        thread: MAIN.into(),
        parent: "elsewhere".into(),
        req,
        page: page(vec![child("x1", "dan", "elsewhere")], None),
    });
    assert!(app.thread("other").is_none());
    assert!(!state(&app, MAIN).children.contains_key("elsewhere"));
    assert_eq!(reply_ids(&app, "r0"), Vec::<String>::new());
    assert_eq!(reply_ids(&app, MAIN), names(&["r0", "r1"]));
}

#[test]
fn a_child_response_for_a_stale_request_is_dropped() {
    let mut app = opened(120, 40, vec![], 2, others(0, 2), None);
    key(&mut app, 'j');
    let effects = key(&mut app, 'o');
    let req = replies_req(&effects, "r0");
    app.update(Action::RepliesLoaded {
        thread: MAIN.into(),
        parent: "r0".into(),
        req: ReqId(req.0.wrapping_add(1000)),
        page: page(vec![child("x0", "dan", "r0")], None),
    });
    assert_eq!(reply_ids(&app, "r0"), Vec::<String>::new());
    deliver_replies(
        &mut app,
        &effects,
        "r0",
        vec![child("x0", "dan", "r0")],
        None,
    );
    assert_eq!(reply_ids(&app, "r0"), names(&["x0"]));
}

fn with_child_page_pending() -> App {
    let mut app = opened(120, 40, vec![], 2, others(0, 2), None);
    key(&mut app, 'j');
    let effects = key(&mut app, 'o');
    deliver_replies(
        &mut app,
        &effects,
        "r0",
        vec![child("x0", "dan", "r0")],
        Some("c2"),
    );
    // Entries: main 0, r0 1, x0 2, Show more 3, r1 4.
    type_str(&mut app, "jj");
    assert_eq!(selected(&app), 3);
    app
}

#[test]
fn o_on_a_child_show_more_row_requests_the_next_child_page() {
    let mut app = with_child_page_pending();
    let effects = key(&mut app, 'o');
    assert_eq!(
        replies_fetches(&effects),
        vec![fetch(MAIN, "r0", Some("c2"))]
    );
}

#[test]
fn enter_on_a_child_show_more_row_requests_the_next_child_page() {
    let mut app = with_child_page_pending();
    let effects = press(&mut app, KeyCode::Enter);
    assert_eq!(
        replies_fetches(&effects),
        vec![fetch(MAIN, "r0", Some("c2"))]
    );
    assert_eq!(app.screen(), Screen::Thread(MAIN.into()));
}

#[test]
fn a_child_show_more_row_does_not_stack_requests_while_loading() {
    let mut app = with_child_page_pending();
    let first = key(&mut app, 'o');
    assert_eq!(replies_fetches(&first).len(), 1);
    assert!(key(&mut app, 'o').is_empty());
    assert!(press(&mut app, KeyCode::Enter).is_empty());
}

#[test]
fn the_next_child_page_is_appended_after_the_first() {
    let mut app = with_child_page_pending();
    let effects = key(&mut app, 'o');
    deliver_replies(
        &mut app,
        &effects,
        "r0",
        vec![child("x0", "dan", "r0"), child("x1", "erin", "r0")],
        None,
    );
    assert_eq!(reply_ids(&app, "r0"), names(&["x0", "x1"]));
}

#[test]
fn child_lists_are_never_prefetched() {
    let mut app = opened(120, 40, vec![], 2, others(0, 2), None);
    key(&mut app, 'j');
    let effects = key(&mut app, 'o');
    deliver_replies(
        &mut app,
        &effects,
        "r0",
        vec![child("x0", "dan", "r0")],
        Some("c2"),
    );
    assert_eq!(reply_ids(&app, "r0"), names(&["x0"]));
    // Entries: main 0, r0 1, x0 2, Show more 3, r1 4.
    let mut got = Vec::new();
    for _ in 0..4 {
        got.extend(replies_fetches(&key(&mut app, 'j')));
    }
    assert_eq!(selected(&app), 4);
    assert!(got.is_empty());
}

fn failed_prefetch_app() -> App {
    let mut app = opened(120, 40, vec![], 12, others(0, 3), Some("p2"));
    let effects = key(&mut app, 'j');
    app.update(Action::RepliesFailed {
        thread: MAIN.into(),
        parent: MAIN.into(),
        req: replies_req(&effects, MAIN),
        failure: Failure::new("http_500"),
    });
    app
}

/// Collects the page requests over the key presses that select and activate the row,
/// so the test holds whether or not selecting the row already prefetches.
fn first_level_requests(
    activate: impl FnOnce(&mut App) -> Vec<Effect>,
) -> Vec<(String, String, Option<String>)> {
    let mut app = failed_prefetch_app();
    let mut got = replies_fetches(&key(&mut app, 'G'));
    // Entries: main 0, r0 1, r1 2, r2 3, Show 9 more 4.
    assert_eq!(selected(&app), 4);
    got.extend(replies_fetches(&activate(&mut app)));
    got
}

#[test]
fn enter_on_the_first_level_show_more_row_requests_the_next_page_once() {
    let got = first_level_requests(|app| press(app, KeyCode::Enter));
    assert_eq!(got, vec![fetch(MAIN, MAIN, Some("p2"))]);
}

#[test]
fn o_on_the_first_level_show_more_row_requests_the_next_page_once() {
    let got = first_level_requests(|app| key(app, 'o'));
    assert_eq!(got, vec![fetch(MAIN, MAIN, Some("p2"))]);
}

#[test]
fn the_first_level_show_more_row_is_the_last_selectable_item() {
    let mut app = opened(120, 40, vec![], 12, others(0, 3), Some("p2"));
    key(&mut app, 'G');
    assert_eq!(selected(&app), 4);
}

// --- guests -----------------------------------------------------------------

fn guest_thread() -> (App, Vec<Effect>) {
    let mut app = guest(120, 40);
    app.update(Action::CloseModal);
    let effects = open_thread(&mut app, MAIN);
    (app, effects)
}

#[test]
fn a_guest_requests_the_thread_but_not_the_replies() {
    let (_, effects) = guest_thread();
    assert_eq!(thread_fetches(&effects), names(&[MAIN]));
    assert!(replies_fetches(&effects).is_empty());
    assert_eq!(effects.len(), 1);
}

#[test]
fn a_guest_refresh_requests_only_the_thread() {
    let (mut app, effects) = guest_thread();
    deliver_thread(&mut app, &effects, vec![], 2);
    let refetch = key_r(&mut app);
    assert_eq!(thread_fetches(&refetch), names(&[MAIN]));
    assert!(replies_fetches(&refetch).is_empty());
}

#[test]
fn a_guest_can_only_select_the_main_commit() {
    let (mut app, effects) = guest_thread();
    deliver_thread(&mut app, &effects, vec![], 2);
    key(&mut app, 'j');
    key(&mut app, 'G');
    assert_eq!(selected(&app), 0);
}

/// A root-side ancestor: it has no parent of its own in these fixtures.
pub(crate) fn ancestor(id: &str, author: &str) -> ThreadItem {
    item_of(commit_json(id, author, 7200))
}

pub(crate) fn ancestor_tomb(id: &str) -> ThreadItem {
    serde_json::from_value(json!({
        "id": id,
        "parent_id": null,
        "deleted_at": "2026-10-20T11:00:00Z",
        "replies_count": 1,
    }))
    .unwrap()
}

// --- review follow-ups ------------------------------------------------------

#[test]
fn o_on_a_reply_without_replies_requests_nothing() {
    let mut app = opened(120, 40, vec![], 1, vec![item("r1", "bob")], None);
    key(&mut app, 'j');
    assert_eq!(selected(&app), 1);
    assert!(key(&mut app, 'o').is_empty());
    assert!(state(&app, MAIN).expanded.is_empty());
}

#[test]
fn o_after_a_failed_child_request_requests_the_children_again() {
    let mut app = opened(120, 40, vec![], 2, others(0, 2), None);
    key(&mut app, 'j');
    let effects = key(&mut app, 'o');
    app.update(Action::RepliesFailed {
        thread: MAIN.into(),
        parent: "r0".into(),
        req: replies_req(&effects, "r0"),
        failure: Failure::new("http_500"),
    });
    let list = state(&app, MAIN).children.get("r0").cloned();
    let list = list.unwrap_or_default();
    assert_eq!(failed_code(&list.load), Some("http_500"));
    let again = key(&mut app, 'o');
    assert_eq!(replies_fetches(&again), vec![fetch(MAIN, "r0", None)]);
}

#[test]
fn o_twice_on_the_first_level_show_more_row_does_not_stack_requests() {
    let mut app = failed_prefetch_app();
    let mut got = replies_fetches(&key(&mut app, 'G'));
    assert_eq!(selected(&app), 4);
    got.extend(replies_fetches(&key(&mut app, 'o')));
    assert!(key(&mut app, 'o').is_empty());
    assert_eq!(got, vec![fetch(MAIN, MAIN, Some("p2"))]);
}

#[test]
fn a_thread_failure_arriving_after_goto_does_not_recreate_the_state() {
    let mut app = signed_in(120, 40);
    let effects = open_thread(&mut app, MAIN);
    assert!(app.thread(MAIN).is_some());
    type_str(&mut app, "gl");
    app.update(Action::ThreadFailed {
        id: MAIN.into(),
        req: thread_req(&effects, MAIN),
        failure: Failure::new("commit_unavailable"),
    });
    assert!(app.thread(MAIN).is_none());
}

#[test]
fn children_arriving_above_the_selection_do_not_move_it_to_another_item() {
    let mut app = opened(120, 40, vec![], 2, others(0, 2), None);
    key(&mut app, 'j');
    let effects = key(&mut app, 'o');
    key(&mut app, 'j');
    assert_eq!(selected(&app), 2);
    deliver_replies(
        &mut app,
        &effects,
        "r0",
        vec![child("x0", "dan", "r0"), child("x1", "erin", "r0")],
        None,
    );
    // Entries: main 0, r0 1, x0 2, x1 3, r1 4.
    assert_eq!(selected(&app), 4);
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.screen(), Screen::Thread("r1".into()));
}

#[test]
fn a_failed_child_request_keeps_the_selection_on_the_same_item() {
    let mut app = opened(120, 40, vec![], 2, others(0, 2), None);
    key(&mut app, 'j');
    let effects = key(&mut app, 'o');
    key(&mut app, 'j');
    app.update(Action::RepliesFailed {
        thread: MAIN.into(),
        parent: "r0".into(),
        req: replies_req(&effects, "r0"),
        failure: Failure::new("http_500"),
    });
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.screen(), Screen::Thread("r1".into()));
}

#[test]
fn rows_right_after_the_selected_entry_scroll_into_view() {
    let mut app = opened(120, 24, vec![], 12, others(0, 12), None);
    for _ in 0..12 {
        key(&mut app, 'j');
        let s = state(&app, MAIN);
        let v = rows(&app, MAIN, list_width(&app));
        let start = v.entries[s.selected].start;
        let end = v
            .entries
            .get(s.selected + 1)
            .map_or(v.lines.len(), |next| next.start);
        assert!(
            end <= s.scroll as usize + VIEW_120X24,
            "rows {start}..{end} cut off: scroll={}",
            s.scroll
        );
    }
}

#[test]
fn going_back_drops_the_state_of_the_thread_that_was_left() {
    let mut app = opened(120, 40, vec![], 2, others(0, 2), None);
    key(&mut app, 'j');
    press(&mut app, KeyCode::Enter);
    assert!(app.thread("r0").is_some());
    press(&mut app, KeyCode::Esc);
    assert!(app.thread("r0").is_none());
    assert!(app.thread(MAIN).is_some());
    let effects = press(&mut app, KeyCode::Enter);
    assert_eq!(thread_fetches(&effects), names(&["r0"]));
}
