//! Key files are read-only once they exist; every case runs inside a tempdir.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;

use super::keys::{KeyError, KeyPair};
use super::test_support::*;

fn mode(path: &std::path::Path) -> u32 {
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

fn fingerprint_shape_ok(fp: &str) -> bool {
    fp.strip_prefix("SHA256:").is_some_and(|rest| {
        rest.len() == 43
            && rest
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/')
    })
}

#[test]
fn creates_a_missing_key_with_private_permissions() {
    let dir = tempfile::tempdir().unwrap();
    let key_dir = dir.path().join("lazydn");
    let path = key_dir.join("id_ed25519");

    let pair = KeyPair::load_or_create(&path);

    assert!(pair.is_ok(), "{pair:?}");
    assert_eq!(mode(&path), 0o600);
    assert_eq!(mode(&key_dir), 0o700);
    let public = std::fs::read_to_string(key_dir.join("id_ed25519.pub")).unwrap_or_default();
    assert!(public.starts_with("ssh-ed25519 "), "{public:?}");
    assert!(fingerprint_shape_ok(&pair.unwrap().fingerprint()));
}

#[test]
fn reads_an_existing_key_without_rewriting_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("id_ed25519");
    let first = KeyPair::load_or_create(&path).expect("first call creates the key");
    let before = std::fs::read(&path).unwrap();
    let mtime = std::fs::metadata(&path).unwrap().modified().unwrap();

    let second = KeyPair::load_or_create(&path).expect("second call reads the key");

    assert_eq!(first.fingerprint(), second.fingerprint());
    assert!(fingerprint_shape_ok(&second.fingerprint()));
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert_eq!(std::fs::metadata(&path).unwrap().modified().unwrap(), mtime);
}

#[test]
fn rejects_a_broken_file_and_leaves_it_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("id_ed25519");
    write_private_key(&path, "this is not a key\n");

    let result = KeyPair::load_or_create(&path);

    assert!(matches!(result, Err(KeyError::Parse(_))), "{result:?}");
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "this is not a key\n"
    );
}

#[test]
fn rejects_an_rsa_key_and_leaves_it_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("id_rsa");
    write_private_key(&path, &rsa_key_pem());

    let result = KeyPair::load_or_create(&path);

    assert!(matches!(result, Err(KeyError::NotEd25519)), "{result:?}");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), rsa_key_pem());
}

#[test]
fn rejects_an_encrypted_key_and_leaves_it_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("id_ed25519");
    write_private_key(&path, &encrypted_key_pem());

    let result = KeyPair::load_or_create(&path);

    assert!(matches!(result, Err(KeyError::Encrypted)), "{result:?}");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), encrypted_key_pem());
}

#[test]
fn fingerprint_of_the_golden_key_matches_ssh_keygen() {
    let dir = tempfile::tempdir().unwrap();
    let path = golden_key_in(dir.path());

    let pair = KeyPair::load_or_create(&path).expect("golden key loads");

    assert_eq!(pair.fingerprint(), GOLDEN_FINGERPRINT);
}

#[test]
fn rebuilds_a_missing_public_file_from_the_private_key() {
    let dir = tempfile::tempdir().unwrap();
    let path = golden_key_in(dir.path());
    let pub_path = dir.path().join("id_ed25519.pub");
    assert!(!pub_path.exists());

    let pair = KeyPair::load_or_create(&path);

    assert!(pair.is_ok(), "{pair:?}");
    let written = std::fs::read_to_string(&pub_path).unwrap_or_default();
    let golden_blob = GOLDEN_PUB.split_whitespace().nth(1).unwrap();
    assert_eq!(written.split_whitespace().nth(1), Some(golden_blob));
}

#[test]
fn trusts_the_private_key_over_a_mismatched_public_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = golden_key_in(dir.path());
    let pub_path = dir.path().join("id_ed25519.pub");
    let stale =
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOtherOtherOtherOtherOtherOtherOtherOtherOth stale\n";
    std::fs::write(&pub_path, stale).unwrap();

    let pair = KeyPair::load_or_create(&path).expect("loads despite a stale .pub");

    assert_eq!(std::fs::read_to_string(&pub_path).unwrap(), stale);
    assert_eq!(pair.fingerprint(), GOLDEN_FINGERPRINT);
    let golden_blob = GOLDEN_PUB.split_whitespace().nth(1).unwrap();
    assert_eq!(
        pair.public_key_openssh().split_whitespace().nth(1),
        Some(golden_blob)
    );
}

#[test]
fn keeps_the_permissions_of_an_existing_parent_directory() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    let path = dir.path().join("id_ed25519");

    let pair = KeyPair::load_or_create(&path);

    assert!(pair.is_ok(), "{pair:?}");
    assert_eq!(mode(dir.path()), 0o755);
}

#[test]
fn debug_output_does_not_expose_the_secret() {
    let dir = tempfile::tempdir().unwrap();
    let path = golden_key_in(dir.path());
    let pair = KeyPair::load_or_create(&path).expect("golden key loads");

    let shown = format!("{pair:?}");

    assert!(!shown.contains("PRIVATE KEY"), "{shown}");
    assert!(!shown.contains(GOLDEN_SEED_HEX), "{shown}");
    let seed_decimal = format!("{:?}", golden_seed_bytes());
    assert!(!shown.contains(&seed_decimal), "{shown}");
}

#[test]
fn an_unreadable_existing_file_is_an_io_error_and_is_not_replaced() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("id_ed25519");
    write_private_key(&path, &golden_key_openssh());
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::fs::read(&path).is_ok() {
        return; // running as root: the file is readable anyway
    }

    let other = dir.path().join("readable");
    write_private_key(&other, &golden_key_openssh());
    assert!(KeyPair::load_or_create(&other).is_ok());

    let result = KeyPair::load_or_create(&path);

    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert!(matches!(result, Err(KeyError::Io(_))), "{result:?}");
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        golden_key_openssh()
    );
}

#[test]
fn creation_leaves_no_temporary_files_behind() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("id_ed25519");

    let pair = KeyPair::load_or_create(&path);

    assert!(pair.is_ok(), "{pair:?}");
    let mut names: Vec<String> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    assert_eq!(names, vec!["id_ed25519", "id_ed25519.pub"]);
}

#[test]
fn a_key_file_open_to_group_or_others_is_refused_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let path = golden_key_in(dir.path());
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();

    let result = KeyPair::load_or_create(&path);

    assert!(
        matches!(result, Err(KeyError::InsecurePermissions)),
        "{result:?}"
    );
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        golden_key_openssh()
    );
    assert_eq!(mode(&path), 0o640);
}

#[test]
fn a_path_that_is_not_a_regular_file_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("id_ed25519");
    std::fs::create_dir(&path).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();

    let result = KeyPair::load_or_create(&path);

    assert!(
        matches!(result, Err(KeyError::NotARegularFile)),
        "{result:?}"
    );
}

#[test]
fn a_missing_public_file_that_cannot_be_written_does_not_fail_loading() {
    let dir = tempfile::tempdir().unwrap();
    let keys = dir.path().join("keys");
    std::fs::create_dir(&keys).unwrap();
    let path = golden_key_in(&keys);
    std::fs::set_permissions(&keys, std::fs::Permissions::from_mode(0o500)).unwrap();
    let probe = keys.join("probe");
    let writable = std::fs::write(&probe, b"").is_ok();

    let result = KeyPair::load_or_create(&path);

    std::fs::set_permissions(&keys, std::fs::Permissions::from_mode(0o700)).unwrap();
    if writable {
        return; // running as root: the directory is writable anyway
    }
    assert!(result.is_ok(), "{result:?}");
    assert!(!keys.join("id_ed25519.pub").exists());
}
