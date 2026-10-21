use crossterm::event::KeyCode;
use serde_json::{Value, json};

use crate::action::{Action, Effect, Failure};
use crate::app::{App, ConnTab, ProfileTab, Screen};
use crate::feed::{FeedKey, FeedPage, ReqId};
use crate::feed_tests::{FOLLOWING, deliver, list_width, started};
use crate::model::{Commit, Meta, Page, User, UserResponse};
use crate::test_support::*;
use crate::thread_tests::{
    MAIN, ancestor, commit_json, commit_of, item, merge, opened, others, tomb, user_json,
};
use crate::ui::connections::rows as conn_rows;
use crate::ui::feed::CardSpan;
use crate::ui::profile::rows;

// --- fixtures ---------------------------------------------------------------

/// A profile owner with every optional field filled in; `extra` overrides single fields.
pub(crate) fn full_user(username: &str, extra: Value) -> Value {
    merge(
        merge(
            user_json(username),
            json!({
                "bio": "Building terminal things.",
                "company": "@laravel",
                "location": "Portugal",
                "website": "alice.dev",
                "created_at": "2026-10-15T13:02:44Z",
                "commits_count": 187,
                "following_count": 87,
                "followers_count": 3647,
            }),
        ),
        extra,
    )
}

/// The signed `GET /v1/users/{u}` body; `extra` holds the top-level fields next to `data`.
pub(crate) fn user_resp(username: &str, extra: Value) -> UserResponse {
    let value = merge(json!({ "data": full_user(username, json!({})) }), extra);
    serde_json::from_value(value).unwrap()
}

pub(crate) fn user_of(value: Value) -> User {
    serde_json::from_value(value).unwrap()
}

pub(crate) fn post(id: &str, author: &str) -> Commit {
    commit_of(commit_json(id, author, 600))
}

pub(crate) fn posts(prefix: &str, from: usize, to: usize, author: &str) -> Vec<Commit> {
    (from..to)
        .map(|i| post(&format!("{prefix}{i}"), author))
        .collect()
}

pub(crate) fn fpage(items: Vec<Commit>, cursor: Option<&str>) -> FeedPage {
    FeedPage {
        items,
        next_cursor: cursor.map(String::from),
        is_ranked: false,
        suggestions: Vec::new(),
    }
}

pub(crate) fn upage(users: Vec<User>, cursor: Option<&str>) -> Page<User> {
    Page {
        data: users,
        meta: Meta {
            next_cursor: cursor.map(String::from),
        },
    }
}

pub(crate) fn users(from: usize, to: usize) -> Vec<User> {
    (from..to)
        .map(|i| user_of(user_json(&format!("u{i}"))))
        .collect()
}

// --- effects ----------------------------------------------------------------

pub(crate) fn user_fetches(effects: &[Effect]) -> Vec<String> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::FetchUser { username, .. } => Some(username.clone()),
            _ => None,
        })
        .collect()
}

pub(crate) fn commit_fetches(effects: &[Effect]) -> Vec<(String, bool, Option<String>)> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::FetchUserCommits {
                username,
                replies,
                cursor,
                ..
            } => Some((username.clone(), *replies, cursor.clone())),
            _ => None,
        })
        .collect()
}

pub(crate) fn conn_fetches(effects: &[Effect]) -> Vec<(String, ConnTab, Option<String>)> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::FetchConnections {
                username,
                tab,
                cursor,
                ..
            } => Some((username.clone(), *tab, cursor.clone())),
            _ => None,
        })
        .collect()
}

/// Falls back to a bogus id so that a missing effect fails the assertions, not an unwrap.
pub(crate) fn user_req(effects: &[Effect]) -> ReqId {
    effects
        .iter()
        .find_map(|e| match e {
            Effect::FetchUser { req, .. } => Some(*req),
            _ => None,
        })
        .unwrap_or(ReqId(u64::MAX))
}

pub(crate) fn commits_req(effects: &[Effect], want_replies: bool) -> ReqId {
    effects
        .iter()
        .find_map(|e| match e {
            Effect::FetchUserCommits { replies, req, .. } if *replies == want_replies => Some(*req),
            _ => None,
        })
        .unwrap_or(ReqId(u64::MAX))
}

pub(crate) fn conn_req(effects: &[Effect]) -> ReqId {
    effects
        .iter()
        .find_map(|e| match e {
            Effect::FetchConnections { req, .. } => Some(*req),
            _ => None,
        })
        .unwrap_or(ReqId(u64::MAX))
}

// --- scenario helpers -------------------------------------------------------

pub(crate) fn open_profile(app: &mut App, name: &str) -> Vec<Effect> {
    app.update(Action::Open(Screen::Profile(name.into())))
}

pub(crate) fn deliver_user(app: &mut App, effects: &[Effect], name: &str, resp: UserResponse) {
    app.update(Action::UserLoaded {
        username: name.into(),
        req: user_req(effects),
        response: Box::new(resp),
    });
}

pub(crate) fn deliver_commits(
    app: &mut App,
    effects: &[Effect],
    name: &str,
    tab: ProfileTab,
    items: Vec<Commit>,
    cursor: Option<&str>,
) {
    app.update(Action::ProfileFeedLoaded {
        username: name.into(),
        tab,
        req: commits_req(effects, tab == ProfileTab::Replies),
        page: fpage(items, cursor),
    });
}

pub(crate) fn deliver_conns(
    app: &mut App,
    effects: &[Effect],
    name: &str,
    tab: ConnTab,
    items: Vec<User>,
    cursor: Option<&str>,
) {
    app.update(Action::ConnectionsLoaded {
        username: name.into(),
        tab,
        req: conn_req(effects),
        page: upage(items, cursor),
    });
}

/// Signed-in view of `name`'s profile with the user and the first page of commits loaded.
pub(crate) fn profile_with(
    app: App,
    name: &str,
    resp: UserResponse,
    items: Vec<Commit>,
    cursor: Option<&str>,
) -> App {
    let mut app = app;
    let effects = open_profile(&mut app, name);
    deliver_user(&mut app, &effects, name, resp);
    deliver_commits(&mut app, &effects, name, ProfileTab::Commits, items, cursor);
    app
}

pub(crate) fn alice(w: u16, h: u16, n: usize) -> App {
    profile_with(
        signed_in(w, h),
        "alice",
        user_resp("alice", json!({"is_following": false})),
        posts("p", 0, n, "alice"),
        None,
    )
}

/// Opens Followers of `name` with `items` loaded and `cursor` as the next page.
pub(crate) fn followers(w: u16, h: u16, name: &str, items: Vec<User>, cursor: Option<&str>) -> App {
    let mut app = signed_in(w, h);
    let effects = app.update(Action::Open(Screen::Connections(
        name.into(),
        ConnTab::Followers,
    )));
    deliver_conns(&mut app, &effects, name, ConnTab::Followers, items, cursor);
    app
}

pub(crate) fn key(app: &mut App, c: char) -> Vec<Effect> {
    press(app, KeyCode::Char(c))
}

fn presses(app: &mut App, c: char, n: usize) -> Vec<Effect> {
    (0..n).flat_map(|_| key(app, c)).collect()
}

pub(crate) fn dismissed_guest() -> App {
    let mut app = guest(120, 40);
    app.update(Action::CloseModal);
    app
}

fn profile_of(app: &App, name: &str) -> crate::profile::ProfileState {
    assert!(app.profile(name).is_some(), "no state for profile {name}");
    app.profile(name).cloned().unwrap_or_default()
}

// --- 1. fetching on open ----------------------------------------------------

#[test]
fn opening_a_profile_requests_the_user_and_its_commits() {
    let mut app = signed_in(120, 40);
    let effects = open_profile(&mut app, "alice");
    assert_eq!(app.screen(), Screen::Profile("alice".into()));
    assert_eq!(user_fetches(&effects), vec!["alice"]);
    assert_eq!(
        commit_fetches(&effects),
        vec![("alice".to_string(), false, None)]
    );
}

#[test]
fn the_screen_keeps_the_typed_case_while_state_is_found_without_regard_to_case() {
    let mut app = signed_in(120, 40);
    let effects = open_profile(&mut app, "ALICE");
    assert_eq!(app.screen(), Screen::Profile("ALICE".into()));
    assert_eq!(user_fetches(&effects), vec!["ALICE"]);
    assert!(app.profile("alice").is_some());
    assert!(app.profile("Alice").is_some());
    deliver_user(&mut app, &effects, "alice", user_resp("alice", json!({})));
    assert!(profile_of(&app, "ALICE").user.is_some());
}

#[test]
fn returning_to_a_loaded_profile_requests_nothing_and_keeps_the_selection() {
    let mut app = alice(120, 40, 5);
    presses(&mut app, 'j', 2);
    let effects = press(&mut app, KeyCode::Enter);
    assert_eq!(app.screen(), Screen::Thread("p2".into()));
    assert!(!effects.is_empty());
    let back = press(&mut app, KeyCode::Esc);
    assert_eq!(app.screen(), Screen::Profile("alice".into()));
    assert!(back.is_empty(), "{back:?}");
    assert_eq!(profile_of(&app, "alice").selected(), 2);
}

#[test]
fn a_user_response_fills_the_user_and_clears_the_pending_request() {
    let mut app = signed_in(120, 40);
    let effects = open_profile(&mut app, "alice");
    assert_eq!(
        profile_of(&app, "alice").user_pending,
        Some(user_req(&effects))
    );
    deliver_user(&mut app, &effects, "alice", user_resp("alice", json!({})));
    let state = profile_of(&app, "alice");
    assert_eq!(state.user.map(|u| u.data.username), Some("alice".into()));
    assert_eq!(state.user_pending, None);
}

// --- 2. tabs ----------------------------------------------------------------

#[test]
fn switching_to_replies_requests_them_once() {
    let mut app = alice(120, 40, 3);
    let effects = key(&mut app, '2');
    assert_eq!(
        commit_fetches(&effects),
        vec![("alice".to_string(), true, None)]
    );
    assert_eq!(profile_of(&app, "alice").tab, ProfileTab::Replies);
    assert!(key(&mut app, '1').is_empty());
    assert_eq!(profile_of(&app, "alice").tab, ProfileTab::Commits);
    assert!(key(&mut app, '2').is_empty());
}

#[test]
fn replies_and_commits_keep_separate_lists() {
    let mut app = alice(120, 40, 3);
    let effects = key(&mut app, '2');
    deliver_commits(
        &mut app,
        &effects,
        "alice",
        ProfileTab::Replies,
        posts("q", 0, 2, "alice"),
        None,
    );
    let state = profile_of(&app, "alice");
    assert_eq!(state.commits.items.len(), 3);
    assert_eq!(state.replies.items.len(), 2);
}

// --- 3. stale responses -----------------------------------------------------

#[test]
fn a_profile_feed_response_for_another_request_is_dropped() {
    let mut app = signed_in(120, 40);
    let effects = open_profile(&mut app, "alice");
    app.update(Action::ProfileFeedLoaded {
        username: "alice".into(),
        tab: ProfileTab::Commits,
        req: ReqId(u64::MAX),
        page: fpage(posts("p", 0, 3, "alice"), None),
    });
    assert!(profile_of(&app, "alice").commits.items.is_empty());
    deliver_commits(
        &mut app,
        &effects,
        "alice",
        ProfileTab::Commits,
        posts("p", 0, 3, "alice"),
        None,
    );
    assert_eq!(profile_of(&app, "alice").commits.items.len(), 3);
}

#[test]
fn a_profile_feed_response_for_the_other_tab_is_dropped() {
    let mut app = signed_in(120, 40);
    let effects = open_profile(&mut app, "alice");
    app.update(Action::ProfileFeedLoaded {
        username: "alice".into(),
        tab: ProfileTab::Replies,
        req: commits_req(&effects, false),
        page: fpage(posts("p", 0, 3, "alice"), None),
    });
    let state = profile_of(&app, "alice");
    assert!(state.replies.items.is_empty());
    assert!(state.commits.items.is_empty());
}

#[test]
fn a_profile_feed_failure_for_another_request_is_dropped() {
    let mut app = signed_in(120, 40);
    open_profile(&mut app, "alice");
    app.update(Action::ProfileFeedFailed {
        username: "alice".into(),
        tab: ProfileTab::Commits,
        req: ReqId(u64::MAX),
        failure: Failure::new("http_500"),
    });
    assert!(profile_of(&app, "alice").commits.pending.is_some());
}

#[test]
fn a_user_response_for_a_superseded_request_is_dropped() {
    let mut app = signed_in(120, 40);
    let first = open_profile(&mut app, "alice");
    let second = key(&mut app, 'R');
    deliver_user(&mut app, &first, "alice", user_resp("alice", json!({})));
    assert!(profile_of(&app, "alice").user.is_none());
    deliver_user(&mut app, &second, "alice", user_resp("alice", json!({})));
    assert!(profile_of(&app, "alice").user.is_some());
}

#[test]
fn a_user_failure_for_another_request_is_dropped() {
    let mut app = signed_in(120, 40);
    open_profile(&mut app, "alice");
    app.update(Action::UserFailed {
        username: "alice".into(),
        req: ReqId(u64::MAX),
        failure: Failure::new("user_unavailable"),
        blocked_user: None,
    });
    assert_eq!(profile_of(&app, "alice").error, None);
}

#[test]
fn a_connections_response_for_another_request_is_dropped() {
    let mut app = signed_in(120, 40);
    let effects = app.update(Action::Open(Screen::Connections(
        "alice".into(),
        ConnTab::Followers,
    )));
    app.update(Action::ConnectionsLoaded {
        username: "alice".into(),
        tab: ConnTab::Followers,
        req: ReqId(u64::MAX),
        page: upage(users(0, 3), None),
    });
    let stale_state = app.connections("alice", ConnTab::Followers).cloned();
    assert!(stale_state.is_some_and(|s| s.users.is_empty()));
    deliver_conns(
        &mut app,
        &effects,
        "alice",
        ConnTab::Followers,
        users(0, 3),
        None,
    );
    let state = app.connections("alice", ConnTab::Followers).cloned();
    assert_eq!(state.map(|s| s.users.len()), Some(3));
}

// --- refresh ----------------------------------------------------------------

#[test]
fn r_requests_the_user_and_the_current_tab_again() {
    let mut app = alice(120, 40, 3);
    let effects = key(&mut app, 'R');
    assert_eq!(user_fetches(&effects), vec!["alice"]);
    assert_eq!(
        commit_fetches(&effects),
        vec![("alice".to_string(), false, None)]
    );

    key(&mut app, '2');
    let effects = key(&mut app, 'R');
    assert_eq!(user_fetches(&effects), vec!["alice"]);
    assert_eq!(
        commit_fetches(&effects),
        vec![("alice".to_string(), true, None)]
    );
}

#[test]
fn r_retries_a_failed_user_request() {
    let mut app = signed_in(120, 40);
    let effects = open_profile(&mut app, "alice");
    app.update(Action::UserFailed {
        username: "alice".into(),
        req: user_req(&effects),
        failure: Failure::new("http_500"),
        blocked_user: None,
    });
    assert_eq!(profile_of(&app, "alice").error.as_deref(), Some("http_500"));
    let effects = key(&mut app, 'R');
    assert_eq!(user_fetches(&effects), vec!["alice"]);
    deliver_user(&mut app, &effects, "alice", user_resp("alice", json!({})));
    let state = profile_of(&app, "alice");
    assert_eq!(state.error, None);
    assert!(state.user.is_some());
}

// --- 4. @ opens the author --------------------------------------------------

fn authored(id: &str, author: &str) -> Commit {
    post(id, author)
}

#[test]
fn at_opens_the_author_of_the_selected_log_card() {
    let (mut app, effects) = started(120, 40);
    let page = fpage(vec![authored("c0", "alice"), authored("c1", "bob")], None);
    deliver(&mut app, FOLLOWING, &effects, page);
    key(&mut app, 'j');
    let effects = key(&mut app, '@');
    assert_eq!(app.screen(), Screen::Profile("bob".into()));
    assert_eq!(user_fetches(&effects), vec!["bob"]);
    press(&mut app, KeyCode::Esc);
    assert!(matches!(app.screen(), Screen::Log(_)));
}

#[test]
fn at_opens_the_author_of_the_selected_saved_card() {
    let mut app = signed_in(120, 40);
    let effects = app.update(Action::Goto(Screen::Saved));
    deliver(
        &mut app,
        FeedKey::Saved,
        &effects,
        fpage(vec![authored("s0", "carol")], None),
    );
    key(&mut app, '@');
    assert_eq!(app.screen(), Screen::Profile("carol".into()));
}

#[test]
fn at_opens_the_author_of_the_selected_thread_item() {
    let mut app = opened(
        120,
        50,
        vec![ancestor("a1", "zed")],
        2,
        vec![item("r1", "bob"), item("r2", "carol")],
        None,
    );
    key(&mut app, '@');
    assert_eq!(app.screen(), Screen::Profile("alice".into()));
    press(&mut app, KeyCode::Esc);
    key(&mut app, 'k');
    key(&mut app, '@');
    assert_eq!(app.screen(), Screen::Profile("zed".into()));
    press(&mut app, KeyCode::Esc);
    key(&mut app, 'j');
    key(&mut app, 'j');
    key(&mut app, '@');
    assert_eq!(app.screen(), Screen::Profile("bob".into()));
}

#[test]
fn at_does_nothing_on_a_tombstone_in_a_thread() {
    let mut app = opened(120, 50, vec![], 1, vec![tomb("t1")], None);
    // The same key works on the main commit, so the silence below is the tombstone's.
    key(&mut app, '@');
    assert_eq!(app.screen(), Screen::Profile("alice".into()));
    press(&mut app, KeyCode::Esc);
    key(&mut app, 'j');
    let effects = key(&mut app, '@');
    assert_eq!(app.screen(), Screen::Thread(MAIN.into()));
    assert!(effects.is_empty());
}

#[test]
fn at_does_nothing_on_a_show_more_row() {
    let mut app = opened(120, 40, vec![], 12, others(0, 3), Some("p2"));
    // The same key works on the main commit, so the silence below is the show-more row's.
    key(&mut app, '@');
    assert_eq!(app.screen(), Screen::Profile("alice".into()));
    press(&mut app, KeyCode::Esc);
    key(&mut app, 'G');
    let effects = key(&mut app, '@');
    assert_eq!(app.screen(), Screen::Thread(MAIN.into()));
    assert!(user_fetches(&effects).is_empty());
}

#[test]
fn at_on_a_card_of_another_author_in_a_profile_list_opens_that_author() {
    let mut app = profile_with(
        signed_in(120, 40),
        "alice",
        user_resp("alice", json!({})),
        vec![authored("p0", "bob")],
        None,
    );
    key(&mut app, '@');
    assert_eq!(app.screen(), Screen::Profile("bob".into()));
}

#[test]
fn at_on_the_profile_being_viewed_does_nothing() {
    for typed in ["alice", "ALICE"] {
        let mut app = profile_with(
            signed_in(120, 40),
            typed,
            user_resp("alice", json!({})),
            posts("p", 0, 2, "alice"),
            None,
        );
        assert!(profile_of(&app, typed).user.is_some());
        let effects = key(&mut app, '@');
        assert_eq!(app.screen(), Screen::Profile(typed.into()));
        assert!(effects.is_empty(), "{effects:?}");
    }
}

// --- 5. 3 / 4 open the connections ------------------------------------------

#[test]
fn three_and_four_open_followers_and_following() {
    let mut app = alice(120, 40, 2);
    let effects = key(&mut app, '3');
    assert_eq!(
        app.screen(),
        Screen::Connections("alice".into(), ConnTab::Followers)
    );
    assert_eq!(
        conn_fetches(&effects),
        vec![("alice".to_string(), ConnTab::Followers, None)]
    );
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.screen(), Screen::Profile("alice".into()));
    let effects = key(&mut app, '4');
    assert_eq!(
        app.screen(),
        Screen::Connections("alice".into(), ConnTab::Following)
    );
    assert_eq!(
        conn_fetches(&effects),
        vec![("alice".to_string(), ConnTab::Following, None)]
    );
}

#[test]
fn the_connections_keep_the_typed_case_of_the_username() {
    let mut app = profile_with(
        signed_in(120, 40),
        "ALICE",
        user_resp("alice", json!({})),
        posts("p", 0, 1, "alice"),
        None,
    );
    let effects = key(&mut app, '3');
    assert_eq!(
        app.screen(),
        Screen::Connections("ALICE".into(), ConnTab::Followers)
    );
    assert_eq!(
        conn_fetches(&effects),
        vec![("ALICE".to_string(), ConnTab::Followers, None)]
    );
}

// --- 6. connections: tabs, selection, Enter ---------------------------------

#[test]
fn one_and_two_replace_the_screen_without_a_history_entry() {
    let mut app = alice(120, 40, 2);
    let effects = key(&mut app, '3');
    deliver_conns(
        &mut app,
        &effects,
        "alice",
        ConnTab::Followers,
        users(0, 2),
        None,
    );
    let effects = key(&mut app, '2');
    assert_eq!(
        app.screen(),
        Screen::Connections("alice".into(), ConnTab::Following)
    );
    assert_eq!(
        conn_fetches(&effects),
        vec![("alice".to_string(), ConnTab::Following, None)]
    );
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.screen(), Screen::Profile("alice".into()));
}

#[test]
fn switching_back_to_a_loaded_connections_tab_requests_nothing() {
    let mut app = followers(120, 40, "alice", users(0, 2), None);
    let effects = key(&mut app, '2');
    assert_eq!(conn_fetches(&effects).len(), 1);
    deliver_conns(
        &mut app,
        &effects,
        "alice",
        ConnTab::Following,
        users(5, 7),
        None,
    );
    assert!(key(&mut app, '1').is_empty());
    assert_eq!(
        app.screen(),
        Screen::Connections("alice".into(), ConnTab::Followers)
    );
    assert!(key(&mut app, '1').is_empty());
}

#[test]
fn j_k_and_the_jump_keys_move_the_connections_selection() {
    let mut app = followers(120, 40, "alice", users(0, 3), None);
    let sel = |app: &App| {
        app.connections("alice", ConnTab::Followers)
            .map_or(usize::MAX, |s| s.selected)
    };
    assert_eq!(sel(&app), 0);
    key(&mut app, 'j');
    assert_eq!(sel(&app), 1);
    key(&mut app, 'j');
    key(&mut app, 'j');
    assert_eq!(sel(&app), 2);
    key(&mut app, 'k');
    assert_eq!(sel(&app), 1);
    key(&mut app, 'G');
    assert_eq!(sel(&app), 2);
    type_str(&mut app, "gg");
    assert_eq!(sel(&app), 0);
    key(&mut app, 'k');
    assert_eq!(sel(&app), 0);
}

#[test]
fn enter_on_a_connection_opens_its_profile_and_esc_keeps_the_selection() {
    let mut app = followers(120, 40, "alice", users(0, 3), None);
    key(&mut app, 'j');
    let effects = press(&mut app, KeyCode::Enter);
    assert_eq!(app.screen(), Screen::Profile("u1".into()));
    assert_eq!(user_fetches(&effects), vec!["u1"]);
    let back = press(&mut app, KeyCode::Esc);
    assert!(conn_fetches(&back).is_empty() && user_fetches(&back).is_empty());
    assert_eq!(
        app.screen(),
        Screen::Connections("alice".into(), ConnTab::Followers)
    );
    assert_eq!(
        app.connections("alice", ConnTab::Followers)
            .map(|s| s.selected),
        Some(1)
    );
}

#[test]
fn a_connections_response_fills_the_list() {
    let app = followers(120, 40, "alice", users(0, 4), Some("n1"));
    let state = app.connections("alice", ConnTab::Followers).cloned();
    let state = state.unwrap_or_default();
    assert_eq!(state.users.len(), 4);
    assert_eq!(state.next_cursor.as_deref(), Some("n1"));
    assert!(state.pending.is_none());
}

// --- 7. prefetch ------------------------------------------------------------

#[test]
fn connections_prefetch_the_next_page_five_rows_before_the_end() {
    let mut app = followers(120, 40, "alice", users(0, 30), Some("n1"));
    let early = presses(&mut app, 'j', 23);
    assert!(conn_fetches(&early).is_empty(), "{early:?}");
    let effects = key(&mut app, 'j');
    assert_eq!(
        conn_fetches(&effects),
        vec![("alice".to_string(), ConnTab::Followers, Some("n1".into()))]
    );
    assert!(conn_fetches(&key(&mut app, 'j')).is_empty());
}

#[test]
fn connections_without_a_cursor_never_prefetch() {
    let mut app = followers(120, 40, "alice", users(0, 30), None);
    let loaded = app
        .connections("alice", ConnTab::Followers)
        .map(|s| s.users.len());
    assert_eq!(loaded, Some(30));
    let effects = key(&mut app, 'G');
    assert!(conn_fetches(&effects).is_empty());
}

#[test]
fn profile_commits_prefetch_counts_the_pinned_commit() {
    let pinned = commit_json("pin", "alice", 900);
    let mut app = profile_with(
        signed_in(120, 40),
        "alice",
        user_resp("alice", json!({"pinned_commit": pinned})),
        posts("p", 0, 30, "alice"),
        Some("n1"),
    );
    let early = presses(&mut app, 'j', 24);
    assert!(commit_fetches(&early).is_empty(), "{early:?}");
    let effects = key(&mut app, 'j');
    assert_eq!(
        commit_fetches(&effects),
        vec![("alice".to_string(), false, Some("n1".into()))]
    );
}

// --- 8. guests --------------------------------------------------------------

#[test]
fn a_guest_can_open_someone_elses_profile() {
    let mut app = dismissed_guest();
    let effects = open_profile(&mut app, "bob");
    assert_eq!(app.screen(), Screen::Profile("bob".into()));
    assert_eq!(app.modal(), None);
    assert_eq!(user_fetches(&effects), vec!["bob"]);
    assert_eq!(
        commit_fetches(&effects),
        vec![("bob".to_string(), false, None)]
    );
}

#[test]
fn a_guest_can_open_the_connections_of_someone() {
    let mut app = dismissed_guest();
    let effects = app.update(Action::Open(Screen::Connections(
        "bob".into(),
        ConnTab::Following,
    )));
    assert_eq!(app.modal(), None);
    assert_eq!(
        conn_fetches(&effects),
        vec![("bob".to_string(), ConnTab::Following, None)]
    );
}

#[test]
fn g_p_asks_a_guest_to_sign_in() {
    let mut app = dismissed_guest();
    type_str(&mut app, "gp");
    assert_eq!(app.modal(), Some(crate::app::Modal::SignIn));
    assert!(matches!(app.screen(), Screen::Log(_)));
}

#[test]
fn the_nav_profile_item_asks_a_guest_to_sign_in() {
    let mut app = dismissed_guest();
    press(&mut app, KeyCode::Tab);
    type_str(&mut app, "jj");
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.modal(), Some(crate::app::Modal::SignIn));
    assert!(matches!(app.screen(), Screen::Log(_)));
}

#[test]
fn a_signed_in_user_opens_the_own_profile_by_g_p() {
    let mut app = signed_in(120, 40);
    let effects = type_str(&mut app, "gp");
    assert_eq!(app.screen(), Screen::Profile("myuron".into()));
    assert_eq!(user_fetches(&effects), vec!["myuron"]);
}

// --- 9. errors --------------------------------------------------------------

fn fail_user(app: &mut App, effects: &[Effect], code: &str, blocked: Option<User>) {
    app.update(Action::UserFailed {
        username: "alice".into(),
        req: user_req(effects),
        failure: Failure::new(code),
        blocked_user: blocked.map(Box::new),
    });
}

#[test]
fn a_blocked_user_error_keeps_the_user_from_the_error_body() {
    let mut app = signed_in(120, 40);
    let effects = open_profile(&mut app, "alice");
    fail_user(
        &mut app,
        &effects,
        "user_blocked",
        Some(user_of(user_json("alice"))),
    );
    let state = profile_of(&app, "alice");
    assert_eq!(state.error.as_deref(), Some("user_blocked"));
    assert_eq!(state.blocked_user.map(|u| u.username), Some("alice".into()));
    assert!(state.user.is_none());
    assert_eq!(state.user_pending, None);
}

#[test]
fn an_unavailable_or_missing_user_is_recorded_by_code() {
    for code in ["user_unavailable", "http_404"] {
        let mut app = signed_in(120, 40);
        let effects = open_profile(&mut app, "alice");
        fail_user(&mut app, &effects, code, None);
        let state = profile_of(&app, "alice");
        assert_eq!(state.error.as_deref(), Some(code));
        assert!(state.blocked_user.is_none());
    }
}

#[test]
fn three_and_four_do_nothing_on_a_blocked_or_unavailable_profile() {
    for (code, blocked) in [
        ("user_blocked", Some(user_of(user_json("alice")))),
        ("user_unavailable", None),
    ] {
        let mut app = signed_in(120, 40);
        let effects = open_profile(&mut app, "alice");
        fail_user(&mut app, &effects, code, blocked);
        assert_eq!(profile_of(&app, "alice").error.as_deref(), Some(code));
        for k in ['3', '4'] {
            let out = key(&mut app, k);
            assert!(out.is_empty(), "{code} {k}: {out:?}");
            assert_eq!(app.screen(), Screen::Profile("alice".into()), "{code} {k}");
        }
    }
}

#[test]
fn three_opens_the_connections_once_the_profile_loaded() {
    let mut app = alice(120, 40, 1);
    key(&mut app, '3');
    assert!(matches!(app.screen(), Screen::Connections(..)));
}

// --- 10. Goto drops the states ---------------------------------------------

#[test]
fn goto_drops_the_profile_and_connection_states() {
    let mut app = alice(120, 40, 2);
    let effects = key(&mut app, '3');
    deliver_conns(
        &mut app,
        &effects,
        "alice",
        ConnTab::Followers,
        users(0, 2),
        None,
    );
    type_str(&mut app, "gl");
    assert!(app.profile("alice").is_none());
    assert!(app.connections("alice", ConnTab::Followers).is_none());
    let effects = open_profile(&mut app, "alice");
    assert_eq!(user_fetches(&effects), vec!["alice"]);
}

#[test]
fn late_responses_after_goto_do_not_bring_the_states_back() {
    let mut app = signed_in(120, 40);
    let profile = open_profile(&mut app, "alice");
    let conns = app.update(Action::Open(Screen::Connections(
        "alice".into(),
        ConnTab::Followers,
    )));
    assert!(app.profile("alice").is_some());
    assert!(app.connections("alice", ConnTab::Followers).is_some());
    type_str(&mut app, "gl");
    deliver_user(&mut app, &profile, "alice", user_resp("alice", json!({})));
    deliver_commits(
        &mut app,
        &profile,
        "alice",
        ProfileTab::Commits,
        posts("p", 0, 2, "alice"),
        None,
    );
    app.update(Action::ProfileFeedFailed {
        username: "alice".into(),
        tab: ProfileTab::Replies,
        req: ReqId(1),
        failure: Failure::new("http_500"),
    });
    app.update(Action::UserFailed {
        username: "alice".into(),
        req: user_req(&profile),
        failure: Failure::new("user_unavailable"),
        blocked_user: None,
    });
    deliver_conns(
        &mut app,
        &conns,
        "alice",
        ConnTab::Followers,
        users(0, 2),
        None,
    );
    app.update(Action::ConnectionsFailed {
        username: "alice".into(),
        tab: ConnTab::Followers,
        req: conn_req(&conns),
        failure: Failure::new("http_500"),
    });
    assert!(app.profile("alice").is_none());
    assert!(app.connections("alice", ConnTab::Followers).is_none());
}

// --- pinned commit ----------------------------------------------------------

fn pinned_alice(order_user_first: bool) -> App {
    let mut app = signed_in(120, 40);
    let effects = open_profile(&mut app, "alice");
    let resp = user_resp(
        "alice",
        json!({"pinned_commit": commit_json("pin", "alice", 900)}),
    );
    let items = posts("p", 0, 2, "alice");
    if order_user_first {
        deliver_user(&mut app, &effects, "alice", resp);
        deliver_commits(
            &mut app,
            &effects,
            "alice",
            ProfileTab::Commits,
            items,
            None,
        );
    } else {
        deliver_commits(
            &mut app,
            &effects,
            "alice",
            ProfileTab::Commits,
            items,
            None,
        );
        deliver_user(&mut app, &effects, "alice", resp);
    }
    app
}

fn thread_target_of_each_card(mut app: App) -> Vec<Screen> {
    let mut out = Vec::new();
    for step in 0..3 {
        if step > 0 {
            key(&mut app, 'j');
        }
        press(&mut app, KeyCode::Enter);
        out.push(app.screen());
        press(&mut app, KeyCode::Esc);
    }
    out
}

#[test]
fn the_pinned_commit_is_card_zero_whichever_response_comes_first() {
    let want = vec![
        Screen::Thread("pin".into()),
        Screen::Thread("p0".into()),
        Screen::Thread("p1".into()),
    ];
    assert_eq!(thread_target_of_each_card(pinned_alice(true)), want);
    assert_eq!(thread_target_of_each_card(pinned_alice(false)), want);
}

#[test]
fn the_pinned_commit_is_listed_once_when_the_feed_holds_it_too() {
    let mut app = signed_in(120, 40);
    let effects = open_profile(&mut app, "alice");
    deliver_user(
        &mut app,
        &effects,
        "alice",
        user_resp(
            "alice",
            json!({"pinned_commit": commit_json("pin", "alice", 900)}),
        ),
    );
    deliver_commits(
        &mut app,
        &effects,
        "alice",
        ProfileTab::Commits,
        vec![post("pin", "alice"), post("p1", "alice")],
        None,
    );
    let cards = rows(&app, "alice", list_width(&app)).cards.len();
    assert_eq!(cards, 2);
}

#[test]
fn replies_ignore_the_pinned_commit() {
    let mut app = pinned_alice(true);
    let effects = key(&mut app, '2');
    deliver_commits(
        &mut app,
        &effects,
        "alice",
        ProfileTab::Replies,
        posts("q", 0, 2, "alice"),
        None,
    );
    let cards = rows(&app, "alice", list_width(&app)).cards.len();
    assert_eq!(cards, 2);
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.screen(), Screen::Thread("q0".into()));
}

// --- 11. scrolling ----------------------------------------------------------

/// Hand-counted: 24 rows minus the status line and the two borders of Main.
const VIEW_120X24: usize = 21;

#[test]
fn the_selected_card_stays_visible_while_the_header_scrolls_away() {
    let mut app = alice(120, 24, 15);
    assert_eq!(profile_of(&app, "alice").scroll(), 0);
    for step in 1..=14 {
        key(&mut app, 'j');
        let state = profile_of(&app, "alice");
        let card = rows(&app, "alice", list_width(&app))
            .cards
            .get(state.selected())
            .copied();
        assert!(card.is_some(), "step {step}: no card {}", state.selected());
        let card = card.unwrap_or(CardSpan {
            start: 0,
            height: 0,
        });
        let scroll = usize::from(state.scroll());
        assert!(
            card.start >= scroll && card.start + card.height <= scroll + VIEW_120X24,
            "step {step}: card {card:?} outside {scroll}..{}",
            scroll + VIEW_120X24
        );
    }
    assert!(profile_of(&app, "alice").scroll() > 0);
}

#[test]
fn the_scroll_returns_to_zero_at_the_first_card() {
    let mut app = alice(120, 24, 15);
    key(&mut app, 'G');
    assert!(profile_of(&app, "alice").scroll() > 0);
    type_str(&mut app, "gg");
    let state = profile_of(&app, "alice");
    assert_eq!(state.selected(), 0);
    assert_eq!(state.scroll(), 0);
}

#[test]
fn the_header_scrolls_off_the_screen_with_the_list() {
    let mut app = alice(120, 24, 15);
    let text = |app: &App| {
        let terminal = render(app);
        let buffer = terminal.backend().buffer().clone();
        (0..buffer.area.height)
            .map(|y| row_text(&buffer, y))
            .collect::<Vec<_>>()
            .join("\n")
    };
    assert!(text(&app).contains("[f] Follow"));
    key(&mut app, 'G');
    assert!(!text(&app).contains("[f] Follow"));
}

// --- connections: scrolling and refresh ------------------------------------

/// Hand-counted: 24 rows minus the status line, the two borders and the three fixed rows above the list.
const CONN_VIEW_120X24: usize = 18;

#[test]
fn the_last_connection_is_visible_after_g_and_gg_returns_to_the_top() {
    let mut app = followers(120, 24, "alice", users(0, 30), None);
    key(&mut app, 'G');
    let state = app
        .connections("alice", ConnTab::Followers)
        .cloned()
        .unwrap_or_default();
    let last = conn_rows(&app, "alice", ConnTab::Followers, list_width(&app))
        .users
        .get(29)
        .copied();
    assert!(last.is_some());
    let last = last.unwrap_or(CardSpan {
        start: 0,
        height: 0,
    });
    let scroll = usize::from(state.scroll);
    assert!(scroll > 0);
    assert!(
        last.start >= scroll && last.start + last.height <= scroll + CONN_VIEW_120X24,
        "{last:?} outside {scroll}..{}",
        scroll + CONN_VIEW_120X24
    );
    type_str(&mut app, "gg");
    let state = app
        .connections("alice", ConnTab::Followers)
        .cloned()
        .unwrap_or_default();
    assert_eq!((state.selected, state.scroll), (0, 0));
}

#[test]
fn r_after_a_failed_first_page_requests_it_again_and_fills_the_list() {
    let mut app = signed_in(120, 40);
    let effects = app.update(Action::Open(Screen::Connections(
        "alice".into(),
        ConnTab::Followers,
    )));
    app.update(Action::ConnectionsFailed {
        username: "alice".into(),
        tab: ConnTab::Followers,
        req: conn_req(&effects),
        failure: Failure::new("http_500"),
    });
    let again = key(&mut app, 'R');
    assert_eq!(
        conn_fetches(&again),
        vec![("alice".to_string(), ConnTab::Followers, None)]
    );
    deliver_conns(
        &mut app,
        &again,
        "alice",
        ConnTab::Followers,
        users(0, 3),
        None,
    );
    let state = app.connections("alice", ConnTab::Followers).cloned();
    assert_eq!(state.map(|s| s.users.len()), Some(3));
}

// --- Back drops the states --------------------------------------------------

#[test]
fn back_from_an_opened_profile_drops_its_state() {
    let (mut app, effects) = started(120, 40);
    let page = fpage(vec![authored("c0", "bob")], None);
    deliver(&mut app, FOLLOWING, &effects, page);
    key(&mut app, '@');
    assert!(app.profile("bob").is_some());
    press(&mut app, KeyCode::Esc);
    assert!(app.profile("bob").is_none());
}

#[test]
fn back_keeps_the_states_that_history_can_still_reach() {
    let mut app = alice(120, 40, 3);
    let effects = key(&mut app, '3');
    deliver_conns(
        &mut app,
        &effects,
        "alice",
        ConnTab::Followers,
        users(0, 3),
        None,
    );
    let effects = key(&mut app, '2');
    deliver_conns(
        &mut app,
        &effects,
        "alice",
        ConnTab::Following,
        users(0, 3),
        None,
    );
    key(&mut app, 'j');
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.screen(), Screen::Profile("u1".into()));
    assert!(app.profile("u1").is_some());

    press(&mut app, KeyCode::Esc);
    assert!(app.profile("u1").is_none());
    assert!(app.connections("alice", ConnTab::Followers).is_some());
    assert!(app.connections("alice", ConnTab::Following).is_some());
    assert!(app.profile("alice").is_some());

    press(&mut app, KeyCode::Esc);
    assert_eq!(app.screen(), Screen::Profile("alice".into()));
    assert!(app.connections("alice", ConnTab::Followers).is_none());
    assert!(app.connections("alice", ConnTab::Following).is_none());
    assert!(app.profile("alice").is_some());
}

#[test]
fn opening_the_connections_again_after_back_dropped_them_requests_them_again() {
    let mut app = alice(120, 40, 2);
    let effects = key(&mut app, '3');
    deliver_conns(
        &mut app,
        &effects,
        "alice",
        ConnTab::Followers,
        users(0, 2),
        None,
    );
    press(&mut app, KeyCode::Esc);
    assert!(app.connections("alice", ConnTab::Followers).is_none());
    let again = key(&mut app, '3');
    assert_eq!(
        conn_fetches(&again),
        vec![("alice".to_string(), ConnTab::Followers, None)]
    );
}

#[test]
fn a_list_that_is_not_shown_cannot_be_acted_on() {
    let mut app = signed_in(120, 40);
    let effects = open_profile(&mut app, "alice");
    deliver_commits(
        &mut app,
        &effects,
        "alice",
        ProfileTab::Commits,
        posts("p", 0, 3, "alice"),
        None,
    );
    app.update(Action::UserFailed {
        username: "alice".into(),
        req: user_req(&effects),
        failure: Failure::new("http_500"),
        blocked_user: None,
    });
    for k in ['j', '@', 'o'] {
        key(&mut app, k);
    }
    let opened = press(&mut app, KeyCode::Enter);
    assert!(opened.is_empty(), "{opened:?}");
    assert_eq!(app.screen(), Screen::Profile("alice".into()));
    assert_eq!(profile_of(&app, "alice").selected(), 0);
}
