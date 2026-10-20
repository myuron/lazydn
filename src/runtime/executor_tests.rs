use std::sync::Arc;
use std::time::Duration;

use serde::de::DeserializeOwned;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

use super::executor::{execute, failure_for};
use crate::action::{Action, Effect, Failure};
use crate::api::client::{ApiClient, RateLimit};
use crate::api::clock::FixedClock;
use crate::api::test_support::TestSigner;
use crate::app::{ConnTab, LogTab, ProfileTab};
use crate::feed::{FeedKey, FeedPage, ReqId};
use crate::model::{
    GroupPage, LogPage, MeResponse, NotificationsPage, Page, ThreadItem, ThreadResponse, User,
    UserResponse,
};

const ME: &str = include_str!("../../tests/fixtures/me.json");
const LOG: &str = include_str!("../../tests/fixtures/log_following.json");
const THREAD: &str = include_str!("../../tests/fixtures/thread.json");
const REPLIES: &str = include_str!("../../tests/fixtures/replies_with_tombstone.json");
const USER: &str = include_str!("../../tests/fixtures/user.json");
const COMMITS: &str = include_str!("../../tests/fixtures/commits.json");
const FOLLOWERS: &str = include_str!("../../tests/fixtures/followers.json");
const NOTIFICATIONS: &str = include_str!("../../tests/fixtures/notifications.json");
const MUTED: &str = include_str!("../../tests/fixtures/muted.json");

const REQ: ReqId = ReqId(7);
const ONLINE: Action = Action::ApiStatus {
    rate: None,
    online: true,
};

// --- fixtures -----------------------------------------------------------------

fn client_for(base: &str, signed: bool) -> ApiClient {
    let signer: Option<Arc<dyn crate::api::sign::Signer>> = if signed {
        Some(Arc::new(TestSigner::golden()))
    } else {
        None
    };
    ApiClient::new(base, signer, Arc::new(FixedClock(1_700_000_000))).expect("client builds")
}

fn signed(server: &MockServer) -> ApiClient {
    client_for(&server.uri(), true)
}

/// Nothing listens on port 1 of the loopback, so the connection is refused at once.
fn unreachable_client() -> ApiClient {
    client_for("http://127.0.0.1:1", true)
}

fn parse<T: DeserializeOwned>(body: &str) -> T {
    serde_json::from_str(body).expect("fixture parses")
}

fn json(body: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_raw(body.as_bytes().to_vec(), "application/json")
}

fn error_body(status: u16, code: &str) -> ResponseTemplate {
    ResponseTemplate::new(status).set_body_raw(
        format!(r#"{{"message":"m","code":"{code}"}}"#).into_bytes(),
        "application/json",
    )
}

fn failure_mut(action: &mut Action) -> Option<&mut Failure> {
    match action {
        Action::FeedFailed { failure, .. }
        | Action::ThreadFailed { failure, .. }
        | Action::RepliesFailed { failure, .. }
        | Action::UserFailed { failure, .. }
        | Action::ProfileFeedFailed { failure, .. }
        | Action::ConnectionsFailed { failure, .. }
        | Action::MeFailed { failure, .. }
        | Action::NotificationsFailed { failure, .. }
        | Action::GroupFailed { failure, .. }
        | Action::MutedFailed { failure, .. }
        | Action::BlockedFailed { failure, .. }
        | Action::CommitFailed { failure, .. } => Some(failure),
        _ => None,
    }
}

async fn serve(server: &MockServer, p: &str, response: ResponseTemplate) {
    Mock::given(method("GET"))
        .and(path(p))
        .respond_with(response)
        .mount(server)
        .await;
}

async fn received(server: &MockServer) -> Vec<Request> {
    server.received_requests().await.expect("recording is on")
}

/// The one `*Loaded` or `*Failed` action, with the trailing status split off.
fn split(actions: Vec<Action>) -> (Vec<Action>, Vec<Action>) {
    actions
        .into_iter()
        .partition(|a| !matches!(a, Action::ApiStatus { .. }))
}

async fn run_ok(effect: Effect, p: &str, body: &str) -> Action {
    let server = MockServer::start().await;
    serve(&server, p, json(body)).await;
    let (results, status) = split(execute(&signed(&server), effect).await);
    assert_eq!(status, vec![ONLINE]);
    assert_eq!(results.len(), 1, "{results:?}");
    results.into_iter().next().unwrap()
}

fn feed_page_of_log() -> FeedPage {
    FeedPage::from(parse::<LogPage>(LOG))
}

fn feed_page_of_commits() -> FeedPage {
    FeedPage::from(parse::<Page<crate::model::Commit>>(COMMITS))
}

/// Every effect that calls the API and has a `*Failed` counterpart, with the path its call hits.
fn api_effects() -> Vec<(Effect, &'static str)> {
    vec![
        (
            Effect::FetchLog {
                tab: LogTab::Following,
                cursor: None,
                req: REQ,
            },
            "/api/v1/log/following",
        ),
        (
            Effect::FetchSaved {
                cursor: None,
                req: REQ,
            },
            "/api/v1/saved",
        ),
        (
            Effect::FetchThread {
                id: "c1".into(),
                req: REQ,
            },
            "/api/v1/commits/c1",
        ),
        (
            Effect::FetchReplies {
                thread: "c1".into(),
                parent: "c1".into(),
                cursor: None,
                req: REQ,
            },
            "/api/v1/commits/c1/replies",
        ),
        (
            Effect::FetchUser {
                username: "alice".into(),
                req: REQ,
            },
            "/api/v1/users/alice",
        ),
        (
            Effect::FetchUserCommits {
                username: "alice".into(),
                replies: false,
                cursor: None,
                req: REQ,
            },
            "/api/v1/users/alice/commits",
        ),
        (
            Effect::FetchConnections {
                username: "alice".into(),
                tab: ConnTab::Followers,
                cursor: None,
                req: REQ,
            },
            "/api/v1/users/alice/followers",
        ),
        (Effect::FetchMe { req: REQ }, "/api/v1/me"),
        (
            Effect::FetchNotifications {
                types: "all".into(),
                cursor: None,
                req: REQ,
            },
            "/api/v1/notifications",
        ),
        (
            Effect::FetchNotificationGroup {
                id: "n1".into(),
                types: "all".into(),
                cursor: None,
                req: REQ,
            },
            "/api/v1/notifications/n1/group",
        ),
        (
            Effect::FetchMuted {
                cursor: None,
                req: REQ,
            },
            "/api/v1/me/muted",
        ),
        (
            Effect::FetchBlocked {
                cursor: None,
                req: REQ,
            },
            "/api/v1/me/blocked",
        ),
    ]
}

// --- 1. each effect becomes its *Loaded action --------------------------------

#[tokio::test]
async fn fetch_log_becomes_feed_loaded_for_the_same_key_and_req() {
    let effect = Effect::FetchLog {
        tab: LogTab::Following,
        cursor: None,
        req: REQ,
    };
    let action = run_ok(effect, "/api/v1/log/following", LOG).await;
    assert_eq!(
        action,
        Action::FeedLoaded {
            key: FeedKey::Log(LogTab::Following),
            req: REQ,
            page: feed_page_of_log(),
        }
    );
}

#[tokio::test]
async fn fetch_log_asks_for_the_log_named_by_the_tab_and_passes_the_cursor() {
    let server = MockServer::start().await;
    serve(&server, "/api/v1/log/trending", json(LOG)).await;
    let effect = Effect::FetchLog {
        tab: LogTab::Trending,
        cursor: Some("cur".into()),
        req: REQ,
    };
    let actions = execute(&signed(&server), effect).await;
    assert!(matches!(
        actions.first(),
        Some(Action::FeedLoaded {
            key: FeedKey::Log(LogTab::Trending),
            ..
        })
    ));
    let requests = received(&server).await;
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].url.query(), Some("cursor=cur"));
}

#[tokio::test]
async fn fetch_saved_becomes_feed_loaded_for_the_saved_key() {
    let effect = Effect::FetchSaved {
        cursor: None,
        req: REQ,
    };
    let action = run_ok(effect, "/api/v1/saved", COMMITS).await;
    assert_eq!(
        action,
        Action::FeedLoaded {
            key: FeedKey::Saved,
            req: REQ,
            page: feed_page_of_commits(),
        }
    );
}

#[tokio::test]
async fn fetch_thread_becomes_thread_loaded() {
    let effect = Effect::FetchThread {
        id: "c1".into(),
        req: REQ,
    };
    let action = run_ok(effect, "/api/v1/commits/c1", THREAD).await;
    assert_eq!(
        action,
        Action::ThreadLoaded {
            id: "c1".into(),
            req: REQ,
            response: Box::new(parse::<ThreadResponse>(THREAD)),
        }
    );
}

#[tokio::test]
async fn fetch_replies_becomes_replies_loaded_with_thread_and_parent() {
    let effect = Effect::FetchReplies {
        thread: "t1".into(),
        parent: "p1".into(),
        cursor: None,
        req: REQ,
    };
    let action = run_ok(effect, "/api/v1/commits/p1/replies", REPLIES).await;
    assert_eq!(
        action,
        Action::RepliesLoaded {
            thread: "t1".into(),
            parent: "p1".into(),
            req: REQ,
            page: parse::<Page<ThreadItem>>(REPLIES),
        }
    );
}

#[tokio::test]
async fn fetch_user_becomes_user_loaded() {
    let effect = Effect::FetchUser {
        username: "alice".into(),
        req: REQ,
    };
    let action = run_ok(effect, "/api/v1/users/alice", USER).await;
    assert_eq!(
        action,
        Action::UserLoaded {
            username: "alice".into(),
            req: REQ,
            response: Box::new(parse::<UserResponse>(USER)),
        }
    );
}

#[tokio::test]
async fn fetch_user_commits_becomes_profile_feed_loaded_on_the_commits_tab() {
    let effect = Effect::FetchUserCommits {
        username: "alice".into(),
        replies: false,
        cursor: None,
        req: REQ,
    };
    let action = run_ok(effect, "/api/v1/users/alice/commits", COMMITS).await;
    assert_eq!(
        action,
        Action::ProfileFeedLoaded {
            username: "alice".into(),
            tab: ProfileTab::Commits,
            req: REQ,
            page: feed_page_of_commits(),
        }
    );
}

#[tokio::test]
async fn fetch_user_commits_with_replies_lands_on_the_replies_tab() {
    let server = MockServer::start().await;
    serve(&server, "/api/v1/users/alice/commits", json(COMMITS)).await;
    let effect = Effect::FetchUserCommits {
        username: "alice".into(),
        replies: true,
        cursor: None,
        req: REQ,
    };
    let actions = execute(&signed(&server), effect).await;
    assert!(matches!(
        actions.first(),
        Some(Action::ProfileFeedLoaded {
            tab: ProfileTab::Replies,
            ..
        })
    ));
    assert_eq!(received(&server).await[0].url.query(), Some("replies=true"));
}

#[tokio::test]
async fn fetch_connections_picks_followers_or_following_by_tab() {
    let server = MockServer::start().await;
    serve(&server, "/api/v1/users/alice/following", json(FOLLOWERS)).await;
    let effect = Effect::FetchConnections {
        username: "alice".into(),
        tab: ConnTab::Following,
        cursor: None,
        req: REQ,
    };
    let (results, _) = split(execute(&signed(&server), effect).await);
    assert_eq!(
        results,
        vec![Action::ConnectionsLoaded {
            username: "alice".into(),
            tab: ConnTab::Following,
            req: REQ,
            page: parse::<Page<User>>(FOLLOWERS),
        }]
    );
}

#[tokio::test]
async fn fetch_connections_for_followers_becomes_connections_loaded() {
    let effect = Effect::FetchConnections {
        username: "alice".into(),
        tab: ConnTab::Followers,
        cursor: None,
        req: REQ,
    };
    let action = run_ok(effect, "/api/v1/users/alice/followers", FOLLOWERS).await;
    assert!(matches!(
        action,
        Action::ConnectionsLoaded {
            tab: ConnTab::Followers,
            ..
        }
    ));
}

#[tokio::test]
async fn fetch_me_becomes_me_loaded() {
    let action = run_ok(Effect::FetchMe { req: REQ }, "/api/v1/me", ME).await;
    assert_eq!(
        action,
        Action::MeLoaded {
            req: REQ,
            response: Box::new(parse::<MeResponse>(ME)),
        }
    );
}

#[tokio::test]
async fn fetch_notifications_becomes_notifications_loaded() {
    let effect = Effect::FetchNotifications {
        types: "all".into(),
        cursor: None,
        req: REQ,
    };
    let action = run_ok(effect, "/api/v1/notifications", NOTIFICATIONS).await;
    assert_eq!(
        action,
        Action::NotificationsLoaded {
            req: REQ,
            page: Box::new(parse::<NotificationsPage>(NOTIFICATIONS)),
        }
    );
}

#[tokio::test]
async fn fetch_notification_group_becomes_group_loaded_with_the_asked_id() {
    let effect = Effect::FetchNotificationGroup {
        id: "n1".into(),
        types: "all".into(),
        cursor: None,
        req: REQ,
    };
    let action = run_ok(effect, "/api/v1/notifications/n1/group", NOTIFICATIONS).await;
    assert_eq!(
        action,
        Action::GroupLoaded {
            id: "n1".into(),
            req: REQ,
            page: parse::<GroupPage>(NOTIFICATIONS),
        }
    );
}

#[tokio::test]
async fn fetch_muted_and_blocked_become_their_loaded_actions() {
    let muted = Effect::FetchMuted {
        cursor: None,
        req: REQ,
    };
    let blocked = Effect::FetchBlocked {
        cursor: None,
        req: ReqId(8),
    };
    assert_eq!(
        run_ok(muted, "/api/v1/me/muted", MUTED).await,
        Action::MutedLoaded {
            req: REQ,
            page: parse::<Page<User>>(MUTED),
        }
    );
    assert_eq!(
        run_ok(blocked, "/api/v1/me/blocked", MUTED).await,
        Action::BlockedLoaded {
            req: ReqId(8),
            page: parse::<Page<User>>(MUTED),
        }
    );
}

#[tokio::test]
async fn poll_log_becomes_new_commits_with_the_server_count() {
    let server = MockServer::start().await;
    serve(&server, "/api/v1/log/latest", json(LOG)).await;
    let effect = Effect::PollLog {
        tab: LogTab::Latest,
        since: "c0".into(),
    };
    let (results, status) = split(execute(&signed(&server), effect).await);
    assert_eq!(
        results,
        vec![Action::NewCommits {
            key: FeedKey::Log(LogTab::Latest),
            since: "c0".into(),
            count: 3,
        }]
    );
    assert_eq!(status, vec![ONLINE]);
    assert_eq!(received(&server).await[0].url.query(), Some("since=c0"));
}

#[tokio::test]
async fn poll_log_without_a_count_reports_zero() {
    let mut value: serde_json::Value = parse(LOG);
    value.as_object_mut().unwrap().remove("new_commits_count");
    let server = MockServer::start().await;
    serve(&server, "/api/v1/log/latest", json(&value.to_string())).await;
    let effect = Effect::PollLog {
        tab: LogTab::Latest,
        since: "c0".into(),
    };
    let (results, _) = split(execute(&signed(&server), effect).await);
    assert_eq!(
        results,
        vec![Action::NewCommits {
            key: FeedKey::Log(LogTab::Latest),
            since: "c0".into(),
            count: 0,
        }]
    );
}

#[tokio::test]
async fn effects_without_an_api_call_produce_no_actions_and_no_requests() {
    let server = MockServer::start().await;
    let client = signed(&server);
    for effect in [
        Effect::Quit,
        Effect::UseGuestClient,
        Effect::UseSignedClient,
        Effect::PrepareSignIn { create: false },
        Effect::OpenUrl("https://example.invalid/".into()),
        Effect::CopyToClipboard("text".into()),
        Effect::RegisterDevice {
            code: "c".into(),
            verifier: "v".into(),
        },
    ] {
        assert_eq!(execute(&client, effect).await, Vec::<Action>::new());
    }
    assert!(received(&server).await.is_empty());
}

// --- 2. failures become *Failed with the code and the req ---------------------

#[tokio::test]
async fn every_api_effect_fails_into_its_failed_action_with_the_code() {
    for (effect, p) in api_effects() {
        let server = MockServer::start().await;
        serve(&server, p, error_body(404, "commit_unavailable")).await;
        let expected = failure_for(&effect, "commit_unavailable")
            .unwrap_or_else(|| panic!("{effect:?} has no failure action"));

        let actions = execute(&signed(&server), effect.clone()).await;

        // The server's message is the one the mock sends; the rest is what this test is about.
        let mut expected = expected;
        if let Some(failure) = failure_mut(&mut expected) {
            failure.message = Some("m".into());
        }
        assert_eq!(actions[0], expected, "{effect:?}");
        assert_eq!(actions[1..], [ONLINE], "{effect:?}");
    }
}

#[tokio::test]
async fn a_failure_keeps_the_req_and_the_identity_of_the_request() {
    let server = MockServer::start().await;
    serve(
        &server,
        "/api/v1/commits/p1/replies",
        error_body(500, "boom"),
    )
    .await;
    let effect = Effect::FetchReplies {
        thread: "t1".into(),
        parent: "p1".into(),
        cursor: None,
        req: ReqId(41),
    };
    let (results, _) = split(execute(&signed(&server), effect).await);
    assert_eq!(
        results,
        vec![Action::RepliesFailed {
            thread: "t1".into(),
            parent: "p1".into(),
            req: ReqId(41),
            failure: Failure {
                message: Some("m".into()),
                ..Failure::new("boom")
            },
        }]
    );
}

#[tokio::test]
async fn an_error_without_a_code_is_reported_as_http_status() {
    let server = MockServer::start().await;
    serve(&server, "/api/v1/me", ResponseTemplate::new(503)).await;
    let (results, _) = split(execute(&signed(&server), Effect::FetchMe { req: REQ }).await);
    assert_eq!(
        results,
        vec![Action::MeFailed {
            req: REQ,
            failure: Failure {
                message: Some("Service Unavailable".into()),
                ..Failure::new("http_503")
            },
        }]
    );
}

#[tokio::test]
async fn retry_after_travels_with_the_failure() {
    let server = MockServer::start().await;
    serve(
        &server,
        "/api/v1/log/following",
        error_body(429, "rate_limited").insert_header("Retry-After", "12"),
    )
    .await;
    let effect = Effect::FetchLog {
        tab: LogTab::Following,
        cursor: None,
        req: REQ,
    };
    let (results, _) = split(execute(&signed(&server), effect).await);
    assert_eq!(
        results,
        vec![Action::FeedFailed {
            key: FeedKey::Log(LogTab::Following),
            req: REQ,
            failure: Failure {
                code: "rate_limited".into(),
                retry_after: Some(12),
                message: Some("m".into()),
            },
        }]
    );
}

#[tokio::test]
async fn a_blocked_user_failure_carries_the_user_of_the_body() {
    let user: serde_json::Value = parse::<serde_json::Value>(USER)["data"].clone();
    let body = serde_json::json!({"message": "Blocked", "code": "user_blocked", "user": user});
    let server = MockServer::start().await;
    serve(
        &server,
        "/api/v1/users/bob",
        ResponseTemplate::new(403).set_body_raw(body.to_string().into_bytes(), "application/json"),
    )
    .await;
    let effect = Effect::FetchUser {
        username: "bob".into(),
        req: REQ,
    };
    let (results, _) = split(execute(&signed(&server), effect).await);
    assert_eq!(
        results,
        vec![Action::UserFailed {
            username: "bob".into(),
            req: REQ,
            failure: Failure {
                message: Some("Blocked".into()),
                ..Failure::new("user_blocked")
            },
            blocked_user: Some(Box::new(parse::<UserResponse>(USER).data)),
        }]
    );
}

#[tokio::test]
async fn a_user_failure_with_another_code_has_no_blocked_user() {
    let server = MockServer::start().await;
    serve(
        &server,
        "/api/v1/users/bob",
        error_body(404, "user_unavailable"),
    )
    .await;
    let effect = Effect::FetchUser {
        username: "bob".into(),
        req: REQ,
    };
    let (results, _) = split(execute(&signed(&server), effect).await);
    assert_eq!(
        results,
        vec![Action::UserFailed {
            username: "bob".into(),
            req: REQ,
            failure: Failure {
                message: Some("m".into()),
                ..Failure::new("user_unavailable")
            },
            blocked_user: None,
        }]
    );
}

#[tokio::test]
async fn an_unreadable_success_body_is_a_decode_failure_and_the_server_is_online() {
    let server = MockServer::start().await;
    serve(&server, "/api/v1/me", json("not json")).await;
    let actions = execute(&signed(&server), Effect::FetchMe { req: REQ }).await;
    assert_eq!(
        actions,
        vec![
            Action::MeFailed {
                req: REQ,
                failure: Failure::new("decode"),
            },
            ONLINE
        ]
    );
}

#[tokio::test]
async fn a_poll_failure_reports_only_the_status() {
    let server = MockServer::start().await;
    serve(&server, "/api/v1/log/latest", error_body(500, "boom")).await;
    let effect = Effect::PollLog {
        tab: LogTab::Latest,
        since: "c0".into(),
    };
    assert_eq!(execute(&signed(&server), effect).await, vec![ONLINE]);
}

#[tokio::test]
async fn a_poll_that_cannot_reach_the_server_reports_offline_only() {
    let effect = Effect::PollLog {
        tab: LogTab::Latest,
        since: "c0".into(),
    };
    assert_eq!(
        execute(&unreachable_client(), effect).await,
        vec![Action::ApiStatus {
            rate: None,
            online: false
        }]
    );
}

// --- 3. ApiStatus carries the rate limit and the connection state -------------

#[tokio::test]
async fn a_success_carries_the_rate_limit_headers() {
    let server = MockServer::start().await;
    serve(
        &server,
        "/api/v1/me",
        json(ME)
            .insert_header("X-RateLimit-Limit", "120")
            .insert_header("X-RateLimit-Remaining", "118"),
    )
    .await;
    let (_, status) = split(execute(&signed(&server), Effect::FetchMe { req: REQ }).await);
    assert_eq!(
        status,
        vec![Action::ApiStatus {
            rate: Some(RateLimit {
                limit: 120,
                remaining: 118
            }),
            online: true,
        }]
    );
}

#[tokio::test]
async fn a_response_without_rate_headers_reports_no_rate_but_online() {
    let server = MockServer::start().await;
    serve(&server, "/api/v1/me", json(ME)).await;
    let (_, status) = split(execute(&signed(&server), Effect::FetchMe { req: REQ }).await);
    assert_eq!(status, vec![ONLINE]);
}

#[tokio::test]
async fn an_error_response_still_counts_as_online() {
    let server = MockServer::start().await;
    serve(&server, "/api/v1/me", error_body(401, "invalid_signature")).await;
    let (_, status) = split(execute(&signed(&server), Effect::FetchMe { req: REQ }).await);
    assert_eq!(status, vec![ONLINE]);
}

#[tokio::test]
async fn a_refused_connection_is_a_network_failure_and_offline() {
    let actions = execute(&unreachable_client(), Effect::FetchMe { req: REQ }).await;
    assert_eq!(
        actions,
        vec![
            Action::MeFailed {
                req: REQ,
                failure: Failure::new("network"),
            },
            Action::ApiStatus {
                rate: None,
                online: false
            }
        ]
    );
}

#[tokio::test]
async fn a_response_slower_than_the_client_timeout_is_a_network_failure() {
    let server = MockServer::start().await;
    serve(
        &server,
        "/api/v1/me",
        json(ME).set_delay(Duration::from_secs(1)),
    )
    .await;
    let client = signed(&server).with_timeout(Duration::from_millis(100));
    let started = std::time::Instant::now();

    let actions = execute(&client, Effect::FetchMe { req: REQ }).await;

    assert_eq!(
        actions,
        vec![
            Action::MeFailed {
                req: REQ,
                failure: Failure::new("network"),
            },
            Action::ApiStatus {
                rate: None,
                online: false
            }
        ]
    );
    assert!(started.elapsed() < Duration::from_millis(900));
}

#[tokio::test]
async fn a_guest_calling_a_signed_endpoint_fails_locally_without_changing_the_status() {
    let server = MockServer::start().await;
    let guest = client_for(&server.uri(), false);
    let actions = execute(&guest, Effect::FetchMe { req: REQ }).await;
    assert_eq!(
        actions,
        vec![Action::MeFailed {
            req: REQ,
            failure: Failure::new("sign_in_required"),
        }]
    );
    assert!(received(&server).await.is_empty());
}

#[tokio::test]
async fn an_unsafe_argument_fails_locally_without_changing_the_status() {
    let server = MockServer::start().await;
    let effect = Effect::FetchThread {
        id: "a/b".into(),
        req: REQ,
    };
    let actions = execute(&signed(&server), effect).await;
    assert_eq!(
        actions,
        vec![Action::ThreadFailed {
            id: "a/b".into(),
            req: REQ,
            failure: Failure::new("invalid_argument"),
        }]
    );
    assert!(received(&server).await.is_empty());
}

// --- 4. failure_for ----------------------------------------------------------

#[test]
fn failure_for_builds_the_failed_action_of_each_effect() {
    let f = || Failure::new("panic");
    let cases: Vec<(Effect, Action)> = vec![
        (
            Effect::FetchLog {
                tab: LogTab::Latest,
                cursor: Some("x".into()),
                req: REQ,
            },
            Action::FeedFailed {
                key: FeedKey::Log(LogTab::Latest),
                req: REQ,
                failure: f(),
            },
        ),
        (
            Effect::FetchSaved {
                cursor: None,
                req: REQ,
            },
            Action::FeedFailed {
                key: FeedKey::Saved,
                req: REQ,
                failure: f(),
            },
        ),
        (
            Effect::FetchThread {
                id: "c1".into(),
                req: REQ,
            },
            Action::ThreadFailed {
                id: "c1".into(),
                req: REQ,
                failure: f(),
            },
        ),
        (
            Effect::FetchReplies {
                thread: "t".into(),
                parent: "p".into(),
                cursor: None,
                req: REQ,
            },
            Action::RepliesFailed {
                thread: "t".into(),
                parent: "p".into(),
                req: REQ,
                failure: f(),
            },
        ),
        (
            Effect::FetchUser {
                username: "alice".into(),
                req: REQ,
            },
            Action::UserFailed {
                username: "alice".into(),
                req: REQ,
                failure: f(),
                blocked_user: None,
            },
        ),
        (
            Effect::FetchUserCommits {
                username: "alice".into(),
                replies: true,
                cursor: None,
                req: REQ,
            },
            Action::ProfileFeedFailed {
                username: "alice".into(),
                tab: ProfileTab::Replies,
                req: REQ,
                failure: f(),
            },
        ),
        (
            Effect::FetchConnections {
                username: "alice".into(),
                tab: ConnTab::Following,
                cursor: None,
                req: REQ,
            },
            Action::ConnectionsFailed {
                username: "alice".into(),
                tab: ConnTab::Following,
                req: REQ,
                failure: f(),
            },
        ),
        (
            Effect::FetchMe { req: REQ },
            Action::MeFailed {
                req: REQ,
                failure: f(),
            },
        ),
        (
            Effect::FetchNotifications {
                types: "all".into(),
                cursor: None,
                req: REQ,
            },
            Action::NotificationsFailed {
                req: REQ,
                failure: f(),
            },
        ),
        (
            Effect::FetchNotificationGroup {
                id: "n1".into(),
                types: "all".into(),
                cursor: None,
                req: REQ,
            },
            Action::GroupFailed {
                id: "n1".into(),
                req: REQ,
                failure: f(),
            },
        ),
        (
            Effect::FetchMuted {
                cursor: None,
                req: REQ,
            },
            Action::MutedFailed {
                req: REQ,
                failure: f(),
            },
        ),
        (
            Effect::FetchBlocked {
                cursor: None,
                req: REQ,
            },
            Action::BlockedFailed {
                req: REQ,
                failure: f(),
            },
        ),
    ];
    for (effect, expected) in cases {
        assert_eq!(failure_for(&effect, "panic"), Some(expected), "{effect:?}");
    }
}

#[test]
fn failure_for_has_no_action_for_effects_nobody_waits_on() {
    let poll = Effect::PollLog {
        tab: LogTab::Latest,
        since: "c0".into(),
    };
    for effect in [
        poll,
        Effect::Quit,
        Effect::UseGuestClient,
        Effect::SaveDrafts {
            account: "a".into(),
            drafts: crate::drafts::Drafts::default(),
        },
        Effect::LoadDrafts {
            account: "a".into(),
        },
        Effect::UseSignedClient,
    ] {
        assert_eq!(failure_for(&effect, "panic"), None, "{effect:?}");
    }
}

// --- query parameters reach the server ------------------------------------------------------

#[tokio::test]
async fn cursor_and_types_are_sent_as_query_parameters() {
    let c = || Some("cur".to_string());
    type Case<'a> = (Effect, &'a str, &'a str, Vec<(&'a str, &'a str)>);
    let cases: Vec<Case> = vec![
        (
            Effect::FetchNotifications {
                types: "replies".into(),
                cursor: c(),
                req: REQ,
            },
            "/api/v1/notifications",
            NOTIFICATIONS,
            vec![("cursor", "cur"), ("type", "replies")],
        ),
        (
            Effect::FetchNotificationGroup {
                id: "n1".into(),
                types: "none".into(),
                cursor: c(),
                req: REQ,
            },
            "/api/v1/notifications/n1/group",
            NOTIFICATIONS,
            vec![("cursor", "cur"), ("type", "none")],
        ),
        (
            Effect::FetchConnections {
                username: "alice".into(),
                tab: ConnTab::Followers,
                cursor: c(),
                req: REQ,
            },
            "/api/v1/users/alice/followers",
            FOLLOWERS,
            vec![("cursor", "cur")],
        ),
        (
            Effect::FetchReplies {
                thread: "t".into(),
                parent: "p".into(),
                cursor: c(),
                req: REQ,
            },
            "/api/v1/commits/p/replies",
            REPLIES,
            vec![("cursor", "cur")],
        ),
        (
            Effect::FetchSaved {
                cursor: c(),
                req: REQ,
            },
            "/api/v1/saved",
            COMMITS,
            vec![("cursor", "cur")],
        ),
        (
            Effect::FetchMuted {
                cursor: c(),
                req: REQ,
            },
            "/api/v1/me/muted",
            MUTED,
            vec![("cursor", "cur")],
        ),
        (
            Effect::FetchBlocked {
                cursor: c(),
                req: REQ,
            },
            "/api/v1/me/blocked",
            MUTED,
            vec![("cursor", "cur")],
        ),
        (
            Effect::FetchUserCommits {
                username: "alice".into(),
                replies: false,
                cursor: c(),
                req: REQ,
            },
            "/api/v1/users/alice/commits",
            COMMITS,
            vec![("cursor", "cur")],
        ),
    ];
    for (effect, p, body, expected) in cases {
        let server = MockServer::start().await;
        serve(&server, p, json(body)).await;
        execute(&signed(&server), effect.clone()).await;
        let requests = received(&server).await;
        assert_eq!(requests.len(), 1, "{effect:?}");
        let mut got: Vec<(String, String)> = requests[0]
            .url
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        got.sort();
        let expected: Vec<(String, String)> = expected
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        assert_eq!(got, expected, "{effect:?}");
    }
}

// --- 5. posting a commit -----------------------------------------------------------

const POSTED: &str = include_str!("../../tests/fixtures/commit.json");

fn posted_commit() -> crate::model::Commit {
    serde_json::from_value(parse::<serde_json::Value>(POSTED)["data"].clone())
        .expect("fixture commit parses")
}

async fn serve_post(server: &MockServer, response: ResponseTemplate) {
    Mock::given(method("POST"))
        .and(path("/api/v1/commits"))
        .respond_with(response)
        .mount(server)
        .await;
}

fn create(body: &str, parent: Option<&str>, quoted: Option<&str>) -> Effect {
    Effect::CreateCommit {
        body: body.into(),
        parent_id: parent.map(String::from),
        quoted_commit_id: quoted.map(String::from),
        req: REQ,
    }
}

fn sent_json(request: &Request) -> serde_json::Value {
    serde_json::from_slice(&request.body).expect("the request body is JSON")
}

#[tokio::test]
async fn creating_a_commit_posts_the_body_and_answers_with_the_commit() {
    let server = MockServer::start().await;
    serve_post(
        &server,
        ResponseTemplate::new(201).set_body_raw(POSTED.as_bytes().to_vec(), "application/json"),
    )
    .await;

    let (results, status) = split(execute(&signed(&server), create("hello", None, None)).await);

    assert_eq!(
        results,
        vec![Action::CommitCreated {
            req: REQ,
            commit: Box::new(posted_commit()),
        }]
    );
    assert_eq!(status, vec![ONLINE]);
    let requests = received(&server).await;
    assert_eq!(requests.len(), 1);
    assert_eq!(
        sent_json(&requests[0]),
        serde_json::json!({"body": "hello"})
    );
}

#[tokio::test]
async fn a_reply_posts_the_parent_id_and_a_quote_the_quoted_id() {
    let server = MockServer::start().await;
    serve_post(
        &server,
        ResponseTemplate::new(201).set_body_raw(POSTED.as_bytes().to_vec(), "application/json"),
    )
    .await;
    let client = signed(&server);

    execute(&client, create("re", Some("p-1"), None)).await;
    execute(&client, create("qt", None, Some("q-1"))).await;

    let requests = received(&server).await;
    assert_eq!(requests.len(), 2);
    assert_eq!(
        sent_json(&requests[0]),
        serde_json::json!({"body": "re", "parent_id": "p-1"})
    );
    assert_eq!(
        sent_json(&requests[1]),
        serde_json::json!({"body": "qt", "quoted_commit_id": "q-1"})
    );
}

#[tokio::test]
async fn a_refused_commit_carries_the_code_and_the_message_of_the_server() {
    let server = MockServer::start().await;
    serve_post(
        &server,
        ResponseTemplate::new(422).set_body_raw(
            br#"{"message":"The body field is required.","code":"validation_failed"}"#.to_vec(),
            "application/json",
        ),
    )
    .await;

    let (results, status) = split(execute(&signed(&server), create("x", None, None)).await);

    assert_eq!(
        results,
        vec![Action::CommitFailed {
            req: REQ,
            failure: Failure {
                code: "validation_failed".into(),
                retry_after: None,
                message: Some("The body field is required.".into()),
            },
        }]
    );
    assert_eq!(status, vec![ONLINE]);
}

#[tokio::test]
async fn a_rate_limited_commit_carries_retry_after_and_the_message() {
    let server = MockServer::start().await;
    serve_post(
        &server,
        ResponseTemplate::new(429)
            .set_body_raw(
                br#"{"message":"Too Many Attempts.","code":"rate_limited"}"#.to_vec(),
                "application/json",
            )
            .insert_header("Retry-After", "12"),
    )
    .await;

    let (results, _) = split(execute(&signed(&server), create("x", None, None)).await);

    assert_eq!(
        results,
        vec![Action::CommitFailed {
            req: REQ,
            failure: Failure {
                code: "rate_limited".into(),
                retry_after: Some(12),
                message: Some("Too Many Attempts.".into()),
            },
        }]
    );
}

#[tokio::test]
async fn a_commit_that_cannot_reach_the_server_fails_as_network_with_no_message() {
    let actions = execute(&unreachable_client(), create("x", None, None)).await;

    assert_eq!(
        actions,
        vec![
            Action::CommitFailed {
                req: REQ,
                failure: Failure::new("network"),
            },
            Action::ApiStatus {
                rate: None,
                online: false
            }
        ]
    );
}

#[tokio::test]
async fn a_commit_that_times_out_fails_as_network_although_the_server_got_it() {
    let server = MockServer::start().await;
    serve_post(
        &server,
        ResponseTemplate::new(201)
            .set_body_raw(POSTED.as_bytes().to_vec(), "application/json")
            .set_delay(Duration::from_secs(1)),
    )
    .await;
    let client = signed(&server).with_timeout(Duration::from_millis(100));

    let actions = execute(&client, create("x", None, None)).await;

    assert_eq!(
        actions,
        vec![
            Action::CommitFailed {
                req: REQ,
                failure: Failure::new("network"),
            },
            Action::ApiStatus {
                rate: None,
                online: false
            }
        ]
    );
    assert_eq!(received(&server).await.len(), 1);
}

#[tokio::test]
async fn a_guest_cannot_post_and_nothing_is_sent() {
    let server = MockServer::start().await;
    let guest = client_for(&server.uri(), false);

    let actions = execute(&guest, create("x", None, None)).await;

    assert_eq!(
        actions,
        vec![Action::CommitFailed {
            req: REQ,
            failure: Failure::new("sign_in_required"),
        }]
    );
    assert!(received(&server).await.is_empty());
}

#[tokio::test]
async fn a_commit_that_is_both_a_reply_and_a_quote_fails_locally() {
    let server = MockServer::start().await;

    let actions = execute(&signed(&server), create("x", Some("p"), Some("q"))).await;

    assert_eq!(
        actions,
        vec![Action::CommitFailed {
            req: REQ,
            failure: Failure::new("invalid_argument"),
        }]
    );
    assert!(received(&server).await.is_empty());
}

#[test]
fn a_panicked_post_fails_into_commit_failed_with_the_req() {
    assert_eq!(
        failure_for(&create("x", None, None), "panic"),
        Some(Action::CommitFailed {
            req: REQ,
            failure: Failure::new("panic"),
        })
    );
}

#[tokio::test]
async fn saving_and_loading_drafts_are_not_api_calls() {
    let server = MockServer::start().await;
    let client = signed(&server);

    for effect in [
        Effect::SaveDrafts {
            account: "a".into(),
            drafts: crate::drafts::Drafts::default(),
        },
        Effect::LoadDrafts {
            account: "a".into(),
        },
    ] {
        assert_eq!(execute(&client, effect).await, Vec::<Action>::new());
    }
    assert!(received(&server).await.is_empty());
}
