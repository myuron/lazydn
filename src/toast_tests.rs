use time::Duration;

use crate::action::{Action, Effect, Failure};
use crate::api::client::RateLimit;
use crate::app::{App, ConnState, Session, ToastKind};
use crate::feed::{FeedPage, ReqId};
use crate::feed_tests::{FOLLOWING, deliver, first_req, started};
use crate::model::LogPage;
use crate::notifications_tests::{me_req, me_resp};
use crate::test_support::*;

const LOG: &str = include_str!("../tests/fixtures/log_following.json");

// --- helpers ----------------------------------------------------------------

pub(crate) fn log_page() -> FeedPage {
    FeedPage::from(serde_json::from_str::<LogPage>(LOG).expect("fixture parses"))
}

/// Signed in at 120x40 with a Following list that has items and a next page.
pub(crate) fn loaded() -> App {
    let (mut app, effects) = started(120, 40);
    deliver(&mut app, FOLLOWING, &effects, log_page());
    // Clears the startup request so that a later Tick can send the next one.
    app.update(Action::MeLoaded {
        req: me_req(&effects),
        response: Box::new(me_resp(12)),
    });
    app
}

fn failure(code: &str, retry_after: Option<u64>) -> Failure {
    Failure {
        code: code.into(),
        retry_after,
    }
}

/// Refreshes the list and answers the request with `failure`; returns the effects of the refresh.
pub(crate) fn refresh_and_fail(app: &mut App, failure: Failure) -> Vec<Effect> {
    let effects = type_str(app, "R");
    app.update(Action::FeedFailed {
        key: FOLLOWING,
        req: first_req(&effects),
        failure,
    });
    effects
}

fn toast_text(app: &App) -> Option<String> {
    app.toast().map(|t| t.text.clone())
}

fn secs(s: i64) -> time::OffsetDateTime {
    NOW + Duration::seconds(s)
}

// --- 4. text per code --------------------------------------------------------

#[test]
fn rate_limited_names_the_wait_when_retry_after_is_known() {
    let mut app = loaded();
    refresh_and_fail(&mut app, failure("rate_limited", Some(12)));
    assert_eq!(
        toast_text(&app).as_deref(),
        Some("rate_limited: retry in 12s")
    );
}

#[test]
fn rate_limited_without_retry_after_shows_the_bare_code() {
    let mut app = loaded();
    refresh_and_fail(&mut app, failure("rate_limited", None));
    assert_eq!(toast_text(&app).as_deref(), Some("rate_limited"));
}

#[test]
fn invalid_signature_points_at_the_system_clock() {
    let mut app = loaded();
    refresh_and_fail(&mut app, failure("invalid_signature", None));
    assert_eq!(
        toast_text(&app).as_deref(),
        Some("invalid_signature: check your system clock")
    );
    assert!(matches!(app.session(), Session::SignedIn(_)));
}

#[test]
fn network_failure_tells_the_user_to_retry() {
    let mut app = loaded();
    refresh_and_fail(&mut app, failure("network", None));
    assert_eq!(
        toast_text(&app).as_deref(),
        Some("network error — press R to retry")
    );
}

#[test]
fn any_other_code_is_shown_as_it_is() {
    let mut app = loaded();
    refresh_and_fail(&mut app, failure("http_500", None));
    assert_eq!(toast_text(&app).as_deref(), Some("http_500"));
}

#[test]
fn a_failure_toast_is_an_error_toast() {
    let mut app = loaded();
    refresh_and_fail(&mut app, failure("http_500", None));
    assert_eq!(app.toast().map(|t| t.kind), Some(ToastKind::Error));
}

#[test]
fn control_characters_in_a_server_code_are_removed() {
    let mut app = loaded();
    refresh_and_fail(&mut app, failure("bad\u{1b}[31m\r\ncode\u{7}", None));
    assert_eq!(toast_text(&app).as_deref(), Some("bad[31mcode"));
}

#[test]
fn a_server_code_is_cut_at_64_characters() {
    let mut app = loaded();
    refresh_and_fail(&mut app, failure(&"x".repeat(100), None));
    assert_eq!(toast_text(&app), Some("x".repeat(64)));
}

// --- 4. lifetime, coalescing, newest only --------------------------------------

#[test]
fn a_toast_lasts_three_seconds_from_the_clock_of_the_app() {
    let mut app = loaded();
    refresh_and_fail(&mut app, failure("http_500", None));
    assert_eq!(app.toast().map(|t| t.until), Some(secs(3)));

    app.update(Action::Tick(secs(2)));
    assert!(app.toast().is_some());
    app.update(Action::Tick(secs(3)));
    assert_eq!(app.toast(), None);
}

#[test]
fn only_the_newest_toast_is_kept() {
    let mut app = loaded();
    refresh_and_fail(&mut app, failure("http_500", None));
    refresh_and_fail(&mut app, failure("network", None));
    assert_eq!(
        toast_text(&app).as_deref(),
        Some("network error — press R to retry")
    );
}

#[test]
fn the_same_message_again_only_extends_the_deadline() {
    let mut app = loaded();
    refresh_and_fail(&mut app, failure("http_500", None));
    let first = app.toast().cloned().expect("a toast is shown");

    app.update(Action::Tick(secs(2)));
    refresh_and_fail(&mut app, failure("http_500", None));

    let second = app.toast().cloned().expect("still shown");
    assert_eq!((second.kind, second.text), (first.kind, first.text));
    assert_eq!(second.until, secs(5));
    app.update(Action::Tick(secs(4)));
    assert!(app.toast().is_some());
    app.update(Action::Tick(secs(5)));
    assert_eq!(app.toast(), None);
}

// --- when no toast is shown -------------------------------------------------------

#[test]
fn a_failure_of_an_unknown_request_shows_no_toast() {
    let mut app = loaded();
    type_str(&mut app, "R");
    app.update(Action::FeedFailed {
        key: FOLLOWING,
        req: ReqId(9999),
        failure: failure("http_500", None),
    });
    assert_eq!(app.toast(), None);
}

#[test]
fn a_failure_of_a_superseded_request_shows_no_toast() {
    let mut app = loaded();
    let old = type_str(&mut app, "R");
    type_str(&mut app, "R");
    app.update(Action::FeedFailed {
        key: FOLLOWING,
        req: first_req(&old),
        failure: failure("http_500", None),
    });
    assert_eq!(app.toast(), None);
}

#[test]
fn a_failed_first_load_shows_the_error_line_and_no_toast() {
    let (mut app, effects) = started(120, 40);
    app.update(Action::FeedFailed {
        key: FOLLOWING,
        req: first_req(&effects),
        failure: failure("http_500", None),
    });
    assert_eq!(app.toast(), None);
    let text = row_texts(&app);
    assert!(
        text.iter().any(|l| l.contains("error: http_500")),
        "{text:?}"
    );
}

#[test]
fn a_failed_background_refresh_of_the_current_user_shows_no_toast() {
    let (mut app, init) = started(120, 40);
    app.update(Action::MeLoaded {
        req: me_req(&init),
        response: Box::new(me_resp(12)),
    });
    let due = app.update(Action::Tick(secs(30)));
    app.update(Action::MeFailed {
        req: me_req(&due),
        failure: failure("network", None),
    });
    assert_eq!(app.toast(), None);
    assert!(matches!(app.session(), Session::SignedIn(_)));
}

#[test]
fn a_rate_limited_background_refresh_shows_no_toast() {
    let (mut app, init) = started(120, 40);
    app.update(Action::MeLoaded {
        req: me_req(&init),
        response: Box::new(me_resp(12)),
    });
    let due = app.update(Action::Tick(secs(30)));
    app.update(Action::MeFailed {
        req: me_req(&due),
        failure: failure("rate_limited", Some(5)),
    });
    assert_eq!(app.toast(), None);
}

#[test]
fn a_background_refresh_rejected_as_not_approved_still_shows_its_toast() {
    let (mut app, init) = started(120, 40);
    app.update(Action::MeLoaded {
        req: me_req(&init),
        response: Box::new(me_resp(12)),
    });
    let due = app.update(Action::Tick(secs(30)));
    app.update(Action::MeFailed {
        req: me_req(&due),
        failure: failure("device_not_approved", None),
    });
    assert_eq!(
        toast_text(&app).as_deref(),
        Some("this device is no longer approved — sign in again")
    );
}

// --- 6. connection state and rate limit in the app ---------------------------------

fn status(rate: Option<RateLimit>, online: bool) -> Action {
    Action::ApiStatus { rate, online }
}

const RATE: RateLimit = RateLimit {
    limit: 120,
    remaining: 118,
};

#[test]
fn the_connection_starts_as_connecting_with_no_rate() {
    let app = signed_in(120, 40);
    assert_eq!(app.conn_state(), ConnState::Connecting);
    assert_eq!(app.rate(), None);
}

#[test]
fn a_response_makes_the_connection_online_and_records_the_rate() {
    let mut app = signed_in(120, 40);
    app.update(status(Some(RATE), true));
    assert_eq!(app.conn_state(), ConnState::Online);
    assert_eq!(app.rate(), Some(RATE));
}

#[test]
fn a_status_without_a_rate_keeps_the_previous_rate() {
    let mut app = signed_in(120, 40);
    app.update(status(Some(RATE), true));
    app.update(status(None, true));
    assert_eq!(app.rate(), Some(RATE));
}

#[test]
fn an_offline_status_keeps_the_rate_and_a_later_response_brings_it_back_online() {
    let mut app = signed_in(120, 40);
    app.update(status(Some(RATE), true));
    app.update(status(None, false));
    assert_eq!(app.conn_state(), ConnState::Offline);
    assert_eq!(app.rate(), Some(RATE));
    app.update(status(None, true));
    assert_eq!(app.conn_state(), ConnState::Online);
}

#[test]
fn a_newer_rate_replaces_the_older_one() {
    let mut app = signed_in(120, 40);
    app.update(status(Some(RATE), true));
    let newer = RateLimit {
        limit: 120,
        remaining: 117,
    };
    app.update(status(Some(newer), true));
    assert_eq!(app.rate(), Some(newer));
}

#[test]
fn the_host_label_is_kept_for_the_status_line() {
    let mut app = signed_in(120, 40);
    assert_eq!(app.host_label(), None);
    app.set_host_label(Some("127.0.0.1:8080".into()));
    assert_eq!(app.host_label(), Some("127.0.0.1:8080"));
}

fn row_texts(app: &App) -> Vec<String> {
    let terminal = render(app);
    let buffer = terminal.backend().buffer().clone();
    (0..buffer.area.height)
        .map(|y| row_text(&buffer, y))
        .collect()
}

#[test]
fn control_characters_are_removed_before_the_text_is_cut() {
    let mut app = loaded();
    let code = format!("{}{}", "\u{1b}".repeat(10), "x".repeat(70));
    refresh_and_fail(&mut app, failure(&code, None));
    assert_eq!(toast_text(&app), Some("x".repeat(64)));
}

// --- failures of the other lists: a toast only when there is something on screen ---

mod other_lists {
    use super::*;
    use crate::app::{ConnTab, Screen};
    use crate::notifications_tests::{
        group_req, grouped, loaded as notifs_loaded, notif_req, on_notifications,
    };
    use crate::profile_tests::{conn_req, deliver_conns, users};
    use crate::settings_tests::{
        blocked_req, load_blocked, load_muted, muted_req, open_safety, settings_app,
    };
    use crate::thread_tests::{MAIN, item, open_thread, opened, replies_req};

    fn fail(code: &str) -> Failure {
        Failure::new(code)
    }

    #[test]
    fn a_failed_notifications_refresh_over_a_list_shows_a_toast() {
        let mut app = notifs_loaded(120, 40, vec![grouped("starred", 3)], None);
        let effects = type_str(&mut app, "R");
        app.update(Action::NotificationsFailed {
            req: notif_req(&effects),
            failure: fail("http_500"),
        });
        assert_eq!(toast_text(&app).as_deref(), Some("http_500"));
    }

    #[test]
    fn a_failed_first_notifications_load_shows_no_toast() {
        let (mut app, effects) = on_notifications(120, 40);
        app.update(Action::NotificationsFailed {
            req: notif_req(&effects),
            failure: fail("http_500"),
        });
        assert_eq!(app.toast(), None);
    }

    #[test]
    fn a_failed_group_expansion_shows_no_toast_because_the_group_says_it_itself() {
        let mut app = notifs_loaded(120, 40, vec![grouped("starred", 3)], None);
        let effects = type_str(&mut app, "o");
        app.update(Action::GroupFailed {
            id: "n0".into(),
            req: group_req(&effects),
            failure: fail("http_500"),
        });
        assert_eq!(app.toast(), None);
    }

    fn replies(n: usize) -> Vec<crate::model::ThreadItem> {
        (0..n).map(|i| item(&format!("r{i}"), "bob")).collect()
    }

    #[test]
    fn a_failed_replies_page_below_loaded_replies_shows_a_toast() {
        let mut app = opened(120, 40, vec![], 9, replies(8), Some("next"));
        let effects = type_str(&mut app, "G");
        assert!(
            effects
                .iter()
                .any(|e| matches!(e, Effect::FetchReplies { .. })),
            "{effects:?}"
        );
        app.update(Action::RepliesFailed {
            thread: MAIN.into(),
            parent: MAIN.into(),
            req: replies_req(&effects, MAIN),
            failure: fail("http_500"),
        });
        assert_eq!(toast_text(&app).as_deref(), Some("http_500"));
    }

    #[test]
    fn a_failed_first_replies_load_shows_no_toast() {
        let mut app = signed_in(120, 40);
        let effects = open_thread(&mut app, MAIN);
        app.update(Action::RepliesFailed {
            thread: MAIN.into(),
            parent: MAIN.into(),
            req: replies_req(&effects, MAIN),
            failure: fail("http_500"),
        });
        assert_eq!(app.toast(), None);
    }

    fn open_followers(app: &mut App) -> Vec<Effect> {
        app.update(Action::Open(Screen::Connections(
            "alice".into(),
            ConnTab::Followers,
        )))
    }

    #[test]
    fn a_failed_connections_refresh_over_a_list_shows_a_toast() {
        let mut app = signed_in(120, 40);
        let first = open_followers(&mut app);
        deliver_conns(
            &mut app,
            &first,
            "alice",
            ConnTab::Followers,
            users(0, 2),
            None,
        );
        let effects = type_str(&mut app, "R");
        app.update(Action::ConnectionsFailed {
            username: "alice".into(),
            tab: ConnTab::Followers,
            req: conn_req(&effects),
            failure: fail("http_500"),
        });
        assert_eq!(toast_text(&app).as_deref(), Some("http_500"));
    }

    #[test]
    fn a_failed_first_connections_load_shows_no_toast() {
        let mut app = signed_in(120, 40);
        let first = open_followers(&mut app);
        app.update(Action::ConnectionsFailed {
            username: "alice".into(),
            tab: ConnTab::Followers,
            req: conn_req(&first),
            failure: fail("http_500"),
        });
        assert_eq!(app.toast(), None);
    }

    #[test]
    fn failed_muted_and_blocked_refreshes_over_lists_show_a_toast() {
        let mut app = settings_app(120, 30);
        let first = open_safety(&mut app);
        load_muted(&mut app, &first, &["m"], None);
        load_blocked(&mut app, &first, &["b"], None);

        let effects = type_str(&mut app, "R");
        app.update(Action::MutedFailed {
            req: muted_req(&effects),
            failure: fail("http_500"),
        });
        assert_eq!(toast_text(&app).as_deref(), Some("http_500"));

        app.update(Action::Tick(NOW + Duration::seconds(5)));
        let effects = type_str(&mut app, "R");
        app.update(Action::BlockedFailed {
            req: blocked_req(&effects),
            failure: fail("rate_limited"),
        });
        assert_eq!(toast_text(&app).as_deref(), Some("rate_limited"));
    }

    #[test]
    fn failed_first_muted_and_blocked_loads_show_no_toast() {
        let mut app = settings_app(120, 30);
        let first = open_safety(&mut app);
        app.update(Action::MutedFailed {
            req: muted_req(&first),
            failure: fail("http_500"),
        });
        app.update(Action::BlockedFailed {
            req: blocked_req(&first),
            failure: fail("http_500"),
        });
        assert_eq!(app.toast(), None);
    }
}
