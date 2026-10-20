use crossterm::event::KeyCode;
use ratatui::layout::Rect;
use serde_json::{Value, json};
use time::{Duration, UtcOffset};

use crate::action::{Action, Effect};
use crate::app::{App, LogTab, Screen, Session};
use crate::feed::{Feed, FeedKey, FeedPage, LoadState, ReqId, ensure_visible};
use crate::model::Commit;
use crate::test_support::*;
use crate::ui::feed::{CardSpan, rows};
use crate::ui::layout::split;

pub(crate) const FOLLOWING: FeedKey = FeedKey::Log(LogTab::Following);
pub(crate) const LATEST: FeedKey = FeedKey::Log(LogTab::Latest);
pub(crate) const TRENDING: FeedKey = FeedKey::Log(LogTab::Trending);

// --- fixtures ---------------------------------------------------------------

fn user_json(username: &str) -> Value {
    json!({
        "id": format!("user-{username}"),
        "username": username,
        "name": username,
        "created_at": "2026-10-01T00:00:00Z",
        "lane": 1,
    })
}

fn commit_json(i: usize, author: &str) -> Value {
    let created = NOW - Duration::minutes(i as i64 + 1);
    json!({
        "id": format!("c{i}"),
        "short_hash": format!("{i:07}"),
        "body": format!("commit {i}\nsecond line of commit {i}"),
        "parent_id": null,
        "html_url": null,
        "created_at": created.format(&time::format_description::well_known::Rfc3339).unwrap(),
        "stars_count": 1,
        "replies_count": 0,
        "link_preview": null,
        "images": [],
        "author": user_json(author),
        "body_segments": [{"type": "text", "text": format!("commit {i}\nsecond line of commit {i}")}],
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

fn commit(i: usize) -> Commit {
    serde_json::from_value(commit_json(i, "alice")).unwrap()
}

/// A commit whose two replies are previewed once the card is expanded.
fn commit_with_replies(i: usize) -> Commit {
    let mut value = commit_json(i, "alice");
    value["replies_count"] = json!(2);
    value["reply_preview"] = json!({
        "data": [commit_json(900 + i, "bob"), commit_json(950 + i, "carol")],
        "next_cursor": null,
    });
    serde_json::from_value(value).unwrap()
}

fn page(from: usize, to: usize, cursor: Option<&str>) -> FeedPage {
    FeedPage {
        items: (from..to).map(commit).collect(),
        next_cursor: cursor.map(String::from),
        is_ranked: false,
        suggestions: Vec::new(),
    }
}

fn ranked_page(n: usize) -> FeedPage {
    FeedPage {
        is_ranked: true,
        ..page(0, n, None)
    }
}

/// Mimics the run loop's startup: Init, then Resize.
pub(crate) fn started(w: u16, h: u16) -> (App, Vec<Effect>) {
    let mut app = App::new(Session::SignedIn(fixture_me()), NOW, UtcOffset::UTC);
    let mut effects = app.update(Action::Init);
    effects.extend(app.update(Action::Resize(w, h)));
    (app, effects)
}

fn req_of(effect: &Effect) -> Option<ReqId> {
    match effect {
        Effect::FetchLog { req, .. } | Effect::FetchSaved { req, .. } => Some(*req),
        Effect::PollLog { .. }
        | Effect::FetchThread { .. }
        | Effect::FetchReplies { .. }
        | Effect::Quit => None,
    }
}

/// Falls back to a bogus id so that a missing effect fails the assertions, not an unwrap.
pub(crate) fn first_req(effects: &[Effect]) -> ReqId {
    effects.iter().find_map(req_of).unwrap_or(ReqId(u64::MAX))
}

fn log_fetches(effects: &[Effect], tab: LogTab) -> Vec<Option<String>> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::FetchLog { tab: t, cursor, .. } if *t == tab => Some(cursor.clone()),
            _ => None,
        })
        .collect()
}

fn saved_fetches(effects: &[Effect]) -> Vec<Option<String>> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::FetchSaved { cursor, .. } => Some(cursor.clone()),
            _ => None,
        })
        .collect()
}

fn polls(effects: &[Effect]) -> Vec<(LogTab, String)> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::PollLog { tab, since, .. } => Some((*tab, since.clone())),
            _ => None,
        })
        .collect()
}

pub(crate) fn deliver(app: &mut App, key: FeedKey, effects: &[Effect], page: FeedPage) {
    app.update(Action::FeedLoaded {
        key,
        req: first_req(effects),
        page,
    });
}

/// Signed-in Following feed with `n` items and `cursor` as the next page.
fn loaded(w: u16, h: u16, n: usize, cursor: Option<&str>) -> App {
    let (mut app, effects) = started(w, h);
    deliver(&mut app, FOLLOWING, &effects, page(0, n, cursor));
    app
}

/// The append flag of the request in flight, if any.
fn loading(app: &App, key: FeedKey) -> Option<bool> {
    let feed = app.feed(key);
    feed.pending.as_ref().map(|p| p.append)
}

pub(crate) fn down(app: &mut App, n: usize) -> Vec<Effect> {
    let mut effects = Vec::new();
    for _ in 0..n {
        effects.extend(press(app, KeyCode::Char('j')));
    }
    effects
}

fn key_of(app: &App) -> FeedKey {
    match app.screen() {
        Screen::Saved => FeedKey::Saved,
        Screen::Log(tab) => FeedKey::Log(tab),
        other => panic!("not a list screen: {other:?}"),
    }
}

pub(crate) fn list_width(app: &App) -> u16 {
    let (w, h) = app.size();
    split(Rect::new(0, 0, w, h), app.layout_mode())
        .main
        .width
        .saturating_sub(2)
}

fn card_spans(app: &App, key: FeedKey) -> Vec<CardSpan> {
    rows(app.feed(key), key, app, list_width(app)).cards
}

/// Hand-counted rows of the scrolling area (terminal minus status line, borders and, for Log, tabs).
fn list_view_rows(key: FeedKey, w: u16, h: u16) -> usize {
    match (key, w, h) {
        (FeedKey::Log(_), 120, 40) => 35,
        (FeedKey::Log(_), 120, 24) => 19,
        (FeedKey::Log(_), 60, 30) => 22,
        (FeedKey::Log(_), 60, 24) => 16,
        (FeedKey::Log(_), 160, 60) => 55,
        (FeedKey::Saved, 120, 40) => 37,
        _ => panic!("hand-count the list view for {key:?} {w}x{h}"),
    }
}

fn total_lines(app: &App, key: FeedKey) -> usize {
    rows(app.feed(key), key, app, list_width(app)).lines.len()
}

/// Computes visibility from the rows' card spans, the scroll offset and the view height.
fn assert_selected_visible(app: &App) {
    let key = key_of(app);
    let feed: &Feed = app.feed(key);
    let (w, h) = app.size();
    let view = list_view_rows(key, w, h);
    let spans = card_spans(app, key);
    assert_eq!(spans.len(), feed.items.len(), "one span per item");
    let card = spans.get(feed.selected).copied();
    assert!(card.is_some(), "selected {} has no card", feed.selected);
    let card = card.unwrap_or(CardSpan {
        start: 0,
        height: 0,
    });
    let scroll = feed.scroll as usize;
    let top_visible = scroll <= card.start && card.start < scroll + view;
    let fits = card.start + card.height <= scroll + view;
    assert!(
        top_visible && (fits || card.height > view),
        "card {card:?} not visible: scroll={scroll} view={view}"
    );
}

// --- Screen transitions -----------------------------------------------------

#[test]
fn g_p_opens_the_own_profile_by_handle() {
    let mut app = signed_in(120, 40);
    type_str(&mut app, "gp");
    assert_eq!(app.screen(), Screen::Profile("myuron".into()));
}

#[test]
fn enter_opens_the_selected_commit_as_thread_and_esc_returns() {
    let mut app = loaded(120, 40, 5, None);
    down(&mut app, 2);
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.screen(), Screen::Thread("c2".into()));
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.screen(), Screen::Log(LogTab::Following));
}

#[test]
fn enter_on_saved_opens_the_thread() {
    let mut app = loaded(120, 40, 3, None);
    let effects = type_str(&mut app, "gv");
    deliver(&mut app, FeedKey::Saved, &effects, page(10, 13, None));
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.screen(), Screen::Thread("c10".into()));
}

// --- fetch on entering a screen ----------------------------------------------

#[test]
fn init_requests_the_first_following_page_when_signed_in() {
    let (app, effects) = started(120, 40);
    assert_eq!(log_fetches(&effects, LogTab::Following), vec![None]);
    assert_eq!(effects.len(), 1);
    assert_eq!(loading(&app, FOLLOWING), Some(false));
}

#[test]
fn g_v_requests_the_first_saved_page() {
    let (mut app, _) = started(120, 40);
    let effects = type_str(&mut app, "gv");
    assert_eq!(saved_fetches(&effects), vec![None]);
}

#[test]
fn reentering_a_loaded_screen_requests_nothing() {
    let (mut app, effects) = started(120, 40);
    assert_eq!(log_fetches(&effects, LogTab::Following), vec![None]);
    deliver(&mut app, FOLLOWING, &effects, page(0, 3, None));
    assert!(type_str(&mut app, "gs").is_empty());
    assert!(type_str(&mut app, "gl").is_empty());
}

#[test]
fn reentering_a_loaded_screen_through_back_requests_nothing() {
    let mut app = loaded(120, 40, 3, None);
    assert_eq!(app.feed(FOLLOWING).items.len(), 3);
    press(&mut app, KeyCode::Enter);
    assert!(press(&mut app, KeyCode::Esc).is_empty());
    assert_eq!(app.screen(), Screen::Log(LogTab::Following));
}

#[test]
fn guest_log_requests_nothing_on_init_refresh_and_tick() {
    let mut app = App::new(Session::Guest, NOW, UtcOffset::UTC);
    assert!(app.update(Action::Init).is_empty());
    app.update(Action::Resize(120, 40));
    app.update(Action::CloseModal);
    assert!(type_str(&mut app, "R").is_empty());
    assert!(
        app.update(Action::Tick(NOW + Duration::seconds(60)))
            .is_empty()
    );
}

#[test]
fn switching_to_an_unloaded_tab_requests_its_first_page() {
    let mut app = loaded(120, 40, 3, None);
    let effects = type_str(&mut app, "2");
    assert_eq!(log_fetches(&effects, LogTab::Latest), vec![None]);
    assert_eq!(app.screen(), Screen::Log(LogTab::Latest));
}

#[test]
fn goto_retries_a_screen_whose_first_load_failed() {
    let (mut app, effects) = started(120, 40);
    app.update(Action::FeedFailed {
        key: FOLLOWING,
        req: first_req(&effects),
        code: "http_500".into(),
    });
    type_str(&mut app, "gs");
    let effects = type_str(&mut app, "gl");
    assert_eq!(log_fetches(&effects, LogTab::Following), vec![None]);
}

#[test]
fn back_retries_a_screen_whose_first_load_failed() {
    let (mut app, effects) = started(120, 40);
    app.update(Action::FeedFailed {
        key: FOLLOWING,
        req: first_req(&effects),
        code: "http_500".into(),
    });
    app.update(Action::Open(Screen::Thread("c0".into())));
    let effects = press(&mut app, KeyCode::Esc);
    assert_eq!(log_fetches(&effects, LogTab::Following), vec![None]);
}

#[test]
fn goto_shows_the_cache_with_selection_and_scroll_reset() {
    let mut app = loaded(120, 40, 20, Some("cur"));
    down(&mut app, 12);
    assert_eq!(app.feed(FOLLOWING).selected, 12);
    assert!(app.feed(FOLLOWING).scroll > 0);
    type_str(&mut app, "gs");
    let effects = type_str(&mut app, "gl");
    assert!(effects.is_empty());
    assert_eq!(app.feed(FOLLOWING).items.len(), 20);
    assert_eq!(app.feed(FOLLOWING).selected, 0);
    assert_eq!(app.feed(FOLLOWING).scroll, 0);
}

// --- FeedLoaded / FeedFailed --------------------------------------------------

#[test]
fn first_page_fills_items_and_resets_selection() {
    let app = loaded(120, 40, 5, Some("cur"));
    let feed = app.feed(FOLLOWING);
    assert_eq!(feed.items.len(), 5);
    assert_eq!(feed.selected, 0);
    assert_eq!(feed.next_cursor.as_deref(), Some("cur"));
    assert_eq!(feed.load, LoadState::Idle);
    assert!(feed.loaded_once);
    assert!(feed.pending.is_none());
}

#[test]
fn appended_page_extends_items_and_keeps_selection() {
    let mut app = loaded(120, 40, 20, Some("cur"));
    let effects = down(&mut app, 14);
    assert_eq!(log_fetches(&effects, LogTab::Following).len(), 1);
    deliver(&mut app, FOLLOWING, &effects, page(20, 25, None));
    let feed = app.feed(FOLLOWING);
    assert_eq!(feed.items.len(), 25);
    assert_eq!(feed.items[24].id, "c24");
    assert_eq!(feed.selected, 14);
    assert_eq!(feed.next_cursor, None);
}

#[test]
fn appended_page_drops_items_already_present() {
    let mut app = loaded(120, 40, 20, Some("cur"));
    let effects = down(&mut app, 14);
    assert_eq!(log_fetches(&effects, LogTab::Following).len(), 1);
    deliver(&mut app, FOLLOWING, &effects, page(18, 25, None));
    let ids: Vec<&str> = app
        .feed(FOLLOWING)
        .items
        .iter()
        .map(|c| c.id.as_str())
        .collect();
    assert_eq!(ids.len(), 25);
    let mut unique = ids.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), 25);
}

#[test]
fn refreshed_page_replaces_items_and_resets_selection_and_scroll() {
    let mut app = loaded(120, 40, 20, Some("cur"));
    down(&mut app, 12);
    assert_eq!(app.feed(FOLLOWING).selected, 12);
    let effects = type_str(&mut app, "R");
    deliver(&mut app, FOLLOWING, &effects, page(100, 104, None));
    let feed = app.feed(FOLLOWING);
    assert_eq!(feed.items.len(), 4);
    assert_eq!(feed.items[0].id, "c100");
    assert_eq!(feed.selected, 0);
    assert_eq!(feed.scroll, 0);
}

#[test]
fn feeds_of_different_keys_do_not_share_items() {
    let mut app = loaded(120, 40, 3, None);
    let effects = type_str(&mut app, "gv");
    deliver(&mut app, FeedKey::Saved, &effects, page(50, 52, None));
    assert_eq!(app.feed(FOLLOWING).items.len(), 3);
    assert_eq!(app.feed(FeedKey::Saved).items.len(), 2);
}

#[test]
fn failed_refresh_keeps_items_and_records_the_code() {
    let mut app = loaded(120, 40, 20, Some("cur"));
    let effects = type_str(&mut app, "R");
    app.update(Action::FeedFailed {
        key: FOLLOWING,
        req: first_req(&effects),
        code: "rate_limited".into(),
    });
    let feed = app.feed(FOLLOWING);
    assert_eq!(feed.items.len(), 20);
    assert_eq!(
        feed.load,
        LoadState::Failed {
            code: "rate_limited".into(),
            append: false
        }
    );
}

#[test]
fn response_to_a_superseded_request_is_ignored() {
    let mut app = loaded(120, 40, 20, Some("cur"));
    let old = down(&mut app, 14);
    assert_eq!(log_fetches(&old, LogTab::Following).len(), 1);
    let refresh = type_str(&mut app, "R");
    assert_eq!(log_fetches(&refresh, LogTab::Following), vec![None]);
    assert_ne!(first_req(&old), first_req(&refresh));
    deliver(&mut app, FOLLOWING, &old, page(20, 25, None));
    assert_eq!(app.feed(FOLLOWING).items.len(), 20);
    deliver(&mut app, FOLLOWING, &refresh, page(100, 104, None));
    assert_eq!(app.feed(FOLLOWING).items.len(), 4);
}

#[test]
fn response_without_a_pending_request_is_ignored() {
    let (mut app, effects) = started(120, 40);
    deliver(&mut app, FOLLOWING, &effects, page(0, 3, None));
    deliver(&mut app, FOLLOWING, &effects, page(100, 110, None));
    assert_eq!(app.feed(FOLLOWING).items.len(), 3);
}

// --- selection keys ----------------------------------------------------------

#[test]
fn j_and_k_move_one_item_and_stop_at_both_ends() {
    let mut app = loaded(120, 40, 3, None);
    press(&mut app, KeyCode::Char('k'));
    assert_eq!(app.feed(FOLLOWING).selected, 0);
    down(&mut app, 1);
    assert_eq!(app.feed(FOLLOWING).selected, 1);
    down(&mut app, 5);
    assert_eq!(app.feed(FOLLOWING).selected, 2);
    press(&mut app, KeyCode::Char('k'));
    assert_eq!(app.feed(FOLLOWING).selected, 1);
}

#[test]
fn arrow_keys_move_the_selection_like_j_and_k() {
    let mut app = loaded(120, 40, 3, None);
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Down);
    assert_eq!(app.feed(FOLLOWING).selected, 2);
    press(&mut app, KeyCode::Up);
    assert_eq!(app.feed(FOLLOWING).selected, 1);
}

#[test]
fn g_g_selects_the_first_and_shift_g_selects_the_last() {
    let mut app = loaded(120, 40, 6, None);
    type_str(&mut app, "G");
    assert_eq!(app.feed(FOLLOWING).selected, 5);
    type_str(&mut app, "gg");
    assert_eq!(app.feed(FOLLOWING).selected, 0);
    assert!(!app.g_pending());
}

#[test]
fn j_with_nav_focus_moves_the_nav_cursor_not_the_selection() {
    let mut app = loaded(120, 40, 5, None);
    down(&mut app, 1);
    assert_eq!(app.feed(FOLLOWING).selected, 1);
    press(&mut app, KeyCode::Tab);
    down(&mut app, 2);
    assert_eq!(app.feed(FOLLOWING).selected, 1);
}

#[test]
fn navigation_keys_on_an_empty_list_change_nothing() {
    let (mut app, started_effects) = started(120, 40);
    let mut effects = Vec::new();
    for key in ['j', 'k', 'o', 'G'] {
        effects.extend(type_str(&mut app, &key.to_string()));
    }
    effects.extend(type_str(&mut app, "gg"));
    effects.extend(press(&mut app, KeyCode::Enter));
    assert!(effects.is_empty());
    assert_eq!(app.screen(), Screen::Log(LogTab::Following));
    let feed = app.feed(FOLLOWING);
    assert_eq!(feed.selected, 0);
    assert!(feed.expanded.is_empty());
    deliver(&mut app, FOLLOWING, &started_effects, page(0, 2, None));
    down(&mut app, 1);
    assert_eq!(app.feed(FOLLOWING).selected, 1);
}

#[test]
fn o_toggles_the_selected_commit_and_changes_its_card_height() {
    let (mut app, effects) = started(120, 40);
    let mut first = page(0, 3, None);
    first.items[1] = commit_with_replies(1);
    deliver(&mut app, FOLLOWING, &effects, first);
    down(&mut app, 1);
    let collapsed = card_spans(&app, FOLLOWING);
    assert_eq!(collapsed.len(), 3);
    type_str(&mut app, "o");
    assert!(app.feed(FOLLOWING).expanded.contains("c1"));
    let expanded = card_spans(&app, FOLLOWING);
    assert!(expanded[1].height > collapsed[1].height);
    type_str(&mut app, "o");
    assert!(!app.feed(FOLLOWING).expanded.contains("c1"));
    assert_eq!(card_spans(&app, FOLLOWING)[1].height, collapsed[1].height);
}

// --- prefetch ----------------------------------------------------------------

#[test]
fn next_page_is_requested_once_when_selection_reaches_the_fifth_from_last() {
    let mut app = loaded(120, 40, 20, Some("cur"));
    for step in 1..=18 {
        let effects = press(&mut app, KeyCode::Char('j'));
        let fetches = log_fetches(&effects, LogTab::Following);
        if step == 14 {
            assert_eq!(fetches, vec![Some("cur".to_string())], "step {step}");
            assert_eq!(loading(&app, FOLLOWING), Some(true));
        } else {
            assert!(fetches.is_empty(), "step {step}");
        }
    }
}

#[test]
fn next_page_is_not_requested_without_a_cursor() {
    let mut app = loaded(120, 40, 20, Some("cur"));
    assert_eq!(log_fetches(&down(&mut app, 14), LogTab::Following).len(), 1);
    let mut app = loaded(120, 40, 20, None);
    assert!(down(&mut app, 19).is_empty());
}

#[test]
fn jumping_to_the_last_item_requests_the_next_page() {
    let mut app = loaded(120, 40, 20, Some("cur"));
    let effects = type_str(&mut app, "G");
    assert_eq!(
        log_fetches(&effects, LogTab::Following),
        vec![Some("cur".to_string())]
    );
}

#[test]
fn failed_append_is_not_retried_by_moving_and_r_refetches_that_page() {
    let mut app = loaded(120, 40, 20, Some("cur"));
    let effects = down(&mut app, 14);
    assert_eq!(log_fetches(&effects, LogTab::Following).len(), 1);
    app.update(Action::FeedFailed {
        key: FOLLOWING,
        req: first_req(&effects),
        code: "http_500".into(),
    });
    assert_eq!(
        app.feed(FOLLOWING).load,
        LoadState::Failed {
            code: "http_500".into(),
            append: true
        }
    );
    assert!(down(&mut app, 1).is_empty());
    let retry = type_str(&mut app, "R");
    assert_eq!(
        log_fetches(&retry, LogTab::Following),
        vec![Some("cur".to_string())]
    );
    assert_eq!(app.feed(FOLLOWING).items.len(), 20);
}

// --- refresh and new commits ---------------------------------------------------

#[test]
fn r_requests_the_first_page_again() {
    let mut app = loaded(120, 40, 5, None);
    let effects = type_str(&mut app, "R");
    assert_eq!(log_fetches(&effects, LogTab::Following), vec![None]);
}

#[test]
fn refreshed_page_clears_the_new_commit_count() {
    let mut app = loaded(120, 40, 5, None);
    app.update(Action::NewCommits {
        key: FOLLOWING,
        since: "c0".into(),
        count: 3,
    });
    assert_eq!(app.feed(FOLLOWING).new_commits, 3);
    let effects = type_str(&mut app, "R");
    deliver(&mut app, FOLLOWING, &effects, page(0, 5, None));
    assert_eq!(app.feed(FOLLOWING).new_commits, 0);
}

#[test]
fn new_commit_count_is_stored_only_for_the_current_first_item() {
    let mut app = loaded(120, 40, 5, None);
    app.update(Action::NewCommits {
        key: FOLLOWING,
        since: "stale".into(),
        count: 9,
    });
    assert_eq!(app.feed(FOLLOWING).new_commits, 0);
    app.update(Action::NewCommits {
        key: FOLLOWING,
        since: "c0".into(),
        count: 2,
    });
    assert_eq!(app.feed(FOLLOWING).new_commits, 2);
}

#[test]
fn k_at_the_top_refreshes_only_when_new_commits_exist() {
    let mut app = loaded(120, 40, 5, None);
    assert!(press(&mut app, KeyCode::Char('k')).is_empty());
    app.update(Action::NewCommits {
        key: FOLLOWING,
        since: "c0".into(),
        count: 2,
    });
    let effects = press(&mut app, KeyCode::Char('k'));
    assert_eq!(log_fetches(&effects, LogTab::Following), vec![None]);
}

// --- polling -----------------------------------------------------------------

fn tick(app: &mut App, seconds: i64) -> Vec<Effect> {
    app.update(Action::Tick(NOW + Duration::seconds(seconds)))
}

#[test]
fn tick_polls_with_the_first_id_after_thirty_seconds_only() {
    let mut app = loaded(120, 40, 5, None);
    assert!(polls(&tick(&mut app, 29)).is_empty());
    assert_eq!(
        polls(&tick(&mut app, 30)),
        vec![(LogTab::Following, "c0".to_string())]
    );
}

#[test]
fn poll_timer_restarts_after_each_poll() {
    let mut app = loaded(120, 40, 5, None);
    assert_eq!(polls(&tick(&mut app, 30)).len(), 1);
    assert!(polls(&tick(&mut app, 59)).is_empty());
    assert_eq!(polls(&tick(&mut app, 60)).len(), 1);
}

#[test]
fn poll_timer_starts_when_the_first_page_arrives() {
    let (mut app, effects) = started(120, 40);
    tick(&mut app, 20);
    deliver(&mut app, FOLLOWING, &effects, page(0, 5, None));
    assert!(polls(&tick(&mut app, 49)).is_empty());
    assert_eq!(polls(&tick(&mut app, 50)).len(), 1);
}

#[test]
fn tick_does_not_poll_an_empty_feed() {
    let (mut app, effects) = started(120, 40);
    assert!(polls(&tick(&mut app, 120)).is_empty());
    deliver(&mut app, FOLLOWING, &effects, page(0, 1, None));
    assert_eq!(polls(&tick(&mut app, 150)).len(), 1);
}

#[test]
fn tick_does_not_poll_trending() {
    let mut app = loaded(120, 40, 5, None);
    assert_eq!(polls(&tick(&mut app, 30)).len(), 1);
    let effects = type_str(&mut app, "3");
    assert_eq!(log_fetches(&effects, LogTab::Trending), vec![None]);
    app.update(Action::Tick(NOW + Duration::seconds(30)));
    deliver(&mut app, TRENDING, &effects, ranked_page(5));
    assert!(polls(&tick(&mut app, 120)).is_empty());
}

#[test]
fn tick_polls_latest_too() {
    let mut app = loaded(120, 40, 5, None);
    let effects = type_str(&mut app, "2");
    deliver(&mut app, LATEST, &effects, page(0, 5, None));
    assert_eq!(
        polls(&tick(&mut app, 30)),
        vec![(LogTab::Latest, "c0".to_string())]
    );
}

#[test]
fn tick_does_not_poll_while_saved_is_shown() {
    let mut app = loaded(120, 40, 5, None);
    assert_eq!(polls(&tick(&mut app, 30)).len(), 1);
    let effects = type_str(&mut app, "gv");
    deliver(&mut app, FeedKey::Saved, &effects, page(0, 5, None));
    assert!(polls(&tick(&mut app, 120)).is_empty());
}

// --- state restore -----------------------------------------------------------

#[test]
fn selection_survives_a_trip_to_a_thread_and_back() {
    let mut app = loaded(120, 40, 6, None);
    down(&mut app, 2);
    press(&mut app, KeyCode::Enter);
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.feed(FOLLOWING).selected, 2);
}

#[test]
fn selection_survives_switching_tabs_and_returning() {
    let mut app = loaded(120, 40, 6, None);
    let effects = type_str(&mut app, "2");
    deliver(&mut app, LATEST, &effects, page(0, 6, None));
    down(&mut app, 3);
    assert_eq!(app.feed(LATEST).selected, 3);
    type_str(&mut app, "1");
    type_str(&mut app, "2");
    assert_eq!(app.feed(LATEST).selected, 3);
}

// --- ensure_visible -----------------------------------------------------------

#[test]
fn ensure_visible_aligns_a_card_above_the_view_to_its_top() {
    assert_eq!(ensure_visible(2, 3, 5, 10), 2);
}

#[test]
fn ensure_visible_aligns_a_card_below_the_view_to_its_bottom() {
    assert_eq!(ensure_visible(20, 5, 0, 10), 15);
}

#[test]
fn ensure_visible_aligns_a_card_taller_than_the_view_to_its_top() {
    assert_eq!(ensure_visible(30, 15, 0, 10), 30);
}

#[test]
fn ensure_visible_keeps_the_scroll_when_the_card_fits() {
    assert_eq!(ensure_visible(5, 3, 2, 10), 2);
    assert_eq!(ensure_visible(7, 5, 2, 10), 2);
}

// --- scroll reconciliation ------------------------------------------------------

#[test]
fn selected_card_stays_visible_while_moving_down_through_ten_items() {
    let mut app = loaded(120, 40, 10, None);
    assert_eq!(app.feed(FOLLOWING).items.len(), 10);
    assert_selected_visible(&app);
    for step in 1..=9 {
        down(&mut app, 1);
        assert_eq!(app.feed(FOLLOWING).selected, step);
        assert_selected_visible(&app);
    }
}

#[test]
fn selected_card_stays_visible_while_moving_back_up() {
    let mut app = loaded(120, 40, 10, None);
    type_str(&mut app, "G");
    assert_eq!(app.feed(FOLLOWING).selected, 9);
    assert_selected_visible(&app);
    for _ in 0..9 {
        press(&mut app, KeyCode::Char('k'));
        assert_selected_visible(&app);
    }
    assert_eq!(app.feed(FOLLOWING).scroll, 0);
}

#[test]
fn scroll_is_zero_whenever_the_first_item_is_selected() {
    let mut app = loaded(120, 40, 10, None);
    type_str(&mut app, "G");
    assert!(app.feed(FOLLOWING).scroll > 0);
    type_str(&mut app, "gg");
    assert_eq!(app.feed(FOLLOWING).selected, 0);
    assert_eq!(app.feed(FOLLOWING).scroll, 0);
}

#[test]
fn shrinking_the_terminal_keeps_the_selected_card_visible() {
    let mut app = loaded(120, 40, 10, None);
    type_str(&mut app, "G");
    assert_eq!(app.feed(FOLLOWING).selected, 9);
    app.update(Action::Resize(60, 30));
    assert_selected_visible(&app);
}

#[test]
fn returning_from_a_thread_after_a_resize_keeps_the_selected_card_visible() {
    let mut app = loaded(120, 40, 10, None);
    type_str(&mut app, "G");
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.screen(), Screen::Thread("c9".into()));
    app.update(Action::Resize(60, 30));
    press(&mut app, KeyCode::Esc);
    assert_selected_visible(&app);
}

#[test]
fn expanding_the_selected_card_keeps_it_visible() {
    let (mut app, effects) = started(120, 24);
    let mut first = page(0, 8, None);
    first.items[7] = commit_with_replies(7);
    deliver(&mut app, FOLLOWING, &effects, first);
    type_str(&mut app, "G");
    type_str(&mut app, "o");
    assert!(app.feed(FOLLOWING).expanded.contains("c7"));
    assert_selected_visible(&app);
}

#[test]
fn selection_still_moves_when_the_size_is_unknown() {
    let mut app = App::new(Session::SignedIn(fixture_me()), NOW, UtcOffset::UTC);
    let effects = app.update(Action::Init);
    deliver(&mut app, FOLLOWING, &effects, page(0, 10, None));
    down(&mut app, 3);
    type_str(&mut app, "G");
    assert_eq!(app.feed(FOLLOWING).selected, 9);
    assert_eq!(app.feed(FOLLOWING).scroll, 0);
}

#[test]
fn selection_still_moves_when_the_terminal_is_too_small() {
    let mut app = loaded(120, 40, 10, None);
    app.update(Action::Resize(120, 10));
    down(&mut app, 3);
    type_str(&mut app, "G");
    assert_eq!(app.feed(FOLLOWING).selected, 9);
    assert_eq!(app.feed(FOLLOWING).scroll, 0);
}

// --- additional cases from the test review --------------------------------------

#[test]
fn poll_timer_is_kept_per_log_feed() {
    let mut app = loaded(120, 40, 5, None);
    assert!(polls(&tick(&mut app, 20)).is_empty());
    let effects = type_str(&mut app, "gv");
    deliver(&mut app, FeedKey::Saved, &effects, page(50, 52, None));
    type_str(&mut app, "gl");
    assert_eq!(
        polls(&tick(&mut app, 30)),
        vec![(LogTab::Following, "c0".to_string())]
    );
}

#[test]
fn appended_page_does_not_restart_the_poll_timer() {
    let mut app = loaded(120, 40, 20, Some("cur"));
    tick(&mut app, 20);
    let effects = down(&mut app, 14);
    assert_eq!(log_fetches(&effects, LogTab::Following).len(), 1);
    deliver(&mut app, FOLLOWING, &effects, page(20, 25, None));
    assert_eq!(polls(&tick(&mut app, 30)).len(), 1);
}

#[test]
fn entering_a_screen_again_while_its_request_is_pending_does_not_refetch() {
    let (mut app, _) = started(120, 40);
    let first = type_str(&mut app, "gv");
    assert_eq!(saved_fetches(&first), vec![None]);
    type_str(&mut app, "gl");
    assert!(saved_fetches(&type_str(&mut app, "gv")).is_empty());
}

#[test]
fn failure_of_a_superseded_request_is_ignored() {
    let mut app = loaded(120, 40, 20, Some("cur"));
    let old = down(&mut app, 14);
    assert_eq!(log_fetches(&old, LogTab::Following).len(), 1);
    let refresh = type_str(&mut app, "R");
    assert_eq!(log_fetches(&refresh, LogTab::Following), vec![None]);
    app.update(Action::FeedFailed {
        key: FOLLOWING,
        req: first_req(&old),
        code: "http_500".into(),
    });
    assert_eq!(loading(&app, FOLLOWING), Some(false));
}

#[test]
fn nav_enter_on_the_current_screen_shows_the_cache_from_the_top() {
    let mut app = loaded(120, 40, 20, Some("cur"));
    down(&mut app, 12);
    assert!(app.feed(FOLLOWING).scroll > 0);
    press(&mut app, KeyCode::Tab);
    let effects = press(&mut app, KeyCode::Enter);
    assert!(effects.is_empty());
    assert_eq!(app.feed(FOLLOWING).items.len(), 20);
    assert_eq!(app.feed(FOLLOWING).selected, 0);
    assert_eq!(app.feed(FOLLOWING).scroll, 0);
}

#[test]
fn scroll_is_frozen_while_too_small_and_recovers_after_growing_back() {
    let mut app = loaded(120, 40, 10, None);
    type_str(&mut app, "G");
    let before = app.feed(FOLLOWING).scroll;
    assert!(before > 0);
    app.update(Action::Resize(120, 10));
    for _ in 0..3 {
        press(&mut app, KeyCode::Char('k'));
    }
    assert_eq!(app.feed(FOLLOWING).selected, 6);
    assert_eq!(app.feed(FOLLOWING).scroll, before);
    app.update(Action::Resize(120, 40));
    assert_selected_visible(&app);
}

// --- review follow-ups ---------------------------------------------------------

#[test]
fn list_view_height_is_35_rows_for_log_and_37_for_saved_at_120x40() {
    // The last card's bottom aligns with the bottom of the view, so scroll is total - view.
    let mut app = loaded(120, 40, 30, None);
    type_str(&mut app, "G");
    let spans = card_spans(&app, FOLLOWING);
    let last = spans[29];
    let total = total_lines(&app, FOLLOWING);
    assert_eq!(app.feed(FOLLOWING).scroll as usize, total - 35);
    // Without a next cursor the root-commit line follows the last card.
    assert_eq!(last.start + last.height + 1, total);

    let effects = type_str(&mut app, "gv");
    deliver(&mut app, FeedKey::Saved, &effects, page(0, 30, None));
    type_str(&mut app, "G");
    let total = total_lines(&app, FeedKey::Saved);
    assert_eq!(app.feed(FeedKey::Saved).scroll as usize, total - 37);
}

#[test]
fn saved_selection_stays_visible_while_scrolling() {
    let mut app = loaded(120, 40, 3, None);
    let effects = type_str(&mut app, "gv");
    deliver(&mut app, FeedKey::Saved, &effects, page(0, 15, None));
    type_str(&mut app, "G");
    assert_eq!(app.feed(FeedKey::Saved).selected, 14);
    assert!(app.feed(FeedKey::Saved).scroll > 0);
    assert_selected_visible(&app);
    for _ in 0..14 {
        press(&mut app, KeyCode::Char('k'));
        assert_selected_visible(&app);
    }
}

#[test]
fn enlarging_the_terminal_leaves_no_blank_rows_below_the_list() {
    let mut app = loaded(60, 24, 10, None);
    type_str(&mut app, "G");
    assert!(app.feed(FOLLOWING).scroll > 0);
    app.update(Action::Resize(160, 60));
    let total = total_lines(&app, FOLLOWING);
    assert!(app.feed(FOLLOWING).scroll as usize <= total.saturating_sub(55));
    assert_selected_visible(&app);
}

#[test]
fn collapsing_the_last_card_leaves_no_blank_rows_below_the_list() {
    let (mut app, effects) = started(60, 24);
    let mut first = page(0, 10, None);
    first.items[9] = commit_with_replies(9);
    deliver(&mut app, FOLLOWING, &effects, first);
    type_str(&mut app, "G");
    type_str(&mut app, "o");
    let expanded_scroll = app.feed(FOLLOWING).scroll;
    type_str(&mut app, "o");
    let total = total_lines(&app, FOLLOWING);
    assert!(app.feed(FOLLOWING).scroll as usize <= total.saturating_sub(16));
    assert!(app.feed(FOLLOWING).scroll < expanded_scroll);
}

#[test]
fn k_at_the_top_refreshes_from_the_first_page_even_after_a_failed_append() {
    let mut app = loaded(120, 40, 20, Some("cur"));
    let effects = down(&mut app, 14);
    app.update(Action::FeedFailed {
        key: FOLLOWING,
        req: first_req(&effects),
        code: "http_500".into(),
    });
    type_str(&mut app, "gg");
    app.update(Action::NewCommits {
        key: FOLLOWING,
        since: "c0".into(),
        count: 2,
    });
    let effects = press(&mut app, KeyCode::Char('k'));
    assert_eq!(log_fetches(&effects, LogTab::Following), vec![None]);
    assert_eq!(loading(&app, FOLLOWING), Some(false));
}

#[test]
fn refreshed_page_collapses_expanded_cards() {
    let mut app = loaded(120, 40, 5, None);
    type_str(&mut app, "o");
    assert!(app.feed(FOLLOWING).expanded.contains("c0"));
    let effects = type_str(&mut app, "R");
    deliver(&mut app, FOLLOWING, &effects, page(0, 5, None));
    assert!(app.feed(FOLLOWING).expanded.is_empty());
}
