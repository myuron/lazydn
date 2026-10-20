use crossterm::event::KeyCode;
use serde_json::{Value, json};
use time::format_description::well_known::Rfc3339;
use time::macros::datetime;
use time::{Duration, OffsetDateTime};

use crate::action::{Action, Effect, Failure};
use crate::app::{App, Me, Screen, Session};
use crate::feed::{LoadState, ReqId};
use crate::feed_tests::started;
use crate::model::{Lane, MeResponse, Meta, Notification, NotificationsPage, Page, Rgb};
use crate::notifications::Filter;
use crate::test_support::*;
use crate::thread_tests::{commit_json, merge, user_json};

// --- fixtures ---------------------------------------------------------------

/// The `read_at` that the server reports for the response being built.
pub(crate) const READ_AT: &str = "2026-10-20T11:59:50Z";
/// A `read_at` from an earlier visit, so that the notification counts as already seen.
pub(crate) const EARLIER: &str = "2026-10-19T00:00:00Z";

pub(crate) fn rfc(t: OffsetDateTime) -> String {
    t.format(&Rfc3339).unwrap()
}

/// A new notification from bob, `secs_ago` old at the fixed clock, that stands for a single event.
pub(crate) fn notif_json(id: &str, kind: &str, secs_ago: i64) -> Value {
    json!({
        "id": id,
        "group_id": format!("g-{id}"),
        "type": kind,
        "subject_id": null,
        "reason": null,
        "read_at": READ_AT,
        "created_at": rfc(NOW - Duration::seconds(secs_ago)),
        "actor": user_json("bob"),
        "actors": [user_json("bob")],
        "event_count": 1,
        "actors_count": 1,
        "unread_count": 0,
        "commit": null,
    })
}

pub(crate) fn seen(item: Value) -> Value {
    merge(item, json!({ "read_at": EARLIER }))
}

pub(crate) fn with_commit(item: Value, commit_id: &str, author: &str, body: &str) -> Value {
    let commit = merge(
        commit_json(commit_id, author, 600),
        json!({ "body": body, "body_segments": [{"type": "text", "text": body}] }),
    );
    merge(item, json!({ "commit": commit }))
}

pub(crate) fn notif_of(value: Value) -> Notification {
    serde_json::from_value(value).unwrap()
}

pub(crate) fn npage(
    items: Vec<Value>,
    cursor: Option<&str>,
    read_at: Option<&str>,
) -> NotificationsPage {
    serde_json::from_value(json!({
        "data": items,
        "meta": { "next_cursor": cursor },
        "read_at": read_at,
        "filters": { "type": "all" },
    }))
    .unwrap()
}

pub(crate) fn gpage(items: Vec<Value>, cursor: Option<&str>) -> Page<Notification> {
    Page {
        data: items.into_iter().map(notif_of).collect(),
        meta: Meta {
            next_cursor: cursor.map(String::from),
        },
    }
}

/// Matches `fixture_me` except for the unread count.
pub(crate) fn me_json(unread: u32, extra: Value) -> Value {
    let data = merge(
        json!({
            "id": "user-myuron",
            "username": "myuron",
            "name": "myuron",
            "bio": "Rust / TUI enjoyer",
            "created_at": "2026-10-01T00:00:00Z",
            "commits_count": 42,
            "followers_count": 12,
            "following_count": 10,
            "lane": 1,
            "pasteis_mode": true,
            "pasteis_remaining": 7,
            "pasteis_resets_at": "2026-10-21T00:00:00Z",
            "pasteis_received": 3,
            "preferred_lane": null,
            "automatic_lane": 1,
            "sponsors_url": null,
            "github_synced_at": "2026-10-20T10:00:00Z",
        }),
        extra,
    );
    json!({ "data": data, "unread_notifications_count": unread })
}

pub(crate) fn me_resp(unread: u32) -> MeResponse {
    serde_json::from_value(me_json(unread, json!({}))).unwrap()
}

fn me_resp_with(unread: u32, extra: Value) -> MeResponse {
    serde_json::from_value(me_json(unread, extra)).unwrap()
}

// --- effects ----------------------------------------------------------------

pub(crate) fn notif_fetches(effects: &[Effect]) -> Vec<(String, Option<String>)> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::FetchNotifications { types, cursor, .. } => {
                Some((types.clone(), cursor.clone()))
            }
            _ => None,
        })
        .collect()
}

pub(crate) fn group_fetches(effects: &[Effect]) -> Vec<(String, String, Option<String>)> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::FetchNotificationGroup {
                id, types, cursor, ..
            } => Some((id.clone(), types.clone(), cursor.clone())),
            _ => None,
        })
        .collect()
}

fn me_fetches(effects: &[Effect]) -> usize {
    effects
        .iter()
        .filter(|e| matches!(e, Effect::FetchMe { .. }))
        .count()
}

/// Falls back to a bogus id so that a missing effect fails the assertions, not an unwrap.
pub(crate) fn me_req(effects: &[Effect]) -> ReqId {
    effects
        .iter()
        .find_map(|e| match e {
            Effect::FetchMe { req } => Some(*req),
            _ => None,
        })
        .unwrap_or(ReqId(u64::MAX))
}

pub(crate) fn notif_req(effects: &[Effect]) -> ReqId {
    effects
        .iter()
        .find_map(|e| match e {
            Effect::FetchNotifications { req, .. } => Some(*req),
            _ => None,
        })
        .unwrap_or(ReqId(u64::MAX))
}

pub(crate) fn group_req(effects: &[Effect]) -> ReqId {
    effects
        .iter()
        .find_map(|e| match e {
            Effect::FetchNotificationGroup { req, .. } => Some(*req),
            _ => None,
        })
        .unwrap_or(ReqId(u64::MAX))
}

fn thread_fetches(effects: &[Effect]) -> Vec<String> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::FetchThread { id, .. } => Some(id.clone()),
            _ => None,
        })
        .collect()
}

// --- scenario helpers -------------------------------------------------------

pub(crate) fn me_of(app: &App) -> Me {
    match app.session() {
        Session::SignedIn(me) => me.clone(),
        Session::Guest | Session::Restoring => panic!("the app is not signed in"),
    }
}

pub(crate) fn deliver_notifs(app: &mut App, effects: &[Effect], page: NotificationsPage) {
    app.update(Action::NotificationsLoaded {
        req: notif_req(effects),
        page: Box::new(page),
    });
}

/// Starts signed in, answers the startup Me request, and opens Notifications with `g n`.
pub(crate) fn on_notifications(w: u16, h: u16) -> (App, Vec<Effect>) {
    let (mut app, init) = started(w, h);
    app.update(Action::MeLoaded {
        req: me_req(&init),
        response: Box::new(me_resp(12)),
    });
    let effects = type_str(&mut app, "gn");
    (app, effects)
}

pub(crate) fn loaded(w: u16, h: u16, items: Vec<Value>, cursor: Option<&str>) -> App {
    let (mut app, effects) = on_notifications(w, h);
    deliver_notifs(&mut app, &effects, npage(items, cursor, Some(READ_AT)));
    app
}

/// Lets 30 seconds pass and answers the Me request that the poll makes.
pub(crate) fn refresh_me(app: &mut App, unread: u32) {
    let at = app.now() + Duration::seconds(30);
    let effects = app.update(Action::Tick(at));
    app.update(Action::MeLoaded {
        req: me_req(&effects),
        response: Box::new(me_resp(unread)),
    });
}

/// `count` plain replies from bob, newest first.
fn replies(count: usize) -> Vec<Value> {
    (0..count)
        .map(|i| notif_json(&format!("n{i}"), "replied", 60 * (i as i64 + 1)))
        .collect()
}

fn ids(app: &App) -> Vec<String> {
    app.notifications()
        .items
        .iter()
        .map(|n| n.id.clone())
        .collect()
}

fn all_types() -> String {
    "all".to_string()
}

// --- 1. Me: requests --------------------------------------------------------

#[test]
fn init_requests_the_current_user_when_signed_in() {
    let (_, effects) = started(120, 40);
    assert_eq!(me_fetches(&effects), 1);
}

#[test]
fn init_does_not_request_the_current_user_for_a_guest() {
    let mut app = guest(120, 40);
    let effects = app.update(Action::Init);
    assert_eq!(me_fetches(&effects), 0);
}

#[test]
fn tick_requests_the_current_user_once_30_seconds_have_passed_since_the_last_request() {
    let (mut app, init) = started(120, 40);
    app.update(Action::MeLoaded {
        req: me_req(&init),
        response: Box::new(me_resp(12)),
    });

    let early = app.update(Action::Tick(NOW + Duration::seconds(29)));
    assert_eq!(me_fetches(&early), 0);
    let due = app.update(Action::Tick(NOW + Duration::seconds(30)));
    assert_eq!(me_fetches(&due), 1);
}

#[test]
fn the_30_seconds_are_counted_from_the_latest_request() {
    let (mut app, init) = started(120, 40);
    app.update(Action::MeLoaded {
        req: me_req(&init),
        response: Box::new(me_resp(12)),
    });
    let first = app.update(Action::Tick(NOW + Duration::seconds(30)));
    app.update(Action::MeLoaded {
        req: me_req(&first),
        response: Box::new(me_resp(12)),
    });

    let early = app.update(Action::Tick(NOW + Duration::seconds(59)));
    assert_eq!(me_fetches(&early), 0);
    let due = app.update(Action::Tick(NOW + Duration::seconds(60)));
    assert_eq!(me_fetches(&due), 1);
}

#[test]
fn tick_does_not_stack_a_second_request_while_one_is_pending() {
    let (mut app, init) = started(120, 40);
    let effects = app.update(Action::Tick(NOW + Duration::seconds(90)));
    assert_eq!(me_fetches(&effects), 0);

    app.update(Action::MeLoaded {
        req: me_req(&init),
        response: Box::new(me_resp(12)),
    });
    let effects = app.update(Action::Tick(NOW + Duration::seconds(90)));
    assert_eq!(me_fetches(&effects), 1);
}

#[test]
fn tick_requests_the_current_user_again_after_a_failure() {
    let (mut app, init) = started(120, 40);
    app.update(Action::MeFailed {
        req: me_req(&init),
        failure: Failure::new("http_500"),
    });
    let effects = app.update(Action::Tick(NOW + Duration::seconds(30)));
    assert_eq!(me_fetches(&effects), 1);
}

#[test]
fn tick_requests_the_current_user_whichever_screen_is_shown() {
    let (mut app, init) = started(120, 40);
    app.update(Action::MeLoaded {
        req: me_req(&init),
        response: Box::new(me_resp(12)),
    });
    type_str(&mut app, "gs");
    assert_eq!(app.screen(), Screen::Settings);
    let effects = app.update(Action::Tick(NOW + Duration::seconds(30)));
    assert_eq!(me_fetches(&effects), 1);
}

#[test]
fn tick_never_requests_the_current_user_for_a_guest() {
    let mut app = guest(120, 40);
    app.update(Action::Init);
    let effects = app.update(Action::Tick(NOW + Duration::seconds(300)));
    assert_eq!(me_fetches(&effects), 0);
}

// --- 2. Me: responses -------------------------------------------------------

#[test]
fn me_loaded_replaces_the_values_shown_in_the_aside_and_the_nav_badge() {
    let (mut app, init) = started(120, 40);
    let response = me_resp_with(
        5,
        json!({
            "name": "Mira Lee",
            "username": "mira",
            "commits_count": 120,
            "following_count": 3,
            "followers_count": 45,
            "pasteis_remaining": 4,
            "bio": "Terminal tinkerer",
            "pasteis_mode": false,
        }),
    );
    app.update(Action::MeLoaded {
        req: me_req(&init),
        response: Box::new(response),
    });

    let me = me_of(&app);
    assert_eq!(me.name, "Mira Lee");
    assert_eq!(me.handle, "mira");
    assert_eq!(me.commits, 120);
    assert_eq!(me.following, 3);
    assert_eq!(me.followers, 45);
    assert_eq!(me.pasteis_left, 4);
    assert_eq!(me.bio, "Terminal tinkerer");
    assert_eq!(me.unread, 5);
    assert!(!me.pasteis_mode);
}

#[test]
fn me_loaded_for_a_superseded_request_is_dropped() {
    let (mut app, init) = started(120, 40);
    app.update(Action::MeFailed {
        req: me_req(&init),
        failure: Failure::new("http_500"),
    });
    let current = app.update(Action::Tick(NOW + Duration::seconds(30)));

    app.update(Action::MeLoaded {
        req: me_req(&init),
        response: Box::new(me_resp(99)),
    });
    assert_eq!(me_of(&app).unread, 12);
    app.update(Action::MeLoaded {
        req: me_req(&current),
        response: Box::new(me_resp(7)),
    });
    assert_eq!(me_of(&app).unread, 7);
}

#[test]
fn me_failed_keeps_the_current_values() {
    let (mut app, init) = started(120, 40);
    let before = me_of(&app);
    app.update(Action::MeFailed {
        req: me_req(&init),
        failure: Failure::new("http_500"),
    });
    assert_eq!(me_of(&app), before);

    let retry = app.update(Action::Tick(NOW + Duration::seconds(30)));
    app.update(Action::MeLoaded {
        req: me_req(&retry),
        response: Box::new(me_resp(1)),
    });
    assert_eq!(me_of(&app).unread, 1);
}

// --- 2b. Me: conversion -----------------------------------------------------

#[test]
fn me_is_built_from_every_field_of_the_response() {
    let response = me_resp_with(
        4,
        json!({
            "lane": 3,
            "preferred_lane": "#ff8800",
            "automatic_lane": 2,
            "pasteis_received": 9,
            "pasteis_resets_at": "2026-10-21T00:00:00Z",
            "github_synced_at": "2026-10-19T08:00:00Z",
        }),
    );
    let me = Me::from(&response);
    assert_eq!(me.id, "user-myuron");
    assert_eq!(me.name, "myuron");
    assert_eq!(me.handle, "myuron");
    assert_eq!(me.commits, 42);
    assert_eq!(me.following, 10);
    assert_eq!(me.followers, 12);
    assert_eq!(me.pasteis_left, 7);
    assert_eq!(me.pasteis_max, 10);
    assert_eq!(me.bio, "Rust / TUI enjoyer");
    assert_eq!(me.unread, 4);
    assert!(me.pasteis_mode);
    assert_eq!(me.lane, Lane::Index(3));
    assert_eq!(me.preferred_lane, Some(Lane::Custom(Rgb(0xff, 0x88, 0x00))));
    assert_eq!(me.automatic_lane, 2);
    assert_eq!(me.pasteis_received, 9);
    assert_eq!(me.pasteis_resets_at, datetime!(2026-10-21 00:00 UTC));
    assert_eq!(me.github_synced_at, Some(datetime!(2026-10-19 08:00 UTC)));
}

#[test]
fn me_falls_back_when_the_response_leaves_values_out() {
    let response = me_resp_with(
        0,
        json!({
            "name": null,
            "bio": null,
            "commits_count": null,
            "following_count": null,
            "followers_count": null,
            "lane": null,
            "automatic_lane": 4,
            "github_synced_at": null,
        }),
    );
    let me = Me::from(&response);
    assert_eq!(me.name, "myuron");
    assert_eq!(me.bio, "");
    assert_eq!(me.commits, 0);
    assert_eq!(me.following, 0);
    assert_eq!(me.followers, 0);
    assert_eq!(me.github_synced_at, None);
    // An unset lane is the automatic one, which the response names.
    assert_eq!(me.lane, Lane::Index(4));
}

// --- model ------------------------------------------------------------------

#[test]
fn a_notification_is_parsed_with_all_of_its_fields() {
    let n = notif_of(with_commit(
        merge(
            notif_json("n1", "rejected", 60),
            json!({
                "subject_id": "s1",
                "reason": "spam",
                "event_count": 3,
                "actors_count": 2,
                "unread_count": 1,
            }),
        ),
        "c1",
        "myuron",
        "hello",
    ));
    assert_eq!(n.id, "n1");
    assert_eq!(n.group_id, "g-n1");
    assert_eq!(n.kind, crate::model::NotificationType::Rejected);
    assert_eq!(n.subject_id.as_deref(), Some("s1"));
    assert_eq!(n.reason.as_deref(), Some("spam"));
    assert_eq!(n.read_at, Some(datetime!(2026-10-20 11:59:50 UTC)));
    assert_eq!(n.created_at, NOW - Duration::seconds(60));
    assert_eq!(n.actor.map(|u| u.username), Some("bob".into()));
    assert_eq!(n.actors.len(), 1);
    assert_eq!((n.event_count, n.actors_count, n.unread_count), (3, 2, 1));
    assert!(n.commit.is_some());
}

#[test]
fn an_unknown_notification_type_is_unknown() {
    let n = notif_of(notif_json("n1", "poked", 60));
    assert_eq!(n.kind, crate::model::NotificationType::Unknown);
}

#[test]
fn a_group_notification_without_actors_has_an_empty_list() {
    let mut value = notif_json("n1", "starred", 60);
    value.as_object_mut().unwrap().remove("actors");
    assert!(notif_of(value).actors.is_empty());
}

#[test]
fn a_notification_page_carries_the_top_level_read_at_and_the_cursor() {
    let page = npage(replies(2), Some("next"), Some(READ_AT));
    assert_eq!(page.data.len(), 2);
    assert_eq!(page.meta.next_cursor.as_deref(), Some("next"));
    assert_eq!(page.read_at, Some(datetime!(2026-10-20 11:59:50 UTC)));
    assert_eq!(page.filters.map(|f| f.kind), Some("all".into()));
}

// --- 4. entering Notifications ---------------------------------------------

#[test]
fn g_n_requests_every_type_from_the_first_page() {
    let (_, effects) = on_notifications(120, 40);
    assert_eq!(notif_fetches(&effects), vec![(all_types(), None)]);
}

#[test]
fn the_nav_requests_notifications_too() {
    let (mut app, init) = started(120, 40);
    app.update(Action::MeLoaded {
        req: me_req(&init),
        response: Box::new(me_resp(12)),
    });
    press(&mut app, KeyCode::Tab);
    type_str(&mut app, "j");
    let effects = press(&mut app, KeyCode::Enter);
    assert_eq!(app.screen(), Screen::Notifications);
    assert_eq!(notif_fetches(&effects), vec![(all_types(), None)]);
}

#[test]
fn entering_again_requests_again_even_with_cached_items() {
    let mut app = loaded(120, 40, replies(2), None);
    type_str(&mut app, "gl");
    let effects = type_str(&mut app, "gn");
    assert_eq!(notif_fetches(&effects), vec![(all_types(), None)]);
}

#[test]
fn coming_back_with_esc_does_not_request_again() {
    let item = with_commit(notif_json("n0", "replied", 60), "c1", "dave", "hi");
    let mut app = loaded(120, 40, vec![item], None);
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.screen(), Screen::Thread("c1".into()));

    let effects = press(&mut app, KeyCode::Esc);
    assert_eq!(app.screen(), Screen::Notifications);
    assert_eq!(notif_fetches(&effects), vec![]);
    assert_eq!(ids(&app), vec!["n0"]);
}

#[test]
fn a_guest_cannot_open_notifications() {
    let mut app = guest(120, 40);
    press(&mut app, KeyCode::Esc);
    let effects = type_str(&mut app, "gn");
    assert_eq!(notif_fetches(&effects), vec![]);
    assert_ne!(app.screen(), Screen::Notifications);
}

// --- 5. responses -----------------------------------------------------------

#[test]
fn a_response_fills_the_list_and_puts_the_selection_on_the_first_row() {
    let app = loaded(120, 40, replies(3), Some("next"));
    let state = app.notifications();
    assert_eq!(ids(&app), vec!["n0", "n1", "n2"]);
    assert_eq!(state.next_cursor.as_deref(), Some("next"));
    assert_eq!(state.selected, 0);
    assert!(state.pending.is_none());
}

#[test]
fn loading_notifications_zeroes_the_unread_badge() {
    let (mut app, effects) = on_notifications(120, 40);
    assert_eq!(me_of(&app).unread, 12);
    deliver_notifs(&mut app, &effects, npage(replies(1), None, Some(READ_AT)));
    assert_eq!(me_of(&app).unread, 0);
}

#[test]
fn a_response_to_a_superseded_request_is_dropped() {
    let (mut app, first) = on_notifications(120, 40);
    let second = type_str(&mut app, "R");
    deliver_notifs(&mut app, &first, npage(replies(1), None, Some(READ_AT)));
    assert_eq!(ids(&app), Vec::<String>::new());
    assert_eq!(me_of(&app).unread, 12);

    deliver_notifs(&mut app, &second, npage(replies(2), None, Some(READ_AT)));
    assert_eq!(ids(&app), vec!["n0", "n1"]);
}

#[test]
fn a_failure_keeps_the_list_and_records_the_code() {
    let mut app = loaded(120, 40, replies(2), None);
    let effects = type_str(&mut app, "R");
    app.update(Action::NotificationsFailed {
        req: notif_req(&effects),
        failure: Failure::new("http_500"),
    });
    let state = app.notifications();
    assert_eq!(ids(&app), vec!["n0", "n1"]);
    assert!(state.pending.is_none());
    assert!(matches!(&state.load, LoadState::Failed { code, .. } if code == "http_500"));
}

#[test]
fn a_failure_of_a_superseded_request_is_dropped() {
    let (mut app, first) = on_notifications(120, 40);
    type_str(&mut app, "R");
    app.update(Action::NotificationsFailed {
        req: notif_req(&first),
        failure: Failure::new("http_500"),
    });
    assert!(matches!(app.notifications().load, LoadState::Idle));
    assert!(app.notifications().pending.is_some());
}

// --- 6. filters -------------------------------------------------------------

#[test]
fn every_filter_starts_on() {
    let app = loaded(120, 40, vec![], None);
    for filter in Filter::ALL {
        assert!(app.notifications().filters.is_on(filter), "{filter:?}");
    }
}

#[test]
fn each_number_key_toggles_its_filter_and_requests_the_remaining_types() {
    let cases = [
        ("1", "quotes,stars,mentions,follows,moderation"),
        ("2", "replies,stars,mentions,follows,moderation"),
        ("3", "replies,quotes,mentions,follows,moderation"),
        ("4", "replies,quotes,stars,follows,moderation"),
        ("5", "replies,quotes,stars,mentions,moderation"),
        ("6", "replies,quotes,stars,mentions,follows"),
    ];
    for (key, types) in cases {
        let mut app = loaded(120, 40, replies(2), None);
        let effects = type_str(&mut app, key);
        assert_eq!(
            notif_fetches(&effects),
            vec![(types.to_string(), None)],
            "key {key}"
        );
    }
}

#[test]
fn key_4_turns_mentions_off_and_a_second_press_turns_it_on() {
    let mut app = loaded(120, 40, replies(2), None);
    type_str(&mut app, "4");
    assert!(!app.notifications().filters.is_on(Filter::Mentions));
    assert!(app.notifications().filters.is_on(Filter::Replies));

    let effects = type_str(&mut app, "4");
    assert!(app.notifications().filters.is_on(Filter::Mentions));
    assert_eq!(notif_fetches(&effects), vec![(all_types(), None)]);
}

#[test]
fn a_switches_between_all_off_and_all_on() {
    let mut app = loaded(120, 40, replies(2), None);
    let off = type_str(&mut app, "a");
    assert_eq!(notif_fetches(&off), vec![("none".to_string(), None)]);
    for filter in Filter::ALL {
        assert!(!app.notifications().filters.is_on(filter), "{filter:?}");
    }

    let on = type_str(&mut app, "a");
    assert_eq!(notif_fetches(&on), vec![(all_types(), None)]);
    for filter in Filter::ALL {
        assert!(app.notifications().filters.is_on(filter), "{filter:?}");
    }
}

#[test]
fn a_turns_everything_on_when_only_some_filters_are_on() {
    let mut app = loaded(120, 40, replies(2), None);
    type_str(&mut app, "4");
    let effects = type_str(&mut app, "a");
    assert_eq!(notif_fetches(&effects), vec![(all_types(), None)]);
}

#[test]
fn turning_all_six_off_one_by_one_requests_none() {
    let mut app = loaded(120, 40, replies(2), None);
    let effects = type_str(&mut app, "123456");
    assert_eq!(
        notif_fetches(&effects).last(),
        Some(&("none".to_string(), None))
    );
    let again = type_str(&mut app, "2");
    assert_eq!(notif_fetches(&again), vec![("quotes".to_string(), None)]);
}

#[test]
fn number_keys_and_a_do_nothing_while_the_nav_has_the_focus() {
    let mut app = loaded(120, 40, replies(2), None);
    press(&mut app, KeyCode::Tab);
    let effects = type_str(&mut app, "123456a");
    assert_eq!(notif_fetches(&effects), vec![]);
    for filter in Filter::ALL {
        assert!(app.notifications().filters.is_on(filter), "{filter:?}");
    }

    press(&mut app, KeyCode::Tab);
    let effects = type_str(&mut app, "4");
    assert_eq!(notif_fetches(&effects).len(), 1);
}

#[test]
fn changing_a_filter_closes_the_open_groups() {
    let item = merge(notif_json("n0", "starred", 60), json!({"event_count": 3}));
    let mut app = loaded(120, 40, vec![item.clone()], None);
    let open = type_str(&mut app, "o");
    app.update(Action::GroupLoaded {
        id: "n0".into(),
        req: group_req(&open),
        page: gpage(vec![notif_json("k0", "starred", 60)], None),
    });
    assert!(app.notifications().expanded.contains("g-n0"));

    let effects = type_str(&mut app, "4");
    deliver_notifs(&mut app, &effects, npage(vec![item], None, Some(READ_AT)));
    assert!(app.notifications().expanded.is_empty());
    assert!(app.notifications().groups.is_empty());
}

#[test]
fn a_manual_refresh_closes_the_open_groups_too() {
    let item = merge(notif_json("n0", "starred", 60), json!({"event_count": 3}));
    let mut app = loaded(120, 40, vec![item.clone()], None);
    type_str(&mut app, "o");
    assert!(app.notifications().expanded.contains("g-n0"));

    let effects = type_str(&mut app, "R");
    deliver_notifs(&mut app, &effects, npage(vec![item], None, Some(READ_AT)));
    assert!(app.notifications().expanded.is_empty());
    assert!(app.notifications().groups.is_empty());
}

// --- 7. new notifications ---------------------------------------------------

#[test]
fn unread_news_while_viewing_notifications_become_the_new_count() {
    let mut app = loaded(120, 40, replies(2), None);
    refresh_me(&mut app, 2);
    assert_eq!(app.notifications().new_count, 2);
    assert_eq!(me_of(&app).unread, 2);
}

#[test]
fn the_new_count_is_left_alone_on_other_screens() {
    let mut app = loaded(120, 40, replies(2), None);
    type_str(&mut app, "gl");
    refresh_me(&mut app, 2);
    assert_eq!(app.notifications().new_count, 0);
    assert_eq!(me_of(&app).unread, 2);
}

#[test]
fn a_late_me_response_cannot_bring_back_the_unread_count() {
    let (mut app, init) = started(120, 40);
    let effects = type_str(&mut app, "gn");
    deliver_notifs(&mut app, &effects, npage(replies(1), None, Some(READ_AT)));
    assert_eq!(me_of(&app).unread, 0);

    app.update(Action::MeLoaded {
        req: me_req(&init),
        response: Box::new(me_resp(5)),
    });
    assert_eq!(me_of(&app).unread, 0);
    assert_eq!(app.notifications().new_count, 0);
}

#[test]
fn a_late_me_response_still_updates_the_other_values() {
    let (mut app, init) = started(120, 40);
    let effects = type_str(&mut app, "gn");
    deliver_notifs(&mut app, &effects, npage(replies(1), None, Some(READ_AT)));
    app.update(Action::MeLoaded {
        req: me_req(&init),
        response: Box::new(me_resp_with(5, json!({"commits_count": 77}))),
    });
    assert_eq!(me_of(&app).commits, 77);
}

#[test]
fn zero_unread_does_not_clear_the_new_count() {
    let mut app = loaded(120, 40, replies(2), None);
    refresh_me(&mut app, 2);
    refresh_me(&mut app, 0);
    assert_eq!(app.notifications().new_count, 2);
}

// --- 8. top of the list, refresh -------------------------------------------

#[test]
fn k_on_the_first_row_loads_the_news_when_there_are_some() {
    let mut app = loaded(120, 40, replies(3), Some("next"));
    refresh_me(&mut app, 2);
    let effects = type_str(&mut app, "k");
    assert_eq!(notif_fetches(&effects), vec![(all_types(), None)]);

    deliver_notifs(
        &mut app,
        &effects,
        npage(replies(4), None, Some("2026-10-20T12:00:20Z")),
    );
    assert_eq!(app.notifications().new_count, 0);
    assert_eq!(
        app.notifications().read_at,
        Some(datetime!(2026-10-20 12:00:20 UTC))
    );
    assert_eq!(app.notifications().selected, 0);
}

#[test]
fn k_on_the_first_row_does_nothing_without_news() {
    let mut app = loaded(120, 40, replies(3), None);
    let effects = type_str(&mut app, "k");
    assert_eq!(notif_fetches(&effects), vec![]);
}

#[test]
fn k_below_the_first_row_only_moves_the_selection() {
    let mut app = loaded(120, 40, replies(3), None);
    refresh_me(&mut app, 2);
    type_str(&mut app, "j");
    assert_eq!(app.notifications().selected, 1);
    let effects = type_str(&mut app, "k");
    assert_eq!(notif_fetches(&effects), vec![]);
    assert_eq!(app.notifications().selected, 0);
}

#[test]
fn shift_r_requests_the_first_page_again_and_clears_the_new_count_once_loaded() {
    let mut app = loaded(120, 40, replies(3), Some("next"));
    refresh_me(&mut app, 2);
    let effects = type_str(&mut app, "R");
    assert_eq!(notif_fetches(&effects), vec![(all_types(), None)]);

    deliver_notifs(&mut app, &effects, npage(replies(3), None, Some(READ_AT)));
    assert_eq!(app.notifications().new_count, 0);
}

// --- selection --------------------------------------------------------------

#[test]
fn j_k_g_g_and_shift_g_move_the_selection_over_the_notifications() {
    let mut app = loaded(120, 40, replies(3), None);
    type_str(&mut app, "j");
    assert_eq!(app.notifications().selected, 1);
    type_str(&mut app, "G");
    assert_eq!(app.notifications().selected, 2);
    type_str(&mut app, "gg");
    assert_eq!(app.notifications().selected, 0);
}

#[test]
fn the_selection_stops_at_the_last_notification() {
    let mut app = loaded(120, 40, replies(2), None);
    type_str(&mut app, "jjj");
    assert_eq!(app.notifications().selected, 1);
}

// --- 9. prefetch ------------------------------------------------------------

#[test]
fn moving_within_five_of_the_end_requests_the_next_page_with_the_cursor() {
    let mut app = loaded(120, 40, replies(10), Some("next"));
    for _ in 0..3 {
        assert_eq!(notif_fetches(&type_str(&mut app, "j")), vec![]);
    }
    let effects = type_str(&mut app, "j");
    assert_eq!(
        notif_fetches(&effects),
        vec![(all_types(), Some("next".to_string()))]
    );
}

#[test]
fn the_prefetch_is_not_repeated_while_it_is_pending() {
    let mut app = loaded(120, 40, replies(10), Some("next"));
    let effects = type_str(&mut app, "jjjj");
    assert_eq!(notif_fetches(&effects).len(), 1);
    let effects = type_str(&mut app, "j");
    assert_eq!(notif_fetches(&effects), vec![]);
}

#[test]
fn shift_g_requests_the_next_page_too() {
    let mut app = loaded(120, 40, replies(10), Some("next"));
    let effects = type_str(&mut app, "G");
    assert_eq!(
        notif_fetches(&effects),
        vec![(all_types(), Some("next".to_string()))]
    );
}

#[test]
fn nothing_is_prefetched_without_a_cursor() {
    let mut app = loaded(120, 40, replies(10), None);
    let effects = type_str(&mut app, "G");
    assert_eq!(app.notifications().selected, 9);
    assert_eq!(notif_fetches(&effects), vec![]);
}

#[test]
fn the_prefetch_uses_the_current_filters() {
    let mut app = loaded(120, 40, replies(10), Some("next"));
    let refetch = type_str(&mut app, "4");
    deliver_notifs(
        &mut app,
        &refetch,
        npage(replies(10), Some("next"), Some(READ_AT)),
    );
    let effects = type_str(&mut app, "G");
    assert_eq!(
        notif_fetches(&effects),
        vec![(
            "replies,quotes,stars,follows,moderation".to_string(),
            Some("next".to_string())
        )]
    );
}

#[test]
fn a_prefetched_page_is_appended_after_the_list() {
    let mut app = loaded(120, 40, replies(6), Some("next"));
    let effects = type_str(&mut app, "G");
    let more: Vec<Value> = (6..9)
        .map(|i| notif_json(&format!("n{i}"), "replied", 60 * (i + 1)))
        .collect();
    deliver_notifs(&mut app, &effects, npage(more, None, Some(READ_AT)));
    assert_eq!(ids(&app).len(), 9);
    assert_eq!(app.notifications().next_cursor, None);
    assert_eq!(app.notifications().selected, 5);
}

#[test]
fn a_failed_prefetch_keeps_the_list_and_marks_the_append() {
    let mut app = loaded(120, 40, replies(6), Some("next"));
    let effects = type_str(&mut app, "G");
    app.update(Action::NotificationsFailed {
        req: notif_req(&effects),
        failure: Failure::new("http_500"),
    });
    assert_eq!(ids(&app).len(), 6);
    assert!(matches!(
        &app.notifications().load,
        LoadState::Failed { append: true, .. }
    ));
}

// --- read_at and duplicates -------------------------------------------------

#[test]
fn a_first_page_stores_the_top_level_read_at() {
    let app = loaded(120, 40, replies(2), None);
    assert_eq!(
        app.notifications().read_at,
        Some(datetime!(2026-10-20 11:59:50 UTC))
    );
}

#[test]
fn the_read_at_of_an_appended_page_is_ignored() {
    let mut app = loaded(120, 40, replies(6), Some("next"));
    let effects = type_str(&mut app, "G");
    deliver_notifs(
        &mut app,
        &effects,
        npage(
            vec![notif_json("x", "replied", 900)],
            None,
            Some("2026-10-20T12:00:09Z"),
        ),
    );
    assert_eq!(
        app.notifications().read_at,
        Some(datetime!(2026-10-20 11:59:50 UTC))
    );
}

#[test]
fn a_refresh_replaces_the_stored_read_at() {
    let mut app = loaded(120, 40, replies(2), None);
    let effects = type_str(&mut app, "R");
    deliver_notifs(
        &mut app,
        &effects,
        npage(replies(2), None, Some("2026-10-20T12:00:20Z")),
    );
    assert_eq!(
        app.notifications().read_at,
        Some(datetime!(2026-10-20 12:00:20 UTC))
    );
}

#[test]
fn an_appended_notification_replaces_the_one_of_the_same_group() {
    let mut first = replies(2);
    first[0] = merge(first[0].clone(), json!({"group_id": "shared"}));
    let mut app = loaded(120, 40, first, Some("next"));
    let effects = type_str(&mut app, "G");

    let newer = merge(
        notif_json("fresh", "replied", 30),
        json!({"group_id": "shared"}),
    );
    deliver_notifs(&mut app, &effects, npage(vec![newer], None, Some(READ_AT)));
    assert_eq!(ids(&app), vec!["fresh", "n1"]);
}

// --- 10. groups -------------------------------------------------------------

pub(crate) fn grouped(kind: &str, event_count: u32) -> Value {
    merge(
        notif_json("n0", kind, 60),
        json!({ "event_count": event_count, "actors_count": event_count }),
    )
}

fn child(id: &str, actor: &str, secs_ago: i64) -> Value {
    let mut value = merge(
        notif_json(id, "starred", secs_ago),
        json!({ "group_id": "g-n0", "actor": user_json(actor) }),
    );
    value.as_object_mut().unwrap().remove("actors");
    value
}

#[test]
fn o_on_a_group_requests_its_notifications_and_opens_it() {
    let mut app = loaded(120, 40, vec![grouped("starred", 3)], None);
    let effects = type_str(&mut app, "o");
    assert_eq!(
        group_fetches(&effects),
        vec![("n0".to_string(), all_types(), None)]
    );
    assert!(app.notifications().expanded.contains("g-n0"));
    assert!(group_of(&app, "g-n0").pending.is_some());
}

#[test]
fn the_group_response_fills_the_children() {
    let mut app = loaded(120, 40, vec![grouped("starred", 3)], None);
    let effects = type_str(&mut app, "o");
    app.update(Action::GroupLoaded {
        id: "n0".into(),
        req: group_req(&effects),
        page: gpage(
            vec![child("k0", "bob", 60), child("k1", "carol", 120)],
            Some("more"),
        ),
    });
    let group = group_of(&app, "g-n0");
    assert_eq!(group.items.len(), 2);
    assert_eq!(group.next_cursor.as_deref(), Some("more"));
    assert!(group.pending.is_none());
}

#[test]
fn o_again_closes_the_group_and_a_third_press_reopens_it_from_the_cache() {
    let mut app = loaded(120, 40, vec![grouped("starred", 3)], None);
    let effects = type_str(&mut app, "o");
    app.update(Action::GroupLoaded {
        id: "n0".into(),
        req: group_req(&effects),
        page: gpage(vec![child("k0", "bob", 60)], None),
    });

    let close = type_str(&mut app, "o");
    assert_eq!(group_fetches(&close), vec![]);
    assert!(!app.notifications().expanded.contains("g-n0"));

    let reopen = type_str(&mut app, "o");
    assert_eq!(group_fetches(&reopen), vec![]);
    assert!(app.notifications().expanded.contains("g-n0"));
    assert_eq!(group_of(&app, "g-n0").items.len(), 1);
}

#[test]
fn o_on_a_single_event_does_nothing() {
    let mut app = loaded(120, 40, vec![grouped("starred", 1)], None);
    let effects = type_str(&mut app, "o");
    assert_eq!(group_fetches(&effects), vec![]);
    assert!(app.notifications().expanded.is_empty());

    let mut app = loaded(120, 40, vec![grouped("starred", 2)], None);
    let effects = type_str(&mut app, "o");
    assert_eq!(group_fetches(&effects).len(), 1);
}

#[test]
fn replies_and_follows_open_as_groups_too() {
    for kind in ["replied", "mentioned", "followed", "quoted"] {
        let mut app = loaded(120, 40, vec![grouped(kind, 2)], None);
        let effects = type_str(&mut app, "o");
        assert_eq!(group_fetches(&effects).len(), 1, "{kind}");
    }
}

#[test]
fn the_group_request_carries_the_current_filter_types() {
    let mut app = loaded(120, 40, vec![grouped("starred", 3)], None);
    let refetch = type_str(&mut app, "4");
    deliver_notifs(
        &mut app,
        &refetch,
        npage(vec![grouped("starred", 3)], None, Some(READ_AT)),
    );
    let effects = type_str(&mut app, "o");
    assert_eq!(
        group_fetches(&effects),
        vec![(
            "n0".to_string(),
            "replies,quotes,stars,follows,moderation".to_string(),
            None
        )]
    );
}

#[test]
fn a_group_response_to_a_superseded_request_is_dropped() {
    let mut app = loaded(120, 40, vec![grouped("starred", 3)], None);
    let effects = type_str(&mut app, "o");
    let ReqId(n) = group_req(&effects);
    app.update(Action::GroupLoaded {
        id: "n0".into(),
        req: ReqId(n.wrapping_add(100)),
        page: gpage(vec![child("k0", "bob", 60)], None),
    });
    let group = group_of(&app, "g-n0");
    assert!(group.items.is_empty());
    assert!(group.pending.is_some());
}

#[test]
fn a_group_response_after_a_refresh_is_dropped() {
    let mut app = loaded(120, 40, vec![grouped("starred", 3)], None);
    let effects = type_str(&mut app, "o");
    assert!(group_of(&app, "g-n0").pending.is_some());
    let refetch = type_str(&mut app, "R");
    deliver_notifs(
        &mut app,
        &refetch,
        npage(vec![grouped("starred", 3)], None, Some(READ_AT)),
    );
    app.update(Action::GroupLoaded {
        id: "n0".into(),
        req: group_req(&effects),
        page: gpage(vec![child("k0", "bob", 60)], None),
    });
    assert!(app.notifications().groups.is_empty());
}

#[test]
fn a_failed_group_request_records_the_code() {
    let mut app = loaded(120, 40, vec![grouped("starred", 3)], None);
    let effects = type_str(&mut app, "o");
    app.update(Action::GroupFailed {
        id: "n0".into(),
        req: group_req(&effects),
        failure: Failure::new("http_500"),
    });
    let group = group_of(&app, "g-n0");
    assert!(group.pending.is_none());
    assert!(matches!(&group.load, LoadState::Failed { code, .. } if code == "http_500"));
}

// --- 11. Enter and @ --------------------------------------------------------

#[test]
fn enter_on_a_reply_opens_the_thread_of_its_commit() {
    let item = with_commit(notif_json("n0", "replied", 60), "c1", "bob", "nice");
    let mut app = loaded(120, 40, vec![item], None);
    let effects = press(&mut app, KeyCode::Enter);
    assert_eq!(app.screen(), Screen::Thread("c1".into()));
    assert_eq!(thread_fetches(&effects), vec!["c1"]);
}

#[test]
fn enter_opens_the_thread_for_every_kind_that_has_a_commit() {
    for kind in ["mentioned", "quoted", "starred", "approved", "rejected"] {
        let item = with_commit(notif_json("n0", kind, 60), "c1", "bob", "text");
        let mut app = loaded(120, 40, vec![item], None);
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.screen(), Screen::Thread("c1".into()), "{kind}");
    }
}

#[test]
fn enter_acts_on_the_selected_notification() {
    let items = vec![
        with_commit(notif_json("n0", "replied", 60), "c1", "bob", "one"),
        with_commit(notif_json("n1", "replied", 120), "c2", "bob", "two"),
    ];
    let mut app = loaded(120, 40, items, None);
    type_str(&mut app, "j");
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.screen(), Screen::Thread("c2".into()));
}

#[test]
fn enter_on_a_follow_opens_the_profile_of_the_actor() {
    let mut app = loaded(120, 40, vec![notif_json("n0", "followed", 60)], None);
    let effects = press(&mut app, KeyCode::Enter);
    assert_eq!(app.screen(), Screen::Profile("bob".into()));
    assert!(!effects.is_empty());
}

#[test]
fn enter_on_a_deleted_commit_does_nothing() {
    let deleted = merge(
        notif_json("n0", "replied", 60),
        json!({"commit": {
            "id": "t1",
            "parent_id": null,
            "deleted_at": "2026-10-20T11:00:00Z",
            "replies_count": 0,
        }}),
    );
    let live = with_commit(notif_json("n1", "replied", 120), "c2", "bob", "two");
    let mut app = loaded(120, 40, vec![deleted, live], None);
    let effects = press(&mut app, KeyCode::Enter);
    assert_eq!(app.screen(), Screen::Notifications);
    assert_eq!(effects, vec![]);

    type_str(&mut app, "j");
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.screen(), Screen::Thread("c2".into()));
}

#[test]
fn enter_without_a_commit_does_nothing() {
    let live = with_commit(notif_json("n1", "replied", 120), "c2", "bob", "two");
    let mut app = loaded(120, 40, vec![notif_json("n0", "replied", 60), live], None);
    let effects = press(&mut app, KeyCode::Enter);
    assert_eq!(app.screen(), Screen::Notifications);
    assert_eq!(effects, vec![]);

    type_str(&mut app, "j");
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.screen(), Screen::Thread("c2".into()));
}

#[test]
fn at_opens_the_profile_of_the_actor() {
    let item = merge(
        notif_json("n0", "replied", 60),
        json!({"actor": user_json("dave")}),
    );
    let mut app = loaded(120, 40, vec![item], None);
    type_str(&mut app, "@");
    assert_eq!(app.screen(), Screen::Profile("dave".into()));
}

#[test]
fn at_does_nothing_on_moderation_results() {
    for kind in ["approved", "rejected"] {
        let own = merge(
            notif_json("n0", kind, 60),
            json!({"actor": user_json("myuron")}),
        );
        let other = merge(
            notif_json("n1", "replied", 120),
            json!({"actor": user_json("dave")}),
        );
        let mut app = loaded(120, 40, vec![own, other], None);
        let effects = type_str(&mut app, "@");
        assert_eq!(app.screen(), Screen::Notifications, "{kind}");
        assert_eq!(effects, vec![], "{kind}");

        type_str(&mut app, "j@");
        assert_eq!(app.screen(), Screen::Profile("dave".into()), "{kind}");
    }
}

#[test]
fn at_does_nothing_without_an_actor() {
    let none = merge(notif_json("n0", "starred", 60), json!({"actor": null}));
    let other = merge(
        notif_json("n1", "replied", 120),
        json!({"actor": user_json("dave")}),
    );
    let mut app = loaded(120, 40, vec![none, other], None);
    let effects = type_str(&mut app, "@");
    assert_eq!(app.screen(), Screen::Notifications);
    assert_eq!(effects, vec![]);

    type_str(&mut app, "j@");
    assert_eq!(app.screen(), Screen::Profile("dave".into()));
}

/// An absent group reads as an empty one, so that a missing entry fails the assertions, not an unwrap.
fn group_of(app: &App, group_id: &str) -> crate::notifications::GroupList {
    app.notifications()
        .groups
        .get(group_id)
        .cloned()
        .unwrap_or_default()
}

// --- rev.4 -------------------------------------------------------------------

fn unknown_with_commit() -> Value {
    with_commit(
        merge(
            notif_json("nu", "poked", 30),
            json!({ "actor": user_json("zed"), "actors": [user_json("zed")] }),
        ),
        "cu",
        "zed",
        "from an unknown kind",
    )
}

#[test]
fn enter_and_at_skip_a_notification_of_an_unknown_kind_at_the_top() {
    let known = with_commit(notif_json("n0", "replied", 120), "c1", "bob", "hello");
    let mut app = loaded(120, 40, vec![unknown_with_commit(), known], None);

    type_str(&mut app, "@");
    assert_eq!(app.screen(), Screen::Profile("bob".into()));
    press(&mut app, KeyCode::Esc);
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.screen(), Screen::Thread("c1".into()));
}

#[test]
fn the_selection_never_lands_on_a_notification_of_an_unknown_kind() {
    let first = with_commit(notif_json("n0", "replied", 60), "c1", "bob", "one");
    let last = with_commit(notif_json("n2", "replied", 120), "c3", "carol", "three");
    let mut app = loaded(120, 40, vec![first, unknown_with_commit(), last], None);

    type_str(&mut app, "j");
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.screen(), Screen::Thread("c3".into()));
}

#[test]
fn k_with_news_drops_the_open_groups_when_the_list_is_replaced() {
    let item = merge(notif_json("n0", "starred", 60), json!({"event_count": 3}));
    let mut app = loaded(120, 40, vec![item.clone()], None);
    let open = type_str(&mut app, "o");
    app.update(Action::GroupLoaded {
        id: "n0".into(),
        req: group_req(&open),
        page: gpage(vec![notif_json("k0", "starred", 60)], None),
    });
    assert!(app.notifications().expanded.contains("g-n0"));
    refresh_me(&mut app, 2);

    let effects = type_str(&mut app, "k");
    assert_eq!(notif_fetches(&effects).len(), 1);
    deliver_notifs(&mut app, &effects, npage(vec![item], None, Some(READ_AT)));
    assert!(app.notifications().expanded.is_empty());
    assert!(app.notifications().groups.is_empty());
}

#[test]
fn a_me_response_older_than_a_prefetch_cannot_bring_back_the_unread_count() {
    let mut app = loaded(120, 40, replies(10), Some("next"));
    let tick = app.update(Action::Tick(app.now() + Duration::seconds(30)));
    let late = me_req(&tick);

    let prefetch = type_str(&mut app, "G");
    // Distinct ids so the appended groups are not deduplicated away.
    let next: Vec<Value> = (10..12)
        .map(|i| notif_json(&format!("n{i}"), "replied", 60 * (i as i64 + 1)))
        .collect();
    deliver_notifs(&mut app, &prefetch, npage(next, None, Some(READ_AT)));
    assert_eq!(ids(&app).len(), 12);

    app.update(Action::MeLoaded {
        req: late,
        response: Box::new(me_resp(5)),
    });
    assert_eq!(me_of(&app).unread, 0);
    assert_eq!(app.notifications().new_count, 0);
}

// --- review fixes ------------------------------------------------------------

#[test]
fn an_appended_notification_of_an_open_group_under_a_new_id_closes_the_group() {
    let first = merge(notif_json("n0", "starred", 60), json!({"event_count": 3}));
    let mut app = loaded(
        120,
        40,
        vec![first, notif_json("n1", "replied", 120)],
        Some("next"),
    );
    let asked = type_str(&mut app, "o");
    assert_eq!(group_fetches(&asked).len(), 1);
    // The prefetch is triggered by moving, then the selection returns to the group.
    let prefetch = type_str(&mut app, "j");
    type_str(&mut app, "k");

    let newer = merge(
        notif_json("n0b", "starred", 30),
        json!({"group_id": "g-n0", "event_count": 4}),
    );
    deliver_notifs(&mut app, &prefetch, npage(vec![newer], None, Some(READ_AT)));
    assert!(app.notifications().expanded.is_empty());
    assert!(app.notifications().groups.is_empty());

    let reopened = type_str(&mut app, "o");
    assert_eq!(
        group_fetches(&reopened),
        vec![("n0b".to_string(), all_types(), None)]
    );
}

#[test]
fn a_page_of_hidden_kinds_asks_for_the_next_page() {
    let (mut app, effects) = on_notifications(120, 40);
    let hidden = npage(
        vec![
            notif_json("u0", "poked", 60),
            notif_json("u1", "poked", 120),
        ],
        Some("c2"),
        Some(READ_AT),
    );
    let next = app.update(Action::NotificationsLoaded {
        req: notif_req(&effects),
        page: Box::new(hidden),
    });
    assert_eq!(
        notif_fetches(&next),
        vec![(all_types(), Some("c2".to_string()))]
    );
    assert!(app.notifications().items.is_empty());

    app.update(Action::NotificationsLoaded {
        req: notif_req(&next),
        page: Box::new(npage(replies(2), None, Some(READ_AT))),
    });
    assert_eq!(ids(&app), vec!["n0", "n1"]);
}

#[test]
fn a_page_with_enough_shown_notifications_is_not_followed_up_by_itself() {
    let (mut app, effects) = on_notifications(120, 40);
    let mut items = replies(6);
    items.push(notif_json("u0", "poked", 600));
    let next = app.update(Action::NotificationsLoaded {
        req: notif_req(&effects),
        page: Box::new(npage(items, Some("c2"), Some(READ_AT))),
    });
    assert_eq!(notif_fetches(&next), vec![]);
}

#[test]
fn k_while_the_news_are_loading_does_not_ask_again() {
    let mut app = loaded(120, 40, replies(3), None);
    refresh_me(&mut app, 2);
    let first = type_str(&mut app, "k");
    assert_eq!(notif_fetches(&first).len(), 1);
    let second = type_str(&mut app, "k");
    assert_eq!(notif_fetches(&second), vec![]);

    deliver_notifs(&mut app, &first, npage(replies(3), None, Some(READ_AT)));
    assert_eq!(app.notifications().new_count, 0);
}
