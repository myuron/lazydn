use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::exec::TokioExec;
use super::run::{ClientKind, Exec};
use super::startup::session_for_key;
use crate::action::{Action, Effect};
use crate::api::clock::FixedClock;
use crate::api::keys::KeyPair;
use crate::api::test_support::{golden_key_in, verify_received};
use crate::app::Session;
use crate::feed::ReqId;

const ME: &str = include_str!("../../tests/fixtures/me.json");

async fn me_server() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/me"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(ME, "application/json"))
        .mount(&server)
        .await;
    server
}

fn exec_for(
    base: &str,
    dir: &Path,
    key: Option<Arc<KeyPair>>,
) -> (TokioExec, UnboundedReceiver<Action>) {
    let (tx, rx) = unbounded_channel();
    let exec = TokioExec::new(
        base,
        dir.join("id_ed25519"),
        "lazydn on testhost",
        key,
        Arc::new(FixedClock(1_700_000_000)),
        tx,
    )
    .expect("the executor builds");
    (exec, rx)
}

async fn next(rx: &mut UnboundedReceiver<Action>) -> Action {
    tokio::time::timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("an action arrives in time")
        .expect("the channel is open")
}

fn fetch_me() -> Effect {
    Effect::FetchMe { req: ReqId(1) }
}

#[tokio::test]
async fn a_signed_request_carries_the_signature_headers() {
    let server = me_server().await;
    let dir = tempfile::tempdir().unwrap();
    let key = KeyPair::load(&golden_key_in(dir.path())).unwrap().unwrap();
    let (mut exec, mut rx) = exec_for(&server.uri(), dir.path(), Some(Arc::new(key)));
    exec.spawn(ClientKind::Signed, fetch_me());
    assert!(matches!(next(&mut rx).await, Action::MeLoaded { .. }));
    let requests = server.received_requests().await.unwrap();
    assert_eq!(verify_received(&requests[0]), Ok(()));
}

#[tokio::test]
async fn a_guest_request_carries_no_signature() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/commits/c1"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            include_str!("../../tests/fixtures/thread.json"),
            "application/json",
        ))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let key = KeyPair::load(&golden_key_in(dir.path())).unwrap().unwrap();
    let (mut exec, mut rx) = exec_for(&server.uri(), dir.path(), Some(Arc::new(key)));
    exec.spawn(
        ClientKind::Guest,
        Effect::FetchThread {
            id: "c1".into(),
            req: ReqId(1),
        },
    );
    next(&mut rx).await;
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].headers.get("X-SSH-Signature").is_none());
}

#[tokio::test]
async fn the_key_read_at_startup_signs_the_first_request_of_a_restoring_session() {
    let server = me_server().await;
    let dir = tempfile::tempdir().unwrap();
    let key_path = golden_key_in(dir.path());
    let (session, _, key) = session_for_key(&key_path);
    assert_eq!(session, Session::Restoring);
    let (mut exec, mut rx) = exec_for(&server.uri(), dir.path(), key);
    exec.spawn(ClientKind::Signed, fetch_me());
    next(&mut rx).await;
    let requests = server.received_requests().await.unwrap();
    assert_eq!(verify_received(&requests[0]), Ok(()));
}

#[tokio::test]
async fn prepare_sign_in_answers_through_the_channel_and_writes_the_key_in_place() {
    let server = MockServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    let (mut exec, mut rx) = exec_for(&server.uri(), dir.path(), None);
    exec.spawn(ClientKind::Guest, Effect::PrepareSignIn { create: true });
    assert!(matches!(
        next(&mut rx).await,
        Action::SignInPrepared { created: true, .. }
    ));
    assert!(dir.path().join("id_ed25519").exists());
}

// --- starting without a key ---------------------------------------------------

fn ok_with(body: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_raw(body, "application/json")
}

#[tokio::test]
async fn a_key_made_by_prepare_sign_in_signs_the_requests_after_it() {
    let server = me_server().await;
    let dir = tempfile::tempdir().unwrap();
    let (mut exec, mut rx) = exec_for(&server.uri(), dir.path(), None);
    exec.spawn(ClientKind::Guest, Effect::PrepareSignIn { create: true });
    let Action::SignInPrepared { fingerprint, .. } = next(&mut rx).await else {
        panic!("expected SignInPrepared");
    };
    exec.spawn(ClientKind::Signed, fetch_me());
    assert!(matches!(next(&mut rx).await, Action::MeLoaded { .. }));
    let made = KeyPair::load(&dir.path().join("id_ed25519"))
        .unwrap()
        .expect("the key file");
    assert_eq!(made.fingerprint(), fingerprint);
    let requests = server.received_requests().await.unwrap();
    let signed = requests
        .iter()
        .find(|r| r.headers.get("X-SSH-Signature").is_some())
        .expect("a signed request");
    assert_eq!(verify_with(&made, signed), Ok(()));
}

#[tokio::test]
async fn register_device_answers_device_registered() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/devices"))
        .respond_with(ok_with(ME))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let (mut exec, mut rx) = exec_for(&server.uri(), dir.path(), None);
    exec.spawn(ClientKind::Guest, Effect::PrepareSignIn { create: true });
    next(&mut rx).await;
    exec.spawn(
        ClientKind::Guest,
        Effect::RegisterDevice {
            code: "c".repeat(64),
            verifier: "the-verifier".into(),
        },
    );
    assert!(matches!(
        next(&mut rx).await,
        Action::DeviceRegistered { .. }
    ));
}

/// Checks the signature against the public half of `key`, so the test needs no fixed key.
fn verify_with(key: &KeyPair, req: &wiremock::Request) -> Result<(), String> {
    let header = |name: &str| req.headers.get(name).unwrap().to_str().unwrap().to_string();
    let sig = crate::api::test_support::parse_signature_header(&header("X-SSH-Signature"))?;
    let mut uri = req.url.path().to_string();
    if let Some(q) = req.url.query() {
        uri.push('?');
        uri.push_str(q);
    }
    let mut msg = format!(
        "{}\n{}\n{}\n{uri}\n",
        header("X-Devnull-Timestamp"),
        header("X-Devnull-Nonce"),
        req.method
    )
    .into_bytes();
    msg.extend_from_slice(&req.body);
    let public =
        ssh_key::PublicKey::from_openssh(&key.public_key_openssh()).map_err(|e| e.to_string())?;
    public
        .verify(crate::api::request::NAMESPACE, &msg, &sig)
        .map_err(|e| e.to_string())
}
