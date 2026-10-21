//! The writes of Settings: the Profile tab (sync, lane color, pastéis mode, account deletion) and the Safety tab
//! (selection, scrolling, unmuting and unblocking).
//!
//! - On the Profile tab `j` / `k` move the focus through Sync, Lane color, Pasteis mode and Delete account.
//!   `S` syncs from any row, `D` opens the deletion dialog from any row, and `Enter` acts on the focused row.
//! - On the Lane color row `Left` / `Right` / `h` / `l` move a cursor over Auto, the six colors and custom; the
//!   saved choice is not the cursor. Nothing is shown as saved before the server answers with the account. The
//!   cursor goes back to the saved choice when the tab or the screen is left.
//! - `Enter` on custom opens a field for six hex digits. While it is open every key is text (`Esc` closes the
//!   field, `Enter` sends, `Ctrl+C` quits); only `0-9a-f` is taken, uppercase as lowercase.
//! - Pastéis mode is flipped by `Enter` or `Space`, again only by the answer. One write of each kind is out at a
//!   time.
//! - The account is deleted after the exact username (case included) is typed. The answer takes a guest or the
//!   same account back to a guest, whatever happened to the dialog or the session meanwhile, but not a new
//!   sign-in; the key file, the drafts and the state file stay on disk.
//! - On the Safety tab one selection walks the muted users, then the blocked ones; the headings are skipped and
//!   the list scrolls with the selection. `u` or `Enter` takes the person off the list at once and asks the
//!   server; a refusal puts them back. Only the first page of each list (20 people) is reachable.
//! - A guest may open Settings; Profile and Safety show a sign-in hint, and their write keys ask to sign in.

use crossterm::event::KeyCode;
use serde_json::{Value, json};

use crate::action::{Action, Effect, Failure, MeWrite};
use crate::app::{App, Me, Modal, Screen, Session, SettingsTab, ToastKind};
use crate::feed::ReqId;
use crate::keymap::SettingsText;
use crate::model::{Lane, LaneSetting, MeResponse, MuteResponse, Rgb};
use crate::notifications_tests::me_json;
use crate::profile_tests::{deliver_user, open_profile, user_resp};
use crate::settings::ProfileRow;
use crate::settings_tests::*;
use crate::test_support::*;
use crate::write_tests::{card, key, log_app_of, mapped, me_of, sign_in_again, toast};

const NOT_APPROVED: &str = "device_not_approved";
const NETWORK_TEXT: &str = "could not confirm the deletion — check dev-null.sh before trying again";
const SYNC_RATE_TEXT: &str = "You just synced. Try again in a minute.";
const SYNC_FAILED_TEXT: &str = "Could not sync from GitHub. Try again in a minute.";

// --- fixtures -----------------------------------------------------------------------------------

fn answer(unread: u32, extra: Value) -> Box<MeResponse> {
    Box::new(serde_json::from_value(me_json(unread, extra)).unwrap())
}

/// Signed in on the Profile tab of Settings.
fn profile_app() -> App {
    settings_app(120, 40)
}

fn focus(row: ProfileRow) -> App {
    let mut app = profile_app();
    for _ in 0..[
        ProfileRow::Sync,
        ProfileRow::Lane,
        ProfileRow::Pasteis,
        ProfileRow::Delete,
    ]
    .iter()
    .position(|r| *r == row)
    .unwrap_or(0)
    {
        key(&mut app, 'j');
    }
    assert_eq!(app.settings().profile_row, row, "the focus did not move");
    app
}

fn syncs(effects: &[Effect]) -> Vec<ReqId> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::SyncGithub { req } => Some(*req),
            _ => None,
        })
        .collect()
}

fn lanes(effects: &[Effect]) -> Vec<(LaneSetting, ReqId)> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::SetLane { lane, req } => Some((*lane, *req)),
            _ => None,
        })
        .collect()
}

fn modes(effects: &[Effect]) -> Vec<(bool, ReqId)> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::SetPasteisMode { enabled, req } => Some((*enabled, *req)),
            _ => None,
        })
        .collect()
}

fn deletions(effects: &[Effect]) -> Vec<ReqId> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::DeleteAccount { req } => Some(*req),
            _ => None,
        })
        .collect()
}

/// `(username, req)` of every unmute among the effects.
fn unmutes(effects: &[Effect]) -> Vec<(String, ReqId)> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::MuteUser {
                username,
                on: false,
                req,
            } => Some((username.clone(), *req)),
            _ => None,
        })
        .collect()
}

fn unblocks(effects: &[Effect]) -> Vec<(String, ReqId)> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::BlockUser {
                username,
                on: false,
                req,
            } => Some((username.clone(), *req)),
            _ => None,
        })
        .collect()
}

/// The only item, which a test that needs the request of an effect asks for.
fn one<T: Clone>(items: &[T]) -> T {
    assert_eq!(items.len(), 1, "expected exactly one effect");
    items[0].clone()
}

fn failed(kind: MeWrite, req: ReqId, code: &str) -> Action {
    Action::MeWriteFailed {
        kind,
        req,
        failure: Failure::new(code),
    }
}

// --- 1. the focus of the Profile tab ------------------------------------------------------------

#[test]
fn j_and_k_walk_the_four_rows_of_the_profile_tab_and_stop_at_the_ends() {
    let mut app = profile_app();
    assert_eq!(app.settings().profile_row, ProfileRow::Sync);
    key(&mut app, 'k');
    assert_eq!(app.settings().profile_row, ProfileRow::Sync);
    let mut seen = Vec::new();
    for _ in 0..4 {
        key(&mut app, 'j');
        seen.push(app.settings().profile_row);
    }
    assert_eq!(
        seen,
        vec![
            ProfileRow::Lane,
            ProfileRow::Pasteis,
            ProfileRow::Delete,
            ProfileRow::Delete
        ]
    );
    key(&mut app, 'k');
    assert_eq!(app.settings().profile_row, ProfileRow::Pasteis);
}

#[test]
fn the_arrow_keys_up_and_down_move_the_focus_too() {
    let mut app = profile_app();
    press(&mut app, KeyCode::Down);
    assert_eq!(app.settings().profile_row, ProfileRow::Lane);
    press(&mut app, KeyCode::Up);
    assert_eq!(app.settings().profile_row, ProfileRow::Sync);
}

#[test]
fn j_moves_the_nav_not_the_rows_while_the_nav_has_the_focus() {
    let mut app = profile_app();
    press(&mut app, KeyCode::Tab);
    key(&mut app, 'j');
    assert_eq!(app.settings().profile_row, ProfileRow::Sync);
    press(&mut app, KeyCode::Tab);
    key(&mut app, 'j');
    assert_eq!(app.settings().profile_row, ProfileRow::Lane);
}

#[test]
fn the_row_focus_belongs_to_the_profile_tab() {
    let mut app = profile_app();
    key(&mut app, '2');
    key(&mut app, 'j');
    assert_eq!(app.settings().profile_row, ProfileRow::Sync);
    key(&mut app, '3');
    key(&mut app, 'j');
    assert_eq!(app.settings().profile_row, ProfileRow::Sync);
    key(&mut app, '1');
    key(&mut app, 'j');
    assert_eq!(app.settings().profile_row, ProfileRow::Lane);
}

// --- 2. sync -------------------------------------------------------------------------------------

#[test]
fn s_sends_a_sync_from_every_row() {
    for row in [
        ProfileRow::Sync,
        ProfileRow::Lane,
        ProfileRow::Pasteis,
        ProfileRow::Delete,
    ] {
        let mut app = focus(row);
        let effects = key(&mut app, 'S');
        assert_eq!(syncs(&effects).len(), 1, "{row:?}: {effects:?}");
        assert_eq!(effects.len(), 1, "{row:?}: {effects:?}");
    }
}

#[test]
fn enter_on_the_sync_row_syncs_too() {
    let mut app = profile_app();
    let effects = press(&mut app, KeyCode::Enter);
    assert_eq!(syncs(&effects).len(), 1, "{effects:?}");
}

#[test]
fn s_does_nothing_on_the_other_tabs_and_with_the_nav_focused() {
    let mut app = profile_app();
    key(&mut app, '2');
    assert!(syncs(&key(&mut app, 'S')).is_empty());
    key(&mut app, '3');
    assert!(syncs(&key(&mut app, 'S')).is_empty());
    let mut nav = profile_app();
    press(&mut nav, KeyCode::Tab);
    assert!(syncs(&key(&mut nav, 'S')).is_empty());
    press(&mut nav, KeyCode::Tab);
    assert_eq!(syncs(&key(&mut nav, 'S')).len(), 1);
}

#[test]
fn a_second_s_while_a_sync_is_out_sends_nothing_until_it_is_answered() {
    let mut app = profile_app();
    let req = one(&syncs(&key(&mut app, 'S')));
    assert!(syncs(&key(&mut app, 'S')).is_empty());
    app.update(failed(MeWrite::Sync, req, "http_500"));
    assert_eq!(syncs(&key(&mut app, 'S')).len(), 1);
}

#[test]
fn a_sync_answer_replaces_me_but_keeps_the_unread_count() {
    let mut app = profile_app();
    let req = one(&syncs(&key(&mut app, 'S')));
    let extra = json!({
        "name": "New Name",
        "bio": "fresh bio",
        "github_synced_at": "2026-10-20T12:00:00Z",
    });
    app.update(Action::MeWriteDone {
        kind: MeWrite::Sync,
        req,
        response: answer(99, extra),
    });
    let me = me_of(&app);
    assert_eq!(me.name, "New Name");
    assert_eq!(me.bio, "fresh bio");
    assert_eq!(me.github_synced_at, Some(NOW));
    assert_eq!(me.unread, 12);
}

#[test]
fn a_sync_answer_for_another_request_is_dropped_and_the_sync_stays_out() {
    let mut app = profile_app();
    let req = one(&syncs(&key(&mut app, 'S')));
    app.update(Action::MeWriteDone {
        kind: MeWrite::Sync,
        req: ReqId(req.0 + 100),
        response: answer(0, json!({"name": "Stale"})),
    });
    assert_eq!(me_of(&app).name, "myuron");
    assert!(syncs(&key(&mut app, 'S')).is_empty());
}

#[test]
fn a_sync_asks_for_your_own_profile_again_only_when_it_is_loaded() {
    let mut app = signed_in(120, 40);
    let opened = open_profile(&mut app, "myuron");
    deliver_user(&mut app, &opened, "myuron", user_resp("myuron", json!({})));
    app.update(Action::Open(Screen::Settings));
    let req = one(&syncs(&key(&mut app, 'S')));
    let effects = app.update(Action::MeWriteDone {
        kind: MeWrite::Sync,
        req,
        response: answer(0, json!({})),
    });
    assert!(
        effects.iter().any(
            |e| matches!(e, Effect::FetchUser { username, .. } if username.eq_ignore_ascii_case("myuron"))
        ),
        "{effects:?}"
    );

    let mut bare = profile_app();
    let req = one(&syncs(&key(&mut bare, 'S')));
    let effects = bare.update(Action::MeWriteDone {
        kind: MeWrite::Sync,
        req,
        response: answer(0, json!({})),
    });
    assert!(
        !effects
            .iter()
            .any(|e| matches!(e, Effect::FetchUser { .. })),
        "{effects:?}"
    );
}

#[test]
fn a_refused_sync_reads_in_the_words_of_the_sync() {
    let failure = |code: &str, message: Option<&str>, retry_after: Option<u64>| Failure {
        code: code.into(),
        retry_after,
        message: message.map(String::from),
    };
    let cases = [
        (
            failure("rate_limited", Some(SYNC_RATE_TEXT), Some(60)),
            SYNC_RATE_TEXT,
        ),
        (
            failure("rate_limited", Some("Wait a bit."), None),
            "Wait a bit.",
        ),
        (failure("rate_limited", None, Some(30)), SYNC_RATE_TEXT),
        (failure("http_429", None, None), SYNC_RATE_TEXT),
        (
            failure("github_sync_failed", Some(SYNC_FAILED_TEXT), None),
            SYNC_FAILED_TEXT,
        ),
        (
            failure(
                "github_sync_failed",
                Some("\u{1b}[31mCould not\u{202e} sync"),
                None,
            ),
            "[31mCould not sync",
        ),
        (failure("github_sync_failed", None, None), SYNC_FAILED_TEXT),
        (failure("http_503", None, None), SYNC_FAILED_TEXT),
        (failure("http_500", None, None), "http_500"),
        (
            failure("network", None, None),
            "network error — press R to retry",
        ),
    ];
    for (failure, want) in cases {
        let mut app = profile_app();
        let req = one(&syncs(&key(&mut app, 'S')));
        let shown = failure.clone();
        app.update(Action::MeWriteFailed {
            kind: MeWrite::Sync,
            req,
            failure,
        });
        assert_eq!(
            toast(&app),
            Some((ToastKind::Error, want.to_string())),
            "{shown:?}"
        );
        assert_eq!(me_of(&app).name, "myuron");
    }
}

// --- 3. lane color -------------------------------------------------------------------------------

/// The choice a saved lane stands for: Auto, the six colors in order, then custom.
fn saved_choice(preferred: Option<Lane>) -> usize {
    match preferred {
        None => 0,
        Some(Lane::Index(i)) => usize::from(i) + 1,
        Some(Lane::Custom(_)) => 7,
    }
}

fn lane_choice(app: &App) -> usize {
    app.settings()
        .lane_cursor
        .unwrap_or_else(|| saved_choice(me_of(app).preferred_lane))
}

fn lane_app(preferred: Option<Lane>) -> App {
    let mut me: Me = fixture_me();
    me.preferred_lane = preferred;
    if let Some(lane) = preferred {
        me.lane = lane;
    }
    let mut app = settings_app_for(me, 120, 40);
    key(&mut app, 'j');
    assert_eq!(app.settings().profile_row, ProfileRow::Lane);
    app
}

fn lane_app_at(preferred: Option<Lane>, choice: usize) -> App {
    let mut app = lane_app(preferred);
    let from = lane_choice(&app);
    for _ in from..choice {
        key(&mut app, 'l');
    }
    for _ in choice..from {
        key(&mut app, 'h');
    }
    assert_eq!(lane_choice(&app), choice, "the cursor did not move");
    app
}

#[test]
fn the_lane_cursor_starts_on_the_saved_choice() {
    for (preferred, want) in [
        (None, 0),
        (Some(Lane::Index(0)), 1),
        (Some(Lane::Index(5)), 6),
        (Some(Lane::Custom(Rgb(1, 2, 3))), 7),
    ] {
        let app = lane_app(preferred);
        assert_eq!(lane_choice(&app), want, "{preferred:?}");
    }
}

#[test]
fn right_and_left_and_l_and_h_move_the_cursor_by_one_choice_and_stop_at_the_ends() {
    let mut app = lane_app(None);
    key(&mut app, 'h');
    assert_eq!(lane_choice(&app), 0);
    key(&mut app, 'l');
    assert_eq!(lane_choice(&app), 1);
    press(&mut app, KeyCode::Right);
    assert_eq!(lane_choice(&app), 2);
    press(&mut app, KeyCode::Left);
    assert_eq!(lane_choice(&app), 1);
    for _ in 0..10 {
        key(&mut app, 'l');
    }
    assert_eq!(lane_choice(&app), 7);
}

#[test]
fn moving_the_cursor_does_not_change_the_saved_lane_or_send_anything() {
    let mut app = lane_app(Some(Lane::Index(1)));
    let before = me_of(&app);
    let effects = key(&mut app, 'l');
    assert!(effects.is_empty(), "{effects:?}");
    assert_eq!(lane_choice(&app), 3);
    assert_eq!(me_of(&app), before);
}

#[test]
fn the_lane_keys_do_nothing_on_the_other_rows() {
    for row in [ProfileRow::Sync, ProfileRow::Pasteis, ProfileRow::Delete] {
        let mut app = focus(row);
        let saved = lane_choice(&app);
        for c in ['l', 'h'] {
            key(&mut app, c);
        }
        press(&mut app, KeyCode::Right);
        assert_eq!(lane_choice(&app), saved, "{row:?}");
    }
    let mut on_lane = lane_app(None);
    key(&mut on_lane, 'l');
    assert_eq!(lane_choice(&on_lane), 1);
}

#[test]
fn enter_sends_the_choice_under_the_cursor() {
    let cases = [
        (0, LaneSetting::Automatic),
        (1, LaneSetting::Palette(0)),
        (2, LaneSetting::Palette(1)),
        (6, LaneSetting::Palette(5)),
    ];
    for (choice, want) in cases {
        let mut app = lane_app_at(None, choice);
        let effects = press(&mut app, KeyCode::Enter);
        let (lane, _) = one(&lanes(&effects));
        assert_eq!(lane, want, "choice {choice}");
        assert_eq!(effects.len(), 1, "choice {choice}: {effects:?}");
    }
}

#[test]
fn nothing_is_saved_before_the_answer() {
    let mut app = lane_app_at(None, 3);
    press(&mut app, KeyCode::Enter);
    let me = me_of(&app);
    assert_eq!(me.preferred_lane, None);
    assert_eq!(me.lane, Lane::Index(1));
}

#[test]
fn enter_while_the_lane_is_out_sends_nothing_until_it_is_answered() {
    let mut app = lane_app_at(None, 3);
    let (_, req) = one(&lanes(&press(&mut app, KeyCode::Enter)));
    assert!(lanes(&press(&mut app, KeyCode::Enter)).is_empty());
    app.update(failed(MeWrite::Lane, req, "http_500"));
    assert_eq!(lanes(&press(&mut app, KeyCode::Enter)).len(), 1);
}

#[test]
fn the_lane_answer_replaces_me_and_the_saved_choice_follows() {
    let mut app = lane_app_at(None, 4);
    let (_, req) = one(&lanes(&press(&mut app, KeyCode::Enter)));
    app.update(Action::MeWriteDone {
        kind: MeWrite::Lane,
        req,
        response: answer(0, json!({"lane": 3, "preferred_lane": 3})),
    });
    let me = me_of(&app);
    assert_eq!(me.preferred_lane, Some(Lane::Index(3)));
    assert_eq!(me.lane, Lane::Index(3));
    assert_eq!(lane_choice(&app), 4);
}

#[test]
fn a_lane_answer_for_another_request_is_dropped() {
    let mut app = lane_app_at(None, 4);
    let (_, req) = one(&lanes(&press(&mut app, KeyCode::Enter)));
    app.update(Action::MeWriteDone {
        kind: MeWrite::Lane,
        req: ReqId(req.0 + 100),
        response: answer(0, json!({"lane": 3, "preferred_lane": 3})),
    });
    assert_eq!(me_of(&app).preferred_lane, None);
}

#[test]
fn the_lane_answer_recolors_your_own_commits_in_every_list_but_nobody_elses() {
    let mut app = log_app_of(
        fixture_me(),
        vec![card("own", "myuron"), card("other", "alice")],
    );
    type_str(&mut app, "gs");
    key(&mut app, 'j');
    for _ in 0..4 {
        key(&mut app, 'l');
    }
    let (_, req) = one(&lanes(&press(&mut app, KeyCode::Enter)));
    app.update(Action::MeWriteDone {
        kind: MeWrite::Lane,
        req,
        response: answer(0, json!({"lane": 3, "preferred_lane": 3})),
    });
    let items = &app
        .feed(crate::feed::FeedKey::Log(crate::app::LogTab::Following))
        .items;
    let lane_of = |id: &str| items.iter().find(|c| c.id == id).map(|c| c.author.lane);
    assert_eq!(lane_of("own"), Some(Some(Lane::Index(3))));
    assert_eq!(lane_of("other"), Some(Some(Lane::Index(1))));
}

#[test]
fn a_refused_lane_toasts_and_leaves_the_account_alone() {
    let mut app = lane_app_at(None, 3);
    let (_, req) = one(&lanes(&press(&mut app, KeyCode::Enter)));
    app.update(failed(MeWrite::Lane, req, "http_500"));
    assert_eq!(toast(&app), Some((ToastKind::Error, "http_500".into())));
    assert_eq!(me_of(&app).preferred_lane, None);
}

#[test]
fn leaving_the_tab_or_the_screen_puts_the_cursor_back_on_the_saved_choice() {
    let mut app = lane_app_at(Some(Lane::Index(1)), 5);
    key(&mut app, '2');
    key(&mut app, '1');
    assert_eq!(lane_choice(&app), 2, "after another tab");

    let mut app = lane_app_at(Some(Lane::Index(1)), 5);
    type_str(&mut app, "gl");
    type_str(&mut app, "gs");
    assert_eq!(lane_choice(&app), 2, "after another screen");

    let mut app = signed_in(120, 40);
    app.update(Action::Open(Screen::Settings));
    key(&mut app, 'j');
    key(&mut app, 'l');
    key(&mut app, 'l');
    press(&mut app, KeyCode::Esc);
    app.update(Action::Open(Screen::Settings));
    assert_eq!(lane_choice(&app), 0, "after going back");
}

// --- 4. custom color -------------------------------------------------------------------------------

fn hex_app() -> App {
    let mut app = lane_app_at(None, 7);
    let effects = press(&mut app, KeyCode::Enter);
    assert!(effects.is_empty(), "{effects:?}");
    assert_eq!(
        app.settings().hex_input.as_deref(),
        Some(""),
        "the field did not open"
    );
    app
}

fn hex_of(app: &App) -> Option<String> {
    app.settings().hex_input.clone()
}

#[test]
fn enter_on_custom_opens_an_empty_field_and_sends_nothing() {
    let _ = hex_app();
    let mut saved = lane_app_at(Some(Lane::Custom(Rgb(0x3a, 0x7b, 0xd5))), 7);
    let effects = press(&mut saved, KeyCode::Enter);
    assert!(effects.is_empty(), "{effects:?}");
    assert_eq!(hex_of(&saved), Some(String::new()));
}

#[test]
fn only_hex_digits_are_typed_as_lowercase_and_at_most_six() {
    let mut app = hex_app();
    type_str(&mut app, "G3a7Bz!d5");
    assert_eq!(hex_of(&app), Some("3a7bd5".into()));
    type_str(&mut app, "ff");
    assert_eq!(hex_of(&app), Some("3a7bd5".into()));
}

#[test]
fn backspace_takes_the_last_digit_back() {
    let mut app = hex_app();
    type_str(&mut app, "3a7");
    press(&mut app, KeyCode::Backspace);
    assert_eq!(hex_of(&app), Some("3a".into()));
    press(&mut app, KeyCode::Backspace);
    press(&mut app, KeyCode::Backspace);
    press(&mut app, KeyCode::Backspace);
    assert_eq!(hex_of(&app), Some(String::new()));
}

#[test]
fn enter_needs_six_digits() {
    let mut app = hex_app();
    type_str(&mut app, "3a7bd");
    let effects = press(&mut app, KeyCode::Enter);
    assert!(effects.is_empty(), "{effects:?}");
    assert_eq!(hex_of(&app), Some("3a7bd".into()));
}

#[test]
fn six_digits_send_the_color_and_the_field_stays_open_until_the_answer() {
    let mut app = hex_app();
    type_str(&mut app, "3A7bd5");
    let (lane, req) = one(&lanes(&press(&mut app, KeyCode::Enter)));
    assert_eq!(lane, LaneSetting::Color(Rgb(0x3a, 0x7b, 0xd5)));
    assert_eq!(hex_of(&app), Some("3a7bd5".into()));
    assert!(lanes(&press(&mut app, KeyCode::Enter)).is_empty());
    assert_eq!(me_of(&app).preferred_lane, None);

    app.update(Action::MeWriteDone {
        kind: MeWrite::Lane,
        req,
        response: answer(0, json!({"lane": "#3a7bd5", "preferred_lane": "#3a7bd5"})),
    });
    assert_eq!(hex_of(&app), None);
    assert_eq!(
        me_of(&app).preferred_lane,
        Some(Lane::Custom(Rgb(0x3a, 0x7b, 0xd5)))
    );
    assert_eq!(lane_choice(&app), 7);
}

#[test]
fn a_refused_color_keeps_the_field_for_another_try() {
    let mut app = hex_app();
    type_str(&mut app, "3a7bd5");
    let (_, req) = one(&lanes(&press(&mut app, KeyCode::Enter)));
    app.update(failed(MeWrite::Lane, req, "http_500"));
    assert_eq!(hex_of(&app), Some("3a7bd5".into()));
    assert_eq!(toast(&app), Some((ToastKind::Error, "http_500".into())));
    assert_eq!(lanes(&press(&mut app, KeyCode::Enter)).len(), 1);
}

#[test]
fn esc_closes_the_field_and_stays_on_settings_and_the_next_esc_goes_back() {
    let mut app = signed_in(120, 40);
    app.update(Action::Open(Screen::Settings));
    key(&mut app, 'j');
    for _ in 0..7 {
        key(&mut app, 'l');
    }
    press(&mut app, KeyCode::Enter);
    type_str(&mut app, "3a");
    assert_eq!(hex_of(&app), Some("3a".into()));

    press(&mut app, KeyCode::Esc);
    assert_eq!(hex_of(&app), None);
    assert_eq!(app.screen(), Screen::Settings);
    assert_eq!(app.settings().tab, SettingsTab::Profile);

    press(&mut app, KeyCode::Esc);
    assert!(matches!(app.screen(), Screen::Log(_)));
}

#[test]
fn every_key_is_text_while_the_field_is_open() {
    let mut app = hex_app();
    for c in ['Q', 'j', 'k', 'S', 'D', '?', 'g', 'R'] {
        let effects = type_str(&mut app, &c.to_string());
        assert!(
            !quit_requested(&effects) && effects.is_empty(),
            "{c}: {effects:?}"
        );
    }
    assert_eq!(hex_of(&app), Some("d".into()));
    assert_eq!(app.modal(), None);
    assert_eq!(app.settings().profile_row, ProfileRow::Lane);
    assert!(!app.g_pending());

    type_str(&mut app, "c");
    type_str(&mut app, "2");
    type_str(&mut app, "1");
    assert_eq!(hex_of(&app), Some("dc21".into()));
    assert_eq!(app.settings().tab, SettingsTab::Profile);
    assert_eq!(app.screen(), Screen::Settings);
}

#[test]
fn ctrl_c_quits_while_the_field_is_open() {
    let mut app = hex_app();
    assert!(quit_requested(&ctrl(&mut app, 'c')));
}

// --- 5. pastéis mode ---------------------------------------------------------------------------------

#[test]
fn enter_and_space_on_the_pasteis_row_ask_for_the_opposite_mode() {
    for code in [KeyCode::Enter, KeyCode::Char(' ')] {
        let mut app = focus(ProfileRow::Pasteis);
        let (enabled, _) = one(&modes(&press(&mut app, code)));
        assert!(!enabled, "{code:?}: the mode is on in the fixture");

        let mut me = fixture_me();
        me.pasteis_mode = false;
        let mut off = settings_app_for(me, 120, 40);
        key(&mut off, 'j');
        key(&mut off, 'j');
        let (enabled, _) = one(&modes(&press(&mut off, code)));
        assert!(enabled, "{code:?}");
    }
}

#[test]
fn space_does_nothing_on_the_other_rows() {
    for row in [ProfileRow::Sync, ProfileRow::Lane, ProfileRow::Delete] {
        let mut app = focus(row);
        let effects = press(&mut app, KeyCode::Char(' '));
        assert!(effects.is_empty(), "{row:?}: {effects:?}");
        assert_eq!(app.modal(), None, "{row:?}");
    }
    let mut pasteis = focus(ProfileRow::Pasteis);
    assert_eq!(modes(&press(&mut pasteis, KeyCode::Char(' '))).len(), 1);
}

#[test]
fn the_mode_changes_only_with_the_answer() {
    let mut app = focus(ProfileRow::Pasteis);
    let (_, req) = one(&modes(&press(&mut app, KeyCode::Enter)));
    assert!(me_of(&app).pasteis_mode);
    app.update(Action::MeWriteDone {
        kind: MeWrite::PasteisMode,
        req,
        response: answer(0, json!({"pasteis_mode": false})),
    });
    assert!(!me_of(&app).pasteis_mode);
}

#[test]
fn a_second_toggle_while_one_is_out_sends_nothing_and_a_refusal_frees_it() {
    let mut app = focus(ProfileRow::Pasteis);
    let (_, req) = one(&modes(&press(&mut app, KeyCode::Enter)));
    assert!(modes(&press(&mut app, KeyCode::Char(' '))).is_empty());
    app.update(failed(MeWrite::PasteisMode, req, "http_500"));
    assert_eq!(toast(&app), Some((ToastKind::Error, "http_500".into())));
    assert!(me_of(&app).pasteis_mode);
    assert_eq!(modes(&press(&mut app, KeyCode::Enter)).len(), 1);
}

#[test]
fn a_pasteis_answer_for_another_request_is_dropped() {
    let mut app = focus(ProfileRow::Pasteis);
    let (_, req) = one(&modes(&press(&mut app, KeyCode::Enter)));
    app.update(Action::MeWriteDone {
        kind: MeWrite::PasteisMode,
        req: ReqId(req.0 + 100),
        response: answer(0, json!({"pasteis_mode": false})),
    });
    assert!(me_of(&app).pasteis_mode);
}

#[test]
fn the_writes_of_different_kinds_are_out_independently() {
    let mut app = focus(ProfileRow::Pasteis);
    assert_eq!(syncs(&key(&mut app, 'S')).len(), 1);
    assert_eq!(modes(&press(&mut app, KeyCode::Enter)).len(), 1);
    key(&mut app, 'k');
    assert_eq!(lanes(&press(&mut app, KeyCode::Enter)).len(), 1);
}

// --- 6. deleting the account ---------------------------------------------------------------------------

fn dialog_app() -> App {
    let mut app = profile_app();
    let effects = key(&mut app, 'D');
    assert!(effects.is_empty(), "{effects:?}");
    assert_eq!(app.modal(), Some(Modal::ConfirmAccountDelete));
    app
}

/// The dialog with the username typed and Enter pressed; returns the app and the request.
fn deleting() -> (App, ReqId) {
    let mut app = dialog_app();
    type_str(&mut app, "myuron");
    let req = one(&deletions(&press(&mut app, KeyCode::Enter)));
    (app, req)
}

fn input_of(app: &App) -> Option<String> {
    app.account_delete().map(|d| d.input.clone())
}

#[test]
fn d_opens_the_dialog_from_any_row_of_the_profile_tab_with_an_empty_field() {
    for row in [
        ProfileRow::Sync,
        ProfileRow::Lane,
        ProfileRow::Pasteis,
        ProfileRow::Delete,
    ] {
        let mut app = focus(row);
        key(&mut app, 'D');
        assert_eq!(app.modal(), Some(Modal::ConfirmAccountDelete), "{row:?}");
        assert_eq!(input_of(&app), Some(String::new()), "{row:?}");
    }
}

#[test]
fn enter_on_the_delete_row_opens_the_dialog() {
    let mut app = focus(ProfileRow::Delete);
    let effects = press(&mut app, KeyCode::Enter);
    assert!(effects.is_empty(), "{effects:?}");
    assert_eq!(app.modal(), Some(Modal::ConfirmAccountDelete));
}

#[test]
fn d_does_nothing_on_the_other_tabs_and_with_the_nav_focused() {
    let mut app = profile_app();
    key(&mut app, '2');
    key(&mut app, 'D');
    assert_eq!(app.modal(), None);
    key(&mut app, '3');
    key(&mut app, 'D');
    assert_eq!(app.modal(), None);
    let mut nav = profile_app();
    press(&mut nav, KeyCode::Tab);
    key(&mut nav, 'D');
    assert_eq!(nav.modal(), None);
    press(&mut nav, KeyCode::Tab);
    key(&mut nav, 'D');
    assert_eq!(nav.modal(), Some(Modal::ConfirmAccountDelete));
}

#[test]
fn every_key_is_text_in_the_username_field() {
    let mut app = dialog_app();
    let effects = type_str(&mut app, "Qcgyn1?jD");
    assert!(
        !quit_requested(&effects) && effects.is_empty(),
        "{effects:?}"
    );
    assert_eq!(input_of(&app), Some("Qcgyn1?jD".into()));
    assert_eq!(app.modal(), Some(Modal::ConfirmAccountDelete));
    press(&mut app, KeyCode::Backspace);
    assert_eq!(input_of(&app), Some("Qcgyn1?j".into()));
}

#[test]
fn ctrl_c_quits_in_the_dialog() {
    let mut app = dialog_app();
    assert!(quit_requested(&ctrl(&mut app, 'c')));
}

#[test]
fn esc_closes_the_dialog_without_sending_and_stays_on_settings() {
    let mut app = dialog_app();
    type_str(&mut app, "myuron");
    let effects = press(&mut app, KeyCode::Esc);
    assert!(effects.is_empty(), "{effects:?}");
    assert_eq!(app.modal(), None);
    assert_eq!(app.account_delete(), None);
    assert_eq!(app.screen(), Screen::Settings);
}

#[test]
fn enter_sends_nothing_for_an_empty_a_short_or_a_differently_cased_username() {
    for typed in [
        "", "myuro", "Myuron", "MYURON", "myuron ", "@myuron", "myuronx",
    ] {
        let mut app = dialog_app();
        type_str(&mut app, typed);
        let effects = press(&mut app, KeyCode::Enter);
        assert!(effects.is_empty(), "{typed:?}: {effects:?}");
        assert_eq!(app.modal(), Some(Modal::ConfirmAccountDelete), "{typed:?}");
        assert_eq!(input_of(&app), Some(typed.into()), "{typed:?}");
    }
}

#[test]
fn the_exact_username_sends_one_deletion_and_a_second_enter_sends_none() {
    let mut app = dialog_app();
    type_str(&mut app, "myuron");
    let effects = press(&mut app, KeyCode::Enter);
    assert_eq!(deletions(&effects).len(), 1, "{effects:?}");
    assert_eq!(effects.len(), 1, "{effects:?}");
    assert_eq!(app.account_delete().map(|d| d.sending), Some(true));
    assert!(deletions(&press(&mut app, KeyCode::Enter)).is_empty());
}

#[test]
fn the_answer_takes_you_back_to_a_guest_without_the_sign_in_dialog() {
    let (mut app, req) = deleting();
    let effects = app.update(Action::AccountDeleted { req });
    assert_eq!(app.session(), &Session::Guest);
    assert_eq!(app.modal(), None);
    assert_eq!(app.account_delete(), None);
    assert!(matches!(app.screen(), Screen::Log(_)));
    assert!(effects.contains(&Effect::UseGuestClient), "{effects:?}");
    assert!(
        !effects
            .iter()
            .any(|e| matches!(e, Effect::PrepareSignIn { .. })),
        "{effects:?}"
    );
    assert_eq!(
        toast(&app),
        Some((ToastKind::Info, "Account deleted".into()))
    );
}

#[test]
fn the_answer_drops_what_the_account_had_loaded() {
    let mut app = log_app_of(fixture_me(), vec![card("own", "myuron")]);
    type_str(&mut app, "gs");
    key(&mut app, 'D');
    type_str(&mut app, "myuron");
    let req = one(&deletions(&press(&mut app, KeyCode::Enter)));
    app.update(Action::AccountDeleted { req });
    assert!(
        app.feed(crate::feed::FeedKey::Log(crate::app::LogTab::Following))
            .items
            .is_empty()
    );
    assert_eq!(app.settings().profile_row, ProfileRow::Sync);
}

#[test]
fn the_answer_applies_after_the_dialog_was_closed() {
    let (mut app, req) = deleting();
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.modal(), None);
    app.update(Action::AccountDeleted { req });
    assert_eq!(app.session(), &Session::Guest);
    assert_eq!(
        toast(&app),
        Some((ToastKind::Info, "Account deleted".into()))
    );
}

#[test]
fn the_answer_after_the_device_was_refused_only_replaces_the_toast_and_closes_the_dialog() {
    let (mut app, req) = deleting();
    app.update(Action::MeFailed {
        req: ReqId(1),
        failure: Failure::new(NOT_APPROVED),
    });
    assert_eq!(app.session(), &Session::Guest);
    assert_eq!(app.modal(), Some(Modal::SignIn));
    assert_eq!(app.account_delete(), None);

    let effects = app.update(Action::AccountDeleted { req });
    assert!(effects.is_empty(), "{effects:?}");
    assert_eq!(app.session(), &Session::Guest);
    assert_eq!(app.modal(), None);
    assert_eq!(
        toast(&app),
        Some((ToastKind::Info, "Account deleted".into()))
    );
}

#[test]
fn the_answer_after_a_new_sign_in_changes_nothing() {
    let (mut app, req) = deleting();
    sign_in_again(&mut app);
    assert!(matches!(app.session(), Session::SignedIn(_)));
    let before = toast(&app);
    let effects = app.update(Action::AccountDeleted { req });
    assert!(effects.is_empty(), "{effects:?}");
    assert!(matches!(app.session(), Session::SignedIn(_)));
    assert_eq!(toast(&app), before);
    assert_ne!(
        toast(&app),
        Some((ToastKind::Info, "Account deleted".into()))
    );
}

#[test]
fn an_answer_to_another_request_changes_nothing() {
    let (mut app, req) = deleting();
    let effects = app.update(Action::AccountDeleted {
        req: ReqId(req.0 + 100),
    });
    assert!(effects.is_empty(), "{effects:?}");
    assert!(matches!(app.session(), Session::SignedIn(_)));
}

#[test]
fn a_second_answer_changes_nothing() {
    let (mut app, req) = deleting();
    app.update(Action::AccountDeleted { req });
    app.update(Action::Tick(NOW + time::Duration::seconds(10)));
    assert_eq!(toast(&app), None);
    let effects = app.update(Action::AccountDeleted { req });
    assert!(effects.is_empty(), "{effects:?}");
    assert_eq!(toast(&app), None);
}

#[test]
fn a_refused_deletion_toasts_keeps_the_dialog_and_lets_enter_try_again() {
    let (mut app, req) = deleting();
    app.update(Action::AccountDeleteFailed {
        req,
        failure: Failure::new("http_500"),
    });
    assert_eq!(toast(&app), Some((ToastKind::Error, "http_500".into())));
    assert_eq!(app.modal(), Some(Modal::ConfirmAccountDelete));
    assert_eq!(app.account_delete().map(|d| d.sending), Some(false));
    assert!(matches!(app.session(), Session::SignedIn(_)));
    type_str(&mut app, "myuron");
    let again = one(&deletions(&press(&mut app, KeyCode::Enter)));
    assert_ne!(again, req);
}

#[test]
fn a_failed_deletion_empties_the_username_field() {
    let (mut app, req) = deleting();
    assert_eq!(input_of(&app), Some("myuron".into()));
    app.update(Action::AccountDeleteFailed {
        req,
        failure: Failure::new("http_500"),
    });
    assert_eq!(input_of(&app), Some(String::new()));
    assert!(deletions(&press(&mut app, KeyCode::Enter)).is_empty());
}

#[test]
fn a_timeout_or_a_network_failure_says_to_check_before_trying_again() {
    let (mut app, req) = deleting();
    app.update(Action::AccountDeleteFailed {
        req,
        failure: Failure::new("network"),
    });
    assert_eq!(toast(&app), Some((ToastKind::Error, NETWORK_TEXT.into())));
    assert!(matches!(app.session(), Session::SignedIn(_)));
}

#[test]
fn the_server_message_of_a_refusal_is_shown_cleaned() {
    let (mut app, req) = deleting();
    app.update(Action::AccountDeleteFailed {
        req,
        failure: Failure {
            code: "forbidden".into(),
            retry_after: None,
            message: Some("\u{202e}Nope".into()),
        },
    });
    assert_eq!(toast(&app), Some((ToastKind::Error, "Nope".into())));
}

#[test]
fn a_failure_after_the_return_to_a_guest_still_shows_and_one_after_a_new_sign_in_shows_nothing() {
    let (mut app, req) = deleting();
    app.update(Action::MeFailed {
        req: ReqId(1),
        failure: Failure::new(NOT_APPROVED),
    });
    app.update(Action::AccountDeleteFailed {
        req,
        failure: Failure::new("network"),
    });
    assert_eq!(toast(&app), Some((ToastKind::Error, NETWORK_TEXT.into())));

    let (mut app, req) = deleting();
    sign_in_again(&mut app);
    app.update(Action::Tick(NOW + time::Duration::seconds(10)));
    app.update(Action::AccountDeleteFailed {
        req,
        failure: Failure::new("network"),
    });
    assert_eq!(toast(&app), None);
    assert!(matches!(app.session(), Session::SignedIn(_)));
}

#[test]
fn the_refusal_of_the_device_by_the_deletion_itself_returns_to_a_guest() {
    let (mut app, req) = deleting();
    app.update(Action::AccountDeleteFailed {
        req,
        failure: Failure::new(NOT_APPROVED),
    });
    assert_eq!(app.session(), &Session::Guest);
    assert_eq!(app.modal(), Some(Modal::SignIn));
    assert_eq!(
        toast(&app),
        Some((
            ToastKind::Error,
            "this device is no longer approved — sign in again".into()
        ))
    );
}

#[test]
fn the_refusal_of_the_device_by_a_settings_write_returns_to_a_guest() {
    for kind in [MeWrite::Sync, MeWrite::Lane, MeWrite::PasteisMode] {
        let mut app = focus(ProfileRow::Sync);
        let req = match kind {
            MeWrite::Sync => one(&syncs(&key(&mut app, 'S'))),
            MeWrite::Lane => {
                key(&mut app, 'j');
                key(&mut app, 'l');
                one(&lanes(&press(&mut app, KeyCode::Enter))).1
            }
            MeWrite::PasteisMode => {
                key(&mut app, 'j');
                key(&mut app, 'j');
                one(&modes(&press(&mut app, KeyCode::Enter))).1
            }
        };
        app.update(failed(kind, req, NOT_APPROVED));
        assert_eq!(app.session(), &Session::Guest, "{kind:?}");
        assert_eq!(app.modal(), Some(Modal::SignIn), "{kind:?}");
    }
}

#[test]
fn a_settings_write_refused_before_the_latest_sign_in_changes_nothing() {
    for kind in [MeWrite::Sync, MeWrite::Lane, MeWrite::PasteisMode] {
        let mut app = profile_app();
        let req = send_write(&mut app, kind);
        sign_in_again(&mut app);
        app.update(failed(kind, req, NOT_APPROVED));
        assert!(matches!(app.session(), Session::SignedIn(_)), "{kind:?}");
    }
    let (mut app, req) = deleting();
    sign_in_again(&mut app);
    app.update(Action::AccountDeleteFailed {
        req,
        failure: Failure::new(NOT_APPROVED),
    });
    assert!(matches!(app.session(), Session::SignedIn(_)));
}

/// Sends the write of `kind` from the Profile tab and returns its request.
fn send_write(app: &mut App, kind: MeWrite) -> ReqId {
    match kind {
        MeWrite::Sync => one(&syncs(&key(app, 'S'))),
        MeWrite::Lane => {
            key(app, 'j');
            key(app, 'l');
            one(&lanes(&press(app, KeyCode::Enter))).1
        }
        MeWrite::PasteisMode => {
            key(app, 'j');
            key(app, 'j');
            one(&modes(&press(app, KeyCode::Enter))).1
        }
    }
}

#[test]
fn every_write_can_be_sent_again_after_a_return_to_a_guest_and_a_new_sign_in() {
    for kind in [MeWrite::Sync, MeWrite::Lane, MeWrite::PasteisMode] {
        let mut app = profile_app();
        send_write(&mut app, kind);
        sign_in_again(&mut app);
        type_str(&mut app, "gs");
        assert_eq!(app.screen(), Screen::Settings, "{kind:?}");
        send_write(&mut app, kind);
    }
}

#[test]
fn returning_to_a_guest_closes_the_dialogs_and_forgets_the_settings_state() {
    let mut app = dialog_app();
    type_str(&mut app, "my");
    app.update(Action::MeFailed {
        req: ReqId(1),
        failure: Failure::new(NOT_APPROVED),
    });
    assert_eq!(app.account_delete(), None);

    let mut app = hex_app();
    type_str(&mut app, "3a");
    app.update(Action::MeFailed {
        req: ReqId(1),
        failure: Failure::new(NOT_APPROVED),
    });
    assert_eq!(hex_of(&app), None);
    assert_eq!(app.settings().profile_row, ProfileRow::Sync);
}

// --- 7. Safety: selection and scrolling -------------------------------------------------------------------

const MUTED_12: [&str; 12] = [
    "amber", "bella", "chloe", "diego", "elena", "felix", "grace", "hiram", "irene", "jonas",
    "karin", "lucas",
];
const BLOCKED_8: [&str; 8] = [
    "nadia", "oscar", "petra", "quinn", "ravi", "sofia", "tomas", "ursula",
];

pub(crate) fn safety_app(muted: &[&str], blocked: &[&str], w: u16, h: u16) -> App {
    let mut app = settings_app(w, h);
    let effects = open_safety(&mut app);
    load_muted(&mut app, &effects, muted, None);
    load_blocked(&mut app, &effects, blocked, None);
    app
}

/// `(blocked, username)` of every selectable person, muted ones first.
fn flat(app: &App) -> Vec<(bool, String)> {
    let state = app.settings();
    state
        .muted
        .users
        .iter()
        .map(|u| (false, u.username.clone()))
        .chain(
            state
                .blocked
                .users
                .iter()
                .map(|u| (true, u.username.clone())),
        )
        .collect()
}

fn selected(app: &App) -> Option<(bool, String)> {
    flat(app).get(app.settings().safety_selected).cloned()
}

fn names(users: &[crate::model::User]) -> Vec<&str> {
    users.iter().map(|u| u.username.as_str()).collect()
}

fn muted_blocked(names_m: &[&str], names_b: &[&str]) -> App {
    safety_app(names_m, names_b, 120, 40)
}

#[test]
fn the_selection_starts_on_the_first_muted_user_and_walks_on_to_the_blocked_ones() {
    let mut app = muted_blocked(&["amber", "bella"], &["nadia"]);
    assert_eq!(selected(&app), Some((false, "amber".into())));
    key(&mut app, 'j');
    assert_eq!(selected(&app), Some((false, "bella".into())));
    key(&mut app, 'j');
    assert_eq!(selected(&app), Some((true, "nadia".into())));
    key(&mut app, 'j');
    assert_eq!(selected(&app), Some((true, "nadia".into())));
    key(&mut app, 'k');
    assert_eq!(selected(&app), Some((false, "bella".into())));
    press(&mut app, KeyCode::Down);
    assert_eq!(selected(&app), Some((true, "nadia".into())));
    press(&mut app, KeyCode::Up);
    assert_eq!(selected(&app), Some((false, "bella".into())));
}

#[test]
fn capital_g_selects_the_last_person_and_g_g_the_first() {
    let mut app = muted_blocked(&["amber", "bella"], &["nadia", "oscar"]);
    key(&mut app, 'G');
    assert_eq!(selected(&app), Some((true, "oscar".into())));
    type_str(&mut app, "gg");
    assert_eq!(selected(&app), Some((false, "amber".into())));
    assert!(!app.g_pending());
}

#[test]
fn with_only_blocked_people_the_selection_starts_on_the_first_of_them() {
    let mut app = muted_blocked(&[], &["nadia", "oscar"]);
    assert_eq!(selected(&app), Some((true, "nadia".into())));
    key(&mut app, 'j');
    assert_eq!(selected(&app), Some((true, "oscar".into())));
}

#[test]
fn the_selection_keys_do_nothing_with_both_lists_empty() {
    let mut app = muted_blocked(&[], &[]);
    for c in ['j', 'k', 'G'] {
        assert!(key(&mut app, c).is_empty());
    }
    assert!(press(&mut app, KeyCode::Enter).is_empty());
    assert!(key(&mut app, 'u').is_empty());
    assert_eq!(app.settings().safety_selected, 0);
}

#[test]
fn the_selection_keys_do_not_move_the_profile_rows() {
    let mut app = muted_blocked(&["amber", "bella"], &[]);
    key(&mut app, 'j');
    assert_eq!(app.settings().profile_row, ProfileRow::Sync);
    assert_eq!(selected(&app), Some((false, "bella".into())));
}

#[test]
fn a_reload_that_shortens_the_lists_keeps_the_selection_inside_them() {
    let mut app = muted_blocked(&["amber", "bella", "chloe"], &["nadia"]);
    key(&mut app, 'G');
    assert_eq!(app.settings().safety_selected, 3);
    let reload = press(&mut app, KeyCode::Char('R'));
    load_muted(&mut app, &reload, &["amber"], None);
    load_blocked(&mut app, &reload, &[], None);
    assert_eq!(app.settings().safety_selected, 0);
    assert_eq!(selected(&app), Some((false, "amber".into())));
}

#[test]
fn the_safety_body_scrolls_only_as_far_as_the_selection_needs() {
    let mut app = safety_app(&MUTED_12, &BLOCKED_8, 80, 24);
    assert_eq!(app.settings().safety_scroll, 0);

    key(&mut app, 'G');
    assert_eq!(selected(&app), Some((true, "ursula".into())));
    assert_eq!(app.settings().safety_scroll, 5);

    type_str(&mut app, "gg");
    assert_eq!(app.settings().safety_selected, 0);
    assert_eq!(app.settings().safety_scroll, 0);

    for _ in 0..16 {
        key(&mut app, 'j');
    }
    assert_eq!(selected(&app), Some((true, "ravi".into())));
    assert_eq!(app.settings().safety_scroll, 2);

    for _ in 0..15 {
        key(&mut app, 'k');
    }
    assert_eq!(app.settings().safety_selected, 1);
    assert_eq!(app.settings().safety_scroll, 2);
    key(&mut app, 'k');
    assert_eq!(app.settings().safety_selected, 0);
    assert_eq!(app.settings().safety_scroll, 0);
}

#[test]
fn nothing_scrolls_when_both_lists_fit() {
    let mut app = muted_blocked(&["amber", "bella"], &["nadia"]);
    key(&mut app, 'G');
    assert_eq!(app.settings().safety_selected, 2);
    assert_eq!(app.settings().safety_scroll, 0);
}

// --- 8. Safety: unmuting and unblocking ----------------------------------------------------------------

fn three_muted() -> App {
    let mut app = muted_blocked(&["amber", "bella", "chloe"], &["nadia", "oscar"]);
    key(&mut app, 'j');
    app
}

#[test]
fn u_unmutes_the_selected_person_and_takes_them_off_the_list_at_once() {
    let mut app = three_muted();
    let effects = key(&mut app, 'u');
    let (name, _) = one(&unmutes(&effects));
    assert_eq!(name, "bella");
    assert_eq!(effects.len(), 1, "{effects:?}");
    assert_eq!(names(&app.settings().muted.users), vec!["amber", "chloe"]);
    assert_eq!(selected(&app), Some((false, "chloe".into())));
    assert_eq!(names(&app.settings().blocked.users), vec!["nadia", "oscar"]);
}

#[test]
fn enter_does_the_same_as_u() {
    let mut app = three_muted();
    let effects = press(&mut app, KeyCode::Enter);
    assert_eq!(one(&unmutes(&effects)).0, "bella");
    assert_eq!(names(&app.settings().muted.users), vec!["amber", "chloe"]);
}

#[test]
fn u_on_a_blocked_person_unblocks_them() {
    let mut app = three_muted();
    key(&mut app, 'G');
    let effects = key(&mut app, 'u');
    assert_eq!(one(&unblocks(&effects)).0, "oscar");
    assert!(unmutes(&effects).is_empty());
    assert_eq!(names(&app.settings().blocked.users), vec!["nadia"]);
    assert_eq!(selected(&app), Some((true, "nadia".into())));
}

#[test]
fn u_twice_releases_the_next_person_not_the_same_one_again() {
    let mut app = three_muted();
    let first = one(&unmutes(&key(&mut app, 'u')));
    let second = one(&unmutes(&key(&mut app, 'u')));
    assert_eq!(first.0, "bella");
    assert_eq!(second.0, "chloe");
    assert_ne!(first.1, second.1);
    assert_eq!(names(&app.settings().muted.users), vec!["amber"]);
}

#[test]
fn u_does_nothing_on_the_other_tabs_and_with_the_nav_focused() {
    let mut app = three_muted();
    key(&mut app, '1');
    assert!(key(&mut app, 'u').is_empty());
    key(&mut app, '3');
    press(&mut app, KeyCode::Tab);
    assert!(key(&mut app, 'u').is_empty());
    assert_eq!(names(&app.settings().muted.users).len(), 3);
    press(&mut app, KeyCode::Tab);
    assert_eq!(unmutes(&key(&mut app, 'u')).len(), 1);
}

#[test]
fn the_server_taking_the_unmute_leaves_the_list_alone_and_marks_it_for_a_reload() {
    let mut app = three_muted();
    let (name, req) = one(&unmutes(&key(&mut app, 'u')));
    let effects = app.update(Action::MuteDone {
        username: name.clone(),
        on: false,
        req,
        response: Box::new(MuteResponse {
            data: person(&name),
            is_muting: false,
        }),
    });
    assert!(effects.is_empty(), "{effects:?}");
    assert_eq!(names(&app.settings().muted.users), vec!["amber", "chloe"]);
    assert!(app.settings().muted.stale);
    assert!(app.settings().muted.loaded_once);
}

#[test]
fn the_server_taking_the_unblock_leaves_the_list_alone_and_marks_it_for_a_reload() {
    let mut app = three_muted();
    key(&mut app, 'G');
    let (name, req) = one(&unblocks(&key(&mut app, 'u')));
    let effects = app.update(Action::BlockDone {
        username: name.clone(),
        on: false,
        req,
        response: Box::new(crate::model::BlockResponse {
            data: person(&name),
            is_blocking: false,
        }),
    });
    assert!(effects.is_empty(), "{effects:?}");
    assert_eq!(names(&app.settings().blocked.users), vec!["nadia"]);
    assert!(app.settings().blocked.stale);
    assert!(app.settings().blocked.loaded_once);
}

#[test]
fn a_refused_unmute_puts_the_person_back_where_they_were_and_selects_them() {
    let mut app = three_muted();
    let (name, req) = one(&unmutes(&key(&mut app, 'u')));
    app.update(Action::MuteFailed {
        username: name,
        on: false,
        req,
        failure: Failure::new("http_500"),
    });
    assert_eq!(
        names(&app.settings().muted.users),
        vec!["amber", "bella", "chloe"]
    );
    assert_eq!(selected(&app), Some((false, "bella".into())));
    assert_eq!(toast(&app), Some((ToastKind::Error, "http_500".into())));
}

#[test]
fn a_refused_unblock_puts_the_person_back_where_they_were() {
    let mut app = three_muted();
    key(&mut app, 'G');
    key(&mut app, 'k');
    let (name, req) = one(&unblocks(&key(&mut app, 'u')));
    assert_eq!(name, "nadia");
    app.update(Action::BlockFailed {
        username: name,
        on: false,
        req,
        failure: Failure::new("http_500"),
    });
    assert_eq!(names(&app.settings().blocked.users), vec!["nadia", "oscar"]);
    assert_eq!(selected(&app), Some((true, "nadia".into())));
}

#[test]
fn a_refusal_after_a_reload_that_still_lists_the_person_adds_nobody() {
    let mut app = three_muted();
    let (name, req) = one(&unmutes(&key(&mut app, 'u')));
    let reload = press(&mut app, KeyCode::Char('R'));
    load_muted(&mut app, &reload, &["amber", "bella", "chloe"], None);
    load_blocked(&mut app, &reload, &["nadia", "oscar"], None);
    app.update(Action::MuteFailed {
        username: name,
        on: false,
        req,
        failure: Failure::new("http_500"),
    });
    assert_eq!(
        names(&app.settings().muted.users),
        vec!["amber", "bella", "chloe"]
    );
}

#[test]
fn a_refusal_after_a_reload_that_dropped_the_person_puts_them_at_the_end() {
    let mut app = three_muted();
    let (name, req) = one(&unmutes(&key(&mut app, 'u')));
    let reload = press(&mut app, KeyCode::Char('R'));
    load_muted(&mut app, &reload, &["amber", "chloe"], None);
    load_blocked(&mut app, &reload, &["nadia", "oscar"], None);
    app.update(Action::MuteFailed {
        username: name,
        on: false,
        req,
        failure: Failure::new("http_500"),
    });
    assert_eq!(
        names(&app.settings().muted.users),
        vec!["amber", "chloe", "bella"]
    );
}

#[test]
fn a_reload_that_arrives_after_the_removal_wins() {
    let mut app = three_muted();
    assert_eq!(unmutes(&key(&mut app, 'u')).len(), 1);
    let reload = press(&mut app, KeyCode::Char('R'));
    load_muted(&mut app, &reload, &["amber", "bella", "chloe"], None);
    assert_eq!(
        names(&app.settings().muted.users),
        vec!["amber", "bella", "chloe"]
    );
}

// --- 9. guests ----------------------------------------------------------------------------------------------

fn guest_settings() -> App {
    let mut app = guest(120, 40);
    press(&mut app, KeyCode::Esc);
    type_str(&mut app, "gs");
    assert_eq!(
        app.screen(),
        Screen::Settings,
        "a guest could not open Settings"
    );
    assert_eq!(app.modal(), None);
    app
}

#[test]
fn a_guest_opens_settings_without_the_sign_in_dialog() {
    let app = guest_settings();
    assert_eq!(app.screen(), Screen::Settings);
    assert_eq!(app.modal(), None);
    assert_eq!(app.settings().tab, SettingsTab::Profile);
}

#[test]
fn a_guest_asks_for_no_lists_on_the_safety_tab() {
    let mut app = guest_settings();
    let effects = key(&mut app, '3');
    assert!(effects.is_empty(), "{effects:?}");
    assert_eq!(app.settings().tab, SettingsTab::Safety);
    assert!(app.settings().muted.pending.is_none());
    assert!(app.settings().blocked.pending.is_none());
}

#[test]
fn the_write_keys_ask_a_guest_to_sign_in_and_send_nothing() {
    for (c, tab) in [('S', '1'), ('D', '1')] {
        let mut app = guest_settings();
        key(&mut app, tab);
        let effects = key(&mut app, c);
        assert_eq!(app.modal(), Some(Modal::SignIn), "{c}");
        assert!(
            syncs(&effects).is_empty() && deletions(&effects).is_empty(),
            "{c}: {effects:?}"
        );
    }
    let mut app = guest_settings();
    let effects = press(&mut app, KeyCode::Enter);
    assert_eq!(app.modal(), Some(Modal::SignIn));
    assert!(syncs(&effects).is_empty(), "{effects:?}");
}

#[test]
fn the_movement_keys_do_not_ask_a_guest_to_sign_in() {
    let mut app = guest_settings();
    key(&mut app, 'j');
    key(&mut app, 'l');
    assert_eq!(app.modal(), None);
}

#[test]
fn a_session_being_restored_cannot_enter_settings_and_takes_no_settings_writes() {
    let mut app = App::new(Session::Restoring, NOW, time::UtcOffset::UTC);
    app.update(Action::Resize(120, 40));
    type_str(&mut app, "gs");
    assert!(matches!(app.screen(), Screen::Log(_)));
    assert_eq!(app.modal(), None);
    for action in [
        Action::SyncGithub,
        Action::OpenAccountDelete,
        Action::SettingsToggle,
        Action::ReleaseSelected,
        Action::OpenSelected,
        Action::SettingsText(SettingsText::Submit),
        Action::SettingsText(SettingsText::Insert('a')),
    ] {
        let effects = app.update(action.clone());
        assert!(effects.is_empty(), "{action:?}: {effects:?}");
        assert_eq!(app.modal(), None, "{action:?}");
    }
}

// --- 10. keys ------------------------------------------------------------------------------------------------

#[test]
fn the_profile_tab_keys_map_to_their_actions() {
    let app = profile_app();
    assert_eq!(mapped(&app, 'j'), Some(Action::MoveSelection(1)));
    assert_eq!(mapped(&app, 'k'), Some(Action::MoveSelection(-1)));
    assert_eq!(mapped(&app, 'l'), Some(Action::SettingsStep(1)));
    assert_eq!(mapped(&app, 'h'), Some(Action::SettingsStep(-1)));
    assert_eq!(mapped(&app, ' '), Some(Action::SettingsToggle));
    assert_eq!(mapped(&app, 'S'), Some(Action::SyncGithub));
    assert_eq!(mapped(&app, 'D'), Some(Action::OpenAccountDelete));
    assert_eq!(mapped(&app, 'R'), Some(Action::Refresh));
    assert_eq!(mapped(&app, 'u'), None);
}

#[test]
fn the_safety_tab_keys_map_to_their_actions() {
    let mut app = profile_app();
    key(&mut app, '3');
    assert_eq!(mapped(&app, 'j'), Some(Action::MoveSelection(1)));
    assert_eq!(mapped(&app, 'k'), Some(Action::MoveSelection(-1)));
    assert_eq!(mapped(&app, 'G'), Some(Action::SelectLast));
    assert_eq!(mapped(&app, 'u'), Some(Action::ReleaseSelected));
    assert_eq!(mapped(&app, 'S'), None);
    assert_eq!(mapped(&app, 'D'), None);
    key(&mut app, 'g');
    assert_eq!(mapped(&app, 'g'), Some(Action::SelectFirst));
}

#[test]
fn g_g_does_not_select_on_the_profile_tab() {
    let mut app = profile_app();
    key(&mut app, 'g');
    assert_eq!(mapped(&app, 'g'), Some(Action::CancelGPrefix));
}

#[test]
fn h_and_l_belong_to_settings_when_main_has_the_focus_on_a_narrow_screen() {
    let mut app = settings_app(60, 30);
    assert_eq!(mapped(&app, 'l'), Some(Action::SettingsStep(1)));
    assert_eq!(mapped(&app, 'h'), Some(Action::SettingsStep(-1)));
    press(&mut app, KeyCode::Tab);
    assert_eq!(mapped(&app, 'l'), Some(Action::NavStep(1)));
}

#[test]
fn the_field_maps_every_key_to_text_except_esc_enter_backspace_and_ctrl_c() {
    let app = hex_app();
    let text = |c: char| Some(Action::SettingsText(SettingsText::Insert(c)));
    for c in ['Q', 'j', '1', 'g', 'c', 'z', '?', ' '] {
        assert_eq!(mapped(&app, c), text(c), "{c}");
    }
    let map = |code| {
        crate::keymap::map_key(
            &app,
            key_event(
                code,
                crossterm::event::KeyModifiers::NONE,
                crossterm::event::KeyEventKind::Press,
            ),
        )
    };
    assert_eq!(
        map(KeyCode::Esc),
        Some(Action::SettingsText(SettingsText::Cancel))
    );
    assert_eq!(
        map(KeyCode::Enter),
        Some(Action::SettingsText(SettingsText::Submit))
    );
    assert_eq!(
        map(KeyCode::Backspace),
        Some(Action::SettingsText(SettingsText::Backspace))
    );
    let release = crate::keymap::map_key(
        &app,
        key_event(
            KeyCode::Char('a'),
            crossterm::event::KeyModifiers::NONE,
            crossterm::event::KeyEventKind::Release,
        ),
    );
    assert_eq!(release, None);
}

#[test]
fn the_username_dialog_maps_every_key_to_text_except_esc_enter_backspace_and_ctrl_c() {
    let app = dialog_app();
    let text = |c: char| Some(Action::SettingsText(SettingsText::Insert(c)));
    for c in ['Q', 'y', 'n', 'c', 'D', '1', ' '] {
        assert_eq!(mapped(&app, c), text(c), "{c}");
    }
    let map = |code| {
        crate::keymap::map_key(
            &app,
            key_event(
                code,
                crossterm::event::KeyModifiers::NONE,
                crossterm::event::KeyEventKind::Press,
            ),
        )
    };
    assert_eq!(map(KeyCode::Esc), Some(Action::CloseModal));
    assert_eq!(
        map(KeyCode::Enter),
        Some(Action::SettingsText(SettingsText::Submit))
    );
    assert_eq!(
        map(KeyCode::Backspace),
        Some(Action::SettingsText(SettingsText::Backspace))
    );
}

// --- 11. review additions -------------------------------------------------------------------------------

#[test]
fn reopening_the_dialog_while_a_deletion_is_out_opens_it_as_sending_and_sends_nothing() {
    for via_enter in [false, true] {
        let (mut app, req) = deleting();
        press(&mut app, KeyCode::Esc);
        assert_eq!(app.modal(), None);
        if via_enter {
            for _ in 0..3 {
                key(&mut app, 'j');
            }
            press(&mut app, KeyCode::Enter);
        } else {
            key(&mut app, 'D');
        }
        assert_eq!(
            app.modal(),
            Some(Modal::ConfirmAccountDelete),
            "via_enter {via_enter}"
        );
        assert_eq!(
            app.account_delete().map(|d| d.sending),
            Some(true),
            "via_enter {via_enter}"
        );
        type_str(&mut app, "myuron");
        let effects = press(&mut app, KeyCode::Enter);
        assert!(deletions(&effects).is_empty(), "{effects:?}");

        app.update(Action::AccountDeleted { req });
        assert_eq!(app.session(), &Session::Guest);
        assert_eq!(
            toast(&app),
            Some((ToastKind::Info, "Account deleted".into()))
        );
    }
}

#[test]
fn a_guest_on_the_delete_row_is_asked_to_sign_in_not_to_confirm() {
    let mut app = guest_settings();
    for _ in 0..3 {
        key(&mut app, 'j');
    }
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.modal(), Some(Modal::SignIn));
    assert_eq!(app.account_delete(), None);

    let mut direct = guest_settings();
    direct.update(Action::OpenAccountDelete);
    assert_ne!(direct.modal(), Some(Modal::ConfirmAccountDelete));
    assert_eq!(direct.account_delete(), None);
}

#[test]
fn a_guest_on_the_safety_tab_gets_no_effects_from_u_and_r() {
    let mut app = guest_settings();
    key(&mut app, '3');
    assert!(key(&mut app, 'u').is_empty());
    assert!(key(&mut app, 'R').is_empty());
    assert!(app.settings().muted.pending.is_none());
}

#[test]
fn text_actions_do_nothing_when_no_field_and_no_dialog_is_open() {
    let mut app = profile_app();
    for text in [
        SettingsText::Submit,
        SettingsText::Insert('a'),
        SettingsText::Backspace,
        SettingsText::Cancel,
    ] {
        let effects = app.update(Action::SettingsText(text));
        assert!(effects.is_empty(), "{text:?}: {effects:?}");
    }
    assert_eq!(app.modal(), None);
    assert_eq!(hex_of(&app), None);
    assert_eq!(app.account_delete(), None);
}

#[test]
fn a_guest_enters_settings_from_the_nav_without_the_sign_in_dialog() {
    let mut app = guest(120, 40);
    press(&mut app, KeyCode::Esc);
    press(&mut app, KeyCode::Tab);
    for _ in 0..4 {
        key(&mut app, 'j');
    }
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.screen(), Screen::Settings);
    assert_eq!(app.modal(), None);
}

#[test]
fn tab_is_ignored_while_the_field_is_open() {
    let mut app = hex_app();
    type_str(&mut app, "3a");
    let effects = press(&mut app, KeyCode::Tab);
    assert!(effects.is_empty(), "{effects:?}");
    assert_eq!(app.focus(), crate::app::Pane::Main);
    assert_eq!(hex_of(&app), Some("3a".into()));
}

#[test]
fn selecting_the_first_blocked_person_brings_the_blocked_heading_into_view() {
    let muted: Vec<String> = (1..=20).map(|i| format!("m{i:02}")).collect();
    let blocked: Vec<String> = (1..=20).map(|i| format!("b{i:02}")).collect();
    let m: Vec<&str> = muted.iter().map(String::as_str).collect();
    let b: Vec<&str> = blocked.iter().map(String::as_str).collect();
    let mut app = safety_app(&m, &b, 80, 24);
    key(&mut app, 'G');
    assert_eq!(app.settings().safety_scroll, 25);
    for _ in 0..19 {
        key(&mut app, 'k');
    }
    assert_eq!(selected(&app), Some((true, "b01".into())));
    assert_eq!(app.settings().safety_scroll, 22);
}

// --- 12. implementation review ----------------------------------------------------------------------------

#[test]
fn releasing_the_last_person_keeps_the_empty_text_and_the_next_visit_fetches_again() {
    for blocked in [false, true] {
        let (muted, other): (&[&str], &[&str]) = if blocked {
            (&[], &["nadia"])
        } else {
            (&["amber"], &[])
        };
        let mut app = muted_blocked(muted, other);
        let effects = key(&mut app, 'u');
        let (name, req) = if blocked {
            one(&unblocks(&effects))
        } else {
            one(&unmutes(&effects))
        };
        let done = if blocked {
            Action::BlockDone {
                username: name.clone(),
                on: false,
                req,
                response: Box::new(crate::model::BlockResponse {
                    data: person(&name),
                    is_blocking: false,
                }),
            }
        } else {
            Action::MuteDone {
                username: name.clone(),
                on: false,
                req,
                response: Box::new(MuteResponse {
                    data: person(&name),
                    is_muting: false,
                }),
            }
        };
        app.update(done);
        let list = if blocked {
            &app.settings().blocked
        } else {
            &app.settings().muted
        };
        assert!(list.loaded_once, "blocked {blocked}");
        assert!(list.stale, "blocked {blocked}");
        key(&mut app, '1');
        let again = open_safety(&mut app);
        let fetches = if blocked {
            blocked_fetches(&again)
        } else {
            muted_fetches(&again)
        };
        assert_eq!(fetches, vec![None], "blocked {blocked}");
        let reloaded = if blocked {
            &app.settings().blocked
        } else {
            &app.settings().muted
        };
        assert!(reloaded.pending.is_some());
    }
}

#[test]
fn a_deletion_refused_by_the_device_drops_its_record() {
    let (mut app, req) = deleting();
    app.update(Action::AccountDeleteFailed {
        req,
        failure: Failure::new(NOT_APPROVED),
    });
    assert_eq!(app.session(), &Session::Guest);
    app.update(Action::Tick(NOW + time::Duration::seconds(10)));
    let effects = app.update(Action::AccountDeleted { req });
    assert!(effects.is_empty(), "{effects:?}");
    assert_eq!(toast(&app), None);
}

#[test]
fn a_lane_in_flight_keeps_the_hex_field_closed() {
    let mut app = lane_app_at(None, 4);
    one(&lanes(&press(&mut app, KeyCode::Enter)));
    key(&mut app, 'l');
    key(&mut app, 'l');
    key(&mut app, 'l');
    assert_eq!(lane_choice(&app), 7);
    let effects = press(&mut app, KeyCode::Enter);
    assert!(effects.is_empty(), "{effects:?}");
    assert_eq!(hex_of(&app), None);
}

#[test]
fn only_a_lane_answer_puts_the_cursor_back() {
    let mut app = lane_app_at(None, 3);
    key(&mut app, 'j');
    let (_, req) = one(&modes(&press(&mut app, KeyCode::Enter)));
    app.update(Action::MeWriteDone {
        kind: MeWrite::PasteisMode,
        req,
        response: answer(0, json!({"pasteis_mode": false})),
    });
    assert_eq!(app.settings().lane_cursor, Some(3));
}

#[test]
fn submitting_after_leaving_the_profile_tab_sends_nothing_and_closes_the_dialog() {
    let mut app = dialog_app();
    type_str(&mut app, "myuron");
    app.update(Action::Goto(Screen::Log(crate::app::LogTab::Latest)));
    let effects = app.update(Action::SettingsText(SettingsText::Submit));
    assert!(deletions(&effects).is_empty(), "{effects:?}");
    assert_eq!(app.account_delete(), None);
    assert_eq!(app.modal(), None);

    let mut other_tab = dialog_app();
    type_str(&mut other_tab, "myuron");
    other_tab.update(Action::SwitchSettingsTab(SettingsTab::Safety));
    let effects = other_tab.update(Action::SettingsText(SettingsText::Submit));
    assert!(deletions(&effects).is_empty(), "{effects:?}");
}

#[test]
fn a_sync_that_renames_you_asks_for_the_profile_under_the_new_name_only() {
    let mut app = signed_in(120, 40);
    let opened = open_profile(&mut app, "myuron");
    deliver_user(&mut app, &opened, "myuron", user_resp("myuron", json!({})));
    app.update(Action::Open(Screen::Settings));
    let req = one(&syncs(&key(&mut app, 'S')));
    let effects = app.update(Action::MeWriteDone {
        kind: MeWrite::Sync,
        req,
        response: answer(0, json!({"username": "renamed"})),
    });
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::FetchUser { username, .. } if username == "renamed")),
        "{effects:?}"
    );
    assert!(app.profile("myuron").is_none());
    assert!(app.profile("renamed").is_some());
}
