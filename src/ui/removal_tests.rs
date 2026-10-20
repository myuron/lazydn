//! How the confirmation of a delete, the card that is being deleted, a deleted main commit and the report dialog
//! look.
//!
//! - The confirmation is a double-framed dialog titled `Revert commit` that asks `Delete a1b2c3d?`, says that
//!   replies stay as a tombstone, and offers `[y] Delete` and `[n] Cancel`.
//! - The card that is being deleted is replaced by a frame that reads `rm a1b2c3d → /dev/null`.
//! - A deleted main commit reads `[deleted] This commit was deleted.` with the replies still below it, and its
//!   header shows the hash alone, since there is no link to copy.
//! - The report dialog is titled `Report a1b2c3d`. It lists the four reasons with a filled dot and a mark on the
//!   chosen one while the reasons have the focus, a field for the details with the counter `N/500` under it, and
//!   `Esc cancel   Enter submit`. A refusal is written to the left of the counter as `✗ reason`.
//! - The cursor sits in the details field, after the text, while the details have the focus.

use crossterm::event::KeyCode;
use serde_json::json;
use time::UtcOffset;

use crate::action::{Action, Effect, Failure};
use crate::app::{App, Session};
use crate::delete_tests::{done, thread_by};
use crate::feed::ReqId;
use crate::feed_tests::{FOLLOWING, deliver};
use crate::model::Commit;
use crate::test_support::*;
use crate::thread_tests::{MAIN, commit_json, item_of, merge};
use crate::write_tests::{card, card_with, feed_page, key};

macro_rules! snapshot {
    ($name:literal, $app:expr) => {{
        let terminal = render(&$app);
        insta::assert_snapshot!($name, terminal.backend());
    }};
}

fn log_sized(w: u16, h: u16, items: Vec<Commit>) -> App {
    let mut app = App::new(Session::SignedIn(fixture_me()), NOW, UtcOffset::UTC);
    let mut effects = app.update(Action::Init);
    effects.extend(app.update(Action::Resize(w, h)));
    deliver(&mut app, FOLLOWING, &effects, feed_page(items));
    app
}

/// Your `a1b2c3d` first, then two commits of alice.
fn own_first(w: u16, h: u16) -> App {
    let first = card_with("c0", "myuron", json!({"short_hash": "a1b2c3d"}));
    log_sized(w, h, vec![first, card("c1", "alice"), card("c2", "alice")])
}

/// The commit `a1b2c3d` of alice first, then two more.
fn theirs_first(w: u16, h: u16) -> App {
    let first = card_with("c0", "alice", json!({"short_hash": "a1b2c3d"}));
    log_sized(w, h, vec![first, card("c1", "bob"), card("c2", "carol")])
}

/// `d` and `y`; a missing request is left to the snapshot to show.
fn delete(app: &mut App) -> ReqId {
    key(app, 'd');
    key(app, 'y')
        .iter()
        .find_map(|e| match e {
            Effect::DeleteCommit { req, .. } => Some(*req),
            _ => None,
        })
        .unwrap_or(ReqId(u64::MAX))
}

fn cursor_of(app: &App) -> (u16, u16) {
    let mut terminal = render(app);
    let position = terminal.get_cursor_position().unwrap_or_default();
    (position.x, position.y)
}

fn screen_rows(app: &App) -> Vec<String> {
    let terminal = render(app);
    let buffer = terminal.backend().buffer().clone();
    (0..buffer.area.height)
        .map(|y| row_text(&buffer, y))
        .collect()
}

fn deleted_main() -> App {
    let mut app = thread_by(
        "myuron",
        2,
        vec![],
        vec![
            item_of(merge(
                commit_json("r1", "bob", 600),
                json!({"parent_id": MAIN}),
            )),
            item_of(merge(
                commit_json("r2", "carol", 500),
                json!({"parent_id": MAIN}),
            )),
        ],
    );
    let req = delete(&mut app);
    app.update(done(MAIN, req));
    // The toast of the delete would cover the status line; the thread is what this view is about, so it goes first.
    app.update(Action::Tick(NOW + time::Duration::seconds(4)));
    app
}

// --- the confirmation and the card ---------------------------------------------------------

#[test]
fn dl1_delete_confirm() {
    let mut app = own_first(120, 40);
    key(&mut app, 'd');

    snapshot!("delete_confirm", app);
}

#[test]
fn dl2_deleting_card() {
    let mut app = own_first(120, 40);
    delete(&mut app);

    snapshot!("deleting_card", app);
}

#[test]
fn tb1_thread_main_deleted() {
    snapshot!("thread_main_deleted", deleted_main());
}

#[test]
fn the_card_is_back_when_the_delete_is_refused() {
    let mut app = own_first(120, 40);
    let req = delete(&mut app);
    assert!(
        screen_rows(&app)
            .iter()
            .any(|r| r.contains("rm a1b2c3d → /dev/null")),
        "the frame is not shown"
    );

    app.update(Action::DeleteFailed {
        id: "c0".into(),
        req,
        failure: Failure::new("http_500"),
    });

    let rows = screen_rows(&app);
    assert!(rows.iter().all(|r| !r.contains("rm a1b2c3d")), "{rows:#?}");
    assert!(rows.iter().any(|r| r.contains("body of c0")), "{rows:#?}");
}

#[test]
fn the_help_lists_the_report_key_after_the_revert_key() {
    let mut app = own_first(120, 40);
    app.update(Action::OpenHelp);
    let rows = screen_rows(&app);

    let revert = rows.iter().position(|r| r.contains("revert (own)"));
    let report = rows.iter().position(|r| r.contains("!   report"));

    assert!(revert.is_some() && report.is_some(), "{rows:#?}");
    assert_eq!(report, revert.map(|r| r + 1));
}

// --- the report dialog -------------------------------------------------------------------------

#[test]
fn rp1_report_modal() {
    let mut app = theirs_first(120, 40);
    key(&mut app, '!');

    snapshot!("report_modal", app);
}

fn typing_details() -> App {
    let mut app = theirs_first(60, 24);
    key(&mut app, '!');
    press(&mut app, KeyCode::Tab);
    type_str(&mut app, "this is a scam");
    app
}

#[test]
fn rp2_report_modal_details() {
    let app = typing_details();

    snapshot!("report_modal_details", app);
    assert_eq!(cursor_of(&app), (26, 14));
}

#[test]
fn rp3_report_modal_error() {
    let mut app = theirs_first(60, 24);
    key(&mut app, '!');
    let effects = press(&mut app, KeyCode::Enter);
    let req = effects
        .iter()
        .find_map(|e| match e {
            Effect::ReportCommit { req, .. } => Some(*req),
            _ => None,
        })
        .unwrap_or(ReqId(u64::MAX));
    app.update(Action::ReportFailed {
        id: "c0".into(),
        req,
        failure: Failure::new("commit_unavailable"),
    });

    snapshot!("report_modal_error", app);
}

#[test]
fn the_cursor_stays_after_the_text_and_follows_the_arrow_keys() {
    let mut app = typing_details();
    assert_eq!(cursor_of(&app), (26, 14));

    press(&mut app, KeyCode::Left);
    assert_eq!(cursor_of(&app), (25, 14));
    press(&mut app, KeyCode::Home);
    assert_eq!(cursor_of(&app), (12, 14));
}

#[test]
fn the_cursor_is_at_the_start_of_an_empty_field_once_the_details_have_the_focus() {
    let mut app = theirs_first(60, 24);
    key(&mut app, '!');
    press(&mut app, KeyCode::Tab);

    assert_eq!(cursor_of(&app), (12, 14));
}

#[test]
fn the_counter_counts_code_points_and_reaches_500() {
    let mut app = theirs_first(60, 24);
    key(&mut app, '!');
    press(&mut app, KeyCode::Tab);
    type_str(&mut app, "あいう");
    assert!(screen_rows(&app).iter().any(|r| r.contains(" 3/500 ")));

    type_str(&mut app, &"x".repeat(600));

    assert!(screen_rows(&app).iter().any(|r| r.contains(" 500/500 ")));
}

#[test]
fn the_dialog_is_gone_with_the_screen_once_it_is_closed() {
    let mut app = theirs_first(120, 40);
    let plain = screen_rows(&app);
    key(&mut app, '!');
    assert_ne!(screen_rows(&app), plain);

    press(&mut app, KeyCode::Esc);

    assert_eq!(screen_rows(&app), plain);
}

// --- long details, long errors and the sending footer ---------------------------------------------

fn typing_x(w: u16, h: u16, n: usize) -> App {
    let mut app = theirs_first(w, h);
    key(&mut app, '!');
    press(&mut app, KeyCode::Tab);
    type_str(&mut app, &"x".repeat(n));
    app
}

/// The row of the screen where the modal's top border is.
fn modal_top(app: &App) -> Option<usize> {
    screen_rows(app).iter().position(|r| r.contains("╔ Report"))
}

fn modal_height(app: &App) -> Option<usize> {
    let rows = screen_rows(app);
    let top = modal_top(app)?;
    let bottom = rows.iter().rposition(|r| r.contains('╚'))?;
    Some(bottom - top + 1)
}

#[test]
fn a_field_of_two_rows_makes_the_dialog_one_row_taller_and_puts_it_back_in_the_middle() {
    let app = typing_x(60, 24, 40);

    assert_eq!(modal_height(&app), Some(15));
    assert_eq!(modal_top(&app), Some(4));
    assert_eq!(cursor_of(&app), (16, 14));
}

#[test]
fn a_field_stops_growing_at_four_rows_and_follows_the_cursor_to_the_end() {
    let app = typing_x(60, 24, 500);

    assert_eq!(modal_height(&app), Some(17));
    assert_eq!(modal_top(&app), Some(3));
    assert_eq!(cursor_of(&app), (44, 15));
}

#[test]
fn home_scrolls_a_long_field_back_to_its_first_row() {
    let mut app = typing_x(60, 24, 500);

    press(&mut app, KeyCode::Home);

    assert_eq!(modal_height(&app), Some(17));
    assert_eq!(cursor_of(&app), (12, 12));
}

#[test]
fn rp4_report_modal_details_long() {
    let app = typing_x(60, 24, 500);

    snapshot!("report_modal_details_long", app);
}

#[test]
fn an_error_longer_than_the_room_left_of_the_counter_is_cut_with_an_ellipsis() {
    let mut app = theirs_first(60, 24);
    key(&mut app, '!');
    let effects = press(&mut app, KeyCode::Enter);
    let req = effects
        .iter()
        .find_map(|e| match e {
            Effect::ReportCommit { req, .. } => Some(*req),
            _ => None,
        })
        .unwrap_or(ReqId(u64::MAX));
    let message = "The details field must not be greater than 500 characters.";
    app.update(Action::ReportFailed {
        id: "c0".into(),
        req,
        failure: Failure {
            code: "validation_failed".into(),
            retry_after: None,
            message: Some(message.into()),
        },
    });

    let rows = screen_rows(&app);

    let row = rows.iter().find(|r| r.contains("0/500"));
    assert!(
        row.is_some_and(|r| r.contains("║ ✗ The details field must not be g… ")),
        "{rows:#?}"
    );
    assert!(rows.iter().all(|r| !r.contains("greater")), "{rows:#?}");
}

#[test]
fn while_the_report_is_out_the_footer_says_so() {
    let mut app = theirs_first(60, 24);
    key(&mut app, '!');
    press(&mut app, KeyCode::Enter);

    let rows = screen_rows(&app);

    assert!(
        rows.iter().any(|r| r.contains("Esc cancel   sending…")),
        "{rows:#?}"
    );
    assert!(
        rows.iter().all(|r| !r.contains("Enter submit")),
        "{rows:#?}"
    );
}

// --- the card that is being deleted inside a thread ---------------------------------------------

#[test]
fn dl3_thread_reply_deleting() {
    let reply = item_of(merge(
        commit_json("r1", "myuron", 600),
        json!({"parent_id": MAIN}),
    ));
    let mut app = thread_by("alice", 1, vec![], vec![reply]);
    key(&mut app, 'j');
    delete(&mut app);

    snapshot!("thread_reply_deleting", app);
}

#[test]
fn the_main_commit_that_is_being_deleted_is_drawn_as_the_frame() {
    let mut app = thread_by("myuron", 0, vec![], vec![]);
    delete(&mut app);

    let rows = screen_rows(&app);

    assert!(
        rows.iter().any(|r| r.contains("│ rm m1 → /dev/null │")),
        "{rows:#?}"
    );
    assert!(rows.iter().all(|r| !r.contains("body of m1")), "{rows:#?}");
}
