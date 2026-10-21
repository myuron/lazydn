use std::collections::HashMap;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Arc;

use url::Url;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::sign_in::{SignInHost, device_name_from};
use crate::action::{Action, Failure};
use crate::api::authorize::REDIRECT_URI;
use crate::api::clock::FixedClock;
use crate::api::keys::{KeyError, KeyPair};
use crate::api::pkce::Pkce;
use crate::api::test_support::{GOLDEN_FINGERPRINT, golden_key_in, verify_received};
use crate::model::MeResponse;

const ME: &str = include_str!("../../tests/fixtures/me.json");
const CODE: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn host_in(dir: &Path, base: &str) -> SignInHost {
    SignInHost::new(
        dir.join("id_ed25519"),
        base,
        "lazydn on testhost",
        Arc::new(FixedClock(1_700_000_000)),
    )
}

/// The only action a `prepare` call answered with, taken apart.
struct Prepared {
    fingerprint: String,
    url: Url,
    pkce: Pkce,
    state: String,
    created: bool,
}

fn prepared(actions: Vec<Action>) -> Prepared {
    let [
        Action::SignInPrepared {
            fingerprint,
            url,
            pkce,
            state,
            created,
        },
    ] = actions.as_slice()
    else {
        panic!("expected one SignInPrepared, got {actions:?}");
    };
    Prepared {
        fingerprint: fingerprint.clone(),
        url: Url::parse(url).expect("the approval URL parses"),
        pkce: pkce.clone(),
        state: state.clone(),
        created: *created,
    }
}

fn query(url: &Url) -> HashMap<String, String> {
    url.query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect()
}

fn me_response() -> MeResponse {
    serde_json::from_str(ME).expect("fixture parses")
}

// --- PrepareSignIn: the key ----------------------------------------------------------------------

#[test]
fn prepare_with_create_makes_the_key_in_the_given_directory() {
    let dir = tempfile::tempdir().unwrap();
    let host = host_in(dir.path(), "http://127.0.0.1:9");

    let made = prepared(host.prepare(true));

    let key_path = dir.path().join("id_ed25519");
    let mode = std::fs::metadata(&key_path).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600);
    assert!(made.created);
    let loaded = KeyPair::load(&key_path)
        .unwrap()
        .expect("the key was written");
    assert_eq!(made.fingerprint, loaded.fingerprint());
}

#[test]
fn prepare_never_replaces_an_existing_key() {
    let dir = tempfile::tempdir().unwrap();
    let key_path = golden_key_in(dir.path());
    let before = std::fs::read(&key_path).unwrap();
    let host = host_in(dir.path(), "http://127.0.0.1:9");

    let kept = prepared(host.prepare(true));

    assert!(!kept.created);
    assert_eq!(kept.fingerprint, GOLDEN_FINGERPRINT);
    assert_eq!(std::fs::read(&key_path).unwrap(), before);
}

#[test]
fn prepare_without_create_reads_the_existing_key() {
    let dir = tempfile::tempdir().unwrap();
    golden_key_in(dir.path());
    let host = host_in(dir.path(), "http://127.0.0.1:9");

    let read = prepared(host.prepare(false));

    assert!(!read.created);
    assert_eq!(read.fingerprint, GOLDEN_FINGERPRINT);
}

#[test]
fn prepare_without_create_writes_nothing_when_there_is_no_key() {
    let dir = tempfile::tempdir().unwrap();
    let host = host_in(dir.path(), "http://127.0.0.1:9");

    let actions = host.prepare(false);

    assert_eq!(actions, vec![Action::SignInKeyMissing]);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    assert!(host.key().is_none());
}

#[test]
fn prepare_with_create_reports_a_key_that_cannot_be_written() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().join("not-a-directory");
    std::fs::write(&parent, b"file").unwrap();
    let host = SignInHost::new(
        parent.join("id_ed25519"),
        "http://127.0.0.1:9",
        "lazydn on testhost",
        Arc::new(FixedClock(1_700_000_000)),
    );

    let actions = host.prepare(true);

    assert!(
        matches!(actions.as_slice(), [Action::SignInKeyFailed { .. }]),
        "{actions:?}"
    );
    assert!(host.key().is_none());
}

#[test]
fn prepare_reports_a_key_that_others_can_read() {
    let dir = tempfile::tempdir().unwrap();
    let key_path = golden_key_in(dir.path());
    std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o644)).unwrap();
    let host = host_in(dir.path(), "http://127.0.0.1:9");

    let actions = host.prepare(false);

    assert_eq!(
        actions,
        vec![Action::SignInKeyFailed {
            error: KeyError::InsecurePermissions
        }]
    );
    assert!(host.key().is_none());
}

#[test]
fn the_host_keeps_the_key_that_prepare_read() {
    let dir = tempfile::tempdir().unwrap();
    golden_key_in(dir.path());
    let host = host_in(dir.path(), "http://127.0.0.1:9");
    assert!(host.key().is_none());

    host.prepare(false);

    let key = host.key().expect("the key is kept");
    assert_eq!(key.fingerprint(), GOLDEN_FINGERPRINT);
}

// --- PrepareSignIn: the URL ----------------------------------------------------------------------

#[test]
fn the_approval_url_carries_the_fingerprint_challenge_state_and_redirect() {
    let dir = tempfile::tempdir().unwrap();
    golden_key_in(dir.path());
    let host = host_in(dir.path(), "http://127.0.0.1:9");

    let made = prepared(host.prepare(false));

    let q = query(&made.url);
    assert_eq!(made.url.path(), "/mobile/authorize");
    assert_eq!(q["fingerprint"], GOLDEN_FINGERPRINT);
    assert_eq!(q["device_name"], "lazydn on testhost");
    assert_eq!(q["code_challenge"], made.pkce.challenge);
    assert_eq!(
        q["code_challenge"],
        Pkce::challenge_for(&made.pkce.verifier)
    );
    assert_eq!(q["code_challenge_method"], "S256");
    assert_eq!(q["state"], made.state);
    assert_eq!(q["redirect_uri"], REDIRECT_URI);
    assert_eq!(q.len(), 6);
}

#[test]
fn the_approval_url_never_contains_the_verifier() {
    let dir = tempfile::tempdir().unwrap();
    golden_key_in(dir.path());
    let host = host_in(dir.path(), "http://127.0.0.1:9");

    let made = prepared(host.prepare(false));

    assert!(!made.url.as_str().contains(&made.pkce.verifier));
}

#[test]
fn the_approval_url_is_on_the_configured_host() {
    let dir = tempfile::tempdir().unwrap();
    golden_key_in(dir.path());
    for base in ["http://127.0.0.1:9", "https://dev-null.sh"] {
        let host = host_in(dir.path(), base);

        let made = prepared(host.prepare(false));

        assert!(
            made.url
                .as_str()
                .starts_with(&format!("{base}/mobile/authorize?")),
            "{base}: {}",
            made.url
        );
    }
}

#[test]
fn every_prepare_makes_a_new_verifier_and_state() {
    let dir = tempfile::tempdir().unwrap();
    golden_key_in(dir.path());
    let host = host_in(dir.path(), "http://127.0.0.1:9");

    let first = prepared(host.prepare(false));
    let second = prepared(host.prepare(false));

    assert_ne!(first.pkce.verifier, second.pkce.verifier);
    assert_ne!(first.state, second.state);
}

// --- the signed client -----------------------------------------------------------------------------

#[test]
fn the_signed_client_exists_only_once_a_key_is_known() {
    let dir = tempfile::tempdir().unwrap();
    golden_key_in(dir.path());
    let host = host_in(dir.path(), "http://127.0.0.1:9");
    assert!(host.signed_client().is_none());

    host.prepare(false);

    assert!(host.signed_client().is_some());
}

// --- RegisterDevice ----------------------------------------------------------------------------------

fn json(body: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_raw(body.as_bytes().to_vec(), "application/json")
}

async fn serve_devices(server: &MockServer, response: ResponseTemplate) {
    Mock::given(method("POST"))
        .and(path("/api/v1/devices"))
        .respond_with(response)
        .mount(server)
        .await;
}

fn online() -> Action {
    Action::ApiStatus {
        rate: None,
        online: true,
    }
}

#[tokio::test]
async fn register_posts_the_code_and_verifier_signed_with_the_key_and_answers_the_account() {
    let server = MockServer::start().await;
    serve_devices(&server, json(ME)).await;
    let dir = tempfile::tempdir().unwrap();
    golden_key_in(dir.path());
    let host = host_in(dir.path(), &server.uri());
    host.prepare(false);

    let actions = host.register(CODE, "the-verifier").await;

    assert_eq!(
        actions,
        vec![
            Action::DeviceRegistered {
                response: Box::new(me_response())
            },
            online()
        ]
    );
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert_eq!(
        body,
        serde_json::json!({"code": CODE, "code_verifier": "the-verifier"})
    );
    assert_eq!(verify_received(&requests[0]), Ok(()));
}

#[tokio::test]
async fn register_reports_the_error_code_of_a_refusal() {
    let server = MockServer::start().await;
    let refusal = ResponseTemplate::new(400).set_body_raw(
        br#"{"message":"m","code":"invalid_grant"}"#.to_vec(),
        "application/json",
    );
    serve_devices(&server, refusal).await;
    let dir = tempfile::tempdir().unwrap();
    golden_key_in(dir.path());
    let host = host_in(dir.path(), &server.uri());
    host.prepare(false);

    let actions = host.register(CODE, "v").await;

    assert_eq!(
        actions,
        vec![
            Action::SignInFailed {
                failure: Failure::new("invalid_grant")
            },
            online()
        ]
    );
}

#[tokio::test]
async fn register_reports_a_host_that_cannot_be_reached_as_offline() {
    let dir = tempfile::tempdir().unwrap();
    golden_key_in(dir.path());
    // Nothing listens on port 1 of the loopback, so the connection is refused at once.
    let host = host_in(dir.path(), "http://127.0.0.1:1");
    host.prepare(false);

    let actions = host.register(CODE, "v").await;

    assert_eq!(
        actions,
        vec![
            Action::SignInFailed {
                failure: Failure::new("network")
            },
            Action::ApiStatus {
                rate: None,
                online: false
            }
        ]
    );
}

#[tokio::test]
async fn register_without_a_key_sends_nothing() {
    let server = MockServer::start().await;
    serve_devices(&server, json(ME)).await;
    let dir = tempfile::tempdir().unwrap();
    let host = host_in(dir.path(), &server.uri());

    let actions = host.register(CODE, "v").await;

    assert_eq!(
        actions,
        vec![Action::SignInFailed {
            failure: Failure::new("key_missing")
        }]
    );
    assert!(server.received_requests().await.unwrap().is_empty());
}

// --- the device name ------------------------------------------------------------------------------

#[test]
fn the_device_name_says_which_host_it_runs_on() {
    assert_eq!(device_name_from(Some("box")), "lazydn on box");
    assert_eq!(device_name_from(Some("box\n")), "lazydn on box");
}

#[test]
fn the_device_name_is_plain_lazydn_without_a_usable_host_name() {
    for hostname in [None, Some(""), Some("  \n"), Some("\x1b\x07")] {
        assert_eq!(device_name_from(hostname), "lazydn", "{hostname:?}");
    }
}

#[test]
fn the_device_name_drops_control_characters() {
    assert_eq!(device_name_from(Some("bo\x1b[0mx\r")), "lazydn on bo[0mx");
}

#[test]
fn the_device_name_is_cut_at_100_characters_in_all() {
    let long = "h".repeat(150);

    assert_eq!(
        device_name_from(Some(&long)),
        format!("lazydn on {}", "h".repeat(90))
    );
    assert!(device_name_from(Some(&long)).chars().count() <= 100);
}

#[test]
fn the_device_name_drops_bidi_marks() {
    assert_eq!(
        device_name_from(Some("bo\u{202e}x\u{2066}")),
        "lazydn on box"
    );
}

#[test]
fn the_device_name_drops_the_arabic_letter_mark_and_line_separators() {
    assert_eq!(
        device_name_from(Some("a\u{061c}b\u{2028}c\u{2029}")),
        "lazydn on abc"
    );
}
