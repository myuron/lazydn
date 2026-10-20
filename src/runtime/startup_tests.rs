use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use time::UtcOffset;

use super::startup::{Startup, build_app, session_for_key};
use crate::action::Effect;
use crate::api::keys::KeyError;
use crate::api::test_support::{
    GOLDEN_FINGERPRINT, golden_key_in, golden_key_openssh, write_private_key,
};
use crate::app::{KeyStatus, LogTab, Prefs, Session, Theme, ToastKind};
use crate::config::{LastVisits, LoadedConfig};
use crate::test_support::NOW;

#[test]
fn no_key_file_starts_a_guest_who_can_create_one() {
    let dir = tempfile::tempdir().unwrap();
    let (session, status, _) = session_for_key(&dir.path().join("id_ed25519"));
    assert_eq!(session, Session::Guest);
    assert_eq!(status, KeyStatus::Absent);
}

#[test]
fn a_usable_key_starts_a_restoring_session() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("id_ed25519");
    write_private_key(&path, &golden_key_openssh());
    let (session, status, _) = session_for_key(&path);
    assert_eq!(session, Session::Restoring);
    assert_eq!(status, KeyStatus::Present);
}

#[test]
fn a_key_open_to_others_starts_a_guest_with_the_reason() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("id_ed25519");
    write_private_key(&path, &golden_key_openssh());
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    let (session, status, _) = session_for_key(&path);
    assert_eq!(session, Session::Guest);
    assert_eq!(
        status,
        KeyStatus::Unreadable {
            error: KeyError::InsecurePermissions,
            path,
        }
    );
}

#[test]
fn a_file_that_is_not_a_key_starts_a_guest_with_the_reason() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("id_ed25519");
    write_private_key(&path, "not a key\n");
    let (session, status, _) = session_for_key(&path);
    assert_eq!(session, Session::Guest);
    assert!(
        matches!(
            status,
            KeyStatus::Unreadable {
                error: KeyError::Parse(_),
                ..
            }
        ),
        "{status:?}"
    );
}

#[test]
fn a_directory_at_the_key_path_is_unreadable() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("id_ed25519");
    std::fs::create_dir(&path).unwrap();
    let (session, status, _) = session_for_key(&path);
    assert_eq!(session, Session::Guest);
    assert_eq!(
        status,
        KeyStatus::Unreadable {
            error: KeyError::NotARegularFile,
            path,
        }
    );
}

#[test]
fn a_usable_key_is_handed_on_with_its_fingerprint() {
    let dir = tempfile::tempdir().unwrap();
    let path = golden_key_in(dir.path());
    let (_, _, key) = session_for_key(&path);
    assert_eq!(key.expect("the key").fingerprint(), GOLDEN_FINGERPRINT);
}

#[test]
fn no_key_is_handed_on_without_a_key_file_or_with_an_unusable_one() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("id_ed25519");
    assert!(session_for_key(&missing).2.is_none());
    write_private_key(&missing, "not a key\n");
    assert!(session_for_key(&missing).2.is_none());
}

// --- build_app ----------------------------------------------------------------

fn startup() -> Startup {
    Startup {
        session: Session::Guest,
        key_status: KeyStatus::Absent,
        key_path: PathBuf::from("/k/id_ed25519"),
        config: LoadedConfig {
            prefs: Prefs::default(),
            warning: None,
        },
        truecolor: true,
        host_label: None,
        last_visits: LastVisits::new(),
        now: NOW,
        offset: UtcOffset::UTC,
        size: (100, 30),
    }
}

#[test]
fn the_app_gets_the_key_path_and_status() {
    let (app, _) = build_app(Startup {
        key_status: KeyStatus::Present,
        ..startup()
    });
    assert_eq!(app.key_path(), Path::new("/k/id_ed25519"));
    assert_eq!(*app.key_status(), KeyStatus::Present);
}

#[test]
fn the_app_gets_the_host_label() {
    let (app, _) = build_app(Startup {
        host_label: Some("127.0.0.1:8080".into()),
        ..startup()
    });
    assert_eq!(app.host_label(), Some("127.0.0.1:8080"));
}

#[test]
fn truecolor_is_the_argument_and_not_the_config() {
    for flag in [true, false] {
        let (app, _) = build_app(Startup {
            truecolor: flag,
            config: LoadedConfig {
                prefs: Prefs {
                    truecolor: !flag,
                    ..Prefs::default()
                },
                warning: None,
            },
            ..startup()
        });
        assert_eq!(app.prefs().truecolor, flag);
    }
}

#[test]
fn the_other_prefs_come_from_the_config() {
    let (app, _) = build_app(Startup {
        config: LoadedConfig {
            prefs: Prefs {
                theme: Theme::Light,
                ascii: true,
                ..Prefs::default()
            },
            warning: None,
        },
        ..startup()
    });
    assert_eq!(app.prefs().theme, Theme::Light);
    assert!(app.prefs().ascii);
}

#[test]
fn a_config_warning_becomes_a_toast() {
    let (app, _) = build_app(Startup {
        config: LoadedConfig {
            prefs: Prefs::default(),
            warning: Some("config.toml could not be parsed".into()),
        },
        ..startup()
    });
    let toast = app.toast().expect("a toast");
    assert_eq!(toast.kind, ToastKind::Error);
    assert!(toast.text.contains("config.toml could not be parsed"));
}

#[test]
fn no_warning_means_no_toast() {
    let (app, _) = build_app(startup());
    assert!(app.toast().is_none());
}

#[test]
fn last_visits_are_set_per_tab() {
    let (app, _) = build_app(Startup {
        last_visits: LastVisits::from([
            (LogTab::Following, "f1".to_string()),
            (LogTab::Latest, "l1".to_string()),
        ]),
        ..startup()
    });
    assert_eq!(app.last_visit_of(LogTab::Following), Some("f1"));
    assert_eq!(app.last_visit_of(LogTab::Latest), Some("l1"));
    assert_eq!(app.last_visit_of(LogTab::Trending), None);
}

#[test]
fn a_restoring_session_starts_by_asking_for_the_account() {
    let (_, effects) = build_app(Startup {
        session: Session::Restoring,
        key_status: KeyStatus::Present,
        ..startup()
    });
    let signed = effects.iter().position(|e| *e == Effect::UseSignedClient);
    let fetch = effects
        .iter()
        .position(|e| matches!(e, Effect::FetchMe { .. }));
    assert!(signed.is_some() && fetch.is_some(), "{effects:?}");
    assert!(signed < fetch, "{effects:?}");
}

#[test]
fn the_app_knows_the_terminal_size_when_it_starts() {
    let (app, _) = build_app(Startup {
        size: (91, 33),
        ..startup()
    });
    assert_eq!(app.size(), (91, 33));
}
