use ssh_key::HashAlg;

use super::keys::KeyPair;
use super::request::NAMESPACE;
use super::sign::Signer;
use super::test_support::*;

fn golden_pair() -> (tempfile::TempDir, KeyPair) {
    let dir = tempfile::tempdir().unwrap();
    let path = golden_key_in(dir.path());
    let pair = KeyPair::load_or_create(&path).expect("golden key loads");
    (dir, pair)
}

#[test]
fn signature_matches_the_ssh_keygen_golden_vectors() {
    let (_dir, pair) = golden_pair();

    let sig1 = pair.sign(NAMESPACE, GOLDEN_MSG1);
    let sig2 = pair.sign(NAMESPACE, GOLDEN_MSG2);

    assert_eq!(sig1, Ok(armored_to_bytes(GOLDEN_SIG1)));
    assert_eq!(sig2, Ok(armored_to_bytes(GOLDEN_SIG2)));
}

#[test]
fn signature_verifies_with_the_public_key() {
    let (_dir, pair) = golden_pair();

    let bytes = pair.sign(NAMESPACE, b"hello").expect("signs");

    let sig = bytes_to_sshsig(&bytes).expect("parses as SSHSIG");
    assert_eq!(sig.namespace(), NAMESPACE);
    assert!(
        golden_public_key()
            .verify(NAMESPACE, b"hello", &sig)
            .is_ok()
    );
}

#[test]
fn signature_uses_sha512() {
    let (_dir, pair) = golden_pair();

    let bytes = pair.sign(NAMESPACE, b"hello").expect("signs");

    let sig = bytes_to_sshsig(&bytes).expect("parses as SSHSIG");
    assert_eq!(sig.hash_alg(), HashAlg::Sha512);
}

#[test]
fn verification_fails_for_another_namespace_or_message() {
    let (_dir, pair) = golden_pair();
    let bytes = pair.sign(NAMESPACE, b"hello").expect("signs");
    let sig = bytes_to_sshsig(&bytes).expect("parses as SSHSIG");
    let public = golden_public_key();

    assert!(public.verify("other-namespace", b"hello", &sig).is_err());
    assert!(public.verify(NAMESPACE, b"hellO", &sig).is_err());
}

#[test]
fn a_freshly_created_key_signs_verifiably() {
    let dir = tempfile::tempdir().unwrap();
    let pair = KeyPair::load_or_create(&dir.path().join("k")).expect("creates a key");

    let bytes = pair.sign(NAMESPACE, b"msg").expect("signs");

    let sig = bytes_to_sshsig(&bytes).expect("parses as SSHSIG");
    let public = ssh_key::PublicKey::from_openssh(&pair.public_key_openssh()).expect("pub parses");
    assert!(public.verify(NAMESPACE, b"msg", &sig).is_ok());
}
