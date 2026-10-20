use std::fs;
use std::path::Path;

use crate::drafts::{
    DraftKey, Drafts, REPLY_QUOTE_CAP, drafts_path, is_valid_account_id, load_drafts, save_drafts,
};
use crate::editor::Editor;

const ACCOUNT: &str = "0190a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b";
const OTHER_ACCOUNT: &str = "0190ffff-0000-7000-8000-000000000001";

// --- helpers ------------------------------------------------------------------

fn ed(text: &str) -> Editor {
    Editor::from_text(text)
}

fn reply(n: usize) -> DraftKey {
    DraftKey::Reply(format!("a{n}"))
}

fn quote(n: usize) -> DraftKey {
    DraftKey::Quote(format!("b{n}"))
}

fn file_of(dir: &Path, account: &str) -> std::path::PathBuf {
    dir.join(format!("drafts-{account}.toml"))
}

fn write_file(dir: &Path, account: &str, text: &str) {
    fs::create_dir_all(dir).unwrap();
    fs::write(file_of(dir, account), text).unwrap();
}

fn entries(dir: &Path, account: &str) -> Vec<(DraftKey, String)> {
    let drafts = load_drafts(dir, account);
    drafts
        .keys()
        .into_iter()
        .map(|k| {
            let text = drafts.text(&k).unwrap_or_default();
            (k, text)
        })
        .collect()
}

fn file_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

// --- Drafts: what is kept -------------------------------------------------------

#[test]
fn a_draft_is_found_by_its_key() {
    let mut drafts = Drafts::default();
    drafts.set(DraftKey::New, ed("hello"));
    drafts.set(reply(1), ed("re"));

    assert_eq!(drafts.text(&DraftKey::New), Some("hello".to_string()));
    assert_eq!(drafts.text(&reply(1)), Some("re".to_string()));
    assert_eq!(drafts.text(&reply(2)), None);
    assert_eq!(drafts.text(&quote(1)), None);
    assert_eq!(
        drafts.get(&DraftKey::New).map(Editor::text),
        Some("hello".to_string())
    );
    assert_eq!(drafts.len(), 2);
    assert!(!drafts.is_empty());
}

#[test]
fn a_reply_and_a_quote_of_the_same_commit_are_different_drafts() {
    let mut drafts = Drafts::default();
    drafts.set(DraftKey::Reply("c1".into()), ed("re"));
    drafts.set(DraftKey::Quote("c1".into()), ed("qt"));

    assert_eq!(
        drafts.text(&DraftKey::Reply("c1".into())),
        Some("re".to_string())
    );
    assert_eq!(
        drafts.text(&DraftKey::Quote("c1".into())),
        Some("qt".to_string())
    );
}

#[test]
fn setting_an_empty_editor_removes_the_draft() {
    let mut drafts = Drafts::default();
    drafts.set(DraftKey::New, ed("hello"));
    assert_eq!(drafts.text(&DraftKey::New), Some("hello".to_string()));

    drafts.set(DraftKey::New, Editor::new());

    assert_eq!(drafts.text(&DraftKey::New), None);
    assert!(drafts.is_empty());
}

#[test]
fn setting_an_empty_editor_for_an_absent_key_adds_nothing() {
    let mut drafts = Drafts::default();

    drafts.set(reply(1), Editor::new());

    assert!(drafts.is_empty());
    assert_eq!(drafts.keys(), Vec::<DraftKey>::new());
}

#[test]
fn removing_a_draft_leaves_the_others() {
    let mut drafts = Drafts::default();
    drafts.set(DraftKey::New, ed("a"));
    drafts.set(reply(1), ed("b"));

    drafts.remove(&DraftKey::New);

    assert_eq!(drafts.text(&DraftKey::New), None);
    assert_eq!(drafts.text(&reply(1)), Some("b".to_string()));
}

#[test]
fn keys_are_listed_oldest_first_and_setting_again_makes_a_draft_the_newest() {
    let mut drafts = Drafts::default();
    drafts.set(reply(1), ed("a"));
    drafts.set(DraftKey::New, ed("b"));
    drafts.set(reply(2), ed("c"));
    assert_eq!(drafts.keys(), vec![reply(1), DraftKey::New, reply(2)]);

    drafts.set(reply(1), ed("a2"));

    assert_eq!(drafts.keys(), vec![DraftKey::New, reply(2), reply(1)]);
    assert_eq!(drafts.text(&reply(1)), Some("a2".to_string()));
}

#[test]
fn more_than_50_reply_drafts_drop_the_oldest() {
    let mut drafts = Drafts::default();
    for n in 0..=REPLY_QUOTE_CAP {
        drafts.set(reply(n), ed("x"));
    }

    assert_eq!(drafts.len(), REPLY_QUOTE_CAP);
    assert_eq!(drafts.text(&reply(0)), None);
    assert_eq!(drafts.text(&reply(1)), Some("x".to_string()));
    assert_eq!(drafts.text(&reply(REPLY_QUOTE_CAP)), Some("x".to_string()));
}

#[test]
fn replies_and_quotes_share_the_cap_and_the_new_draft_is_outside_it() {
    let mut drafts = Drafts::default();
    drafts.set(DraftKey::New, ed("mine"));
    for n in 0..30 {
        drafts.set(reply(n), ed("x"));
        drafts.set(quote(n), ed("x"));
    }

    assert_eq!(drafts.len(), REPLY_QUOTE_CAP + 1);
    assert_eq!(drafts.text(&DraftKey::New), Some("mine".to_string()));
    assert_eq!(drafts.text(&reply(0)), None);
    assert_eq!(drafts.text(&quote(0)), None);
    assert_eq!(drafts.text(&reply(29)), Some("x".to_string()));
    assert_eq!(drafts.text(&quote(29)), Some("x".to_string()));
}

#[test]
fn editing_a_draft_at_the_cap_drops_nothing() {
    let mut drafts = Drafts::default();
    for n in 0..REPLY_QUOTE_CAP {
        drafts.set(reply(n), ed("x"));
    }

    drafts.set(reply(10), ed("edited"));

    assert_eq!(drafts.len(), REPLY_QUOTE_CAP);
    assert_eq!(drafts.text(&reply(0)), Some("x".to_string()));
    assert_eq!(drafts.text(&reply(10)), Some("edited".to_string()));
}

// --- account ids -------------------------------------------------------------

#[test]
fn an_account_id_is_lowercase_hex_and_hyphens_up_to_64_characters() {
    assert!(is_valid_account_id(ACCOUNT));
    assert!(is_valid_account_id("a"));
    assert!(is_valid_account_id(&"f".repeat(64)));
    assert!(is_valid_account_id("0-9-a-f"));
}

#[test]
fn any_other_account_id_is_refused() {
    for id in [
        "",
        "user-myuron",
        "ABCDEF",
        "../evil",
        "a/b",
        "a\\b",
        "a b",
        "a.b",
        "a\0b",
        "あ",
        &"f".repeat(65),
    ] {
        assert!(!is_valid_account_id(id), "{id:?}");
    }
}

#[test]
fn the_file_is_named_after_the_account_in_the_state_dir() {
    let dir = Path::new("/state/lazydn");

    assert_eq!(
        drafts_path(dir, ACCOUNT),
        Some(dir.join(format!("drafts-{ACCOUNT}.toml")))
    );
    assert_eq!(drafts_path(dir, "../evil"), None);
    assert_eq!(drafts_path(dir, "user-myuron"), None);
}

// --- save and load ----------------------------------------------------------------

#[test]
fn saved_drafts_load_back_in_the_same_order() {
    let tmp = tempfile::tempdir().unwrap();
    let mut drafts = Drafts::default();
    drafts.set(DraftKey::New, ed("hello\nworld"));
    drafts.set(reply(1), ed("re: \"quoted\" and `code`"));
    drafts.set(quote(2), ed("あ🙂\n\nlast"));

    save_drafts(tmp.path(), ACCOUNT, &drafts).unwrap();

    assert_eq!(
        entries(tmp.path(), ACCOUNT),
        vec![
            (DraftKey::New, "hello\nworld".to_string()),
            (reply(1), "re: \"quoted\" and `code`".to_string()),
            (quote(2), "あ🙂\n\nlast".to_string()),
        ]
    );
}

#[test]
fn the_file_holds_one_draft_table_per_entry_in_toml() {
    let tmp = tempfile::tempdir().unwrap();
    let mut drafts = Drafts::default();
    drafts.set(DraftKey::New, ed("hello"));
    drafts.set(reply(1), ed("re"));

    save_drafts(tmp.path(), ACCOUNT, &drafts).unwrap();

    let text = fs::read_to_string(file_of(tmp.path(), ACCOUNT)).unwrap();
    let table: toml::Table = text.parse().unwrap();
    let list = table["draft"].as_array().unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list[0]["kind"].as_str(), Some("new"));
    assert_eq!(list[0]["text"].as_str(), Some("hello"));
    assert_eq!(list[1]["kind"].as_str(), Some("reply"));
    assert_eq!(list[1]["id"].as_str(), Some("a1"));
    assert_eq!(list[1]["text"].as_str(), Some("re"));
}

#[test]
fn saving_makes_the_state_dir_when_it_is_missing() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("a/b");
    let mut drafts = Drafts::default();
    drafts.set(DraftKey::New, ed("x"));

    save_drafts(&dir, ACCOUNT, &drafts).unwrap();

    assert!(file_of(&dir, ACCOUNT).is_file());
}

#[cfg(unix)]
#[test]
fn the_file_is_private_to_the_user_and_so_is_a_dir_made_for_it() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("state");
    let mut drafts = Drafts::default();
    drafts.set(DraftKey::New, ed("secret"));

    save_drafts(&dir, ACCOUNT, &drafts).unwrap();

    let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&file_of(&dir, ACCOUNT)), 0o600);
    assert_eq!(mode(&dir), 0o700);
}

#[cfg(unix)]
#[test]
fn replacing_a_file_that_others_could_read_leaves_a_private_one() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let path = file_of(tmp.path(), ACCOUNT);
    fs::write(&path, "").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    let mut drafts = Drafts::default();
    drafts.set(DraftKey::New, ed("secret"));

    save_drafts(tmp.path(), ACCOUNT, &drafts).unwrap();

    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[test]
fn saving_leaves_no_temporary_file_behind() {
    let tmp = tempfile::tempdir().unwrap();
    let mut drafts = Drafts::default();
    drafts.set(DraftKey::New, ed("one"));
    save_drafts(tmp.path(), ACCOUNT, &drafts).unwrap();
    drafts.set(DraftKey::New, ed("two"));
    save_drafts(tmp.path(), ACCOUNT, &drafts).unwrap();

    assert_eq!(
        file_names(tmp.path()),
        vec![format!("drafts-{ACCOUNT}.toml")]
    );
    assert_eq!(
        entries(tmp.path(), ACCOUNT),
        vec![(DraftKey::New, "two".to_string())]
    );
}

#[test]
fn saving_no_drafts_removes_the_file() {
    let tmp = tempfile::tempdir().unwrap();
    let mut drafts = Drafts::default();
    drafts.set(DraftKey::New, ed("one"));
    save_drafts(tmp.path(), ACCOUNT, &drafts).unwrap();
    assert!(file_of(tmp.path(), ACCOUNT).is_file());

    save_drafts(tmp.path(), ACCOUNT, &Drafts::default()).unwrap();

    assert_eq!(file_names(tmp.path()), Vec::<String>::new());
    assert!(load_drafts(tmp.path(), ACCOUNT).is_empty());
}

#[test]
fn saving_no_drafts_when_there_is_no_file_is_not_an_error() {
    let tmp = tempfile::tempdir().unwrap();

    assert!(save_drafts(tmp.path(), ACCOUNT, &Drafts::default()).is_ok());
}

#[test]
fn each_account_has_its_own_file() {
    let tmp = tempfile::tempdir().unwrap();
    let mut mine = Drafts::default();
    mine.set(DraftKey::New, ed("mine"));
    let mut theirs = Drafts::default();
    theirs.set(DraftKey::New, ed("theirs"));

    save_drafts(tmp.path(), ACCOUNT, &mine).unwrap();
    save_drafts(tmp.path(), OTHER_ACCOUNT, &theirs).unwrap();

    assert_eq!(
        entries(tmp.path(), ACCOUNT),
        vec![(DraftKey::New, "mine".to_string())]
    );
    assert_eq!(
        entries(tmp.path(), OTHER_ACCOUNT),
        vec![(DraftKey::New, "theirs".to_string())]
    );
}

#[test]
fn an_invalid_account_id_saves_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("state");
    let mut drafts = Drafts::default();
    drafts.set(DraftKey::New, ed("x"));

    for id in ["user-myuron", "../evil", ""] {
        assert!(save_drafts(&dir, id, &drafts).is_err(), "{id:?}");
    }

    assert!(!dir.exists());
    assert_eq!(file_names(tmp.path()), Vec::<String>::new());
}

#[test]
fn an_invalid_account_id_loads_nothing_even_when_a_file_of_that_name_exists() {
    let tmp = tempfile::tempdir().unwrap();
    write_file(
        tmp.path(),
        "user-myuron",
        "[[draft]]\nkind = \"new\"\ntext = \"x\"\n",
    );

    assert!(load_drafts(tmp.path(), "user-myuron").is_empty());
}

// --- load: what the file may hold -----------------------------------------------------

#[test]
fn a_missing_file_gives_no_drafts() {
    let tmp = tempfile::tempdir().unwrap();

    assert!(load_drafts(tmp.path(), ACCOUNT).is_empty());
    assert!(load_drafts(&tmp.path().join("absent"), ACCOUNT).is_empty());
}

#[test]
fn a_corrupt_file_is_ignored() {
    let tmp = tempfile::tempdir().unwrap();
    for text in [
        "not toml at all [[[",
        "draft = 3\n",
        "[[draft]]\nkind = 5\ntext = 6\n",
        "\0\0",
    ] {
        write_file(tmp.path(), ACCOUNT, text);

        assert!(load_drafts(tmp.path(), ACCOUNT).is_empty(), "{text:?}");
    }
}

#[test]
fn a_corrupt_file_is_left_for_the_next_save_to_replace() {
    let tmp = tempfile::tempdir().unwrap();
    write_file(tmp.path(), ACCOUNT, "garbage [[[");
    let mut drafts = Drafts::default();
    drafts.set(DraftKey::New, ed("fresh"));

    save_drafts(tmp.path(), ACCOUNT, &drafts).unwrap();

    assert_eq!(
        entries(tmp.path(), ACCOUNT),
        vec![(DraftKey::New, "fresh".to_string())]
    );
}

#[test]
fn an_entry_that_cannot_be_read_is_skipped_and_the_others_are_kept() {
    let tmp = tempfile::tempdir().unwrap();
    write_file(
        tmp.path(),
        ACCOUNT,
        r#"
[[draft]]
kind = "new"
text = "kept"

[[draft]]
kind = "sideways"
id = "c1"
text = "unknown kind"

[[draft]]
kind = "reply"
text = "a reply needs an id"

[[draft]]
kind = "reply"
id = "c2"

[[draft]]
kind = "reply"
id = "c3"
text = "kept too"
"#,
    );

    assert_eq!(
        entries(tmp.path(), ACCOUNT),
        vec![
            (DraftKey::New, "kept".to_string()),
            (DraftKey::Reply("c3".into()), "kept too".to_string()),
        ]
    );
}

#[test]
fn an_entry_with_no_text_is_not_a_draft() {
    let tmp = tempfile::tempdir().unwrap();
    write_file(
        tmp.path(),
        ACCOUNT,
        "[[draft]]\nkind = \"new\"\ntext = \"\"\n",
    );

    assert!(load_drafts(tmp.path(), ACCOUNT).is_empty());
}

#[test]
fn control_and_bidi_characters_in_the_file_are_dropped() {
    let tmp = tempfile::tempdir().unwrap();
    write_file(
        tmp.path(),
        ACCOUNT,
        "[[draft]]\nkind = \"new\"\ntext = \"a\\u0007b\\u202ec\\u001b[31md\\nline two\"\n",
    );

    assert_eq!(
        entries(tmp.path(), ACCOUNT),
        vec![(DraftKey::New, "abc[31md\nline two".to_string())]
    );
}

#[test]
fn a_text_longer_than_8000_characters_is_cut() {
    let tmp = tempfile::tempdir().unwrap();
    let long = "x".repeat(9000);
    write_file(
        tmp.path(),
        ACCOUNT,
        &format!("[[draft]]\nkind = \"new\"\ntext = \"{long}\"\n"),
    );

    let drafts = load_drafts(tmp.path(), ACCOUNT);

    assert_eq!(drafts.text(&DraftKey::New), Some("x".repeat(8000)));
}

#[test]
fn only_the_newest_50_reply_and_quote_drafts_are_loaded() {
    let tmp = tempfile::tempdir().unwrap();
    let mut file = String::from("[[draft]]\nkind = \"new\"\ntext = \"mine\"\n");
    for n in 0..60 {
        file.push_str(&format!(
            "\n[[draft]]\nkind = \"reply\"\nid = \"a{n}\"\ntext = \"x\"\n"
        ));
    }
    write_file(tmp.path(), ACCOUNT, &file);

    let drafts = load_drafts(tmp.path(), ACCOUNT);

    assert_eq!(drafts.len(), REPLY_QUOTE_CAP + 1);
    assert_eq!(drafts.text(&DraftKey::New), Some("mine".to_string()));
    assert_eq!(drafts.text(&reply(9)), None);
    assert_eq!(drafts.text(&reply(10)), Some("x".to_string()));
    assert_eq!(drafts.text(&reply(59)), Some("x".to_string()));
}

#[test]
fn a_loaded_draft_puts_the_cursor_at_the_end_of_the_text() {
    let tmp = tempfile::tempdir().unwrap();
    let mut drafts = Drafts::default();
    drafts.set(DraftKey::New, ed("ab\ncde"));
    save_drafts(tmp.path(), ACCOUNT, &drafts).unwrap();

    let loaded = load_drafts(tmp.path(), ACCOUNT);

    assert_eq!(loaded.get(&DraftKey::New).map(Editor::cursor), Some((1, 3)));
}

// --- merging what was read from disk ----------------------------------------------------------

#[test]
fn loaded_drafts_fill_only_the_slots_that_hold_no_text() {
    let mut drafts = Drafts::default();
    drafts.set(DraftKey::New, ed("typed"));
    let mut loaded = Drafts::default();
    loaded.set(DraftKey::New, ed("from disk"));
    loaded.set(reply(1), ed("re"));

    drafts.fill_missing(loaded);

    assert_eq!(drafts.text(&DraftKey::New), Some("typed".to_string()));
    assert_eq!(drafts.text(&reply(1)), Some("re".to_string()));
}

#[test]
fn over_the_cap_the_oldest_drafts_from_disk_go_first() {
    let mut drafts = Drafts::default();
    for n in 0..30 {
        drafts.set(reply(n), ed("session"));
    }
    let mut loaded = Drafts::default();
    for n in 100..130 {
        loaded.set(reply(n), ed("disk"));
    }

    drafts.fill_missing(loaded);

    assert_eq!(drafts.len(), REPLY_QUOTE_CAP);
    for n in 0..30 {
        assert_eq!(drafts.text(&reply(n)), Some("session".to_string()), "{n}");
    }
    assert_eq!(drafts.text(&reply(100)), None);
    assert_eq!(drafts.text(&reply(109)), None);
    assert_eq!(drafts.text(&reply(110)), Some("disk".to_string()));
    assert_eq!(drafts.text(&reply(129)), Some("disk".to_string()));
}

// --- more of what the file may hold ----------------------------------------------------------------

#[test]
fn a_reply_or_quote_id_that_is_not_hex_and_hyphens_is_not_taken() {
    let tmp = tempfile::tempdir().unwrap();
    write_file(
        tmp.path(),
        ACCOUNT,
        r#"
[[draft]]
kind = "reply"
id = "../evil"
text = "bad"

[[draft]]
kind = "quote"
id = "ZZZ"
text = "bad"

[[draft]]
kind = "reply"
id = "0190a1b2-c3d4"
text = "good"
"#,
    );

    assert_eq!(
        entries(tmp.path(), ACCOUNT),
        vec![(DraftKey::Reply("0190a1b2-c3d4".into()), "good".to_string())]
    );
}

#[test]
fn when_a_key_appears_twice_the_later_entry_wins() {
    let tmp = tempfile::tempdir().unwrap();
    write_file(
        tmp.path(),
        ACCOUNT,
        r#"
[[draft]]
kind = "new"
text = "first"

[[draft]]
kind = "reply"
id = "a1"
text = "re"

[[draft]]
kind = "new"
text = "second"
"#,
    );

    let drafts = load_drafts(tmp.path(), ACCOUNT);

    assert_eq!(drafts.text(&DraftKey::New), Some("second".to_string()));
    assert_eq!(drafts.len(), 2);
}

#[test]
fn a_file_over_one_mebibyte_is_not_read() {
    let tmp = tempfile::tempdir().unwrap();
    let filler = "# ".to_string() + &"x".repeat(1024 * 1024) + "\n";
    write_file(
        tmp.path(),
        ACCOUNT,
        &format!("[[draft]]\nkind = \"new\"\ntext = \"small\"\n{filler}"),
    );
    assert!(load_drafts(tmp.path(), ACCOUNT).is_empty());

    write_file(
        tmp.path(),
        ACCOUNT,
        "[[draft]]\nkind = \"new\"\ntext = \"small\"\n",
    );
    assert_eq!(
        load_drafts(tmp.path(), ACCOUNT).text(&DraftKey::New),
        Some("small".to_string())
    );
}

#[cfg(unix)]
#[test]
fn a_dir_that_already_exists_keeps_its_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    fs::set_permissions(tmp.path(), fs::Permissions::from_mode(0o755)).unwrap();
    let mut drafts = Drafts::default();
    drafts.set(DraftKey::New, ed("x"));

    save_drafts(tmp.path(), ACCOUNT, &drafts).unwrap();

    assert!(file_of(tmp.path(), ACCOUNT).is_file());
    assert_eq!(
        fs::metadata(tmp.path()).unwrap().permissions().mode() & 0o777,
        0o755
    );
}
