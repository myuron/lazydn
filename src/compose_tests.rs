use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};
use serde_json::json;
use time::{Duration, UtcOffset};

use crate::action::{Action, Effect, Failure};
use crate::api::pkce::Pkce;
use crate::app::{App, KeyStatus, LogTab, Me, Modal, Pane, Screen, Session, ToastKind};
use crate::compose::{CounterTone, Surface, counter};
use crate::drafts::{DraftKey, Drafts};
use crate::editor::Editor;
use crate::feed::{FeedKey, ReqId};
use crate::feed_tests::{FOLLOWING, LATEST, deliver, page, started};
use crate::keymap::map_event;
use crate::model::Commit;
use crate::notifications_tests::{me_json, me_resp};
use crate::test_support::*;
use crate::thread_tests::{commit_json, commit_of, merge};

const ACCOUNT: &str = "0190a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b";
const KEY_PATH: &str = "/home/user/.config/lazydn/id_ed25519";
const CODE: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const NOT_APPROVED: &str = "device_not_approved";

// --- helpers ------------------------------------------------------------------

fn account_me() -> Me {
    Me {
        id: ACCOUNT.into(),
        ..fixture_me()
    }
}

/// Signed in as an account whose id may name a drafts file; `signed_in` has an id that may not.
fn account_app() -> App {
    let mut app = App::new(Session::SignedIn(account_me()), NOW, UtcOffset::UTC);
    app.update(Action::Resize(120, 40));
    app
}

fn guest_without_dialog() -> App {
    let mut app = guest(120, 40);
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.modal(), None);
    app
}

fn at(seconds: i64) -> time::OffsetDateTime {
    NOW + Duration::seconds(seconds)
}

/// Types `text` the way a person would: Enter for each line break.
fn write(app: &mut App, text: &str) {
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            press(app, KeyCode::Enter);
        }
        type_str(app, line);
    }
}

fn paste(app: &mut App, text: &str) -> Vec<Effect> {
    app.update(Action::Paste(text.into()))
}

fn draft(app: &App) -> Option<String> {
    app.drafts().text(&DraftKey::New)
}

fn submit(app: &mut App) -> Vec<Effect> {
    ctrl(app, 's')
}

fn ctrl_enter(app: &mut App) -> Vec<Effect> {
    send(
        app,
        key_event(KeyCode::Enter, KeyModifiers::CONTROL, KeyEventKind::Press),
    )
}

/// An open inline composer on Log with `text` typed and not yet sent.
fn composing(text: &str) -> App {
    let mut app = signed_in(120, 40);
    type_str(&mut app, "c");
    write(&mut app, text);
    assert!(app.composer_focused(), "c did not open the composer");
    app
}

/// Like `composing`, with the text sent; returns the request the commit went out with.
fn sent(text: &str) -> (App, ReqId) {
    let mut app = composing(text);
    let effects = submit(&mut app);
    let req = create_req(&effects);
    assert!(app.compose().is_some_and(|c| c.submitting), "{effects:?}");
    (app, req)
}

fn creates(effects: &[Effect]) -> Vec<&Effect> {
    effects
        .iter()
        .filter(|e| matches!(e, Effect::CreateCommit { .. }))
        .collect()
}

/// Falls back to a bogus id so that a missing effect fails the assertions, not an unwrap.
fn create_req(effects: &[Effect]) -> ReqId {
    effects
        .iter()
        .find_map(|e| match e {
            Effect::CreateCommit { req, .. } => Some(*req),
            _ => None,
        })
        .unwrap_or(ReqId(u64::MAX))
}

fn saves(effects: &[Effect]) -> Vec<(String, Drafts)> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::SaveDrafts { account, drafts } => Some((account.clone(), drafts.clone())),
            _ => None,
        })
        .collect()
}

fn loads(effects: &[Effect]) -> Vec<String> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::LoadDrafts { account } => Some(account.clone()),
            _ => None,
        })
        .collect()
}

fn new_commit(status: &str) -> Commit {
    commit_of(merge(
        commit_json("n1", "myuron", 5),
        json!({"short_hash": "a1b2c3d", "status": status}),
    ))
}

fn created(req: ReqId, commit: Commit) -> Action {
    Action::CommitCreated {
        req,
        commit: Box::new(commit),
    }
}

fn failed(req: ReqId, failure: Failure) -> Action {
    Action::CommitFailed { req, failure }
}

fn failure_with(code: &str, message: &str) -> Failure {
    Failure {
        message: Some(message.into()),
        ..Failure::new(code)
    }
}

fn toast(app: &App) -> Option<(ToastKind, String)> {
    app.toast().map(|t| (t.kind, t.text.clone()))
}

fn ids(app: &App, key: FeedKey) -> Vec<String> {
    app.feed(key).items.iter().map(|c| c.id.clone()).collect()
}

/// Signed in with the Following list loaded with `n` items, and the Latest list too when `latest` is set.
fn with_lists(n: usize, latest: bool) -> App {
    let (mut app, effects) = started(120, 40);
    deliver(&mut app, FOLLOWING, &effects, page(0, n, None));
    if latest {
        let fetch = type_str(&mut app, "2");
        deliver(&mut app, LATEST, &fetch, page(0, n, None));
        type_str(&mut app, "1");
    }
    app
}

fn screen_text(app: &App) -> String {
    let terminal = render(app);
    let buffer = terminal.backend().buffer().clone();
    (0..buffer.area.height)
        .map(|y| row_text(&buffer, y))
        .collect::<Vec<_>>()
        .join("\n")
}

fn is_signed_in(app: &App) -> bool {
    matches!(app.session(), Session::SignedIn(_))
}

// --- counter -----------------------------------------------------------------

#[test]
fn the_counter_is_hidden_while_more_than_200_characters_are_left() {
    assert_eq!(counter(0), None);
    assert_eq!(counter(3799), None);
}

#[test]
fn the_counter_shows_from_200_left() {
    let shown = counter(3800).expect("200 left is shown");
    assert_eq!(shown.remaining, 200);
    assert_eq!(shown.tone, CounterTone::Normal);
    assert_eq!(
        counter(3979).map(|c| (c.remaining, c.tone)),
        Some((21, CounterTone::Normal))
    );
}

#[test]
fn the_counter_turns_yellow_from_20_left() {
    assert_eq!(
        counter(3980).map(|c| (c.remaining, c.tone)),
        Some((20, CounterTone::Warn))
    );
    assert_eq!(
        counter(3999).map(|c| (c.remaining, c.tone)),
        Some((1, CounterTone::Warn))
    );
}

#[test]
fn the_counter_is_red_at_zero_and_below() {
    assert_eq!(
        counter(4000).map(|c| (c.remaining, c.tone)),
        Some((0, CounterTone::Over))
    );
    assert_eq!(
        counter(4001).map(|c| (c.remaining, c.tone)),
        Some((-1, CounterTone::Over))
    );
    assert_eq!(
        counter(4500).map(|c| (c.remaining, c.tone)),
        Some((-500, CounterTone::Over))
    );
}

// --- opening and closing -----------------------------------------------------------

#[test]
fn c_on_log_opens_the_composer_inline() {
    let mut app = signed_in(120, 40);

    let effects = type_str(&mut app, "c");

    let compose = app.compose().expect("a composer is open");
    assert_eq!(compose.key, DraftKey::New);
    assert_eq!(compose.surface, Surface::Inline);
    assert!(!compose.submitting);
    assert_eq!(app.modal(), None);
    assert!(app.composer_focused());
    assert_eq!(app.focus(), Pane::Main);
    assert!(effects.is_empty(), "{effects:?}");
}

#[test]
fn c_opens_the_composer_inline_on_every_log_tab() {
    for tab in ["1", "2", "3"] {
        let mut app = signed_in(120, 40);
        type_str(&mut app, tab);

        type_str(&mut app, "c");

        assert_eq!(
            app.compose().map(|c| c.surface),
            Some(Surface::Inline),
            "tab {tab}"
        );
        assert_eq!(app.modal(), None, "tab {tab}");
    }
}

#[test]
fn c_from_the_nav_pane_moves_the_focus_to_main() {
    let mut app = signed_in(120, 40);
    press(&mut app, KeyCode::Tab);
    assert_eq!(app.focus(), Pane::Nav);

    type_str(&mut app, "c");

    assert_eq!(app.focus(), Pane::Main);
    assert_eq!(app.compose().map(|c| c.surface), Some(Surface::Inline));
}

#[test]
fn c_on_any_other_screen_opens_the_composer_as_a_modal() {
    let screens = [
        Screen::Saved,
        Screen::Settings,
        Screen::Notifications,
        Screen::Profile("myuron".into()),
    ];
    for screen in screens {
        let mut app = signed_in(120, 40);
        app.update(Action::Goto(screen.clone()));

        let effects = type_str(&mut app, "c");

        let compose = app
            .compose()
            .unwrap_or_else(|| panic!("no composer on {screen:?}"));
        assert_eq!(compose.surface, Surface::Modal, "{screen:?}");
        assert_eq!(app.modal(), Some(Modal::Composer), "{screen:?}");
        assert!(app.composer_focused(), "{screen:?}");
        assert_eq!(app.screen(), screen);
        assert!(creates(&effects).is_empty(), "{screen:?}");
    }
}

#[test]
fn c_on_a_thread_or_a_follow_list_opens_the_composer_as_a_modal() {
    for screen in [
        Screen::Thread("c1".into()),
        Screen::Connections("myuron".into(), crate::app::ConnTab::Followers),
    ] {
        let mut app = signed_in(120, 40);
        app.update(Action::Open(screen.clone()));

        type_str(&mut app, "c");

        assert_eq!(app.modal(), Some(Modal::Composer), "{screen:?}");
        assert_eq!(app.screen(), screen);
    }
}

#[test]
fn a_guests_c_opens_the_sign_in_dialog_and_no_composer() {
    let mut app = guest_without_dialog();

    type_str(&mut app, "c");

    assert_eq!(app.modal(), Some(Modal::SignIn));
    assert!(app.compose().is_none());
    assert!(!app.composer_focused());
}

#[test]
fn c_while_restoring_does_nothing() {
    let mut app = App::new(Session::Restoring, NOW, UtcOffset::UTC);
    app.update(Action::Resize(120, 40));

    let effects = type_str(&mut app, "c");
    let direct = app.update(Action::Commit);

    assert!(app.compose().is_none());
    assert_eq!(app.modal(), None);
    assert!(effects.is_empty() && direct.is_empty());
}

#[test]
fn esc_closes_the_inline_composer_and_keeps_the_draft() {
    let mut app = composing("hello");
    let screen = app.screen();

    press(&mut app, KeyCode::Esc);

    assert!(app.compose().is_none());
    assert!(!app.composer_focused());
    assert_eq!(draft(&app), Some("hello".to_string()));
    assert_eq!(app.screen(), screen);
    assert_eq!(app.focus(), Pane::Main);
}

#[test]
fn esc_closes_the_modal_composer_and_keeps_the_draft() {
    let mut app = signed_in(120, 40);
    app.update(Action::Goto(Screen::Saved));
    type_str(&mut app, "c");
    write(&mut app, "hello");

    press(&mut app, KeyCode::Esc);

    assert!(app.compose().is_none());
    assert_eq!(app.modal(), None);
    assert_eq!(app.screen(), Screen::Saved);
    assert_eq!(draft(&app), Some("hello".to_string()));
}

#[test]
fn opening_and_closing_without_typing_leaves_no_draft() {
    let mut app = signed_in(120, 40);

    type_str(&mut app, "c");
    assert!(app.compose().is_some());
    press(&mut app, KeyCode::Esc);

    assert!(app.drafts().is_empty());
}

#[test]
fn the_inline_composer_and_the_modal_share_one_draft() {
    let mut app = signed_in(120, 40);
    type_str(&mut app, "c");
    write(&mut app, "hello");
    press(&mut app, KeyCode::Esc);

    app.update(Action::Goto(Screen::Saved));
    type_str(&mut app, "c");
    assert_eq!(draft(&app), Some("hello".to_string()));
    type_str(&mut app, "!");
    press(&mut app, KeyCode::Esc);

    app.update(Action::Goto(Screen::Log(LogTab::Following)));
    type_str(&mut app, "c");
    type_str(&mut app, "?");

    assert_eq!(draft(&app), Some("hello!?".to_string()));
}

#[test]
fn the_cursor_of_a_reopened_draft_is_at_its_end() {
    let mut app = composing("ab\ncd");
    press(&mut app, KeyCode::Esc);

    type_str(&mut app, "c");
    type_str(&mut app, "X");

    assert_eq!(draft(&app), Some("ab\ncdX".to_string()));
}

#[test]
fn opening_the_composer_scrolls_the_list_back_to_the_top() {
    let mut app = with_lists(30, false);
    type_str(&mut app, "G");
    assert!(app.feed(FOLLOWING).scroll > 0);

    type_str(&mut app, "c");

    assert_eq!(app.feed(FOLLOWING).scroll, 0);
}

// --- keys while composing --------------------------------------------------------------

#[test]
fn keys_with_a_global_meaning_are_typed_into_the_inline_composer() {
    let mut app = composing("");

    let effects = type_str(&mut app, "jkqQg?cr1@L");

    assert_eq!(draft(&app), Some("jkqQg?cr1@L".to_string()));
    assert!(!quit_requested(&effects));
    assert_eq!(app.modal(), None);
    assert_eq!(app.screen(), Screen::Log(LogTab::Following));
    assert!(!app.g_pending());
}

#[test]
fn keys_with_a_global_meaning_are_typed_into_the_modal_composer() {
    let mut app = signed_in(120, 40);
    app.update(Action::Goto(Screen::Saved));
    type_str(&mut app, "c");

    let effects = type_str(&mut app, "jkqQg?cr1@L");

    assert_eq!(draft(&app), Some("jkqQg?cr1@L".to_string()));
    assert!(!quit_requested(&effects));
    assert_eq!(app.modal(), Some(Modal::Composer));
    assert_eq!(app.screen(), Screen::Saved);
}

#[test]
fn ctrl_c_quits_from_either_composer() {
    let mut inline = composing("x");
    assert!(quit_requested(&ctrl(&mut inline, 'c')));

    let mut modal = signed_in(120, 40);
    modal.update(Action::Goto(Screen::Saved));
    type_str(&mut modal, "c");
    assert!(modal.composer_focused());
    assert!(quit_requested(&ctrl(&mut modal, 'c')));
}

#[test]
fn enter_starts_a_new_line_and_backspace_joins_it_back() {
    let mut app = composing("ab");

    press(&mut app, KeyCode::Enter);
    type_str(&mut app, "c");
    assert_eq!(draft(&app), Some("ab\nc".to_string()));

    press(&mut app, KeyCode::Backspace);
    press(&mut app, KeyCode::Backspace);
    assert_eq!(draft(&app), Some("ab".to_string()));
}

#[test]
fn the_arrows_home_end_and_delete_edit_in_place() {
    let mut app = composing("abc");

    press(&mut app, KeyCode::Home);
    press(&mut app, KeyCode::Delete);
    assert_eq!(draft(&app), Some("bc".to_string()));

    press(&mut app, KeyCode::End);
    type_str(&mut app, "d");
    press(&mut app, KeyCode::Left);
    press(&mut app, KeyCode::Left);
    type_str(&mut app, "X");
    press(&mut app, KeyCode::Right);
    type_str(&mut app, "Y");

    assert_eq!(draft(&app), Some("bXcYd".to_string()));
}

#[test]
fn up_and_down_move_between_lines() {
    let mut app = composing("ab\ncd");

    press(&mut app, KeyCode::Up);
    type_str(&mut app, "X");
    press(&mut app, KeyCode::Down);
    type_str(&mut app, "Y");

    assert_eq!(draft(&app), Some("abX\ncdY".to_string()));
}

#[test]
fn tab_inserts_four_spaces_and_does_not_move_the_focus() {
    let mut app = composing("a");

    press(&mut app, KeyCode::Tab);

    assert_eq!(draft(&app), Some("a    ".to_string()));
    assert_eq!(app.focus(), Pane::Main);
}

#[test]
fn ctrl_w_deletes_a_word_and_ctrl_u_the_line() {
    let mut app = composing("foo bar baz");

    ctrl(&mut app, 'w');
    assert_eq!(draft(&app), Some("foo bar ".to_string()));

    ctrl(&mut app, 'u');
    assert_eq!(draft(&app), None);
}

#[test]
fn ctrl_j_and_shift_enter_start_a_new_line() {
    let mut app = composing("a");
    ctrl(&mut app, 'j');
    send(
        &mut app,
        key_event(KeyCode::Enter, KeyModifiers::SHIFT, KeyEventKind::Press),
    );
    type_str(&mut app, "b");

    assert_eq!(draft(&app), Some("a\n\nb".to_string()));
}

#[test]
fn alt_with_a_character_types_nothing() {
    let mut app = composing("a");

    send(
        &mut app,
        key_event(KeyCode::Char('x'), KeyModifiers::ALT, KeyEventKind::Press),
    );

    assert_eq!(draft(&app), Some("a".to_string()));
}

#[test]
fn text_beyond_8000_characters_is_not_taken() {
    let mut app = composing("");
    paste(&mut app, &"a".repeat(7999));

    type_str(&mut app, "bcd");

    assert_eq!(draft(&app).map(|t| t.chars().count()), Some(8000));
}

// --- paste ------------------------------------------------------------------------

#[test]
fn a_paste_goes_to_the_open_composer_and_keeps_its_line_breaks() {
    let mut app = composing("x");

    let effects = paste(&mut app, "a\r\nb\rc");

    assert_eq!(draft(&app), Some("xa\nb\nc".to_string()));
    assert!(effects.is_empty(), "{effects:?}");
}

#[test]
fn a_paste_reaches_the_modal_composer_too() {
    let mut app = signed_in(120, 40);
    app.update(Action::Goto(Screen::Saved));
    type_str(&mut app, "c");

    paste(&mut app, "pasted");

    assert_eq!(draft(&app), Some("pasted".to_string()));
}

#[test]
fn the_event_loop_hands_a_paste_to_an_open_composer() {
    let app = composing("");

    let action = map_event(&app, &Event::Paste("hi".into()));

    assert_eq!(action, Some(Action::Paste("hi".into())));
}

#[test]
fn the_event_loop_drops_a_paste_when_nothing_takes_text() {
    let app = signed_in(120, 40);

    assert_eq!(map_event(&app, &Event::Paste("hi".into())), None);
}

#[test]
fn a_paste_with_no_composer_and_no_dialog_changes_nothing() {
    let mut app = signed_in(120, 40);

    let effects = paste(&mut app, "stray");

    assert!(effects.is_empty());
    assert!(app.drafts().is_empty());
    assert!(app.compose().is_none());
}

#[test]
fn a_paste_cut_at_8000_characters_says_so() {
    let mut app = composing("");

    paste(&mut app, &"a".repeat(8100));

    assert_eq!(draft(&app).map(|t| t.chars().count()), Some(8000));
    assert_eq!(
        toast(&app),
        Some((
            ToastKind::Info,
            "pasted text was cut at 8000 characters".to_string()
        ))
    );
}

#[test]
fn a_paste_that_fits_shows_no_toast() {
    let mut app = composing("");

    paste(&mut app, &"a".repeat(8000));

    assert_eq!(app.toast(), None);
    assert_eq!(draft(&app).map(|t| t.chars().count()), Some(8000));
}

// --- submit -----------------------------------------------------------------------

#[test]
fn ctrl_s_sends_the_text_as_a_new_commit() {
    let mut app = composing("hello");

    let effects = submit(&mut app);

    assert_eq!(effects.len(), 1, "{effects:?}");
    assert_eq!(
        effects[0],
        Effect::CreateCommit {
            body: "hello".into(),
            parent_id: None,
            quoted_commit_id: None,
            req: create_req(&effects),
        }
    );
    let compose = app
        .compose()
        .expect("the composer stays open until the answer");
    assert!(compose.submitting);
    assert_eq!(app.in_flight().map(|f| f.sent_text.as_str()), Some("hello"));
}

#[test]
fn ctrl_enter_sends_it_too() {
    let mut app = composing("hello");

    let effects = ctrl_enter(&mut app);

    assert_eq!(creates(&effects).len(), 1, "{effects:?}");
    assert!(app.compose().is_some_and(|c| c.submitting));
}

#[test]
fn the_modal_composer_sends_too() {
    let mut app = signed_in(120, 40);
    app.update(Action::Goto(Screen::Saved));
    type_str(&mut app, "c");
    write(&mut app, "from a modal");

    let effects = submit(&mut app);

    assert!(matches!(
        effects.as_slice(),
        [Effect::CreateCommit { body, parent_id: None, quoted_commit_id: None, .. }] if body == "from a modal"
    ));
}

#[test]
fn the_text_is_sent_as_typed_and_left_to_the_server_to_normalize() {
    let mut app = composing("a\n\n\n\nb  ");

    let effects = submit(&mut app);

    assert!(matches!(
        effects.as_slice(),
        [Effect::CreateCommit { body, .. }] if body == "a\n\n\n\nb  "
    ));
}

#[test]
fn nothing_is_sent_for_an_empty_or_blank_text() {
    for text in ["", "   ", "\n\n", " \n \n ", "\u{200b}\u{feff}"] {
        let mut app = composing(text);

        let effects = submit(&mut app);

        assert!(creates(&effects).is_empty(), "{text:?}");
        assert!(app.compose().is_some_and(|c| !c.submitting), "{text:?}");
    }
}

#[test]
fn one_character_is_enough() {
    let mut app = composing("a");

    assert_eq!(creates(&submit(&mut app)).len(), 1);
}

#[test]
fn four_thousand_characters_are_sent_and_one_more_is_not() {
    let mut ok = composing("");
    paste(&mut ok, &"a".repeat(4000));
    assert_eq!(creates(&submit(&mut ok)).len(), 1);

    let mut too_long = composing("");
    paste(&mut too_long, &"a".repeat(4001));
    assert!(creates(&submit(&mut too_long)).is_empty());
    assert!(too_long.compose().is_some_and(|c| !c.submitting));
}

#[test]
fn the_limit_counts_code_points_after_the_server_normalizes() {
    let mut emoji = composing("");
    paste(&mut emoji, &"🙂".repeat(4000));
    assert_eq!(creates(&submit(&mut emoji)).len(), 1);
    let mut emoji_over = composing("");
    paste(&mut emoji_over, &"🙂".repeat(4001));
    assert!(creates(&submit(&mut emoji_over)).is_empty());

    let mut collapsed = composing("");
    paste(
        &mut collapsed,
        &format!("{}{}{}", "x".repeat(3990), "\n".repeat(10), "y".repeat(3)),
    );
    assert_eq!(creates(&submit(&mut collapsed)).len(), 1);
}

#[test]
fn a_second_submit_while_the_first_is_out_sends_nothing() {
    let (mut app, _) = sent("hello");

    let again = submit(&mut app);
    let again_enter = ctrl_enter(&mut app);

    assert!(creates(&again).is_empty());
    assert!(creates(&again_enter).is_empty());
}

#[test]
fn closing_and_reopening_while_the_commit_is_out_does_not_allow_a_second_one() {
    let (mut app, _) = sent("hello");
    press(&mut app, KeyCode::Esc);

    type_str(&mut app, "c");
    let effects = submit(&mut app);

    assert!(creates(&effects).is_empty(), "{effects:?}");
}

#[test]
fn a_reopened_composer_still_shows_the_commit_as_out_and_closes_with_the_answer() {
    let (mut app, req) = sent("hello");
    assert_eq!(app.in_flight().map(|f| f.req), Some(req));
    press(&mut app, KeyCode::Esc);
    assert!(app.compose().is_none());
    assert_eq!(app.in_flight().map(|f| f.req), Some(req));

    type_str(&mut app, "c");

    assert!(app.compose().is_some_and(|c| c.submitting));
    assert!(screen_text(&app).contains("Esc cancel  Committing…"));
    app.update(created(req, new_commit("pending")));
    assert!(app.compose().is_none());
    assert_eq!(draft(&app), None);
    assert_eq!(app.in_flight(), None);
}

#[test]
fn typing_is_ignored_while_the_commit_is_out_but_esc_and_ctrl_c_still_act() {
    let (mut app, _) = sent("hello");

    type_str(&mut app, "x");
    press(&mut app, KeyCode::Enter);
    press(&mut app, KeyCode::Backspace);
    paste(&mut app, "pasted");
    ctrl(&mut app, 'w');
    assert_eq!(draft(&app), Some("hello".to_string()));

    assert!(quit_requested(&ctrl(&mut app, 'c')));
    press(&mut app, KeyCode::Esc);
    assert!(app.compose().is_none());
}

#[test]
fn each_submit_gets_a_new_request_id() {
    let (mut app, first) = sent("hello");
    app.update(failed(first, Failure::new("http_500")));

    let again = submit(&mut app);

    assert!(create_req(&again).0 > first.0);
}

// --- the commit was created -----------------------------------------------------------

#[test]
fn a_created_commit_closes_the_composer_and_clears_the_draft() {
    let (mut app, req) = sent("hello");

    let effects = app.update(created(req, new_commit("pending")));

    assert!(app.compose().is_none());
    assert!(!app.composer_focused());
    assert!(app.drafts().is_empty());
    assert!(effects.is_empty(), "{effects:?}");
}

#[test]
fn a_created_commit_closes_the_modal_too() {
    let mut app = signed_in(120, 40);
    app.update(Action::Goto(Screen::Saved));
    type_str(&mut app, "c");
    write(&mut app, "hello");
    assert_eq!(app.modal(), Some(Modal::Composer));
    let req = create_req(&submit(&mut app));

    app.update(created(req, new_commit("pending")));

    assert_eq!(app.modal(), None);
    assert!(app.compose().is_none());
    assert_eq!(app.screen(), Screen::Saved);
}

#[test]
fn the_toast_for_a_pending_commit_says_it_is_in_review() {
    let (mut app, req) = sent("hello");

    app.update(created(req, new_commit("pending")));

    assert_eq!(
        toast(&app),
        Some((
            ToastKind::Success,
            "Committed a1b2c3d (in review)".to_string()
        ))
    );
}

#[test]
fn the_toast_for_an_approved_commit_does_not() {
    let (mut app, req) = sent("hello");

    app.update(created(req, new_commit("approved")));

    assert_eq!(
        toast(&app),
        Some((ToastKind::Success, "Committed a1b2c3d".to_string()))
    );
}

#[test]
fn control_characters_in_the_hash_are_left_out_of_the_toast() {
    let (mut app, req) = sent("hello");
    let commit = commit_of(merge(
        commit_json("n1", "myuron", 5),
        json!({"short_hash": "a1\u{202e}b2\u{7}c", "status": "approved"}),
    ));

    app.update(created(req, commit));

    assert_eq!(
        toast(&app).map(|t| t.1),
        Some("Committed a1b2c".to_string())
    );
}

#[test]
fn a_created_commit_goes_to_the_head_of_following_and_latest() {
    let mut app = with_lists(3, true);
    type_str(&mut app, "c");
    write(&mut app, "hello");
    let req = create_req(&submit(&mut app));

    app.update(created(req, new_commit("pending")));

    assert_eq!(ids(&app, FOLLOWING), ["n1", "c0", "c1", "c2"]);
    assert_eq!(ids(&app, LATEST), ["n1", "c0", "c1", "c2"]);
}

#[test]
fn trending_and_saved_are_left_alone() {
    let mut app = with_lists(3, true);
    type_str(&mut app, "c");
    write(&mut app, "hello");
    let req = create_req(&submit(&mut app));

    app.update(created(req, new_commit("pending")));

    assert_eq!(ids(&app, FOLLOWING)[0], "n1");
    assert!(app.feed(FeedKey::Log(LogTab::Trending)).items.is_empty());
    assert!(app.feed(FeedKey::Saved).items.is_empty());
}

#[test]
fn a_list_that_was_never_loaded_stays_unloaded() {
    let mut app = with_lists(3, false);
    type_str(&mut app, "c");
    write(&mut app, "hello");
    let req = create_req(&submit(&mut app));

    app.update(created(req, new_commit("pending")));

    assert_eq!(ids(&app, FOLLOWING), ["n1", "c0", "c1", "c2"]);
    let latest = app.feed(LATEST);
    assert!(latest.items.is_empty());
    assert!(!latest.loaded_once);
    press(&mut app, KeyCode::Esc);
    let effects = type_str(&mut app, "2");
    assert!(
        effects.iter().any(|e| matches!(
            e,
            Effect::FetchLog {
                tab: LogTab::Latest,
                ..
            }
        )),
        "{effects:?}"
    );
}

#[test]
fn the_selected_commit_stays_selected_when_one_is_put_in_front() {
    let mut app = with_lists(5, false);
    type_str(&mut app, "jj");
    assert_eq!(app.feed(FOLLOWING).selected, 2);
    type_str(&mut app, "c");
    write(&mut app, "hello");
    let req = create_req(&submit(&mut app));

    app.update(created(req, new_commit("pending")));

    let feed = app.feed(FOLLOWING);
    assert_eq!(feed.selected, 3);
    assert_eq!(feed.items[feed.selected].id, "c2");
}

#[test]
fn with_the_first_commit_selected_the_selection_stays_at_the_top() {
    let mut app = with_lists(3, false);
    type_str(&mut app, "c");
    write(&mut app, "hello");
    let req = create_req(&submit(&mut app));

    app.update(created(req, new_commit("pending")));

    assert_eq!(ids(&app, FOLLOWING)[0], "n1");
    assert_eq!(app.feed(FOLLOWING).selected, 0);
}

#[test]
fn a_commit_already_in_the_list_is_not_put_in_again() {
    let mut app = with_lists(3, false);
    type_str(&mut app, "c");
    write(&mut app, "hello");
    let req = create_req(&submit(&mut app));
    let known = commit_of(merge(
        commit_json("c1", "myuron", 5),
        json!({"short_hash": "a1b2c3d", "status": "pending"}),
    ));

    app.update(created(req, known));

    assert_eq!(ids(&app, FOLLOWING), ["c0", "c1", "c2"]);
    assert!(toast(&app).is_some());
}

#[test]
fn a_list_being_replaced_is_left_for_the_answer_to_fill() {
    let mut app = with_lists(3, false);
    type_str(&mut app, "c");
    write(&mut app, "hello");
    let req = create_req(&submit(&mut app));
    assert!(app.compose().is_some_and(|c| c.submitting));
    let refresh = app.update(Action::Refresh);
    assert!(refresh.iter().any(|e| matches!(e, Effect::FetchLog { .. })));

    app.update(created(req, new_commit("pending")));

    assert!(app.compose().is_none());
    assert_eq!(ids(&app, FOLLOWING), ["c0", "c1", "c2"]);
}

#[test]
fn a_list_that_is_only_loading_a_further_page_still_takes_it() {
    let (mut app, effects) = started(120, 40);
    deliver(&mut app, FOLLOWING, &effects, page(0, 3, Some("next")));
    type_str(&mut app, "c");
    write(&mut app, "hello");
    let req = create_req(&submit(&mut app));
    let more = app.update(Action::MoveSelection(2));
    assert!(more.iter().any(|e| matches!(
        e,
        Effect::FetchLog {
            cursor: Some(_),
            ..
        }
    )));

    app.update(created(req, new_commit("pending")));

    assert_eq!(ids(&app, FOLLOWING)[0], "n1");
}

#[test]
fn a_created_commit_does_not_change_the_commit_count_of_the_user() {
    let (mut app, req) = sent("hello");

    app.update(created(req, new_commit("pending")));

    match app.session() {
        Session::SignedIn(me) => assert_eq!(me.commits, 42),
        other => panic!("{other:?}"),
    }
}

#[test]
fn an_answer_for_another_request_is_still_put_in_the_list_and_announced() {
    let mut app = with_lists(3, false);
    type_str(&mut app, "c");
    write(&mut app, "hello");
    let req = create_req(&submit(&mut app));
    let other = ReqId(req.0.wrapping_add(100));

    let effects = app.update(created(other, new_commit("pending")));

    assert_eq!(ids(&app, FOLLOWING)[0], "n1");
    assert_eq!(
        toast(&app),
        Some((
            ToastKind::Success,
            "Committed a1b2c3d (in review)".to_string()
        ))
    );
    let compose = app
        .compose()
        .expect("the request in flight still owns the composer");
    assert!(compose.submitting);
    assert_eq!(draft(&app), Some("hello".to_string()));
    assert!(effects.is_empty(), "{effects:?}");
}

#[test]
fn an_answer_after_esc_still_clears_the_draft_and_is_put_in_the_list() {
    let mut app = with_lists(3, false);
    type_str(&mut app, "c");
    write(&mut app, "hello");
    let req = create_req(&submit(&mut app));
    press(&mut app, KeyCode::Esc);
    assert_eq!(draft(&app), Some("hello".to_string()));

    app.update(created(req, new_commit("pending")));

    assert!(app.drafts().is_empty());
    assert_eq!(ids(&app, FOLLOWING)[0], "n1");
    assert_eq!(
        toast(&app),
        Some((
            ToastKind::Success,
            "Committed a1b2c3d (in review)".to_string()
        ))
    );
}

// --- the commit was refused ---------------------------------------------------------------

#[test]
fn a_refused_commit_keeps_the_composer_open_with_its_draft_and_says_why() {
    let (mut app, req) = sent("hello");

    let effects = app.update(failed(
        req,
        failure_with("validation_failed", "The body field is required."),
    ));

    let compose = app.compose().expect("the composer stays open");
    assert!(!compose.submitting);
    assert_eq!(
        compose.error.as_deref(),
        Some("The body field is required.")
    );
    assert_eq!(draft(&app), Some("hello".to_string()));
    assert_eq!(
        toast(&app),
        Some((ToastKind::Error, "The body field is required.".to_string()))
    );
    assert!(effects.is_empty(), "{effects:?}");
}

#[test]
fn after_a_refusal_the_same_text_can_be_sent_again() {
    let (mut app, req) = sent("hello");
    app.update(failed(req, failure_with("validation_failed", "Nope.")));

    let effects = submit(&mut app);

    assert_eq!(creates(&effects).len(), 1);
    assert!(
        app.compose()
            .is_some_and(|c| c.submitting && c.error.is_none())
    );
}

#[test]
fn typing_after_a_refusal_goes_on_where_it_left_off() {
    let (mut app, req) = sent("hello");
    app.update(failed(req, failure_with("validation_failed", "Nope.")));

    type_str(&mut app, "!");

    assert_eq!(draft(&app), Some("hello!".to_string()));
}

#[test]
fn a_rate_limit_says_when_to_try_again() {
    let (mut app, req) = sent("hello");
    let failure = Failure {
        retry_after: Some(30),
        ..failure_with("rate_limited", "Too Many Attempts.")
    };

    app.update(failed(req, failure));

    let text = "rate_limited: retry in 30s";
    assert_eq!(app.compose().and_then(|c| c.error.as_deref()), Some(text));
    assert_eq!(toast(&app), Some((ToastKind::Error, text.to_string())));
}

#[test]
fn a_network_failure_warns_that_the_commit_may_have_been_posted() {
    let (mut app, req) = sent("hello");

    app.update(failed(req, Failure::new("network")));

    let text = "network error — check your log before sending again (it may have been posted)";
    assert_eq!(app.compose().and_then(|c| c.error.as_deref()), Some(text));
    assert_eq!(toast(&app), Some((ToastKind::Error, text.to_string())));
    assert_eq!(draft(&app), Some("hello".to_string()));
}

#[test]
fn a_failure_with_no_message_shows_its_code() {
    let (mut app, req) = sent("hello");

    app.update(failed(req, Failure::new("http_500")));

    assert_eq!(
        app.compose().and_then(|c| c.error.as_deref()),
        Some("http_500")
    );
}

#[test]
fn the_server_message_is_cleaned_and_cut_before_it_is_shown() {
    let (mut app, req) = sent("hello");
    let message = format!("bad\u{7}\u{202e} {}", "x".repeat(100));

    app.update(failed(req, failure_with("validation_failed", &message)));

    let shown = app
        .compose()
        .and_then(|c| c.error.clone())
        .unwrap_or_default();
    assert!(shown.starts_with("bad x"), "{shown:?}");
    assert!(
        shown.chars().all(|c| !crate::text::is_unsafe_char(c)),
        "{shown:?}"
    );
    assert_eq!(shown.chars().count(), 64);
    assert_eq!(toast(&app).map(|t| t.1), Some(shown));
}

#[test]
fn an_answer_for_another_request_is_ignored_when_it_is_a_refusal() {
    let (mut app, req) = sent("hello");

    let effects = app.update(failed(
        ReqId(req.0.wrapping_add(100)),
        failure_with("validation_failed", "Nope."),
    ));

    let compose = app.compose().expect("still open");
    assert!(compose.submitting);
    assert_eq!(compose.error, None);
    assert_eq!(app.toast(), None);
    assert!(effects.is_empty());
}

#[test]
fn a_refusal_after_esc_is_toasted_and_frees_the_next_submit() {
    let (mut app, req) = sent("hello");
    press(&mut app, KeyCode::Esc);

    app.update(failed(req, failure_with("validation_failed", "Nope.")));

    assert_eq!(toast(&app), Some((ToastKind::Error, "Nope.".to_string())));
    type_str(&mut app, "c");
    assert_eq!(creates(&submit(&mut app)).len(), 1);
}

#[test]
fn a_refusal_after_the_composer_was_closed_and_reopened_shows_no_stale_error() {
    let (mut app, req) = sent("hello");
    press(&mut app, KeyCode::Esc);
    app.update(failed(req, failure_with("validation_failed", "Nope.")));

    type_str(&mut app, "c");

    assert_eq!(app.compose().and_then(|c| c.error.clone()), None);
}

// --- a rejected device -------------------------------------------------------------------

#[test]
fn a_device_that_is_no_longer_approved_takes_the_user_back_to_the_guest_view() {
    let (mut app, req) = sent("hello");

    app.update(failed(req, Failure::new(NOT_APPROVED)));

    assert_eq!(app.session(), &Session::Guest);
    assert!(app.compose().is_none());
    assert!(!app.composer_focused());
    assert_eq!(app.modal(), Some(Modal::SignIn));
    assert!(app.drafts().is_empty());
}

#[test]
fn a_rejection_found_by_another_request_closes_the_composer_too() {
    let mut app = composing("hello");

    app.update(Action::FeedFailed {
        key: FOLLOWING,
        req: ReqId(1),
        failure: Failure::new(NOT_APPROVED),
    });

    assert_eq!(app.session(), &Session::Guest);
    assert!(app.compose().is_none());
    assert_eq!(app.modal(), Some(Modal::SignIn));
}

#[test]
fn going_back_to_the_guest_view_saves_the_drafts_first() {
    let mut app = account_app();
    type_str(&mut app, "c");
    write(&mut app, "hello");
    let req = create_req(&submit(&mut app));

    let effects = app.update(failed(req, Failure::new(NOT_APPROVED)));

    let saved = saves(&effects);
    assert_eq!(saved.len(), 1, "{effects:?}");
    assert_eq!(saved[0].0, ACCOUNT);
    assert_eq!(saved[0].1.text(&DraftKey::New), Some("hello".to_string()));
    assert_eq!(
        effects.first(),
        Some(&Effect::SaveDrafts {
            account: saved[0].0.clone(),
            drafts: saved[0].1.clone(),
        })
    );
    assert!(effects.contains(&Effect::UseGuestClient));
    assert!(app.drafts().is_empty());
}

// --- the account signs in again ----------------------------------------------------------

fn pkce() -> Pkce {
    Pkce {
        verifier: "verifier".into(),
        challenge: "challenge".into(),
    }
}

fn state() -> String {
    format!("{:032x}", 1)
}

/// An account that sent a commit, was refused as a device, and signed in again; returns the request
/// the commit went out with, which is older than the new sign-in.
fn signed_in_again() -> (App, ReqId) {
    let mut app = App::with_key_status(
        Session::SignedIn(account_me()),
        NOW,
        UtcOffset::UTC,
        KeyStatus::Present,
        KEY_PATH.into(),
    );
    app.update(Action::Resize(120, 40));
    type_str(&mut app, "c");
    write(&mut app, "hello");
    let req = create_req(&submit(&mut app));

    assert!(app.compose().is_some_and(|c| c.submitting));
    let back = app.update(Action::FeedFailed {
        key: FOLLOWING,
        req: ReqId(1),
        failure: Failure::new(NOT_APPROVED),
    });
    assert!(
        back.contains(&Effect::PrepareSignIn { create: false }),
        "{back:?}"
    );
    app.update(Action::SignInPrepared {
        fingerprint: "SHA256:abc".into(),
        url: "https://dev-null.sh/mobile/authorize".into(),
        pkce: pkce(),
        state: state(),
        created: false,
    });
    type_str(&mut app, CODE);
    let register = press(&mut app, KeyCode::Enter);
    assert!(
        register
            .iter()
            .any(|e| matches!(e, Effect::RegisterDevice { .. })),
        "{register:?}"
    );
    let response = serde_json::from_value(me_json(0, json!({"id": ACCOUNT}))).unwrap();
    let signed = app.update(Action::DeviceRegistered {
        response: Box::new(response),
    });
    assert!(is_signed_in(&app));
    let fetch = signed
        .iter()
        .find_map(|e| match e {
            Effect::FetchLog {
                tab: LogTab::Following,
                req,
                ..
            } => Some(*req),
            _ => None,
        })
        .expect("the new session loads Following");
    app.update(Action::FeedLoaded {
        key: FOLLOWING,
        req: fetch,
        page: page(0, 3, None),
    });
    (app, req)
}

#[test]
fn a_commit_created_for_a_request_from_before_the_new_sign_in_is_announced_but_not_listed() {
    let (mut app, old) = signed_in_again();
    let drafts_before = app.drafts().clone();
    let compose_before = app.compose().cloned();

    app.update(created(old, new_commit("pending")));

    assert_eq!(ids(&app, FOLLOWING), ["c0", "c1", "c2"]);
    assert_eq!(app.drafts(), &drafts_before);
    assert_eq!(app.compose().cloned(), compose_before);
    assert_eq!(
        toast(&app),
        Some((
            ToastKind::Success,
            "Committed a1b2c3d (in review)".to_string()
        ))
    );
}

#[test]
fn a_rejection_of_a_request_from_before_the_new_sign_in_changes_nothing() {
    let (mut app, old) = signed_in_again();

    app.update(failed(old, Failure::new(NOT_APPROVED)));

    assert!(is_signed_in(&app));
    assert_eq!(ids(&app, FOLLOWING), ["c0", "c1", "c2"]);
}

#[test]
fn signing_in_loads_the_drafts_of_the_account() {
    let mut registering = App::with_key_status(
        Session::Guest,
        NOW,
        UtcOffset::UTC,
        KeyStatus::Present,
        KEY_PATH.into(),
    );
    registering.update(Action::Resize(120, 40));
    registering.update(Action::Init);
    registering.update(Action::SignInPrepared {
        fingerprint: "SHA256:abc".into(),
        url: "https://dev-null.sh/mobile/authorize".into(),
        pkce: pkce(),
        state: state(),
        created: false,
    });
    type_str(&mut registering, CODE);
    press(&mut registering, KeyCode::Enter);
    let response = serde_json::from_value(me_json(0, json!({"id": ACCOUNT}))).unwrap();

    let effects = registering.update(Action::DeviceRegistered {
        response: Box::new(response),
    });

    assert_eq!(loads(&effects), vec![ACCOUNT.to_string()]);
}

// --- drafts on disk: when to write them -------------------------------------------------------

#[test]
fn esc_saves_the_drafts_on_the_spot() {
    let mut app = account_app();
    type_str(&mut app, "c");
    write(&mut app, "hello");

    let effects = press(&mut app, KeyCode::Esc);

    let saved = saves(&effects);
    assert_eq!(saved.len(), 1, "{effects:?}");
    assert_eq!(saved[0].0, ACCOUNT);
    assert_eq!(saved[0].1.text(&DraftKey::New), Some("hello".to_string()));
}

#[test]
fn esc_without_a_change_saves_nothing() {
    let mut app = account_app();
    type_str(&mut app, "c");
    assert!(app.compose().is_some());

    let effects = press(&mut app, KeyCode::Esc);

    assert!(saves(&effects).is_empty(), "{effects:?}");
}

#[test]
fn a_second_esc_after_the_save_saves_nothing_again() {
    let mut app = account_app();
    type_str(&mut app, "c");
    write(&mut app, "hello");
    assert_eq!(saves(&press(&mut app, KeyCode::Esc)).len(), 1);
    type_str(&mut app, "c");

    let effects = press(&mut app, KeyCode::Esc);

    assert!(saves(&effects).is_empty(), "{effects:?}");
}

#[test]
fn typing_alone_writes_nothing_until_two_seconds_after_the_last_edit() {
    let mut app = account_app();
    type_str(&mut app, "c");
    let typed = type_str(&mut app, "a");
    assert!(saves(&typed).is_empty());

    assert!(saves(&app.update(Action::Tick(at(1)))).is_empty());
    let due = app.update(Action::Tick(at(2)));

    let saved = saves(&due);
    assert_eq!(saved.len(), 1, "{due:?}");
    assert_eq!(saved[0].0, ACCOUNT);
    assert_eq!(saved[0].1.text(&DraftKey::New), Some("a".to_string()));
}

#[test]
fn nothing_is_written_again_until_the_next_edit() {
    let mut app = account_app();
    type_str(&mut app, "c");
    type_str(&mut app, "a");
    assert_eq!(saves(&app.update(Action::Tick(at(2)))).len(), 1);

    for secs in [3, 10, 60] {
        assert!(
            saves(&app.update(Action::Tick(at(secs)))).is_empty(),
            "at {secs}s"
        );
    }
}

#[test]
fn each_edit_restarts_the_two_seconds() {
    let mut app = account_app();
    type_str(&mut app, "c");
    type_str(&mut app, "a");
    app.update(Action::Tick(at(1)));
    type_str(&mut app, "b");

    assert!(saves(&app.update(Action::Tick(at(2)))).is_empty());
    let due = app.update(Action::Tick(at(3)));

    let saved = saves(&due);
    assert_eq!(saved.len(), 1, "{due:?}");
    assert_eq!(saved[0].1.text(&DraftKey::New), Some("ab".to_string()));
}

#[test]
fn a_created_commit_saves_the_emptied_drafts_on_the_spot() {
    let mut app = account_app();
    type_str(&mut app, "c");
    write(&mut app, "hello");
    let req = create_req(&submit(&mut app));

    let effects = app.update(created(req, new_commit("pending")));

    let saved = saves(&effects);
    assert_eq!(saved.len(), 1, "{effects:?}");
    assert_eq!(saved[0].0, ACCOUNT);
    assert!(saved[0].1.is_empty());
}

#[test]
fn ctrl_c_saves_unsaved_drafts_before_quitting() {
    let mut app = account_app();
    type_str(&mut app, "c");
    write(&mut app, "hello");

    let effects = ctrl(&mut app, 'c');

    assert_eq!(effects.len(), 2, "{effects:?}");
    assert!(matches!(&effects[0], Effect::SaveDrafts { account, drafts }
        if account == ACCOUNT && drafts.text(&DraftKey::New) == Some("hello".to_string())));
    assert_eq!(effects[1], Effect::Quit);
}

#[test]
fn quitting_with_nothing_unsaved_is_just_a_quit() {
    let mut app = account_app();

    assert_eq!(app.update(Action::Quit), vec![Effect::Quit]);

    type_str(&mut app, "c");
    write(&mut app, "hello");
    assert_eq!(saves(&press(&mut app, KeyCode::Esc)).len(), 1);
    assert_eq!(app.update(Action::Quit), vec![Effect::Quit]);
}

#[test]
fn an_account_id_that_cannot_name_a_file_is_never_saved_or_loaded() {
    let mut app = signed_in(120, 40);
    type_str(&mut app, "c");
    write(&mut app, "hello");

    let tick = app.update(Action::Tick(at(5)));
    let esc = press(&mut app, KeyCode::Esc);
    let quit = app.update(Action::Quit);

    assert!(saves(&tick).is_empty(), "{tick:?}");
    assert!(saves(&esc).is_empty(), "{esc:?}");
    assert_eq!(quit, vec![Effect::Quit]);
    assert_eq!(draft(&app), Some("hello".to_string()));
}

// --- drafts on disk: when to read them ------------------------------------------------------------

fn restoring_with(id: &str) -> (App, Vec<Effect>) {
    let mut app = App::new(Session::Restoring, NOW, UtcOffset::UTC);
    app.update(Action::Resize(120, 40));
    let init = app.update(Action::Init);
    let req = init
        .iter()
        .find_map(|e| match e {
            Effect::FetchMe { req } => Some(*req),
            _ => None,
        })
        .expect("Init asks who the user is");
    let response = serde_json::from_value(me_json(0, json!({"id": id}))).unwrap();
    let effects = app.update(Action::MeLoaded {
        req,
        response: Box::new(response),
    });
    (app, effects)
}

#[test]
fn a_restored_session_asks_for_the_drafts_of_its_account() {
    let (app, effects) = restoring_with(ACCOUNT);

    assert!(is_signed_in(&app));
    assert_eq!(loads(&effects), vec![ACCOUNT.to_string()]);
}

#[test]
fn a_restored_session_with_an_unusable_account_id_asks_for_nothing() {
    let (app, effects) = restoring_with("user-myuron");

    assert!(is_signed_in(&app));
    assert!(loads(&effects).is_empty(), "{effects:?}");
}

#[test]
fn a_later_poll_of_the_user_does_not_ask_again() {
    let (mut app, first) = restoring_with(ACCOUNT);
    assert!(!loads(&first).is_empty());
    let tick = app.update(Action::Tick(at(60)));
    let req = tick
        .iter()
        .find_map(|e| match e {
            Effect::FetchMe { req } => Some(*req),
            _ => None,
        })
        .expect("a poll is due");

    let effects = app.update(Action::MeLoaded {
        req,
        response: Box::new(me_resp(0)),
    });

    assert!(loads(&effects).is_empty());
}

fn drafts_of(entries: &[(DraftKey, &str)]) -> Drafts {
    let mut drafts = Drafts::default();
    for (key, text) in entries {
        drafts.set(key.clone(), Editor::from_text(text));
    }
    drafts
}

fn loaded(app: &mut App, account: &str, drafts: Drafts) -> Vec<Effect> {
    app.update(Action::DraftsLoaded {
        account: account.into(),
        drafts,
    })
}

#[test]
fn loaded_drafts_fill_the_empty_slots() {
    let mut app = account_app();

    let effects = loaded(
        &mut app,
        ACCOUNT,
        drafts_of(&[
            (DraftKey::New, "from disk"),
            (DraftKey::Reply("c1".into()), "re"),
        ]),
    );

    assert_eq!(draft(&app), Some("from disk".to_string()));
    assert_eq!(
        app.drafts().text(&DraftKey::Reply("c1".into())),
        Some("re".to_string())
    );
    assert!(effects.is_empty(), "{effects:?}");
}

#[test]
fn a_loaded_draft_never_replaces_text_already_there() {
    let mut app = account_app();
    type_str(&mut app, "c");
    write(&mut app, "typed meanwhile");
    press(&mut app, KeyCode::Esc);

    loaded(
        &mut app,
        ACCOUNT,
        drafts_of(&[
            (DraftKey::New, "from disk"),
            (DraftKey::Reply("c1".into()), "re"),
        ]),
    );

    assert_eq!(draft(&app), Some("typed meanwhile".to_string()));
    assert_eq!(
        app.drafts().text(&DraftKey::Reply("c1".into())),
        Some("re".to_string())
    );
}

#[test]
fn a_loaded_draft_is_not_written_straight_back() {
    let mut app = account_app();
    loaded(
        &mut app,
        ACCOUNT,
        drafts_of(&[(DraftKey::New, "from disk")]),
    );
    assert_eq!(draft(&app), Some("from disk".to_string()));

    for secs in [2, 10] {
        assert!(
            saves(&app.update(Action::Tick(at(secs)))).is_empty(),
            "at {secs}s"
        );
    }
}

#[test]
fn drafts_of_another_account_are_ignored() {
    let mut app = account_app();

    loaded(
        &mut app,
        "0190ffff-0000-7000-8000-000000000001",
        drafts_of(&[(DraftKey::New, "someone else")]),
    );

    assert!(app.drafts().is_empty());
}

#[test]
fn drafts_that_arrive_for_a_guest_are_ignored() {
    let mut app = guest_without_dialog();

    loaded(&mut app, ACCOUNT, drafts_of(&[(DraftKey::New, "late")]));

    assert!(app.drafts().is_empty());
}

#[test]
fn loaded_reply_drafts_stay_within_the_cap_and_the_new_draft_is_kept() {
    let mut app = account_app();
    let mut entries: Vec<(DraftKey, String)> = (0..5)
        .map(|n| (DraftKey::Reply(format!("old{n}")), "x".to_string()))
        .collect();
    let seed = drafts_of(
        &entries
            .iter()
            .map(|(k, t)| (k.clone(), t.as_str()))
            .collect::<Vec<_>>(),
    );
    loaded(&mut app, ACCOUNT, seed);
    entries = (0..50)
        .map(|n| (DraftKey::Reply(format!("r{n}")), "y".to_string()))
        .collect();
    entries.push((DraftKey::New, "mine".to_string()));

    loaded(
        &mut app,
        ACCOUNT,
        drafts_of(
            &entries
                .iter()
                .map(|(k, t)| (k.clone(), t.as_str()))
                .collect::<Vec<_>>(),
        ),
    );

    assert_eq!(draft(&app), Some("mine".to_string()));
    assert_eq!(app.drafts().len(), 51);
}

// --- the editor's view of long text -------------------------------------------------------------

#[test]
fn the_view_follows_the_cursor_down_past_ten_rows() {
    let lines: Vec<String> = (1..=15).map(|n| format!("line {n}")).collect();
    let app = composing(&lines.join("\n"));

    assert_eq!(app.compose().map(|c| c.view_top), Some(5));
}

#[test]
fn the_view_follows_the_cursor_back_up() {
    let lines: Vec<String> = (1..=15).map(|n| format!("line {n}")).collect();
    let mut app = composing(&lines.join("\n"));

    for _ in 0..10 {
        press(&mut app, KeyCode::Up);
    }

    assert_eq!(app.compose().map(|c| c.view_top), Some(4));
}

#[test]
fn short_text_is_not_scrolled() {
    let app = composing("one\ntwo\nthree");

    assert_eq!(app.compose().map(|c| c.view_top), Some(0));
}

// --- the composer and the width of a terminal -----------------------------------------------------

#[test]
fn resizing_keeps_the_composer_and_its_text() {
    let mut app = composing("hello");

    app.update(Action::Resize(80, 30));

    assert!(app.compose().is_some());
    assert_eq!(draft(&app), Some("hello".to_string()));
}

// --- an answer that comes after the user became a guest ------------------------------------------------

#[test]
fn an_answer_that_comes_while_the_user_is_a_guest_is_announced_and_changes_nothing_else() {
    let (mut app, req) = sent("hello");
    app.update(failed(req, Failure::new(NOT_APPROVED)));
    assert_eq!(app.session(), &Session::Guest);
    let drafts_before = app.drafts().clone();
    let compose_before = app.compose().cloned();

    let effects = app.update(created(req, new_commit("pending")));

    assert_eq!(
        toast(&app),
        Some((
            ToastKind::Success,
            "Committed a1b2c3d (in review)".to_string()
        ))
    );
    assert!(ids(&app, LATEST).is_empty());
    assert!(ids(&app, FOLLOWING).is_empty());
    assert_eq!(app.drafts(), &drafts_before);
    assert_eq!(app.compose().cloned(), compose_before);
    assert_eq!(app.in_flight(), None);
    assert!(effects.is_empty(), "{effects:?}");
}

// --- closing a dialog over a thread ------------------------------------------------------------------------

#[test]
fn esc_in_the_dialog_over_a_thread_closes_the_composer_and_stays_on_the_thread() {
    let mut app = signed_in(120, 40);
    app.update(Action::Open(Screen::Thread("c1".into())));
    type_str(&mut app, "c");
    assert_eq!(app.modal(), Some(Modal::Composer));

    press(&mut app, KeyCode::Esc);

    assert_eq!(app.screen(), Screen::Thread("c1".into()));
    assert_eq!(app.modal(), None);
    assert!(app.compose().is_none());
}

// --- a write that failed ----------------------------------------------------------------------------

#[test]
fn drafts_whose_write_failed_are_written_again_when_quitting() {
    let mut app = account_app();
    type_str(&mut app, "c");
    type_str(&mut app, "a");
    let due = app.update(Action::Tick(at(2)));
    assert_eq!(saves(&due).len(), 1, "{due:?}");

    app.update(Action::DraftsSaveFailed);
    let quit = app.update(Action::Quit);

    assert_eq!(quit.len(), 2, "{quit:?}");
    assert!(matches!(&quit[0], Effect::SaveDrafts { drafts, .. }
        if drafts.text(&DraftKey::New) == Some("a".to_string())));
    assert_eq!(quit[1], Effect::Quit);
}

#[test]
fn drafts_whose_write_failed_are_not_retried_on_a_timer() {
    let mut app = account_app();
    type_str(&mut app, "c");
    type_str(&mut app, "a");
    assert_eq!(saves(&app.update(Action::Tick(at(2)))).len(), 1);

    app.update(Action::DraftsSaveFailed);

    for secs in [4, 10, 60] {
        assert!(
            saves(&app.update(Action::Tick(at(secs)))).is_empty(),
            "at {secs}s"
        );
    }
    type_str(&mut app, "b");
    assert!(saves(&app.update(Action::Tick(at(61)))).is_empty());
    assert_eq!(saves(&app.update(Action::Tick(at(62)))).len(), 1);
}

#[test]
fn drafts_whose_write_failed_are_written_again_when_the_composer_is_closed() {
    let mut app = account_app();
    type_str(&mut app, "c");
    type_str(&mut app, "a");
    assert_eq!(saves(&app.update(Action::Tick(at(2)))).len(), 1);
    app.update(Action::DraftsSaveFailed);

    let effects = press(&mut app, KeyCode::Esc);

    assert_eq!(saves(&effects).len(), 1, "{effects:?}");
}
