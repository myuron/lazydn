use super::nonce;
use super::pkce::{Pkce, random_state};
use super::test_support::is_lower_hex32;

#[test]
fn verifier_is_43_to_128_unreserved_characters() {
    let pkce = Pkce::generate();

    assert!(
        (43..=128).contains(&pkce.verifier.len()),
        "{}",
        pkce.verifier
    );
    assert!(
        pkce.verifier
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._~-".contains(&b)),
        "{}",
        pkce.verifier
    );
}

#[test]
fn challenge_matches_the_rfc7636_appendix_b_vector() {
    let challenge = Pkce::challenge_for("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk");

    assert_eq!(challenge, "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
}

#[test]
fn generated_challenge_belongs_to_the_generated_verifier() {
    let pkce = Pkce::generate();

    assert_eq!(pkce.challenge.len(), 43);
    assert_eq!(pkce.challenge, Pkce::challenge_for(&pkce.verifier));
}

#[test]
fn two_pkce_pairs_differ() {
    let a = Pkce::generate();
    let b = Pkce::generate();

    assert_ne!(a.verifier, b.verifier);
}

#[test]
fn nonce_is_32_lowercase_hex_and_unique() {
    let a = nonce::generate();
    let b = nonce::generate();

    assert!(is_lower_hex32(&a), "{a:?}");
    assert!(is_lower_hex32(&b), "{b:?}");
    assert_ne!(a, b);
}

#[test]
fn state_is_32_lowercase_hex_and_unique() {
    let a = random_state();
    let b = random_state();

    assert!(is_lower_hex32(&a), "{a:?}");
    assert!(is_lower_hex32(&b), "{b:?}");
    assert_ne!(a, b);
}
