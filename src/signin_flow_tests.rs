use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};
use time::{Duration, UtcOffset};

use crate::action::{Action, Effect, Failure};
use crate::api::keys::KeyError;
use crate::api::pkce::Pkce;
use crate::app::{
    App, KeyStatus, LogTab, Me, Modal, Screen, Session, SignInState, SignInStep, ToastKind,
};
use crate::feed::ReqId;
use crate::feed_tests::{FOLLOWING, first_req};
use crate::keymap::{map_event, map_key};
use crate::notifications_tests::{me_req, me_resp};
use crate::test_support::*;

const FINGERPRINT: &str = "SHA256:KhVovNmj/HaIYLFS2Thqiu7ov5FlDlXpnntLqnfTL0I";
const KEY_PATH: &str = "/home/user/.config/lazydn/id_ed25519";
const CODE: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const NOT_APPROVED: &str = "device_not_approved";

// --- helpers ----------------------------------------------------------------

fn state_of(n: u32) -> String {
    format!("{n:032x}")
}

fn url_of(n: u32) -> String {
    format!(
        "https://dev-null.sh/mobile/authorize?fingerprint=SHA256%3Aabc&state={}&n={n}",
        state_of(n)
    )
}

fn pkce_of(n: u32) -> Pkce {
    Pkce {
        verifier: format!("verifier-{n}"),
        challenge: format!("challenge-{n}"),
    }
}

/// What the executor answers to `PrepareSignIn`; `n` tells one attempt from another.
fn prepared(n: u32, created: bool) -> Action {
    Action::SignInPrepared {
        fingerprint: FINGERPRINT.into(),
        url: url_of(n),
        pkce: pkce_of(n),
        state: state_of(n),
        created,
    }
}

fn callback(code: &str, n: u32) -> String {
    format!(
        "devnull://lazydn/callback?code={code}&state={}",
        state_of(n)
    )
}

fn failure(code: &str) -> Failure {
    Failure::new(code)
}

fn app_with(session: Session, key: KeyStatus) -> App {
    let mut app = App::with_key_status(session, NOW, UtcOffset::UTC, key, KEY_PATH.into());
    app.update(Action::Resize(120, 40));
    app
}

/// A guest whose dialog is open and waiting for the answer to `PrepareSignIn`.
fn preparing() -> App {
    let mut app = app_with(Session::Guest, KeyStatus::Present);
    assert_eq!(
        app.update(Action::Init),
        vec![Effect::PrepareSignIn { create: false }]
    );
    app
}

/// A guest whose dialog shows the approval URL of attempt 1.
fn ready() -> App {
    let mut app = preparing();
    app.update(prepared(1, false));
    app
}

/// Like `ready`, with the code typed and sent.
fn submitted() -> App {
    let mut app = ready();
    type_str(&mut app, CODE);
    assert_eq!(press(&mut app, KeyCode::Enter).len(), 1);
    app
}

fn toast(app: &App) -> Option<(ToastKind, String)> {
    app.toast().map(|t| (t.kind, t.text.clone()))
}

fn screen_text(app: &App) -> String {
    let terminal = render(app);
    let buffer = terminal.backend().buffer().clone();
    (0..buffer.area.height)
        .map(|y| row_text(&buffer, y))
        .collect::<Vec<_>>()
        .join("\n")
}

fn register(verifier: &str) -> Effect {
    Effect::RegisterDevice {
        code: CODE.into(),
        verifier: verifier.into(),
    }
}

fn prepare_effects(effects: &[Effect]) -> Vec<&Effect> {
    effects
        .iter()
        .filter(|e| matches!(e, Effect::PrepareSignIn { .. }))
        .collect()
}

// --- 1. opening the dialog asks for the URL when a key exists -----------------------------------

#[test]
fn a_guest_with_a_key_prepares_the_url_at_start_without_creating_anything() {
    let mut app = app_with(Session::Guest, KeyStatus::Present);

    let effects = app.update(Action::Init);

    assert_eq!(effects, vec![Effect::PrepareSignIn { create: false }]);
    assert_eq!(app.modal(), Some(Modal::SignIn));
    assert!(app.sign_in().preparing);
}

#[test]
fn a_guest_without_a_key_asks_for_nothing_and_waits_for_enter() {
    let mut app = app_with(Session::Guest, KeyStatus::Absent);

    let effects = app.update(Action::Init);

    assert_eq!(effects, Vec::<Effect>::new());
    assert_eq!(app.modal(), Some(Modal::SignIn));
    assert_eq!(app.sign_in().step, SignInStep::Idle);
}

/// Opens the dialog by `route` on a fresh app and returns what the app asked for.
fn open_by(route: &str, session: Session, key: KeyStatus) -> (App, Vec<Effect>) {
    let restoring = matches!(session, Session::Restoring);
    let mut app = app_with(session, key);
    let effects = match route {
        "init" => app.update(Action::Init),
        "L" => {
            press(&mut app, KeyCode::Esc);
            type_str(&mut app, "L")
        }
        "OpenSignIn" => {
            press(&mut app, KeyCode::Esc);
            app.update(Action::OpenSignIn)
        }
        "c" => {
            press(&mut app, KeyCode::Esc);
            type_str(&mut app, "c")
        }
        "g n" => {
            press(&mut app, KeyCode::Esc);
            type_str(&mut app, "gn")
        }
        "following tab" => {
            press(&mut app, KeyCode::Esc);
            type_str(&mut app, "1")
        }
        "nav enter" => {
            press(&mut app, KeyCode::Esc);
            press(&mut app, KeyCode::Tab);
            type_str(&mut app, "jjj");
            press(&mut app, KeyCode::Enter)
        }
        "rejected request" => app.update(Action::FeedFailed {
            key: FOLLOWING,
            req: ReqId(1),
            failure: failure(NOT_APPROVED),
        }),
        "invalid signature while restoring" => {
            assert!(restoring);
            let init = app.update(Action::Init);
            app.update(Action::MeFailed {
                req: me_req(&init),
                failure: failure("invalid_signature"),
            })
        }
        other => panic!("unknown route {other}"),
    };
    (app, effects)
}

const ROUTES: [&str; 9] = [
    "init",
    "L",
    "OpenSignIn",
    "c",
    "g n",
    "following tab",
    "nav enter",
    "rejected request",
    "invalid signature while restoring",
];

fn session_for(route: &str) -> Session {
    match route {
        "rejected request" => Session::SignedIn(fixture_me()),
        "invalid signature while restoring" => Session::Restoring,
        _ => Session::Guest,
    }
}

#[test]
fn every_route_to_the_dialog_asks_to_prepare_when_a_key_exists() {
    for route in ROUTES {
        let (app, effects) = open_by(route, session_for(route), KeyStatus::Present);

        assert_eq!(app.modal(), Some(Modal::SignIn), "{route}");
        assert_eq!(
            prepare_effects(&effects),
            vec![&Effect::PrepareSignIn { create: false }],
            "{route}: {effects:?}"
        );
        assert!(app.sign_in().preparing, "{route}");
    }
}

#[test]
fn no_route_to_the_dialog_creates_or_reads_a_key_when_none_exists() {
    // A signed-in or restoring session has a key, so only the guest routes can meet a missing one.
    for route in ROUTES
        .into_iter()
        .filter(|r| session_for(r) == Session::Guest)
    {
        let (app, effects) = open_by(route, session_for(route), KeyStatus::Absent);

        assert_eq!(app.modal(), Some(Modal::SignIn), "{route}");
        assert_eq!(prepare_effects(&effects), Vec::<&Effect>::new(), "{route}");
        assert_eq!(app.sign_in().step, SignInStep::Idle, "{route}");
    }
}

#[test]
fn reopening_while_the_answer_is_pending_does_not_ask_again() {
    let mut app = preparing();
    press(&mut app, KeyCode::Esc);

    let effects = type_str(&mut app, "L");

    assert_eq!(effects, Vec::<Effect>::new());
    assert_eq!(app.modal(), Some(Modal::SignIn));
}

#[test]
fn closing_and_reopening_keeps_the_url_the_code_and_the_pkce() {
    let mut app = ready();
    type_str(&mut app, "abc");
    let before = app.sign_in().clone();
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.modal(), None);

    let effects = type_str(&mut app, "L");

    assert_eq!(effects, Vec::<Effect>::new());
    assert_eq!(app.modal(), Some(Modal::SignIn));
    assert_eq!(app.sign_in(), &before);
    assert_eq!(app.sign_in().code, "abc");
    assert_eq!(app.sign_in().pkce, Some(pkce_of(1)));
}

// --- 2. the answer to PrepareSignIn ------------------------------------------------------------

#[test]
fn a_prepared_url_is_stored_with_its_fingerprint_pkce_and_state() {
    let app = ready();

    let state = app.sign_in();
    assert_eq!(state.step, SignInStep::Ready);
    assert_eq!(state.fingerprint.as_deref(), Some(FINGERPRINT));
    assert_eq!(state.url, Some(url_of(1)));
    assert_eq!(state.pkce, Some(pkce_of(1)));
    assert_eq!(state.oauth_state, Some(state_of(1)));
    assert!(!state.created);
    assert_eq!(app.key_status(), &KeyStatus::Present);
}

#[test]
fn a_prepared_url_and_fingerprint_show_in_the_dialog() {
    let text = screen_text(&ready());

    assert!(text.contains(FINGERPRINT), "{text}");
    assert!(
        text.contains("https://dev-null.sh/mobile/authorize?fingerprint="),
        "{text}"
    );
}

#[test]
fn a_key_made_by_the_attempt_is_marked_as_created() {
    let mut app = app_with(Session::Guest, KeyStatus::Absent);
    press(&mut app, KeyCode::Enter);

    app.update(prepared(1, true));

    assert!(app.sign_in().created);
    assert_eq!(app.sign_in().step, SignInStep::Ready);
    assert_eq!(app.key_status(), &KeyStatus::Present);
}

#[test]
fn a_key_that_could_not_be_used_is_reported_with_its_path() {
    let mut app = preparing();

    app.update(Action::SignInKeyFailed {
        error: KeyError::InsecurePermissions,
    });

    assert_eq!(
        app.key_status(),
        &KeyStatus::Unreadable {
            error: KeyError::InsecurePermissions,
            path: KEY_PATH.into(),
        }
    );
    assert_eq!(app.sign_in().step, SignInStep::Idle);
    // The dialog wraps the advice, so the command and the path may sit on separate rows.
    let text = screen_text(&app);
    assert!(
        text.contains("chmod 600") && text.contains(KEY_PATH),
        "{text}"
    );
}

// --- 3. no key yet: Enter creates it ------------------------------------------------------------

#[test]
fn enter_asks_to_create_the_key_when_none_exists() {
    let mut app = app_with(Session::Guest, KeyStatus::Absent);

    let effects = press(&mut app, KeyCode::Enter);

    assert_eq!(effects, vec![Effect::PrepareSignIn { create: true }]);
    assert!(app.sign_in().preparing);
}

#[test]
fn enter_does_not_ask_twice_for_the_key_to_be_created() {
    let mut app = app_with(Session::Guest, KeyStatus::Absent);
    press(&mut app, KeyCode::Enter);

    assert_eq!(press(&mut app, KeyCode::Enter), Vec::<Effect>::new());
}

#[test]
fn typing_and_pasting_are_ignored_until_a_key_exists() {
    let mut app = app_with(Session::Guest, KeyStatus::Absent);
    type_str(&mut app, "abc");
    app.update(Action::Paste("def".into()));
    assert_eq!(app.sign_in().code, "");

    press(&mut app, KeyCode::Enter);
    type_str(&mut app, "abc");
    app.update(Action::Paste("def".into()));
    assert_eq!(app.sign_in().code, "");
}

// --- 4. waiting for the URL --------------------------------------------------------------------

#[test]
fn while_the_url_is_being_prepared_enter_and_input_are_ignored() {
    let mut app = preparing();

    assert_eq!(press(&mut app, KeyCode::Enter), Vec::<Effect>::new());
    type_str(&mut app, "abc");
    app.update(Action::Paste("def".into()));

    assert_eq!(app.sign_in().code, "");
    assert!(app.sign_in().preparing);
}

// --- 5. a key that cannot be used --------------------------------------------------------------

fn unreadable() -> App {
    app_with(
        Session::Guest,
        KeyStatus::Unreadable {
            error: KeyError::InsecurePermissions,
            path: KEY_PATH.into(),
        },
    )
}

#[test]
fn with_an_unreadable_key_the_dialog_gives_the_reason_and_ignores_typing() {
    let mut app = unreadable();
    let init = app.update(Action::Init);
    assert_eq!(init, Vec::<Effect>::new());

    type_str(&mut app, "abc");
    app.update(Action::Paste("def".into()));

    assert_eq!(app.sign_in().code, "");
    // The dialog wraps the advice, so the command and the path may sit on separate rows.
    let text = screen_text(&app);
    assert!(
        text.contains("chmod 600") && text.contains(KEY_PATH),
        "{text}"
    );
}

#[test]
fn enter_tries_an_unreadable_key_again() {
    let mut app = unreadable();

    let effects = press(&mut app, KeyCode::Enter);

    assert_eq!(effects, vec![Effect::PrepareSignIn { create: false }]);
    assert!(app.sign_in().preparing);
    assert_eq!(press(&mut app, KeyCode::Enter), Vec::<Effect>::new());
}

#[test]
fn a_key_that_could_not_be_created_shows_its_reason_and_enter_reads_it_again() {
    let mut app = app_with(Session::Guest, KeyStatus::Absent);
    press(&mut app, KeyCode::Enter);

    app.update(Action::SignInKeyFailed {
        error: KeyError::Io("permission denied".into()),
    });

    assert!(!app.sign_in().preparing);
    assert_eq!(
        app.key_status(),
        &KeyStatus::Unreadable {
            error: KeyError::Io("permission denied".into()),
            path: KEY_PATH.into(),
        }
    );
    assert!(screen_text(&app).contains("could not be read (permission denied)"));
    assert_eq!(
        press(&mut app, KeyCode::Enter),
        vec![Effect::PrepareSignIn { create: false }]
    );
}

#[test]
fn a_key_that_vanished_goes_back_to_offering_to_create_it() {
    let mut app = preparing();

    app.update(Action::SignInKeyMissing);

    assert_eq!(app.key_status(), &KeyStatus::Absent);
    assert!(!app.sign_in().preparing);
    assert_eq!(app.sign_in().step, SignInStep::Idle);
    assert!(screen_text(&app).contains("press Enter to create"));
    assert_eq!(
        press(&mut app, KeyCode::Enter),
        vec![Effect::PrepareSignIn { create: true }]
    );
}

#[test]
fn ctrl_r_reads_an_unreadable_key_again() {
    let mut app = unreadable();

    let effects = ctrl(&mut app, 'r');

    assert_eq!(effects, vec![Effect::PrepareSignIn { create: false }]);
    assert!(app.sign_in().preparing);
}

#[test]
fn a_repaired_key_turns_the_dialog_to_ready() {
    let mut app = unreadable();
    ctrl(&mut app, 'r');

    app.update(prepared(1, false));

    assert_eq!(app.key_status(), &KeyStatus::Present);
    assert_eq!(app.sign_in().step, SignInStep::Ready);
}

#[test]
fn a_key_that_is_still_unreadable_keeps_the_same_reason() {
    let mut app = unreadable();
    ctrl(&mut app, 'r');

    app.update(Action::SignInKeyFailed {
        error: KeyError::InsecurePermissions,
    });

    assert_eq!(app.sign_in().step, SignInStep::Idle);
    // The dialog wraps the advice, so the command and the path may sit on separate rows.
    let text = screen_text(&app);
    assert!(
        text.contains("chmod 600") && text.contains(KEY_PATH),
        "{text}"
    );
}

// --- 6. typing into the Code field ---------------------------------------------------------------

#[test]
fn characters_go_to_the_code_field_even_when_they_are_commands_elsewhere() {
    let mut app = ready();

    let effects = type_str(&mut app, "Q?gnLc");

    assert_eq!(effects, Vec::<Effect>::new());
    assert_eq!(app.sign_in().code, "Q?gnLc");
    assert_eq!(app.modal(), Some(Modal::SignIn));
    assert_eq!(app.screen(), Screen::Log(LogTab::Latest));
}

#[test]
fn backspace_removes_one_character_and_a_held_backspace_keeps_removing() {
    let mut app = ready();
    type_str(&mut app, "abcd");

    press(&mut app, KeyCode::Backspace);
    assert_eq!(app.sign_in().code, "abc");
    send(
        &mut app,
        key_event(KeyCode::Backspace, KeyModifiers::NONE, KeyEventKind::Repeat),
    );
    assert_eq!(app.sign_in().code, "ab");
}

#[test]
fn backspace_on_an_empty_field_changes_nothing() {
    let mut app = ready();

    press(&mut app, KeyCode::Backspace);

    assert_eq!(app.sign_in().code, "");
}

#[test]
fn a_held_key_repeats_its_character_and_a_release_adds_nothing() {
    let mut app = ready();

    for kind in [
        KeyEventKind::Press,
        KeyEventKind::Repeat,
        KeyEventKind::Release,
    ] {
        send(
            &mut app,
            key_event(KeyCode::Char('x'), KeyModifiers::NONE, kind),
        );
    }

    assert_eq!(app.sign_in().code, "xx");
}

#[test]
fn ctrl_u_empties_the_field() {
    let mut app = ready();
    type_str(&mut app, "abcd");

    ctrl(&mut app, 'u');

    assert_eq!(app.sign_in().code, "");
}

#[test]
fn other_control_keys_add_no_text() {
    let mut app = ready();

    ctrl(&mut app, 'a');
    ctrl(&mut app, 'w');

    assert_eq!(app.sign_in().code, "");
}

#[test]
fn ctrl_c_quits_and_esc_closes_the_dialog_at_every_step() {
    let steps: [fn() -> App; 3] = [
        || app_with(Session::Guest, KeyStatus::Absent),
        preparing,
        ready,
    ];
    for make in steps {
        let mut app = make();
        assert!(quit_requested(&ctrl(&mut app, 'c')));
        press(&mut app, KeyCode::Esc);
        assert_eq!(app.modal(), None);
    }
}

// --- 7. what the keymap sends in the dialog -----------------------------------------------------

#[test]
fn the_dialog_keys_map_to_their_actions() {
    let app = ready();
    let key = |code, modifiers| key_event(code, modifiers, KeyEventKind::Press);
    let none = KeyModifiers::NONE;
    let ctrl_mod = KeyModifiers::CONTROL;
    let cases = [
        (key(KeyCode::Char('o'), ctrl_mod), Action::SignInOpenBrowser),
        (key(KeyCode::Char('y'), ctrl_mod), Action::SignInCopyUrl),
        (key(KeyCode::Char('u'), ctrl_mod), Action::SignInClear),
        (key(KeyCode::Char('r'), ctrl_mod), Action::SignInRefresh),
        (key(KeyCode::Char('c'), ctrl_mod), Action::Quit),
        (key(KeyCode::Backspace, none), Action::SignInBackspace),
        (key(KeyCode::Enter, none), Action::SignInSubmit),
        (key(KeyCode::Esc, none), Action::CloseModal),
        (key(KeyCode::Char('x'), none), Action::SignInInput('x')),
        (
            key(KeyCode::Char('X'), KeyModifiers::SHIFT),
            Action::SignInInput('X'),
        ),
        (
            key_event(KeyCode::Backspace, none, KeyEventKind::Repeat),
            Action::SignInBackspace,
        ),
    ];
    for (event, action) in cases {
        assert_eq!(map_key(&app, event), Some(action.clone()), "{action:?}");
    }
}

#[test]
fn the_dialog_keys_do_nothing_when_the_dialog_is_closed() {
    let mut app = ready();
    press(&mut app, KeyCode::Esc);
    for c in ['o', 'y', 'u', 'r'] {
        let event = key_event(KeyCode::Char(c), KeyModifiers::CONTROL, KeyEventKind::Press);
        assert_eq!(map_key(&app, event), None, "ctrl+{c}");
    }
    assert_eq!(app.sign_in().code, "");
}

#[test]
fn a_paste_reaches_the_dialog_only() {
    let mut app = ready();
    let paste = Event::Paste("a\nb".into());
    assert_eq!(map_event(&app, &paste), Some(Action::Paste("a\nb".into())));

    press(&mut app, KeyCode::Esc);
    assert_eq!(map_event(&app, &paste), None);
}

#[test]
fn keys_and_resizes_pass_through_map_event() {
    let app = ready();
    let key = key_event(KeyCode::Esc, KeyModifiers::NONE, KeyEventKind::Press);
    assert_eq!(map_event(&app, &Event::Key(key)), Some(Action::CloseModal));
    assert_eq!(
        map_event(&app, &Event::Resize(80, 24)),
        Some(Action::Resize(80, 24))
    );
}

// --- 8. paste ----------------------------------------------------------------------------------

#[test]
fn a_paste_is_added_to_the_field_without_its_newlines() {
    let mut app = ready();
    type_str(&mut app, "ab");

    app.update(Action::Paste("cd\r\nef\n".into()));

    assert_eq!(app.sign_in().code, "abcdef");
}

#[test]
fn the_field_never_holds_more_than_2kb() {
    let mut app = ready();

    app.update(Action::Paste("a".repeat(1500)));
    app.update(Action::Paste("b".repeat(1500)));

    assert_eq!(app.sign_in().code.chars().count(), 2048);
    assert!(app.sign_in().code.starts_with('a'));
}

#[test]
fn a_paste_outside_the_dialog_is_ignored() {
    let mut app = ready();
    type_str(&mut app, "ab");
    press(&mut app, KeyCode::Esc);

    app.update(Action::Paste("cd".into()));

    assert_eq!(app.sign_in().code, "ab");
}

#[test]
fn a_paste_does_not_reach_a_signed_in_screen() {
    let mut app = signed_in(120, 40);
    let before = app.screen();

    let effects = app.update(Action::Paste("gn".into()));

    assert_eq!(effects, Vec::<Effect>::new());
    assert_eq!(app.screen(), before);
    assert_eq!(app.sign_in(), &SignInState::default());
}

// --- 9. open the browser, copy the URL -----------------------------------------------------------

#[test]
fn ctrl_o_opens_the_approval_url() {
    let mut app = ready();

    assert_eq!(ctrl(&mut app, 'o'), vec![Effect::OpenUrl(url_of(1))]);
}

#[test]
fn ctrl_o_does_nothing_without_a_url() {
    for mut app in [preparing(), app_with(Session::Guest, KeyStatus::Absent)] {
        assert_eq!(ctrl(&mut app, 'o'), Vec::<Effect>::new());
    }
}

#[test]
fn ctrl_y_copies_the_url_and_says_only_that_the_copy_was_sent() {
    let mut app = ready();

    let effects = ctrl(&mut app, 'y');

    assert_eq!(effects, vec![Effect::CopyToClipboard(url_of(1))]);
    assert_eq!(
        toast(&app),
        Some((ToastKind::Info, "Copy sent to the terminal (OSC 52)".into()))
    );
}

#[test]
fn ctrl_y_does_nothing_without_a_url() {
    let mut app = preparing();

    assert_eq!(ctrl(&mut app, 'y'), Vec::<Effect>::new());
    assert_eq!(toast(&app), None);
}

#[test]
fn a_notice_from_the_executor_becomes_a_toast() {
    let mut app = ready();

    app.update(Action::Notice {
        kind: ToastKind::Error,
        text: "could not open the browser".into(),
    });

    assert_eq!(
        toast(&app),
        Some((ToastKind::Error, "could not open the browser".into()))
    );
}

// --- 10. a new URL --------------------------------------------------------------------------------

#[test]
fn ctrl_r_discards_the_url_and_asks_for_a_new_one() {
    let mut app = ready();

    let effects = ctrl(&mut app, 'r');

    assert_eq!(effects, vec![Effect::PrepareSignIn { create: false }]);
    assert!(app.sign_in().preparing);
    assert_eq!(app.sign_in().url, None);
    assert_eq!(app.sign_in().pkce, None);
}

#[test]
fn the_new_url_replaces_the_pkce_and_the_state() {
    let mut app = ready();
    ctrl(&mut app, 'r');

    app.update(prepared(2, false));

    assert_eq!(app.sign_in().url, Some(url_of(2)));
    assert_eq!(app.sign_in().pkce, Some(pkce_of(2)));
    assert_eq!(app.sign_in().oauth_state, Some(state_of(2)));
    assert_eq!(app.sign_in().step, SignInStep::Ready);
}

#[test]
fn ctrl_r_does_nothing_while_waiting_or_when_no_key_exists() {
    for mut app in [preparing(), app_with(Session::Guest, KeyStatus::Absent)] {
        assert_eq!(ctrl(&mut app, 'r'), Vec::<Effect>::new());
    }
}

#[test]
fn ctrl_r_does_nothing_while_the_code_is_being_sent() {
    let mut app = submitted();

    assert_eq!(ctrl(&mut app, 'r'), Vec::<Effect>::new());
    assert_eq!(app.sign_in().step, SignInStep::Submitting);
}

// --- 11. Enter sends the code ---------------------------------------------------------------------

#[test]
fn enter_sends_the_typed_code_with_the_verifier() {
    let mut app = ready();
    type_str(&mut app, CODE);

    let effects = press(&mut app, KeyCode::Enter);

    assert_eq!(effects, vec![register("verifier-1")]);
    assert_eq!(app.sign_in().step, SignInStep::Submitting);
}

#[test]
fn enter_takes_the_code_out_of_a_pasted_callback_url() {
    let mut app = ready();
    app.update(Action::Paste(callback(CODE, 1)));

    let effects = press(&mut app, KeyCode::Enter);

    assert_eq!(effects, vec![register("verifier-1")]);
}

#[test]
fn enter_while_the_code_is_being_sent_sends_nothing_more() {
    let mut app = submitted();

    assert_eq!(press(&mut app, KeyCode::Enter), Vec::<Effect>::new());
    assert_eq!(app.sign_in().step, SignInStep::Submitting);
}

#[test]
fn enter_on_an_empty_field_does_nothing_and_says_nothing() {
    let mut app = ready();

    assert_eq!(press(&mut app, KeyCode::Enter), Vec::<Effect>::new());
    assert_eq!(app.sign_in().error, None);
    assert_eq!(app.sign_in().step, SignInStep::Ready);
}

#[test]
fn a_code_that_cannot_be_read_is_explained_and_not_sent() {
    let mut app = ready();
    type_str(&mut app, "hello");

    assert_eq!(press(&mut app, KeyCode::Enter), Vec::<Effect>::new());

    assert!(app.sign_in().error.is_some());
    assert_eq!(app.sign_in().step, SignInStep::Ready);
    assert_eq!(app.sign_in().code, "hello");
}

#[test]
fn a_callback_for_another_attempt_is_refused_with_the_state_message() {
    let mut app = ready();
    app.update(Action::Paste(callback(CODE, 2)));

    assert_eq!(press(&mut app, KeyCode::Enter), Vec::<Effect>::new());

    assert_eq!(
        app.sign_in().error.as_deref(),
        Some("state mismatch — approve again")
    );
}

#[test]
fn a_cancelled_approval_is_reported() {
    let mut app = ready();
    app.update(Action::Paste(format!(
        "devnull://lazydn/callback?error=access_denied&state={}",
        state_of(1)
    )));

    assert_eq!(press(&mut app, KeyCode::Enter), Vec::<Effect>::new());

    assert_eq!(
        app.sign_in().error.as_deref(),
        Some("approval was cancelled")
    );
}

#[test]
fn the_message_goes_away_when_the_user_edits_the_field() {
    let mut app = ready();
    type_str(&mut app, "hello");
    press(&mut app, KeyCode::Enter);
    assert!(app.sign_in().error.is_some());

    type_str(&mut app, "x");

    assert_eq!(app.sign_in().error, None);
}

// --- 12. the device is registered ---------------------------------------------------------------

/// A guest who looked at a profile and a thread, then signed in.
fn registered() -> (App, Vec<Effect>) {
    let mut app = ready();
    app.update(Action::Open(Screen::Profile("alice".into())));
    app.update(Action::Open(Screen::Thread("c1".into())));
    assert!(app.profile("alice").is_some() && app.thread("c1").is_some());
    type_str(&mut app, CODE);
    press(&mut app, KeyCode::Enter);
    let effects = app.update(Action::DeviceRegistered {
        response: Box::new(me_resp(12)),
    });
    (app, effects)
}

#[test]
fn a_registered_device_signs_in_with_the_account_from_the_response() {
    let (app, _) = registered();

    assert_eq!(app.session(), &Session::SignedIn(Me::from(&me_resp(12))));
    assert_eq!(app.modal(), None);
}

#[test]
fn a_registered_device_switches_to_the_signed_client_and_then_fetches_following() {
    let (app, effects) = registered();

    assert_eq!(effects.first(), Some(&Effect::UseSignedClient));
    assert_eq!(effects.len(), 2, "{effects:?}");
    assert!(matches!(
        effects[1],
        Effect::FetchLog {
            tab: LogTab::Following,
            cursor: None,
            ..
        }
    ));
    assert_eq!(app.screen(), Screen::Log(LogTab::Following));
}

#[test]
fn a_registered_device_discards_what_the_guest_loaded_and_the_history() {
    let (mut app, _) = registered();

    assert!(app.profile("alice").is_none());
    assert!(app.thread("c1").is_none());
    assert_eq!(app.update(Action::Back), Vec::<Effect>::new());
    assert_eq!(app.screen(), Screen::Log(LogTab::Following));
}

#[test]
fn a_registered_device_does_not_ask_for_the_current_user_again_at_once() {
    let (mut app, _) = registered();
    let me_fetches = |effects: &[Effect]| {
        effects
            .iter()
            .filter(|e| matches!(e, Effect::FetchMe { .. }))
            .count()
    };

    let soon = app.update(Action::Tick(NOW + Duration::seconds(10)));
    let later = app.update(Action::Tick(NOW + Duration::seconds(31)));

    assert_eq!(me_fetches(&soon), 0);
    assert_eq!(me_fetches(&later), 1);
}

#[test]
fn a_registered_device_starts_the_next_sign_in_from_scratch() {
    let (app, _) = registered();

    assert_eq!(app.sign_in(), &SignInState::default());
}

// --- 13. the server refuses -----------------------------------------------------------------------

const INVALID_GRANT_TEXT: &str = "the code was not accepted — approve again and paste the new URL";

#[test]
fn an_invalid_grant_asks_for_a_new_url_and_keeps_the_typed_code() {
    let mut app = submitted();

    let effects = app.update(Action::SignInFailed {
        failure: Failure::new("invalid_grant"),
    });

    assert_eq!(effects, vec![Effect::PrepareSignIn { create: false }]);
    assert_eq!(
        app.sign_in().step,
        SignInStep::Failed("invalid_grant".into())
    );
    assert_eq!(app.sign_in().error.as_deref(), Some(INVALID_GRANT_TEXT));
    assert_eq!(app.sign_in().code, CODE);
    assert_eq!(app.sign_in().pkce, None);
    assert_eq!(app.sign_in().url, None);
}

#[test]
fn after_an_invalid_grant_enter_waits_for_the_new_url_and_then_uses_its_verifier() {
    let mut app = submitted();
    app.update(Action::SignInFailed {
        failure: Failure::new("invalid_grant"),
    });
    assert_eq!(press(&mut app, KeyCode::Enter), Vec::<Effect>::new());

    app.update(prepared(2, false));

    assert_eq!(app.sign_in().url, Some(url_of(2)));
    assert_eq!(app.sign_in().error.as_deref(), Some(INVALID_GRANT_TEXT));
    assert_eq!(
        press(&mut app, KeyCode::Enter),
        vec![register("verifier-2")]
    );
}

#[test]
fn a_failure_other_than_invalid_grant_keeps_the_url_and_lets_enter_try_again() {
    let mut app = submitted();

    let effects = app.update(Action::SignInFailed {
        failure: Failure::new("network"),
    });

    assert_eq!(effects, Vec::<Effect>::new());
    assert_eq!(app.sign_in().step, SignInStep::Failed("network".into()));
    assert_eq!(
        app.sign_in().error.as_deref(),
        Some("network error — press Enter to try again")
    );
    assert_eq!(app.sign_in().code, CODE);
    assert_eq!(app.sign_in().url, Some(url_of(1)));
    assert_eq!(
        press(&mut app, KeyCode::Enter),
        vec![register("verifier-1")]
    );
}

#[test]
fn failures_without_a_dedicated_message_name_their_code() {
    for (code, text) in [
        ("http_500", "sign-in failed: http_500"),
        (
            "invalid_signature",
            "invalid_signature: check your system clock",
        ),
    ] {
        let mut app = submitted();

        app.update(Action::SignInFailed {
            failure: Failure::new(code),
        });

        assert_eq!(app.sign_in().error.as_deref(), Some(text), "{code}");
    }
}

#[test]
fn editing_the_field_clears_the_failure_message_but_not_the_step() {
    let mut app = submitted();
    app.update(Action::SignInFailed {
        failure: Failure::new("network"),
    });

    type_str(&mut app, "x");

    assert_eq!(app.sign_in().error, None);
    assert_eq!(app.sign_in().step, SignInStep::Failed("network".into()));
}

// --- 14. a late rejection from before the sign-in --------------------------------------------------

/// A signed-in app with a key, Following loaded and the current-user request answered.
fn signed_in_with_key() -> App {
    let mut app = App::with_key_status(
        Session::SignedIn(fixture_me()),
        NOW,
        UtcOffset::UTC,
        KeyStatus::Present,
        KEY_PATH.into(),
    );
    app.update(Action::Resize(120, 40));
    let init = app.update(Action::Init);
    crate::feed_tests::deliver(&mut app, FOLLOWING, &init, crate::toast_tests::log_page());
    app.update(Action::MeLoaded {
        req: me_req(&init),
        response: Box::new(me_resp(12)),
    });
    app
}

/// Rejects the app through a request of its own, then signs in again.
/// Returns the app and the effects of the `DeviceRegistered`.
fn rejected_and_signed_in_again(mut app: App) -> (App, Vec<Effect>) {
    let refresh = type_str(&mut app, "R");
    let back = app.update(Action::FeedFailed {
        key: FOLLOWING,
        req: first_req(&refresh),
        failure: failure(NOT_APPROVED),
    });
    assert_eq!(back.first(), Some(&Effect::UseGuestClient));
    app.update(prepared(1, false));
    type_str(&mut app, CODE);
    press(&mut app, KeyCode::Enter);
    let registered = app.update(Action::DeviceRegistered {
        response: Box::new(me_resp(3)),
    });
    assert!(matches!(app.session(), Session::SignedIn(_)));
    (app, registered)
}

fn assert_still_signed_in(app: &App, effects: &[Effect]) {
    assert_eq!(effects, Vec::<Effect>::new());
    assert!(matches!(app.session(), Session::SignedIn(_)));
    assert_eq!(app.modal(), None);
}

#[test]
fn a_late_rejection_of_a_request_sent_before_the_sign_in_is_ignored() {
    let mut app = signed_in_with_key();
    let old = first_req(&type_str(&mut app, "R"));
    let (mut app, _) = rejected_and_signed_in_again(app);

    let effects = app.update(Action::FeedFailed {
        key: FOLLOWING,
        req: old,
        failure: failure(NOT_APPROVED),
    });

    assert_still_signed_in(&app, &effects);
}

#[test]
fn a_late_rejection_of_a_current_user_request_from_before_the_sign_in_is_ignored() {
    let mut app = signed_in_with_key();
    let due = app.update(Action::Tick(NOW + Duration::seconds(30)));
    let old = me_req(&due);
    let (mut app, _) = rejected_and_signed_in_again(app);

    let effects = app.update(Action::MeFailed {
        req: old,
        failure: failure(NOT_APPROVED),
    });

    assert_still_signed_in(&app, &effects);
}

#[test]
fn a_late_rejection_of_a_thread_request_from_before_the_sign_in_is_ignored() {
    let mut app = signed_in_with_key();
    let opened = app.update(Action::Open(Screen::Thread("c1".into())));
    let old = first_thread_req(&opened);
    let (mut app, _) = rejected_and_signed_in_again(app);

    let effects = app.update(Action::ThreadFailed {
        id: "c1".into(),
        req: old,
        failure: failure(NOT_APPROVED),
    });

    assert_still_signed_in(&app, &effects);
}

fn first_thread_req(effects: &[Effect]) -> ReqId {
    effects
        .iter()
        .find_map(|e| match e {
            Effect::FetchThread { req, .. } => Some(*req),
            _ => None,
        })
        .expect("FetchThread")
}

#[test]
fn a_rejection_of_the_fetch_the_sign_in_itself_started_returns_to_a_guest() {
    let (mut app, registered) = rejected_and_signed_in_again(signed_in_with_key());

    let effects = app.update(Action::FeedFailed {
        key: FOLLOWING,
        req: first_req(&registered),
        failure: failure(NOT_APPROVED),
    });

    assert_eq!(effects.first(), Some(&Effect::UseGuestClient));
    assert_eq!(app.session(), &Session::Guest);
}

#[test]
fn a_rejection_of_a_request_sent_after_the_sign_in_returns_to_a_guest() {
    let (mut app, _) = rejected_and_signed_in_again(signed_in_with_key());
    let refresh = type_str(&mut app, "R");

    let effects = app.update(Action::FeedFailed {
        key: FOLLOWING,
        req: first_req(&refresh),
        failure: failure(NOT_APPROVED),
    });

    assert_eq!(effects.first(), Some(&Effect::UseGuestClient));
    assert_eq!(app.session(), &Session::Guest);
}

// --- 15. secrets stay out of Debug ------------------------------------------------------------

#[test]
fn the_verifier_does_not_appear_in_debug_output() {
    let effect = register("SECRET-VERIFIER");
    assert!(!format!("{effect:?}").contains("SECRET-VERIFIER"));
    assert!(!format!("{:?}", prepared(1, false)).contains("verifier-1"));
    assert!(!format!("{:?}", ready()).contains("verifier-1"));
}

// --- review additions ------------------------------------------------------------------------

#[test]
fn while_a_new_url_is_awaited_after_an_invalid_grant_nothing_is_asked_twice() {
    let mut app = submitted();
    app.update(Action::SignInFailed {
        failure: Failure::new("invalid_grant"),
    });
    assert!(app.sign_in().preparing);

    press(&mut app, KeyCode::Esc);
    assert_eq!(type_str(&mut app, "L"), Vec::<Effect>::new());
    assert_eq!(ctrl(&mut app, 'r'), Vec::<Effect>::new());
    assert_eq!(press(&mut app, KeyCode::Enter), Vec::<Effect>::new());
}

#[test]
fn a_server_code_in_the_failure_text_loses_control_characters_and_is_cut_at_64() {
    let mut app = submitted();
    let noisy = format!("bad\x1b[31mcode{}", "x".repeat(100));

    app.update(Action::SignInFailed {
        failure: Failure::new(noisy),
    });

    let expected: String = format!("bad[31mcode{}", "x".repeat(100))
        .chars()
        .take(64)
        .collect();
    assert_eq!(
        app.sign_in().error,
        Some(format!("sign-in failed: {expected}"))
    );
}

#[test]
fn typing_stops_at_2048_characters_like_a_paste() {
    let mut app = ready();

    type_str(&mut app, &"a".repeat(2100));

    assert_eq!(app.sign_in().code.chars().count(), 2048);
}

#[test]
fn a_registered_device_discards_the_guests_connection_lists() {
    let mut app = ready();
    let opened = app.update(Action::Open(Screen::Connections(
        "alice".into(),
        crate::app::ConnTab::Followers,
    )));
    crate::profile_tests::deliver_conns(
        &mut app,
        &opened,
        "alice",
        crate::app::ConnTab::Followers,
        crate::profile_tests::users(0, 3),
        None,
    );
    assert!(
        app.connections("alice", crate::app::ConnTab::Followers)
            .is_some()
    );
    type_str(&mut app, CODE);
    press(&mut app, KeyCode::Enter);

    app.update(Action::DeviceRegistered {
        response: Box::new(me_resp(12)),
    });

    assert!(
        app.connections("alice", crate::app::ConnTab::Followers)
            .is_none()
    );
}

#[test]
fn an_approval_url_that_could_not_be_built_ends_the_wait_and_can_be_tried_again() {
    for retry in [
        (|app: &mut App| press(app, KeyCode::Enter)) as fn(&mut App) -> Vec<Effect>,
        |app| ctrl(app, 'r'),
    ] {
        let mut app = preparing();

        let effects = app.update(Action::SignInPrepareFailed {
            failure: Failure::new("invalid_base_url"),
        });

        assert_eq!(effects, Vec::<Effect>::new());
        assert!(!app.sign_in().preparing);
        assert!(app.sign_in().error.is_some());
        assert!(screen_text(&app).contains("invalid_base_url"));
        assert_eq!(
            retry(&mut app),
            vec![Effect::PrepareSignIn { create: false }]
        );
    }
}

#[test]
fn a_key_that_could_not_be_generated_is_generated_again_by_enter() {
    let mut app = app_with(Session::Guest, KeyStatus::Absent);
    press(&mut app, KeyCode::Enter);
    app.update(Action::SignInKeyFailed {
        error: KeyError::Generate("no entropy".into()),
    });

    let effects = press(&mut app, KeyCode::Enter);

    assert_eq!(effects, vec![Effect::PrepareSignIn { create: true }]);
}

#[test]
fn a_signed_in_session_cannot_open_the_dialog() {
    let mut app = signed_in(120, 40);

    app.update(Action::OpenSignIn);

    assert_eq!(app.modal(), None);
}

#[test]
fn after_a_failed_preparation_the_dialog_says_it_is_not_ready_and_offers_a_retry() {
    let mut app = preparing();
    app.update(Action::SignInPrepareFailed {
        failure: Failure::new("invalid_base_url"),
    });

    let text = screen_text(&app);

    assert!(text.contains("(not ready)"), "{text}");
    assert!(text.contains("Enter retry"), "{text}");
    assert!(!text.contains("preparing…"), "{text}");
}

#[test]
fn a_new_url_clears_an_old_message_unless_a_submit_failed() {
    let mut app = preparing();
    app.update(Action::SignInPrepareFailed {
        failure: Failure::new("invalid_base_url"),
    });
    // Enter retries without clearing the message itself, so only the new URL can clear it.
    press(&mut app, KeyCode::Enter);
    assert!(app.sign_in().error.is_some());
    app.update(prepared(1, false));
    assert_eq!(app.sign_in().error, None);

    let mut failed = submitted();
    failed.update(Action::SignInFailed {
        failure: Failure::new("invalid_grant"),
    });
    failed.update(prepared(2, false));
    assert_eq!(failed.sign_in().error.as_deref(), Some(INVALID_GRANT_TEXT));
}
