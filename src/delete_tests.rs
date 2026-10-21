//! Deleting your own commit: the confirmation, the card shown while the delete is out, and what the answer changes.
//!
//! - `d` asks for confirmation on your own commit of Log, Saved, a profile list or a thread; `y` deletes, `n` and
//!   Esc cancel, Ctrl+C quits and every other key is ignored. Someone else's commit, a tombstone, an empty list and
//!   a session being restored do nothing, and a guest is asked to sign in.
//! - While the delete is out the card reads `rm a1b2c3d → /dev/null` and takes no `s`, `b`, `p`, `d`, `r` or `q`.
//! - The commit leaves every list. A commit that replies or loaded replies still lean on stays in a thread as a
//!   tombstone; the main commit and the ancestors of a thread always do. The parent and the quoted commit of a
//!   commit that is gone count one reply less, once.
//! - An answer of 404 is a delete that already happened. Any other refusal puts the card back and shows a toast.
//! - What the answer decided is applied to loads that were already out when it arrived.

use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};
use serde_json::{Value, json};
use time::UtcOffset;

use crate::action::{Action, Effect, Failure};
use crate::app::{App, LogTab, Me, Modal, ProfileTab, Screen, Session, ToastKind};
use crate::drafts::DraftKey;
use crate::feed::{FeedKey, ReqId};
use crate::feed_tests::{FOLLOWING, LATEST, TRENDING, deliver, down};
use crate::keymap::{map_event, map_key};
use crate::model::{Commit, ThreadItem, ThreadResponse};
use crate::profile_tests::{deliver_commits, deliver_user, open_profile, user_resp};
use crate::test_support::*;
use crate::thread::{Entry, item_id};
use crate::thread_tests::{
    MAIN, ancestor, commit_json, commit_of, deliver_replies, deliver_thread, item_of, merge,
    open_thread, replies_req, state, thread_req, tomb, user_json,
};
use crate::write_tests::{card, card_with, feed_page, guest_profile, key, log_app_of, mapped};
use crate::write_tests::{me_of, sign_in_again, toast};

const ACCOUNT: &str = "0190a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b";
const NOT_APPROVED: &str = "device_not_approved";

// --- helpers ------------------------------------------------------------------

/// Your own commit; the fixture account is `myuron`.
pub(crate) fn own(id: &str) -> Commit {
    card(id, "myuron")
}

pub(crate) fn other(id: &str) -> Commit {
    card(id, "alice")
}

/// The hash the fixtures give a commit, which the toast and the frame name.
fn hash(id: &str) -> String {
    format!("{id:0>7}")
}

fn rm_text(id: &str) -> String {
    format!("rm {} → /dev/null", hash(id))
}

fn deletes(effects: &[Effect]) -> Vec<(String, ReqId)> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::DeleteCommit { id, req } => Some((id.clone(), *req)),
            _ => None,
        })
        .collect()
}

/// `d`, then `y`: the request of the delete that went out.
pub(crate) fn delete(app: &mut App) -> ReqId {
    key(app, 'd');
    assert_eq!(
        app.modal(),
        Some(Modal::ConfirmDelete),
        "d did not open the confirmation"
    );
    let effects = key(app, 'y');
    let sent = deletes(&effects);
    assert_eq!(sent.len(), 1, "y did not send one delete: {effects:?}");
    sent[0].1
}

pub(crate) fn done(id: &str, req: ReqId) -> Action {
    Action::DeleteDone { id: id.into(), req }
}

fn failed(id: &str, req: ReqId, failure: Failure) -> Action {
    Action::DeleteFailed {
        id: id.into(),
        req,
        failure,
    }
}

fn ids(app: &App, key: FeedKey) -> Vec<String> {
    app.feed(key).items.iter().map(|c| c.id.clone()).collect()
}

fn selected_id(app: &App, key: FeedKey) -> Option<String> {
    let feed = app.feed(key);
    feed.items.get(feed.selected).map(|c| c.id.clone())
}

fn replies_of(app: &App, key: FeedKey, id: &str) -> Option<Option<u32>> {
    app.feed(key)
        .items
        .iter()
        .find(|c| c.id == id)
        .map(|c| c.replies_count)
}

/// Where a commit is held as a commit, by name.
fn places(app: &App, id: &str) -> Vec<String> {
    let mut out = Vec::new();
    for key in FeedKey::ALL {
        let feed = app.feed(key);
        if feed
            .items
            .iter()
            .chain(&feed.suggestions)
            .any(|c| c.id == id)
        {
            out.push(format!("{key:?}"));
        }
    }
    if let Some(profile) = app.profile("myuron") {
        if profile.commits.items.iter().any(|c| c.id == id) {
            out.push("profile commits".into());
        }
        if profile.replies.items.iter().any(|c| c.id == id) {
            out.push("profile replies".into());
        }
        let pinned = profile.user.as_ref().and_then(|u| u.pinned_commit.as_ref());
        if pinned.is_some_and(|c| c.id == id) {
            out.push("pinned".into());
        }
    }
    if let Some(thread) = app.thread(MAIN) {
        let is_commit =
            |item: &ThreadItem| matches!(item, ThreadItem::Commit(_)) && item_id(item) == id;
        if thread.main.as_ref().is_some_and(is_commit) {
            out.push("thread main".into());
        }
        let listed = thread.children.values().flat_map(|l| l.items.iter());
        if thread.ancestors.iter().chain(listed).any(is_commit) {
            out.push("thread item".into());
        }
    }
    out
}

/// Whether the thread holds the commit as a tombstone, in the main place, an ancestor or a list of replies.
fn tombstoned(app: &App, id: &str) -> bool {
    let Some(thread) = app.thread(MAIN) else {
        return false;
    };
    let is_tomb =
        |item: &ThreadItem| matches!(item, ThreadItem::Tombstone(_)) && item_id(item) == id;
    let listed = thread.children.values().flat_map(|l| l.items.iter());
    thread.main.iter().any(is_tomb) || thread.ancestors.iter().chain(listed).any(is_tomb)
}

fn children_of(app: &App, parent: &str) -> Vec<String> {
    state(app, MAIN)
        .children
        .get(parent)
        .map(|l| l.items.iter().map(|i| item_id(i).to_string()).collect())
        .unwrap_or_default()
}

fn main_replies(app: &App) -> Option<u32> {
    state(app, MAIN).main_commit().and_then(|c| c.replies_count)
}

fn entry_of(app: &App) -> Option<Entry> {
    let thread = state(app, MAIN);
    thread.entries(MAIN, false).into_iter().nth(thread.selected)
}

fn event_key(app: &App, code: KeyCode, modifiers: KeyModifiers) -> Option<Action> {
    map_key(app, key_event(code, modifiers, KeyEventKind::Press))
}

/// A reply of yours to `parent`.
fn reply_of(id: &str, author: &str, parent: &str, extra: Value) -> ThreadItem {
    item_of(merge(
        merge(commit_json(id, author, 600), json!({"parent_id": parent})),
        extra,
    ))
}

/// The thread of `MAIN` by `author`, with the given ancestors and first level of replies; the main commit is
/// selected.
pub(crate) fn thread_by(
    author: &str,
    replies_count: u32,
    ancestors: Vec<ThreadItem>,
    replies: Vec<ThreadItem>,
) -> App {
    let mut app = signed_in(120, 40);
    let effects = open_thread(&mut app, MAIN);
    let main = commit_of(merge(
        commit_json(MAIN, author, 3600),
        json!({"replies_count": replies_count, "short_hash": "m1"}),
    ));
    app.update(Action::ThreadLoaded {
        id: MAIN.into(),
        req: thread_req(&effects, MAIN),
        response: Box::new(ThreadResponse {
            data: main,
            ancestors,
        }),
    });
    deliver_replies(&mut app, &effects, MAIN, replies, None);
    app
}

/// Following, Latest, Trending and Saved with `c0`..`c2`, your `c1` among them; your profile lists `c1` in its
/// commits, its replies and as the pinned commit; the thread of `MAIN` holds `c1` as an ancestor and is kept in
/// the history. Following is shown with `c1` selected. Eight copies of `c1` are held.
fn own_everywhere() -> App {
    let page = || feed_page(vec![other("c0"), own("c1"), other("c2")]);
    let mut app = log_app_of(fixture_me(), page().items);
    let fetch = type_str(&mut app, "2");
    deliver(&mut app, LATEST, &fetch, page());
    let fetch = type_str(&mut app, "3");
    deliver(&mut app, TRENDING, &fetch, page());
    let fetch = app.update(Action::Goto(Screen::Saved));
    deliver(&mut app, FeedKey::Saved, &fetch, page());
    let effects = open_profile(&mut app, "myuron");
    deliver_user(
        &mut app,
        &effects,
        "myuron",
        user_resp(
            "myuron",
            json!({"pinned_commit": commit_json("c1", "myuron", 120)}),
        ),
    );
    deliver_commits(
        &mut app,
        &effects,
        "myuron",
        ProfileTab::Commits,
        vec![own("c0"), own("c1")],
        None,
    );
    let effects = key(&mut app, '2');
    deliver_commits(
        &mut app,
        &effects,
        "myuron",
        ProfileTab::Replies,
        vec![own("c1")],
        None,
    );
    key(&mut app, '1');
    app.update(Action::Open(Screen::Log(LogTab::Following)));
    let effects = open_thread(&mut app, MAIN);
    deliver_thread(&mut app, &effects, vec![ancestor("c1", "myuron")], 0);
    deliver_replies(&mut app, &effects, MAIN, vec![], None);
    app.update(Action::Open(Screen::Log(LogTab::Following)));
    down(&mut app, 1);
    app
}

fn account_me() -> Me {
    Me {
        id: ACCOUNT.into(),
        ..fixture_me()
    }
}

/// A commit of the account that can keep drafts.
fn account_own(id: &str) -> Commit {
    card_with(
        id,
        "myuron",
        json!({"author": merge(user_json("myuron"), json!({"id": ACCOUNT}))}),
    )
}

// --- keymap ---------------------------------------------------------------------

#[test]
fn d_and_the_bang_act_on_the_card_of_every_list_with_the_main_pane_focused() {
    let mut saved = signed_in(120, 40);
    saved.update(Action::Goto(Screen::Saved));
    let mut profile = signed_in(120, 40);
    open_profile(&mut profile, "alice");
    let thread = {
        let mut app = signed_in(120, 40);
        open_thread(&mut app, MAIN);
        app
    };
    let scenes = [
        ("log", log_app_of(fixture_me(), vec![own("c0")])),
        ("saved", saved),
        ("thread", thread),
        ("profile", profile),
    ];
    for (name, app) in scenes {
        assert_eq!(mapped(&app, 'd'), Some(Action::Delete), "{name}");
        for modifiers in [KeyModifiers::NONE, KeyModifiers::SHIFT] {
            let bang = event_key(&app, KeyCode::Char('!'), modifiers);
            assert_eq!(bang, Some(Action::Report), "{name} {modifiers:?}");
        }
    }
}

#[test]
fn d_and_the_bang_are_not_bound_on_screens_without_cards_or_with_the_nav_focused() {
    let mut scenes = Vec::new();
    for screen in [
        Screen::Notifications,
        Screen::Settings,
        Screen::Connections("alice".into(), crate::app::ConnTab::Followers),
    ] {
        let mut app = signed_in(120, 40);
        app.update(Action::Open(screen.clone()));
        scenes.push((format!("{screen:?}"), app));
    }
    let mut nav = log_app_of(fixture_me(), vec![own("c0")]);
    press(&mut nav, KeyCode::Tab);
    scenes.push(("nav focus".to_string(), nav));
    for (name, app) in scenes {
        assert_eq!(mapped(&app, 'd'), None, "{name} d");
        let bang = event_key(&app, KeyCode::Char('!'), KeyModifiers::NONE);
        assert_eq!(bang, None, "{name} !");
    }
}

#[test]
fn the_confirmation_reads_y_n_esc_and_ctrl_c_and_nothing_else() {
    let mut app = log_app_of(fixture_me(), vec![own("c0"), other("c1")]);
    key(&mut app, 'd');
    assert_eq!(app.modal(), Some(Modal::ConfirmDelete));

    let none = KeyModifiers::NONE;
    assert_eq!(
        event_key(&app, KeyCode::Char('y'), none),
        Some(Action::ConfirmDelete)
    );
    assert_eq!(
        event_key(&app, KeyCode::Char('n'), none),
        Some(Action::CloseModal)
    );
    assert_eq!(
        event_key(&app, KeyCode::Esc, none),
        Some(Action::CloseModal)
    );
    assert_eq!(
        event_key(&app, KeyCode::Char('c'), KeyModifiers::CONTROL),
        Some(Action::Quit)
    );
    for code in [
        KeyCode::Char('Y'),
        KeyCode::Char('N'),
        KeyCode::Char('d'),
        KeyCode::Char('j'),
        KeyCode::Char('s'),
        KeyCode::Char('Q'),
        KeyCode::Char('?'),
        KeyCode::Char('g'),
        KeyCode::Char('c'),
        KeyCode::Enter,
        KeyCode::Tab,
        KeyCode::Down,
    ] {
        assert_eq!(event_key(&app, code, none), None, "{code:?}");
    }
    assert_eq!(
        map_event(&app, &Event::Paste("pasted".into())),
        None,
        "a paste has nowhere to go"
    );
}

// --- the confirmation -----------------------------------------------------------------

#[test]
fn d_on_your_own_commit_asks_first_and_sends_nothing() {
    let mut saved = signed_in(120, 40);
    let effects = saved.update(Action::Goto(Screen::Saved));
    deliver(
        &mut saved,
        FeedKey::Saved,
        &effects,
        feed_page(vec![own("s0")]),
    );
    let mut profile = signed_in(120, 40);
    let effects = open_profile(&mut profile, "myuron");
    deliver_user(
        &mut profile,
        &effects,
        "myuron",
        user_resp("myuron", json!({})),
    );
    let listed = vec![own("p0")];
    deliver_commits(
        &mut profile,
        &effects,
        "myuron",
        ProfileTab::Commits,
        listed,
        None,
    );
    let mut reply = thread_by(
        "alice",
        1,
        vec![],
        vec![reply_of("r1", "myuron", MAIN, json!({}))],
    );
    down(&mut reply, 1);
    let main = thread_by("myuron", 0, vec![], vec![]);
    let mut ancestor_of = thread_by("alice", 0, vec![ancestor("a0", "myuron")], vec![]);
    key(&mut ancestor_of, 'k');
    let scenes = [
        (
            "log",
            log_app_of(fixture_me(), vec![own("c0")]),
            "c0",
            hash("c0"),
        ),
        ("saved", saved, "s0", hash("s0")),
        ("profile", profile, "p0", hash("p0")),
        ("thread reply", reply, "r1", hash("r1")),
        ("thread main", main, MAIN, "m1".to_string()),
        ("thread ancestor", ancestor_of, "a0", hash("a0")),
    ];
    for (name, mut app, id, short_hash) in scenes {
        let effects = key(&mut app, 'd');

        assert_eq!(effects, Vec::<Effect>::new(), "{name}");
        assert_eq!(app.modal(), Some(Modal::ConfirmDelete), "{name}");
        let confirm = app.delete_confirm().cloned();
        assert_eq!(confirm.as_ref().map(|c| c.id.as_str()), Some(id), "{name}");
        assert_eq!(confirm.map(|c| c.short_hash), Some(short_hash), "{name}");
        assert!(!app.is_deleting(id), "{name}: nothing is out yet");
    }
}

#[test]
fn d_does_nothing_on_the_commit_of_someone_else() {
    let mut log = log_app_of(fixture_me(), vec![other("c0")]);
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
        vec![other("p0")],
        None,
    );
    let mut thread = thread_by("alice", 0, vec![], vec![]);
    for (name, app) in [
        ("log", &mut log),
        ("profile", &mut profile),
        ("thread", &mut thread),
    ] {
        assert_eq!(key(app, 'd'), Vec::<Effect>::new(), "{name}");
        assert_eq!(app.modal(), None, "{name}");
        assert_eq!(app.delete_confirm(), None, "{name}");
    }
}

#[test]
fn whose_commit_it_is_follows_the_id_of_the_author_and_not_the_name() {
    let same_name = card_with(
        "c0",
        "myuron",
        json!({"author": merge(user_json("myuron"), json!({"id": "somebody-else"}))}),
    );
    let same_id = card_with(
        "c1",
        "renamed",
        json!({"author": merge(user_json("renamed"), json!({"id": "user-myuron"}))}),
    );
    let mut app = log_app_of(fixture_me(), vec![same_name, same_id]);

    key(&mut app, 'd');
    assert_eq!(app.modal(), None, "the name alone is not enough");
    down(&mut app, 1);
    key(&mut app, 'd');
    assert_eq!(app.modal(), Some(Modal::ConfirmDelete), "the id is");
}

#[test]
fn d_does_nothing_on_a_tombstone_or_an_empty_list() {
    let replies = vec![tomb("r0")];
    let mut thread = thread_by("alice", 1, vec![], replies);
    down(&mut thread, 1);
    assert!(matches!(entry_of(&thread), Some(Entry::Reply(id, _)) if id == "r0"));
    let mut empty = log_app_of(fixture_me(), vec![]);
    for (name, app) in [("tombstone", &mut thread), ("empty", &mut empty)] {
        assert_eq!(key(app, 'd'), Vec::<Effect>::new(), "{name}");
        assert_eq!(app.modal(), None, "{name}");
    }
}

#[test]
fn a_guest_is_asked_to_sign_in_and_a_session_being_restored_does_nothing() {
    let mut guest = guest_profile(vec![other("c0")]);
    let effects = guest.update(Action::Delete);
    assert_eq!(guest.modal(), Some(Modal::SignIn));
    assert!(deletes(&effects).is_empty());
    assert_eq!(guest.delete_confirm(), None);

    let mut restoring = App::new(Session::Restoring, NOW, UtcOffset::UTC);
    restoring.update(Action::Resize(120, 40));
    restoring.update(Action::Init);
    assert_eq!(restoring.update(Action::Delete), Vec::<Effect>::new());
    assert_eq!(restoring.modal(), None);
}

#[test]
fn n_and_esc_cancel_and_nothing_goes_out() {
    for code in [KeyCode::Char('n'), KeyCode::Esc] {
        let mut app = log_app_of(fixture_me(), vec![own("c0"), other("c1")]);
        key(&mut app, 'd');

        let effects = press(&mut app, code);

        assert_eq!(effects, Vec::<Effect>::new(), "{code:?}");
        assert_eq!(app.modal(), None, "{code:?}");
        assert_eq!(app.delete_confirm(), None, "{code:?}");
        assert!(!app.is_deleting("c0"), "{code:?}");
        assert_eq!(app.screen(), Screen::Log(LogTab::Following), "{code:?}");
        let again = key(&mut app, 'd');
        assert!(
            again.is_empty() && app.modal() == Some(Modal::ConfirmDelete),
            "{code:?}"
        );
    }
}

#[test]
fn every_other_key_leaves_the_confirmation_as_it_is_and_ctrl_c_quits() {
    let mut app = log_app_of(fixture_me(), vec![own("c0"), other("c1")]);
    key(&mut app, 'd');

    let mut effects = type_str(&mut app, "jQ?scbpr@gd");
    effects.extend(press(&mut app, KeyCode::Tab));
    effects.extend(press(&mut app, KeyCode::Enter));
    effects.extend(press(&mut app, KeyCode::Down));

    assert_eq!(effects, Vec::<Effect>::new());
    assert_eq!(app.modal(), Some(Modal::ConfirmDelete));
    assert_eq!(app.feed(FOLLOWING).selected, 0);
    assert_eq!(app.screen(), Screen::Log(LogTab::Following));
    assert!(quit_requested(&ctrl(&mut app, 'c')));
}

#[test]
fn y_in_the_confirmation_deletes_and_does_not_copy_the_link() {
    let mut app = log_app_of(
        fixture_me(),
        vec![
            card_with(
                "c0",
                "myuron",
                json!({"html_url": "https://dev-null.sh/@myuron/commits/c0"}),
            ),
            other("c1"),
        ],
    );
    key(&mut app, 'd');

    let effects = key(&mut app, 'y');

    assert_eq!(deletes(&effects).len(), 1, "{effects:?}");
    assert_eq!(deletes(&effects)[0].0, "c0");
    assert_eq!(effects.len(), 1, "{effects:?}");
    assert!(!effects.iter().any(|e| matches!(e, Effect::CopyLink(_))));
    assert_eq!(app.modal(), None);
    assert_eq!(app.delete_confirm(), None);
}

#[test]
fn a_second_confirmation_after_the_dialog_closed_sends_nothing() {
    let mut app = log_app_of(fixture_me(), vec![own("c0"), other("c1")]);
    delete(&mut app);

    assert_eq!(app.update(Action::ConfirmDelete), Vec::<Effect>::new());
}

// --- the card while the delete is out -----------------------------------------------------

#[test]
fn while_the_delete_is_out_the_card_takes_none_of_the_card_keys() {
    let mut app = log_app_of(fixture_me(), vec![own("c0"), other("c1")]);
    delete(&mut app);
    assert!(app.is_deleting("c0"));
    assert!(!app.is_deleting("c1"));

    for c in ['s', 'b', 'p', 'd', 'r', 'q'] {
        let effects = key(&mut app, c);
        assert_eq!(effects, Vec::<Effect>::new(), "{c}");
        assert_eq!(app.modal(), None, "{c}");
        assert!(app.compose().is_none(), "{c}");
    }
    assert!(app.delete_confirm().is_none());
    assert_eq!(app.feed(FOLLOWING).items[0].starred, Some(false));
    assert_eq!(app.feed(FOLLOWING).items[0].is_saved, Some(false));
}

#[test]
fn while_the_delete_is_out_the_other_cards_take_their_keys() {
    let mut app = log_app_of(fixture_me(), vec![own("c0"), other("c1")]);
    delete(&mut app);
    down(&mut app, 1);

    let effects = key(&mut app, 's');

    assert!(matches!(effects.as_slice(), [Effect::StarCommit { id, on: true, .. }] if id == "c1"));
}

#[test]
fn the_card_that_is_being_deleted_is_marked_in_every_place_it_is_held() {
    let mut app = own_everywhere();

    delete(&mut app);

    assert!(app.is_deleting("c1"));
    assert_eq!(
        places(&app, "c1").len(),
        8,
        "the lists are left alone until the answer"
    );
}

// --- success: the lists ----------------------------------------------------------------------

#[test]
fn the_answer_removes_the_commit_from_every_list_and_shows_the_toast() {
    let mut app = own_everywhere();
    assert_eq!(places(&app, "c1").len(), 8);
    let req = delete(&mut app);

    app.update(done("c1", req));

    assert_eq!(places(&app, "c1"), Vec::<String>::new());
    assert!(!app.is_deleting("c1"));
    assert_eq!(toast(&app), Some((ToastKind::Success, rm_text("c1"))));
    assert_eq!(ids(&app, FOLLOWING), ["c0", "c2"]);
    assert_eq!(ids(&app, FeedKey::Saved), ["c0", "c2"]);
}

#[test]
fn an_ancestor_of_a_thread_becomes_a_tombstone_whatever_its_reply_count() {
    let mut app = own_everywhere();
    let req = delete(&mut app);

    app.update(done("c1", req));

    assert!(tombstoned(&app, "c1"));
    assert_eq!(state(&app, MAIN).ancestors.len(), 1);
}

#[test]
fn a_selection_after_the_removed_row_moves_up_with_its_card() {
    let items = vec![own("o0"), other("a1"), other("a2"), other("a3")];
    let mut app = log_app_of(fixture_me(), items);
    let req = delete(&mut app);
    down(&mut app, 3);
    assert_eq!(selected_id(&app, FOLLOWING).as_deref(), Some("a3"));

    app.update(done("o0", req));

    assert_eq!(app.feed(FOLLOWING).selected, 2);
    assert_eq!(selected_id(&app, FOLLOWING).as_deref(), Some("a3"));
}

#[test]
fn a_selection_on_the_removed_row_keeps_its_place_and_so_selects_the_next_card() {
    let mut app = log_app_of(
        fixture_me(),
        vec![other("a0"), own("o1"), other("a2"), other("a3")],
    );
    down(&mut app, 1);
    let req = delete(&mut app);

    app.update(done("o1", req));

    assert_eq!(app.feed(FOLLOWING).selected, 1);
    assert_eq!(selected_id(&app, FOLLOWING).as_deref(), Some("a2"));
}

#[test]
fn a_selection_on_the_removed_last_row_moves_to_the_new_last_one() {
    let mut app = log_app_of(fixture_me(), vec![other("a0"), other("a1"), own("o2")]);
    down(&mut app, 2);
    let req = delete(&mut app);

    app.update(done("o2", req));

    assert_eq!(app.feed(FOLLOWING).selected, 1);
    assert_eq!(selected_id(&app, FOLLOWING).as_deref(), Some("a1"));
}

#[test]
fn a_selection_before_the_removed_row_stays_where_it_is() {
    let mut app = log_app_of(fixture_me(), vec![other("a0"), own("o1"), other("a2")]);
    down(&mut app, 1);
    let req = delete(&mut app);
    press(&mut app, KeyCode::Char('k'));

    app.update(done("o1", req));

    assert_eq!(app.feed(FOLLOWING).selected, 0);
    assert_eq!(selected_id(&app, FOLLOWING).as_deref(), Some("a0"));
}

#[test]
fn a_tab_that_is_not_shown_moves_its_selection_when_a_row_above_it_goes() {
    let mut app = log_app_of(fixture_me(), vec![own("o0"), other("a1")]);
    let fetch = type_str(&mut app, "2");
    let latest = vec![other("a5"), own("o0"), other("a6"), other("a7")];
    deliver(&mut app, LATEST, &fetch, feed_page(latest));
    down(&mut app, 3);
    assert_eq!(selected_id(&app, LATEST).as_deref(), Some("a7"));
    type_str(&mut app, "1");
    let req = delete(&mut app);

    app.update(done("o0", req));

    assert_eq!(ids(&app, LATEST), ["a5", "a6", "a7"]);
    assert_eq!(app.feed(LATEST).selected, 2);
    assert_eq!(selected_id(&app, LATEST).as_deref(), Some("a7"));
    assert_eq!(app.feed(FOLLOWING).selected, 0);
}

#[test]
fn a_profile_counts_the_pinned_commit_as_its_first_card_when_it_moves_the_selection() {
    let build = || {
        let mut app = signed_in(120, 40);
        let effects = open_profile(&mut app, "myuron");
        let pinned = json!({"pinned_commit": commit_json("c1", "myuron", 120)});
        deliver_user(&mut app, &effects, "myuron", user_resp("myuron", pinned));
        let items = vec![own("c0"), own("c1"), own("c2"), own("c3")];
        deliver_commits(
            &mut app,
            &effects,
            "myuron",
            ProfileTab::Commits,
            items,
            None,
        );
        app
    };
    let cards = |app: &App| -> Vec<String> {
        app.profile("myuron")
            .map(|p| p.cards(true).iter().map(|c| c.id.clone()).collect())
            .unwrap_or_default()
    };
    let selected = |app: &App| app.profile("myuron").map(|p| p.selected());

    let mut app = build();
    assert_eq!(cards(&app), ["c1", "c0", "c2", "c3"]);
    let req = delete(&mut app);
    down(&mut app, 3);
    assert_eq!(selected(&app), Some(3));
    app.update(done("c1", req));
    assert_eq!(cards(&app), ["c0", "c2", "c3"]);
    assert_eq!(
        selected(&app),
        Some(2),
        "the pinned card was before the selection"
    );

    let mut app = build();
    let req = delete(&mut app);
    app.update(done("c1", req));
    assert_eq!(cards(&app), ["c0", "c2", "c3"]);
    assert_eq!(selected(&app), Some(0), "the pinned card was the selection");

    let mut app = build();
    down(&mut app, 3);
    let req = delete(&mut app);
    app.update(done("c3", req));
    assert_eq!(cards(&app), ["c1", "c0", "c2"]);
    assert_eq!(selected(&app), Some(2), "the last card was the selection");
}

// --- success: threads -------------------------------------------------------------------------

#[test]
fn the_main_commit_of_a_thread_becomes_a_tombstone_and_the_replies_stay() {
    let replies = vec![
        reply_of("r1", "bob", MAIN, json!({})),
        reply_of("r2", "carol", MAIN, json!({})),
    ];
    let mut app = thread_by("myuron", 2, vec![], replies);
    let req = delete(&mut app);

    app.update(done(MAIN, req));

    assert!(
        matches!(&state(&app, MAIN).main, Some(ThreadItem::Tombstone(t)) if t.id == MAIN),
        "{:?}",
        state(&app, MAIN).main
    );
    assert_eq!(children_of(&app, MAIN), ["r1", "r2"]);
    assert_eq!(
        toast(&app),
        Some((ToastKind::Success, "rm m1 → /dev/null".into()))
    );
}

#[test]
fn a_tombstone_main_commit_takes_no_card_key() {
    let replies = vec![reply_of("r1", "bob", MAIN, json!({}))];
    let mut app = thread_by("myuron", 1, vec![], replies);
    let req = delete(&mut app);
    app.update(done(MAIN, req));

    for c in ['s', 'b', 'p', 'd', 'r', 'q', 'y', 'O'] {
        assert_eq!(key(&mut app, c), Vec::<Effect>::new(), "{c}");
        assert_eq!(app.modal(), None, "{c}");
    }
    assert_eq!(state(&app, MAIN).selected, 0);
    down(&mut app, 1);
    assert!(matches!(entry_of(&app), Some(Entry::Reply(id, _)) if id == "r1"));
}

#[test]
fn a_reply_without_replies_leaves_the_thread_and_counts_one_less_under_its_parent() {
    let replies = vec![
        reply_of("r1", "myuron", MAIN, json!({"replies_count": 0})),
        reply_of("r2", "bob", MAIN, json!({})),
    ];
    let mut app = thread_by("alice", 2, vec![], replies);
    down(&mut app, 1);
    let req = delete(&mut app);

    app.update(done("r1", req));

    assert_eq!(children_of(&app, MAIN), ["r2"]);
    assert!(!tombstoned(&app, "r1"));
    assert_eq!(main_replies(&app), Some(1));
}

#[test]
fn a_reply_that_has_replies_stays_as_a_tombstone_and_its_parent_keeps_its_count() {
    let replies = vec![
        reply_of("r1", "myuron", MAIN, json!({"replies_count": 2})),
        reply_of("r2", "bob", MAIN, json!({})),
    ];
    let mut app = thread_by("alice", 2, vec![], replies);
    down(&mut app, 1);
    let req = delete(&mut app);

    app.update(done("r1", req));

    assert_eq!(children_of(&app, MAIN), ["r1", "r2"]);
    assert!(tombstoned(&app, "r1"));
    assert_eq!(main_replies(&app), Some(2));
}

#[test]
fn a_reply_whose_replies_are_loaded_stays_even_when_every_copy_counts_zero() {
    let replies = vec![reply_of("r1", "myuron", MAIN, json!({"replies_count": 1}))];
    let mut app = thread_by("alice", 1, vec![], replies);
    down(&mut app, 1);
    let opened = key(&mut app, 'o');
    deliver_replies(
        &mut app,
        &opened,
        "r1",
        vec![reply_of("x1", "bob", "r1", json!({}))],
        None,
    );
    // A bookmark answer carries the count of the server, which is how a copy comes to say 0 above loaded replies.
    let saved = key(&mut app, 'b');
    assert_eq!(saved.len(), 1, "{saved:?}");
    let server = commit_of(merge(
        commit_json("r1", "myuron", 600),
        json!({"parent_id": MAIN, "replies_count": 0, "is_saved": true}),
    ));
    let bookmark_req = saved
        .iter()
        .find_map(|e| match e {
            Effect::BookmarkCommit { req, .. } => Some(*req),
            _ => None,
        })
        .unwrap_or(ReqId(u64::MAX));
    app.update(Action::BookmarkDone {
        id: "r1".into(),
        req: bookmark_req,
        commit: Box::new(server),
    });
    let req = delete(&mut app);

    app.update(done("r1", req));

    assert!(tombstoned(&app, "r1"));
    assert_eq!(children_of(&app, "r1"), ["x1"]);
    assert_eq!(main_replies(&app), Some(1));
}

#[test]
fn a_commit_shown_as_an_ancestor_stays_a_tombstone_when_it_is_deleted_from_a_profile() {
    let ancestors = vec![
        reply_of("p0", "alice", "root", json!({"replies_count": 1})),
        // No copy counts a reply, so only the place in the thread makes the commit stay.
        reply_of("c1", "myuron", "p0", json!({"replies_count": 0})),
    ];
    let mut app = thread_by("alice", 0, ancestors, vec![]);
    let listed = card_with(
        "c1",
        "myuron",
        json!({"parent_id": "p0", "replies_count": null}),
    );
    let effects = open_profile(&mut app, "myuron");
    deliver_user(&mut app, &effects, "myuron", user_resp("myuron", json!({})));
    deliver_commits(
        &mut app,
        &effects,
        "myuron",
        ProfileTab::Commits,
        vec![listed],
        None,
    );
    let req = delete(&mut app);

    app.update(done("c1", req));

    assert!(tombstoned(&app, "c1"));
    let p0 = &state(&app, MAIN).ancestors[0];
    assert!(
        matches!(p0, ThreadItem::Commit(c) if c.replies_count == Some(1)),
        "the parent keeps its count: {p0:?}"
    );
}

#[test]
fn a_continuation_that_goes_leaves_the_selection_on_the_row_that_takes_its_place() {
    let replies = vec![
        reply_of("k1", "myuron", MAIN, json!({})),
        reply_of("r1", "bob", MAIN, json!({})),
        reply_of("r2", "carol", MAIN, json!({})),
    ];
    let mut app = thread_by("myuron", 3, vec![], replies);
    down(&mut app, 1);
    assert_eq!(entry_of(&app), Some(Entry::Continuation));
    let req = delete(&mut app);

    app.update(done("k1", req));

    assert_eq!(state(&app, MAIN).selected, 1);
    assert_eq!(entry_of(&app), Some(Entry::Reply("r1".into(), MAIN.into())));
}

#[test]
fn a_continuation_that_was_the_last_row_leaves_the_selection_on_the_main_commit() {
    let replies = vec![reply_of("k1", "myuron", MAIN, json!({}))];
    let mut app = thread_by("myuron", 1, vec![], replies);
    down(&mut app, 1);
    let req = delete(&mut app);

    app.update(done("k1", req));

    assert_eq!(entry_of(&app), Some(Entry::Main));
}

#[test]
fn a_reply_above_the_selection_that_goes_leaves_the_same_reply_selected() {
    let replies = vec![
        reply_of("r1", "myuron", MAIN, json!({})),
        reply_of("r2", "bob", MAIN, json!({})),
        reply_of("r3", "carol", MAIN, json!({})),
    ];
    let mut app = thread_by("alice", 3, vec![], replies);
    down(&mut app, 1);
    let req = delete(&mut app);
    down(&mut app, 2);
    assert_eq!(entry_of(&app), Some(Entry::Reply("r3".into(), MAIN.into())));

    app.update(done("r1", req));

    assert_eq!(entry_of(&app), Some(Entry::Reply("r3".into(), MAIN.into())));
}

#[test]
fn the_selected_reply_that_goes_leaves_the_next_row_selected() {
    let replies = vec![
        reply_of("r1", "myuron", MAIN, json!({})),
        reply_of("r2", "bob", MAIN, json!({})),
    ];
    let mut app = thread_by("alice", 2, vec![], replies);
    down(&mut app, 1);
    let req = delete(&mut app);

    app.update(done("r1", req));

    assert_eq!(entry_of(&app), Some(Entry::Reply("r2".into(), MAIN.into())));
}

// --- success: counts ----------------------------------------------------------------------------

fn log_with_parent(parent_rc: u32, child: Commit) -> App {
    let parent = card_with("p1", "alice", json!({"replies_count": parent_rc}));
    let mut app = log_app_of(fixture_me(), vec![parent, child]);
    down(&mut app, 1);
    app
}

#[test]
fn a_reply_that_is_gone_counts_one_less_in_every_copy_of_its_parent() {
    let child = card_with("n1", "myuron", json!({"parent_id": "p1"}));
    let mut app = log_with_parent(3, child);
    let effects = app.update(Action::Goto(Screen::Saved));
    let saved = vec![card_with("p1", "alice", json!({"replies_count": 3}))];
    deliver(&mut app, FeedKey::Saved, &effects, feed_page(saved));
    app.update(Action::Goto(Screen::Log(LogTab::Following)));
    down(&mut app, 1);
    let req = delete(&mut app);

    app.update(done("n1", req));

    assert_eq!(replies_of(&app, FOLLOWING, "p1"), Some(Some(2)));
    assert_eq!(replies_of(&app, FeedKey::Saved, "p1"), Some(Some(2)));
}

#[test]
fn a_quote_that_is_gone_counts_one_less_under_the_commit_it_quoted() {
    let quote = card_with("n1", "myuron", json!({"quoted_commit_id": "p1"}));
    let mut app = log_with_parent(3, quote);
    let req = delete(&mut app);

    app.update(done("n1", req));

    assert_eq!(replies_of(&app, FOLLOWING, "p1"), Some(Some(2)));
}

#[test]
fn a_count_is_never_taken_below_zero_and_a_count_not_loaded_stays_not_loaded() {
    let child = card_with("n1", "myuron", json!({"parent_id": "p1"}));
    let mut app = log_with_parent(0, child);
    let req = delete(&mut app);
    app.update(done("n1", req));
    assert_eq!(replies_of(&app, FOLLOWING, "p1"), Some(Some(0)));

    let parent = card_with("p1", "alice", json!({"replies_count": null}));
    let child = card_with("n1", "myuron", json!({"parent_id": "p1"}));
    let mut app = log_app_of(fixture_me(), vec![parent, child]);
    down(&mut app, 1);
    let req = delete(&mut app);
    app.update(done("n1", req));
    assert_eq!(replies_of(&app, FOLLOWING, "p1"), Some(None));
}

#[test]
fn a_commit_that_stays_leaves_the_count_of_its_parent_alone() {
    let child = card_with(
        "n1",
        "myuron",
        json!({"parent_id": "p1", "replies_count": 1}),
    );
    let mut app = log_with_parent(3, child);
    let req = delete(&mut app);

    app.update(done("n1", req));

    assert_eq!(replies_of(&app, FOLLOWING, "p1"), Some(Some(3)));
}

#[test]
fn the_parent_is_counted_once_even_when_the_answer_comes_twice_or_the_list_is_loaded_later() {
    let child = card_with("n1", "myuron", json!({"parent_id": "p1"}));
    let mut app = log_with_parent(3, child);
    let refresh = key(&mut app, 'R');
    let req = delete(&mut app);
    app.update(done("n1", req));
    app.update(done("n1", req));
    assert_eq!(replies_of(&app, FOLLOWING, "p1"), Some(Some(2)));

    // The server already counts the parent without the commit, and the load must not take one more off.
    let page = feed_page(vec![
        card_with("p1", "alice", json!({"replies_count": 2})),
        card_with("n1", "myuron", json!({"parent_id": "p1"})),
    ]);
    deliver(&mut app, FOLLOWING, &refresh, page);
    assert_eq!(ids(&app, FOLLOWING), ["p1"]);

    assert_eq!(replies_of(&app, FOLLOWING, "p1"), Some(Some(2)));
}

#[test]
fn the_commit_counts_of_you_and_your_profile_go_down_by_one() {
    let mut app = signed_in(120, 40);
    let effects = open_profile(&mut app, "myuron");
    deliver_user(&mut app, &effects, "myuron", user_resp("myuron", json!({})));
    deliver_commits(
        &mut app,
        &effects,
        "myuron",
        ProfileTab::Commits,
        vec![own("c0")],
        None,
    );
    let commits = |app: &App| {
        app.profile("myuron")
            .and_then(|p| p.user.as_ref())
            .and_then(|u| u.data.commits_count)
    };
    assert_eq!((me_of(&app).commits, commits(&app)), (42, Some(187)));
    let req = delete(&mut app);

    app.update(done("c0", req));

    assert_eq!((me_of(&app).commits, commits(&app)), (41, Some(186)));
    app.update(done("c0", req));
    assert_eq!(
        (me_of(&app).commits, commits(&app)),
        (41, Some(186)),
        "a second answer counts nothing"
    );
}

#[test]
fn the_commit_count_of_someone_else_is_left_alone() {
    let mut app = signed_in(120, 40);
    let effects = open_profile(&mut app, "alice");
    deliver_user(&mut app, &effects, "alice", user_resp("alice", json!({})));
    deliver_commits(
        &mut app,
        &effects,
        "alice",
        ProfileTab::Commits,
        vec![other("c0")],
        None,
    );
    let own_effects = app.update(Action::Open(Screen::Log(LogTab::Following)));
    deliver(
        &mut app,
        FOLLOWING,
        &own_effects,
        feed_page(vec![own("o0")]),
    );
    let req = delete(&mut app);

    app.update(done("o0", req));

    let alice = app
        .profile("alice")
        .and_then(|p| p.user.as_ref())
        .and_then(|u| u.data.commits_count);
    assert_eq!(alice, Some(187));
}

// --- success: drafts and composers ----------------------------------------------------------------

#[test]
fn the_reply_and_quote_drafts_of_the_commit_go_and_are_saved_while_the_others_stay() {
    let mut app = log_app_of(account_me(), vec![account_own("c0"), other("c1")]);
    key(&mut app, 'r');
    type_str(&mut app, "reply to mine");
    press(&mut app, KeyCode::Esc);
    key(&mut app, 'q');
    type_str(&mut app, "quote of mine");
    press(&mut app, KeyCode::Esc);
    down(&mut app, 1);
    key(&mut app, 'r');
    type_str(&mut app, "reply to theirs");
    press(&mut app, KeyCode::Esc);
    press(&mut app, KeyCode::Char('k'));
    let reply = DraftKey::Reply("c0".into());
    let quote = DraftKey::Quote("c0".into());
    assert!(app.drafts().get(&reply).is_some() && app.drafts().get(&quote).is_some());
    let req = delete(&mut app);

    let effects = app.update(done("c0", req));

    assert!(app.drafts().get(&reply).is_none());
    assert!(app.drafts().get(&quote).is_none());
    assert!(app.drafts().get(&DraftKey::Reply("c1".into())).is_some());
    let saved: Vec<_> = effects
        .iter()
        .filter_map(|e| match e {
            Effect::SaveDrafts { account, drafts } => Some((account.clone(), drafts.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(saved.len(), 1, "{effects:?}");
    assert_eq!(saved[0].0, ACCOUNT);
    assert!(
        saved[0].1.get(&reply).is_none() && saved[0].1.get(&DraftKey::Reply("c1".into())).is_some()
    );
}

#[test]
fn a_composer_open_on_the_deleted_commit_is_closed() {
    let mut app = log_app_of(account_me(), vec![account_own("c0"), other("c1")]);
    key(&mut app, 'r');
    assert!(app.compose().is_some());
    // Keys cannot reach a delete while a composer takes them all, so the actions are sent straight.
    app.update(Action::Delete);
    assert!(
        app.delete_confirm().is_some(),
        "the confirmation did not open"
    );
    let sent = app.update(Action::ConfirmDelete);
    let req = deletes(&sent)
        .first()
        .map(|d| d.1)
        .unwrap_or(ReqId(u64::MAX));
    assert_ne!(req, ReqId(u64::MAX), "{sent:?}");

    app.update(done("c0", req));

    assert!(app.compose().is_none());
    assert_ne!(app.modal(), Some(Modal::Composer));
}

// --- failure ------------------------------------------------------------------------------------

#[test]
fn a_refusal_puts_the_card_back_and_shows_why() {
    let cases = [
        (
            Failure {
                code: "forbidden".into(),
                retry_after: None,
                message: Some("You can only delete your own commits.".into()),
            },
            "You can only delete your own commits.",
        ),
        (
            Failure {
                code: "rate_limited".into(),
                retry_after: Some(9),
                message: Some("Too many".into()),
            },
            "rate_limited: retry in 9s",
        ),
        (Failure::new("network"), "network error — press R to retry"),
        (Failure::new("http_500"), "http_500"),
    ];
    for (failure, text) in cases {
        let mut app = log_app_of(fixture_me(), vec![own("c0"), other("c1")]);
        let req = delete(&mut app);

        app.update(failed("c0", req, failure.clone()));

        assert!(!app.is_deleting("c0"), "{failure:?}");
        assert_eq!(ids(&app, FOLLOWING), ["c0", "c1"], "{failure:?}");
        assert_eq!(
            toast(&app),
            Some((ToastKind::Error, text.into())),
            "{failure:?}"
        );
        let again = key(&mut app, 'd');
        assert!(
            again.is_empty() && app.modal() == Some(Modal::ConfirmDelete),
            "{failure:?}"
        );
    }
}

#[test]
fn an_answer_of_not_found_is_a_delete_that_already_happened() {
    // The code decides, whatever the message says.
    for code in ["commit_unavailable", "http_404"] {
        let mut app = own_everywhere();
        let req = delete(&mut app);
        let refusal = Failure {
            code: code.into(),
            retry_after: None,
            message: Some("No query results for model.".into()),
        };

        app.update(failed("c1", req, refusal));

        assert_eq!(places(&app, "c1"), Vec::<String>::new(), "{code}");
        assert!(tombstoned(&app, "c1"), "{code}");
        assert!(!app.is_deleting("c1"), "{code}");
        assert_eq!(
            toast(&app),
            Some((ToastKind::Success, rm_text("c1"))),
            "{code}"
        );
    }
}

#[test]
fn an_answer_for_another_request_or_another_commit_changes_nothing() {
    let mut app = log_app_of(fixture_me(), vec![own("c0"), other("c1")]);
    let req = delete(&mut app);
    let stale = ReqId(req.0 + 100);

    app.update(done("c0", stale));
    app.update(failed("c0", stale, Failure::new("http_500")));
    app.update(done("c1", req));

    assert!(app.is_deleting("c0"));
    assert_eq!(ids(&app, FOLLOWING), ["c0", "c1"]);
    assert_eq!(toast(&app), None);
    app.update(done("c0", req));
    assert_eq!(ids(&app, FOLLOWING), ["c1"]);
}

// --- the answer reaches loads that were out before it ------------------------------------------------

#[test]
fn a_list_that_was_loading_does_not_bring_the_deleted_commit_back() {
    let mut app = log_app_of(fixture_me(), vec![own("o0"), other("a1")]);
    let refresh = key(&mut app, 'R');
    let req = delete(&mut app);
    app.update(done("o0", req));

    let page = feed_page(vec![own("o0"), other("a1"), other("a2")]);
    deliver(&mut app, FOLLOWING, &refresh, page);

    assert_eq!(ids(&app, FOLLOWING), ["a1", "a2"]);
}

#[test]
fn a_profile_that_was_loading_does_not_bring_back_the_deleted_commit_or_the_pinned_one() {
    let mut app = signed_in(120, 40);
    let effects = open_profile(&mut app, "myuron");
    let pinned = json!({"pinned_commit": commit_json("c1", "myuron", 120)});
    deliver_user(
        &mut app,
        &effects,
        "myuron",
        user_resp("myuron", pinned.clone()),
    );
    deliver_commits(
        &mut app,
        &effects,
        "myuron",
        ProfileTab::Commits,
        vec![own("c0"), own("c1")],
        None,
    );
    let refresh = key(&mut app, 'R');
    let req = delete(&mut app);
    app.update(done("c1", req));

    deliver_user(&mut app, &refresh, "myuron", user_resp("myuron", pinned));
    deliver_commits(
        &mut app,
        &refresh,
        "myuron",
        ProfileTab::Commits,
        vec![own("c0"), own("c1")],
        None,
    );

    let profile = app.profile("myuron");
    let held = profile.map(|p| {
        p.cards(true)
            .iter()
            .map(|c| c.id.clone())
            .collect::<Vec<_>>()
    });
    assert_eq!(held, Some(vec!["c0".to_string()]));
    assert_eq!(
        profile
            .and_then(|p| p.user.as_ref())
            .map(|u| u.pinned_commit.is_none()),
        Some(true)
    );
}

/// A refresh of the thread while the delete of `r1` is out: the lists are cleared, the answer comes, and the
/// answers to the refresh come last.
fn refreshed_while_deleting(rc: u32) -> (App, Vec<Effect>) {
    let replies = vec![reply_of("r1", "myuron", MAIN, json!({"replies_count": rc}))];
    let mut app = thread_by("alice", 1, vec![], replies);
    down(&mut app, 1);
    let req = delete(&mut app);
    let refresh = key(&mut app, 'R');
    app.update(done("r1", req));
    (app, refresh)
}

fn deliver_thread_again(app: &mut App, refresh: &[Effect], items: Vec<ThreadItem>) {
    deliver_thread(app, refresh, vec![], 1);
    app.update(Action::RepliesLoaded {
        thread: MAIN.into(),
        parent: MAIN.into(),
        req: replies_req(refresh, MAIN),
        page: crate::thread_tests::page(items, None),
    });
}

#[test]
fn replies_that_load_late_follow_what_the_answer_decided() {
    // The load brings the count of the server, which the deleted reply must not lower once more.
    let (mut app, refresh) = refreshed_while_deleting(0);
    deliver_thread_again(
        &mut app,
        &refresh,
        vec![reply_of("r1", "myuron", MAIN, json!({"replies_count": 0}))],
    );
    assert_eq!(
        children_of(&app, MAIN),
        Vec::<String>::new(),
        "gone stays gone"
    );
    assert_eq!(main_replies(&app), Some(1));
}

#[test]
fn a_reply_that_comes_back_as_a_tombstone_stays_one() {
    let (mut app, refresh) = refreshed_while_deleting(0);
    deliver_thread_again(&mut app, &refresh, vec![tomb("r1")]);

    assert_eq!(children_of(&app, MAIN), ["r1"]);
    assert!(tombstoned(&app, "r1"));
}

#[test]
fn a_reply_that_comes_back_with_replies_of_its_own_becomes_a_tombstone_and_the_fate_follows() {
    let (mut app, refresh) = refreshed_while_deleting(0);
    deliver_thread_again(
        &mut app,
        &refresh,
        vec![reply_of("r1", "myuron", MAIN, json!({"replies_count": 2}))],
    );
    assert!(tombstoned(&app, "r1"));

    // Once a load showed replies, the commit stays as a tombstone for the loads after it.
    let preview = json!({"data": [commit_json("r1", "myuron", 600)], "next_cursor": null});
    let parent = card_with(
        "p9",
        "alice",
        json!({"replies_count": 1, "reply_preview": preview}),
    );
    let fetch = app.update(Action::Goto(Screen::Log(LogTab::Following)));
    deliver(&mut app, FOLLOWING, &fetch, feed_page(vec![parent]));
    let held = app.feed(FOLLOWING).items[0]
        .reply_preview
        .clone()
        .map(|p| p.data);
    assert!(
        matches!(held.as_deref(), Some([ThreadItem::Tombstone(t)]) if t.id == "r1"),
        "{held:?}"
    );
}

#[test]
fn a_thread_that_loads_late_shows_the_deleted_main_commit_and_ancestors_as_tombstones() {
    let ancestors = vec![reply_of("c1", "myuron", "root", json!({}))];
    let mut app = thread_by("alice", 0, ancestors, vec![]);
    key(&mut app, 'k');
    let refresh = key(&mut app, 'R');
    let req = delete(&mut app);
    app.update(done("c1", req));

    deliver_thread(&mut app, &refresh, vec![ancestor("c1", "myuron")], 0);

    assert!(tombstoned(&app, "c1"));
    assert!(state(&app, MAIN).main_commit().is_some());
}

#[test]
fn a_thread_that_loads_late_shows_the_deleted_main_commit_as_a_tombstone() {
    let replies = vec![reply_of("r1", "bob", MAIN, json!({}))];
    let mut app = thread_by("myuron", 1, vec![], replies);
    let refresh = key(&mut app, 'R');
    let req = delete(&mut app);
    app.update(done(MAIN, req));

    deliver_thread(&mut app, &refresh, vec![], 1);

    assert!(
        matches!(&state(&app, MAIN).main, Some(ThreadItem::Tombstone(t)) if t.id == MAIN),
        "{:?}",
        state(&app, MAIN).main
    );
}

#[test]
fn the_previews_of_replies_lose_the_deleted_reply_or_show_it_as_a_tombstone() {
    for (rc, want_tombstone) in [(0, false), (2, true)] {
        let preview = json!({
            "data": [
                merge(commit_json("r1", "myuron", 600), json!({"parent_id": MAIN, "replies_count": rc})),
                merge(commit_json("r2", "bob", 500), json!({"parent_id": MAIN})),
            ],
            "next_cursor": null,
        });
        let head = card_with(
            MAIN,
            "alice",
            json!({"replies_count": 2, "reply_preview": preview}),
        );
        let mut app = log_app_of(fixture_me(), vec![head]);
        let effects = press(&mut app, KeyCode::Enter);
        let replies = vec![
            reply_of("r1", "myuron", MAIN, json!({"replies_count": rc})),
            reply_of("r2", "bob", MAIN, json!({})),
        ];
        deliver_thread(&mut app, &effects, vec![], 2);
        deliver_replies(&mut app, &effects, MAIN, replies, None);
        down(&mut app, 1);
        let req = delete(&mut app);

        app.update(done("r1", req));

        let listed = app.feed(FOLLOWING).items[0].reply_preview.clone().map(|p| {
            p.data
                .iter()
                .map(|i| {
                    (
                        item_id(i).to_string(),
                        matches!(i, ThreadItem::Tombstone(_)),
                    )
                })
                .collect::<Vec<_>>()
        });
        let want = if want_tombstone {
            vec![("r1".to_string(), true), ("r2".to_string(), false)]
        } else {
            vec![("r2".to_string(), false)]
        };
        assert_eq!(listed, Some(want), "replies_count {rc}");
    }
}

#[test]
fn a_page_that_loads_late_with_a_deleted_reply_in_a_preview_is_cleaned_the_same_way() {
    let replies = vec![reply_of("r1", "myuron", MAIN, json!({}))];
    let mut app = thread_by("alice", 1, vec![], replies);
    down(&mut app, 1);
    let req = delete(&mut app);
    app.update(done("r1", req));
    let preview = json!({
        "data": [commit_json("r1", "myuron", 600), commit_json("r2", "bob", 500)],
        "next_cursor": null,
    });
    let head = card_with("p9", "alice", json!({"reply_preview": preview}));
    let fetch = app.update(Action::Goto(Screen::Log(LogTab::Following)));

    deliver(&mut app, FOLLOWING, &fetch, feed_page(vec![head]));

    let listed = app.feed(FOLLOWING).items[0].reply_preview.clone().map(|p| {
        p.data
            .iter()
            .map(|i| item_id(i).to_string())
            .collect::<Vec<_>>()
    });
    assert_eq!(listed, Some(vec!["r2".to_string()]));
}

// --- the session ------------------------------------------------------------------------------------

#[test]
fn a_rejected_device_in_the_answer_returns_to_guest_and_drops_what_was_pending() {
    let mut app = log_app_of(fixture_me(), vec![own("c0")]);
    let req = delete(&mut app);

    app.update(failed("c0", req, Failure::new(NOT_APPROVED)));

    assert_eq!(app.session(), &Session::Guest);
    assert_eq!(app.modal(), Some(Modal::SignIn));
    assert!(!app.is_deleting("c0"));
    assert_eq!(
        toast(&app),
        Some((
            ToastKind::Error,
            "this device is no longer approved — sign in again".into()
        ))
    );
}

#[test]
fn a_rejection_elsewhere_closes_the_dialogs_and_forgets_the_deletes() {
    let mut confirming = log_app_of(fixture_me(), vec![own("c0")]);
    key(&mut confirming, 'd');
    let mut reporting = log_app_of(fixture_me(), vec![other("c0")]);
    key(&mut reporting, '!');
    let mut deleting = log_app_of(fixture_me(), vec![own("c0")]);
    delete(&mut deleting);
    for (name, app) in [
        ("confirm", &mut confirming),
        ("report", &mut reporting),
        ("deleting", &mut deleting),
    ] {
        assert_ne!(
            (
                app.delete_confirm().is_some(),
                app.report().is_some(),
                app.is_deleting("c0")
            ),
            (false, false, false),
            "{name}: the dialog did not open"
        );

        app.update(Action::FeedFailed {
            key: FOLLOWING,
            req: ReqId(1),
            failure: Failure::new(NOT_APPROVED),
        });

        assert_eq!(app.delete_confirm(), None, "{name}");
        assert_eq!(app.report(), None, "{name}");
        assert!(!app.is_deleting("c0"), "{name}");
        assert_eq!(app.modal(), Some(Modal::SignIn), "{name}");
    }
}

#[test]
fn what_was_deleted_is_forgotten_with_the_session() {
    let mut app = log_app_of(fixture_me(), vec![own("c0"), other("c1")]);
    let req = delete(&mut app);
    app.update(done("c0", req));

    // The helper signs in again and loads `c0`..`c2`, which holds a commit with the id that was deleted.
    sign_in_again(&mut app);

    assert_eq!(ids(&app, FOLLOWING), ["c0", "c1", "c2"]);
}

#[test]
fn an_answer_from_before_the_new_sign_in_changes_nothing() {
    let mut app = log_app_of(fixture_me(), vec![own("c0")]);
    let old = delete(&mut app);
    sign_in_again(&mut app);
    let before = ids(&app, FOLLOWING);
    let shown = toast(&app);

    app.update(done("c0", old));
    app.update(failed("c0", old, Failure::new("http_500")));
    app.update(failed("c0", old, Failure::new(NOT_APPROVED)));

    assert!(matches!(app.session(), Session::SignedIn(_)));
    assert_eq!(ids(&app, FOLLOWING), before);
    assert_eq!(toast(&app), shown);
}

#[test]
fn an_answer_that_arrives_after_the_return_to_guest_changes_neither_the_toast_nor_the_session() {
    let mut app = log_app_of(fixture_me(), vec![own("c0")]);
    let req = delete(&mut app);
    app.update(failed("c0", req, Failure::new(NOT_APPROVED)));
    let shown = toast(&app);

    app.update(done("c0", req));
    app.update(failed("c0", req, Failure::new("http_500")));

    assert_eq!(toast(&app), shown);
    assert_eq!(app.session(), &Session::Guest);
}

#[test]
fn after_signing_in_again_the_same_card_can_be_deleted_again() {
    let mut app = log_app_of(fixture_me(), vec![own("c0")]);
    let old = delete(&mut app);
    sign_in_again(&mut app);
    // `sign_in_again` lists `c0` by alice, so a commit of yours is put in its place.
    let effects = app.update(Action::Refresh);
    deliver(&mut app, FOLLOWING, &effects, feed_page(vec![own("c0")]));

    let req = delete(&mut app);

    assert!(req.0 > old.0);
}

#[test]
fn a_guest_without_a_card_is_asked_to_sign_in_by_the_keys() {
    for c in ['d', '!'] {
        let mut app = crate::write_tests::guest_log();
        assert_eq!(app.modal(), None);

        let effects = key(&mut app, c);

        assert_eq!(effects, Vec::<Effect>::new(), "{c}");
        assert_eq!(app.modal(), Some(Modal::SignIn), "{c}");
    }
}

fn preview_of(items: Vec<Value>) -> Value {
    json!({"data": items, "next_cursor": null})
}

#[test]
fn a_copy_inside_a_preview_that_counts_replies_makes_the_commit_stay() {
    let inside = merge(
        commit_json("c1", "myuron", 600),
        json!({"parent_id": "p1", "replies_count": 1}),
    );
    let head = card_with(
        "h1",
        "alice",
        json!({"reply_preview": preview_of(vec![inside])}),
    );
    let parent = card_with("p1", "alice", json!({"replies_count": 3}));
    let mut app = log_app_of(fixture_me(), vec![head, parent]);
    let effects = open_profile(&mut app, "myuron");
    deliver_user(&mut app, &effects, "myuron", user_resp("myuron", json!({})));
    let listed = card_with(
        "c1",
        "myuron",
        json!({"parent_id": "p1", "replies_count": 0}),
    );
    deliver_commits(
        &mut app,
        &effects,
        "myuron",
        ProfileTab::Commits,
        vec![listed],
        None,
    );
    let req = delete(&mut app);

    app.update(done("c1", req));

    assert_eq!(replies_of(&app, FOLLOWING, "p1"), Some(Some(3)));
    let kept = app.feed(FOLLOWING).items[0]
        .reply_preview
        .clone()
        .map(|p| p.data);
    assert!(
        matches!(kept.as_deref(), Some([ThreadItem::Tombstone(t)]) if t.id == "c1"),
        "{kept:?}"
    );
}

#[test]
fn the_count_of_a_parent_goes_down_in_the_copies_inside_previews_too() {
    let inside = merge(commit_json("p1", "alice", 900), json!({"replies_count": 3}));
    let head = card_with(
        "h2",
        "alice",
        json!({"reply_preview": preview_of(vec![inside])}),
    );
    let child = card_with("n1", "myuron", json!({"parent_id": "p1"}));
    let parent = card_with("p1", "alice", json!({"replies_count": 3}));
    let mut app = log_app_of(fixture_me(), vec![child, parent, head]);
    let req = delete(&mut app);

    app.update(done("n1", req));

    assert_eq!(replies_of(&app, FOLLOWING, "p1"), Some(Some(2)));
    let inside = app.feed(FOLLOWING).items[1]
        .reply_preview
        .clone()
        .map(|p| p.data);
    assert!(
        matches!(inside.as_deref(), Some([ThreadItem::Commit(c)]) if c.replies_count == Some(2)),
        "{inside:?}"
    );
}
