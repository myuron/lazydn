use std::path::{Path, PathBuf};

use crate::app::{LogTab, Prefs, Theme};
use std::collections::HashMap;
use std::ffi::OsString;

use crate::config::{
    BaseUrl, ConfigError, Env, LastVisits, base_url, config_dir, is_truecolor, key_path,
    load_last_visits, load_prefs, save_last_visits, save_prefs, state_dir,
};

fn p(path: &str) -> Option<PathBuf> {
    Some(PathBuf::from(path))
}

fn names_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

fn visits(pairs: &[(LogTab, &str)]) -> LastVisits {
    pairs.iter().map(|(t, id)| (*t, id.to_string())).collect()
}

// --- config_dir ---------------------------------------------------------------

#[test]
fn config_dir_is_the_override_when_set() {
    let env = Env {
        config_dir: p("/custom"),
        xdg_config_home: p("/xdg"),
        home: p("/home/u"),
        ..Env::default()
    };
    assert_eq!(config_dir(&env), Ok(PathBuf::from("/custom")));
}

#[test]
fn config_dir_needs_no_home_when_overridden() {
    let env = Env {
        config_dir: p("/custom"),
        ..Env::default()
    };
    assert_eq!(config_dir(&env), Ok(PathBuf::from("/custom")));
}

#[test]
fn config_dir_is_lazydn_under_xdg_config_home() {
    let env = Env {
        xdg_config_home: p("/xdg"),
        home: p("/home/u"),
        ..Env::default()
    };
    assert_eq!(config_dir(&env), Ok(PathBuf::from("/xdg/lazydn")));
}

#[test]
fn config_dir_falls_back_to_dot_config_under_home() {
    let env = Env {
        home: p("/home/u"),
        ..Env::default()
    };
    assert_eq!(
        config_dir(&env),
        Ok(PathBuf::from("/home/u/.config/lazydn"))
    );
}

#[test]
fn config_dir_ignores_a_relative_xdg_config_home() {
    let env = Env {
        xdg_config_home: p("relative/dir"),
        home: p("/home/u"),
        ..Env::default()
    };
    assert_eq!(
        config_dir(&env),
        Ok(PathBuf::from("/home/u/.config/lazydn"))
    );
}

#[test]
fn config_dir_is_an_error_without_any_base() {
    assert_eq!(config_dir(&Env::default()), Err(ConfigError::NoHome));
}

#[test]
fn config_dir_is_an_error_when_only_a_relative_xdg_remains() {
    let env = Env {
        xdg_config_home: p("relative"),
        ..Env::default()
    };
    assert_eq!(config_dir(&env), Err(ConfigError::NoHome));
}

#[test]
fn config_dir_does_not_follow_the_state_override() {
    let env = Env {
        state_dir: p("/state"),
        home: p("/home/u"),
        ..Env::default()
    };
    assert_eq!(
        config_dir(&env),
        Ok(PathBuf::from("/home/u/.config/lazydn"))
    );
}

// --- state_dir ----------------------------------------------------------------

#[test]
fn state_dir_is_the_override_when_set() {
    let env = Env {
        state_dir: p("/custom-state"),
        xdg_state_home: p("/xdg-state"),
        home: p("/home/u"),
        ..Env::default()
    };
    assert_eq!(state_dir(&env), Ok(PathBuf::from("/custom-state")));
}

#[test]
fn state_dir_is_lazydn_under_xdg_state_home() {
    let env = Env {
        xdg_state_home: p("/xdg-state"),
        home: p("/home/u"),
        ..Env::default()
    };
    assert_eq!(state_dir(&env), Ok(PathBuf::from("/xdg-state/lazydn")));
}

#[test]
fn state_dir_falls_back_to_local_state_under_home() {
    let env = Env {
        home: p("/home/u"),
        ..Env::default()
    };
    assert_eq!(
        state_dir(&env),
        Ok(PathBuf::from("/home/u/.local/state/lazydn"))
    );
}

#[test]
fn state_dir_ignores_a_relative_xdg_state_home() {
    let env = Env {
        xdg_state_home: p("relative"),
        home: p("/home/u"),
        ..Env::default()
    };
    assert_eq!(
        state_dir(&env),
        Ok(PathBuf::from("/home/u/.local/state/lazydn"))
    );
}

#[test]
fn state_dir_is_an_error_without_any_base() {
    assert_eq!(state_dir(&Env::default()), Err(ConfigError::NoHome));
}

#[test]
fn state_dir_does_not_follow_the_config_override() {
    let env = Env {
        config_dir: p("/custom"),
        home: p("/home/u"),
        ..Env::default()
    };
    assert_eq!(
        state_dir(&env),
        Ok(PathBuf::from("/home/u/.local/state/lazydn"))
    );
}

#[test]
fn key_path_is_id_ed25519_in_the_config_dir() {
    assert_eq!(
        key_path(Path::new("/c/lazydn")),
        PathBuf::from("/c/lazydn/id_ed25519")
    );
}

// --- base_url -----------------------------------------------------------------

fn env_with_url(url: &str) -> Env {
    Env {
        base_url: Some(url.into()),
        ..Env::default()
    }
}

#[test]
fn base_url_defaults_to_production_without_a_label() {
    assert_eq!(
        base_url(&Env::default()),
        Ok(BaseUrl {
            url: "https://dev-null.sh".into(),
            host_label: None,
        })
    );
}

#[test]
fn base_url_naming_production_explicitly_has_no_label() {
    assert_eq!(
        base_url(&env_with_url("https://dev-null.sh")),
        Ok(BaseUrl {
            url: "https://dev-null.sh".into(),
            host_label: None,
        })
    );
}

#[test]
fn base_url_accepted_values_carry_a_host_label() {
    let cases = [
        ("https://staging.example.com", "staging.example.com"),
        ("http://127.0.0.1:8080", "127.0.0.1:8080"),
        ("http://localhost:3000", "localhost:3000"),
        ("http://[::1]:9000", "[::1]:9000"),
    ];
    for (input, label) in cases {
        assert_eq!(
            base_url(&env_with_url(input)),
            Ok(BaseUrl {
                url: input.into(),
                host_label: Some(label.into()),
            }),
            "{input}"
        );
    }
}

#[test]
fn base_url_rejected_values_are_startup_errors() {
    let cases = [
        "http://dev-null.sh",
        "http://example.com",
        "http://localhost.evil.example",
        "http://127.0.0.1.evil.example",
        "ftp://example.com",
        "file:///etc/passwd",
        "not a url",
        "https://user:secret@example.com",
        " https://example.com",
        "https://example.com ",
        "http://localhost.",
        "https://localhost@evil.example",
    ];
    for input in cases {
        assert_eq!(
            base_url(&env_with_url(input)),
            Err(ConfigError::InvalidBaseUrl(input.into())),
            "{input:?}"
        );
    }
}

// --- truecolor ----------------------------------------------------------------

#[test]
fn truecolor_follows_colorterm() {
    let cases = [
        (Some("truecolor"), true),
        (Some("24bit"), true),
        (Some("256color"), false),
        (Some("yes"), false),
        (Some(""), false),
        (None, false),
    ];
    for (value, expected) in cases {
        assert_eq!(is_truecolor(value), expected, "{value:?}");
    }
}

// --- config.toml --------------------------------------------------------------

fn config_with(content: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("config.toml"), content).unwrap();
    dir
}

#[test]
fn missing_config_gives_defaults_and_creates_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("lazydn");
    let loaded = load_prefs(&config);
    assert_eq!(loaded.prefs, Prefs::default());
    assert_eq!(loaded.warning, None);
    assert!(!config.exists());
}

#[test]
fn config_values_are_loaded() {
    let dir = config_with(
        "theme = \"light\"\ncollapse_nav = true\nlane_dimming = true\nreply_preview_limit = 10\nascii = true\n",
    );
    let loaded = load_prefs(dir.path());
    assert_eq!(
        loaded.prefs,
        Prefs {
            theme: Theme::Light,
            collapse_nav: true,
            lane_dimming: true,
            reply_preview_limit: 10,
            ascii: true,
            ..Prefs::default()
        }
    );
    assert_eq!(loaded.warning, None);
}

#[test]
fn config_with_some_fields_leaves_the_rest_at_defaults() {
    let dir = config_with("theme = \"light\"\n");
    let loaded = load_prefs(dir.path());
    assert_eq!(
        loaded.prefs,
        Prefs {
            theme: Theme::Light,
            ..Prefs::default()
        }
    );
}

#[test]
fn one_field_of_the_wrong_type_does_not_spoil_the_others() {
    let dir = config_with("theme = \"light\"\ncollapse_nav = \"yes\"\nascii = true\n");
    let prefs = load_prefs(dir.path()).prefs;
    assert_eq!(prefs.theme, Theme::Light);
    assert!(prefs.ascii);
    assert_eq!(prefs.collapse_nav, Prefs::default().collapse_nav);
}

#[test]
fn an_unknown_theme_falls_back_alone() {
    let dir = config_with("theme = \"solarized\"\nlane_dimming = true\n");
    let loaded = load_prefs(dir.path());
    assert!(loaded.warning.is_some());
    let prefs = loaded.prefs;
    assert_eq!(prefs.theme, Theme::Dark);
    assert!(prefs.lane_dimming);
}

#[test]
fn unknown_keys_are_ignored() {
    let dir = config_with("future_option = 1\nascii = true\n");
    let prefs = load_prefs(dir.path()).prefs;
    assert!(prefs.ascii);
}

#[test]
fn reply_preview_limit_accepts_only_the_listed_counts() {
    let cases = [
        ("1", 1),
        ("3", 3),
        ("5", 5),
        ("10", 10),
        ("15", 15),
        ("20", 20),
        ("25", 25),
        ("0", 3),
        ("2", 3),
        ("4", 3),
        ("30", 3),
        ("-5", 3),
        ("\"5\"", 3),
    ];
    for (written, expected) in cases {
        let dir = config_with(&format!("reply_preview_limit = {written}\n"));
        assert_eq!(
            load_prefs(dir.path()).prefs.reply_preview_limit,
            expected,
            "{written}"
        );
    }
}

#[test]
fn an_unparsable_config_gives_defaults_and_a_warning() {
    let dir = config_with("this is = = not toml [\n");
    let loaded = load_prefs(dir.path());
    assert_eq!(loaded.prefs, Prefs::default());
    assert!(loaded.warning.is_some());
}

// --- state.toml ---------------------------------------------------------------

#[test]
fn missing_state_is_empty() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(load_last_visits(dir.path()), LastVisits::new());
}

#[test]
fn saved_visits_read_back() {
    let dir = tempfile::tempdir().unwrap();
    let saved = visits(&[(LogTab::Following, "f1"), (LogTab::Latest, "l1")]);
    save_last_visits(dir.path(), &saved).unwrap();
    assert_eq!(load_last_visits(dir.path()), saved);
}

#[test]
fn the_state_file_is_state_toml_with_a_last_visit_table() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("state.toml"),
        "[last_visit]\nfollowing = \"f1\"\nlatest = \"l1\"\n",
    )
    .unwrap();
    assert_eq!(
        load_last_visits(dir.path()),
        visits(&[(LogTab::Following, "f1"), (LogTab::Latest, "l1")])
    );
}

#[test]
fn saving_leaves_no_temporary_file() {
    let dir = tempfile::tempdir().unwrap();
    save_last_visits(dir.path(), &visits(&[(LogTab::Following, "f1")])).unwrap();
    save_last_visits(dir.path(), &visits(&[(LogTab::Following, "f2")])).unwrap();
    assert_eq!(names_in(dir.path()), vec!["state.toml".to_string()]);
}

#[test]
fn saving_creates_a_missing_directory() {
    let dir = tempfile::tempdir().unwrap();
    let nested = dir.path().join("a").join("lazydn");
    save_last_visits(&nested, &visits(&[(LogTab::Latest, "l1")])).unwrap();
    assert_eq!(load_last_visits(&nested), visits(&[(LogTab::Latest, "l1")]));
}

#[test]
fn saving_overwrites_only_the_tabs_it_was_given() {
    let dir = tempfile::tempdir().unwrap();
    save_last_visits(
        dir.path(),
        &visits(&[(LogTab::Following, "old-f"), (LogTab::Latest, "old-l")]),
    )
    .unwrap();
    save_last_visits(dir.path(), &visits(&[(LogTab::Latest, "new-l")])).unwrap();
    assert_eq!(
        load_last_visits(dir.path()),
        visits(&[(LogTab::Following, "old-f"), (LogTab::Latest, "new-l")])
    );
}

#[test]
fn saving_nothing_keeps_the_stored_visits() {
    let dir = tempfile::tempdir().unwrap();
    let saved = visits(&[(LogTab::Following, "f1")]);
    save_last_visits(dir.path(), &saved).unwrap();
    save_last_visits(dir.path(), &LastVisits::new()).unwrap();
    assert_eq!(load_last_visits(dir.path()), saved);
}

#[test]
fn a_corrupt_state_is_ignored_on_load() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("state.toml"), "= = garbage [").unwrap();
    assert_eq!(load_last_visits(dir.path()), LastVisits::new());
}

#[test]
fn a_corrupt_state_is_replaced_on_save() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("state.toml"), "= = garbage [").unwrap();
    save_last_visits(dir.path(), &visits(&[(LogTab::Following, "f1")])).unwrap();
    assert_eq!(
        load_last_visits(dir.path()),
        visits(&[(LogTab::Following, "f1")])
    );
}

#[test]
fn a_mistyped_entry_is_dropped_and_the_others_stay() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("state.toml"),
        "[last_visit]\nfollowing = 5\nlatest = \"l1\"\n",
    )
    .unwrap();
    assert_eq!(
        load_last_visits(dir.path()),
        visits(&[(LogTab::Latest, "l1")])
    );
}

// --- base_url, more values ----------------------------------------------------

#[test]
fn base_url_host_is_lowercased_in_the_label() {
    let loaded = base_url(&env_with_url("https://Staging.Example.COM")).unwrap();
    assert_eq!(loaded.host_label, Some("staging.example.com".into()));
}

#[test]
fn base_url_production_with_a_trailing_slash_has_no_label() {
    let loaded = base_url(&env_with_url("https://dev-null.sh/")).unwrap();
    assert_eq!(loaded.host_label, None);
}

#[test]
fn base_url_accepts_any_address_in_127_0_0_0_8() {
    let loaded = base_url(&env_with_url("http://127.0.0.2:8080")).unwrap();
    assert_eq!(loaded.host_label, Some("127.0.0.2:8080".into()));
}

// --- Env::from_lookup ---------------------------------------------------------

fn lookup(pairs: &[(&str, &str)]) -> Result<Env, ConfigError> {
    let vars: HashMap<String, OsString> = pairs
        .iter()
        .map(|(k, v)| (k.to_string(), OsString::from(v)))
        .collect();
    Env::from_lookup(|name| vars.get(name).cloned())
}

#[test]
fn from_lookup_reads_every_variable() {
    let env = lookup(&[
        ("LAZYDN_CONFIG_DIR", "/c"),
        ("LAZYDN_STATE_DIR", "/s"),
        ("XDG_CONFIG_HOME", "/xc"),
        ("XDG_STATE_HOME", "/xs"),
        ("HOME", "/h"),
        ("LAZYDN_BASE_URL", "https://example.com"),
        ("COLORTERM", "truecolor"),
    ])
    .unwrap();
    assert_eq!(
        env,
        Env {
            config_dir: p("/c"),
            state_dir: p("/s"),
            xdg_config_home: p("/xc"),
            xdg_state_home: p("/xs"),
            home: p("/h"),
            base_url: Some("https://example.com".into()),
            colorterm: Some("truecolor".into()),
        }
    );
}

#[test]
fn from_lookup_treats_empty_values_as_unset() {
    let env = lookup(&[
        ("LAZYDN_BASE_URL", ""),
        ("HOME", ""),
        ("LAZYDN_CONFIG_DIR", ""),
        ("COLORTERM", "24bit"),
    ])
    .unwrap();
    assert_eq!(env.base_url, None);
    assert_eq!(env.home, None);
    assert_eq!(env.config_dir, None);
    assert_eq!(env.colorterm, Some("24bit".into()));
    assert_eq!(config_dir(&env), Err(ConfigError::NoHome));
    assert_eq!(base_url(&env).unwrap().host_label, None);
}

#[test]
fn from_lookup_refuses_a_relative_config_dir() {
    assert_eq!(
        lookup(&[("LAZYDN_CONFIG_DIR", "relative/dir")]),
        Err(ConfigError::RelativeDir("LAZYDN_CONFIG_DIR".into()))
    );
}

#[test]
fn from_lookup_refuses_a_relative_state_dir() {
    assert_eq!(
        lookup(&[("LAZYDN_STATE_DIR", "relative/dir")]),
        Err(ConfigError::RelativeDir("LAZYDN_STATE_DIR".into()))
    );
}

#[test]
fn state_dir_is_an_error_when_only_a_relative_xdg_remains() {
    let env = Env {
        xdg_state_home: p("relative"),
        ..Env::default()
    };
    assert_eq!(state_dir(&env), Err(ConfigError::NoHome));
}

// --- config.toml, more --------------------------------------------------------

#[test]
fn truecolor_in_the_config_file_is_ignored() {
    let dir = config_with("truecolor = false\nascii = true\n");
    let prefs = load_prefs(dir.path()).prefs;
    assert!(prefs.truecolor);
    assert!(prefs.ascii);
}

#[test]
fn a_mistyped_field_is_reported_in_the_warning() {
    let dir = config_with("collapse_nav = \"yes\"\n");
    assert!(load_prefs(dir.path()).warning.is_some());
}

#[test]
fn a_clean_config_has_no_warning() {
    let dir = config_with("collapse_nav = true\n");
    assert_eq!(load_prefs(dir.path()).warning, None);
}

#[test]
fn a_trending_entry_in_the_state_file_is_ignored() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("state.toml"),
        "[last_visit]\nfollowing = \"f1\"\ntrending = \"t1\"\n",
    )
    .unwrap();
    assert_eq!(
        load_last_visits(dir.path()),
        visits(&[(LogTab::Following, "f1")])
    );
}

#[test]
fn a_disallowed_reply_preview_limit_is_reported_in_the_warning() {
    let dir = config_with("reply_preview_limit = 4\n");
    let loaded = load_prefs(dir.path());
    assert_eq!(loaded.prefs.reply_preview_limit, 3);
    assert!(loaded.warning.is_some());
}

#[test]
fn an_allowed_reply_preview_limit_gives_no_warning() {
    let dir = config_with("reply_preview_limit = 5\n");
    assert_eq!(load_prefs(dir.path()).warning, None);
}

#[test]
fn base_url_with_a_path_query_or_fragment_is_rejected() {
    for input in [
        "https://x.example/api",
        "https://x.example/?a=1",
        "https://x.example/#f",
    ] {
        assert_eq!(
            base_url(&env_with_url(input)),
            Err(ConfigError::InvalidBaseUrl(input.into())),
            "{input}"
        );
    }
}

#[cfg(unix)]
#[test]
fn from_lookup_refuses_a_base_url_that_is_not_utf8() {
    use std::os::unix::ffi::OsStringExt;
    let bad = OsString::from_vec(b"https://x.example/\xff".to_vec());
    let result = Env::from_lookup(|name| (name == "LAZYDN_BASE_URL").then(|| bad.clone()));
    assert!(
        matches!(result, Err(ConfigError::InvalidBaseUrl(_))),
        "{result:?}"
    );
}

#[test]
fn a_relative_home_is_ignored() {
    let env = Env {
        home: p("relative"),
        ..Env::default()
    };
    assert_eq!(config_dir(&env), Err(ConfigError::NoHome));
    assert_eq!(state_dir(&env), Err(ConfigError::NoHome));
}

// --- saving the Appearance settings ------------------------------------------

mod save {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    const PARSE_TEXT: &str = "config.toml could not be parsed; settings not saved";
    const READ_TEXT: &str = "config.toml could not be read; settings not saved";
    const SYMLINK_TEXT: &str = "config.toml is a symlink; settings not saved";

    fn changed() -> Prefs {
        Prefs {
            theme: Theme::Light,
            collapse_nav: true,
            lane_dimming: true,
            reply_preview_limit: 15,
            ascii: true,
            ..Prefs::default()
        }
    }

    fn mode(path: &Path) -> u32 {
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    fn table_of(dir: &Path) -> toml::Table {
        std::fs::read_to_string(dir.join("config.toml"))
            .unwrap()
            .parse()
            .unwrap()
    }

    #[test]
    fn the_five_values_are_written_and_read_back_by_load_prefs() {
        let dir = tempfile::tempdir().unwrap();

        assert_eq!(save_prefs(dir.path(), &changed()), Ok(()));

        let table = table_of(dir.path());
        assert_eq!(table["theme"].as_str(), Some("light"));
        assert_eq!(table["collapse_nav"].as_bool(), Some(true));
        assert_eq!(table["lane_dimming"].as_bool(), Some(true));
        assert_eq!(table["reply_preview_limit"].as_integer(), Some(15));
        assert_eq!(table["ascii"].as_bool(), Some(true));
        let loaded = load_prefs(dir.path());
        assert_eq!(loaded.warning, None);
        assert_eq!(loaded.prefs, changed());
    }

    #[test]
    fn the_runtime_only_truecolor_is_not_written() {
        let dir = tempfile::tempdir().unwrap();

        save_prefs(
            dir.path(),
            &Prefs {
                truecolor: false,
                ..changed()
            },
        )
        .unwrap();

        assert!(!table_of(dir.path()).contains_key("truecolor"));
    }

    #[test]
    fn a_missing_file_is_created_with_mode_0600() {
        let dir = tempfile::tempdir().unwrap();

        save_prefs(dir.path(), &changed()).unwrap();

        assert_eq!(mode(&dir.path().join("config.toml")), 0o600);
        assert_eq!(names_in(dir.path()), vec!["config.toml"]);
    }

    #[test]
    fn a_missing_directory_is_created_with_mode_0700() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("nested").join("lazydn");

        save_prefs(&config, &changed()).unwrap();

        assert_eq!(mode(&config), 0o700);
        assert_eq!(mode(&config.join("config.toml")), 0o600);
    }

    #[test]
    fn an_existing_directory_keeps_its_mode() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();

        save_prefs(dir.path(), &changed()).unwrap();

        assert_eq!(mode(dir.path()), 0o755);
        assert!(dir.path().join("config.toml").is_file());
    }

    #[test]
    fn an_existing_file_becomes_0600() {
        let dir = config_with("ascii = false\n");
        let path = dir.path().join("config.toml");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

        save_prefs(dir.path(), &changed()).unwrap();

        assert_eq!(mode(&path), 0o600);
    }

    #[test]
    fn keys_that_are_not_ours_survive_with_their_values() {
        let dir = config_with(
            "future_option = 1\ntruecolor = false\ntheme = \"dark\"\n[extra]\nname = \"kept\"\n",
        );

        save_prefs(dir.path(), &changed()).unwrap();

        let table = table_of(dir.path());
        assert_eq!(table["future_option"].as_integer(), Some(1));
        assert_eq!(table["truecolor"].as_bool(), Some(false));
        assert_eq!(table["extra"]["name"].as_str(), Some("kept"));
        assert_eq!(table["theme"].as_str(), Some("light"));
    }

    #[test]
    fn a_mistyped_known_key_is_overwritten_with_the_value() {
        let dir = config_with("collapse_nav = \"yes\"\nreply_preview_limit = 99\n");

        save_prefs(dir.path(), &changed()).unwrap();

        let table = table_of(dir.path());
        assert_eq!(table["collapse_nav"].as_bool(), Some(true));
        assert_eq!(table["reply_preview_limit"].as_integer(), Some(15));
    }

    #[test]
    fn the_file_is_read_again_at_every_save() {
        let dir = config_with("first = 1\n");
        save_prefs(dir.path(), &Prefs::default()).unwrap();
        let path = dir.path().join("config.toml");
        let mut text = std::fs::read_to_string(&path).unwrap();
        text.push_str("added_by_hand = 2\n");
        std::fs::write(&path, text).unwrap();

        save_prefs(dir.path(), &changed()).unwrap();

        let table = table_of(dir.path());
        assert_eq!(table["first"].as_integer(), Some(1));
        assert_eq!(table["added_by_hand"].as_integer(), Some(2));
        assert_eq!(table["theme"].as_str(), Some("light"));
    }

    #[test]
    fn no_temporary_file_is_left_behind() {
        let dir = config_with("a = 1\n");

        save_prefs(dir.path(), &changed()).unwrap();
        save_prefs(dir.path(), &Prefs::default()).unwrap();

        assert_eq!(names_in(dir.path()), vec!["config.toml"]);
        assert_eq!(load_prefs(dir.path()).prefs, Prefs::default());
        assert_eq!(table_of(dir.path())["a"].as_integer(), Some(1));
    }

    #[test]
    fn a_file_that_does_not_parse_is_refused_and_left_as_it_was() {
        let dir = config_with("theme = [unclosed\n");

        assert_eq!(
            save_prefs(dir.path(), &changed()),
            Err(PARSE_TEXT.to_string())
        );

        assert_eq!(
            std::fs::read_to_string(dir.path().join("config.toml")).unwrap(),
            "theme = [unclosed\n"
        );
        assert_eq!(names_in(dir.path()), vec!["config.toml"]);
    }

    #[test]
    fn a_file_that_cannot_be_read_is_refused_and_left_as_it_was() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("config.toml")).unwrap();

        assert_eq!(
            save_prefs(dir.path(), &changed()),
            Err(READ_TEXT.to_string())
        );

        assert!(dir.path().join("config.toml").is_dir());
    }

    #[test]
    fn a_symlinked_file_is_refused_and_neither_link_nor_target_changes() {
        let dir = tempfile::tempdir().unwrap();
        let managed = tempfile::tempdir().unwrap();
        let target = managed.path().join("real.toml");
        std::fs::write(&target, "ascii = false\n").unwrap();
        let link = dir.path().join("config.toml");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        assert_eq!(
            save_prefs(dir.path(), &changed()),
            Err(SYMLINK_TEXT.to_string())
        );

        assert!(link.symlink_metadata().unwrap().file_type().is_symlink());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "ascii = false\n");
        assert_eq!(names_in(dir.path()), vec!["config.toml"]);
    }

    #[test]
    fn a_dangling_symlink_is_refused_too() {
        let dir = tempfile::tempdir().unwrap();
        let link = dir.path().join("config.toml");
        std::os::unix::fs::symlink(dir.path().join("nowhere.toml"), &link).unwrap();

        assert_eq!(
            save_prefs(dir.path(), &changed()),
            Err(SYMLINK_TEXT.to_string())
        );

        assert!(!dir.path().join("nowhere.toml").exists());
    }

    #[test]
    fn a_symlinked_directory_is_written_through() {
        let real = tempfile::tempdir().unwrap();
        let holder = tempfile::tempdir().unwrap();
        let link = holder.path().join("lazydn");
        std::os::unix::fs::symlink(real.path(), &link).unwrap();

        assert_eq!(save_prefs(&link, &changed()), Ok(()));

        assert!(link.symlink_metadata().unwrap().file_type().is_symlink());
        assert_eq!(load_prefs(real.path()).prefs, changed());
    }

    #[test]
    fn a_directory_that_cannot_be_made_gives_a_write_error() {
        let dir = tempfile::tempdir().unwrap();
        let blocker = dir.path().join("lazydn");
        std::fs::write(&blocker, "a file").unwrap();

        let error = save_prefs(&blocker.join("inner"), &changed()).unwrap_err();

        assert!(
            error.starts_with("settings could not be saved ("),
            "{error}"
        );
        assert_eq!(std::fs::read_to_string(&blocker).unwrap(), "a file");
    }

    #[test]
    fn an_empty_file_is_written_as_an_empty_table_would_be() {
        let dir = config_with("");

        assert_eq!(save_prefs(dir.path(), &changed()), Ok(()));

        assert_eq!(load_prefs(dir.path()).prefs, changed());
        assert_eq!(table_of(dir.path()).len(), 5);
    }

    #[test]
    fn a_file_that_is_not_utf8_counts_as_unreadable_and_is_left_as_it_was() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, [0x74, 0x3d, 0xff, 0xfe, 0x0a]).unwrap();

        assert_eq!(
            save_prefs(dir.path(), &changed()),
            Err(READ_TEXT.to_string())
        );

        assert_eq!(
            std::fs::read(&path).unwrap(),
            vec![0x74, 0x3d, 0xff, 0xfe, 0x0a]
        );
    }

    #[test]
    fn temporary_names_that_are_taken_are_skipped_and_what_they_point_at_is_not_touched() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let victim = outside.path().join("victim");
        std::fs::write(&victim, "precious").unwrap();
        let name = |n: u32| {
            dir.path()
                .join(format!(".config.toml.tmp-{}-{n}", std::process::id()))
        };
        std::os::unix::fs::symlink(&victim, name(0)).unwrap();
        std::fs::write(name(1), "taken").unwrap();
        std::os::unix::fs::symlink(outside.path().join("nowhere"), name(2)).unwrap();
        std::os::unix::fs::symlink(&victim, name(3)).unwrap();

        assert_eq!(save_prefs(dir.path(), &changed()), Ok(()));

        assert_eq!(std::fs::read_to_string(&victim).unwrap(), "precious");
        assert_eq!(std::fs::read_to_string(name(1)).unwrap(), "taken");
        assert!(!outside.path().join("nowhere").exists());
        assert_eq!(load_prefs(dir.path()).prefs, changed());
        assert!(
            std::fs::symlink_metadata(dir.path().join("config.toml"))
                .unwrap()
                .file_type()
                .is_file()
        );
    }
}
