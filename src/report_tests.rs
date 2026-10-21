//! Reporting the commit of someone else: the dialog, its input, and what the answer of the server changes.
//!
//! - `!` opens the dialog on the selected card of Log, Saved, a profile list or a thread when the commit is not
//!   yours. A tombstone, an empty list and a session being restored do nothing, and a guest is asked to sign in.
//! - The reasons are spam, harassment, unsafe and other, spam first. `j`, `k` and the arrows choose one; Tab moves
//!   to the details and back. In the details every key but Esc, Tab, Enter and Ctrl+C is text; the details hold
//!   500 code points, by typing and by pasting, and a paste turns line breaks and tabs into spaces.
//! - Enter sends the reason and the trimmed details, which are left out when empty. While the report is out the
//!   dialog takes no input except Esc and Ctrl+C.
//! - A report that was taken closes the dialog and shows a toast. A refusal keeps the dialog open with the reason
//!   inside it and no toast. When the dialog was closed with Esc meanwhile, the answer is shown as a toast.

use crossterm::event::{Event, KeyCode, KeyModifiers};
use serde_json::json;
use time::UtcOffset;

use crate::action::{Action, Effect, Failure};
use crate::app::{App, LogTab, Modal, ProfileTab, Screen, Session, ToastKind};
use crate::dialogs::{ReportFocus, ReportState};
use crate::feed::{FeedKey, ReqId};
use crate::feed_tests::{FOLLOWING, deliver, down};
use crate::keymap::{ReportKey, map_event, map_key};
use crate::model::ReportReason;
use crate::profile_tests::{deliver_commits, deliver_user, open_profile, user_resp};
use crate::test_support::*;
use crate::thread_tests::{MAIN, commit_json, deliver_replies, item_of, merge, open_thread, tomb};
use crate::write_tests::{
    card, card_with, feed_page, guest_profile, key, log_app_of, sign_in_again, toast,
};

const NOT_APPROVED: &str = "device_not_approved";

// --- helpers ------------------------------------------------------------------

/// A commit of someone else whose short hash is the one the docs show.
fn theirs(id: &str) -> crate::model::Commit {
    card_with(id, "alice", json!({"short_hash": "a1b2c3d"}))
}

fn log_of_theirs() -> App {
    log_app_of(fixture_me(), vec![theirs("c0"), card("c1", "alice")])
}

fn reports(effects: &[Effect]) -> Vec<(String, ReportReason, Option<String>, ReqId)> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::ReportCommit {
                id,
                reason,
                details,
                req,
            } => Some((id.clone(), *reason, details.clone(), *req)),
            _ => None,
        })
        .collect()
}

fn open(app: &mut App) {
    key(app, '!');
    assert_eq!(
        app.modal(),
        Some(Modal::Report),
        "! did not open the dialog"
    );
}

fn dialog(app: &App) -> ReportState {
    assert!(app.report().is_some(), "no report dialog is open");
    app.report().cloned().expect("a dialog is open")
}

fn details(app: &App) -> String {
    app.report().map(|r| r.details.text()).unwrap_or_default()
}

fn to_details(app: &mut App) {
    press(app, KeyCode::Tab);
    assert!(app.report_input_focused(), "Tab did not reach the details");
}

/// Sends the report and returns the request.
fn send(app: &mut App) -> ReqId {
    let effects = press(app, KeyCode::Enter);
    let sent = reports(&effects);
    assert_eq!(sent.len(), 1, "Enter did not send one report: {effects:?}");
    sent[0].3
}

fn done(id: &str, req: ReqId) -> Action {
    Action::ReportDone { id: id.into(), req }
}

fn failed(id: &str, req: ReqId, failure: Failure) -> Action {
    Action::ReportFailed {
        id: id.into(),
        req,
        failure,
    }
}

fn event_key(app: &App, code: KeyCode, modifiers: KeyModifiers) -> Option<Action> {
    map_key(
        app,
        key_event(code, modifiers, crossterm::event::KeyEventKind::Press),
    )
}

fn plain(app: &App, code: KeyCode) -> Option<Action> {
    event_key(app, code, KeyModifiers::NONE)
}

fn typed(app: &App, c: char) -> Option<Action> {
    let modifiers = if c.is_uppercase() {
        KeyModifiers::SHIFT
    } else {
        KeyModifiers::NONE
    };
    event_key(app, KeyCode::Char(c), modifiers)
}

fn paste(app: &mut App, text: &str) {
    if let Some(action) = map_event(app, &Event::Paste(text.into())) {
        app.update(action);
    }
}

fn refusal(code: &str, message: Option<&str>) -> Failure {
    Failure {
        code: code.into(),
        retry_after: None,
        message: message.map(String::from),
    }
}

// --- keymap ---------------------------------------------------------------------

#[test]
fn the_dialog_reads_the_reason_keys_with_the_reason_focused() {
    let mut app = log_of_theirs();
    open(&mut app);

    assert_eq!(typed(&app, 'j'), Some(Action::ReportKey(ReportKey::Down)));
    assert_eq!(
        plain(&app, KeyCode::Down),
        Some(Action::ReportKey(ReportKey::Down))
    );
    assert_eq!(typed(&app, 'k'), Some(Action::ReportKey(ReportKey::Up)));
    assert_eq!(
        plain(&app, KeyCode::Up),
        Some(Action::ReportKey(ReportKey::Up))
    );
    assert_eq!(
        plain(&app, KeyCode::Tab),
        Some(Action::ReportKey(ReportKey::Tab))
    );
    assert_eq!(
        plain(&app, KeyCode::Enter),
        Some(Action::ReportKey(ReportKey::Submit))
    );
    assert_eq!(plain(&app, KeyCode::Esc), Some(Action::CloseModal));
    assert_eq!(
        event_key(&app, KeyCode::Char('c'), KeyModifiers::CONTROL),
        Some(Action::Quit)
    );
    for c in ['Q', '?', 'y', 'd', 's', 'x', '1', '!', 'g'] {
        assert_eq!(typed(&app, c), None, "{c}");
    }
    assert_eq!(plain(&app, KeyCode::Backspace), None);
    assert_eq!(map_event(&app, &Event::Paste("text".into())), None);
}

#[test]
fn the_dialog_turns_every_key_into_text_with_the_details_focused_except_four() {
    let mut app = log_of_theirs();
    open(&mut app);
    to_details(&mut app);

    for c in ['Q', 'j', 'k', '?', 'y', 'd', 'x', '!', '1', ' ', 'あ'] {
        assert_eq!(
            typed(&app, c),
            Some(Action::ReportKey(ReportKey::Insert(c))),
            "{c}"
        );
    }
    let edits = [
        (KeyCode::Backspace, ReportKey::Backspace),
        (KeyCode::Delete, ReportKey::Delete),
        (KeyCode::Left, ReportKey::Left),
        (KeyCode::Right, ReportKey::Right),
        (KeyCode::Home, ReportKey::Home),
        (KeyCode::End, ReportKey::End),
        (KeyCode::Tab, ReportKey::Tab),
        (KeyCode::Enter, ReportKey::Submit),
    ];
    for (code, want) in edits {
        assert_eq!(plain(&app, code), Some(Action::ReportKey(want)), "{code:?}");
    }
    assert_eq!(plain(&app, KeyCode::Esc), Some(Action::CloseModal));
    assert_eq!(
        event_key(&app, KeyCode::Char('c'), KeyModifiers::CONTROL),
        Some(Action::Quit)
    );
    assert_eq!(
        event_key(&app, KeyCode::Char('u'), KeyModifiers::CONTROL),
        None,
        "a control key other than Ctrl+C is not text"
    );
    assert_eq!(
        map_event(&app, &Event::Paste("text".into())),
        Some(Action::Paste("text".into()))
    );
}

#[test]
fn the_details_focus_is_reported_for_the_dialog_alone() {
    let mut app = log_of_theirs();
    assert!(!app.report_input_focused());
    open(&mut app);
    assert!(!app.report_input_focused());
    press(&mut app, KeyCode::Tab);
    assert!(app.report_input_focused());
    press(&mut app, KeyCode::Tab);
    assert!(!app.report_input_focused());
    press(&mut app, KeyCode::Tab);
    press(&mut app, KeyCode::Esc);
    assert!(!app.report_input_focused());
}

// --- opening ---------------------------------------------------------------------------

#[test]
fn the_bang_opens_the_dialog_on_the_commit_of_someone_else_and_sends_nothing() {
    let mut saved = signed_in(120, 40);
    let effects = saved.update(Action::Goto(Screen::Saved));
    deliver(
        &mut saved,
        FeedKey::Saved,
        &effects,
        feed_page(vec![theirs("c0")]),
    );
    let mut profile = signed_in(120, 40);
    let effects = open_profile(&mut profile, "alice");
    deliver_user(
        &mut profile,
        &effects,
        "alice",
        user_resp("alice", json!({})),
    );
    deliver_commits(
        &mut profile,
        &effects,
        "alice",
        ProfileTab::Commits,
        vec![theirs("c0")],
        None,
    );
    let mut thread = signed_in(120, 40);
    let effects = open_thread(&mut thread, MAIN);
    crate::thread_tests::deliver_thread(&mut thread, &effects, vec![], 1);
    let reply = item_of(merge(
        commit_json("c0", "alice", 600),
        json!({"parent_id": MAIN, "short_hash": "a1b2c3d"}),
    ));
    deliver_replies(&mut thread, &effects, MAIN, vec![reply], None);
    down(&mut thread, 1);
    let scenes = [
        ("log", log_of_theirs()),
        ("saved", saved),
        ("profile", profile),
        ("thread", thread),
    ];
    for (name, mut app) in scenes {
        let effects = key(&mut app, '!');

        assert_eq!(effects, Vec::<Effect>::new(), "{name}");
        assert_eq!(app.modal(), Some(Modal::Report), "{name}");
        let state = dialog(&app);
        assert_eq!(state.id, "c0", "{name}");
        assert_eq!(state.short_hash, "a1b2c3d", "{name}");
        assert_eq!(state.reason, ReportReason::Spam, "{name}");
        assert_eq!(state.focus, ReportFocus::Reason, "{name}");
        assert_eq!(state.details.text(), "", "{name}");
        assert!(!state.sending, "{name}");
        assert_eq!(state.error, None, "{name}");
    }
}

#[test]
fn the_bang_does_nothing_on_your_own_commit_a_tombstone_or_an_empty_list() {
    let mut own = log_app_of(fixture_me(), vec![card("c0", "myuron")]);
    let mut empty = log_app_of(fixture_me(), vec![]);
    let mut tombstone = signed_in(120, 40);
    let effects = open_thread(&mut tombstone, MAIN);
    crate::thread_tests::deliver_thread(&mut tombstone, &effects, vec![], 1);
    deliver_replies(&mut tombstone, &effects, MAIN, vec![tomb("r0")], None);
    down(&mut tombstone, 1);
    for (name, app) in [
        ("own", &mut own),
        ("empty", &mut empty),
        ("tombstone", &mut tombstone),
    ] {
        assert_eq!(key(app, '!'), Vec::<Effect>::new(), "{name}");
        assert_eq!(app.modal(), None, "{name}");
        assert_eq!(app.report(), None, "{name}");
    }
}

#[test]
fn a_guest_is_asked_to_sign_in_and_a_session_being_restored_does_nothing() {
    let mut guest = guest_profile(vec![theirs("c0")]);
    assert_eq!(guest.update(Action::Report), Vec::<Effect>::new());
    assert_eq!(guest.modal(), Some(Modal::SignIn));
    assert_eq!(guest.report(), None);

    let mut restoring = App::new(Session::Restoring, NOW, UtcOffset::UTC);
    restoring.update(Action::Resize(120, 40));
    restoring.update(Action::Init);
    assert_eq!(restoring.update(Action::Report), Vec::<Effect>::new());
    assert_eq!(restoring.modal(), None);
}

// --- choosing and typing -------------------------------------------------------------------

#[test]
fn the_reasons_follow_one_another_and_stop_at_both_ends() {
    let mut app = log_of_theirs();
    open(&mut app);
    let mut seen = vec![dialog(&app).reason];
    for _ in 0..4 {
        key(&mut app, 'j');
        seen.push(dialog(&app).reason);
    }
    assert_eq!(
        seen,
        [
            ReportReason::Spam,
            ReportReason::Harassment,
            ReportReason::Unsafe,
            ReportReason::Other,
            ReportReason::Other,
        ]
    );

    key(&mut app, 'k');
    assert_eq!(dialog(&app).reason, ReportReason::Unsafe);
    press(&mut app, KeyCode::Up);
    press(&mut app, KeyCode::Up);
    press(&mut app, KeyCode::Up);
    assert_eq!(dialog(&app).reason, ReportReason::Spam);
    press(&mut app, KeyCode::Down);
    assert_eq!(dialog(&app).reason, ReportReason::Harassment);
}

#[test]
fn tab_moves_between_the_reasons_and_the_details_and_keeps_the_reason() {
    let mut app = log_of_theirs();
    open(&mut app);
    key(&mut app, 'j');

    press(&mut app, KeyCode::Tab);
    assert_eq!(dialog(&app).focus, ReportFocus::Details);
    assert_eq!(dialog(&app).reason, ReportReason::Harassment);
    press(&mut app, KeyCode::Tab);
    assert_eq!(dialog(&app).focus, ReportFocus::Reason);
    assert_eq!(dialog(&app).reason, ReportReason::Harassment);
}

#[test]
fn keys_that_are_not_bound_do_nothing_with_the_reason_focused() {
    let mut app = log_of_theirs();
    open(&mut app);

    let mut effects = type_str(&mut app, "Q?ydxsgc1");
    effects.extend(press(&mut app, KeyCode::Backspace));

    assert_eq!(effects, Vec::<Effect>::new());
    assert_eq!(app.modal(), Some(Modal::Report));
    assert_eq!(details(&app), "");
    assert_eq!(dialog(&app).reason, ReportReason::Spam);
}

#[test]
fn everything_but_four_keys_is_typed_into_the_details() {
    let mut app = log_of_theirs();
    open(&mut app);
    to_details(&mut app);

    let effects = type_str(&mut app, "Qjk?yd it's 100% spam");

    assert_eq!(effects, Vec::<Effect>::new());
    assert_eq!(details(&app), "Qjk?yd it's 100% spam");
    assert_eq!(app.modal(), Some(Modal::Report));
    assert_eq!(dialog(&app).reason, ReportReason::Spam);
}

#[test]
fn the_details_are_edited_at_the_cursor() {
    let mut app = log_of_theirs();
    open(&mut app);
    to_details(&mut app);
    type_str(&mut app, "abc");

    press(&mut app, KeyCode::Left);
    press(&mut app, KeyCode::Left);
    key(&mut app, 'X');
    assert_eq!(details(&app), "aXbc");
    press(&mut app, KeyCode::Backspace);
    assert_eq!(details(&app), "abc");
    press(&mut app, KeyCode::Delete);
    assert_eq!(details(&app), "ac");
    press(&mut app, KeyCode::Home);
    key(&mut app, 'H');
    press(&mut app, KeyCode::End);
    key(&mut app, 'E');
    assert_eq!(details(&app), "HacE");
    press(&mut app, KeyCode::Right);
    assert_eq!(details(&app), "HacE");
}

#[test]
fn the_details_hold_500_code_points_when_typed() {
    let mut app = log_of_theirs();
    open(&mut app);
    to_details(&mut app);

    type_str(&mut app, &"a".repeat(600));
    assert_eq!(details(&app).chars().count(), 500);

    press(&mut app, KeyCode::Backspace);
    key(&mut app, 'b');
    key(&mut app, 'c');
    assert_eq!(details(&app).chars().count(), 500);
    assert!(details(&app).ends_with('b'));
}

#[test]
fn the_cap_counts_code_points_and_not_bytes_or_graphemes() {
    let mut app = log_of_theirs();
    open(&mut app);
    to_details(&mut app);
    type_str(&mut app, &"あ".repeat(501));
    assert_eq!(details(&app).chars().count(), 500);

    let mut app = log_of_theirs();
    open(&mut app);
    to_details(&mut app);
    type_str(&mut app, &"😀".repeat(501));
    assert_eq!(details(&app).chars().count(), 500);

    let mut app = log_of_theirs();
    open(&mut app);
    to_details(&mut app);
    type_str(&mut app, &"a".repeat(499));
    // One letter and the mark that joins it make two code points, though one grapheme.
    type_str(&mut app, "e\u{301}");
    assert_eq!(details(&app).chars().count(), 500);
    assert!(details(&app).ends_with('e'));
}

// --- pasting ------------------------------------------------------------------------------------

#[test]
fn a_paste_goes_to_the_details_with_breaks_and_tabs_turned_into_spaces() {
    let mut app = log_of_theirs();
    open(&mut app);
    to_details(&mut app);

    paste(&mut app, "line one\nline two\tend");

    assert_eq!(details(&app), "line one line two end");
    assert_eq!(app.modal(), Some(Modal::Report));
}

#[test]
fn a_paste_drops_control_characters() {
    let mut app = log_of_theirs();
    open(&mut app);
    to_details(&mut app);

    paste(&mut app, "a\u{7}b\u{1b}[31mc\u{0}d");

    assert_eq!(details(&app), "ab[31mcd");
}

#[test]
fn a_paste_stops_at_500_code_points_and_adds_what_still_fits() {
    let mut app = log_of_theirs();
    open(&mut app);
    to_details(&mut app);
    paste(&mut app, &"x".repeat(600));
    assert_eq!(details(&app).chars().count(), 500);

    let mut app = log_of_theirs();
    open(&mut app);
    to_details(&mut app);
    type_str(&mut app, &"y".repeat(495));
    paste(&mut app, "123456789");
    assert_eq!(details(&app), format!("{}12345", "y".repeat(495)));
}

#[test]
fn a_paste_with_the_reason_focused_goes_nowhere() {
    let mut app = log_of_theirs();
    open(&mut app);

    paste(&mut app, "text");
    app.update(Action::Paste("text".into()));

    assert_eq!(details(&app), "");
    assert_eq!(app.modal(), Some(Modal::Report));
}

// --- sending --------------------------------------------------------------------------------------

#[test]
fn enter_sends_the_reason_and_the_trimmed_details() {
    let mut app = log_of_theirs();
    open(&mut app);
    key(&mut app, 'j');
    key(&mut app, 'j');
    to_details(&mut app);
    type_str(&mut app, "  sells pills in the replies \u{3000}");

    let effects = press(&mut app, KeyCode::Enter);

    let sent = reports(&effects);
    assert_eq!(effects.len(), 1, "{effects:?}");
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].0, "c0");
    assert_eq!(sent[0].1, ReportReason::Unsafe);
    assert_eq!(sent[0].2.as_deref(), Some("sells pills in the replies"));
    assert!(dialog(&app).sending);
    assert_eq!(dialog(&app).error, None);
    assert_eq!(app.modal(), Some(Modal::Report));
}

#[test]
fn empty_details_and_blank_details_are_left_out() {
    for text in ["", "   ", "\u{3000} "] {
        let mut app = log_of_theirs();
        open(&mut app);
        to_details(&mut app);
        type_str(&mut app, text);

        let effects = press(&mut app, KeyCode::Enter);

        let sent = reports(&effects);
        assert_eq!(sent.len(), 1, "{text:?}");
        assert_eq!(sent[0].2, None, "{text:?}");
        assert_eq!(sent[0].1, ReportReason::Spam, "{text:?}");
    }
}

#[test]
fn enter_with_the_reason_focused_sends_too_and_every_reason_has_its_value() {
    let want = [
        ReportReason::Spam,
        ReportReason::Harassment,
        ReportReason::Unsafe,
        ReportReason::Other,
    ];
    for (n, reason) in want.into_iter().enumerate() {
        let mut app = log_of_theirs();
        open(&mut app);
        for _ in 0..n {
            key(&mut app, 'j');
        }

        let sent = reports(&press(&mut app, KeyCode::Enter));

        assert_eq!(sent.len(), 1, "{reason:?}");
        assert_eq!(sent[0].1, reason);
    }
}

#[test]
fn a_second_enter_while_the_report_is_out_sends_nothing() {
    let mut app = log_of_theirs();
    open(&mut app);
    send(&mut app);

    assert_eq!(press(&mut app, KeyCode::Enter), Vec::<Effect>::new());
    assert_eq!(
        app.update(Action::ReportKey(ReportKey::Submit)),
        Vec::<Effect>::new()
    );
}

#[test]
fn while_the_report_is_out_the_dialog_takes_no_input() {
    let mut app = log_of_theirs();
    open(&mut app);
    key(&mut app, 'j');
    to_details(&mut app);
    type_str(&mut app, "abc");
    send(&mut app);

    let mut effects = type_str(&mut app, "xyzj");
    for code in [
        KeyCode::Backspace,
        KeyCode::Left,
        KeyCode::Down,
        KeyCode::Tab,
        KeyCode::Home,
    ] {
        effects.extend(press(&mut app, code));
    }
    paste(&mut app, "pasted");

    assert_eq!(effects, Vec::<Effect>::new());
    let state = dialog(&app);
    assert_eq!(state.details.text(), "abc");
    assert_eq!(state.reason, ReportReason::Harassment);
    assert_eq!(state.focus, ReportFocus::Details);
    assert!(state.sending);
}

#[test]
fn esc_closes_the_dialog_when_nothing_is_out_and_sends_nothing() {
    let mut app = log_of_theirs();
    open(&mut app);
    to_details(&mut app);
    type_str(&mut app, "abc");

    let effects = press(&mut app, KeyCode::Esc);

    assert_eq!(effects, Vec::<Effect>::new());
    assert_eq!(app.modal(), None);
    assert_eq!(app.report(), None);
    open(&mut app);
    assert_eq!(details(&app), "", "a new dialog starts empty");
}

#[test]
fn ctrl_c_quits_from_the_dialog_in_both_focuses_and_while_sending() {
    let mut app = log_of_theirs();
    open(&mut app);
    assert!(quit_requested(&ctrl(&mut app, 'c')));
    to_details(&mut app);
    assert!(quit_requested(&ctrl(&mut app, 'c')));
    send(&mut app);
    assert!(quit_requested(&ctrl(&mut app, 'c')));
}

#[test]
fn q_typed_in_the_details_is_text_and_does_not_quit() {
    let mut app = log_of_theirs();
    open(&mut app);
    to_details(&mut app);

    let effects = type_str(&mut app, "Q");

    assert!(!quit_requested(&effects));
    assert_eq!(details(&app), "Q");
}

// --- the answer ----------------------------------------------------------------------------------------

#[test]
fn a_taken_report_closes_the_dialog_and_shows_the_toast() {
    let mut app = log_of_theirs();
    open(&mut app);
    let req = send(&mut app);

    app.update(done("c0", req));

    assert_eq!(app.modal(), None);
    assert_eq!(app.report(), None);
    assert_eq!(
        toast(&app),
        Some((ToastKind::Success, "Reported a1b2c3d".into()))
    );
}

#[test]
fn an_answer_for_another_request_or_another_commit_changes_nothing() {
    let mut app = log_of_theirs();
    open(&mut app);
    let req = send(&mut app);
    let stale = ReqId(req.0 + 100);

    app.update(done("c0", stale));
    app.update(failed("c0", stale, Failure::new("http_500")));
    app.update(done("c1", req));
    app.update(failed("c1", req, Failure::new("http_500")));

    assert!(dialog(&app).sending);
    assert_eq!(dialog(&app).error, None);
    assert_eq!(toast(&app), None);
}

#[test]
fn a_refusal_keeps_the_dialog_open_with_the_reason_inside_it_and_no_toast() {
    let cases = [
        (
            refusal("own_commit", Some("You cannot report your own commit.")),
            "You cannot report your own commit.",
        ),
        (refusal("own_commit", None), "own_commit"),
        (
            refusal("commit_unavailable", Some("Not found.")),
            "This commit isn't available.",
        ),
        (
            refusal(
                "validation_failed",
                Some("The details field must not be greater than 500 characters."),
            ),
            "The details field must not be greater than 500 characters.",
        ),
        (
            Failure {
                code: "rate_limited".into(),
                retry_after: Some(20),
                message: Some("Too many reports.".into()),
            },
            "rate_limited: retry in 20s",
        ),
        (
            Failure::new("network"),
            "network error — press Enter to retry",
        ),
    ];
    for (failure, text) in cases {
        let mut app = log_of_theirs();
        open(&mut app);
        key(&mut app, 'j');
        to_details(&mut app);
        type_str(&mut app, "kept");
        let req = send(&mut app);

        app.update(failed("c0", req, failure.clone()));

        assert_eq!(app.modal(), Some(Modal::Report), "{failure:?}");
        let state = dialog(&app);
        assert!(!state.sending, "{failure:?}");
        assert_eq!(state.error.as_deref(), Some(text), "{failure:?}");
        assert_eq!(state.details.text(), "kept", "{failure:?}");
        assert_eq!(state.reason, ReportReason::Harassment, "{failure:?}");
        assert_eq!(toast(&app), None, "{failure:?}");
    }
}

#[test]
fn after_a_refusal_the_report_can_be_sent_again_and_the_error_goes_while_it_is_out() {
    let mut app = log_of_theirs();
    open(&mut app);
    let first = send(&mut app);
    app.update(failed("c0", first, refusal("http_500", None)));
    assert!(dialog(&app).error.is_some());

    let second = send(&mut app);

    assert!(second.0 > first.0);
    assert_eq!(dialog(&app).error, None);
    assert!(dialog(&app).sending);
    app.update(done("c0", second));
    assert_eq!(app.report(), None);
}

#[test]
fn input_works_again_after_a_refusal() {
    let mut app = log_of_theirs();
    open(&mut app);
    let req = send(&mut app);
    app.update(failed("c0", req, refusal("http_500", None)));

    key(&mut app, 'j');
    to_details(&mut app);
    type_str(&mut app, "more");

    assert_eq!(dialog(&app).reason, ReportReason::Harassment);
    assert_eq!(details(&app), "more");
}

#[test]
fn an_answer_after_esc_is_shown_as_a_toast_only() {
    let mut app = log_of_theirs();
    open(&mut app);
    let req = send(&mut app);
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.report(), None);
    assert_eq!(app.modal(), None);

    app.update(done("c0", req));

    assert_eq!(app.modal(), None);
    assert_eq!(app.report(), None);
    assert_eq!(
        toast(&app),
        Some((ToastKind::Success, "Reported a1b2c3d".into()))
    );
}

#[test]
fn a_refusal_after_esc_is_shown_as_a_toast_only() {
    let mut app = log_of_theirs();
    open(&mut app);
    let req = send(&mut app);
    press(&mut app, KeyCode::Esc);

    app.update(failed(
        "c0",
        req,
        refusal("own_commit", Some("You cannot report your own commit.")),
    ));

    assert_eq!(app.modal(), None);
    assert_eq!(app.report(), None);
    assert_eq!(
        toast(&app),
        Some((
            ToastKind::Error,
            "You cannot report your own commit.".into()
        ))
    );
}

#[test]
fn an_answer_to_a_closed_dialog_leaves_the_dialog_that_opened_since() {
    let mut app = log_of_theirs();
    open(&mut app);
    let old = send(&mut app);
    press(&mut app, KeyCode::Esc);
    down(&mut app, 1);
    open(&mut app);
    key(&mut app, 'j');

    app.update(done("c0", old));
    app.update(failed("c0", old, refusal("http_500", None)));

    let state = dialog(&app);
    assert_eq!(state.id, "c1");
    assert_eq!(state.reason, ReportReason::Harassment);
    assert!(!state.sending);
    assert_eq!(state.error, None);
}

// --- the session ---------------------------------------------------------------------------------------

#[test]
fn a_rejected_device_in_the_answer_returns_to_guest_and_closes_the_dialog() {
    let mut app = log_of_theirs();
    open(&mut app);
    let req = send(&mut app);

    app.update(failed("c0", req, Failure::new(NOT_APPROVED)));

    assert_eq!(app.session(), &Session::Guest);
    assert_eq!(app.report(), None);
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
fn an_answer_from_before_the_new_sign_in_changes_nothing() {
    let mut app = log_of_theirs();
    open(&mut app);
    let old = send(&mut app);
    sign_in_again(&mut app);
    let shown = toast(&app);
    let before = app.feed(FOLLOWING).items.clone();

    app.update(done("c0", old));
    app.update(failed("c0", old, Failure::new("http_500")));
    app.update(failed("c0", old, Failure::new(NOT_APPROVED)));

    assert!(matches!(app.session(), Session::SignedIn(_)));
    assert_eq!(toast(&app), shown);
    assert_eq!(app.feed(FOLLOWING).items, before);
    assert_eq!(app.report(), None);
}

#[test]
fn an_answer_after_the_return_to_guest_changes_neither_the_toast_nor_the_session() {
    let mut app = log_of_theirs();
    open(&mut app);
    let req = send(&mut app);
    app.update(failed("c0", req, Failure::new(NOT_APPROVED)));
    let shown = toast(&app);

    app.update(done("c0", req));
    app.update(failed("c0", req, Failure::new("http_500")));

    assert_eq!(toast(&app), shown);
    assert_eq!(app.session(), &Session::Guest);
}

#[test]
fn after_signing_in_again_a_report_can_be_made_again() {
    let mut app = log_of_theirs();
    open(&mut app);
    let old = send(&mut app);
    sign_in_again(&mut app);
    assert_eq!(app.screen(), Screen::Log(LogTab::Following));

    open(&mut app);
    let req = send(&mut app);

    assert!(req.0 > old.0);
}

#[test]
fn a_dialog_closed_with_esc_while_sending_cannot_be_opened_again_until_it_is_answered() {
    let mut app = log_of_theirs();
    open(&mut app);
    let req = send(&mut app);
    press(&mut app, KeyCode::Esc);

    assert_eq!(key(&mut app, '!'), Vec::<Effect>::new());
    assert_eq!(app.modal(), None);
    assert_eq!(app.report(), None);

    app.update(done("c0", req));
    open(&mut app);
}

#[test]
fn shift_tab_moves_the_focus_like_tab() {
    let mut app = log_of_theirs();
    open(&mut app);
    for focused in [false, true] {
        assert_eq!(app.report_input_focused(), focused);
        assert_eq!(
            plain(&app, KeyCode::BackTab),
            Some(Action::ReportKey(ReportKey::Tab))
        );
        press(&mut app, KeyCode::BackTab);
    }
    assert!(!app.report_input_focused());
}
