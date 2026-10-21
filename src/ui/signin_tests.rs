use crossterm::event::KeyCode;
use time::UtcOffset;

use crate::action::{Action, Failure};
use crate::api::keys::KeyError;
use crate::api::pkce::Pkce;
use crate::app::{App, KeyStatus, Session};
use crate::test_support::*;

macro_rules! snapshot {
    ($name:literal, $app:expr) => {{
        let terminal = render(&$app);
        insta::assert_snapshot!($name, terminal.backend());
    }};
}

const KEY_PATH: &str = "/home/user/.config/lazydn/id_ed25519";
const FINGERPRINT: &str = "SHA256:KhVovNmj/HaIYLFS2Thqiu7ov5FlDlXpnntLqnfTL0I";
const URL: &str = "https://dev-null.sh/mobile/authorize?fingerprint=SHA256%3AKhVovNmj%2FHaIYLFS2Thqiu7ov5FlDlXpnntLqnfTL0I&device_name=lazydn+on+testhost&code_challenge=E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM&code_challenge_method=S256&state=0123456789abcdef0123456789abcdef&redirect_uri=devnull%3A%2F%2Flazydn%2Fcallback";
const CODE: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

// --- states of the dialog, over a guest Log ------------------------------------------------------

fn dialog(w: u16, h: u16, key: KeyStatus) -> App {
    let mut app = App::with_key_status(Session::Guest, NOW, UtcOffset::UTC, key, KEY_PATH.into());
    app.update(Action::Resize(w, h));
    app
}

fn no_key(w: u16, h: u16) -> App {
    dialog(w, h, KeyStatus::Absent)
}

fn preparing(w: u16, h: u16) -> App {
    let mut app = dialog(w, h, KeyStatus::Present);
    app.update(Action::Init);
    app
}

/// The key file is readable by others; the toast that says so has run out by the time of the snapshot.
fn unreadable(w: u16, h: u16) -> App {
    let mut app = dialog(
        w,
        h,
        KeyStatus::Unreadable {
            error: KeyError::InsecurePermissions,
            path: KEY_PATH.into(),
        },
    );
    app.update(Action::Tick(NOW + time::Duration::seconds(10)));
    app
}

fn prepared_action() -> Action {
    prepared_with(URL)
}

fn prepared_with(url: &str) -> Action {
    Action::SignInPrepared {
        fingerprint: FINGERPRINT.into(),
        url: url.into(),
        pkce: Pkce {
            verifier: "verifier".into(),
            challenge: "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM".into(),
        },
        state: "0123456789abcdef0123456789abcdef".into(),
        created: false,
    }
}

fn url_shown(w: u16, h: u16) -> App {
    let mut app = preparing(w, h);
    app.update(prepared_action());
    app
}

fn submitting(w: u16, h: u16) -> App {
    let mut app = url_shown(w, h);
    type_str(&mut app, CODE);
    press(&mut app, KeyCode::Enter);
    app
}

/// The code was refused and the user has the new approval URL in front of them.
fn failed(w: u16, h: u16) -> App {
    let mut app = submitting(w, h);
    app.update(Action::SignInFailed {
        failure: Failure::new("invalid_grant"),
    });
    app.update(prepared_action());
    app
}

// --- 120x40 (the no-key state at this size is the snapshot of s07 in ui/tests.rs) ---------------------------------------------------------------------------------------

#[test]
fn wide_preparing_shows_the_key_and_a_pending_url() {
    snapshot!("signin_preparing_120", preparing(120, 40));
}

#[test]
fn wide_shows_the_key_the_fingerprint_the_wrapped_url_and_an_empty_code_field() {
    snapshot!("signin_url_shown_120", url_shown(120, 40));
}

#[test]
fn wide_submitting_shows_the_code_and_that_the_sign_in_is_running() {
    snapshot!("signin_submitting_120", submitting(120, 40));
}

#[test]
fn wide_failed_shows_the_message_the_kept_code_and_the_new_url() {
    snapshot!("signin_failed_120", failed(120, 40));
}

#[test]
fn wide_unreadable_key_gives_the_reason_and_how_to_retry() {
    snapshot!("signin_unreadable_120", unreadable(120, 40));
}

// --- 60x30 ----------------------------------------------------------------------------------------

#[test]
fn narrow_without_a_key_offers_to_create_it() {
    snapshot!("signin_no_key_60", no_key(60, 30));
}

#[test]
fn narrow_preparing_shows_the_key_and_a_pending_url() {
    snapshot!("signin_preparing_60", preparing(60, 30));
}

#[test]
fn narrow_shows_the_key_the_fingerprint_the_wrapped_url_and_an_empty_code_field() {
    snapshot!("signin_url_shown_60", url_shown(60, 30));
}

#[test]
fn narrow_submitting_shows_the_tail_of_the_code_and_that_the_sign_in_is_running() {
    snapshot!("signin_submitting_60", submitting(60, 30));
}

#[test]
fn narrow_failed_wraps_the_message_and_shows_the_tail_of_the_kept_code() {
    snapshot!("signin_failed_60", failed(60, 30));
}

#[test]
fn narrow_unreadable_key_wraps_the_reason_and_how_to_retry() {
    snapshot!("signin_unreadable_60", unreadable(60, 30));
}

// --- long URLs and wide characters ------------------------------------------------------------------

/// The text of the dialog rows between the label column and the right border.
fn dialog_rows(app: &App) -> Vec<String> {
    let terminal = render(app);
    let buffer = terminal.backend().buffer().clone();
    (0..buffer.area.height)
        .map(|y| row_text(&buffer, y))
        .filter(|row| row.contains('║'))
        .map(|row| row.trim_end().to_string())
        .collect()
}

#[test]
fn a_long_url_is_shown_whole_when_the_screen_is_high_enough() {
    let long = format!("{URL}&padding={}", "p".repeat(150));
    let mut app = preparing(60, 40);
    app.update(prepared_with(&long));

    let rows = dialog_rows(&app);
    let start = rows
        .iter()
        .position(|r| r.contains("open this URL"))
        .expect("the heading");
    let end = rows
        .iter()
        .position(|r| r.contains("Ctrl+O"))
        .expect("the hint");
    let shown: String = rows[start + 1..end]
        .iter()
        .map(|r| r.trim_matches(|c| c == '║' || c == ' ').to_string())
        .collect();
    assert_eq!(shown, long);
}

#[test]
fn wide_characters_in_the_code_field_do_not_push_out_the_border() {
    let mut app = url_shown(60, 30);
    app.update(Action::Paste("漢".repeat(60)));

    let terminal = render(&app);
    let buffer = terminal.backend().buffer();
    let y = (0..buffer.area.height)
        .find(|y| row_text(buffer, *y).contains("3. Code"))
        .expect("the code row");
    assert_eq!(buffer[(59, y)].symbol(), "║");
}

#[test]
fn a_low_screen_cuts_the_url_and_keeps_the_frame_and_the_hints() {
    let long = format!("{URL}&padding={}", "p".repeat(150));
    let mut app = preparing(60, 20);
    app.update(prepared_with(&long));

    let terminal = render(&app);
    let buffer = terminal.backend().buffer().clone();
    let screen: Vec<String> = (0..buffer.area.height)
        .map(|y| row_text(&buffer, y))
        .collect();
    assert!(screen[0].starts_with('╔'), "{screen:?}");
    assert!(screen[19].starts_with('╚'), "{screen:?}");
    let hint = screen
        .iter()
        .position(|r| r.contains("Ctrl+O"))
        .expect("the hint");
    assert!(screen[hint - 1].trim_end_matches(['║', ' ']).ends_with('…'));
    assert!(screen.iter().any(|r| r.contains("Enter submit")));
    snapshot!("signin_url_truncated_60x20", app);
}
