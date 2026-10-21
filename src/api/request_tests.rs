use std::sync::Mutex;

use super::clock::FixedClock;
use super::error::ErrorKind;
use super::request::{NAMESPACE, encode_signature, sign_request, signing_message};
use super::sign::{SignError, Signer};
use super::test_support::*;

#[test]
fn message_for_a_get_without_a_body_ends_after_the_newline() {
    let msg = signing_message(
        1700000000,
        "0123456789abcdef0123456789abcdef",
        "GET",
        "/api/v1/me",
        b"",
    );

    assert_eq!(msg, GOLDEN_MSG1);
}

#[test]
fn message_for_a_post_appends_the_raw_json_body() {
    let body = br#"{"code":"abc","code_verifier":"xyz"}"#;

    let msg = signing_message(
        1700000000,
        "0123456789abcdef0123456789abcdef",
        "POST",
        "/api/v1/devices",
        body,
    );

    assert_eq!(msg, GOLDEN_MSG2);
}

#[test]
fn message_keeps_the_query_string_of_the_path() {
    let msg = signing_message(5, "n", "GET", "/api/v1/log/latest?cursor=abc&since=x", b"");

    assert_eq!(msg, b"5\nn\nGET\n/api/v1/log/latest?cursor=abc&since=x\n");
}

#[test]
fn golden_signatures_verify_over_the_assembled_messages() {
    let public = golden_public_key();
    let msg1 = signing_message(
        1700000000,
        "0123456789abcdef0123456789abcdef",
        "GET",
        "/api/v1/me",
        b"",
    );
    let sig1 = bytes_to_sshsig(&armored_to_bytes(GOLDEN_SIG1)).unwrap();

    assert!(public.verify(NAMESPACE, &msg1, &sig1).is_ok());
}

#[test]
fn encoded_signature_is_a_single_line_of_strict_base64() {
    let bytes = armored_to_bytes(GOLDEN_SIG1);

    let header = encode_signature(&bytes);

    assert!(!header.is_empty());
    assert!(
        !header.contains('\r') && !header.contains('\n'),
        "{header:?}"
    );
    assert!(!header.contains("-----"), "{header:?}");
    let parsed = parse_signature_header(&header).expect("server-style parse succeeds");
    assert_eq!(parsed.namespace(), NAMESPACE);
}

#[test]
fn signed_headers_carry_the_clock_time_and_a_verifiable_signature() {
    let signer = TestSigner::golden();
    let clock = FixedClock(1700000000);

    let headers = sign_request(&signer, &clock, "GET", "/api/v1/me?x=1", b"").expect("signs");

    assert_eq!(headers.timestamp, "1700000000");
    assert!(is_lower_hex32(&headers.nonce), "{:?}", headers.nonce);
    let sig = parse_signature_header(&headers.signature).expect("parses");
    let msg = signing_message(1700000000, &headers.nonce, "GET", "/api/v1/me?x=1", b"");
    assert!(golden_public_key().verify(NAMESPACE, &msg, &sig).is_ok());
}

#[test]
fn each_signed_request_gets_its_own_nonce() {
    let signer = TestSigner::golden();
    let clock = FixedClock(1700000000);

    let a = sign_request(&signer, &clock, "GET", "/api/v1/me", b"").expect("signs");
    let b = sign_request(&signer, &clock, "GET", "/api/v1/me", b"").expect("signs");

    assert_ne!(a.nonce, b.nonce);
}

#[test]
fn signer_failure_becomes_a_sign_error_without_leaking_details_to_the_code() {
    let clock = FixedClock(1700000000);

    let err = sign_request(&FailingSigner, &clock, "GET", "/api/v1/me", b"").expect_err("fails");

    assert!(matches!(err.kind, ErrorKind::Sign(_)), "{err:?}");
    assert_eq!(err.code(), "sign");
}

#[test]
fn signer_receives_the_devnull_namespace_and_the_assembled_message() {
    struct Spy(Mutex<Vec<(String, Vec<u8>)>>);
    impl Signer for Spy {
        fn sign(&self, namespace: &str, message: &[u8]) -> Result<Vec<u8>, SignError> {
            self.0
                .lock()
                .unwrap()
                .push((namespace.into(), message.into()));
            Ok(b"SSHSIG".to_vec())
        }
    }
    let spy = Spy(Mutex::new(Vec::new()));

    let headers = sign_request(&spy, &FixedClock(42), "POST", "/api/v1/devices", b"{}");

    let calls = spy.0.lock().unwrap();
    assert!(headers.is_ok(), "{headers:?}");
    let nonce = headers.unwrap().nonce;
    assert_eq!(
        *calls,
        vec![(
            NAMESPACE.to_string(),
            signing_message(42, &nonce, "POST", "/api/v1/devices", b"{}")
        )]
    );
}
