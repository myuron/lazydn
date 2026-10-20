//! How following, muting and blocking look.
//!
//! - After `f` and `m` the header of a profile offers `[f] Unfollow` and `[m] Unmute`, shows the `[Muted]` badge
//!   after `[Follows you]`, and counts one follower more, and the Aside counts one followed person more.
//! - After `f` on a row of Followers / Following the row ends in `[Following]`.
//! - The block confirmation is a double-framed dialog titled `Block user` that asks `Block @alice?`, says that
//!   they won't be able to follow you or see your commits, and offers `[y] Block` and `[n] Cancel`.
//! - After the block the profile reads `You blocked @bob. [B] Unblock` under the name, the bio and the meta of
//!   the person, like a profile that was blocked before it was opened.

use serde_json::{Value, json};
use unicode_width::UnicodeWidthStr;

use crate::action::Action;
use crate::app::{App, ConnTab, Screen};
use crate::feed_tests::list_width;
use crate::model::{BlockResponse, User, UserResponse};
use crate::profile_tests::{
    deliver_conns, deliver_user, followers, full_user, key, open_profile, posts, profile_with,
    user_of, user_resp,
};
use crate::test_support::*;
use crate::thread_tests::{commit_json, merge, user_json};
use crate::ui::connections::rows as conn_rows;
use crate::ui::profile::rows;

/// Inner width of Main at 120 columns (nav 20 + aside 32 + 2 borders).
const W: usize = 66;

macro_rules! snapshot {
    ($name:literal, $app:expr) => {{
        let terminal = render(&$app);
        insta::assert_snapshot!($name, terminal.backend());
    }};
}

// --- helpers ------------------------------------------------------------------------------------

fn trimmed(line: &ratatui::text::Line) -> String {
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    text.trim_end().to_string()
}

fn profile_lines(app: &App, name: &str) -> Vec<String> {
    rows(app, name, list_width(app))
        .lines
        .iter()
        .map(trimmed)
        .collect()
}

fn conn_lines(app: &App, name: &str, tab: ConnTab) -> Vec<String> {
    conn_rows(app, name, tab, list_width(app))
        .lines
        .iter()
        .map(trimmed)
        .collect()
}

/// `right` ends at column `width`, so a row with the free last column passes `W - 1`.
fn right_aligned(left: &str, right: &str, width: usize) -> String {
    let gap = width.saturating_sub(left.width() + right.width());
    format!("{left}{}{right}", " ".repeat(gap))
}

fn screen_rows(app: &App) -> Vec<String> {
    let terminal = render(app);
    let buffer = terminal.backend().buffer().clone();
    (0..buffer.area.height)
        .map(|y| row_text(&buffer, y))
        .collect()
}

fn person(name: &str, extra: Value) -> User {
    user_of(merge(user_json(name), extra))
}

fn alice_resp() -> UserResponse {
    user_resp(
        "alice",
        json!({
            "is_following": false, "is_muting": false, "follows_you": true,
            "pasteis_received": 42,
            "pinned_commit": commit_json("pin", "alice", 10_800),
        }),
    )
}

/// The profile of alice at 120x40 as in the snapshot `profile_other`: not followed, not muted, follows you.
fn alice_profile() -> App {
    profile_with(
        signed_in(120, 40),
        "alice",
        alice_resp(),
        posts("p", 0, 3, "alice"),
        None,
    )
}

/// Followers / Following of alice: bob (followed), carol and dave (not), and you.
fn people() -> Vec<User> {
    vec![
        person(
            "bob",
            json!({"bio": "coffee-driven developer", "follows_you": true, "is_followed": true}),
        ),
        person(
            "carol",
            json!({"bio": "ssh enthusiast", "follows_you": false, "is_followed": false}),
        ),
        person(
            "dave",
            json!({"bio": null, "follows_you": true, "is_followed": false}),
        ),
        person("myuron", json!({"bio": "that is me", "is_followed": false})),
    ]
}

/// The Following tab of alice at 80x24 with carol selected.
fn following_app() -> App {
    let mut app = signed_in(80, 24);
    let effects = app.update(Action::Open(Screen::Connections(
        "alice".into(),
        ConnTab::Following,
    )));
    deliver_conns(
        &mut app,
        &effects,
        "alice",
        ConnTab::Following,
        people(),
        None,
    );
    key(&mut app, 'j');
    app
}

/// Blocks bob from his loaded profile; the answer carries the person as the server sends them.
fn block_bob(w: u16, h: u16) -> App {
    let mut app = signed_in(w, h);
    let effects = open_profile(&mut app, "bob");
    let resp: UserResponse = serde_json::from_value(json!({
        "data": full_user("bob", json!({})),
        "is_following": false, "is_muting": false, "follows_you": false,
    }))
    .unwrap_or_else(|e| panic!("{e}"));
    deliver_user(&mut app, &effects, "bob", resp);
    key(&mut app, 'B');
    let sent = key(&mut app, 'y');
    let req = sent
        .iter()
        .find_map(|e| match e {
            crate::action::Effect::BlockUser { req, .. } => Some(*req),
            _ => None,
        })
        .unwrap_or(crate::feed::ReqId(u64::MAX));
    app.update(Action::BlockDone {
        username: "bob".into(),
        on: true,
        req,
        response: Box::new(BlockResponse {
            data: person("bob", json!({"bio": "Not for you.", "location": "Lisbon"})),
            is_blocking: true,
        }),
    });
    app
}

// --- text ---------------------------------------------------------------------------------------

#[test]
fn after_f_and_m_the_header_offers_unfollow_and_unmute_and_shows_the_muted_badge_and_one_follower_more()
 {
    let mut app = alice_profile();
    key(&mut app, 'f');
    key(&mut app, 'm');

    let got = profile_lines(&app, "alice");

    assert_eq!(
        got.get(1),
        Some(&right_aligned(
            " ◉ Alice",
            "[f] Unfollow  [m] Unmute  [B] Block",
            W - 1
        ))
    );
    assert_eq!(
        got.get(2).map(String::as_str),
        Some(" ● @alice  [Follows you]  [Muted]")
    );
    assert_eq!(
        got.get(8).map(String::as_str),
        Some(" 187 commits   87 following   3648 followers   ◓ 42 received")
    );
}

#[test]
fn the_aside_counts_one_followed_person_more_after_f() {
    let mut app = alice_profile();
    key(&mut app, 'f');

    let rows = screen_rows(&app);

    assert!(
        rows.iter()
            .any(|r| r.contains("11 following  12 followers")),
        "{rows:#?}"
    );
}

#[test]
fn after_f_on_a_row_it_ends_in_following() {
    let mut app = followers(120, 30, "alice", people(), None);
    key(&mut app, 'j');
    key(&mut app, 'f');

    let got = conn_lines(&app, "alice", ConnTab::Followers);

    assert_eq!(
        got.get(2),
        Some(&right_aligned("▌◉ Carol @carol", "[Following]", W - 1))
    );
    assert_eq!(
        got.get(4),
        Some(&right_aligned(
            " ◉ Dave @dave  [Follows you]",
            "[ Follow ]",
            W - 1
        ))
    );
}

#[test]
fn the_block_confirmation_asks_who_and_what_follows() {
    let mut app = alice_profile();
    key(&mut app, 'B');

    let got = screen_rows(&app);

    let has = |text: &str| got.iter().filter(|r| r.contains(text)).count();
    assert_eq!(has("╔ Block user ═"), 1, "{got:#?}");
    assert_eq!(has("Block @alice?"), 1, "{got:#?}");
    assert_eq!(
        has("They won't be able to follow you or see your commits."),
        1,
        "{got:#?}"
    );
    assert_eq!(has("[y] Block  [n] Cancel"), 1, "{got:#?}");
}

#[test]
fn no_dialog_is_drawn_once_the_confirmation_is_cancelled() {
    let mut app = alice_profile();
    key(&mut app, 'B');
    assert!(
        screen_rows(&app)
            .iter()
            .any(|r| r.contains("Block @alice?")),
        "the dialog is not drawn"
    );
    key(&mut app, 'n');

    let got = screen_rows(&app);

    assert!(got.iter().all(|r| !r.contains("Block @alice?")), "{got:#?}");
}

#[test]
fn after_the_block_the_profile_reads_you_blocked_and_unblock_under_the_header() {
    let app = block_bob(120, 24);

    assert_eq!(
        profile_lines(&app, "bob"),
        [
            "",
            " ◉ Bob",
            " ● @bob",
            "",
            " Not for you.",
            "",
            " ⚲ Lisbon   Joined Oct 2026",
            "",
            " You blocked @bob. [B] Unblock",
        ]
    );
}

// --- snapshots ----------------------------------------------------------------------------------

#[test]
fn pf1_profile_following_muted_at_120x40() {
    let mut app = alice_profile();
    key(&mut app, 'f');
    key(&mut app, 'm');

    snapshot!("profile_following_muted", app);
}

#[test]
fn pf2_block_confirm_at_120x40() {
    let mut app = alice_profile();
    key(&mut app, 'B');

    snapshot!("block_confirm", app);
}

#[test]
fn cn1_connections_following_at_80x24() {
    let mut app = following_app();
    key(&mut app, 'f');

    snapshot!("connections_following", app);
}

#[test]
fn pb1_profile_blocked_after_the_block_at_120x24() {
    snapshot!("profile_blocked_after_block", block_bob(120, 24));
}
