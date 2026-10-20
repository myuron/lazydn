//! Replying to and quoting a commit: the reply box under a card, the quote dialog, and what a created reply changes.
//!
//! - `r` opens a box under the selected card of Log, Saved, a profile list or a thread, and `q` opens the composer
//!   dialog titled `Quote @handle`. The text of a reply is the draft `Reply(id)` and that of a quote `Quote(id)`.
//! - Only one composer is open, and only one commit is out. A tombstone cannot be replied to or quoted.
//! - A created reply raises `replies_count` of its parent in every copy, joins the replies of a thread that are
//!   fully loaded, and never enters the Log lists; a created quote enters them like a new commit.

use crossterm::event::{KeyCode, KeyEventKind, KeyModifiers};
use serde_json::json;
use time::UtcOffset;

use crate::action::{Action, Effect, Failure};
use crate::app::{App, ConnTab, LogTab, Modal, Pane, ProfileTab, Screen, Session, ToastKind};
use crate::compose::Surface;
use crate::drafts::{DraftKey, Drafts};
use crate::editor::Editor;
use crate::feed::{FeedKey, FeedPage, ReqId};
use crate::feed_tests::{FOLLOWING, LATEST, TRENDING, deliver, down, page, started};
use crate::keyboard::KeyboardMode;
use crate::keymap::{map_event, map_key};
use crate::model::{Commit, ThreadItem};
use crate::profile_tests::{alice, deliver_commits, deliver_user, open_profile, user_resp};
use crate::test_support::*;
use crate::thread::Entry;
use crate::thread_tests::{
    MAIN, ancestor, ancestor_tomb, child, commit_json, commit_of, deliver_replies, deliver_thread,
    item, merge, open_thread, opened, others, state, tomb, with_replies_count,
};
use crate::ui::compose::compose_text_width;

const ACCOUNT: &str = "0190a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b";
const NOT_APPROVED: &str = "device_not_approved";
const STILL_SENDING: &str = "a commit is still being sent";

// --- helpers ------------------------------------------------------------------

fn key(app: &mut App, c: char) -> Vec<Effect> {
    press(app, KeyCode::Char(c))
}

fn write(app: &mut App, text: &str) {
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            press(app, KeyCode::Enter);
        }
        type_str(app, line);
    }
}

fn submit(app: &mut App) -> Vec<Effect> {
    ctrl(app, 's')
}

/// Signed in on Following with `n` commits `c0`.. by alice; `c0` is selected.
fn log_app(n: usize) -> App {
    let (mut app, effects) = started(120, 40);
    deliver(&mut app, FOLLOWING, &effects, page(0, n, None));
    app
}

/// Like `log_app`, with the drafts of an account whose id may be kept.
fn account_log(n: usize) -> App {
    let me = crate::app::Me {
        id: ACCOUNT.into(),
        ..fixture_me()
    };
    let mut app = App::new(Session::SignedIn(me), NOW, UtcOffset::UTC);
    let mut effects = app.update(Action::Init);
    effects.extend(app.update(Action::Resize(120, 40)));
    deliver(&mut app, FOLLOWING, &effects, page(0, n, None));
    app
}

/// Signed in as an account whose drafts may be kept, with no screen loaded.
fn account_app() -> App {
    let me = crate::app::Me {
        id: ACCOUNT.into(),
        ..fixture_me()
    };
    let mut app = App::new(Session::SignedIn(me), NOW, UtcOffset::UTC);
    app.update(Action::Resize(120, 40));
    app
}

fn saved_app(n: usize) -> App {
    let mut app = signed_in(120, 40);
    let effects = app.update(Action::Goto(Screen::Saved));
    deliver(&mut app, FeedKey::Saved, &effects, page(0, n, None));
    app
}

fn guest_log() -> App {
    let mut app = guest(120, 40);
    let effects = app.update(Action::Init);
    deliver(&mut app, LATEST, &effects, page(0, 3, None));
    app.update(Action::CloseModal);
    app
}

fn reply_key(id: &str) -> DraftKey {
    DraftKey::Reply(id.into())
}

fn quote_key(id: &str) -> DraftKey {
    DraftKey::Quote(id.into())
}

fn compose_key(app: &App) -> Option<DraftKey> {
    app.compose().map(|c| c.key.clone())
}

fn compose_surface(app: &App) -> Option<Surface> {
    app.compose().map(|c| c.surface)
}

fn draft_of(app: &App, key: &DraftKey) -> Option<String> {
    app.drafts().text(key)
}

/// Log with `n` commits, the `index`-th selected, and a reply box opened under it with `text` typed.
fn replying(n: usize, index: usize, text: &str) -> App {
    let mut app = log_app(n);
    down(&mut app, index);
    open_reply(&mut app);
    write(&mut app, text);
    app
}

/// Presses `r` and checks that a box opened, so that the text typed next cannot reach anything else.
fn open_reply(app: &mut App) {
    key(app, 'r');
    assert_eq!(
        compose_surface(app),
        Some(Surface::ReplyBox),
        "r did not open a reply box"
    );
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

fn sent_reply(index: usize, text: &str) -> (App, ReqId) {
    let mut app = replying(3, index, text);
    let req = create_req(&submit(&mut app));
    assert!(
        app.compose().is_some_and(|c| c.submitting),
        "the reply was not sent"
    );
    (app, req)
}

fn reply_commit(parent: &str, status: &str) -> Commit {
    commit_of(merge(
        commit_json("n1", "myuron", 5),
        json!({"short_hash": "a1b2c3d", "status": status, "parent_id": parent}),
    ))
}

fn quote_commit(quoted: &str, status: &str) -> Commit {
    commit_of(merge(
        commit_json("n1", "myuron", 5),
        json!({"short_hash": "a1b2c3d", "status": status, "quoted_commit_id": quoted}),
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

fn toast(app: &App) -> Option<(ToastKind, String)> {
    app.toast().map(|t| (t.kind, t.text.clone()))
}

fn ids(app: &App, key: FeedKey) -> Vec<String> {
    app.feed(key).items.iter().map(|c| c.id.clone()).collect()
}

fn count_in(app: &App, key: FeedKey, index: usize) -> Option<Option<u32>> {
    app.feed(key).items.get(index).map(|c| c.replies_count)
}

fn screen_rows(app: &App) -> Vec<String> {
    let terminal = render(app);
    let buffer = terminal.backend().buffer().clone();
    (0..buffer.area.height)
        .map(|y| row_text(&buffer, y))
        .collect()
}

fn screen_text(app: &App) -> String {
    screen_rows(app).join("\n")
}

fn cursor_of(app: &App) -> (u16, u16) {
    let mut terminal = render(app);
    let p = terminal.get_cursor_position().unwrap_or_default();
    (p.x, p.y)
}

fn drafts_of(entries: &[(DraftKey, &str)]) -> Drafts {
    let mut drafts = Drafts::default();
    for (key, text) in entries {
        drafts.set(key.clone(), Editor::from_text(text));
    }
    drafts
}

fn load_drafts(app: &mut App, entries: &[(DraftKey, &str)]) {
    app.update(Action::DraftsLoaded {
        account: ACCOUNT.into(),
        drafts: drafts_of(entries),
    });
}

fn mapped(app: &App, c: char) -> Option<Action> {
    map_key(
        app,
        key_event(KeyCode::Char(c), KeyModifiers::NONE, KeyEventKind::Press),
    )
}

/// The first and last row of the reply box on screen: the row with its title and the one with its bottom edge.
fn box_rows(app: &App) -> Option<(usize, usize)> {
    let rows = screen_rows(app);
    let top = rows.iter().position(|r| r.contains("╔ Reply to @"))?;
    let bottom = rows.iter().skip(top).position(|r| r.contains('╚'))? + top;
    Some((top, bottom))
}

fn replies_count_of(item: &ThreadItem) -> Option<u32> {
    match item {
        ThreadItem::Commit(c) => c.replies_count,
        ThreadItem::Tombstone(t) => t.replies_count,
    }
}

fn thread_children(app: &App, parent: &str) -> Vec<String> {
    state(app, MAIN)
        .children
        .get(parent)
        .map(|l| {
            l.items
                .iter()
                .map(|i| crate::thread::item_id(i).to_string())
                .collect()
        })
        .unwrap_or_default()
}

fn thread_selected_entry(app: &App) -> Option<Entry> {
    let s = state(app, MAIN);
    s.entries(MAIN, false).into_iter().nth(s.selected)
}

fn feed_page(items: Vec<Commit>) -> FeedPage {
    FeedPage {
        items,
        next_cursor: None,
        is_ranked: false,
        suggestions: Vec::new(),
    }
}

/// Thread of `MAIN` by alice with the replies `r0` (bob) and `r1` (carol), each with one reply under it.
fn thread_app() -> App {
    opened(120, 40, vec![], 2, others(0, 2), None)
}

// --- keymap ------------------------------------------------------------------------

#[test]
fn r_and_q_are_reply_and_quote_on_every_list_with_the_main_pane_focused() {
    let mut saved = signed_in(120, 40);
    saved.update(Action::Goto(Screen::Saved));
    let scenes = [
        ("log", log_app(3)),
        ("saved", saved),
        ("profile", alice(120, 40, 3)),
        ("thread", thread_app()),
    ];
    for (name, app) in scenes {
        assert_eq!(mapped(&app, 'r'), Some(Action::Reply), "{name}");
        assert_eq!(mapped(&app, 'q'), Some(Action::Quote), "{name}");
    }
}

#[test]
fn r_and_q_do_nothing_on_screens_without_commit_cards_and_in_the_nav_pane() {
    let mut app = signed_in(120, 40);
    for screen in [Screen::Notifications, Screen::Settings] {
        app.update(Action::Goto(screen.clone()));
        assert_eq!(mapped(&app, 'r'), None, "{screen:?}");
        assert_eq!(mapped(&app, 'q'), None, "{screen:?}");
    }
    app.update(Action::Open(Screen::Connections(
        "alice".into(),
        ConnTab::Followers,
    )));
    assert_eq!(mapped(&app, 'r'), None);
    assert_eq!(mapped(&app, 'q'), None);

    let mut log = log_app(3);
    press(&mut log, KeyCode::Tab);
    assert_eq!(log.focus(), Pane::Nav);
    assert_eq!(mapped(&log, 'r'), None);
    assert_eq!(mapped(&log, 'q'), None);
}

#[test]
fn r_and_q_are_text_while_the_reply_box_has_the_focus() {
    let app = replying(3, 0, "");

    assert_eq!(
        mapped(&app, 'r'),
        Some(Action::Compose(crate::keymap::ComposerKey::Insert('r')))
    );
    assert_eq!(
        mapped(&app, 'q'),
        Some(Action::Compose(crate::keymap::ComposerKey::Insert('q')))
    );
}

#[test]
fn a_guest_gets_the_actions_too_so_that_the_app_can_ask_to_sign_in() {
    let app = guest_log();

    assert_eq!(mapped(&app, 'r'), Some(Action::Reply));
    assert_eq!(mapped(&app, 'q'), Some(Action::Quote));
}

// --- opening a reply box (plan 1) --------------------------------------------------------

#[test]
fn r_opens_a_reply_box_under_the_selected_log_card() {
    let mut app = log_app(3);
    down(&mut app, 1);

    let effects = key(&mut app, 'r');

    assert_eq!(compose_key(&app), Some(reply_key("c1")));
    assert_eq!(compose_surface(&app), Some(Surface::ReplyBox));
    assert!(app.compose().is_some_and(|c| !c.submitting));
    assert!(app.composer_focused());
    assert_eq!(app.modal(), None);
    assert_eq!(app.screen(), Screen::Log(LogTab::Following));
    assert!(effects.is_empty(), "{effects:?}");
    assert!(
        screen_text(&app).contains("╔ Reply to @alice "),
        "{}",
        screen_text(&app)
    );
}

#[test]
fn r_opens_a_reply_box_on_the_first_card_too() {
    let mut app = log_app(3);

    open_reply(&mut app);

    assert_eq!(compose_key(&app), Some(reply_key("c0")));
}

#[test]
fn opening_and_closing_the_box_without_typing_leaves_no_draft() {
    let mut app = log_app(3);

    open_reply(&mut app);
    press(&mut app, KeyCode::Esc);

    assert!(app.compose().is_none());
    assert!(app.drafts().is_empty());
}

#[test]
fn r_opens_a_reply_box_on_a_saved_card() {
    let mut app = saved_app(3);
    down(&mut app, 2);

    open_reply(&mut app);

    assert_eq!(compose_key(&app), Some(reply_key("c2")));
    assert_eq!(compose_surface(&app), Some(Surface::ReplyBox));
    assert_eq!(app.modal(), None);
}

#[test]
fn r_opens_a_reply_box_on_a_profile_card_on_both_tabs_and_on_the_pinned_one() {
    let mut app = alice(120, 40, 3);
    down(&mut app, 1);
    open_reply(&mut app);
    assert_eq!(compose_key(&app), Some(reply_key("p1")));
    assert_eq!(compose_surface(&app), Some(Surface::ReplyBox));
    press(&mut app, KeyCode::Esc);

    let effects = key(&mut app, '2');
    deliver_commits(
        &mut app,
        &effects,
        "alice",
        ProfileTab::Replies,
        vec![commit_of(commit_json("q0", "alice", 60))],
        None,
    );
    open_reply(&mut app);
    assert_eq!(compose_key(&app), Some(reply_key("q0")));
    press(&mut app, KeyCode::Esc);

    let mut pinned = signed_in(120, 40);
    let effects = open_profile(&mut pinned, "alice");
    deliver_user(
        &mut pinned,
        &effects,
        "alice",
        user_resp(
            "alice",
            json!({"is_following": false, "pinned_commit": commit_json("pin", "alice", 900)}),
        ),
    );
    deliver_commits(
        &mut pinned,
        &effects,
        "alice",
        ProfileTab::Commits,
        vec![commit_of(commit_json("p0", "alice", 60))],
        None,
    );
    open_reply(&mut pinned);
    assert_eq!(compose_key(&pinned), Some(reply_key("pin")));
}

#[test]
fn r_opens_a_reply_box_on_every_kind_of_card_of_a_thread() {
    let ancestors = vec![ancestor("a1", "carol")];
    let replies = vec![
        item("k1", "alice"),
        with_replies_count(item("r1", "bob"), 0),
    ];
    let mut app = opened(120, 40, ancestors, 2, replies, None);
    // The selection starts on the main commit; the ancestor is above it and the continuation below.
    open_reply(&mut app);
    assert_eq!(compose_key(&app), Some(reply_key(MAIN)), "main");
    assert_eq!(compose_surface(&app), Some(Surface::ReplyBox));
    press(&mut app, KeyCode::Esc);

    key(&mut app, 'k');
    open_reply(&mut app);
    assert_eq!(compose_key(&app), Some(reply_key("a1")), "ancestor");
    press(&mut app, KeyCode::Esc);

    down(&mut app, 2);
    assert_eq!(thread_selected_entry(&app), Some(Entry::Continuation));
    open_reply(&mut app);
    assert_eq!(compose_key(&app), Some(reply_key("k1")), "continuation");
    press(&mut app, KeyCode::Esc);

    key(&mut app, 'j');
    open_reply(&mut app);
    assert_eq!(compose_key(&app), Some(reply_key("r1")), "reply");
}

#[test]
fn r_and_q_do_nothing_on_a_tombstone_or_a_show_more_row() {
    let ancestors = vec![ancestor_tomb("a1")];
    let replies = vec![tomb("t1"), item("r1", "bob")];
    let mut app = opened(120, 40, ancestors, 3, replies, Some("next"));
    // Entries: the tombstone ancestor, the main commit, the tombstone reply, r1, "Show more".
    key(&mut app, 'k');
    assert_eq!(thread_selected_entry(&app), Some(Entry::Ancestor(0)));
    for c in ['r', 'q'] {
        key(&mut app, c);
        assert!(app.compose().is_none(), "ancestor tombstone {c}");
        assert_eq!(app.modal(), None, "ancestor tombstone {c}");
    }

    down(&mut app, 2);
    assert!(matches!(thread_selected_entry(&app), Some(Entry::Reply(id, _)) if id == "t1"));
    for c in ['r', 'q'] {
        key(&mut app, c);
        assert!(app.compose().is_none(), "reply tombstone {c}");
        assert_eq!(app.modal(), None, "reply tombstone {c}");
    }

    down(&mut app, 2);
    assert!(matches!(
        thread_selected_entry(&app),
        Some(Entry::ShowMore(_))
    ));
    for c in ['r', 'q'] {
        let effects = key(&mut app, c);
        assert!(app.compose().is_none(), "show more {c}");
        assert!(
            !effects
                .iter()
                .any(|e| matches!(e, Effect::FetchReplies { .. })),
            "{effects:?}"
        );
    }
}

#[test]
fn r_and_q_do_nothing_on_an_empty_list() {
    let (mut app, effects) = started(120, 40);
    deliver(&mut app, FOLLOWING, &effects, page(0, 0, None));

    key(&mut app, 'r');
    key(&mut app, 'q');

    assert!(app.compose().is_none());
    assert_eq!(app.modal(), None);
}

#[test]
fn a_guests_r_and_q_open_the_sign_in_dialog_and_no_composer() {
    for c in ['r', 'q'] {
        let mut app = guest_log();

        let effects = key(&mut app, c);

        assert_eq!(app.modal(), Some(Modal::SignIn), "{c}");
        assert!(app.compose().is_none(), "{c}");
        assert!(creates(&effects).is_empty(), "{c}");
    }
}

#[test]
fn r_and_q_while_restoring_do_nothing() {
    for c in ['r', 'q'] {
        let mut app = App::new(Session::Restoring, NOW, UtcOffset::UTC);
        app.update(Action::Resize(120, 40));

        let effects = key(&mut app, c);

        assert!(app.compose().is_none(), "{c}");
        assert_eq!(app.modal(), None, "{c}");
        assert!(effects.is_empty(), "{c}: {effects:?}");
    }
}

// --- typing in the box (plan 7) ------------------------------------------------------------

#[test]
fn keys_with_a_meaning_elsewhere_are_typed_into_the_box_and_the_selection_stays() {
    let mut app = log_app(5);
    down(&mut app, 2);
    open_reply(&mut app);

    let effects = type_str(&mut app, "jkGq?gl12cQro@");

    assert_eq!(
        draft_of(&app, &reply_key("c2")),
        Some("jkGq?gl12cQro@".into())
    );
    assert_eq!(app.feed(FOLLOWING).selected, 2);
    assert_eq!(app.screen(), Screen::Log(LogTab::Following));
    assert_eq!(app.modal(), None);
    assert!(effects.is_empty(), "{effects:?}");
    assert!(app.composer_focused());
}

#[test]
fn enter_starts_a_new_line_in_the_box_and_does_not_open_the_thread() {
    let mut app = log_app(3);
    open_reply(&mut app);
    type_str(&mut app, "a");

    press(&mut app, KeyCode::Enter);
    type_str(&mut app, "b");

    assert_eq!(draft_of(&app, &reply_key("c0")), Some("a\nb".into()));
    assert_eq!(app.screen(), Screen::Log(LogTab::Following));
}

#[test]
fn a_paste_goes_into_the_box() {
    let mut app = log_app(3);
    open_reply(&mut app);

    let mapped = map_event(&app, &crossterm::event::Event::Paste("one\ntwo".into()));
    assert_eq!(mapped, Some(Action::Paste("one\ntwo".into())));
    app.update(Action::Paste("one\ntwo".into()));

    assert_eq!(draft_of(&app, &reply_key("c0")), Some("one\ntwo".into()));
}

#[test]
fn ctrl_c_quits_from_the_box() {
    let mut app = log_app(3);
    open_reply(&mut app);

    assert!(quit_requested(&ctrl(&mut app, 'c')));
}

#[test]
fn esc_closes_the_box_and_keeps_the_draft() {
    let mut app = replying(3, 1, "keep me");

    press(&mut app, KeyCode::Esc);

    assert!(app.compose().is_none());
    assert!(!app.composer_focused());
    assert_eq!(draft_of(&app, &reply_key("c1")), Some("keep me".into()));
    assert_eq!(app.screen(), Screen::Log(LogTab::Following));
}

// --- drafts per parent (plan 2, 3) -------------------------------------------------------------

#[test]
fn a_reopened_box_holds_the_draft_with_the_cursor_at_its_end() {
    let mut app = replying(3, 1, "first\nsecond");
    press(&mut app, KeyCode::Esc);

    open_reply(&mut app);
    type_str(&mut app, "!");

    assert_eq!(
        draft_of(&app, &reply_key("c1")),
        Some("first\nsecond!".into())
    );
}

#[test]
fn each_card_has_its_own_reply_draft() {
    let mut app = replying(3, 0, "for zero");
    press(&mut app, KeyCode::Esc);
    key(&mut app, 'j');
    open_reply(&mut app);
    write(&mut app, "for one");

    assert_eq!(compose_key(&app), Some(reply_key("c1")));
    assert_eq!(draft_of(&app, &reply_key("c0")), Some("for zero".into()));
    assert_eq!(draft_of(&app, &reply_key("c1")), Some("for one".into()));
}

#[test]
fn a_reply_draft_shows_as_stash_on_the_action_row_of_its_card_only() {
    let mut app = replying(3, 1, "half written");
    press(&mut app, KeyCode::Esc);

    let rows = screen_rows(&app);
    let header = rows.iter().position(|r| r.contains("0000001"));
    let actions: Vec<usize> = rows
        .iter()
        .enumerate()
        .filter(|(_, r)| r.contains("↩ stash@{0}"))
        .map(|(i, _)| i)
        .collect();

    assert_eq!(
        header.map(|h| vec![h + 3]),
        Some(actions),
        "{}",
        rows.join("\n")
    );
    assert_eq!(
        rows.iter().filter(|r| r.contains("↩ Reply")).count(),
        2,
        "{}",
        rows.join("\n")
    );
}

#[test]
fn the_action_row_shows_stash_while_the_box_with_text_is_open() {
    let app = replying(3, 0, "typing");

    assert_eq!(
        screen_text(&app).matches("↩ stash@{0}").count(),
        1,
        "{}",
        screen_text(&app)
    );
}

#[test]
fn an_open_empty_box_leaves_the_action_row_at_reply() {
    let mut app = log_app(3);
    open_reply(&mut app);

    assert!(!screen_text(&app).contains("↩ stash@{0}"));
}

#[test]
fn a_quote_draft_does_not_turn_reply_into_stash() {
    let mut app = account_log(3);
    load_drafts(
        &mut app,
        &[
            (quote_key("c0"), "quoted words"),
            (reply_key("c2"), "reply"),
        ],
    );

    let rows = screen_rows(&app);
    let header = rows.iter().position(|r| r.contains("0000002"));
    let stash: Vec<usize> = rows
        .iter()
        .enumerate()
        .filter(|(_, r)| r.contains("↩ stash@{0}"))
        .map(|(i, _)| i)
        .collect();

    assert_eq!(
        header.map(|h| vec![h + 3]),
        Some(stash),
        "{}",
        rows.join("\n")
    );
}

#[test]
fn has_draft_reaches_the_cards_of_log_saved_and_profile() {
    let mut log = account_log(3);
    load_drafts(&mut log, &[(reply_key("c2"), "x")]);
    assert_eq!(screen_text(&log).matches("↩ stash@{0}").count(), 1, "log");

    let mut saved = account_app();
    let effects = saved.update(Action::Goto(Screen::Saved));
    deliver(&mut saved, FeedKey::Saved, &effects, page(0, 3, None));
    load_drafts(&mut saved, &[(reply_key("c1"), "x")]);
    assert_eq!(
        screen_text(&saved).matches("↩ stash@{0}").count(),
        1,
        "saved"
    );

    let mut profile = account_app();
    let effects = open_profile(&mut profile, "alice");
    deliver_user(
        &mut profile,
        &effects,
        "alice",
        user_resp("alice", json!({"is_following": false})),
    );
    deliver_commits(
        &mut profile,
        &effects,
        "alice",
        ProfileTab::Commits,
        vec![commit_of(commit_json("p0", "alice", 60))],
        None,
    );
    load_drafts(&mut profile, &[(reply_key("p0"), "x")]);
    assert_eq!(
        screen_text(&profile).matches("↩ stash@{0}").count(),
        1,
        "profile"
    );
}

#[test]
fn has_draft_reaches_the_main_commit_the_ancestors_and_the_replies_of_a_thread() {
    let make = || {
        let mut app = account_app();
        let effects = open_thread(&mut app, MAIN);
        deliver_thread(&mut app, &effects, vec![ancestor("a1", "carol")], 2);
        deliver_replies(&mut app, &effects, MAIN, others(0, 2), None);
        app
    };
    for id in [MAIN, "a1", "r0", "r1"] {
        let mut app = make();
        load_drafts(&mut app, &[(reply_key(id), "x")]);
        assert_eq!(
            screen_text(&app).matches("↩ stash@{0}").count(),
            1,
            "{id}: {}",
            screen_text(&app)
        );
    }
}

// --- one composer at a time ----------------------------------------------------------------

#[test]
fn a_reply_on_another_card_replaces_the_open_box_and_keeps_its_draft() {
    let mut app = replying(3, 0, "for zero");
    app.update(Action::MoveSelection(1));

    let effects = app.update(Action::Reply);

    assert_eq!(compose_key(&app), Some(reply_key("c1")));
    assert_eq!(compose_surface(&app), Some(Surface::ReplyBox));
    assert_eq!(draft_of(&app, &reply_key("c0")), Some("for zero".into()));
    assert!(effects.is_empty(), "{effects:?}");
}

#[test]
fn writing_a_new_commit_replaces_the_open_box_and_keeps_its_draft() {
    let mut app = replying(3, 1, "reply text");

    app.update(Action::Commit);

    assert_eq!(compose_key(&app), Some(DraftKey::New));
    assert_eq!(compose_surface(&app), Some(Surface::Inline));
    assert_eq!(draft_of(&app, &reply_key("c1")), Some("reply text".into()));
}

#[test]
fn a_reply_replaces_the_open_new_composer_and_keeps_its_draft() {
    let mut app = log_app(3);
    key(&mut app, 'c');
    write(&mut app, "new text");

    app.update(Action::Reply);

    assert_eq!(compose_key(&app), Some(reply_key("c0")));
    assert_eq!(compose_surface(&app), Some(Surface::ReplyBox));
    assert_eq!(draft_of(&app, &DraftKey::New), Some("new text".into()));
}

#[test]
fn a_quote_replaces_the_open_box_and_a_reply_replaces_the_quote_dialog() {
    let mut app = replying(3, 1, "reply text");

    app.update(Action::Quote);

    assert_eq!(compose_key(&app), Some(quote_key("c1")));
    assert_eq!(compose_surface(&app), Some(Surface::Modal));
    assert_eq!(app.modal(), Some(Modal::Composer));
    assert_eq!(draft_of(&app, &reply_key("c1")), Some("reply text".into()));
    write(&mut app, "quote text");

    app.update(Action::Reply);

    assert_eq!(compose_key(&app), Some(reply_key("c1")));
    assert_eq!(compose_surface(&app), Some(Surface::ReplyBox));
    assert_eq!(app.modal(), None);
    assert_eq!(draft_of(&app, &quote_key("c1")), Some("quote text".into()));
}

#[test]
fn nothing_opens_while_the_help_dialog_is_up() {
    for action in [Action::Reply, Action::Quote] {
        let mut app = log_app(3);
        key(&mut app, '?');
        assert_eq!(app.modal(), Some(Modal::Help));

        app.update(action.clone());

        assert!(app.compose().is_none(), "{action:?}");
        assert_eq!(app.modal(), Some(Modal::Help), "{action:?}");
    }
}

#[test]
fn a_reply_to_the_commit_of_the_open_box_changes_nothing() {
    let mut app = replying(3, 1, "same box");

    app.update(Action::Reply);

    assert_eq!(compose_key(&app), Some(reply_key("c1")));
    assert_eq!(draft_of(&app, &reply_key("c1")), Some("same box".into()));
}

// --- the box goes away with its parent -------------------------------------------------------

#[test]
fn the_box_closes_with_its_draft_kept_when_a_reload_no_longer_holds_the_parent() {
    let mut app = replying(3, 1, "orphaned");
    let effects = app.update(Action::Refresh);

    deliver(&mut app, FOLLOWING, &effects, page(2, 5, None));

    assert!(app.compose().is_none());
    assert!(!app.composer_focused());
    assert_eq!(draft_of(&app, &reply_key("c1")), Some("orphaned".into()));
}

#[test]
fn the_box_stays_when_a_reload_still_holds_the_parent() {
    let mut app = replying(3, 1, "still here");
    let effects = app.update(Action::Refresh);

    deliver(&mut app, FOLLOWING, &effects, page(0, 3, None));

    assert_eq!(compose_key(&app), Some(reply_key("c1")));
    assert_eq!(draft_of(&app, &reply_key("c1")), Some("still here".into()));
}

#[test]
fn the_box_closes_when_the_screen_changes_to_one_without_the_parent() {
    let mut app = replying(3, 1, "left behind");

    app.update(Action::Goto(Screen::Settings));

    assert!(app.compose().is_none());
    assert_eq!(draft_of(&app, &reply_key("c1")), Some("left behind".into()));
}

#[test]
fn the_box_closes_when_a_profile_switches_to_a_tab_without_the_parent() {
    let mut app = alice(120, 40, 3);
    open_reply(&mut app);
    write(&mut app, "tabbed");

    app.update(Action::SwitchProfileTab(ProfileTab::Replies));

    assert!(app.compose().is_none());
    assert_eq!(draft_of(&app, &reply_key("p0")), Some("tabbed".into()));
}

#[test]
fn the_box_of_a_thread_closes_when_a_reload_drops_the_reply() {
    let mut app = thread_app();
    key(&mut app, 'j');
    open_reply(&mut app);
    write(&mut app, "gone");
    assert_eq!(compose_key(&app), Some(reply_key("r0")));

    let effects = app.update(Action::Refresh);
    deliver_thread(&mut app, &effects, vec![], 1);
    deliver_replies(&mut app, &effects, MAIN, vec![item("r9", "carol")], None);

    assert!(app.compose().is_none());
    assert_eq!(draft_of(&app, &reply_key("r0")), Some("gone".into()));
}

#[test]
fn the_quote_dialog_stays_when_its_commit_leaves_the_list() {
    let mut app = log_app(3);
    down(&mut app, 1);
    key(&mut app, 'q');
    write(&mut app, "still quoting");
    let effects = app.update(Action::Refresh);

    deliver(&mut app, FOLLOWING, &effects, page(2, 5, None));

    assert_eq!(compose_key(&app), Some(quote_key("c1")));
    assert_eq!(app.modal(), Some(Modal::Composer));
    assert!(
        screen_text(&app).contains("┌ ❝ alice @alice"),
        "{}",
        screen_text(&app)
    );
}

// --- sending a reply (plan 4) ------------------------------------------------------------------

#[test]
fn ctrl_s_sends_the_text_as_a_reply_to_the_parent() {
    let mut app = replying(3, 1, "nice one");

    let effects = submit(&mut app);

    assert!(matches!(
        effects.as_slice(),
        [Effect::CreateCommit { body, parent_id: Some(parent), quoted_commit_id: None, .. }]
            if body == "nice one" && parent == "c1"
    ));
    assert!(app.compose().is_some_and(|c| c.submitting));
    assert_eq!(
        app.in_flight().map(|f| f.key.clone()),
        Some(reply_key("c1"))
    );
    assert!(screen_text(&app).contains("Esc cancel  Committing…"));
}

#[test]
fn ctrl_enter_sends_a_reply_too() {
    let mut app = replying(3, 0, "enter");

    let effects = send(
        &mut app,
        key_event(KeyCode::Enter, KeyModifiers::CONTROL, KeyEventKind::Press),
    );

    assert_eq!(creates(&effects).len(), 1, "{effects:?}");
}

#[test]
fn a_blank_or_oversized_reply_is_not_sent() {
    for text in ["", "  \n ", "\u{200b}"] {
        let mut app = replying(3, 0, text);
        assert!(creates(&submit(&mut app)).is_empty(), "{text:?}");
        assert!(app.compose().is_some_and(|c| !c.submitting), "{text:?}");
    }
    let mut ok = replying(3, 0, "");
    app_paste(&mut ok, &"a".repeat(4000));
    assert_eq!(creates(&submit(&mut ok)).len(), 1);
    let mut over = replying(3, 0, "");
    app_paste(&mut over, &"a".repeat(4001));
    assert!(creates(&submit(&mut over)).is_empty());
}

fn app_paste(app: &mut App, text: &str) {
    app.update(Action::Paste(text.into()));
}

#[test]
fn a_second_submit_of_the_same_reply_sends_nothing_and_says_nothing() {
    let (mut app, _) = sent_reply(1, "once");
    app.update(Action::Tick(NOW + time::Duration::seconds(10)));

    let again = submit(&mut app);

    assert!(creates(&again).is_empty());
    assert_eq!(toast(&app), None);
}

#[test]
fn reopening_the_box_while_its_reply_is_out_shows_it_as_sending() {
    let (mut app, req) = sent_reply(1, "once");
    press(&mut app, KeyCode::Esc);
    assert!(app.compose().is_none());

    open_reply(&mut app);

    assert!(app.compose().is_some_and(|c| c.submitting));
    assert!(screen_text(&app).contains("Esc cancel  Committing…"));
    app.update(created(req, reply_commit("c1", "pending")));
    assert!(app.compose().is_none());
}

#[test]
fn typing_is_ignored_while_the_reply_is_out() {
    let (mut app, _) = sent_reply(1, "once");

    type_str(&mut app, "more");

    assert_eq!(draft_of(&app, &reply_key("c1")), Some("once".into()));
}

// --- one commit at a time --------------------------------------------------------------------

#[test]
fn a_reply_cannot_be_sent_while_a_new_commit_is_out() {
    let mut app = log_app(3);
    key(&mut app, 'c');
    write(&mut app, "new one");
    submit(&mut app);
    press(&mut app, KeyCode::Esc);
    open_reply(&mut app);
    write(&mut app, "my reply");

    let effects = submit(&mut app);

    assert!(creates(&effects).is_empty(), "{effects:?}");
    assert_eq!(
        toast(&app),
        Some((ToastKind::Info, STILL_SENDING.to_string()))
    );
    assert!(app.compose().is_some_and(|c| !c.submitting));
    assert_eq!(compose_key(&app), Some(reply_key("c0")));
    assert_eq!(draft_of(&app, &reply_key("c0")), Some("my reply".into()));
}

#[test]
fn a_new_commit_cannot_be_sent_while_a_reply_is_out() {
    let (mut app, _) = sent_reply(1, "first");
    press(&mut app, KeyCode::Esc);
    app.update(Action::Commit);
    write(&mut app, "second");

    let effects = submit(&mut app);

    assert!(creates(&effects).is_empty(), "{effects:?}");
    assert_eq!(
        toast(&app),
        Some((ToastKind::Info, STILL_SENDING.to_string()))
    );
    assert!(app.compose().is_some_and(|c| !c.submitting));
    assert_eq!(draft_of(&app, &DraftKey::New), Some("second".into()));
}

#[test]
fn a_quote_cannot_be_sent_while_a_reply_is_out_and_goes_out_once_it_is_answered() {
    let (mut app, req) = sent_reply(1, "first");
    press(&mut app, KeyCode::Esc);
    key(&mut app, 'q');
    write(&mut app, "second");
    assert!(creates(&submit(&mut app)).is_empty());
    assert_eq!(
        toast(&app),
        Some((ToastKind::Info, STILL_SENDING.to_string()))
    );

    app.update(created(req, reply_commit("c1", "pending")));
    // The answer for the reply leaves the dialog of the quote alone.
    assert_eq!(compose_key(&app), Some(quote_key("c1")));
    assert_eq!(app.modal(), Some(Modal::Composer));
    assert_eq!(draft_of(&app, &quote_key("c1")), Some("second".into()));
    let effects = submit(&mut app);

    assert!(matches!(
        effects.as_slice(),
        [Effect::CreateCommit { quoted_commit_id: Some(q), parent_id: None, .. }] if q == "c1"
    ));
}

// --- a created reply (plan 4) ------------------------------------------------------------------

#[test]
fn a_created_reply_closes_the_box_clears_the_draft_and_says_so() {
    let (mut app, req) = sent_reply(1, "nice one");

    app.update(created(req, reply_commit("c1", "pending")));

    assert!(app.compose().is_none());
    assert!(!app.composer_focused());
    assert_eq!(draft_of(&app, &reply_key("c1")), None);
    assert_eq!(app.in_flight(), None);
    assert_eq!(
        toast(&app),
        Some((
            ToastKind::Success,
            "Replied a1b2c3d (in review)".to_string()
        ))
    );
}

#[test]
fn the_toast_for_an_approved_reply_does_not_say_in_review() {
    let (mut app, req) = sent_reply(1, "nice one");

    app.update(created(req, reply_commit("c1", "approved")));

    assert_eq!(
        toast(&app),
        Some((ToastKind::Success, "Replied a1b2c3d".to_string()))
    );
}

#[test]
fn a_created_reply_raises_the_count_of_its_parent_and_no_other() {
    let (mut app, req) = sent_reply(1, "nice one");

    app.update(created(req, reply_commit("c1", "pending")));

    assert_eq!(count_in(&app, FOLLOWING, 0), Some(Some(0)));
    assert_eq!(count_in(&app, FOLLOWING, 1), Some(Some(1)));
    assert_eq!(count_in(&app, FOLLOWING, 2), Some(Some(0)));
}

#[test]
fn a_created_reply_does_not_enter_the_log_lists() {
    let mut app = log_app(3);
    let fetch = type_str(&mut app, "2");
    deliver(&mut app, LATEST, &fetch, page(0, 3, None));
    type_str(&mut app, "1");
    open_reply(&mut app);
    write(&mut app, "nice one");
    let req = create_req(&submit(&mut app));

    app.update(created(req, reply_commit("c0", "pending")));

    assert_eq!(ids(&app, FOLLOWING), ["c0", "c1", "c2"]);
    assert_eq!(ids(&app, LATEST), ["c0", "c1", "c2"]);
}

#[test]
fn a_created_reply_raises_the_count_in_every_copy_of_the_parent() {
    let mut app = log_app(3);
    let fetch = type_str(&mut app, "2");
    deliver(&mut app, LATEST, &fetch, page(0, 3, None));
    let fetch = type_str(&mut app, "3");
    deliver(&mut app, TRENDING, &fetch, page(0, 3, None));
    let fetch = app.update(Action::Goto(Screen::Saved));
    deliver(&mut app, FeedKey::Saved, &fetch, page(0, 3, None));
    let effects = open_profile(&mut app, "alice");
    deliver_user(
        &mut app,
        &effects,
        "alice",
        user_resp(
            "alice",
            json!({"is_following": false, "pinned_commit": commit_json("c1", "alice", 120)}),
        ),
    );
    let profile_page = vec![
        commit_of(commit_json("c0", "alice", 60)),
        commit_of(commit_json("c1", "alice", 120)),
    ];
    deliver_commits(
        &mut app,
        &effects,
        "alice",
        ProfileTab::Commits,
        profile_page,
        None,
    );
    let effects = key(&mut app, '2');
    deliver_commits(
        &mut app,
        &effects,
        "alice",
        ProfileTab::Replies,
        vec![commit_of(commit_json("c1", "alice", 120))],
        None,
    );
    key(&mut app, '1');
    app.update(Action::Open(Screen::Log(LogTab::Following)));
    down(&mut app, 1);
    open_reply(&mut app);
    write(&mut app, "nice one");
    let req = create_req(&submit(&mut app));

    app.update(created(req, reply_commit("c1", "pending")));

    let profile = app.profile("alice").cloned().unwrap_or_default();
    assert_eq!(
        profile.replies.items.first().map(|c| c.replies_count),
        Some(Some(1)),
        "replies tab"
    );
    for feed in [FOLLOWING, LATEST, TRENDING, FeedKey::Saved] {
        assert_eq!(count_in(&app, feed, 1), Some(Some(1)), "{feed:?}");
        assert_eq!(count_in(&app, feed, 0), Some(Some(0)), "{feed:?}");
    }
    let profile = app.profile("alice").cloned().unwrap_or_default();
    assert_eq!(
        profile.commits.items.get(1).map(|c| c.replies_count),
        Some(Some(1)),
        "profile list"
    );
    assert_eq!(
        profile.commits.items.first().map(|c| c.replies_count),
        Some(Some(0)),
        "profile list"
    );
    assert_eq!(
        profile
            .user
            .as_ref()
            .and_then(|u| u.pinned_commit.as_ref())
            .map(|c| c.replies_count),
        Some(Some(1)),
        "pinned"
    );
}

#[test]
fn a_count_that_was_not_loaded_stays_not_loaded() {
    let (mut app, effects) = started(120, 40);
    let mut bare = commit_of(commit_json("c0", "alice", 60));
    bare.replies_count = None;
    deliver(&mut app, FOLLOWING, &effects, feed_page(vec![bare]));
    open_reply(&mut app);
    write(&mut app, "nice one");
    let req = create_req(&submit(&mut app));

    app.update(created(req, reply_commit("c0", "pending")));

    assert_eq!(count_in(&app, FOLLOWING, 0), Some(None));
}

#[test]
fn a_reply_that_is_answered_after_esc_still_counts_and_clears_its_draft() {
    let (mut app, req) = sent_reply(1, "nice one");
    press(&mut app, KeyCode::Esc);

    app.update(created(req, reply_commit("c1", "pending")));

    assert!(app.compose().is_none());
    assert_eq!(draft_of(&app, &reply_key("c1")), None);
    assert_eq!(count_in(&app, FOLLOWING, 1), Some(Some(1)));
    assert_eq!(
        toast(&app),
        Some((
            ToastKind::Success,
            "Replied a1b2c3d (in review)".to_string()
        ))
    );
}

#[test]
fn a_reply_to_my_own_pending_commit_works_like_any_other() {
    let (mut app, effects) = started(120, 40);
    let own = commit_of(merge(
        commit_json("own1", "myuron", 30),
        json!({"status": "pending", "short_hash": "0wn0001"}),
    ));
    deliver(&mut app, FOLLOWING, &effects, feed_page(vec![own]));

    open_reply(&mut app);
    write(&mut app, "talking to myself");
    let sent = submit(&mut app);
    let req = create_req(&sent);

    assert!(matches!(
        sent.as_slice(),
        [Effect::CreateCommit { parent_id: Some(p), .. }] if p == "own1"
    ));
    app.update(created(req, reply_commit("own1", "pending")));
    assert_eq!(count_in(&app, FOLLOWING, 0), Some(Some(1)));
}

#[test]
fn a_failed_reply_keeps_the_box_open_with_the_draft_and_the_reason() {
    let (mut app, req) = sent_reply(1, "will fail");

    app.update(failed(
        req,
        Failure {
            message: Some("The body field is required.".into()),
            ..Failure::new("validation_failed")
        },
    ));

    assert_eq!(compose_key(&app), Some(reply_key("c1")));
    assert!(app.compose().is_some_and(|c| !c.submitting));
    assert_eq!(
        app.compose().and_then(|c| c.error.clone()),
        Some("The body field is required.".to_string())
    );
    assert_eq!(draft_of(&app, &reply_key("c1")), Some("will fail".into()));
    assert_eq!(
        toast(&app),
        Some((ToastKind::Error, "The body field is required.".to_string()))
    );
    assert_eq!(count_in(&app, FOLLOWING, 1), Some(Some(0)));
    assert!(screen_text(&app).contains("✗ The body field is required."));
}

#[test]
fn a_failed_reply_can_be_sent_again() {
    let (mut app, req) = sent_reply(1, "retry me");
    app.update(failed(req, Failure::new("rate_limited")));

    let effects = submit(&mut app);

    assert_eq!(creates(&effects).len(), 1, "{effects:?}");
}

#[test]
fn a_rejected_device_closes_the_box() {
    let (mut app, req) = sent_reply(1, "lost");

    app.update(failed(req, Failure::new(NOT_APPROVED)));

    assert_eq!(app.session(), &Session::Guest);
    assert!(app.compose().is_none());
    assert!(app.drafts().is_empty());
}

#[test]
fn a_reply_answered_after_the_return_to_guest_only_shows_the_toast() {
    let (mut app, req) = sent_reply(1, "late");
    app.update(failed(
        ReqId(req.0.saturating_add(100)),
        Failure::new(NOT_APPROVED),
    ));
    assert_eq!(app.session(), &Session::Guest);

    app.update(created(req, reply_commit("c1", "pending")));

    assert_eq!(ids(&app, FOLLOWING), Vec::<String>::new());
    assert_eq!(
        toast(&app),
        Some((
            ToastKind::Success,
            "Replied a1b2c3d (in review)".to_string()
        ))
    );
}

// --- a created reply in a thread -----------------------------------------------------------

/// Thread of `MAIN` with `r0`, `r1` loaded; replying to `parent` is sent from the open box.
fn thread_reply_sent(app: &mut App, id_selected: usize, text: &str) -> ReqId {
    down(app, id_selected);
    open_reply(app);
    write(app, text);
    create_req(&submit(app))
}

#[test]
fn a_reply_to_the_main_commit_joins_the_replies_and_raises_the_count() {
    let mut app = thread_app();
    let req = thread_reply_sent(&mut app, 0, "hello");
    assert_eq!(compose_key(&app), Some(reply_key(MAIN)));

    app.update(created(req, reply_commit(MAIN, "pending")));

    assert_eq!(thread_children(&app, MAIN), ["r0", "r1", "n1"]);
    assert_eq!(
        state(&app, MAIN).main.map(|m| m.replies_count),
        Some(Some(3))
    );
    assert!(app.compose().is_none());
    assert_eq!(draft_of(&app, &reply_key(MAIN)), None);
}

#[test]
fn a_reply_is_not_added_twice() {
    let mut app = thread_app();
    let req = thread_reply_sent(&mut app, 0, "hello");
    app.update(created(req, reply_commit(MAIN, "pending")));

    app.update(created(req, reply_commit(MAIN, "pending")));

    assert_eq!(thread_children(&app, MAIN), ["r0", "r1", "n1"]);
}

#[test]
fn the_reply_only_counts_when_more_pages_of_replies_follow() {
    let mut app = opened(120, 40, vec![], 5, others(0, 2), Some("next"));
    let req = thread_reply_sent(&mut app, 0, "hello");

    app.update(created(req, reply_commit(MAIN, "pending")));

    assert_eq!(thread_children(&app, MAIN), ["r0", "r1"]);
    assert_eq!(
        state(&app, MAIN).main.map(|m| m.replies_count),
        Some(Some(6))
    );
}

#[test]
fn a_reply_to_an_opened_reply_joins_its_replies_and_raises_its_count() {
    let mut app = thread_app();
    key(&mut app, 'j');
    let effects = key(&mut app, 'o');
    deliver_replies(
        &mut app,
        &effects,
        "r0",
        vec![child("k1", "carol", "r0")],
        None,
    );
    open_reply(&mut app);
    write(&mut app, "deeper");
    let req = create_req(&submit(&mut app));
    assert_eq!(compose_key(&app), Some(reply_key("r0")));

    app.update(created(req, reply_commit("r0", "pending")));

    assert_eq!(thread_children(&app, "r0"), ["k1", "n1"]);
    let first = state(&app, MAIN)
        .children
        .get(MAIN)
        .and_then(|l| l.items.first().cloned());
    assert_eq!(first.as_ref().and_then(replies_count_of), Some(2));
}

#[test]
fn a_reply_to_a_reply_whose_replies_were_never_loaded_only_raises_its_count() {
    let mut app = thread_app();
    let req = thread_reply_sent(&mut app, 2, "to r1");
    assert_eq!(compose_key(&app), Some(reply_key("r1")));

    app.update(created(req, reply_commit("r1", "pending")));

    assert!(!state(&app, MAIN).children.contains_key("r1"));
    let items = state(&app, MAIN)
        .children
        .get(MAIN)
        .map(|l| l.items.clone())
        .unwrap_or_default();
    assert_eq!(items.get(1).and_then(replies_count_of), Some(2));
    assert_eq!(items.first().and_then(replies_count_of), Some(1));
}

#[test]
fn a_reply_to_an_ancestor_raises_the_count_of_the_ancestor() {
    let mut app = opened(
        120,
        40,
        vec![ancestor("a1", "carol")],
        2,
        others(0, 2),
        None,
    );
    key(&mut app, 'k');
    open_reply(&mut app);
    write(&mut app, "up");
    let req = create_req(&submit(&mut app));
    assert_eq!(compose_key(&app), Some(reply_key("a1")));

    app.update(created(req, reply_commit("a1", "pending")));

    let ancestors = state(&app, MAIN).ancestors;
    assert_eq!(ancestors.first().and_then(replies_count_of), Some(1));
    assert_eq!(
        state(&app, MAIN).main.map(|m| m.replies_count),
        Some(Some(2))
    );
}

#[test]
fn the_selected_card_of_a_thread_stays_selected_when_a_reply_joins_above_it() {
    let mut app = thread_app();
    key(&mut app, 'j');
    let effects = key(&mut app, 'o');
    deliver_replies(
        &mut app,
        &effects,
        "r0",
        vec![child("k1", "carol", "r0")],
        None,
    );
    open_reply(&mut app);
    write(&mut app, "deeper");
    let req = create_req(&submit(&mut app));
    press(&mut app, KeyCode::Esc);
    // Entries: main, r0, k1, r1.
    down(&mut app, 2);
    assert!(matches!(thread_selected_entry(&app), Some(Entry::Reply(id, _)) if id == "r1"));

    app.update(created(req, reply_commit("r0", "pending")));

    assert!(
        matches!(thread_selected_entry(&app), Some(Entry::Reply(id, _)) if id == "r1"),
        "{:?}",
        thread_selected_entry(&app)
    );
}

#[test]
fn a_reply_from_a_profile_card_counts_in_the_profile_list() {
    let mut app = alice(120, 40, 3);
    down(&mut app, 1);
    open_reply(&mut app);
    write(&mut app, "hi");
    let req = create_req(&submit(&mut app));

    app.update(created(req, reply_commit("p1", "pending")));

    let profile = app.profile("alice").cloned().unwrap_or_default();
    assert_eq!(
        profile.commits.items.get(1).map(|c| c.replies_count),
        Some(Some(1))
    );
    assert_eq!(
        profile.commits.items.first().map(|c| c.replies_count),
        Some(Some(0))
    );
}

#[test]
fn a_reply_from_a_saved_card_counts_in_the_saved_list() {
    let mut app = saved_app(3);
    open_reply(&mut app);
    write(&mut app, "hi");
    let req = create_req(&submit(&mut app));

    app.update(created(req, reply_commit("c0", "pending")));

    assert_eq!(count_in(&app, FeedKey::Saved, 0), Some(Some(1)));
}

// --- quote (plan 5) ------------------------------------------------------------------------

#[test]
fn q_opens_the_quote_dialog_for_the_selected_card() {
    let mut app = log_app(3);
    down(&mut app, 1);

    let effects = key(&mut app, 'q');

    assert_eq!(compose_key(&app), Some(quote_key("c1")));
    assert_eq!(compose_surface(&app), Some(Surface::Modal));
    assert_eq!(app.modal(), Some(Modal::Composer));
    assert!(app.composer_focused());
    assert!(effects.is_empty(), "{effects:?}");
    let text = screen_text(&app);
    assert!(text.contains("╔ Quote @alice "), "{text}");
    assert!(text.contains("┌ ❝ alice @alice"), "{text}");
}

#[test]
fn q_opens_the_dialog_on_saved_profile_and_thread_cards() {
    let mut saved = saved_app(3);
    key(&mut saved, 'q');
    assert_eq!(compose_key(&saved), Some(quote_key("c0")), "saved");
    assert_eq!(saved.modal(), Some(Modal::Composer), "saved");

    let mut profile = alice(120, 40, 3);
    key(&mut profile, 'q');
    assert_eq!(compose_key(&profile), Some(quote_key("p0")), "profile");

    let mut thread = thread_app();
    key(&mut thread, 'j');
    key(&mut thread, 'q');
    assert_eq!(compose_key(&thread), Some(quote_key("r0")), "thread");
    assert_eq!(thread.modal(), Some(Modal::Composer), "thread");
}

#[test]
fn a_quote_draft_is_kept_per_commit_apart_from_the_new_one() {
    let mut app = log_app(3);
    key(&mut app, 'c');
    write(&mut app, "new text");
    press(&mut app, KeyCode::Esc);
    key(&mut app, 'q');
    write(&mut app, "quote text");

    assert_eq!(draft_of(&app, &DraftKey::New), Some("new text".into()));
    assert_eq!(draft_of(&app, &quote_key("c0")), Some("quote text".into()));
}

#[test]
fn a_quote_draft_comes_back_when_the_dialog_is_opened_again() {
    let mut app = log_app(3);
    down(&mut app, 1);
    key(&mut app, 'q');
    write(&mut app, "first\nsecond");
    press(&mut app, KeyCode::Esc);
    assert!(app.compose().is_none());
    assert_eq!(app.modal(), None);

    key(&mut app, 'q');
    type_str(&mut app, "!");

    assert_eq!(
        draft_of(&app, &quote_key("c1")),
        Some("first\nsecond!".into())
    );
    assert!(screen_text(&app).contains("stash@{0} draft saved"));
}

#[test]
fn ctrl_s_sends_the_text_as_a_quote_of_the_commit() {
    let mut app = log_app(3);
    down(&mut app, 2);
    key(&mut app, 'q');
    write(&mut app, "quoting");

    let effects = submit(&mut app);

    assert!(matches!(
        effects.as_slice(),
        [Effect::CreateCommit { body, parent_id: None, quoted_commit_id: Some(q), .. }]
            if body == "quoting" && q == "c2"
    ));
    assert!(app.compose().is_some_and(|c| c.submitting));
    assert!(screen_text(&app).contains("Esc cancel  Committing…"));
}

#[test]
fn a_created_quote_goes_to_the_head_of_following_and_latest() {
    let mut app = log_app(3);
    let fetch = type_str(&mut app, "2");
    deliver(&mut app, LATEST, &fetch, page(0, 3, None));
    type_str(&mut app, "1");
    down(&mut app, 1);
    key(&mut app, 'q');
    write(&mut app, "quoting");
    let req = create_req(&submit(&mut app));

    app.update(created(req, quote_commit("c1", "pending")));

    assert_eq!(ids(&app, FOLLOWING), ["n1", "c0", "c1", "c2"]);
    assert_eq!(ids(&app, LATEST), ["n1", "c0", "c1", "c2"]);
    assert_eq!(app.feed(FOLLOWING).selected, 2);
    assert_eq!(count_in(&app, FOLLOWING, 2), Some(Some(0)));
    assert!(app.compose().is_none());
    assert_eq!(app.modal(), None);
    assert_eq!(draft_of(&app, &quote_key("c1")), None);
    assert_eq!(
        toast(&app),
        Some((ToastKind::Success, "Quoted a1b2c3d (in review)".to_string()))
    );
}

#[test]
fn the_toast_for_an_approved_quote_does_not_say_in_review() {
    let mut app = log_app(3);
    key(&mut app, 'q');
    write(&mut app, "quoting");
    let req = create_req(&submit(&mut app));

    app.update(created(req, quote_commit("c0", "approved")));

    assert_eq!(
        toast(&app),
        Some((ToastKind::Success, "Quoted a1b2c3d".to_string()))
    );
}

#[test]
fn a_failed_quote_keeps_the_dialog_open_with_the_draft_and_the_reason() {
    let mut app = log_app(3);
    key(&mut app, 'q');
    write(&mut app, "quoting");
    let req = create_req(&submit(&mut app));

    app.update(failed(req, Failure::new("rate_limited")));

    assert_eq!(compose_key(&app), Some(quote_key("c0")));
    assert_eq!(app.modal(), Some(Modal::Composer));
    assert!(app.compose().is_some_and(|c| !c.submitting));
    assert_eq!(draft_of(&app, &quote_key("c0")), Some("quoting".into()));
    assert!(app.compose().is_some_and(|c| c.error.is_some()));
}

#[test]
fn a_quote_of_my_own_pending_commit_works() {
    let (mut app, effects) = started(120, 40);
    let own = commit_of(merge(
        commit_json("own1", "myuron", 30),
        json!({"status": "pending"}),
    ));
    deliver(&mut app, FOLLOWING, &effects, feed_page(vec![own]));

    key(&mut app, 'q');
    write(&mut app, "mine");
    let sent = submit(&mut app);

    assert_eq!(compose_key(&app), Some(quote_key("own1")));
    assert!(matches!(
        sent.as_slice(),
        [Effect::CreateCommit { quoted_commit_id: Some(q), .. }] if q == "own1"
    ));
}

// --- layout and scrolling (plan 8) -------------------------------------------------------------

#[test]
fn the_box_is_one_text_row_when_empty_and_grows_to_eight() {
    let mut app = log_app(3);
    open_reply(&mut app);
    let (top, bottom) = box_rows(&app).unwrap_or((0, 0));
    assert_eq!(bottom.saturating_sub(top), 3, "{}", screen_text(&app));

    write(&mut app, "l1\nl2\nl3");
    let (top, bottom) = box_rows(&app).unwrap_or((0, 0));
    assert_eq!(bottom.saturating_sub(top), 5, "{}", screen_text(&app));

    write(&mut app, "\nl4\nl5\nl6\nl7\nl8\nl9\nl10\nl11\nl12");
    let (top, bottom) = box_rows(&app).unwrap_or((0, 0));
    assert_eq!(bottom.saturating_sub(top), 10, "{}", screen_text(&app));
    let (_, y) = cursor_of(&app);
    assert!(
        screen_rows(&app)[usize::from(y)].contains("l12"),
        "{}",
        screen_text(&app)
    );
}

#[test]
fn a_long_line_wraps_inside_the_box() {
    let mut app = log_app(3);
    open_reply(&mut app);

    write(&mut app, &"w".repeat(59));

    let (top, bottom) = box_rows(&app).unwrap_or((0, 0));
    assert_eq!(bottom.saturating_sub(top), 4, "{}", screen_text(&app));
}

#[test]
fn the_text_width_of_the_box_follows_the_card() {
    let mut log = log_app(3);
    open_reply(&mut log);
    assert_eq!(compose_text_width(&log, Surface::ReplyBox), 58);

    let mut narrow = {
        let (mut app, effects) = started(60, 30);
        deliver(&mut app, FOLLOWING, &effects, page(0, 3, None));
        app
    };
    open_reply(&mut narrow);
    assert_eq!(compose_text_width(&narrow, Surface::ReplyBox), 50);

    let mut thread = thread_app();
    key(&mut thread, 'j');
    open_reply(&mut thread);
    assert_eq!(compose_text_width(&thread, Surface::ReplyBox), 54);
}

#[test]
fn the_footer_of_the_box_shows_the_counter_and_the_keys_of_the_keyboard() {
    let mut app = log_app(3);
    open_reply(&mut app);
    app_paste(&mut app, &"a".repeat(3850));

    let rows = screen_rows(&app);
    let footer = rows
        .iter()
        .find(|r| r.contains("Esc cancel"))
        .cloned()
        .unwrap_or_default();

    assert!(
        footer.contains("150  Esc cancel  Ctrl+S Reply ║"),
        "{footer}"
    );
    app.set_keyboard(KeyboardMode { enhanced: true });
    assert!(screen_text(&app).contains("Esc cancel  Ctrl+Enter Reply"));
}

#[test]
fn the_quote_dialog_names_its_action_quote() {
    let mut app = log_app(3);
    key(&mut app, 'q');

    assert!(
        screen_text(&app).contains("Esc cancel  Ctrl+S Quote"),
        "{}",
        screen_text(&app)
    );
    app.set_keyboard(KeyboardMode { enhanced: true });
    assert!(screen_text(&app).contains("Esc cancel  Ctrl+Enter Quote"));
}

#[test]
fn a_box_on_a_card_near_the_bottom_scrolls_into_view_whole() {
    let mut app = log_app(9);
    down(&mut app, 7);

    open_reply(&mut app);

    let rows = screen_rows(&app);
    let (top, bottom) = box_rows(&app).unwrap_or((0, 0));
    assert!(bottom > top, "{}", rows.join("\n"));
    assert!(
        rows.iter().any(|r| r.contains("0000007")),
        "card header: {}",
        rows.join("\n")
    );
    let (_, y) = cursor_of(&app);
    assert!(
        top < usize::from(y) && usize::from(y) < bottom,
        "cursor {y} in {top}..{bottom}"
    );
}

#[test]
fn typing_more_rows_keeps_the_box_and_cursor_in_view() {
    let mut app = log_app(9);
    down(&mut app, 8);
    open_reply(&mut app);

    write(&mut app, "a\nb\nc\nd\ne\nf\ng\nlast row");

    let (top, bottom) = box_rows(&app).unwrap_or((0, 0));
    assert!(bottom > top, "{}", screen_text(&app));
    let (_, y) = cursor_of(&app);
    assert!(
        screen_rows(&app)[usize::from(y)].contains("last row"),
        "{}",
        screen_text(&app)
    );
}

#[test]
fn a_card_and_box_taller_than_the_screen_keep_the_cursor_row_in_view() {
    let (mut app, effects) = started(120, 24);
    let body = (1..=12)
        .map(|i| format!("body line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let tall = commit_of(merge(
        commit_json("t0", "alice", 60),
        json!({"body": body, "body_segments": [{"type": "text", "text": body}]}),
    ));
    deliver(&mut app, FOLLOWING, &effects, feed_page(vec![tall]));
    open_reply(&mut app);

    write(&mut app, "r1\nr2\nr3\nr4\nr5\nr6\nr7\ncursor row");

    let rows = screen_rows(&app);
    let (x, y) = cursor_of(&app);
    assert!(
        rows[usize::from(y)].contains("cursor row"),
        "({x}, {y})\n{}",
        rows.join("\n")
    );
}

#[test]
fn a_box_in_a_long_thread_scrolls_into_view() {
    let mut app = opened(120, 24, vec![], 12, others(0, 12), None);
    press(&mut app, KeyCode::Char('G'));

    open_reply(&mut app);

    let (top, bottom) = box_rows(&app).unwrap_or((0, 0));
    assert!(bottom > top, "{}", screen_text(&app));
    let (_, y) = cursor_of(&app);
    assert!(top < usize::from(y) && usize::from(y) < bottom);
}

#[test]
fn the_box_of_a_thread_sits_under_the_actions_of_a_reply_that_has_replies() {
    let mut app = thread_app();
    key(&mut app, 'j');
    open_reply(&mut app);
    write(&mut app, "x");

    let (top, bottom) = box_rows(&app).unwrap_or((0, 0));
    let rows = screen_rows(&app);
    let body = rows
        .iter()
        .position(|r| r.contains("body of r0"))
        .unwrap_or(0);
    // The actions row follows the body; the box follows the actions, and the hint stays below the box.
    assert_eq!(top, body + 2, "{}", rows.join("\n"));
    assert_eq!(bottom - top, 3);
    assert!(
        rows[bottom + 1].contains("┊ 1 reply (o)"),
        "{}",
        rows.join("\n")
    );
    // The rail of the card, which has children, continues to the left of the box.
    assert!(
        rows[top].contains("│ │   ╔ Reply to @bob"),
        "{}",
        rows.join("\n")
    );
    assert!(rows[top + 1].contains("│ │   ║ x"), "{}", rows.join("\n"));
    assert_eq!(cursor_of(&app).1 as usize, top + 1);
}

// --- review follow-ups: the copies in a cached thread ---------------------------------------------

/// Following holds `MAIN` and `c1`; the thread of `MAIN` was opened from the list and stays cached, because the
/// history reaches it, while the Log is opened on top of it. The screen is the thread.
fn thread_over_log() -> App {
    let (mut app, effects) = started(120, 40);
    let items = vec![
        commit_of(commit_json(MAIN, "alice", 3600)),
        commit_of(commit_json("c1", "alice", 120)),
    ];
    deliver(&mut app, FOLLOWING, &effects, feed_page(items));
    let opened = press(&mut app, KeyCode::Enter);
    assert_eq!(app.screen(), Screen::Thread(MAIN.into()));
    deliver_thread(&mut app, &opened, vec![], 2);
    deliver_replies(&mut app, &opened, MAIN, others(0, 2), None);
    app
}

#[test]
fn a_reply_from_the_log_raises_the_count_of_the_cached_thread_and_joins_its_replies() {
    let mut app = thread_over_log();
    app.update(Action::Open(Screen::Log(LogTab::Following)));
    assert_eq!(app.screen(), Screen::Log(LogTab::Following));
    open_reply(&mut app);
    write(&mut app, "from the log");
    let req = create_req(&submit(&mut app));
    assert_eq!(compose_key(&app), Some(reply_key(MAIN)));

    app.update(created(req, reply_commit(MAIN, "pending")));

    assert_eq!(count_in(&app, FOLLOWING, 0), Some(Some(1)), "log copy");
    assert_eq!(count_in(&app, FOLLOWING, 1), Some(Some(0)));
    assert_eq!(
        state(&app, MAIN).main.map(|m| m.replies_count),
        Some(Some(3))
    );
    assert_eq!(thread_children(&app, MAIN), ["r0", "r1", "n1"]);
}

#[test]
fn a_reply_from_the_thread_raises_the_count_of_the_card_in_the_log() {
    let mut app = thread_over_log();
    open_reply(&mut app);
    write(&mut app, "from the thread");
    let req = create_req(&submit(&mut app));

    app.update(created(req, reply_commit(MAIN, "pending")));

    assert_eq!(count_in(&app, FOLLOWING, 0), Some(Some(1)), "log copy");
    assert_eq!(count_in(&app, FOLLOWING, 1), Some(Some(0)));
    assert_eq!(
        state(&app, MAIN).main.map(|m| m.replies_count),
        Some(Some(3))
    );
}

// --- review follow-ups: boundaries and combinations -------------------------------------------------

fn pkce() -> crate::api::pkce::Pkce {
    crate::api::pkce::Pkce {
        verifier: "verifier".into(),
        challenge: "challenge".into(),
    }
}

/// An account that sent a reply to `c1`, was refused as a device and signed in again. Returns the request the
/// reply went out with, which is older than the new sign-in.
fn signed_in_again_after_a_reply() -> (App, ReqId) {
    let me = crate::app::Me {
        id: ACCOUNT.into(),
        ..fixture_me()
    };
    let mut app = App::with_key_status(
        Session::SignedIn(me),
        NOW,
        UtcOffset::UTC,
        crate::app::KeyStatus::Present,
        "/home/user/.config/lazydn/id_ed25519".into(),
    );
    let mut effects = app.update(Action::Init);
    effects.extend(app.update(Action::Resize(120, 40)));
    deliver(&mut app, FOLLOWING, &effects, page(0, 3, None));
    down(&mut app, 1);
    open_reply(&mut app);
    write(&mut app, "first");
    let req = create_req(&submit(&mut app));
    assert!(app.compose().is_some_and(|c| c.submitting));
    app.update(Action::FeedFailed {
        key: FOLLOWING,
        req: ReqId(1),
        failure: Failure::new(NOT_APPROVED),
    });
    app.update(Action::SignInPrepared {
        fingerprint: "SHA256:abc".into(),
        url: "https://dev-null.sh/mobile/authorize".into(),
        pkce: pkce(),
        state: format!("{:032x}", 1),
        created: false,
    });
    type_str(&mut app, &"0123456789abcdef".repeat(4));
    press(&mut app, KeyCode::Enter);
    let response = serde_json::from_value(crate::notifications_tests::me_json(
        0,
        json!({"id": ACCOUNT}),
    ))
    .unwrap();
    let signed = app.update(Action::DeviceRegistered {
        response: Box::new(response),
    });
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
        .unwrap_or(ReqId(u64::MAX));
    app.update(Action::FeedLoaded {
        key: FOLLOWING,
        req: fetch,
        page: page(0, 3, None),
    });
    (app, req)
}

#[test]
fn a_reply_from_before_the_new_sign_in_changes_neither_the_session_nor_a_quote_that_is_out() {
    let (mut app, old) = signed_in_again_after_a_reply();
    assert!(matches!(app.session(), Session::SignedIn(_)));
    key(&mut app, 'q');
    write(&mut app, "second");
    let sent = submit(&mut app);
    assert_eq!(creates(&sent).len(), 1, "{sent:?}");
    assert_ne!(
        toast(&app),
        Some((ToastKind::Info, STILL_SENDING.to_string()))
    );
    let new = create_req(&sent);

    app.update(created(old, reply_commit("c1", "pending")));

    assert_eq!(
        toast(&app),
        Some((
            ToastKind::Success,
            "Replied a1b2c3d (in review)".to_string()
        ))
    );
    assert!(app.compose().is_some_and(|c| c.submitting));
    assert_eq!(compose_key(&app), Some(quote_key("c0")));
    assert_eq!(app.in_flight().map(|f| f.req), Some(new));
    assert_eq!(draft_of(&app, &quote_key("c0")), Some("second".into()));
    assert_eq!(count_in(&app, FOLLOWING, 0), Some(Some(0)));
    assert_eq!(count_in(&app, FOLLOWING, 1), Some(Some(0)));
}

#[test]
fn a_reply_answered_while_another_box_is_open_leaves_that_box_and_its_text() {
    let (mut app, req) = sent_reply(1, "first");
    press(&mut app, KeyCode::Esc);
    key(&mut app, 'j');
    open_reply(&mut app);
    write(&mut app, "second");
    assert_eq!(compose_key(&app), Some(reply_key("c2")));
    assert!(creates(&submit(&mut app)).is_empty());
    assert_eq!(
        toast(&app),
        Some((ToastKind::Info, STILL_SENDING.to_string()))
    );

    app.update(created(req, reply_commit("c1", "pending")));

    assert_eq!(compose_key(&app), Some(reply_key("c2")));
    assert!(app.compose().is_some_and(|c| !c.submitting));
    assert_eq!(draft_of(&app, &reply_key("c2")), Some("second".into()));
    assert_eq!(count_in(&app, FOLLOWING, 1), Some(Some(1)));
    let again = submit(&mut app);
    assert!(matches!(
        again.as_slice(),
        [Effect::CreateCommit { parent_id: Some(p), .. }] if p == "c2"
    ));
}

/// The sent text is a new commit that joins the head while a box is open under the old first card.
#[test]
fn a_commit_joining_the_head_leaves_the_open_box_on_its_parent_in_view() {
    let mut app = log_app(3);
    key(&mut app, 'c');
    write(&mut app, "new one");
    let req = create_req(&submit(&mut app));
    press(&mut app, KeyCode::Esc);
    open_reply(&mut app);
    write(&mut app, "re");
    assert_eq!(compose_key(&app), Some(reply_key("c0")));
    let fresh = commit_of(merge(
        commit_json("n1", "myuron", 5),
        json!({"short_hash": "9999999", "status": "pending"}),
    ));

    app.update(created(req, fresh));

    assert_eq!(ids(&app, FOLLOWING), ["n1", "c0", "c1", "c2"]);
    assert_eq!(compose_key(&app), Some(reply_key("c0")));
    assert_eq!(compose_surface(&app), Some(Surface::ReplyBox));
    assert_eq!(draft_of(&app, &reply_key("c0")), Some("re".into()));
    let rows = screen_rows(&app);
    let (top, bottom) = box_rows(&app).unwrap_or((0, 0));
    assert!(bottom > top, "{}", rows.join("\n"));
    let header_of = |needle: &str| {
        rows.iter()
            .position(|r| r.contains(needle) && !r.contains("Committed"))
    };
    let new_card = header_of("9999999");
    let parent_card = header_of("0000000");
    assert!(new_card.is_some(), "{}", rows.join("\n"));
    assert!(parent_card.is_some_and(|p| p < top), "{}", rows.join("\n"));
    // The selection follows the parent of the open box, even from index 0.
    assert_eq!(app.feed(FOLLOWING).selected, 1);
    assert!(
        parent_card.is_some_and(|p| rows[p].contains("┃▌")),
        "{}",
        rows.join("\n")
    );
    assert!(
        new_card.is_some_and(|n| !rows[n].contains('▌')),
        "{}",
        rows.join("\n")
    );
    let (_, y) = cursor_of(&app);
    assert!(top < usize::from(y) && usize::from(y) < bottom);
}

// --- review follow-ups: drafts on disk ------------------------------------------------------------------

fn saves(effects: &[Effect]) -> Vec<(String, Drafts)> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::SaveDrafts { account, drafts } => Some((account.clone(), drafts.clone())),
            _ => None,
        })
        .collect()
}

#[test]
fn closing_a_box_writes_its_draft_on_the_spot() {
    let mut app = account_log(3);
    down(&mut app, 1);
    open_reply(&mut app);
    write(&mut app, "abc");

    let effects = press(&mut app, KeyCode::Esc);

    let written = saves(&effects);
    assert_eq!(written.len(), 1, "{effects:?}");
    assert_eq!(written[0].0, ACCOUNT);
    assert_eq!(written[0].1.text(&reply_key("c1")), Some("abc".into()));
}

#[test]
fn a_created_reply_writes_the_drafts_without_its_own_on_the_spot() {
    let mut app = account_log(3);
    down(&mut app, 1);
    open_reply(&mut app);
    write(&mut app, "abc");
    let req = create_req(&submit(&mut app));

    let effects = app.update(created(req, reply_commit("c1", "pending")));

    let written = saves(&effects);
    assert_eq!(written.len(), 1, "{effects:?}");
    assert_eq!(written[0].1.text(&reply_key("c1")), None);
    assert_eq!(written[0].1.get(&reply_key("c1")), None);
}

#[test]
fn a_quote_writes_its_draft_when_closed_and_drops_it_when_created() {
    let mut app = account_log(3);
    down(&mut app, 1);
    key(&mut app, 'q');
    write(&mut app, "quoting");
    let closed = press(&mut app, KeyCode::Esc);
    let written = saves(&closed);
    assert_eq!(written.len(), 1, "{closed:?}");
    assert_eq!(written[0].1.text(&quote_key("c1")), Some("quoting".into()));

    key(&mut app, 'q');
    let req = create_req(&submit(&mut app));
    let effects = app.update(created(req, quote_commit("c1", "pending")));

    let written = saves(&effects);
    assert_eq!(written.len(), 1, "{effects:?}");
    assert_eq!(written[0].1.get(&quote_key("c1")), None);
}

#[test]
fn text_typed_in_a_box_is_written_after_the_two_second_delay() {
    let mut app = account_log(3);
    open_reply(&mut app);
    write(&mut app, "abc");

    let early = app.update(Action::Tick(NOW + time::Duration::seconds(1)));
    let due = app.update(Action::Tick(NOW + time::Duration::seconds(2)));

    assert!(saves(&early).is_empty(), "{early:?}");
    let written = saves(&due);
    assert_eq!(written.len(), 1, "{due:?}");
    assert_eq!(written[0].1.text(&reply_key("c0")), Some("abc".into()));
}

#[test]
fn a_reply_draft_read_from_disk_comes_into_the_box() {
    let mut app = account_log(3);
    load_drafts(&mut app, &[(reply_key("c1"), "from disk")]);
    down(&mut app, 1);

    open_reply(&mut app);
    type_str(&mut app, "!");

    assert_eq!(
        draft_of(&app, &reply_key("c1")),
        Some("from disk!".to_string())
    );
}

// --- review follow-ups: the last row, low and narrow screens, deeper cards ---------------------------------

#[test]
fn a_narrow_footer_keeps_the_keys_then_the_counter_and_cuts_the_error() {
    let (mut app, effects) = started(60, 30);
    deliver(&mut app, FOLLOWING, &effects, page(0, 3, None));
    open_reply(&mut app);
    app.update(Action::Paste("a".repeat(3900)));
    let req = create_req(&submit(&mut app));

    app.update(failed(
        req,
        Failure {
            message: Some("The body field is required and must be a string.".into()),
            ..Failure::new("validation_failed")
        },
    ));

    let rows = screen_rows(&app);
    let footer = rows
        .iter()
        .find(|r| r.contains("Esc cancel"))
        .cloned()
        .unwrap_or_default();
    assert!(
        footer.contains("║ ✗ The body field is… 100  Esc cancel  Ctrl+S Reply ║"),
        "{footer}"
    );
}

#[test]
fn on_the_lowest_screen_the_box_fits_the_list_and_keeps_its_cursor_and_keys_in_view() {
    // A guard: on the smallest terminal the shrinking rule does not act, since the list (15 rows) is taller than
    // the largest box (11 rows).
    use ratatui::layout::Rect;
    let (mut app, effects) = started(120, 20);
    deliver(&mut app, FOLLOWING, &effects, page(0, 3, None));
    open_reply(&mut app);

    write(
        &mut app,
        &(1..=10)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n"),
    );

    let main = crate::ui::layout::split(
        Rect::new(0, 0, 120, 20),
        crate::layout::LayoutMode::from_size(120, 20),
    )
    .main;
    let view = usize::from(main.height) - 4;
    let rows = screen_rows(&app);
    let (top, bottom) = box_rows(&app).unwrap_or((0, 0));
    assert!(bottom > top, "{}", rows.join("\n"));
    assert!(bottom - top < view, "{}", rows.join("\n"));
    assert!(
        rows[bottom - 1].contains("Esc cancel"),
        "{}",
        rows.join("\n")
    );
    let (_, y) = cursor_of(&app);
    assert!(
        rows[usize::from(y)].contains("line 10"),
        "{}",
        rows.join("\n")
    );
}

#[test]
fn the_quote_dialog_shrinks_to_a_low_narrow_screen_with_the_cursor_inside() {
    let (mut app, effects) = started(40, 20);
    deliver(&mut app, FOLLOWING, &effects, page(0, 3, None));
    key(&mut app, 'q');
    assert_eq!(compose_key(&app), Some(quote_key("c0")));

    write(
        &mut app,
        &(1..=15)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n"),
    );

    let rows = screen_rows(&app);
    let top = rows.iter().position(|r| r.contains('╔'));
    let bottom = rows.iter().position(|r| r.contains('╚'));
    assert!(top.is_some() && bottom.is_some(), "{}", rows.join("\n"));
    let (top, bottom) = (top.unwrap_or(0), bottom.unwrap_or(0));
    let left = rows[top].chars().position(|c| c == '╔').unwrap_or(0);
    let right = rows[top].chars().position(|c| c == '╗').unwrap_or(0);
    let (x, y) = cursor_of(&app);
    assert!(
        top < usize::from(y) && usize::from(y) < bottom,
        "{}",
        rows.join("\n")
    );
    assert!(
        left < usize::from(x) && usize::from(x) < right,
        "{}",
        rows.join("\n")
    );
    assert!(
        rows.iter().any(|r| r.contains("Esc cancel")),
        "{}",
        rows.join("\n")
    );
    // The text field shrinks first and the dialog fits the screen, so the quoted card is still drawn whole.
    assert!(bottom - top < 20, "{}", rows.join("\n"));
    assert!(
        rows.iter().any(|r| r.contains("┌ ❝ alice @alice")),
        "{}",
        rows.join("\n")
    );
    assert!(
        rows.iter().any(|r| r.contains("│ second line of commit 0")),
        "{}",
        rows.join("\n")
    );
    let (_, y) = cursor_of(&app);
    assert!(
        rows[usize::from(y)].contains("line 15"),
        "{}",
        rows.join("\n")
    );
}

#[test]
fn the_box_under_the_main_commit_starts_at_the_body_column_of_the_card() {
    let mut app = thread_app();
    open_reply(&mut app);
    write(&mut app, "x");

    let (top, _) = box_rows(&app).unwrap_or((0, 0));
    let rows = screen_rows(&app);
    let chars: Vec<char> = rows[top].chars().collect();
    assert_eq!(
        chars.iter().position(|c| *c == '╔'),
        Some(24),
        "{}",
        rows.join("\n")
    );
    assert_eq!(
        chars.iter().position(|c| *c == '╗'),
        Some(85),
        "{}",
        rows.join("\n")
    );
    assert!(
        rows[top].contains("┃ │ ╔ Reply to @alice"),
        "{}",
        rows.join("\n")
    );
    assert_eq!(compose_text_width(&app, Surface::ReplyBox), 58);
}

#[test]
fn the_box_under_a_second_level_reply_starts_at_its_body_column() {
    let mut app = thread_app();
    key(&mut app, 'j');
    let effects = key(&mut app, 'o');
    deliver_replies(
        &mut app,
        &effects,
        "r0",
        vec![child("k1", "carol", "r0")],
        None,
    );
    key(&mut app, 'j');
    open_reply(&mut app);
    write(&mut app, "x");
    assert_eq!(compose_key(&app), Some(reply_key("k1")));

    let (top, _) = box_rows(&app).unwrap_or((0, 0));
    let rows = screen_rows(&app);
    let chars: Vec<char> = rows[top].chars().collect();
    assert_eq!(
        chars.iter().position(|c| *c == '╔'),
        Some(30),
        "{}",
        rows.join("\n")
    );
    assert_eq!(
        chars.iter().position(|c| *c == '╗'),
        Some(85),
        "{}",
        rows.join("\n")
    );
    assert!(
        rows[top].contains("┃ │       ╔ Reply to @carol"),
        "{}",
        rows.join("\n")
    );
    assert_eq!(compose_text_width(&app, Surface::ReplyBox), 52);
}

// --- review follow-ups: small guards ----------------------------------------------------------------------

#[test]
fn r_and_q_are_typed_into_the_sign_in_dialog_and_open_no_composer() {
    let mut app = guest(120, 40);
    assert_eq!(app.modal(), Some(Modal::SignIn));

    type_str(&mut app, "rq");

    assert_eq!(app.modal(), Some(Modal::SignIn));
    assert!(app.compose().is_none());
}

#[test]
fn a_second_quote_of_the_same_commit_changes_nothing() {
    let mut app = log_app(3);
    key(&mut app, 'q');
    write(&mut app, "quoting");

    app.update(Action::Quote);

    assert_eq!(compose_key(&app), Some(quote_key("c0")));
    assert_eq!(app.modal(), Some(Modal::Composer));
    assert_eq!(draft_of(&app, &quote_key("c0")), Some("quoting".into()));
}

#[test]
fn writing_a_new_commit_replaces_the_quote_dialog_and_keeps_its_draft() {
    let mut app = log_app(3);
    key(&mut app, 'q');
    write(&mut app, "quoting");

    app.update(Action::Commit);

    assert_eq!(compose_key(&app), Some(DraftKey::New));
    assert_eq!(compose_surface(&app), Some(Surface::Inline));
    assert_eq!(app.modal(), None);
    assert_eq!(draft_of(&app, &quote_key("c0")), Some("quoting".into()));
}

// --- review follow-ups: a tall card at the lowest screen, and a list that failed to load ---------------------

#[test]
fn a_box_taller_than_the_list_keeps_the_cursor_row_and_the_keys_row_in_view() {
    let (mut app, effects) = started(120, 20);
    let body = (1..=6)
        .map(|i| format!("body line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let tall = commit_of(merge(
        commit_json("t0", "alice", 60),
        json!({"body": body, "body_segments": [{"type": "text", "text": body}]}),
    ));
    deliver(&mut app, FOLLOWING, &effects, feed_page(vec![tall]));
    open_reply(&mut app);

    write(
        &mut app,
        &(1..=8)
            .map(|i| format!("row {i}"))
            .collect::<Vec<_>>()
            .join("\n"),
    );

    let rows = screen_rows(&app);
    let (_, y) = cursor_of(&app);
    assert!(
        rows[usize::from(y)].contains("row 8"),
        "{}",
        rows.join("\n")
    );
    assert!(
        rows.iter().any(|r| r.contains("Esc cancel")),
        "{}",
        rows.join("\n")
    );
    assert!(rows.iter().any(|r| r.contains('╚')), "{}", rows.join("\n"));
}

#[test]
fn a_reply_does_not_join_a_list_of_replies_that_failed_to_load() {
    let mut app = thread_app();
    key(&mut app, 'j');
    let effects = key(&mut app, 'o');
    let req = crate::thread_tests::replies_req(&effects, "r0");
    app.update(Action::RepliesFailed {
        thread: MAIN.into(),
        parent: "r0".into(),
        req,
        failure: Failure::new("http_500"),
    });
    open_reply(&mut app);
    write(&mut app, "into the void");
    let sent = create_req(&submit(&mut app));
    assert_eq!(compose_key(&app), Some(reply_key("r0")));

    app.update(created(sent, reply_commit("r0", "pending")));

    assert_eq!(thread_children(&app, "r0"), Vec::<String>::new());
    let items = state(&app, MAIN)
        .children
        .get(MAIN)
        .map(|l| l.items.clone())
        .unwrap_or_default();
    assert_eq!(items.first().and_then(replies_count_of), Some(2));
}
