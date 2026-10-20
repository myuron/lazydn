use ratatui::text::Line;
use serde_json::{Value, json};
use time::UtcOffset;
use unicode_width::UnicodeWidthStr;

use crate::action::Action;
use crate::app::{App, ConnTab, Me, ProfileTab, Screen, Session};
use crate::feed_tests::list_width;
use crate::model::{Commit, User, UserResponse};
use crate::profile_tests::*;
use crate::test_support::*;
use crate::thread_tests::{commit_json, commit_of, merge, user_json};
use crate::ui::connections::rows as conn_rows;
use crate::ui::profile::rows;
use crate::ui::statusline::breadcrumb;

/// Inner width of Main at 120 columns (nav 20 + aside 32 + 2 borders).
const W: usize = 66;

// --- helpers ----------------------------------------------------------------

fn trimmed(line: &Line) -> String {
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    text.trim_end().to_string()
}

fn lines(app: &App, name: &str) -> Vec<String> {
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

fn position(lines: &[String], text: &str) -> Option<usize> {
    lines.iter().position(|l| l.contains(text))
}

fn at(lines: &[String], i: usize) -> String {
    lines.get(i).cloned().unwrap_or_default()
}

fn resp_with(user_extra: Value, extra: Value) -> UserResponse {
    serde_json::from_value(merge(
        json!({ "data": full_user("alice", user_extra) }),
        extra,
    ))
    .unwrap()
}

fn two_posts() -> Vec<Commit> {
    posts("p", 0, 2, "alice")
}

fn alice_with(app: App, resp: UserResponse) -> App {
    profile_with(app, "alice", resp, two_posts(), None)
}

fn signed(resp: UserResponse) -> App {
    alice_with(signed_in(120, 40), resp)
}

fn signed_as(me: Me) -> App {
    let mut app = App::new(Session::SignedIn(me), NOW, UtcOffset::UTC);
    app.update(Action::Resize(120, 40));
    app
}

fn person(name: &str, extra: Value) -> User {
    user_of(merge(user_json(name), extra))
}

/// The text of Main's inner area at 120 columns, row by row.
fn main_rows(app: &App) -> Vec<String> {
    let terminal = render(app);
    let buffer = terminal.backend().buffer().clone();
    (0..buffer.area.height)
        .map(|y| {
            row_text(&buffer, y)
                .chars()
                .skip(21)
                .take(W)
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect()
}

fn frame_top(app: &App) -> String {
    let terminal = render(app);
    row_text(terminal.backend().buffer(), 0)
}

const FOLLOW: &str = "[f] Follow  [m] Mute  [B] Block";

// --- 12. header layout ------------------------------------------------------

#[test]
fn the_header_lists_name_handle_bio_meta_stats_and_tabs_in_order() {
    let app = signed(user_resp(
        "alice",
        json!({"is_following": false, "is_muting": false, "follows_you": true, "pasteis_received": 42}),
    ));
    let got = lines(&app, "alice");
    let want = vec![
        String::new(),
        right_aligned(" ◉ Alice", FOLLOW, W - 1),
        " ● @alice  [Follows you]".to_string(),
        String::new(),
        " Building terminal things.".to_string(),
        String::new(),
        " ⌂ @laravel   ⚲ Portugal   ⛓ alice.dev   Joined Oct 2026".to_string(),
        String::new(),
        " 187 commits   87 following   3647 followers   ◓ 42 received".to_string(),
        String::new(),
        format!("{}Commits{}Replies", " ".repeat(13), " ".repeat(26)),
        format!("{}{}", " ".repeat(13), "━".repeat(7)),
    ];
    assert_eq!(got.get(..12).map(<[String]>::to_vec), Some(want));
}

#[test]
fn a_long_bio_wraps_inside_the_width() {
    let bio = "one two three four five six seven eight nine ten eleven twelve thirteen fourteen \
               fifteen sixteen seventeen eighteen nineteen twenty";
    let app = signed(resp_with(json!({ "bio": bio }), json!({})));
    let got = lines(&app, "alice");
    let end = position(&got, "Joined").unwrap_or(0);
    let bio_rows: Vec<&String> = got.iter().take(end.saturating_sub(1)).skip(4).collect();
    assert!(bio_rows.len() >= 2, "{got:#?}");
    assert!(bio_rows.iter().all(|l| l.width() < W && l.starts_with(' ')));
    let joined: Vec<&str> = bio_rows.iter().flat_map(|l| l.split_whitespace()).collect();
    assert_eq!(joined.join(" "), bio);
}

#[test]
fn a_profile_without_a_bio_has_no_bio_rows() {
    let app = signed(resp_with(json!({ "bio": null }), json!({})));
    let got = lines(&app, "alice");
    assert_eq!(at(&got, 3), "");
    assert!(at(&got, 4).contains("Joined"), "{got:#?}");
}

// --- 12. hints and badges ---------------------------------------------------

#[test]
fn the_hints_follow_the_follow_and_mute_state() {
    let cases = [
        (json!({}), "[f] Follow  [m] Mute  [B] Block"),
        (
            json!({"is_following": false, "is_muting": false}),
            "[f] Follow  [m] Mute  [B] Block",
        ),
        (
            json!({"is_following": true, "is_muting": false}),
            "[f] Unfollow  [m] Mute  [B] Block",
        ),
        (
            json!({"is_following": false, "is_muting": true}),
            "[f] Follow  [m] Unmute  [B] Block",
        ),
        (
            json!({"is_following": true, "is_muting": true}),
            "[f] Unfollow  [m] Unmute  [B] Block",
        ),
    ];
    for (extra, hint) in cases {
        let app = signed(user_resp("alice", extra.clone()));
        let got = lines(&app, "alice");
        assert_eq!(
            at(&got, 1),
            right_aligned(" ◉ Alice", hint, W - 1),
            "{extra}"
        );
    }
}

#[test]
fn the_own_profile_offers_only_the_github_hint_whatever_the_case() {
    for typed in ["myuron", "MyUron"] {
        let app = profile_with(
            signed_in(120, 40),
            typed,
            user_resp("myuron", json!({"is_following": false})),
            posts("p", 0, 1, "myuron"),
            None,
        );
        let got = lines(&app, typed);
        assert_eq!(
            at(&got, 1),
            right_aligned(" ◉ Myuron", "[O] GitHub", W - 1),
            "{typed}"
        );
        assert!(!got.iter().any(|l| l.contains("[f]")), "{typed}");
    }
}

#[test]
fn badges_follow_follows_you_and_muting() {
    let cases = [
        (json!({}), " ● @alice"),
        (
            json!({"follows_you": false, "is_muting": false}),
            " ● @alice",
        ),
        (json!({"follows_you": true}), " ● @alice  [Follows you]"),
        (json!({"is_muting": true}), " ● @alice  [Muted]"),
        (
            json!({"follows_you": true, "is_muting": true}),
            " ● @alice  [Follows you]  [Muted]",
        ),
    ];
    for (extra, want) in cases {
        let app = signed(user_resp("alice", extra.clone()));
        assert_eq!(at(&lines(&app, "alice"), 2), want, "{extra}");
    }
}

#[test]
fn a_guest_sees_no_hints_badges_or_received_count() {
    let signed_fields = json!({
        "is_following": true, "follows_you": true, "is_muting": true, "pasteis_received": 42
    });
    for extra in [json!({}), signed_fields] {
        let app = alice_with(dismissed_guest(), user_resp("alice", extra.clone()));
        let got = lines(&app, "alice");
        assert_eq!(at(&got, 1), " ◉ Alice", "{extra}");
        assert_eq!(at(&got, 2), " ● @alice", "{extra}");
        assert!(!got.iter().any(|l| l.contains('◓')), "{extra}");
    }
}

// --- 12. meta ---------------------------------------------------------------

fn meta_of(user_extra: Value) -> String {
    let app = signed(resp_with(user_extra, json!({})));
    let got = lines(&app, "alice");
    position(&got, "Joined")
        .and_then(|i| got.get(i).cloned())
        .unwrap_or_default()
}

#[test]
fn null_meta_fields_are_left_out() {
    assert_eq!(
        meta_of(json!({})),
        " ⌂ @laravel   ⚲ Portugal   ⛓ alice.dev   Joined Oct 2026"
    );
    assert_eq!(
        meta_of(json!({"company": null})),
        " ⚲ Portugal   ⛓ alice.dev   Joined Oct 2026"
    );
    assert_eq!(
        meta_of(json!({"location": null, "website": null})),
        " ⌂ @laravel   Joined Oct 2026"
    );
    assert_eq!(
        meta_of(json!({"company": null, "location": null, "website": null})),
        " Joined Oct 2026"
    );
}

fn joined_at(created_at: &str, hours: i8) -> String {
    let offset = UtcOffset::from_hms(hours, 0, 0).unwrap_or(UtcOffset::UTC);
    let mut app = App::new(Session::SignedIn(fixture_me()), NOW, offset);
    app.update(Action::Resize(120, 40));
    let app = alice_with(app, resp_with(json!({"created_at": created_at}), json!({})));
    let got = lines(&app, "alice");
    let row = position(&got, "Joined").and_then(|i| got.get(i).cloned());
    row.and_then(|l| l.split("Joined ").nth(1).map(String::from))
        .unwrap_or_default()
}

#[test]
fn joined_follows_the_local_offset_at_a_month_boundary() {
    assert_eq!(joined_at("2026-09-30T23:30:00Z", 0), "Sep 2026");
    assert_eq!(joined_at("2026-09-30T23:30:00Z", 9), "Oct 2026");
    assert_eq!(joined_at("2026-10-01T02:00:00Z", -5), "Sep 2026");
    assert_eq!(joined_at("2026-10-01T02:00:00Z", 0), "Oct 2026");
}

// --- 12. stats --------------------------------------------------------------

fn stats_of(app: &App) -> String {
    let got = lines(app, "alice");
    position(&got, " commits")
        .and_then(|i| got.get(i).cloned())
        .unwrap_or_default()
}

#[test]
fn the_stats_show_received_only_with_pasteis_mode_and_a_value() {
    let with = user_resp("alice", json!({"pasteis_received": 42}));
    let without = user_resp("alice", json!({}));
    let base = " 187 commits   87 following   3647 followers";

    assert_eq!(
        stats_of(&signed(with.clone())),
        format!("{base}   ◓ 42 received")
    );
    assert_eq!(stats_of(&signed(without)), base);

    let off = Me {
        pasteis_mode: false,
        ..fixture_me()
    };
    assert_eq!(stats_of(&alice_with(signed_as(off), with.clone())), base);
    assert_eq!(stats_of(&alice_with(dismissed_guest(), with)), base);
}

// --- frame title ------------------------------------------------------------

#[test]
fn the_title_is_the_name_and_the_commit_count() {
    let app = signed(user_resp("alice", json!({})));
    assert!(
        frame_top(&app).contains("┏ Alice · 187 commits ━"),
        "{}",
        frame_top(&app)
    );
}

#[test]
fn the_title_falls_back_to_the_username_without_a_name() {
    let app = signed(resp_with(json!({ "name": null }), json!({})));
    assert!(
        frame_top(&app).contains("┏ alice · 187 commits ━"),
        "{}",
        frame_top(&app)
    );
}

#[test]
fn the_title_names_the_handle_until_the_user_arrives() {
    let mut app = signed_in(120, 40);
    open_profile(&mut app, "alice");
    assert!(
        frame_top(&app).contains("┏ @alice ━"),
        "{}",
        frame_top(&app)
    );
}

// --- tabs -------------------------------------------------------------------

#[test]
fn the_underline_follows_the_selected_profile_tab() {
    let mut app = signed(user_resp("alice", json!({})));
    key(&mut app, '2');
    let got = lines(&app, "alice");
    let tab = position(&got, "Commits").unwrap_or(0);
    assert_eq!(
        at(&got, tab),
        format!("{}Commits{}Replies", " ".repeat(13), " ".repeat(26))
    );
    assert_eq!(
        at(&got, tab + 1),
        format!("{}{}", " ".repeat(46), "━".repeat(7))
    );
}

// --- pinned -----------------------------------------------------------------

fn pinned_resp() -> UserResponse {
    user_resp(
        "alice",
        json!({"pinned_commit": commit_json("pin", "alice", 10_800)}),
    )
}

#[test]
fn the_pinned_commit_comes_first_under_a_heading() {
    let app = signed(pinned_resp());
    let got = lines(&app, "alice");
    let heading = position(&got, "⚲ Pinned");
    assert!(heading.is_some(), "{got:#?}");
    let i = heading.unwrap_or(0);
    assert_eq!(at(&got, i - 1), " │");
    assert_eq!(at(&got, i), " ⚲ Pinned");
    assert!(at(&got, i + 1).starts_with("▌◉ Alice @alice"));
    assert!(at(&got, i + 1).ends_with("0000pin"));
    assert_eq!(got.iter().filter(|l| l.contains("Pinned")).count(), 1);
}

#[test]
fn the_heading_does_not_depend_on_the_order_of_the_responses() {
    let mut app = signed_in(120, 40);
    let effects = open_profile(&mut app, "alice");
    deliver_commits(
        &mut app,
        &effects,
        "alice",
        ProfileTab::Commits,
        two_posts(),
        None,
    );
    deliver_user(&mut app, &effects, "alice", pinned_resp());
    let got = lines(&app, "alice");
    assert!(position(&got, "⚲ Pinned").is_some(), "{got:#?}");
    let i = position(&got, "⚲ Pinned").unwrap_or(0);
    assert!(at(&got, i + 1).ends_with("0000pin"), "{got:#?}");
}

#[test]
fn replies_and_guests_show_no_pinned_heading() {
    let mut app = signed(pinned_resp());
    let effects = key(&mut app, '2');
    deliver_commits(
        &mut app,
        &effects,
        "alice",
        ProfileTab::Replies,
        posts("q", 0, 2, "alice"),
        None,
    );
    let got = lines(&app, "alice");
    assert!(position(&got, "q0").is_some(), "{got:#?}");
    assert!(position(&got, "Pinned").is_none());
    assert!(position(&got, "0000pin").is_none());

    let guest_app = alice_with(dismissed_guest(), pinned_resp());
    let got = lines(&guest_app, "alice");
    assert!(position(&got, "Pinned").is_none());
    assert!(position(&got, "0000pin").is_none());
    assert_eq!(
        rows(&guest_app, "alice", list_width(&guest_app))
            .cards
            .len(),
        2
    );
}

// --- list rows --------------------------------------------------------------

#[test]
fn the_list_has_no_day_rules_head_marker_or_composer() {
    let items = vec![
        commit_of(commit_json("p0", "alice", 600)),
        commit_of(commit_json("p1", "alice", 200_000)),
        commit_of(commit_json("p2", "alice", 900_000)),
    ];
    let app = profile_with(
        signed_in(120, 50),
        "alice",
        user_resp("alice", json!({})),
        items,
        None,
    );
    let got = lines(&app, "alice");
    assert!(position(&got, "body of p2").is_some(), "{got:#?}");
    for banned in ["╎", "HEAD@", "What did you ship", "new commit"] {
        assert!(position(&got, banned).is_none(), "{banned}: {got:#?}");
    }
}

#[test]
fn every_card_has_a_full_header_even_for_consecutive_commits_of_one_author() {
    let app = signed(user_resp("alice", json!({})));
    let got = lines(&app, "alice");
    let headers = got
        .iter()
        .filter(|l| l.contains("◉ Alice @alice ·"))
        .count();
    assert_eq!(headers, 2, "{got:#?}");
}

#[test]
fn the_end_of_the_list_shows_the_root_commit_line() {
    let app = signed(user_resp("alice", json!({})));
    let got = lines(&app, "alice");
    assert_eq!(
        got.last().map(String::as_str),
        Some(" ○ That's the root commit. There's nothing before it.")
    );
}

#[test]
fn a_failed_list_shows_the_retry_hint_under_the_header() {
    let mut app = signed_in(120, 40);
    let effects = open_profile(&mut app, "alice");
    deliver_user(&mut app, &effects, "alice", user_resp("alice", json!({})));
    app.update(Action::ProfileFeedFailed {
        username: "alice".into(),
        tab: ProfileTab::Commits,
        req: commits_req(&effects, false),
        code: "http_500".into(),
    });
    let got = lines(&app, "alice");
    assert_eq!(
        got.last().map(String::as_str),
        Some(" error: http_500 — press R to retry")
    );
    assert!(position(&got, "[f] Follow").is_some());
}

#[test]
fn a_loading_list_shows_the_loading_row_under_the_header() {
    let mut app = signed_in(120, 40);
    let effects = open_profile(&mut app, "alice");
    deliver_user(&mut app, &effects, "alice", user_resp("alice", json!({})));
    let got = lines(&app, "alice");
    assert_eq!(got.last().map(String::as_str), Some(" ┊ (loading...)"));
}

#[test]
fn a_reply_names_its_parent_only_when_the_parent_is_visible() {
    let hidden = merge(
        commit_json("q0", "alice", 600),
        json!({"parent_id": "x1", "parent": null}),
    );
    let visible = merge(
        commit_json("q1", "alice", 700),
        json!({"parent_id": "x2", "parent": commit_json("x2", "bob", 900)}),
    );
    let mut app = signed(user_resp("alice", json!({})));
    let effects = key(&mut app, '2');
    deliver_commits(
        &mut app,
        &effects,
        "alice",
        ProfileTab::Replies,
        vec![commit_of(hidden), commit_of(visible)],
        None,
    );
    let got = lines(&app, "alice");
    let marks: Vec<usize> = got
        .iter()
        .enumerate()
        .filter(|(_, l)| l.contains("replying to"))
        .map(|(i, _)| i)
        .collect();
    assert_eq!(marks.len(), 1, "{got:#?}");
    assert!(at(&got, marks[0]).contains("│ ↳ replying to @bob"));
    assert!(at(&got, marks[0] + 1).contains("body of q1"));
}

// --- 9. error screens -------------------------------------------------------

fn failed_profile(code: &str, blocked: Option<User>, w: u16, h: u16) -> App {
    let mut app = signed_in(w, h);
    let effects = open_profile(&mut app, "bob");
    app.update(Action::UserFailed {
        username: "bob".into(),
        req: user_req(&effects),
        code: code.into(),
        blocked_user: blocked.map(Box::new),
    });
    app.update(Action::ProfileFeedFailed {
        username: "bob".into(),
        tab: ProfileTab::Commits,
        req: commits_req(&effects, false),
        code: code.into(),
    });
    app
}

fn blocked_bob() -> User {
    person("bob", json!({"bio": "Not for you.", "location": "Lisbon"}))
}

#[test]
fn a_blocked_profile_draws_the_header_and_the_unblock_hint_only() {
    let app = failed_profile("user_blocked", Some(blocked_bob()), 120, 24);
    let got = lines(&app, "bob");
    let want = [
        "",
        " ◉ Bob",
        " ● @bob",
        "",
        " Not for you.",
        "",
        " ⚲ Lisbon   Joined Oct 2026",
        "",
        " You blocked @bob. [B] Unblock",
    ];
    assert_eq!(got, want);
    assert!(!frame_top(&app).contains('·'), "{}", frame_top(&app));
    assert!(frame_top(&app).contains("┏ Bob ━"));
}

#[test]
fn an_unavailable_or_missing_profile_says_it_is_not_available() {
    for code in ["user_unavailable", "http_404"] {
        let app = failed_profile(code, None, 120, 24);
        assert_eq!(
            lines(&app, "bob"),
            ["", " fatal: @bob is not available"],
            "{code}"
        );
    }
}

#[test]
fn any_other_profile_error_offers_a_retry() {
    let app = failed_profile("http_500", None, 120, 24);
    assert_eq!(
        lines(&app, "bob"),
        ["", " error: http_500 — press R to retry"]
    );
}

#[test]
fn a_profile_that_is_still_loading_shows_the_loading_row() {
    let mut app = signed_in(120, 24);
    open_profile(&mut app, "bob");
    assert_eq!(lines(&app, "bob"), ["", " ┊ (loading...)"]);
}

// --- breadcrumb -------------------------------------------------------------

#[test]
fn the_breadcrumb_names_the_profile_and_the_connection_tab() {
    let mut app = signed(user_resp("alice", json!({})));
    assert_eq!(breadcrumb(&app), "Profile");
    key(&mut app, '3');
    assert_eq!(breadcrumb(&app), "Profile > Followers");
    key(&mut app, '2');
    assert_eq!(breadcrumb(&app), "Profile > Following");
}

// --- connections ------------------------------------------------------------

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

fn people_app() -> App {
    followers(120, 30, "alice", people(), None)
}

#[test]
fn each_connection_has_a_name_row_with_badge_and_button_and_a_bio_row() {
    let app = people_app();
    let got = conn_lines(&app, "alice", ConnTab::Followers);
    let want = [
        right_aligned("▌◉ Bob @bob  [Follows you]", "[Following]", W - 1),
        "▌  coffee-driven developer".to_string(),
        right_aligned(" ◉ Carol @carol", "[ Follow ]", W - 1),
        "   ssh enthusiast".to_string(),
        right_aligned(" ◉ Dave @dave  [Follows you]", "[ Follow ]", W - 1),
        " ◉ Myuron @myuron".to_string(),
        "   that is me".to_string(),
    ];
    assert_eq!(got, want);
}

#[test]
fn a_connection_without_a_bio_takes_one_row_and_the_rest_take_two() {
    let app = people_app();
    let heights: Vec<usize> = conn_rows(&app, "alice", ConnTab::Followers, list_width(&app))
        .users
        .iter()
        .map(|s| s.height)
        .collect();
    assert_eq!(heights, [2, 2, 1, 2]);
}

#[test]
fn a_long_bio_is_cut_to_one_row() {
    let long = "word ".repeat(40);
    let app = followers(
        120,
        30,
        "alice",
        vec![person("bob", json!({ "bio": long }))],
        None,
    );
    let rows = conn_rows(&app, "alice", ConnTab::Followers, list_width(&app));
    assert_eq!(rows.users.first().map(|s| s.height), Some(2));
    let got = conn_lines(&app, "alice", ConnTab::Followers);
    let bio = at(&got, 1);
    assert!(bio.starts_with("▌  word"), "{bio}");
    assert!(bio.width() < W, "{bio}");
}

#[test]
fn the_own_row_never_gets_a_button_whatever_the_case() {
    for handle in ["myuron", "MyUron"] {
        let app = followers(
            120,
            30,
            "alice",
            vec![person(handle, json!({"is_followed": false}))],
            None,
        );
        let got = conn_lines(&app, "alice", ConnTab::Followers);
        assert!(at(&got, 0).contains("◉"), "{handle}: {got:#?}");
        assert!(!at(&got, 0).contains('['), "{handle}: {got:#?}");
    }
}

#[test]
fn a_guest_sees_no_badges_or_buttons_on_connections() {
    let mut app = dismissed_guest();
    let effects = app.update(Action::Open(Screen::Connections(
        "alice".into(),
        ConnTab::Followers,
    )));
    deliver_conns(
        &mut app,
        &effects,
        "alice",
        ConnTab::Followers,
        people(),
        None,
    );
    let got = conn_lines(&app, "alice", ConnTab::Followers);
    assert!(!got.iter().any(|l| l.contains('[')), "{got:#?}");
    assert_eq!(at(&got, 0), "▌◉ Bob @bob");
}

#[test]
fn the_selection_marker_moves_with_j() {
    let mut app = people_app();
    key(&mut app, 'j');
    let got = conn_lines(&app, "alice", ConnTab::Followers);
    assert!(at(&got, 0).starts_with(" ◉ Bob"));
    assert!(at(&got, 2).starts_with("▌◉ Carol"));
    assert!(at(&got, 3).starts_with("▌  ssh"));
}

#[test]
fn an_empty_list_says_nobody_is_there() {
    let app = followers(120, 24, "alice", vec![], None);
    assert_eq!(
        conn_lines(&app, "alice", ConnTab::Followers),
        [" fatal: no followers yet"]
    );

    let mut app = followers(120, 24, "alice", vec![], None);
    let effects = key(&mut app, '2');
    deliver_conns(
        &mut app,
        &effects,
        "alice",
        ConnTab::Following,
        vec![],
        None,
    );
    assert_eq!(
        conn_lines(&app, "alice", ConnTab::Following),
        [" fatal: not following anyone yet"]
    );
}

#[test]
fn a_loading_or_failed_first_page_shows_a_row_instead_of_users() {
    let mut app = signed_in(120, 24);
    let effects = app.update(Action::Open(Screen::Connections(
        "alice".into(),
        ConnTab::Followers,
    )));
    assert_eq!(
        conn_lines(&app, "alice", ConnTab::Followers),
        [" ┊ (loading...)"]
    );
    app.update(Action::ConnectionsFailed {
        username: "alice".into(),
        tab: ConnTab::Followers,
        req: conn_req(&effects),
        code: "http_500".into(),
    });
    assert_eq!(
        conn_lines(&app, "alice", ConnTab::Followers),
        [" error: http_500 — press R to retry"]
    );
}

#[test]
fn loading_the_next_page_shows_a_row_after_the_last_user() {
    let mut app = followers(120, 40, "alice", users(0, 30), Some("n1"));
    for _ in 0..24 {
        key(&mut app, 'j');
    }
    let got = conn_lines(&app, "alice", ConnTab::Followers);
    assert_eq!(got.last().map(String::as_str), Some(" ┊ (loading more...)"));
}

#[test]
fn the_tab_row_and_underline_stay_above_the_list() {
    let mut app = people_app();
    let top = main_rows(&app);
    assert_eq!(
        at(&top, 1),
        format!("{}Followers{}Following", " ".repeat(12), " ".repeat(24))
    );
    assert_eq!(at(&top, 2), format!("{}{}", " ".repeat(12), "━".repeat(9)));
    assert_eq!(at(&top, 3), "");
    assert!(at(&top, 4).starts_with("▌◉ Bob"));

    key(&mut app, '2');
    let top = main_rows(&app);
    assert_eq!(at(&top, 2), format!("{}{}", " ".repeat(45), "━".repeat(9)));
}

#[test]
fn the_connections_title_is_the_handle_as_typed() {
    let app = followers(120, 30, "Alice", vec![], None);
    assert!(
        frame_top(&app).contains("┏ @Alice ━"),
        "{}",
        frame_top(&app)
    );
}

// --- snapshots --------------------------------------------------------------

macro_rules! snapshot {
    ($name:literal, $app:expr) => {{
        let terminal = render(&$app);
        insta::assert_snapshot!($name, terminal.backend());
    }};
}

#[test]
fn p1_profile_other_at_120x40() {
    let resp = user_resp(
        "alice",
        json!({
            "is_following": false, "is_muting": false, "follows_you": true,
            "pasteis_received": 42,
            "pinned_commit": commit_json("pin", "alice", 10_800),
        }),
    );
    let app = profile_with(
        signed_in(120, 40),
        "alice",
        resp,
        posts("p", 0, 3, "alice"),
        None,
    );
    snapshot!("profile_other", app);
}

#[test]
fn p2_profile_self_at_120x30() {
    let app = profile_with(
        signed_in(120, 30),
        "myuron",
        user_resp("myuron", json!({"pasteis_received": 42})),
        posts("p", 0, 2, "myuron"),
        None,
    );
    snapshot!("profile_self", app);
}

#[test]
fn p3_profile_blocked_at_120x24() {
    snapshot!(
        "profile_blocked",
        failed_profile("user_blocked", Some(blocked_bob()), 120, 24)
    );
}

#[test]
fn p4_profile_unavailable_at_120x24() {
    snapshot!(
        "profile_unavailable",
        failed_profile("user_unavailable", None, 120, 24)
    );
}

#[test]
fn p5_profile_narrow_at_60x40() {
    let resp = resp_with(
        json!({
            "bio": "Building terminal things with Rust, Nix and a lot of coffee, and writing a TUI client for /dev/null on weekends.",
            "website": "https://alice.example.dev/blog/2026",
        }),
        json!({
            "is_following": true, "is_muting": false, "follows_you": true,
            "pinned_commit": commit_json("pin", "alice", 10_800),
        }),
    );
    let app = profile_with(
        signed_in(60, 40),
        "alice",
        resp,
        posts("p", 0, 2, "alice"),
        None,
    );
    snapshot!("profile_narrow", app);
}

#[test]
fn c1_connections_followers_at_120x30() {
    snapshot!("connections_followers", people_app());
}

#[test]
fn c2_connections_empty_at_120x24() {
    snapshot!(
        "connections_empty",
        followers(120, 24, "alice", vec![], None)
    );
}

// --- review follow-ups ------------------------------------------------------

fn narrow_alice(user_extra: Value) -> App {
    profile_with(
        signed_in(60, 40),
        "alice",
        resp_with(user_extra, json!({})),
        two_posts(),
        None,
    )
}

#[test]
fn a_narrow_meta_row_wraps_by_item_and_keeps_joined() {
    let app = narrow_alice(json!({"website": "https://alice.example.dev/blog/2026"}));
    let got = lines(&app, "alice");
    let joined = position(&got, "Joined Oct 2026");
    assert!(joined.is_some(), "{got:#?}");
    let first = position(&got, "⌂ @laravel").unwrap_or(0);
    assert!(joined.unwrap_or(0) > first);
    assert!(got.iter().all(|l| l.width() <= 57), "{got:#?}");
    assert!(position(&got, "⛓ https://alice.example.dev/blog/2026").is_some());
}

#[test]
fn a_single_meta_item_wider_than_the_row_is_cut_with_an_ellipsis() {
    let site = format!("https://example.dev/{}", "x".repeat(80));
    let app = narrow_alice(json!({ "website": site }));
    let got = lines(&app, "alice");
    let cut = got.iter().find(|l| l.contains("⛓ https://example.dev/x"));
    assert!(cut.is_some(), "{got:#?}");
    let cut = cut.cloned().unwrap_or_default();
    assert!(cut.ends_with('…') && cut.width() <= 57, "{cut}");
    assert!(position(&got, "Joined Oct 2026").is_some());
}

#[test]
fn an_empty_tab_says_there_is_nothing_yet() {
    let mut app = profile_with(
        signed_in(120, 40),
        "alice",
        user_resp("alice", json!({})),
        vec![],
        None,
    );
    assert_eq!(
        lines(&app, "alice").last().map(String::as_str),
        Some(" fatal: no commits yet")
    );
    let effects = key(&mut app, '2');
    deliver_commits(
        &mut app,
        &effects,
        "alice",
        ProfileTab::Replies,
        vec![],
        None,
    );
    assert_eq!(
        lines(&app, "alice").last().map(String::as_str),
        Some(" fatal: no replies yet")
    );
}

#[test]
fn a_signed_in_connection_without_a_follow_flag_gets_no_button_but_keeps_its_badge() {
    let app = followers(
        120,
        30,
        "alice",
        vec![person("bob", json!({"follows_you": true}))],
        None,
    );
    let got = conn_lines(&app, "alice", ConnTab::Followers);
    assert_eq!(at(&got, 0), "▌◉ Bob @bob  [Follows you]");
}

#[test]
fn the_own_profile_shows_no_follows_you_badge() {
    let app = profile_with(
        signed_in(120, 40),
        "myuron",
        user_resp("myuron", json!({"follows_you": true})),
        posts("p", 0, 1, "myuron"),
        None,
    );
    assert_eq!(at(&lines(&app, "myuron"), 2), " ● @myuron");
}

fn error_screen(user_first: bool, user_code: &str, feed: Option<&str>) -> Vec<String> {
    let mut app = signed_in(120, 24);
    let effects = open_profile(&mut app, "bob");
    let blocked = (user_code == "user_blocked").then(|| Box::new(blocked_bob()));
    let fail_user = |app: &mut App| {
        app.update(Action::UserFailed {
            username: "bob".into(),
            req: user_req(&effects),
            code: user_code.into(),
            blocked_user: blocked.clone(),
        });
    };
    let feed_result = |app: &mut App| match feed {
        Some(code) => {
            app.update(Action::ProfileFeedFailed {
                username: "bob".into(),
                tab: ProfileTab::Commits,
                req: commits_req(&effects, false),
                code: code.into(),
            });
        }
        None => deliver_commits(
            app,
            &effects,
            "bob",
            ProfileTab::Commits,
            posts("p", 0, 2, "bob"),
            None,
        ),
    };
    if user_first {
        fail_user(&mut app);
        feed_result(&mut app);
    } else {
        feed_result(&mut app);
        fail_user(&mut app);
    }
    lines(&app, "bob")
}

#[test]
fn a_user_error_is_drawn_the_same_whichever_response_comes_first() {
    for code in ["user_blocked", "user_unavailable", "http_500"] {
        let a = error_screen(true, code, Some(code));
        let b = error_screen(false, code, Some(code));
        assert!(a.len() >= 2, "{code}: {a:#?}");
        assert_eq!(a, b, "{code}");
    }
}

#[test]
fn a_user_error_hides_a_list_that_loaded() {
    let got = error_screen(true, "http_500", None);
    assert_eq!(got, ["", " error: http_500 — press R to retry"]);
    assert_eq!(error_screen(false, "http_500", None), got);
}

fn nav_profile_row(app: &App) -> String {
    let terminal = render(app);
    let buffer = terminal.backend().buffer().clone();
    (0..buffer.area.height)
        .map(|y| row_text(&buffer, y))
        .find(|r| r.contains("◯ Profile"))
        .unwrap_or_default()
}

#[test]
fn the_nav_marks_profile_only_for_the_own_profile() {
    let own = profile_with(
        signed_in(120, 40),
        "myuron",
        user_resp("myuron", json!({})),
        posts("p", 0, 1, "myuron"),
        None,
    );
    assert!(nav_profile_row(&own).starts_with("│▌◯ Profile"));

    let other = signed(user_resp("alice", json!({})));
    assert!(nav_profile_row(&other).starts_with("│ ◯ Profile"));

    let conns = followers(120, 30, "myuron", vec![], None);
    assert!(nav_profile_row(&conns).starts_with("│ ◯ Profile"));
}

// --- pinned commit with a list that is not there yet ------------------------

fn pinned_profile_without_list() -> (App, Vec<crate::action::Effect>) {
    let mut app = signed_in(120, 40);
    let effects = open_profile(&mut app, "alice");
    deliver_user(&mut app, &effects, "alice", pinned_resp());
    (app, effects)
}

#[test]
fn a_loading_list_under_the_pinned_commit_is_not_the_root_commit() {
    let (app, _) = pinned_profile_without_list();
    let got = lines(&app, "alice");
    assert!(position(&got, "0000pin").is_some(), "{got:#?}");
    assert_eq!(got.last().map(String::as_str), Some(" ┊ (loading...)"));
}

#[test]
fn a_failed_list_under_the_pinned_commit_shows_the_retry_hint() {
    let (mut app, effects) = pinned_profile_without_list();
    app.update(Action::ProfileFeedFailed {
        username: "alice".into(),
        tab: ProfileTab::Commits,
        req: commits_req(&effects, false),
        code: "http_500".into(),
    });
    let got = lines(&app, "alice");
    assert!(position(&got, "0000pin").is_some(), "{got:#?}");
    assert_eq!(
        got.last().map(String::as_str),
        Some(" error: http_500 — press R to retry")
    );
}

#[test]
fn an_empty_list_under_the_pinned_commit_says_there_are_no_commits() {
    let (mut app, effects) = pinned_profile_without_list();
    deliver_commits(
        &mut app,
        &effects,
        "alice",
        ProfileTab::Commits,
        vec![],
        None,
    );
    let got = lines(&app, "alice");
    assert_eq!(
        got.last().map(String::as_str),
        Some(" fatal: no commits yet")
    );
}
