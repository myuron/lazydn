use crossterm::event::KeyCode;
use time::{Duration, UtcOffset};

use crate::action::{Action, Effect, Failure};
use crate::api::keys::KeyError;
use crate::app::{
    App, ConnState, ConnTab, KeyStatus, LogTab, Me, Modal, Screen, Session, ToastKind,
};
use crate::feed_tests::{FOLLOWING, deliver, first_req, started};
use crate::notifications_tests::{deliver_notifs, me_req, me_resp};
use crate::settings_tests::{load_blocked, load_muted, open_safety, settings_app};
use crate::test_support::*;
use crate::toast_tests::{loaded, log_page};

use crate::feed::FeedKey;

const LATEST: FeedKey = FeedKey::Log(LogTab::Latest);

// --- helpers ----------------------------------------------------------------

fn failure(code: &str) -> Failure {
    Failure::new(code)
}

fn secs(s: i64) -> time::OffsetDateTime {
    NOW + Duration::seconds(s)
}

/// Starts with a device key present, as main does, and runs Init.
fn restoring(w: u16, h: u16) -> (App, Vec<Effect>) {
    let mut app = App::new(Session::Restoring, NOW, UtcOffset::UTC);
    app.update(Action::Resize(w, h));
    let effects = app.update(Action::Init);
    (app, effects)
}

fn fetch_me_count(effects: &[Effect]) -> usize {
    effects
        .iter()
        .filter(|e| matches!(e, Effect::FetchMe { .. }))
        .count()
}

fn fetch_logs(effects: &[Effect], tab: LogTab) -> Vec<Option<String>> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::FetchLog { tab: t, cursor, .. } if *t == tab => Some(cursor.clone()),
            _ => None,
        })
        .collect()
}

fn any_fetch(effects: &[Effect]) -> bool {
    effects.iter().any(|e| !matches!(e, Effect::UseGuestClient))
}

fn toast_text(app: &App) -> Option<String> {
    app.toast().map(|t| t.text.clone())
}

fn me_loaded(app: &mut App, effects: &[Effect]) -> Vec<Effect> {
    app.update(Action::MeLoaded {
        req: me_req(effects),
        response: Box::new(me_resp(12)),
    })
}

fn me_failed(app: &mut App, effects: &[Effect], code: &str) -> Vec<Effect> {
    app.update(Action::MeFailed {
        req: me_req(effects),
        failure: failure(code),
    })
}

fn main_text(app: &App) -> String {
    let terminal = render(app);
    let buffer = terminal.backend().buffer().clone();
    (0..buffer.area.height)
        .map(|y| row_text(&buffer, y))
        .collect::<Vec<_>>()
        .join("\n")
}

// --- Restoring: start ---------------------------------------------------------

#[test]
fn restoring_starts_on_following_without_a_dialog_and_asks_only_for_me() {
    let (app, effects) = restoring(120, 40);
    assert_eq!(app.session(), &Session::Restoring);
    assert_eq!(app.screen(), Screen::Log(LogTab::Following));
    assert_eq!(app.modal(), None);
    assert_eq!(effects.len(), 1, "{effects:?}");
    assert_eq!(fetch_me_count(&effects), 1);
}

// --- Restoring: MeLoaded ---------------------------------------------------------

#[test]
fn me_loaded_signs_in_and_starts_the_following_fetch_from_the_first_screen() {
    let (mut app, init) = restoring(120, 40);
    let effects = me_loaded(&mut app, &init);
    assert_eq!(app.session(), &Session::SignedIn(Me::from(&me_resp(12))));
    assert_eq!(fetch_logs(&effects, LogTab::Following), vec![None]);
    assert_eq!(app.screen(), Screen::Log(LogTab::Following));
}

#[test]
fn me_loaded_does_not_pull_the_user_back_from_another_tab() {
    let (mut app, init) = restoring(120, 40);
    type_str(&mut app, "2");
    assert_eq!(app.screen(), Screen::Log(LogTab::Latest));

    let effects = me_loaded(&mut app, &init);
    assert!(matches!(app.session(), Session::SignedIn(_)));

    assert_eq!(app.screen(), Screen::Log(LogTab::Latest));
    assert!(fetch_logs(&effects, LogTab::Following).is_empty());
    let back = type_str(&mut app, "1");
    assert_eq!(fetch_logs(&back, LogTab::Following), vec![None]);
}

#[test]
fn me_loaded_does_not_pull_the_user_back_from_a_screen_opened_over_the_first_one() {
    let (mut app, init) = restoring(120, 40);
    app.update(Action::Open(Screen::Thread("c1".into())));

    let effects = me_loaded(&mut app, &init);
    assert!(matches!(app.session(), Session::SignedIn(_)));

    assert_eq!(app.screen(), Screen::Thread("c1".into()));
    assert!(fetch_logs(&effects, LogTab::Following).is_empty());
}

#[test]
fn a_restored_session_polls_the_current_user_every_30_seconds() {
    let (mut app, init) = restoring(120, 40);
    me_loaded(&mut app, &init);
    assert_eq!(fetch_me_count(&app.update(Action::Tick(secs(29)))), 0);
    assert_eq!(fetch_me_count(&app.update(Action::Tick(secs(30)))), 1);
}

// --- Restoring: network failure and backoff ----------------------------------------------

#[test]
fn a_network_failure_keeps_restoring_without_a_toast() {
    let (mut app, init) = restoring(120, 40);
    let effects = me_failed(&mut app, &init, "network");
    app.update(Action::ApiStatus {
        rate: None,
        online: false,
    });
    assert_eq!(app.session(), &Session::Restoring);
    assert_eq!(app.toast(), None);
    assert_eq!(app.conn_state(), ConnState::Offline);
    assert!(effects.is_empty(), "{effects:?}");
}

#[test]
fn restoring_retries_me_after_5_10_30_and_then_every_60_seconds() {
    let (mut app, init) = restoring(120, 40);
    let mut last = me_failed(&mut app, &init, "network");
    assert!(last.is_empty());
    let mut sent_at = 0;
    for wait in [5, 10, 30, 60, 60] {
        let early = app.update(Action::Tick(secs(sent_at + wait - 1)));
        assert_eq!(fetch_me_count(&early), 0, "before {wait}s: {early:?}");
        let due = app.update(Action::Tick(secs(sent_at + wait)));
        assert_eq!(fetch_me_count(&due), 1, "at {wait}s: {due:?}");
        sent_at += wait;
        last = me_failed(&mut app, &due, "network");
        assert!(last.is_empty());
    }
    assert_eq!(app.session(), &Session::Restoring);
}

#[test]
fn a_retry_in_flight_is_not_stacked_by_further_ticks() {
    let (mut app, init) = restoring(120, 40);
    me_failed(&mut app, &init, "network");
    let due = app.update(Action::Tick(secs(5)));
    assert_eq!(fetch_me_count(&due), 1);
    assert_eq!(fetch_me_count(&app.update(Action::Tick(secs(6)))), 0);
    assert_eq!(fetch_me_count(&app.update(Action::Tick(secs(100)))), 0);
}

#[test]
fn a_retry_that_succeeds_signs_in_and_fetches_following() {
    let (mut app, init) = restoring(120, 40);
    me_failed(&mut app, &init, "network");
    let due = app.update(Action::Tick(secs(5)));
    let effects = me_loaded(&mut app, &due);
    assert!(matches!(app.session(), Session::SignedIn(_)));
    assert_eq!(fetch_logs(&effects, LogTab::Following), vec![None]);
}

#[test]
fn restoring_makes_no_log_requests_on_tick() {
    let (mut app, _init) = restoring(120, 40);
    let effects = app.update(Action::Tick(secs(1)));
    assert!(effects.is_empty(), "{effects:?}");
}

// --- Restoring: rejected ---------------------------------------------------------------

#[test]
fn device_not_approved_while_restoring_returns_to_a_guest_on_latest_with_the_dialog() {
    let (mut app, init) = restoring(120, 40);

    let effects = me_failed(&mut app, &init, "device_not_approved");

    assert_eq!(app.session(), &Session::Guest);
    assert_eq!(app.screen(), Screen::Log(LogTab::Latest));
    assert_eq!(app.modal(), Some(Modal::SignIn));
    assert_eq!(effects.first(), Some(&Effect::UseGuestClient));
    assert_eq!(
        toast_text(&app).as_deref(),
        Some("this device is no longer approved — sign in again")
    );
}

#[test]
fn invalid_signature_while_restoring_returns_to_a_guest_and_asks_to_check_the_clock() {
    let (mut app, init) = restoring(120, 40);

    let effects = me_failed(&mut app, &init, "invalid_signature");

    assert_eq!(app.session(), &Session::Guest);
    assert_eq!(app.screen(), Screen::Log(LogTab::Latest));
    assert_eq!(app.modal(), Some(Modal::SignIn));
    assert_eq!(effects.first(), Some(&Effect::UseGuestClient));
    assert_eq!(
        toast_text(&app).as_deref(),
        Some("invalid_signature: check your system clock")
    );
}

// --- Restoring: guards -----------------------------------------------------------------

#[test]
fn protected_screens_do_nothing_while_restoring() {
    for keys in ["gn", "gs", "gv", "gp"] {
        let (mut app, _) = restoring(120, 40);
        let effects = type_str(&mut app, keys);
        assert_eq!(app.screen(), Screen::Log(LogTab::Following), "{keys}");
        assert_eq!(app.modal(), None, "{keys}");
        assert!(effects.is_empty(), "{keys}: {effects:?}");
    }
}

#[test]
fn the_nav_does_not_open_a_protected_screen_while_restoring() {
    let (mut app, _) = restoring(120, 40);
    press(&mut app, KeyCode::Tab);
    type_str(&mut app, "j");
    let effects = press(&mut app, KeyCode::Enter);
    assert_eq!(app.screen(), Screen::Log(LogTab::Following));
    assert_eq!(app.modal(), None);
    assert!(effects.is_empty(), "{effects:?}");
}

#[test]
fn the_sign_in_dialog_does_not_open_while_restoring() {
    let (mut app, _) = restoring(120, 40);
    app.update(Action::OpenSignIn);
    assert_eq!(app.modal(), None);
    type_str(&mut app, "c");
    app.update(Action::Commit);
    assert_eq!(app.modal(), None);
}

#[test]
fn a_public_tab_can_be_viewed_while_restoring() {
    let (mut app, _) = restoring(120, 40);
    type_str(&mut app, "2");
    assert_eq!(app.screen(), Screen::Log(LogTab::Latest));
}

// --- Restoring: what Main says -----------------------------------------------------------

#[test]
fn main_says_that_the_session_is_being_restored() {
    let (app, _) = restoring(120, 40);
    let text = main_text(&app);
    assert!(text.contains("restoring session…"), "{text}");
    assert!(!text.contains("offline — retrying"), "{text}");
}

#[test]
fn main_says_that_it_is_retrying_while_offline() {
    let (mut app, init) = restoring(120, 40);
    me_failed(&mut app, &init, "network");
    app.update(Action::ApiStatus {
        rate: None,
        online: false,
    });
    let text = main_text(&app);
    assert!(text.contains("offline — retrying"), "{text}");
    assert!(!text.contains("restoring session…"), "{text}");
}

// --- Returning to a guest (device_not_approved) ----------------------------------------------

/// A feed with items, a thread and a profile on the history, so that there is state to discard.
fn busy() -> App {
    let mut app = loaded();
    app.update(Action::Open(Screen::Thread("c1".into())));
    app.update(Action::Open(Screen::Profile("alice".into())));
    assert!(app.thread("c1").is_some() && app.profile("alice").is_some());
    app
}

fn reject_through_a_background_me(app: &mut App) -> Vec<Effect> {
    let due = app.update(Action::Tick(secs(30)));
    me_failed(app, &due, "device_not_approved")
}

#[test]
fn any_request_rejected_as_not_approved_returns_to_a_guest_with_the_dialog() {
    let mut app = loaded();
    let refresh = type_str(&mut app, "R");

    let effects = app.update(Action::FeedFailed {
        key: FOLLOWING,
        req: first_req(&refresh),
        failure: failure("device_not_approved"),
    });

    assert_eq!(app.session(), &Session::Guest);
    assert_eq!(app.screen(), Screen::Log(LogTab::Latest));
    assert_eq!(app.modal(), Some(Modal::SignIn));
    assert_eq!(
        effects
            .iter()
            .filter(|e| **e == Effect::UseGuestClient)
            .count(),
        1
    );
    assert_eq!(effects.first(), Some(&Effect::UseGuestClient));
    assert!(!effects.iter().any(|e| matches!(
        e,
        Effect::FetchLog { .. } | Effect::FetchMe { .. } | Effect::PollLog { .. }
    )));
    assert_eq!(
        toast_text(&app).as_deref(),
        Some("this device is no longer approved — sign in again")
    );
}

#[test]
fn a_rejected_background_refresh_returns_to_a_guest_too() {
    let mut app = loaded();
    let effects = reject_through_a_background_me(&mut app);
    assert_eq!(app.session(), &Session::Guest);
    assert_eq!(effects.first(), Some(&Effect::UseGuestClient));
}

#[test]
fn returning_to_a_guest_discards_what_the_account_had_loaded() {
    let mut app = busy();

    reject_through_a_background_me(&mut app);

    for key in FeedKey::ALL {
        let feed = app.feed(key);
        assert!(feed.items.is_empty(), "{key:?}");
        assert!(!feed.loaded_once, "{key:?}");
        assert!(feed.pending.is_none(), "{key:?}");
    }
    assert!(app.thread("c1").is_none());
    assert!(app.profile("alice").is_none());
    let back = app.update(Action::Back);
    assert!(back.is_empty());
    assert_eq!(app.screen(), Screen::Log(LogTab::Latest));
}

#[test]
fn returning_to_a_guest_discards_notifications_and_their_request() {
    let (mut app, init) = started(120, 40);
    me_loaded(&mut app, &init);
    type_str(&mut app, "gn");
    assert!(app.notifications().pending.is_some());

    reject_through_a_background_me(&mut app);

    assert!(app.notifications().pending.is_none());
    assert!(app.notifications().items.is_empty());
    assert_eq!(app.screen(), Screen::Log(LogTab::Latest));
}

#[test]
fn a_late_answer_to_a_request_sent_before_the_return_changes_nothing() {
    let (mut app, init) = started(120, 40);
    me_loaded(&mut app, &init);
    let rejected = reject_through_a_background_me(&mut app);
    assert_eq!(rejected.first(), Some(&Effect::UseGuestClient));

    deliver(&mut app, FOLLOWING, &init, log_page());
    me_loaded(&mut app, &init);

    assert!(app.feed(FOLLOWING).items.is_empty());
    assert_eq!(app.session(), &Session::Guest);
}

#[test]
fn a_guest_after_the_return_makes_no_requests_on_tick() {
    let mut app = loaded();
    reject_through_a_background_me(&mut app);
    let effects = app.update(Action::Tick(secs(120)));
    assert!(!any_fetch(&effects), "{effects:?}");
}

#[test]
fn the_guest_after_the_return_keeps_the_latest_tab_without_fetching() {
    let mut app = loaded();
    let effects = reject_through_a_background_me(&mut app);
    assert_eq!(app.session(), &Session::Guest);
    assert_eq!(app.screen(), Screen::Log(LogTab::Latest));
    assert_eq!(app.feed(LATEST).items.len(), 0);
    assert!(fetch_logs(&effects, LogTab::Latest).is_empty());
}

// --- Restoring: errors other than network are retried the same way -------------------

fn retry_failures() -> Vec<Failure> {
    vec![
        Failure::new("http_500"),
        Failure {
            code: "rate_limited".into(),
            retry_after: Some(30),
        },
        Failure::new("decode"),
    ]
}

#[test]
fn restoring_keeps_waiting_and_retries_after_any_error_but_the_rejections() {
    for first in retry_failures() {
        let (mut app, init) = restoring(120, 40);
        let effects = app.update(Action::MeFailed {
            req: me_req(&init),
            failure: first.clone(),
        });
        assert!(effects.is_empty(), "{first:?}: {effects:?}");
        assert_eq!(app.session(), &Session::Restoring, "{first:?}");
        assert_eq!(app.toast(), None, "{first:?}");

        let early = app.update(Action::Tick(secs(4)));
        assert_eq!(fetch_me_count(&early), 0, "{first:?}");
        let due = app.update(Action::Tick(secs(5)));
        assert_eq!(fetch_me_count(&due), 1, "{first:?}");

        app.update(Action::MeFailed {
            req: me_req(&due),
            failure: first.clone(),
        });
        assert_eq!(fetch_me_count(&app.update(Action::Tick(secs(14)))), 0);
        assert_eq!(fetch_me_count(&app.update(Action::Tick(secs(15)))), 1);
        assert_eq!(app.session(), &Session::Restoring, "{first:?}");
        assert_eq!(app.toast(), None, "{first:?}");
    }
}

#[test]
fn an_error_response_means_online_but_main_still_says_restoring() {
    let (mut app, init) = restoring(120, 40);
    me_failed(&mut app, &init, "http_500");
    app.update(Action::ApiStatus {
        rate: None,
        online: true,
    });
    assert_eq!(app.conn_state(), ConnState::Online);
    let text = main_text(&app);
    assert!(text.contains("restoring session…"), "{text}");
    assert!(!text.contains("offline — retrying"), "{text}");
}

#[test]
fn a_stale_failure_does_not_advance_the_restoring_backoff() {
    let (mut app, init) = restoring(120, 40);
    me_failed(&mut app, &init, "network");
    let due = app.update(Action::Tick(secs(5)));
    // The answer to the first request arrives again, late.
    me_failed(&mut app, &init, "network");
    me_failed(&mut app, &due, "network");
    assert_eq!(fetch_me_count(&app.update(Action::Tick(secs(14)))), 0);
    assert_eq!(fetch_me_count(&app.update(Action::Tick(secs(15)))), 1);
}

// --- the device key could not be read ----------------------------------------------------

fn unreadable(error: KeyError) -> App {
    let status = KeyStatus::Unreadable {
        error,
        path: "/home/u/.config/lazydn/id_ed25519".into(),
    };
    let mut app = App::with_key_status(
        Session::Guest,
        NOW,
        UtcOffset::UTC,
        status,
        "/home/user/.config/lazydn/id_ed25519".into(),
    );
    app.update(Action::Resize(120, 40));
    app
}

#[test]
fn an_unreadable_key_starts_a_guest_with_the_dialog_and_an_error_toast() {
    let app = unreadable(KeyError::InsecurePermissions);
    assert_eq!(app.session(), &Session::Guest);
    assert_eq!(app.modal(), Some(Modal::SignIn));
    assert_eq!(app.toast().map(|t| t.kind), Some(ToastKind::Error));
}

#[test]
fn the_toast_for_an_unreadable_key_names_the_path_and_the_fix() {
    let p = "/home/u/.config/lazydn/id_ed25519";
    let cases = [
        (
            KeyError::InsecurePermissions,
            format!("key {p} is readable by others — run chmod 600 {p}"),
        ),
        (
            KeyError::Encrypted,
            format!("key {p} is encrypted — lazydn needs an unencrypted ed25519 key"),
        ),
        (
            KeyError::NotEd25519,
            format!("key {p} is not an ed25519 key"),
        ),
        (
            KeyError::Parse("x".into()),
            format!("key {p} could not be parsed"),
        ),
        (
            KeyError::NotARegularFile,
            format!("{p} is not a regular file"),
        ),
        (
            KeyError::Io("permission denied".into()),
            format!("key {p} could not be read (permission denied)"),
        ),
    ];
    for (error, expected) in cases {
        let app = unreadable(error.clone());
        assert_eq!(toast_text(&app), Some(expected), "{error:?}");
    }
}

#[test]
fn a_missing_or_usable_key_starts_without_a_toast() {
    for (session, status) in [
        (Session::Guest, KeyStatus::Absent),
        (Session::Restoring, KeyStatus::Present),
    ] {
        let app = App::with_key_status(
            session.clone(),
            NOW,
            UtcOffset::UTC,
            status,
            "/home/user/.config/lazydn/id_ed25519".into(),
        );
        assert_eq!(app.toast(), None);
        assert_eq!(app.session(), &session);
    }
}

// --- more of what the return to a guest discards --------------------------------------------

#[test]
fn a_current_user_answer_in_flight_during_the_return_does_not_sign_in_again() {
    let mut app = loaded();
    let poll = app.update(Action::Tick(secs(30)));
    assert_eq!(fetch_me_count(&poll), 1);
    let refresh = type_str(&mut app, "R");

    app.update(Action::FeedFailed {
        key: FOLLOWING,
        req: first_req(&refresh),
        failure: failure("device_not_approved"),
    });
    assert_eq!(app.session(), &Session::Guest);

    me_loaded(&mut app, &poll);
    assert_eq!(app.session(), &Session::Guest);
}

#[test]
fn returning_to_a_guest_discards_connection_lists() {
    let mut app = loaded();
    app.update(Action::Open(Screen::Connections(
        "alice".into(),
        ConnTab::Followers,
    )));
    assert!(app.connections("alice", ConnTab::Followers).is_some());

    reject_through_a_background_me(&mut app);

    assert!(app.connections("alice", ConnTab::Followers).is_none());
}

#[test]
fn returning_to_a_guest_discards_the_settings_lists_and_their_requests() {
    let mut app = settings_app(120, 30);
    let first = open_safety(&mut app);
    load_muted(&mut app, &first, &["m"], None);
    assert!(app.settings().blocked.pending.is_some());

    app.update(Action::MeFailed {
        req: crate::feed::ReqId(1),
        failure: failure("device_not_approved"),
    });

    let settings = app.settings();
    assert!(settings.muted.users.is_empty() && !settings.muted.loaded_once);
    assert!(settings.blocked.pending.is_none() && !settings.blocked.loaded_once);

    load_blocked(&mut app, &first, &["late"], None);
    assert!(app.settings().blocked.users.is_empty());
}

#[test]
fn a_notifications_answer_after_the_return_is_dropped() {
    let (mut app, init) = started(120, 40);
    me_loaded(&mut app, &init);
    let entered = type_str(&mut app, "gn");
    let due = app.update(Action::Tick(secs(30)));
    me_failed(&mut app, &due, "device_not_approved");

    let page = serde_json::from_str::<crate::model::NotificationsPage>(include_str!(
        "../tests/fixtures/notifications.json"
    ))
    .unwrap();
    deliver_notifs(&mut app, &entered, page);

    assert!(app.notifications().items.is_empty());
}

// --- the rejection arrives more than once ---------------------------------------------------

#[test]
fn two_rejections_return_to_a_guest_only_once() {
    let mut app = loaded();
    let first = type_str(&mut app, "R");
    let second = type_str(&mut app, "R");
    let effects = app.update(Action::FeedFailed {
        key: FOLLOWING,
        req: first_req(&second),
        failure: failure("device_not_approved"),
    });
    assert_eq!(effects.first(), Some(&Effect::UseGuestClient));
    app.update(Action::CloseModal);
    let toast = app.toast().cloned();
    app.update(Action::Tick(secs(2)));

    let again = app.update(Action::FeedFailed {
        key: FOLLOWING,
        req: first_req(&first),
        failure: failure("device_not_approved"),
    });

    assert!(!again.contains(&Effect::UseGuestClient), "{again:?}");
    assert_eq!(app.session(), &Session::Guest);
    assert_eq!(app.modal(), None);
    assert_eq!(app.toast().cloned(), toast);
}

#[test]
fn a_rejection_of_a_superseded_request_still_returns_to_a_guest() {
    let mut app = loaded();
    let old = type_str(&mut app, "R");
    type_str(&mut app, "R");

    let effects = app.update(Action::FeedFailed {
        key: FOLLOWING,
        req: first_req(&old),
        failure: failure("device_not_approved"),
    });

    assert_eq!(app.session(), &Session::Guest);
    assert_eq!(effects.first(), Some(&Effect::UseGuestClient));
    assert_eq!(app.modal(), Some(Modal::SignIn));
}

// --- Restoring: finishing on the screen the user is on, and the retry clock ------------------

#[test]
fn me_loaded_fetches_the_tab_the_user_moved_to_while_restoring() {
    for (keys, tab) in [("2", LogTab::Latest), ("3", LogTab::Trending)] {
        let (mut app, init) = restoring(120, 40);
        type_str(&mut app, keys);
        assert_eq!(app.screen(), Screen::Log(tab));

        let effects = me_loaded(&mut app, &init);

        assert_eq!(app.screen(), Screen::Log(tab));
        assert_eq!(fetch_logs(&effects, tab), vec![None], "{keys}");
        assert!(fetch_logs(&effects, LogTab::Following).is_empty(), "{keys}");
    }
}

#[test]
fn every_log_tab_says_restoring_or_offline_while_the_session_is_checked() {
    for keys in ["", "2", "3"] {
        let (mut app, init) = restoring(120, 40);
        type_str(&mut app, keys);
        let text = main_text(&app);
        assert!(text.contains("restoring session…"), "{keys}: {text}");

        me_failed(&mut app, &init, "network");
        app.update(Action::ApiStatus {
            rate: None,
            online: false,
        });
        let text = main_text(&app);
        assert!(text.contains("offline — retrying"), "{keys}: {text}");
    }
}

#[test]
fn the_retry_delay_counts_from_the_failure_not_from_the_request() {
    let (mut app, init) = restoring(120, 40);
    // The request is still unanswered 15 seconds after it went out.
    app.update(Action::Tick(secs(15)));
    me_failed(&mut app, &init, "network");

    assert_eq!(fetch_me_count(&app.update(Action::Tick(secs(19)))), 0);
    assert_eq!(fetch_me_count(&app.update(Action::Tick(secs(20)))), 1);
}

// --- Restoring: no sign-in hints, since L does nothing yet ----------------------------------

#[test]
fn restoring_shows_no_sign_in_hint_in_the_nav_or_the_aside() {
    for (w, h) in [(120, 40), (80, 24), (60, 24)] {
        let (app, _) = restoring(w, h);
        let text = main_text(&app);
        assert!(!text.contains("Sign in"), "{w}x{h}: {text}");
        assert!(!text.contains("New to /dev/null?"), "{w}x{h}: {text}");
    }
}

#[test]
fn the_aside_says_what_the_session_is_doing() {
    let (mut app, init) = restoring(120, 40);
    let aside = |app: &App| {
        let terminal = render(app);
        let buffer = terminal.backend().buffer().clone();
        (0..buffer.area.height)
            .map(|y| row_text(&buffer, y).chars().skip(88).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    };
    assert!(aside(&app).contains("restoring session…"));

    me_failed(&mut app, &init, "network");
    app.update(Action::ApiStatus {
        rate: None,
        online: false,
    });
    assert!(aside(&app).contains("offline — retrying"));
}
