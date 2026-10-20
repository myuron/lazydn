use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::de::DeserializeOwned;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

use super::client::{ApiClient, RateLimit};
use super::clock::FixedClock;
use super::error::ApiError;
use super::sign::Signer;
use super::test_support::*;

const NOW: i64 = 1_700_000_000;

fn signed(server: &MockServer) -> ApiClient {
    client_for(&server.uri(), Some(Arc::new(TestSigner::golden())))
}

fn guest(server: &MockServer) -> ApiClient {
    client_for(&server.uri(), None)
}

fn client_for(base: &str, signer: Option<Arc<dyn Signer>>) -> ApiClient {
    ApiClient::new(base, signer, Arc::new(FixedClock(NOW))).expect("client builds")
}

fn json(body: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_raw(body.as_bytes().to_vec(), "application/json")
}

fn parse<T: DeserializeOwned>(body: &str) -> T {
    serde_json::from_str(body).expect("fixture parses")
}

async fn serve(server: &MockServer, verb: &str, p: &str, response: ResponseTemplate) {
    Mock::given(method(verb))
        .and(path(p))
        .respond_with(response)
        .mount(server)
        .await;
}

async fn received(server: &MockServer) -> Vec<Request> {
    server.received_requests().await.expect("recording is on")
}

async fn only_request(server: &MockServer) -> Request {
    let mut all = received(server).await;
    assert_eq!(all.len(), 1, "expected exactly one request");
    all.remove(0)
}

fn query(req: &Request) -> BTreeMap<String, String> {
    req.url
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect()
}

fn expect_query(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn code_of<T: std::fmt::Debug>(r: Result<super::client::Response<T>, ApiError>) -> String {
    match r {
        Ok(v) => format!("unexpected success: {v:?}"),
        Err(e) => e.code(),
    }
}

const ME: &str = include_str!("../../tests/fixtures/me.json");
const LOG: &str = include_str!("../../tests/fixtures/log_following.json");
const THREAD: &str = include_str!("../../tests/fixtures/thread.json");
const REPLIES: &str = include_str!("../../tests/fixtures/replies_with_tombstone.json");
const USER: &str = include_str!("../../tests/fixtures/user.json");
const COMMITS: &str = include_str!("../../tests/fixtures/commits.json");
const FOLLOWERS: &str = include_str!("../../tests/fixtures/followers.json");
const NOTIFICATIONS: &str = include_str!("../../tests/fixtures/notifications.json");
const MUTED: &str = include_str!("../../tests/fixtures/muted.json");

// ---- signing mode ----

#[tokio::test]
async fn guest_requests_carry_no_signature_header_at_all() {
    let server = MockServer::start().await;
    serve(&server, "GET", "/api/v1/users/alice", json(USER)).await;

    let result = guest(&server).user("alice").await;

    assert!(result.is_ok(), "{result:?}");
    let req = only_request(&server).await;
    for name in ["x-devnull-timestamp", "x-devnull-nonce", "x-ssh-signature"] {
        assert!(req.headers.get(name).is_none(), "{name} must be absent");
    }
}

#[tokio::test]
async fn guest_reaches_the_optional_signature_endpoints() {
    let server = MockServer::start().await;
    serve(&server, "GET", "/api/v1/commits/c1", json(THREAD)).await;
    serve(&server, "GET", "/api/v1/users/alice/commits", json(COMMITS)).await;
    serve(
        &server,
        "GET",
        "/api/v1/users/alice/followers",
        json(FOLLOWERS),
    )
    .await;
    serve(
        &server,
        "GET",
        "/api/v1/users/alice/following",
        json(FOLLOWERS),
    )
    .await;
    let client = guest(&server);

    assert!(client.commit("c1").await.is_ok());
    assert!(client.user_commits("alice", false, None).await.is_ok());
    assert!(client.followers("alice", None).await.is_ok());
    assert!(client.following("alice", None).await.is_ok());
    assert_eq!(received(&server).await.len(), 4);
    for req in received(&server).await {
        for name in ["x-devnull-timestamp", "x-devnull-nonce", "x-ssh-signature"] {
            assert!(
                req.headers.get(name).is_none(),
                "{} carries {name}",
                req.url
            );
        }
    }
}

#[tokio::test]
async fn guest_calling_a_signature_required_endpoint_fails_locally() {
    let server = MockServer::start().await;
    let client = guest(&server);

    let codes = vec![
        code_of(client.me().await),
        code_of(client.log("latest", None, None).await),
        code_of(client.replies("c1", None).await),
        code_of(client.notifications("all", None).await),
        code_of(client.notification_group("n1", "all", None).await),
        code_of(client.saved(None).await),
        code_of(client.muted(None).await),
        code_of(client.blocked(None).await),
        code_of(client.register_device("code", "verifier").await),
    ];

    assert_eq!(codes, vec!["sign_in_required"; 9]);
    assert!(
        received(&server).await.is_empty(),
        "nothing may reach the server"
    );
}

#[tokio::test]
async fn signed_requests_verify_against_the_request_the_server_received() {
    let server = MockServer::start().await;
    serve(&server, "GET", "/api/v1/me", json(ME)).await;

    let result = signed(&server).me().await;

    assert!(result.is_ok(), "{result:?}");
    let req = only_request(&server).await;
    assert_eq!(
        req.headers.get("x-devnull-timestamp").unwrap(),
        "1700000000"
    );
    assert_eq!(verify_received(&req), Ok(()));
}

#[tokio::test]
async fn signature_header_is_one_line_and_parses_like_the_server_does() {
    let server = MockServer::start().await;
    serve(&server, "GET", "/api/v1/me", json(ME)).await;

    let _ = signed(&server).me().await;

    let req = only_request(&server).await;
    let header = req
        .headers
        .get("x-ssh-signature")
        .expect("header present")
        .to_str()
        .unwrap();
    assert!(!header.contains('\r') && !header.contains('\n'));
    assert!(parse_signature_header(header).is_ok());
}

#[tokio::test]
async fn consecutive_requests_use_different_nonces() {
    let server = MockServer::start().await;
    serve(&server, "GET", "/api/v1/me", json(ME)).await;
    let client = signed(&server);

    let _ = client.me().await;
    let _ = client.me().await;

    let nonces: Vec<_> = received(&server)
        .await
        .iter()
        .map(|r| {
            r.headers
                .get("x-devnull-nonce")
                .map(|v| v.to_str().unwrap().to_string())
        })
        .collect();
    assert_eq!(nonces.len(), 2);
    assert!(nonces[0].is_some() && nonces[1].is_some());
    assert_ne!(nonces[0], nonces[1]);
}

#[tokio::test]
async fn query_with_reserved_characters_is_signed_as_sent() {
    let server = MockServer::start().await;
    serve(&server, "GET", "/api/v1/saved", json(COMMITS)).await;

    let result = signed(&server).saved(Some("a+b=c/d e")).await;

    assert!(result.is_ok(), "{result:?}");
    let req = only_request(&server).await;
    assert_eq!(query(&req), expect_query(&[("cursor", "a+b=c/d e")]));
    assert_eq!(verify_received(&req), Ok(()));
}

#[tokio::test]
async fn comma_list_of_types_is_signed_as_sent() {
    let server = MockServer::start().await;
    serve(&server, "GET", "/api/v1/notifications", json(NOTIFICATIONS)).await;

    let result = signed(&server).notifications("replies,quotes", None).await;

    assert!(result.is_ok(), "{result:?}");
    let req = only_request(&server).await;
    assert_eq!(query(&req), expect_query(&[("type", "replies,quotes")]));
    assert_eq!(verify_received(&req), Ok(()));
}

#[tokio::test]
async fn trailing_slash_of_the_base_url_does_not_double_the_path() {
    let server = MockServer::start().await;
    serve(&server, "GET", "/api/v1/me", json(ME)).await;
    let client = client_for(
        &format!("{}/", server.uri()),
        Some(Arc::new(TestSigner::golden())),
    );

    let result = client.me().await;

    assert!(result.is_ok(), "{result:?}");
    let req = only_request(&server).await;
    assert_eq!(req.url.path(), "/api/v1/me");
    assert_eq!(verify_received(&req), Ok(()));
}

#[tokio::test]
async fn requests_identify_the_client_and_ask_for_json() {
    let server = MockServer::start().await;
    serve(&server, "GET", "/api/v1/me", json(ME)).await;

    let _ = signed(&server).me().await;

    let req = only_request(&server).await;
    let ua = format!("lazydn/{}", env!("CARGO_PKG_VERSION"));
    assert_eq!(req.headers.get("user-agent").unwrap().to_str().unwrap(), ua);
    assert_eq!(
        req.headers.get("accept").map(|v| v.to_str().unwrap()),
        Some("application/json")
    );
}

// ---- read endpoints ----

#[tokio::test]
async fn me_returns_the_current_user() {
    let server = MockServer::start().await;
    serve(&server, "GET", "/api/v1/me", json(ME)).await;

    let result = signed(&server).me().await;

    assert_eq!(result.map(|r| r.data).ok(), Some(parse(ME)));
    assert!(query(&only_request(&server).await).is_empty());
}

#[tokio::test]
async fn log_sends_the_name_since_and_cursor() {
    let server = MockServer::start().await;
    serve(&server, "GET", "/api/v1/log/following", json(LOG)).await;

    let result = signed(&server)
        .log(
            "following",
            Some("01a15626-e25e-705d-8d77-8f69ebbaf4c9"),
            Some("cur"),
        )
        .await;

    assert_eq!(result.map(|r| r.data).ok(), Some(parse(LOG)));
    let req = only_request(&server).await;
    assert_eq!(
        query(&req),
        expect_query(&[
            ("since", "01a15626-e25e-705d-8d77-8f69ebbaf4c9"),
            ("cursor", "cur")
        ])
    );
    assert_eq!(verify_received(&req), Ok(()));
}

#[tokio::test]
async fn log_without_since_or_cursor_has_no_query() {
    let server = MockServer::start().await;
    serve(&server, "GET", "/api/v1/log/latest", json(LOG)).await;

    let result = signed(&server).log("latest", None, None).await;

    assert!(result.is_ok(), "{result:?}");
    assert!(query(&only_request(&server).await).is_empty());
}

#[tokio::test]
async fn commit_returns_the_thread_response() {
    let server = MockServer::start().await;
    serve(&server, "GET", "/api/v1/commits/abc-123", json(THREAD)).await;

    let result = signed(&server).commit("abc-123").await;

    assert_eq!(result.map(|r| r.data).ok(), Some(parse(THREAD)));
}

#[tokio::test]
async fn replies_returns_a_page_that_may_hold_tombstones() {
    let server = MockServer::start().await;
    serve(
        &server,
        "GET",
        "/api/v1/commits/abc-123/replies",
        json(REPLIES),
    )
    .await;

    let result = signed(&server).replies("abc-123", Some("cur")).await;

    assert_eq!(result.map(|r| r.data).ok(), Some(parse(REPLIES)));
    assert_eq!(
        query(&only_request(&server).await),
        expect_query(&[("cursor", "cur")])
    );
}

#[tokio::test]
async fn user_returns_the_profile_response() {
    let server = MockServer::start().await;
    serve(&server, "GET", "/api/v1/users/alice", json(USER)).await;

    let result = signed(&server).user("alice").await;

    assert_eq!(result.map(|r| r.data).ok(), Some(parse(USER)));
}

#[tokio::test]
async fn user_commits_asks_for_replies_only_when_wanted() {
    let server = MockServer::start().await;
    serve(&server, "GET", "/api/v1/users/alice/commits", json(COMMITS)).await;
    let client = signed(&server);

    let posts = client.user_commits("alice", false, None).await;
    let with_replies = client.user_commits("alice", true, Some("cur")).await;

    assert_eq!(posts.map(|r| r.data).ok(), Some(parse(COMMITS)));
    assert!(with_replies.is_ok());
    let reqs = received(&server).await;
    assert_eq!(query(&reqs[0]), expect_query(&[]));
    assert_eq!(
        query(&reqs[1]),
        expect_query(&[("replies", "true"), ("cursor", "cur")])
    );
}

#[tokio::test]
async fn followers_and_following_use_their_own_paths() {
    let server = MockServer::start().await;
    serve(
        &server,
        "GET",
        "/api/v1/users/alice/followers",
        json(FOLLOWERS),
    )
    .await;
    serve(
        &server,
        "GET",
        "/api/v1/users/alice/following",
        json(FOLLOWERS),
    )
    .await;
    let client = signed(&server);

    let a = client.followers("alice", Some("c1")).await;
    let b = client.following("alice", None).await;

    assert_eq!(a.map(|r| r.data).ok(), Some(parse(FOLLOWERS)));
    assert_eq!(b.map(|r| r.data).ok(), Some(parse(FOLLOWERS)));
    let reqs = received(&server).await;
    assert_eq!(query(&reqs[0]), expect_query(&[("cursor", "c1")]));
    assert_eq!(query(&reqs[1]), expect_query(&[]));
}

#[tokio::test]
async fn notifications_send_the_type_filter_and_cursor() {
    let server = MockServer::start().await;
    serve(&server, "GET", "/api/v1/notifications", json(NOTIFICATIONS)).await;

    let result = signed(&server).notifications("none", Some("cur")).await;

    let page = result.expect("ok").data;
    assert_eq!(page, parse(NOTIFICATIONS));
    assert_eq!(page.data.len(), 2);
    assert_eq!(
        query(&only_request(&server).await),
        expect_query(&[("type", "none"), ("cursor", "cur")])
    );
}

#[tokio::test]
async fn notification_group_uses_the_group_path() {
    let server = MockServer::start().await;
    serve(
        &server,
        "GET",
        "/api/v1/notifications/n-1/group",
        json(NOTIFICATIONS),
    )
    .await;

    let result = signed(&server)
        .notification_group("n-1", "replies", Some("cur"))
        .await;

    assert_eq!(result.map(|r| r.data).ok(), Some(parse(NOTIFICATIONS)));
    assert_eq!(
        query(&only_request(&server).await),
        expect_query(&[("type", "replies"), ("cursor", "cur")])
    );
}

#[tokio::test]
async fn saved_muted_and_blocked_are_cursor_pages() {
    let server = MockServer::start().await;
    serve(&server, "GET", "/api/v1/saved", json(COMMITS)).await;
    serve(&server, "GET", "/api/v1/me/muted", json(MUTED)).await;
    serve(&server, "GET", "/api/v1/me/blocked", json(MUTED)).await;
    let client = signed(&server);

    let saved = client.saved(None).await;
    let muted = client.muted(Some("m1")).await;
    let blocked = client.blocked(None).await;

    assert_eq!(saved.map(|r| r.data).ok(), Some(parse(COMMITS)));
    let muted = muted.expect("muted ok").data;
    assert_eq!(muted, parse(MUTED));
    assert_eq!(muted.meta.next_cursor.as_deref(), Some("next-m"));
    assert!(blocked.is_ok());
    let reqs = received(&server).await;
    assert_eq!(query(&reqs[1]), expect_query(&[("cursor", "m1")]));
}

// ---- register_device ----

#[tokio::test]
async fn register_device_posts_the_code_and_verifier_as_signed_json() {
    let server = MockServer::start().await;
    serve(&server, "POST", "/api/v1/devices", json(ME)).await;

    let result = signed(&server).register_device("abc", "xyz").await;

    assert_eq!(result.map(|r| r.data).ok(), Some(parse(ME)));
    let req = only_request(&server).await;
    let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
    assert_eq!(
        body,
        serde_json::json!({"code": "abc", "code_verifier": "xyz"})
    );
    assert!(
        req.headers
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.starts_with("application/json"))
    );
    assert_eq!(verify_received(&req), Ok(()));
}

#[tokio::test]
async fn register_device_accepts_a_201_response() {
    let server = MockServer::start().await;
    let created =
        ResponseTemplate::new(201).set_body_raw(ME.as_bytes().to_vec(), "application/json");
    serve(&server, "POST", "/api/v1/devices", created).await;

    let result = signed(&server).register_device("abc", "xyz").await;

    assert_eq!(result.map(|r| r.data).ok(), Some(parse(ME)));
}

// ---- rate limit ----

#[tokio::test]
async fn rate_limit_headers_are_read_when_present() {
    let server = MockServer::start().await;
    let with_rate = json(ME)
        .insert_header("X-RateLimit-Limit", "60")
        .insert_header("X-RateLimit-Remaining", "59");
    serve(&server, "GET", "/api/v1/me", with_rate).await;

    let result = signed(&server).me().await;

    assert_eq!(
        result.map(|r| r.rate).ok(),
        Some(Some(RateLimit {
            limit: 60,
            remaining: 59
        }))
    );
}

#[tokio::test]
async fn rate_limit_is_none_when_the_headers_are_missing() {
    let server = MockServer::start().await;
    serve(&server, "GET", "/api/v1/me", json(ME)).await;

    let result = signed(&server).me().await;

    assert!(result.is_ok(), "{result:?}");
    assert_eq!(result.unwrap().rate, None);
}

// ---- errors ----

fn error_response(status: u16, body: &str) -> ResponseTemplate {
    ResponseTemplate::new(status).set_body_raw(body.as_bytes().to_vec(), "application/json")
}

async fn me_error(response: ResponseTemplate) -> ApiError {
    let server = MockServer::start().await;
    serve(&server, "GET", "/api/v1/me", response).await;
    signed(&server)
        .me()
        .await
        .expect_err("an error status fails")
}

#[tokio::test]
async fn api_error_code_is_passed_through() {
    let err = me_error(error_response(
        401,
        r#"{"message":"Signature rejected","code":"invalid_signature"}"#,
    ))
    .await;

    assert_eq!(err.code(), "invalid_signature");
    assert!(matches!(
        err.kind,
        super::error::ErrorKind::Http { status: 401, ref message, .. } if message == "Signature rejected"
    ));
}

#[tokio::test]
async fn device_not_approved_keeps_its_code() {
    let err = me_error(error_response(
        401,
        r#"{"message":"Device is not approved","code":"device_not_approved"}"#,
    ))
    .await;

    assert_eq!(err.code(), "device_not_approved");
}

#[tokio::test]
async fn user_blocked_ignores_the_extra_user_field() {
    let err = me_error(error_response(
        403,
        r#"{"message":"Blocked","code":"user_blocked","user":{"id":"u","username":"x"}}"#,
    ))
    .await;

    assert_eq!(err.code(), "user_blocked");
}

#[tokio::test]
async fn service_unavailable_with_a_code_keeps_the_code() {
    let err = me_error(error_response(
        503,
        r#"{"message":"GitHub is down","code":"github_sync_failed"}"#,
    ))
    .await;

    assert_eq!(err.code(), "github_sync_failed");
}

#[tokio::test]
async fn missing_code_falls_back_to_the_http_status() {
    let err = me_error(error_response(404, r#"{"message":"Not Found"}"#)).await;

    assert_eq!(err.code(), "http_404");
}

#[tokio::test]
async fn a_non_json_error_body_still_maps_to_the_http_status() {
    let err = me_error(
        ResponseTemplate::new(502).set_body_raw(b"<html>Bad Gateway</html>".to_vec(), "text/html"),
    )
    .await;

    assert_eq!(err.code(), "http_502");
}

#[tokio::test]
async fn validation_errors_become_http_422_with_the_message() {
    let err = me_error(error_response(
        422,
        r#"{"message":"The body field is required.","errors":{"body":["The body field is required."]}}"#,
    ))
    .await;

    assert_eq!(err.code(), "http_422");
    assert!(matches!(
        err.kind,
        super::error::ErrorKind::Http { ref message, .. } if message == "The body field is required."
    ));
}

#[tokio::test]
async fn rate_limited_reads_retry_after_in_seconds() {
    let err = me_error(
        error_response(429, r#"{"message":"Slow down","code":"rate_limited"}"#)
            .insert_header("Retry-After", "12"),
    )
    .await;

    assert_eq!(err.code(), "rate_limited");
    assert!(matches!(
        err.kind,
        super::error::ErrorKind::Http {
            retry_after: Some(12),
            ..
        }
    ));
}

#[tokio::test]
async fn rate_limited_without_retry_after_has_none() {
    let err = me_error(error_response(429, r#"{"message":"Slow down"}"#)).await;

    assert_eq!(err.code(), "http_429");
    assert!(matches!(
        err.kind,
        super::error::ErrorKind::Http {
            retry_after: None,
            ..
        }
    ));
}

#[tokio::test]
async fn a_success_status_with_a_non_json_body_is_a_decode_error() {
    let server = MockServer::start().await;
    serve(
        &server,
        "GET",
        "/api/v1/me",
        ResponseTemplate::new(200).set_body_raw(b"not json".to_vec(), "text/plain"),
    )
    .await;

    let result = signed(&server).me().await;

    assert_eq!(code_of(result), "decode");
}

#[tokio::test]
async fn a_success_status_with_the_wrong_shape_is_a_decode_error() {
    let server = MockServer::start().await;
    serve(&server, "GET", "/api/v1/me", json(r#"{"unexpected":true}"#)).await;

    let result = signed(&server).me().await;

    assert_eq!(code_of(result), "decode");
}

#[tokio::test]
async fn a_closed_port_is_a_network_error() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let client = client_for(
        &format!("http://{addr}"),
        Some(Arc::new(TestSigner::golden())),
    );

    let result = client.me().await;

    assert_eq!(code_of(result), "network");
}

#[tokio::test]
async fn a_slow_response_times_out_as_a_network_error() {
    let server = MockServer::start().await;
    serve(
        &server,
        "GET",
        "/api/v1/me",
        json(ME).set_delay(Duration::from_secs(1)),
    )
    .await;
    let client = signed(&server).with_timeout(Duration::from_millis(100));
    let started = Instant::now();

    let result = client.me().await;

    assert_eq!(code_of(result), "network");
    assert!(started.elapsed() < Duration::from_millis(900));
}

#[tokio::test]
async fn failures_are_not_retried() {
    let server = MockServer::start().await;
    serve(
        &server,
        "GET",
        "/api/v1/me",
        error_response(503, r#"{"message":"down"}"#),
    )
    .await;

    let result = signed(&server).me().await;

    assert_eq!(code_of(result), "http_503");
    assert_eq!(received(&server).await.len(), 1);
}

#[tokio::test]
async fn redirects_are_not_followed() {
    let server = MockServer::start().await;
    serve(
        &server,
        "GET",
        "/api/v1/me",
        ResponseTemplate::new(302).insert_header("Location", "/elsewhere"),
    )
    .await;
    serve(&server, "GET", "/elsewhere", json(ME)).await;

    let result = signed(&server).me().await;

    assert_eq!(code_of(result), "http_302");
    assert_eq!(received(&server).await.len(), 1);
}

#[tokio::test]
async fn signer_present_means_even_the_optional_endpoints_are_signed() {
    let server = MockServer::start().await;
    serve(&server, "GET", "/api/v1/users/alice", json(USER)).await;
    serve(&server, "GET", "/api/v1/commits/c1", json(THREAD)).await;
    serve(&server, "GET", "/api/v1/users/alice/commits", json(COMMITS)).await;
    serve(
        &server,
        "GET",
        "/api/v1/users/alice/followers",
        json(FOLLOWERS),
    )
    .await;
    serve(
        &server,
        "GET",
        "/api/v1/users/alice/following",
        json(FOLLOWERS),
    )
    .await;
    let client = signed(&server);

    assert!(client.user("alice").await.is_ok());
    assert!(client.commit("c1").await.is_ok());
    assert!(
        client
            .user_commits("alice", true, Some("cur"))
            .await
            .is_ok()
    );
    assert!(client.followers("alice", None).await.is_ok());
    assert!(client.following("alice", None).await.is_ok());

    let reqs = received(&server).await;
    assert_eq!(reqs.len(), 5);
    for req in &reqs {
        assert_eq!(verify_received(req), Ok(()), "{}", req.url);
    }
}

#[tokio::test]
async fn the_query_is_form_encoded_on_the_wire() {
    let server = MockServer::start().await;
    serve(&server, "GET", "/api/v1/saved", json(COMMITS)).await;

    let _ = signed(&server).saved(Some("a+b=c/d e")).await;

    let req = only_request(&server).await;
    assert_eq!(req.url.query(), Some("cursor=a%2Bb%3Dc%2Fd+e"));
}

#[tokio::test]
async fn get_requests_carry_no_content_type() {
    let server = MockServer::start().await;
    serve(&server, "GET", "/api/v1/me", json(ME)).await;

    let _ = signed(&server).me().await;

    assert!(
        only_request(&server)
            .await
            .headers
            .get("content-type")
            .is_none()
    );
}

#[tokio::test]
async fn invalid_grant_keeps_its_code() {
    let err = me_error(error_response(
        400,
        r#"{"message":"Code expired","code":"invalid_grant"}"#,
    ))
    .await;

    assert_eq!(err.code(), "invalid_grant");
}

#[test]
fn notifications_fixture_parses_with_a_commit_attached() {
    let page: crate::model::NotificationsPage = parse(NOTIFICATIONS);

    assert!(page.data[0].commit.is_some());
}

#[tokio::test]
async fn path_segments_that_would_be_rewritten_never_reach_the_network() {
    let server = MockServer::start().await;
    let client = signed(&server);

    let codes = vec![
        code_of(client.user("..").await),
        code_of(client.user(".").await),
        code_of(client.commit("").await),
        code_of(client.replies("../x", None).await),
        code_of(client.commit("a\\b").await),
    ];

    assert_eq!(codes, vec!["invalid_argument"; 5]);
    assert!(received(&server).await.is_empty());
}

#[tokio::test]
async fn an_unusable_base_url_is_its_own_error() {
    let result = ApiClient::new("not a url", None, Arc::new(FixedClock(NOW)));

    assert_eq!(
        result.err().map(|e| e.code()),
        Some("invalid_base_url".to_string())
    );
}

#[tokio::test]
async fn an_oversized_response_is_a_decode_error_naming_the_limit() {
    let server = MockServer::start().await;
    serve(&server, "GET", "/api/v1/me", json(ME)).await;
    let client = signed(&server).with_body_limit(100);

    let err = client
        .me()
        .await
        .expect_err("body is larger than the limit");

    assert_eq!(err.code(), "decode");
    assert!(
        matches!(&err.kind, super::error::ErrorKind::Decode(m) if m.contains("limit")),
        "{err:?}"
    );
}

#[tokio::test]
async fn an_oversized_error_body_is_also_refused() {
    let server = MockServer::start().await;
    serve(
        &server,
        "GET",
        "/api/v1/me",
        error_response(500, &"x".repeat(500)),
    )
    .await;
    let client = signed(&server).with_body_limit(100);

    let result = client.me().await;

    assert_eq!(code_of(result), "decode");
}

/// Serves one raw HTTP response on localhost, with no Content-Length so only
/// the running count can notice the size.
async fn serve_unsized(response: &'static [u8]) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        if let Ok((mut stream, _)) = listener.accept().await {
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf).await;
            let _ = stream.write_all(response).await;
            let _ = stream.shutdown().await;
        }
    });
    format!("http://{addr}")
}

#[tokio::test]
async fn a_chunked_response_over_the_limit_is_refused() {
    let body = "x".repeat(60);
    let raw: &'static [u8] = Box::leak(
        format!(
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nContent-Type: application/json\r\n\r\n3c\r\n{body}\r\n3c\r\n{body}\r\n0\r\n\r\n"
        )
        .into_bytes()
        .into_boxed_slice(),
    );
    let base = serve_unsized(raw).await;
    let client = client_for(&base, Some(Arc::new(TestSigner::golden()))).with_body_limit(100);

    let err = client.me().await.expect_err("120 bytes exceed the limit");

    assert!(
        matches!(&err.kind, super::error::ErrorKind::Decode(m) if m.contains("limit")),
        "{err:?}"
    );
}

#[tokio::test]
async fn a_connection_close_response_over_the_limit_is_refused() {
    let raw: &'static [u8] = Box::leak(
        format!(
            "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: application/json\r\n\r\n{}",
            "x".repeat(300)
        )
        .into_bytes()
        .into_boxed_slice(),
    );
    let base = serve_unsized(raw).await;
    let client = client_for(&base, Some(Arc::new(TestSigner::golden()))).with_body_limit(100);

    let err = client.me().await.expect_err("300 bytes exceed the limit");

    assert!(
        matches!(&err.kind, super::error::ErrorKind::Decode(m) if m.contains("limit")),
        "{err:?}"
    );
}

// ---- write endpoints ----

const COMMIT: &str = include_str!("../../tests/fixtures/commit.json");

fn commit_value() -> serde_json::Value {
    serde_json::from_str::<serde_json::Value>(COMMIT).expect("fixture parses")["data"].clone()
}

fn the_commit() -> crate::model::Commit {
    serde_json::from_value(commit_value()).expect("fixture commit parses")
}

fn the_user() -> crate::model::User {
    let v = serde_json::from_str::<serde_json::Value>(USER).expect("fixture parses");
    serde_json::from_value(v["data"].clone()).expect("fixture user parses")
}

fn no_content() -> ResponseTemplate {
    ResponseTemplate::new(204)
}

fn body_of(req: &Request) -> serde_json::Value {
    serde_json::from_slice(&req.body).expect("the request body is JSON")
}

fn content_type(req: &Request) -> Option<String> {
    req.headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
}

/// A signed JSON body: exact keys and values, a json Content-Type, and a verifiable signature.
fn assert_signed_json(req: &Request, expected: serde_json::Value) {
    assert_eq!(body_of(req), expected);
    assert!(content_type(req).is_some_and(|v| v.starts_with("application/json")));
    assert_eq!(verify_received(req), Ok(()));
}

/// A signed request with no body at all, and so no Content-Type either.
fn assert_signed_bodiless(req: &Request) {
    assert!(req.body.is_empty(), "no body expected");
    assert_eq!(content_type(req), None);
    assert_eq!(verify_received(req), Ok(()));
}

// create_commit

#[tokio::test]
async fn create_commit_sends_only_the_body_when_there_is_no_parent_or_quote() {
    let server = MockServer::start().await;
    let created =
        ResponseTemplate::new(201).set_body_raw(COMMIT.as_bytes().to_vec(), "application/json");
    serve(&server, "POST", "/api/v1/commits", created).await;

    let result = signed(&server).create_commit("hello", None, None).await;

    assert_eq!(result.map(|r| r.data).ok(), Some(the_commit()));
    assert_signed_json(
        &only_request(&server).await,
        serde_json::json!({"body": "hello"}),
    );
}

#[tokio::test]
async fn create_commit_as_a_reply_sends_parent_id() {
    let server = MockServer::start().await;
    let created =
        ResponseTemplate::new(201).set_body_raw(COMMIT.as_bytes().to_vec(), "application/json");
    serve(&server, "POST", "/api/v1/commits", created).await;

    let result = signed(&server).create_commit("re", Some("p-1"), None).await;

    assert!(result.is_ok(), "{result:?}");
    assert_signed_json(
        &only_request(&server).await,
        serde_json::json!({"body": "re", "parent_id": "p-1"}),
    );
}

#[tokio::test]
async fn create_commit_as_a_quote_sends_quoted_commit_id() {
    let server = MockServer::start().await;
    let created =
        ResponseTemplate::new(201).set_body_raw(COMMIT.as_bytes().to_vec(), "application/json");
    serve(&server, "POST", "/api/v1/commits", created).await;

    let result = signed(&server).create_commit("qt", None, Some("q-1")).await;

    assert!(result.is_ok(), "{result:?}");
    assert_signed_json(
        &only_request(&server).await,
        serde_json::json!({"body": "qt", "quoted_commit_id": "q-1"}),
    );
}

#[tokio::test]
async fn create_commit_with_both_parent_and_quote_is_refused_locally() {
    let server = MockServer::start().await;

    let result = signed(&server)
        .create_commit("x", Some("p-1"), Some("q-1"))
        .await;

    assert_eq!(code_of(result), "invalid_argument");
    assert!(received(&server).await.is_empty());
}

#[tokio::test]
async fn create_commit_with_an_empty_body_is_refused_locally() {
    let server = MockServer::start().await;

    let result = signed(&server).create_commit("", None, None).await;

    assert_eq!(code_of(result), "invalid_argument");
    assert!(received(&server).await.is_empty());
}

#[tokio::test]
async fn create_commit_leaves_text_and_length_to_the_server() {
    let server = MockServer::start().await;
    let created =
        ResponseTemplate::new(201).set_body_raw(COMMIT.as_bytes().to_vec(), "application/json");
    serve(&server, "POST", "/api/v1/commits", created).await;
    let text = format!("a\n\n\n\n\u{3042}\"{}", "x".repeat(4000));

    let result = signed(&server).create_commit(&text, None, None).await;

    assert!(result.is_ok(), "{result:?}");
    assert_eq!(
        body_of(&only_request(&server).await),
        serde_json::json!({"body": text})
    );
}

#[tokio::test]
async fn create_commit_404_without_a_code_is_http_404() {
    let server = MockServer::start().await;
    serve(
        &server,
        "POST",
        "/api/v1/commits",
        error_response(404, r#"{"message":"Not found."}"#),
    )
    .await;

    let result = signed(&server)
        .create_commit("re", Some("gone"), None)
        .await;

    assert_eq!(code_of(result), "http_404");
}

#[tokio::test]
async fn create_commit_with_an_empty_parent_or_quote_id_is_refused_locally() {
    let server = MockServer::start().await;
    let client = signed(&server);

    let parent = client.create_commit("x", Some(""), None).await;
    let quote = client.create_commit("x", None, Some("")).await;

    assert_eq!(code_of(parent), "invalid_argument");
    assert_eq!(code_of(quote), "invalid_argument");
    assert!(received(&server).await.is_empty());
}

#[tokio::test]
async fn create_commit_rate_limit_reports_retry_after() {
    let server = MockServer::start().await;
    serve(
        &server,
        "POST",
        "/api/v1/commits",
        error_response(429, r#"{"message":"Slow down","code":"rate_limited"}"#)
            .insert_header("Retry-After", "30"),
    )
    .await;

    let err = signed(&server)
        .create_commit("x", None, None)
        .await
        .expect_err("429 fails");

    assert_eq!(err.code(), "rate_limited");
    assert!(matches!(
        err.kind,
        super::error::ErrorKind::Http {
            retry_after: Some(30),
            ..
        }
    ));
}

#[tokio::test]
async fn create_commit_validation_errors_become_http_422() {
    let server = MockServer::start().await;
    serve(
        &server,
        "POST",
        "/api/v1/commits",
        error_response(422, r#"{"message":"The body field is too long."}"#),
    )
    .await;

    let err = signed(&server)
        .create_commit("x", None, None)
        .await
        .expect_err("422 fails");

    assert_eq!(err.code(), "http_422");
}

// delete_commit

#[tokio::test]
async fn delete_commit_sends_a_bodiless_signed_delete_and_accepts_204() {
    let server = MockServer::start().await;
    serve(&server, "DELETE", "/api/v1/commits/c1", no_content()).await;

    let result = signed(&server).delete_commit("c1").await;

    assert_eq!(result.map(|r| r.data).ok(), Some(()));
    assert_signed_bodiless(&only_request(&server).await);
}

#[tokio::test]
async fn delete_commit_403_without_a_code_is_http_403() {
    let server = MockServer::start().await;
    serve(
        &server,
        "DELETE",
        "/api/v1/commits/c1",
        error_response(403, r#"{"message":"No."}"#),
    )
    .await;

    let result = signed(&server).delete_commit("c1").await;

    assert_eq!(code_of(result), "http_403");
}

// star, unstar, bookmark, unbookmark

macro_rules! commit_write_test {
    ($name:ident, $verb:expr, $path:expr, $call:ident) => {
        #[tokio::test]
        async fn $name() {
            let server = MockServer::start().await;
            serve(&server, $verb, $path, json(COMMIT)).await;

            let result = signed(&server).$call("c1").await;

            assert_eq!(result.map(|r| r.data).ok(), Some(the_commit()));
            let req = only_request(&server).await;
            assert_signed_bodiless(&req);
        }
    };
}

commit_write_test!(
    star_posts_to_the_star_path_and_returns_the_commit,
    "POST",
    "/api/v1/commits/c1/star",
    star
);
commit_write_test!(
    unstar_deletes_the_star_path_and_returns_the_commit,
    "DELETE",
    "/api/v1/commits/c1/star",
    unstar
);
commit_write_test!(
    bookmark_puts_to_the_bookmark_path_and_returns_the_commit,
    "PUT",
    "/api/v1/commits/c1/bookmark",
    bookmark
);
commit_write_test!(
    unbookmark_deletes_the_bookmark_path_and_returns_the_commit,
    "DELETE",
    "/api/v1/commits/c1/bookmark",
    unbookmark
);

#[tokio::test]
async fn star_reads_the_rate_limit_headers() {
    let server = MockServer::start().await;
    serve(
        &server,
        "POST",
        "/api/v1/commits/c1/star",
        json(COMMIT)
            .insert_header("X-RateLimit-Limit", "60")
            .insert_header("X-RateLimit-Remaining", "59"),
    )
    .await;

    let result = signed(&server).star("c1").await.expect("star works");

    assert_eq!(
        result.rate,
        Some(RateLimit {
            limit: 60,
            remaining: 59
        })
    );
}

#[tokio::test]
async fn star_on_an_unavailable_commit_keeps_its_code() {
    let server = MockServer::start().await;
    serve(
        &server,
        "POST",
        "/api/v1/commits/c1/star",
        error_response(404, r#"{"message":"Gone","code":"commit_unavailable"}"#),
    )
    .await;

    let result = signed(&server).star("c1").await;

    assert_eq!(code_of(result), "commit_unavailable");
}

// give_pastel

#[tokio::test]
async fn give_pastel_posts_and_decodes_the_allowance_beside_the_commit() {
    let server = MockServer::start().await;
    let body = serde_json::json!({
        "data": commit_value(),
        "pasteis_remaining": 4,
        "pasteis_resets_at": "2026-10-21T00:00:00Z",
        "author_pasteis_received": 12,
    })
    .to_string();
    serve(&server, "POST", "/api/v1/commits/c1/pasteis", json(&body)).await;

    let result = signed(&server)
        .give_pastel("c1")
        .await
        .expect("pastel works");

    assert_eq!(result.data.data, the_commit());
    assert_eq!(result.data.pasteis_remaining, 4);
    assert_eq!(
        result.data.pasteis_resets_at.unix_timestamp(),
        1_792_540_800
    );
    assert_eq!(result.data.author_pasteis_received, Some(12));
    assert_signed_bodiless(&only_request(&server).await);
}

#[tokio::test]
async fn give_pastel_accepts_a_missing_author_total() {
    let server = MockServer::start().await;
    let body = serde_json::json!({
        "data": commit_value(),
        "pasteis_remaining": 0,
        "pasteis_resets_at": "2026-10-21T00:00:00Z",
        "author_pasteis_received": null,
    })
    .to_string();
    serve(&server, "POST", "/api/v1/commits/c1/pasteis", json(&body)).await;

    let result = signed(&server)
        .give_pastel("c1")
        .await
        .expect("pastel works");

    assert_eq!(result.data.author_pasteis_received, None);
}

#[tokio::test]
async fn give_pastel_exhausted_is_422_with_its_code() {
    let server = MockServer::start().await;
    serve(
        &server,
        "POST",
        "/api/v1/commits/c1/pasteis",
        error_response(
            422,
            r#"{"message":"No pasteis left today.","code":"pasteis_exhausted"}"#,
        ),
    )
    .await;

    let result = signed(&server).give_pastel("c1").await;

    assert_eq!(code_of(result), "pasteis_exhausted");
}

// report

#[tokio::test]
async fn give_pastel_403_without_a_code_is_http_403() {
    let server = MockServer::start().await;
    serve(
        &server,
        "POST",
        "/api/v1/commits/c1/pasteis",
        error_response(403, r#"{"message":"No."}"#),
    )
    .await;

    let result = signed(&server).give_pastel("c1").await;

    assert_eq!(code_of(result), "http_403");
}

#[tokio::test]
async fn report_posts_the_reason_and_details_and_accepts_204() {
    let server = MockServer::start().await;
    serve(&server, "POST", "/api/v1/commits/c1/reports", no_content()).await;

    let result = signed(&server)
        .report("c1", crate::model::ReportReason::Harassment, Some("rude"))
        .await;

    assert_eq!(result.map(|r| r.data).ok(), Some(()));
    assert_signed_json(
        &only_request(&server).await,
        serde_json::json!({"reason": "harassment", "details": "rude"}),
    );
}

#[tokio::test]
async fn report_omits_details_when_there_are_none() {
    let server = MockServer::start().await;
    serve(&server, "POST", "/api/v1/commits/c1/reports", no_content()).await;

    let result = signed(&server)
        .report("c1", crate::model::ReportReason::Spam, None)
        .await;

    assert!(result.is_ok(), "{result:?}");
    assert_signed_json(
        &only_request(&server).await,
        serde_json::json!({"reason": "spam"}),
    );
}

#[tokio::test]
async fn report_reasons_use_their_lowercase_names() {
    use crate::model::ReportReason::*;
    let server = MockServer::start().await;
    serve(&server, "POST", "/api/v1/commits/c1/reports", no_content()).await;
    let client = signed(&server);

    for reason in [Spam, Harassment, Unsafe, Other] {
        client.report("c1", reason, None).await.expect("accepted");
    }

    let sent: Vec<serde_json::Value> = received(&server)
        .await
        .iter()
        .map(|r| body_of(r)["reason"].clone())
        .collect();
    assert_eq!(
        sent,
        vec!["spam", "harassment", "unsafe", "other"]
            .into_iter()
            .map(serde_json::Value::from)
            .collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn report_details_over_500_characters_are_left_for_the_server_to_judge() {
    let server = MockServer::start().await;
    serve(
        &server,
        "POST",
        "/api/v1/commits/c1/reports",
        error_response(422, r#"{"message":"The details field is too long."}"#),
    )
    .await;
    let long = "x".repeat(501);

    let result = signed(&server)
        .report("c1", crate::model::ReportReason::Other, Some(&long))
        .await;

    assert_eq!(code_of(result), "http_422");
    assert_eq!(
        body_of(&only_request(&server).await)["details"],
        serde_json::Value::from(long)
    );
}

#[tokio::test]
async fn report_own_commit_and_unavailable_commit_keep_their_codes() {
    let own = MockServer::start().await;
    serve(
        &own,
        "POST",
        "/api/v1/commits/c1/reports",
        error_response(403, r#"{"message":"Yours.","code":"own_commit"}"#),
    )
    .await;
    let gone = MockServer::start().await;
    serve(
        &gone,
        "POST",
        "/api/v1/commits/c1/reports",
        error_response(404, r#"{"message":"Gone","code":"commit_unavailable"}"#),
    )
    .await;
    let spam = crate::model::ReportReason::Spam;

    assert_eq!(
        code_of(signed(&own).report("c1", spam, None).await),
        "own_commit"
    );
    assert_eq!(
        code_of(signed(&gone).report("c1", spam, None).await),
        "commit_unavailable"
    );
}

// follow, unfollow

#[tokio::test]
async fn follow_posts_a_bodiless_signed_request_and_accepts_204() {
    let server = MockServer::start().await;
    serve(&server, "POST", "/api/v1/users/alice/follow", no_content()).await;

    let result = signed(&server).follow("alice").await;

    assert_eq!(result.map(|r| r.data).ok(), Some(()));
    assert_signed_bodiless(&only_request(&server).await);
}

#[tokio::test]
async fn follow_ignores_an_unexpected_200_body() {
    let server = MockServer::start().await;
    serve(
        &server,
        "POST",
        "/api/v1/users/alice/follow",
        json(r#"{"ok":true}"#),
    )
    .await;

    let result = signed(&server).follow("alice").await;

    assert_eq!(result.map(|r| r.data).ok(), Some(()));
}

#[tokio::test]
async fn unfollow_deletes_the_follow_path_and_accepts_204() {
    let server = MockServer::start().await;
    serve(
        &server,
        "DELETE",
        "/api/v1/users/alice/follow",
        no_content(),
    )
    .await;

    let result = signed(&server).unfollow("alice").await;

    assert_eq!(result.map(|r| r.data).ok(), Some(()));
    assert_signed_bodiless(&only_request(&server).await);
}

#[tokio::test]
async fn follow_403_without_a_code_is_http_403() {
    let server = MockServer::start().await;
    serve(
        &server,
        "POST",
        "/api/v1/users/alice/follow",
        error_response(403, r#"{"message":"No."}"#),
    )
    .await;

    let result = signed(&server).follow("alice").await;

    assert_eq!(code_of(result), "http_403");
}

// mute, unmute, block, unblock

fn user_with(flag: &str, value: bool) -> String {
    let mut v: serde_json::Value = serde_json::from_str(USER).expect("fixture parses");
    v[flag] = serde_json::Value::Bool(value);
    v.to_string()
}

#[tokio::test]
async fn mute_and_unmute_return_the_user_with_the_new_state() {
    let server = MockServer::start().await;
    serve(
        &server,
        "POST",
        "/api/v1/users/alice/mute",
        json(&user_with("is_muting", true)),
    )
    .await;
    serve(
        &server,
        "DELETE",
        "/api/v1/users/alice/mute",
        json(&user_with("is_muting", false)),
    )
    .await;
    let client = signed(&server);

    let muted = client.mute("alice").await.expect("mute works").data;
    let unmuted = client.unmute("alice").await.expect("unmute works").data;

    assert_eq!((muted.data, muted.is_muting), (the_user(), true));
    assert_eq!((unmuted.data, unmuted.is_muting), (the_user(), false));
    for req in received(&server).await {
        assert_signed_bodiless(&req);
    }
}

#[tokio::test]
async fn block_and_unblock_return_the_user_with_the_new_state() {
    let server = MockServer::start().await;
    serve(
        &server,
        "POST",
        "/api/v1/users/alice/block",
        json(&user_with("is_blocking", true)),
    )
    .await;
    serve(
        &server,
        "DELETE",
        "/api/v1/users/alice/block",
        json(&user_with("is_blocking", false)),
    )
    .await;
    let client = signed(&server);

    let blocked = client.block("alice").await.expect("block works").data;
    let unblocked = client.unblock("alice").await.expect("unblock works").data;

    assert_eq!((blocked.data, blocked.is_blocking), (the_user(), true));
    assert_eq!((unblocked.data, unblocked.is_blocking), (the_user(), false));
    for req in received(&server).await {
        assert_signed_bodiless(&req);
    }
}

// update_lane

async fn lane_body(setting: crate::model::LaneSetting) -> serde_json::Value {
    let server = MockServer::start().await;
    serve(&server, "PUT", "/api/v1/me/lane", json(ME)).await;

    let result = signed(&server).update_lane(setting).await;

    assert_eq!(result.map(|r| r.data).ok(), Some(parse(ME)));
    let req = only_request(&server).await;
    assert!(content_type(&req).is_some_and(|v| v.starts_with("application/json")));
    assert_eq!(verify_received(&req), Ok(()));
    body_of(&req)
}

#[tokio::test]
async fn update_lane_automatic_sends_a_null_lane() {
    let body = lane_body(crate::model::LaneSetting::Automatic).await;

    assert_eq!(body, serde_json::json!({"lane": null}));
}

#[tokio::test]
async fn update_lane_palette_sends_the_index() {
    let first = lane_body(crate::model::LaneSetting::Palette(0)).await;
    let last = lane_body(crate::model::LaneSetting::Palette(5)).await;

    assert_eq!(first, serde_json::json!({"lane": 0}));
    assert_eq!(last, serde_json::json!({"lane": 5}));
}

#[tokio::test]
async fn update_lane_color_sends_a_null_lane_and_lowercase_zero_padded_hex() {
    use crate::model::{LaneSetting::Color, Rgb};

    let mixed = lane_body(Color(Rgb(0xAB, 0xCD, 0xEF))).await;
    let dark = lane_body(Color(Rgb(0, 0, 0x0A))).await;

    assert_eq!(mixed, serde_json::json!({"lane": null, "color": "#abcdef"}));
    assert_eq!(dark, serde_json::json!({"lane": null, "color": "#00000a"}));
}

#[tokio::test]
async fn update_lane_with_a_palette_index_above_5_is_refused_locally() {
    let server = MockServer::start().await;

    let result = signed(&server)
        .update_lane(crate::model::LaneSetting::Palette(6))
        .await;

    assert_eq!(code_of(result), "invalid_argument");
    assert!(received(&server).await.is_empty());
}

// update_pasteis_mode

#[tokio::test]
async fn update_pasteis_mode_puts_the_flag_and_returns_the_current_user() {
    let server = MockServer::start().await;
    serve(&server, "PUT", "/api/v1/me/pasteis-mode", json(ME)).await;
    let client = signed(&server);

    let on = client.update_pasteis_mode(true).await;
    let off = client.update_pasteis_mode(false).await;

    assert_eq!(on.map(|r| r.data).ok(), Some(parse(ME)));
    assert!(off.is_ok());
    let bodies: Vec<_> = received(&server).await;
    assert_eq!(bodies.len(), 2);
    assert_signed_json(&bodies[0], serde_json::json!({"enabled": true}));
    assert_signed_json(&bodies[1], serde_json::json!({"enabled": false}));
}

// sync_github

#[tokio::test]
async fn sync_github_puts_a_bodiless_signed_request_and_returns_the_current_user() {
    let server = MockServer::start().await;
    serve(&server, "PUT", "/api/v1/me/github", json(ME)).await;

    let result = signed(&server).sync_github().await;

    assert_eq!(result.map(|r| r.data).ok(), Some(parse(ME)));
    assert_signed_bodiless(&only_request(&server).await);
}

#[tokio::test]
async fn sync_github_failure_and_throttle_keep_their_codes() {
    let failed = MockServer::start().await;
    serve(
        &failed,
        "PUT",
        "/api/v1/me/github",
        error_response(
            503,
            r#"{"message":"GitHub is down.","code":"github_sync_failed"}"#,
        ),
    )
    .await;
    let throttled = MockServer::start().await;
    serve(
        &throttled,
        "PUT",
        "/api/v1/me/github",
        error_response(
            429,
            r#"{"message":"You just synced.","code":"rate_limited"}"#,
        )
        .insert_header("Retry-After", "60"),
    )
    .await;

    assert_eq!(
        code_of(signed(&failed).sync_github().await),
        "github_sync_failed"
    );
    let err = signed(&throttled)
        .sync_github()
        .await
        .expect_err("429 fails");
    assert_eq!(err.code(), "rate_limited");
    assert!(matches!(
        err.kind,
        super::error::ErrorKind::Http {
            retry_after: Some(60),
            ..
        }
    ));
}

// delete_account

#[tokio::test]
async fn delete_account_deletes_me_bodiless_and_accepts_204() {
    let server = MockServer::start().await;
    serve(&server, "DELETE", "/api/v1/me", no_content()).await;

    let result = signed(&server).delete_account().await;

    assert_eq!(result.map(|r| r.data).ok(), Some(()));
    assert_signed_bodiless(&only_request(&server).await);
}

// shared behavior

#[tokio::test]
async fn guest_calling_any_write_endpoint_fails_locally() {
    use crate::model::{LaneSetting, ReportReason};
    let server = MockServer::start().await;
    let c = guest(&server);

    let codes = vec![
        code_of(c.create_commit("x", None, None).await),
        code_of(c.delete_commit("c1").await),
        code_of(c.star("c1").await),
        code_of(c.unstar("c1").await),
        code_of(c.bookmark("c1").await),
        code_of(c.unbookmark("c1").await),
        code_of(c.give_pastel("c1").await),
        code_of(c.report("c1", ReportReason::Spam, None).await),
        code_of(c.follow("alice").await),
        code_of(c.unfollow("alice").await),
        code_of(c.mute("alice").await),
        code_of(c.unmute("alice").await),
        code_of(c.block("alice").await),
        code_of(c.unblock("alice").await),
        code_of(c.update_lane(LaneSetting::Automatic).await),
        code_of(c.update_pasteis_mode(true).await),
        code_of(c.sync_github().await),
        code_of(c.delete_account().await),
    ];

    assert_eq!(codes, vec!["sign_in_required"; 18]);
    assert!(
        received(&server).await.is_empty(),
        "nothing may reach the server"
    );
}

#[tokio::test]
async fn write_path_segments_that_would_be_rewritten_never_reach_the_network() {
    let server = MockServer::start().await;
    let c = signed(&server);

    let codes = vec![
        code_of(c.delete_commit("..").await),
        code_of(c.star("").await),
        code_of(c.unstar("a/b").await),
        code_of(c.bookmark("..").await),
        code_of(c.unbookmark("").await),
        code_of(c.give_pastel("a/b").await),
        code_of(c.follow("..").await),
        code_of(c.unfollow("a/b").await),
        code_of(c.mute("a\\b").await),
        code_of(c.unmute("..").await),
        code_of(c.block("").await),
        code_of(c.unblock("").await),
        code_of(
            c.report("../x", crate::model::ReportReason::Spam, None)
                .await,
        ),
    ];

    assert_eq!(codes, vec!["invalid_argument"; 13]);
    assert!(received(&server).await.is_empty());
}
