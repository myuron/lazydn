//! Starring, saving and giving a pastel to the selected card, and copying or opening its link.
//!
//! - `s` stars, `b` saves and `p` gives a pastel; each acts on the selected card of Log, Saved, a profile list or a
//!   thread. A guest is asked to sign in; a tombstone, an empty list and a session being restored do nothing.
//! - Star and save show their result at once in every copy of the commit and undo it if the server refuses.
//!   A pastel waits for the server, because it spends the daily allowance.
//! - One request per kind and commit is out at a time. The answer of a request that left before the latest
//!   sign-in changes nothing.
//! - `y` copies and `O` opens the link of the selected card; both work for a guest.

use crossterm::event::{KeyCode, KeyEventKind, KeyModifiers};
use serde_json::{Value, json};
use time::{Duration, UtcOffset};

use crate::action::{Action, Effect, Failure};
use crate::app::{App, LogTab, Me, Modal, ProfileTab, Screen, Session, ToastKind};
use crate::feed::{FeedKey, FeedPage, ReqId};
use crate::feed_tests::{FOLLOWING, LATEST, TRENDING, deliver, down, first_req, page, started};
use crate::keymap::map_key;
use crate::model::{Commit, PastelResponse, Quoted, ThreadItem};
use crate::profile_tests::{deliver_commits, deliver_user, open_profile, user_resp};
use crate::test_support::*;
use crate::thread::Entry;
use crate::thread_tests::{
    MAIN, ancestor, commit_json, commit_of, deliver_replies, deliver_thread, item_of, merge,
    open_thread, opened, others, state, tomb,
};

const NOT_APPROVED: &str = "device_not_approved";

// --- helpers ------------------------------------------------------------------

fn key(app: &mut App, c: char) -> Vec<Effect> {
    press(app, KeyCode::Char(c))
}

fn mapped(app: &App, c: char) -> Option<Action> {
    let modifiers = if c.is_uppercase() {
        KeyModifiers::SHIFT
    } else {
        KeyModifiers::NONE
    };
    map_key(
        app,
        key_event(KeyCode::Char(c), modifiers, KeyEventKind::Press),
    )
}

fn card(id: &str, author: &str) -> Commit {
    commit_of(commit_json(id, author, 60))
}

fn card_with(id: &str, author: &str, extra: Value) -> Commit {
    commit_of(merge(commit_json(id, author, 60), extra))
}

fn feed_page(items: Vec<Commit>) -> FeedPage {
    FeedPage {
        items,
        next_cursor: None,
        is_ranked: false,
        suggestions: Vec::new(),
    }
}

fn me_of(app: &App) -> Me {
    match app.session() {
        Session::SignedIn(me) => me.clone(),
        other => panic!("not signed in: {other:?}"),
    }
}

fn toast(app: &App) -> Option<(ToastKind, String)> {
    app.toast().map(|t| (t.kind, t.text.clone()))
}

/// Signed in on Following with `n` commits `c0`.. by alice; `c0` is selected.
fn log_app(n: usize) -> App {
    let (mut app, effects) = started(120, 40);
    deliver(&mut app, FOLLOWING, &effects, page(0, n, None));
    app
}

/// Like `log_app` for the given account.
fn log_app_of(me: Me, items: Vec<Commit>) -> App {
    let mut app = App::new(Session::SignedIn(me), NOW, UtcOffset::UTC);
    let mut effects = app.update(Action::Init);
    effects.extend(app.update(Action::Resize(120, 40)));
    deliver(&mut app, FOLLOWING, &effects, feed_page(items));
    app
}

/// A guest on the Latest tab with the sign-in dialog closed. A guest never fetches the log, so it holds no card.
fn guest_log() -> App {
    let mut app = guest(120, 40);
    app.update(Action::Init);
    app.update(Action::CloseModal);
    app
}

/// A guest on the public profile of alice, which lists the given commits.
fn guest_profile(items: Vec<Commit>) -> App {
    let mut app = guest(120, 40);
    app.update(Action::CloseModal);
    let effects = open_profile(&mut app, "alice");
    deliver_user(&mut app, &effects, "alice", user_resp("alice", json!({})));
    deliver_commits(
        &mut app,
        &effects,
        "alice",
        ProfileTab::Commits,
        items,
        None,
    );
    app
}

/// Saved with `n` commits that are all saved already.
fn saved_app(n: usize) -> App {
    let mut app = signed_in(120, 40);
    let effects = app.update(Action::Goto(Screen::Saved));
    let items = (0..n)
        .map(|i| card_with(&format!("s{i}"), "alice", json!({"is_saved": true})))
        .collect();
    deliver(&mut app, FeedKey::Saved, &effects, feed_page(items));
    app
}

/// Where the same commit is held, by name, so that a missed copy shows in the failure.
fn copies(app: &App, id: &str) -> Vec<(String, Commit)> {
    let mut out = Vec::new();
    for key in FeedKey::ALL {
        let feed = app.feed(key);
        for c in feed.items.iter().chain(&feed.suggestions) {
            if c.id == id {
                out.push((format!("{key:?}"), c.clone()));
            }
        }
    }
    if let Some(profile) = app.profile("alice") {
        let pinned = profile.user.as_ref().and_then(|u| u.pinned_commit.as_ref());
        let lists = [
            ("profile commits", profile.commits.items.iter()),
            ("profile replies", profile.replies.items.iter()),
        ];
        for (name, items) in lists {
            for c in items.filter(|c| c.id == id) {
                out.push((name.to_string(), c.clone()));
            }
        }
        if let Some(c) = pinned.filter(|c| c.id == id) {
            out.push(("pinned".to_string(), c.clone()));
        }
    }
    if let Some(thread) = app.thread(MAIN) {
        for c in thread.main.iter().filter(|c| c.id == id) {
            out.push(("thread main".to_string(), c.clone()));
        }
        let listed = thread.children.values().flat_map(|l| l.items.iter());
        for item in thread.ancestors.iter().chain(listed) {
            if let ThreadItem::Commit(c) = item
                && c.id == id
            {
                out.push(("thread item".to_string(), (**c).clone()));
            }
        }
    }
    out
}

/// `(starred, stars_count)` of every copy.
fn stars(app: &App, id: &str) -> Vec<Option<(Option<bool>, Option<u32>)>> {
    copies(app, id)
        .into_iter()
        .map(|(_, c)| Some((c.starred, c.stars_count)))
        .collect()
}

fn all_copies(app: &App, id: &str, check: impl Fn(&Commit) -> bool) -> bool {
    let copies = copies(app, id);
    !copies.is_empty() && copies.iter().all(|(_, c)| check(c))
}

/// Following, Latest, Trending and Saved with `c0`..`c2`, a profile of alice that lists `c1` in its commits, its
/// replies and as the pinned commit, and the thread of `MAIN` over them whose ancestor `c1` is selected.
/// Eight copies of `c1` are held.
fn everywhere() -> App {
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
            json!({
                "is_following": false,
                "pasteis_received": 10,
                "pinned_commit": commit_json("c1", "alice", 120),
            }),
        ),
    );
    deliver_commits(
        &mut app,
        &effects,
        "alice",
        ProfileTab::Commits,
        vec![card("c0", "alice"), card("c1", "alice")],
        None,
    );
    let effects = key(&mut app, '2');
    deliver_commits(
        &mut app,
        &effects,
        "alice",
        ProfileTab::Replies,
        vec![card("c1", "alice")],
        None,
    );
    key(&mut app, '1');
    app.update(Action::Open(Screen::Log(LogTab::Following)));
    let effects = open_thread(&mut app, MAIN);
    deliver_thread(&mut app, &effects, vec![ancestor("c1", "alice")], 0);
    deliver_replies(&mut app, &effects, MAIN, vec![], None);
    key(&mut app, 'k');
    app
}

/// Following holds `MAIN`, `a1` and `r0`; the thread of `MAIN` with the ancestor `a1` and the given replies is
/// kept in the history under it. `MAIN` is selected.
fn thread_over_log(replies: Vec<ThreadItem>) -> App {
    let (mut app, effects) = started(120, 40);
    let items = vec![card(MAIN, "alice"), card("a1", "alice"), card("r0", "bob")];
    deliver(&mut app, FOLLOWING, &effects, feed_page(items));
    let effects = open_thread(&mut app, MAIN);
    deliver_thread(&mut app, &effects, vec![ancestor("a1", "alice")], 2);
    deliver_replies(&mut app, &effects, MAIN, replies, None);
    app.update(Action::Open(Screen::Log(LogTab::Following)));
    app
}

/// The three writes, so that what they share is tested once for each.
#[derive(Clone, Copy, Debug)]
enum Kind {
    Star,
    Save,
    Pastel,
}

const KINDS: [Kind; 3] = [Kind::Star, Kind::Save, Kind::Pastel];

impl Kind {
    fn key(self) -> char {
        match self {
            Kind::Star => 's',
            Kind::Save => 'b',
            Kind::Pastel => 'p',
        }
    }

    fn action(self) -> Action {
        match self {
            Kind::Star => Action::ToggleStar,
            Kind::Save => Action::ToggleSave,
            Kind::Pastel => Action::GivePastel,
        }
    }

    /// The request the effects carry for this kind, if any.
    fn request(self, effects: &[Effect]) -> Option<ReqId> {
        effects.iter().find_map(|e| match (self, e) {
            (Kind::Star, Effect::StarCommit { req, .. })
            | (Kind::Save, Effect::BookmarkCommit { req, .. })
            | (Kind::Pastel, Effect::GivePastel { req, .. }) => Some(*req),
            _ => None,
        })
    }

    /// The request of the effect that was expected; a missing effect fails here, with the effects that came.
    fn req(self, effects: &[Effect]) -> ReqId {
        let req = self.request(effects);
        assert!(req.is_some(), "no {self:?} request in {effects:?}");
        req.unwrap_or(ReqId(u64::MAX))
    }

    fn failed_with(self, id: &str, req: ReqId, failure: Failure) -> Action {
        let id = id.to_string();
        match self {
            Kind::Star => Action::StarFailed {
                id,
                on: true,
                req,
                failure,
            },
            Kind::Save => Action::BookmarkFailed {
                id,
                on: true,
                req,
                failure,
            },
            Kind::Pastel => Action::PastelFailed { id, req, failure },
        }
    }

    fn failed(self, id: &str, req: ReqId, code: &str) -> Action {
        self.failed_with(id, req, Failure::new(code))
    }

    /// What the server answers: starred, saved, one pastel more and the counts that go with them.
    fn done(self, id: &str, req: ReqId) -> Action {
        let id_owned = id.to_string();
        let commit = server_commit(id);
        match self {
            Kind::Star => Action::StarDone {
                id: id_owned,
                req,
                commit: Box::new(commit),
            },
            Kind::Save => Action::BookmarkDone {
                id: id_owned,
                req,
                commit: Box::new(commit),
            },
            Kind::Pastel => Action::PastelDone {
                id: id_owned,
                req,
                response: Box::new(pastel_response(commit)),
            },
        }
    }
}

fn server_commit(id: &str) -> Commit {
    card_with(
        id,
        "alice",
        json!({"starred": true, "stars_count": 2, "is_saved": true, "pasteis_count": 1}),
    )
}

fn pastel_response(data: Commit) -> PastelResponse {
    PastelResponse {
        data,
        pasteis_remaining: 6,
        pasteis_resets_at: NOW + Duration::hours(2),
        author_pasteis_received: Some(11),
    }
}

fn star_effects(effects: &[Effect]) -> Vec<(String, bool)> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::StarCommit { id, on, .. } => Some((id.clone(), *on)),
            _ => None,
        })
        .collect()
}

fn bookmark_effects(effects: &[Effect]) -> Vec<(String, bool)> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::BookmarkCommit { id, on, .. } => Some((id.clone(), *on)),
            _ => None,
        })
        .collect()
}

fn pastel_effects(effects: &[Effect]) -> Vec<String> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::GivePastel { id, .. } => Some(id.clone()),
            _ => None,
        })
        .collect()
}

fn writes(effects: &[Effect]) -> usize {
    star_effects(effects).len() + bookmark_effects(effects).len() + pastel_effects(effects).len()
}

fn pkce() -> crate::api::pkce::Pkce {
    crate::api::pkce::Pkce {
        verifier: "verifier".into(),
        challenge: "challenge".into(),
    }
}

/// Signs in again after the server refused the device, and loads Following afresh.
fn sign_in_again(app: &mut App) {
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
    type_str(app, &"0123456789abcdef".repeat(4));
    press(app, KeyCode::Enter);
    let response = serde_json::from_value(crate::notifications_tests::me_json(0, json!({})))
        .expect("a current user");
    let signed = app.update(Action::DeviceRegistered {
        response: Box::new(response),
    });
    app.update(Action::FeedLoaded {
        key: FOLLOWING,
        req: first_req(&signed),
        page: page(0, 3, None),
    });
}

/// A write of `kind` on `c0` that went out before the user was refused and signed in again. Returns the
/// request it went out with, which is older than the new sign-in.
fn signed_in_again_after(kind: Kind) -> (App, ReqId) {
    let mut app = log_app(3);
    let req = kind.req(&key(&mut app, kind.key()));
    assert_ne!(req, ReqId(u64::MAX), "the write was not sent");
    sign_in_again(&mut app);
    assert!(matches!(app.session(), Session::SignedIn(_)));
    (app, req)
}

// --- keymap -----------------------------------------------------------------------

#[test]
fn s_b_p_y_and_o_act_on_the_card_of_every_list_with_the_main_pane_focused() {
    let mut saved = signed_in(120, 40);
    saved.update(Action::Goto(Screen::Saved));
    let mut profile = signed_in(120, 40);
    open_profile(&mut profile, "alice");
    let scenes = [
        ("log", log_app(3)),
        ("saved", saved),
        ("thread", thread_over_log(vec![])),
        ("profile", profile),
    ];
    for (name, mut app) in scenes {
        if name == "thread" {
            let effects = open_thread(&mut app, MAIN);
            assert!(effects.is_empty(), "the thread is known");
        }
        assert_eq!(mapped(&app, 's'), Some(Action::ToggleStar), "{name}");
        assert_eq!(mapped(&app, 'b'), Some(Action::ToggleSave), "{name}");
        assert_eq!(mapped(&app, 'p'), Some(Action::GivePastel), "{name}");
        assert_eq!(mapped(&app, 'y'), Some(Action::CopyLink), "{name}");
        assert_eq!(mapped(&app, 'O'), Some(Action::OpenInBrowser), "{name}");
    }
}

#[test]
fn the_card_keys_are_not_bound_on_screens_without_cards_or_with_the_nav_focused() {
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
    let control = log_app(3);
    assert_eq!(mapped(&control, 's'), Some(Action::ToggleStar));
    let mut nav = log_app(3);
    press(&mut nav, KeyCode::Tab);
    scenes.push(("nav focus".to_string(), nav));
    for (name, app) in scenes {
        for c in ['s', 'b', 'y', 'O'] {
            assert_eq!(mapped(&app, c), None, "{name} {c}");
        }
    }
}

#[test]
fn g_then_s_and_g_then_p_still_go_to_settings_and_the_profile() {
    let mut app = log_app(3);
    key(&mut app, 'g');
    assert_eq!(
        mapped(&app, 's'),
        Some(Action::Goto(Screen::Settings)),
        "g s"
    );
    assert_eq!(
        mapped(&app, 'p'),
        Some(Action::Goto(Screen::Profile("myuron".into()))),
        "g p"
    );
}

// --- star: the press -----------------------------------------------------------------

#[test]
fn s_stars_the_selected_card_in_every_copy_and_asks_the_server_once() {
    let mut app = everywhere();
    assert_eq!(copies(&app, "c1").len(), 8, "the fixture holds 8 copies");

    let effects = key(&mut app, 's');

    assert_eq!(star_effects(&effects), [("c1".to_string(), true)]);
    assert_eq!(effects.len(), 1, "{effects:?}");
    for (place, c) in copies(&app, "c1") {
        assert_eq!((c.starred, c.stars_count), (Some(true), Some(2)), "{place}");
    }
    assert_eq!(stars(&app, "c0")[0], Some((Some(false), Some(1))));
}

#[test]
fn s_on_a_card_that_is_starred_unstars_it() {
    let items = vec![card_with(
        "c0",
        "alice",
        json!({"starred": true, "stars_count": 4}),
    )];
    let mut app = log_app_of(fixture_me(), items);

    let effects = key(&mut app, 's');

    assert_eq!(star_effects(&effects), [("c0".to_string(), false)]);
    assert_eq!(stars(&app, "c0"), [Some((Some(false), Some(3)))]);
}

#[test]
fn s_reaches_the_copies_in_a_thread_kept_in_the_history() {
    let mut app = thread_over_log(others(0, 1));
    for (index, id) in [MAIN, "a1", "r0"].into_iter().enumerate() {
        let effects = key(&mut app, 's');
        assert_eq!(star_effects(&effects), [(id.to_string(), true)], "{id}");
        let places = copies(&app, id);
        assert_eq!(places.len(), 2, "{id}: a log copy and a thread copy");
        for (place, c) in places {
            assert_eq!(
                (c.starred, c.stars_count),
                (Some(true), Some(2)),
                "{id} {place}"
            );
        }
        if index < 2 {
            down(&mut app, 1);
        }
    }
}

#[test]
fn a_count_that_was_not_loaded_stays_not_loaded_and_a_zero_count_stays_zero() {
    let items = vec![
        card_with("c0", "alice", json!({"stars_count": null})),
        card_with("c1", "alice", json!({"starred": true, "stars_count": 0})),
    ];
    let mut app = log_app_of(fixture_me(), items);

    key(&mut app, 's');
    down(&mut app, 1);
    key(&mut app, 's');

    assert_eq!(stars(&app, "c0"), [Some((Some(true), None))]);
    assert_eq!(stars(&app, "c1"), [Some((Some(false), Some(0)))]);
}

#[test]
fn a_second_s_while_the_first_is_out_does_nothing_and_another_kind_or_card_is_free() {
    let mut app = log_app(3);
    key(&mut app, 's');

    assert_eq!(writes(&key(&mut app, 's')), 0);
    assert_eq!(stars(&app, "c0"), [Some((Some(true), Some(2)))]);
    let saved = key(&mut app, 'b');
    assert_eq!(bookmark_effects(&saved), [("c0".to_string(), true)]);
    down(&mut app, 1);
    let other = key(&mut app, 's');
    assert_eq!(star_effects(&other), [("c1".to_string(), true)]);
}

#[test]
fn after_the_answer_s_asks_again_and_after_a_failure_too() {
    let mut app = log_app(3);
    let req = Kind::Star.req(&key(&mut app, 's'));
    app.update(Kind::Star.done("c0", req));

    let again = key(&mut app, 's');

    assert_eq!(star_effects(&again), [("c0".to_string(), false)]);
    let req = Kind::Star.req(&again);
    app.update(Action::StarFailed {
        id: "c0".into(),
        on: false,
        req,
        failure: Failure::new("network"),
    });
    let third = key(&mut app, 's');
    assert_eq!(star_effects(&third), [("c0".to_string(), false)]);
}

// --- star: the answer -------------------------------------------------------------------

#[test]
fn the_answer_puts_the_counts_of_the_server_in_every_copy() {
    let mut app = everywhere();
    let req = Kind::Star.req(&key(&mut app, 's'));
    let mut commit = server_commit("c1");
    commit.stars_count = Some(5);

    app.update(Action::StarDone {
        id: "c1".into(),
        req,
        commit: Box::new(commit),
    });

    assert_eq!(copies(&app, "c1").len(), 8);
    for (place, c) in copies(&app, "c1") {
        assert_eq!(
            (c.starred, c.stars_count, c.is_saved, c.pasteis_count),
            (Some(true), Some(5), Some(true), Some(1)),
            "{place}"
        );
    }
}

#[test]
fn the_answer_keeps_the_preview_and_the_parent_and_takes_the_quote_and_the_reply_count() {
    let rich = card_with(
        "c0",
        "alice",
        json!({
            "replies_count": 1,
            "quoted_commit_id": "q0",
            "quoted_commit": commit_json("q0", "carol", 500),
            "reply_preview": {"data": [commit_json("p1", "bob", 30)], "next_cursor": null},
            "parent": commit_json("pp", "dave", 90),
        }),
    );
    let before = rich.clone();
    let mut app = log_app_of(fixture_me(), vec![rich]);
    let req = Kind::Star.req(&key(&mut app, 's'));
    let answer = card_with(
        "c0",
        "alice",
        json!({
            "starred": true,
            "stars_count": 5,
            "replies_count": 4,
            "quoted_commit_id": "q0",
            "quoted_commit": commit_json("q0", "erin", 500),
        }),
    );

    app.update(Action::StarDone {
        id: "c0".into(),
        req,
        commit: Box::new(answer.clone()),
    });

    let held = app.feed(FOLLOWING).items[0].clone();
    assert_eq!(held.reply_preview, before.reply_preview);
    assert!(held.reply_preview.is_some());
    assert_eq!(held.parent, before.parent);
    assert!(held.parent.is_some());
    assert_eq!(held.quoted_commit, answer.quoted_commit);
    assert_ne!(held.quoted_commit, before.quoted_commit);
    assert!(matches!(held.quoted_commit, Quoted::Item(_)));
    assert_eq!(held.replies_count, Some(4));
    assert_eq!((held.starred, held.stars_count), (Some(true), Some(5)));
}

#[test]
fn the_answer_leaves_a_tombstone_copy_as_it_is() {
    let mut app = thread_over_log(vec![tomb("r0")]);
    down(&mut app, 2);
    let req = Kind::Star.req(&key(&mut app, 's'));

    app.update(Kind::Star.done("r0", req));

    let children = state(&app, MAIN).children;
    assert_eq!(children[MAIN].items, vec![tomb("r0")]);
    assert_eq!(stars(&app, "r0"), [Some((Some(true), Some(2)))]);
}

#[test]
fn an_answer_with_another_request_id_is_dropped_and_the_star_stays_out() {
    let mut app = log_app(3);
    let req = Kind::Star.req(&key(&mut app, 's'));
    let wrong = ReqId(req.0 + 50);

    app.update(Kind::Star.done("c0", wrong));
    app.update(Kind::Star.failed("c0", wrong, "network"));

    assert_eq!(stars(&app, "c0"), [Some((Some(true), Some(2)))]);
    assert_eq!(toast(&app), None);
    assert_eq!(
        writes(&key(&mut app, 's')),
        0,
        "the right answer is awaited"
    );
    app.update(Kind::Star.done("c0", req));
    assert_eq!(writes(&key(&mut app, 's')), 1);
}

// --- star: the refusal -----------------------------------------------------------------

#[test]
fn a_refused_star_puts_every_copy_back_and_shows_the_error() {
    let mut app = everywhere();
    let req = Kind::Star.req(&key(&mut app, 's'));

    app.update(Kind::Star.failed("c1", req, "http_500"));

    assert_eq!(copies(&app, "c1").len(), 8);
    for (place, c) in copies(&app, "c1") {
        assert_eq!(
            (c.starred, c.stars_count),
            (Some(false), Some(1)),
            "{place}"
        );
    }
    assert_eq!(toast(&app), Some((ToastKind::Error, "http_500".into())));
}

#[test]
fn the_undo_leaves_a_copy_that_a_reload_has_replaced_in_the_meantime() {
    let mut app = log_app(3);
    let fetch = type_str(&mut app, "2");
    deliver(&mut app, LATEST, &fetch, page(0, 3, None));
    type_str(&mut app, "1");
    let req = Kind::Star.req(&key(&mut app, 's'));
    let refresh = key(&mut app, 'R');
    let mut fresh = page(0, 3, None);
    fresh.items[0].stars_count = Some(7);
    deliver(&mut app, FOLLOWING, &refresh, fresh);
    let reloaded = &app.feed(FOLLOWING).items[0];
    assert_eq!(
        (reloaded.starred, reloaded.stars_count),
        (Some(false), Some(7))
    );
    let latest = &app.feed(LATEST).items[0];
    assert_eq!((latest.starred, latest.stars_count), (Some(true), Some(2)));

    app.update(Kind::Star.failed("c0", req, "http_500"));

    let latest = &app.feed(LATEST).items[0];
    assert_eq!((latest.starred, latest.stars_count), (Some(false), Some(1)));
    let following = &app.feed(FOLLOWING).items[0];
    assert_eq!(
        (following.starred, following.stars_count),
        (Some(false), Some(7)),
        "the reloaded copy was never changed, so it is not changed back"
    );
}

#[test]
fn the_undo_never_takes_a_count_below_zero_and_leaves_a_count_that_is_not_loaded_alone() {
    let items = vec![
        card_with("c0", "alice", json!({"stars_count": null})),
        card_with("c1", "alice", json!({"stars_count": 0})),
    ];
    let mut app = log_app_of(fixture_me(), items);
    let first = Kind::Star.req(&key(&mut app, 's'));
    down(&mut app, 1);
    let second = Kind::Star.req(&key(&mut app, 's'));
    // A copy is told from a reloaded one by its value alone, so a reload that brought the same value is undone too.
    // A reload brings in copies that are already starred; the undo still turns them off.
    let refresh = key(&mut app, 'R');
    let fresh = feed_page(vec![
        card_with("c0", "alice", json!({"starred": true, "stars_count": null})),
        card_with("c1", "alice", json!({"starred": true, "stars_count": 0})),
    ]);
    deliver(&mut app, FOLLOWING, &refresh, fresh);

    app.update(Kind::Star.failed("c0", first, "http_500"));
    app.update(Kind::Star.failed("c1", second, "http_500"));

    assert_eq!(stars(&app, "c0"), [Some((Some(false), None))]);
    assert_eq!(stars(&app, "c1"), [Some((Some(false), Some(0)))]);
}

#[test]
fn what_the_server_says_about_a_refusal_is_cleaned_and_cut_for_the_toast() {
    let long = "x".repeat(100);
    let cases: Vec<(Failure, String)> = vec![
        (
            Failure {
                code: "http_403".into(),
                retry_after: None,
                message: Some("You cannot star this commit.".into()),
            },
            "You cannot star this commit.".into(),
        ),
        (
            Failure {
                code: "http_403".into(),
                retry_after: None,
                message: Some("bad\x1b[31m\x07 news".into()),
            },
            "bad[31m news".into(),
        ),
        (
            Failure {
                code: "http_403".into(),
                retry_after: None,
                message: Some(long.clone()),
            },
            "x".repeat(64),
        ),
        (Failure::new("http_500"), "http_500".into()),
        (
            Failure::new("commit_unavailable"),
            "This commit isn't available.".into(),
        ),
        (
            Failure {
                code: "rate_limited".into(),
                retry_after: Some(5),
                message: Some("Too Many Attempts.".into()),
            },
            "rate_limited: retry in 5s".into(),
        ),
    ];
    for kind in KINDS {
        for (failure, text) in &cases {
            let mut app = log_app(3);
            let req = kind.req(&key(&mut app, kind.key()));

            app.update(kind.failed_with("c0", req, failure.clone()));

            assert_eq!(
                toast(&app),
                Some((ToastKind::Error, text.clone())),
                "{kind:?} {failure:?}"
            );
        }
    }
}

#[test]
fn a_rejected_device_in_an_answer_returns_to_guest_for_every_kind() {
    for kind in KINDS {
        let mut app = log_app(3);
        let req = kind.req(&key(&mut app, kind.key()));

        app.update(kind.failed("c0", req, NOT_APPROVED));

        assert_eq!(app.session(), &Session::Guest, "{kind:?}");
        assert_eq!(app.modal(), Some(Modal::SignIn), "{kind:?}");
        assert_eq!(app.feed(FOLLOWING).items.len(), 0, "{kind:?}");
        assert_eq!(
            toast(&app),
            Some((
                ToastKind::Error,
                "this device is no longer approved — sign in again".into()
            )),
            "{kind:?}"
        );
    }
}

#[test]
fn a_rejection_for_a_guest_changes_nothing() {
    for kind in KINDS {
        let mut app = guest_log();

        app.update(kind.failed("c0", ReqId(3), NOT_APPROVED));

        assert_eq!(app.modal(), None, "{kind:?}");
        assert_eq!(toast(&app), None, "{kind:?}");
        assert_eq!(app.session(), &Session::Guest, "{kind:?}");
    }
}

// --- the sign-in boundary -------------------------------------------------------------------

#[test]
fn an_answer_from_before_the_new_sign_in_changes_nothing_for_every_kind() {
    for kind in KINDS {
        let (mut app, old) = signed_in_again_after(kind);
        let before = app.feed(FOLLOWING).items.clone();
        // The refusal that preceded the new sign-in left its toast up; the old answers must not change it.
        let shown = toast(&app);

        app.update(kind.done("c0", old));
        app.update(kind.failed("c0", old, "http_500"));
        app.update(kind.failed("c0", old, NOT_APPROVED));

        assert!(matches!(app.session(), Session::SignedIn(_)), "{kind:?}");
        assert_eq!(app.feed(FOLLOWING).items, before, "{kind:?}");
        assert_eq!(toast(&app), shown, "{kind:?}");
        assert_eq!(me_of(&app).pasteis_left, 7, "{kind:?}");
    }
}

#[test]
fn after_signing_in_again_the_same_card_can_be_written_to_again_for_every_kind() {
    for kind in KINDS {
        let (mut app, old) = signed_in_again_after(kind);

        let effects = key(&mut app, kind.key());

        let req = kind.request(&effects);
        assert!(req.is_some_and(|r| r.0 > old.0), "{kind:?}: {effects:?}");
    }
}

#[test]
fn a_rejection_with_a_newer_request_id_after_the_new_sign_in_still_ends_the_session() {
    let (mut app, _) = signed_in_again_after(Kind::Star);
    let req = Kind::Star.req(&key(&mut app, 's'));

    app.update(Kind::Star.failed("c0", req, NOT_APPROVED));

    assert_eq!(app.session(), &Session::Guest);
}

// --- nothing to write to ------------------------------------------------------------------------

#[test]
fn s_b_and_p_do_nothing_on_your_own_commit_except_that_it_can_be_saved() {
    let own = card_with("o0", "myuron", json!({}));
    let mut app = log_app_of(fixture_me(), vec![own]);

    assert_eq!(writes(&key(&mut app, 's')), 0, "star");
    assert_eq!(writes(&key(&mut app, 'p')), 0, "pastel");
    assert_eq!(stars(&app, "o0"), [Some((Some(false), Some(1)))]);
    let saved = key(&mut app, 'b');
    assert_eq!(bookmark_effects(&saved), [("o0".to_string(), true)]);
}

#[test]
fn nothing_is_written_for_a_tombstone_or_an_empty_list() {
    let mut app = thread_over_log(vec![tomb("r0")]);
    app.update(Action::Open(Screen::Thread(MAIN.into())));
    // The main commit above takes a star, so the silence below is that of the tombstone.
    assert_eq!(
        star_effects(&key(&mut app, 's')),
        [(MAIN.to_string(), true)]
    );
    down(&mut app, 1);
    let thread = state(&app, MAIN);
    let selected = thread.entries(MAIN, false).into_iter().nth(thread.selected);
    assert!(
        matches!(&selected, Some(Entry::Reply(id, _)) if id == "r0"),
        "{selected:?}"
    );
    for c in ['s', 'b', 'p', 'y', 'O'] {
        assert_eq!(key(&mut app, c), Vec::<Effect>::new(), "tombstone {c}");
    }
    let mut empty = log_app(0);
    for c in ['s', 'b', 'p', 'y', 'O'] {
        assert_eq!(key(&mut empty, c), Vec::<Effect>::new(), "empty {c}");
    }
}

#[test]
fn a_guest_is_asked_to_sign_in_by_s_b_and_p_with_a_card_selected() {
    for kind in KINDS {
        let mut app = guest_profile(vec![card("c0", "alice")]);
        assert_eq!(app.modal(), None);
        let before = app.profile("alice").map(|p| p.commits.items.clone());

        let effects = app.update(kind.action());

        assert_eq!(app.modal(), Some(Modal::SignIn), "{kind:?}");
        assert_eq!(writes(&effects), 0, "{kind:?}");
        let after = app.profile("alice").map(|p| p.commits.items.clone());
        assert_eq!(after, before, "{kind:?}");
    }
}

#[test]
fn a_session_being_restored_does_nothing_for_every_card_key() {
    let mut app = App::new(Session::Restoring, NOW, UtcOffset::UTC);
    app.update(Action::Resize(120, 40));
    app.update(Action::Init);
    let actions = [
        Action::ToggleStar,
        Action::ToggleSave,
        Action::GivePastel,
        Action::CopyLink,
        Action::OpenInBrowser,
    ];
    for action in actions {
        assert_eq!(
            app.update(action.clone()),
            Vec::<Effect>::new(),
            "{action:?}"
        );
        assert_eq!(app.modal(), None, "{action:?}");
    }
}

// --- save -----------------------------------------------------------------------------------

#[test]
fn b_saves_the_selected_card_in_every_copy_and_asks_the_server() {
    let mut app = everywhere();

    let effects = key(&mut app, 'b');

    assert_eq!(bookmark_effects(&effects), [("c1".to_string(), true)]);
    assert_eq!(effects.len(), 1, "{effects:?}");
    assert_eq!(copies(&app, "c1").len(), 8);
    for (place, c) in copies(&app, "c1") {
        assert_eq!(
            (c.is_saved, c.starred, c.stars_count),
            (Some(true), Some(false), Some(1)),
            "{place}"
        );
    }
}

#[test]
fn a_refused_save_puts_every_copy_back_and_shows_the_error() {
    let mut app = everywhere();
    let req = Kind::Save.req(&key(&mut app, 'b'));

    app.update(Kind::Save.failed("c1", req, "http_500"));

    assert!(all_copies(&app, "c1", |c| c.is_saved == Some(false)));
    assert_eq!(toast(&app), Some((ToastKind::Error, "http_500".into())));
}

#[test]
fn the_answer_to_a_save_is_applied_to_every_copy_without_a_toast() {
    let mut app = everywhere();
    let req = Kind::Save.req(&key(&mut app, 'b'));

    app.update(Kind::Save.done("c1", req));

    assert!(all_copies(&app, "c1", |c| {
        (c.is_saved, c.stars_count) == (Some(true), Some(2))
    }));
    assert_eq!(toast(&app), None);
}

#[test]
fn the_answer_to_a_star_shows_no_toast() {
    let mut app = log_app(3);
    let req = Kind::Star.req(&key(&mut app, 's'));

    app.update(Kind::Star.done("c0", req));

    assert_eq!(stars(&app, "c0"), [Some((Some(true), Some(2)))]);
    assert_eq!(toast(&app), None);
}

#[test]
fn unsaving_in_saved_keeps_the_card_in_the_list_and_the_selection_where_it_was() {
    let mut app = saved_app(3);
    down(&mut app, 1);

    let effects = key(&mut app, 'b');
    let req = Kind::Save.req(&effects);
    let mut answer = server_commit("s1");
    answer.is_saved = Some(false);
    app.update(Action::BookmarkDone {
        id: "s1".into(),
        req,
        commit: Box::new(answer),
    });

    assert_eq!(bookmark_effects(&effects), [("s1".to_string(), false)]);
    let ids: Vec<_> = app
        .feed(FeedKey::Saved)
        .items
        .iter()
        .map(|c| c.id.clone())
        .collect();
    assert_eq!(ids, ["s0", "s1", "s2"]);
    assert_eq!(app.feed(FeedKey::Saved).selected, 1);
    let text = screen_text(&app);
    // The Nav item of Saved shows the open box as well.
    assert_eq!(text.matches('⊡').count(), 2, "{text}");
    assert_eq!(text.matches('⊠').count(), 2, "{text}");
}

#[test]
fn saving_a_card_does_not_add_it_to_a_saved_list_that_is_loaded() {
    let mut app = log_app(3);
    let effects = app.update(Action::Goto(Screen::Saved));
    let items = vec![card_with("s0", "alice", json!({"is_saved": true}))];
    deliver(&mut app, FeedKey::Saved, &effects, feed_page(items));
    app.update(Action::Open(Screen::Log(LogTab::Following)));

    let req = Kind::Save.req(&key(&mut app, 'b'));
    app.update(Kind::Save.done("c0", req));

    assert_eq!(app.feed(FOLLOWING).items[0].is_saved, Some(true));
    assert_eq!(app.feed(FeedKey::Saved).items.len(), 1);
    assert_eq!(app.feed(FeedKey::Saved).items[0].id, "s0");
}

fn screen_text(app: &App) -> String {
    let terminal = render(app);
    let buffer = terminal.backend().buffer().clone();
    (0..buffer.area.height)
        .map(|y| row_text(&buffer, y))
        .collect::<Vec<_>>()
        .join("\n")
}

// --- pastel -------------------------------------------------------------------------------------

#[test]
fn p_asks_the_server_and_leaves_the_card_alone_until_it_answers() {
    let mut app = log_app(3);
    let before = app.feed(FOLLOWING).items.clone();

    let effects = key(&mut app, 'p');

    assert_eq!(pastel_effects(&effects), ["c0"]);
    assert_eq!(effects.len(), 1, "{effects:?}");
    assert_eq!(app.feed(FOLLOWING).items, before);
    assert_eq!(writes(&key(&mut app, 'p')), 0, "one pastel at a time");
    down(&mut app, 1);
    assert_eq!(pastel_effects(&key(&mut app, 'p')), ["c1"]);
}

#[test]
fn p_does_nothing_when_the_pasteis_mode_is_off() {
    let me = Me {
        pasteis_mode: false,
        ..fixture_me()
    };
    let mut app = log_app_of(me, vec![card("c0", "alice")]);

    assert_eq!(key(&mut app, 'p'), Vec::<Effect>::new());
    // The card itself is fine, so the silence is that of the mode.
    assert_eq!(bookmark_effects(&key(&mut app, 'b')).len(), 1);
}

#[test]
fn p_asks_even_when_the_allowance_is_known_to_be_used_up() {
    let me = Me {
        pasteis_left: 0,
        ..fixture_me()
    };
    let mut app = log_app_of(me, vec![card("c0", "alice")]);

    assert_eq!(pastel_effects(&key(&mut app, 'p')), ["c0"]);
}

#[test]
fn a_given_pastel_updates_every_copy_the_allowance_and_the_toast() {
    let mut app = everywhere();
    let req = Kind::Pastel.req(&key(&mut app, 'p'));

    app.update(Kind::Pastel.done("c1", req));

    assert_eq!(copies(&app, "c1").len(), 8);
    assert!(all_copies(&app, "c1", |c| c.pasteis_count == Some(1)));
    let me = me_of(&app);
    assert_eq!(me.pasteis_left, 6);
    assert_eq!(me.pasteis_resets_at, NOW + Duration::hours(2));
    assert_eq!(
        toast(&app),
        Some((ToastKind::Success, "Gave a pastel (6 left today)".into()))
    );
}

#[test]
fn a_given_pastel_raises_the_total_of_the_author_when_the_profile_is_loaded() {
    let mut app = everywhere();
    let req = Kind::Pastel.req(&key(&mut app, 'p'));

    app.update(Kind::Pastel.done("c1", req));

    let received = app
        .profile("alice")
        .and_then(|p| p.user.as_ref())
        .and_then(|u| u.pasteis_received);
    assert_eq!(received, Some(11));
}

#[test]
fn an_answer_without_the_total_of_the_author_leaves_the_profile_as_it_is() {
    let mut app = everywhere();
    let req = Kind::Pastel.req(&key(&mut app, 'p'));
    let response = PastelResponse {
        author_pasteis_received: None,
        ..pastel_response(server_commit("c1"))
    };

    app.update(Action::PastelDone {
        id: "c1".into(),
        req,
        response: Box::new(response),
    });

    let received = app
        .profile("alice")
        .and_then(|p| p.user.as_ref())
        .and_then(|u| u.pasteis_received);
    assert_eq!(received, Some(10));
}

#[test]
fn an_answer_to_an_old_pastel_request_is_dropped() {
    let mut app = log_app(3);
    let req = Kind::Pastel.req(&key(&mut app, 'p'));

    app.update(Kind::Pastel.done("c0", ReqId(req.0 + 50)));

    assert_eq!(me_of(&app).pasteis_left, 7);
    assert_eq!(toast(&app), None);
    assert_eq!(app.feed(FOLLOWING).items[0].pasteis_count, Some(0));
}

#[test]
fn an_exhausted_allowance_says_so_with_the_time_left_and_empties_the_count() {
    let mut app = log_app(3);
    let req = Kind::Pastel.req(&key(&mut app, 'p'));

    app.update(Kind::Pastel.failed("c0", req, "pasteis_exhausted"));

    assert_eq!(me_of(&app).pasteis_left, 0);
    assert_eq!(
        toast(&app),
        Some((
            ToastKind::Error,
            "No pastéis left today (resets in 12h)".into()
        ))
    );
    assert_eq!(app.feed(FOLLOWING).items[0].pasteis_count, Some(0));
}

#[test]
fn the_reset_time_is_taken_from_the_account_in_minutes_or_whole_hours_and_left_out_when_it_has_passed()
 {
    let cases = [
        (Duration::minutes(40), Some("40m")),
        (Duration::seconds(20), Some("1m")),
        (Duration::hours(2) + Duration::minutes(30), Some("2h")),
        (Duration::hours(3), Some("3h")),
        (Duration::minutes(-5), None),
    ];
    for (after, left) in cases {
        let me = Me {
            pasteis_resets_at: NOW + after,
            ..fixture_me()
        };
        let text = match left {
            Some(left) => format!("No pastéis left today (resets in {left})"),
            None => "No pastéis left today".to_string(),
        };
        let mut app = log_app_of(me, vec![card("c0", "alice")]);
        let req = Kind::Pastel.req(&key(&mut app, 'p'));

        app.update(Kind::Pastel.failed("c0", req, "pasteis_exhausted"));

        assert_eq!(
            toast(&app),
            Some((ToastKind::Error, text.clone())),
            "{text}"
        );
    }
}

#[test]
fn another_refusal_of_a_pastel_leaves_the_allowance_and_allows_asking_again() {
    let mut app = log_app(3);
    let req = Kind::Pastel.req(&key(&mut app, 'p'));

    app.update(Kind::Pastel.failed("c0", req, "http_403"));

    assert_eq!(me_of(&app).pasteis_left, 7);
    assert_eq!(toast(&app), Some((ToastKind::Error, "http_403".into())));
    assert_eq!(pastel_effects(&key(&mut app, 'p')), ["c0"]);
}

// --- y and O ------------------------------------------------------------------------------------

const LINK: &str = "https://dev-null.sh/@alice/commits/c0";

fn linked(id: &str) -> Commit {
    card_with(
        id,
        "alice",
        json!({"html_url": format!("https://dev-null.sh/@alice/commits/{id}")}),
    )
}

fn link_of(id: &str) -> String {
    format!("https://dev-null.sh/@alice/commits/{id}")
}

#[test]
fn y_asks_the_runtime_to_copy_the_link_and_o_to_open_it_on_every_list() {
    let mut saved = signed_in(120, 40);
    let effects = saved.update(Action::Goto(Screen::Saved));
    deliver(
        &mut saved,
        FeedKey::Saved,
        &effects,
        feed_page(vec![linked("c0")]),
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
        vec![linked("c0")],
        None,
    );
    let reply = item_of(merge(
        commit_json("r0", "bob", 600),
        json!({"parent_id": MAIN, "html_url": link_of("r0")}),
    ));
    let mut thread = opened(120, 40, vec![], 1, vec![reply], None);
    key(&mut thread, 'j');
    let scenes = [
        (
            "log",
            log_app_of(fixture_me(), vec![linked("c0")]),
            link_of("c0"),
        ),
        ("saved", saved, link_of("c0")),
        ("profile", profile, link_of("c0")),
        ("thread", thread, link_of("r0")),
    ];
    for (name, mut app, url) in scenes {
        assert_eq!(
            key(&mut app, 'y'),
            [Effect::CopyLink(url.clone())],
            "{name}"
        );
        assert_eq!(key(&mut app, 'O'), [Effect::OpenUrl(url)], "{name}");
        assert_eq!(toast(&app), None, "{name}: the runtime reports the copy");
    }
}

#[test]
fn y_and_o_work_for_a_guest_without_the_dialog() {
    let mut app = guest_profile(vec![linked("c0")]);
    assert_eq!(app.modal(), None);

    assert_eq!(key(&mut app, 'y'), [Effect::CopyLink(LINK.into())]);
    assert_eq!(key(&mut app, 'O'), [Effect::OpenUrl(LINK.into())]);
    assert_eq!(app.modal(), None);
}

#[test]
fn y_and_o_do_nothing_for_a_commit_without_a_link() {
    let mut app = log_app(3);

    // The card is selectable, so the silence is that of the missing link.
    assert_eq!(star_effects(&key(&mut app, 's')).len(), 1);
    assert_eq!(key(&mut app, 'y'), Vec::<Effect>::new());
    assert_eq!(key(&mut app, 'O'), Vec::<Effect>::new());
}

#[test]
fn y_and_o_work_on_your_own_commit() {
    let own = card_with(
        "o0",
        "myuron",
        json!({"html_url": "https://dev-null.sh/@myuron/commits/o0"}),
    );
    let mut app = log_app_of(fixture_me(), vec![own]);

    assert_eq!(
        key(&mut app, 'y'),
        [Effect::CopyLink(
            "https://dev-null.sh/@myuron/commits/o0".into()
        )]
    );
}

fn profile_with(user_url: Option<&str>, cards: Vec<Commit>) -> App {
    let mut app = signed_in(120, 40);
    let effects = open_profile(&mut app, "alice");
    let mut response = user_resp("alice", json!({"is_following": false}));
    response.data.html_url = user_url.map(String::from);
    deliver_user(&mut app, &effects, "alice", response);
    deliver_commits(
        &mut app,
        &effects,
        "alice",
        ProfileTab::Commits,
        cards,
        None,
    );
    app
}

#[test]
fn o_on_a_profile_without_cards_opens_the_page_of_the_user() {
    let mut app = profile_with(Some("https://dev-null.sh/@alice"), vec![]);

    assert_eq!(
        key(&mut app, 'O'),
        [Effect::OpenUrl("https://dev-null.sh/@alice".into())]
    );
}

#[test]
fn o_on_a_profile_with_cards_opens_the_selected_commit_and_not_the_user() {
    let mut app = profile_with(Some("https://dev-null.sh/@alice"), vec![linked("c0")]);

    assert_eq!(key(&mut app, 'O'), [Effect::OpenUrl(link_of("c0"))]);
}

#[test]
fn o_on_a_profile_without_cards_and_without_a_user_link_does_nothing() {
    let mut control = profile_with(Some("https://dev-null.sh/@alice"), vec![]);
    assert_eq!(key(&mut control, 'O').len(), 1, "with a link it opens");
    let mut loaded = profile_with(None, vec![]);
    let mut unloaded = signed_in(120, 40);
    open_profile(&mut unloaded, "alice");

    assert_eq!(key(&mut loaded, 'O'), Vec::<Effect>::new());
    assert_eq!(key(&mut unloaded, 'O'), Vec::<Effect>::new());
}

#[test]
fn an_exhausted_allowance_gets_the_fixed_wording_whatever_message_the_server_sent() {
    let mut app = log_app(3);
    let req = Kind::Pastel.req(&key(&mut app, 'p'));
    let failure = Failure {
        code: "pasteis_exhausted".into(),
        retry_after: None,
        message: Some("you ran out of pastéis. wait until tomorrow.".into()),
    };

    app.update(Kind::Pastel.failed_with("c0", req, failure));

    assert_eq!(
        toast(&app),
        Some((
            ToastKind::Error,
            "No pastéis left today (resets in 12h)".into()
        ))
    );
}

#[test]
fn a_write_answer_that_arrives_after_the_return_to_guest_leaves_the_toast_and_the_session() {
    let mut app = log_app(3);
    let req = Kind::Star.req(&key(&mut app, 's'));
    app.update(Kind::Star.failed("c0", req, NOT_APPROVED));
    let shown = toast(&app);
    assert_eq!(app.session(), &Session::Guest);
    assert_eq!(
        shown,
        Some((
            ToastKind::Error,
            "this device is no longer approved — sign in again".into()
        ))
    );

    app.update(Kind::Star.failed("c0", req, "http_500"));
    // The ids of the requests do not matter here: every answer is for a session that is gone.
    app.update(Kind::Pastel.done("c0", req));
    app.update(Kind::Star.done("c0", req));

    assert_eq!(toast(&app), shown);
    assert_eq!(app.session(), &Session::Guest);
    assert_eq!(app.feed(FOLLOWING).items.len(), 0);
}

#[test]
fn o_on_a_blocked_profile_opens_the_page_of_the_user_and_not_a_card() {
    let mut app = profile_with(Some("https://dev-null.sh/@alice"), vec![linked("c0")]);
    let effects = key(&mut app, 'R');
    app.update(Action::UserFailed {
        username: "alice".into(),
        req: crate::profile_tests::user_req(&effects),
        failure: Failure::new("user_blocked"),
        blocked_user: Some(Box::new(crate::profile_tests::user_of(
            crate::thread_tests::user_json("alice"),
        ))),
    });
    let shown = app
        .profile("alice")
        .is_some_and(|p| p.user.is_some() && !p.shows_list());
    assert!(shown, "the profile is blocked but its user is known");

    assert_eq!(
        key(&mut app, 'O'),
        [Effect::OpenUrl("https://dev-null.sh/@alice".into())]
    );
}

#[test]
fn o_on_a_blocked_profile_that_never_loaded_opens_the_page_of_the_blocked_user() {
    let mut app = signed_in(120, 40);
    let effects = open_profile(&mut app, "alice");
    let mut blocked = crate::profile_tests::user_of(crate::thread_tests::user_json("alice"));
    blocked.html_url = Some("https://dev-null.sh/@alice".into());
    app.update(Action::UserFailed {
        username: "alice".into(),
        req: crate::profile_tests::user_req(&effects),
        failure: Failure::new("user_blocked"),
        blocked_user: Some(Box::new(blocked)),
    });
    assert!(app.profile("alice").is_some_and(|p| p.user.is_none()));

    assert_eq!(
        key(&mut app, 'O'),
        [Effect::OpenUrl("https://dev-null.sh/@alice".into())]
    );
}

#[test]
fn the_answer_to_a_star_leaves_the_save_that_is_still_out_and_a_later_refusal_undoes_it() {
    let mut app = everywhere();
    let star = Kind::Star.req(&key(&mut app, 's'));
    let save = Kind::Save.req(&key(&mut app, 'b'));
    let mut answer = server_commit("c1");
    answer.is_saved = Some(false);
    answer.stars_count = Some(5);

    app.update(Action::StarDone {
        id: "c1".into(),
        req: star,
        commit: Box::new(answer),
    });

    assert!(all_copies(&app, "c1", |c| c.is_saved == Some(true)));
    assert!(all_copies(&app, "c1", |c| {
        (c.starred, c.stars_count) == (Some(true), Some(5))
    }));
    app.update(Kind::Save.failed("c1", save, "http_500"));
    assert!(all_copies(&app, "c1", |c| c.is_saved == Some(false)));
    assert!(all_copies(&app, "c1", |c| c.starred == Some(true)));
}

#[test]
fn the_answer_to_a_save_leaves_the_star_that_is_still_out() {
    let mut app = log_app(3);
    let star = Kind::Star.req(&key(&mut app, 's'));
    let save = Kind::Save.req(&key(&mut app, 'b'));
    let mut answer = server_commit("c0");
    answer.starred = Some(false);
    answer.stars_count = Some(1);

    app.update(Action::BookmarkDone {
        id: "c0".into(),
        req: save,
        commit: Box::new(answer),
    });

    assert_eq!(stars(&app, "c0"), [Some((Some(true), Some(2)))]);
    app.update(Kind::Star.failed("c0", star, "http_500"));
    assert_eq!(stars(&app, "c0"), [Some((Some(false), Some(1)))]);
}

#[test]
fn the_answer_to_a_pastel_leaves_the_star_that_is_still_out() {
    let mut app = log_app(3);
    let pastel = Kind::Pastel.req(&key(&mut app, 'p'));
    key(&mut app, 's');
    let mut answer = server_commit("c0");
    answer.starred = Some(false);
    answer.stars_count = Some(1);

    app.update(Action::PastelDone {
        id: "c0".into(),
        req: pastel,
        response: Box::new(pastel_response(answer)),
    });

    assert_eq!(stars(&app, "c0"), [Some((Some(true), Some(2)))]);
    assert_eq!(app.feed(FOLLOWING).items[0].pasteis_count, Some(1));
}
