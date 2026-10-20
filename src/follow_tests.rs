//! Following, muting and blocking a person from their profile, and following from Followers / Following.
//!
//! - On a profile, `f` follows or unfollows, `m` mutes or unmutes and `B` blocks. On Followers / Following, `f`
//!   follows or unfollows the person on the selected row; `m` and `B` are not bound there.
//! - A follow shows its result at once in every copy of the person (the profile, the rows of both lists, the
//!   counts of your own profile and of the Aside) and is undone if the server refuses. A mute shows the flag at
//!   once and is undone if the server refuses. Neither answer replaces what is held: a follow answers with no
//!   body, and a mute answers with a person without counts, so only its flag is read.
//! - A block asks first. The answer shows the profile as blocked, takes the follow back in both directions
//!   (counts of you), clears the follow mark of the person in the lists, and takes their commits out of every
//!   Log tab (items and suggestions) and Saved, keeping each selection on the card it was on. Other screens
//!   show the person's commits until they are loaded again.
//! - `B` on a blocked profile unblocks at once and loads the profile again.
//! - One write per kind and person is out at a time, whatever the case of the name. While a block is out, the
//!   same person takes neither `f` nor `m`.
//! - Your own profile, a profile that is loading or unavailable, and a blocked profile take no `f` or `m`.
//!   A guest is asked to sign in; a session being restored does nothing.
//! - A refusal is shown as a toast and undone. A device the server no longer approves ends the session; an
//!   answer that left before the latest sign-in changes nothing.

use crossterm::event::{KeyCode, KeyEventKind, KeyModifiers};
use serde_json::{Value, json};

use crate::action::{Action, Effect, Failure};
use crate::app::{App, ConnTab, Modal, ProfileTab, Screen, Session, ToastKind};
use crate::feed::{FeedKey, FeedPage, ReqId};
use crate::feed_tests::{FOLLOWING, LATEST, TRENDING, deliver, down, started};
use crate::keymap::map_key;
use crate::model::{BlockResponse, MuteResponse, User, UserResponse};
use crate::profile_tests::{
    deliver_commits, deliver_conns, deliver_user, followers, full_user, key, open_profile, posts,
    profile_with, user_fetches, user_of, user_req,
};
use crate::test_support::*;
use crate::thread_tests::{
    MAIN, ancestor, deliver_replies, deliver_thread, merge, open_thread, state, user_json,
};
use crate::write_tests::{card, feed_page, guest_profile, mapped, me_of, sign_in_again, toast};

const NOT_APPROVED: &str = "device_not_approved";

// --- fixtures -----------------------------------------------------------------------------------

/// Your own profile as the server holds it: 10 following and 12 followers, like the Aside.
fn own_resp() -> UserResponse {
    let data = full_user(
        "myuron",
        json!({"following_count": 10, "followers_count": 12}),
    );
    serde_json::from_value(json!({ "data": data })).unwrap()
}

/// The profile of `name` with the flags of someone who is neither followed nor muted.
fn resp_of(name: &str, user_extra: Value, extra: Value) -> UserResponse {
    let flags = json!({"is_following": false, "is_muting": false, "follows_you": false});
    let top = merge(flags, extra);
    let value = merge(json!({ "data": full_user(name, user_extra) }), top);
    serde_json::from_value(value).unwrap()
}

/// Signed in as myuron on the loaded profile of alice (3647 followers, not followed, not muted), with your own
/// profile loaded under it in the history.
fn alice_app(user_extra: Value, extra: Value) -> App {
    let mut app = signed_in(120, 40);
    let effects = open_profile(&mut app, "myuron");
    deliver_user(&mut app, &effects, "myuron", own_resp());
    deliver_commits(
        &mut app,
        &effects,
        "myuron",
        ProfileTab::Commits,
        posts("o", 0, 1, "myuron"),
        None,
    );
    profile_with(
        app,
        "alice",
        resp_of("alice", user_extra, extra),
        posts("p", 0, 3, "alice"),
        None,
    )
}

fn plain_alice() -> App {
    alice_app(json!({}), json!({}))
}

fn held(app: &App, name: &str) -> UserResponse {
    app.profile(name)
        .and_then(|s| s.user.clone())
        .unwrap_or_else(|| panic!("no loaded profile for {name}"))
}

/// `(following_count, followers_count)` of the person on their profile.
fn counts(app: &App, name: &str) -> (Option<u64>, Option<u64>) {
    let user = held(app, name).data;
    (user.following_count, user.followers_count)
}

fn me_counts(app: &App) -> (u32, u32) {
    let me = me_of(app);
    (me.following, me.followers)
}

fn person(name: &str, extra: Value) -> User {
    user_of(merge(user_json(name), extra))
}

/// Followers of alice: bob (followed), carol, dave and you; bob is selected.
fn people() -> Vec<User> {
    vec![
        person("bob", json!({"is_followed": true, "follows_you": true})),
        person("carol", json!({"is_followed": false})),
        person("dave", json!({"is_followed": false})),
        person("myuron", json!({"is_followed": false})),
    ]
}

fn people_app() -> App {
    followers(120, 30, "alice", people(), None)
}

fn row_followed(app: &App, name: &str, tab: ConnTab, who: &str) -> Option<bool> {
    app.connections(name, tab)
        .and_then(|s| s.users.iter().find(|u| u.username == who))
        .and_then(|u| u.is_followed)
}

fn row_state(app: &App, tab: ConnTab, who: &str) -> Option<bool> {
    row_followed(app, "alice", tab, who)
}

/// The profile of bob, then the Followers of bob, where carol is not followed. Your own profile is not loaded.
fn carol_everywhere() -> App {
    let mut app = signed_in(120, 40);
    let effects = open_profile(&mut app, "carol");
    deliver_user(
        &mut app,
        &effects,
        "carol",
        resp_of("carol", json!({"followers_count": 50}), json!({})),
    );
    let effects = open_profile(&mut app, "bob");
    deliver_user(
        &mut app,
        &effects,
        "bob",
        resp_of("bob", json!({}), json!({})),
    );
    let effects = key(&mut app, '3');
    deliver_conns(
        &mut app,
        &effects,
        "bob",
        ConnTab::Followers,
        vec![person("carol", json!({"is_followed": false}))],
        None,
    );
    let effects = key(&mut app, '2');
    deliver_conns(
        &mut app,
        &effects,
        "bob",
        ConnTab::Following,
        vec![person("carol", json!({"is_followed": false}))],
        None,
    );
    app
}

// --- effects and answers --------------------------------------------------------------------------

/// The three writes, so that what they share is tested once for each.
#[derive(Clone, Copy, Debug)]
enum Write {
    Follow,
    Mute,
    Block,
}

const WRITES: [Write; 3] = [Write::Follow, Write::Mute, Write::Block];

impl Write {
    /// Presses what it takes to send the write; a block is confirmed when it asks.
    fn press(self, app: &mut App) -> Vec<Effect> {
        match self {
            Write::Follow => key(app, 'f'),
            Write::Mute => key(app, 'm'),
            Write::Block => {
                let mut effects = key(app, 'B');
                if app.modal() == Some(Modal::ConfirmBlock) {
                    effects.extend(key(app, 'y'));
                }
                effects
            }
        }
    }

    /// `(username, on, req)` of the request of this kind among the effects.
    fn request(self, effects: &[Effect]) -> Option<(String, bool, ReqId)> {
        effects.iter().find_map(|e| match (self, e) {
            (Write::Follow, Effect::FollowUser { username, on, req })
            | (Write::Mute, Effect::MuteUser { username, on, req })
            | (Write::Block, Effect::BlockUser { username, on, req }) => {
                Some((username.clone(), *on, *req))
            }
            _ => None,
        })
    }

    /// `(on, req)` of the request that was expected; a missing one fails here, with the effects that came.
    fn sent(self, effects: &[Effect]) -> (bool, ReqId) {
        let request = self.request(effects);
        assert!(request.is_some(), "no {self:?} request in {effects:?}");
        request.map_or((true, ReqId(u64::MAX)), |(_, on, req)| (on, req))
    }

    /// The request that was expected; a missing one fails here, with the effects that came.
    fn req(self, effects: &[Effect]) -> ReqId {
        let request = self.request(effects);
        assert!(request.is_some(), "no {self:?} request in {effects:?}");
        request.map_or(ReqId(u64::MAX), |(_, _, req)| req)
    }

    fn failed_with(self, name: &str, on: bool, req: ReqId, failure: Failure) -> Action {
        let username = name.to_string();
        match self {
            Write::Follow => Action::FollowFailed {
                username,
                on,
                req,
                failure,
            },
            Write::Mute => Action::MuteFailed {
                username,
                on,
                req,
                failure,
            },
            Write::Block => Action::BlockFailed {
                username,
                on,
                req,
                failure,
            },
        }
    }

    fn failed(self, name: &str, on: bool, req: ReqId, code: &str) -> Action {
        self.failed_with(name, on, req, Failure::new(code))
    }

    /// What the server answers, with the flag it reports now.
    fn done(self, name: &str, on: bool, req: ReqId) -> Action {
        let username = name.to_string();
        let data = person(name, json!({}));
        match self {
            Write::Follow => Action::FollowDone { username, on, req },
            Write::Mute => Action::MuteDone {
                username,
                on,
                req,
                response: Box::new(MuteResponse {
                    data,
                    is_muting: on,
                }),
            },
            Write::Block => Action::BlockDone {
                username,
                on,
                req,
                response: Box::new(BlockResponse {
                    data,
                    is_blocking: on,
                }),
            },
        }
    }
}

fn follow_effects(effects: &[Effect]) -> Vec<(String, bool)> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::FollowUser { username, on, .. } => Some((username.clone(), *on)),
            _ => None,
        })
        .collect()
}

fn mute_effects(effects: &[Effect]) -> Vec<(String, bool)> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::MuteUser { username, on, .. } => Some((username.clone(), *on)),
            _ => None,
        })
        .collect()
}

fn block_effects(effects: &[Effect]) -> Vec<(String, bool)> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::BlockUser { username, on, .. } => Some((username.clone(), *on)),
            _ => None,
        })
        .collect()
}

fn user_writes(effects: &[Effect]) -> usize {
    follow_effects(effects).len() + mute_effects(effects).len() + block_effects(effects).len()
}

/// Presses `c` after checking that it is bound on this screen, so that a silent result is the answer of the app
/// and not an unbound key.
fn bound_key(app: &mut App, c: char) -> Vec<Effect> {
    assert!(
        mapped(app, c).is_some(),
        "{c} is not bound on {:?}",
        app.screen()
    );
    key(app, c)
}

fn event_action(app: &App, code: KeyCode, modifiers: KeyModifiers) -> Option<Action> {
    map_key(app, key_event(code, modifiers, KeyEventKind::Press))
}

/// Blocks alice from her loaded profile and delivers the answer.
fn blocked_alice(is_following: bool, follows_you: bool) -> App {
    let mut app = alice_app(
        json!({}),
        json!({"is_following": is_following, "follows_you": follows_you}),
    );
    let req = Write::Block.req(&Write::Block.press(&mut app));
    app.update(Write::Block.done("alice", true, req));
    app
}

/// The profile of bob as the server answers when bob is blocked.
fn blocked_bob_app() -> App {
    let mut app = signed_in(120, 40);
    let effects = open_profile(&mut app, "bob");
    app.update(Action::UserFailed {
        username: "bob".into(),
        req: user_req(&effects),
        failure: Failure::new("user_blocked"),
        blocked_user: Some(Box::new(person("bob", json!({})))),
    });
    app
}

fn shows_block(app: &App, name: &str) -> bool {
    app.profile(name)
        .is_some_and(|s| s.error.as_deref() == Some("user_blocked"))
}

// --- keymap ---------------------------------------------------------------------------------------

#[test]
fn f_m_and_b_are_bound_on_a_profile_with_the_main_pane_focused() {
    let app = plain_alice();

    assert_eq!(mapped(&app, 'f'), Some(Action::ToggleFollow));
    assert_eq!(mapped(&app, 'm'), Some(Action::ToggleMute));
    assert_eq!(mapped(&app, 'B'), Some(Action::Block));
}

#[test]
fn f_is_bound_on_followers_and_following_and_m_and_b_are_not() {
    let mut app = people_app();
    for _ in 0..2 {
        assert_eq!(mapped(&app, 'f'), Some(Action::ToggleFollow));
        assert_eq!(mapped(&app, 'm'), None);
        assert_eq!(mapped(&app, 'B'), None);
        key(&mut app, '2');
    }
}

#[test]
fn f_m_and_b_are_not_bound_on_other_screens_or_with_the_nav_focused() {
    let mut scenes = Vec::new();
    for screen in [
        Screen::Notifications,
        Screen::Settings,
        Screen::Saved,
        Screen::Thread(MAIN.into()),
    ] {
        let mut app = signed_in(120, 40);
        app.update(Action::Open(screen));
        scenes.push(app);
    }
    let mut nav = plain_alice();
    assert_eq!(mapped(&nav, 'f'), Some(Action::ToggleFollow));
    press(&mut nav, KeyCode::Tab);
    scenes.push(nav);
    for app in scenes {
        for c in ['f', 'm', 'B'] {
            assert_eq!(mapped(&app, c), None, "{:?} {c}", app.screen());
        }
    }
}

#[test]
fn the_block_confirmation_reads_y_n_esc_and_ctrl_c_and_nothing_else() {
    let mut app = plain_alice();
    key(&mut app, 'B');
    assert_eq!(app.modal(), Some(Modal::ConfirmBlock));

    assert_eq!(mapped(&app, 'y'), Some(Action::ConfirmBlock));
    assert_eq!(mapped(&app, 'n'), Some(Action::CloseModal));
    let none = KeyModifiers::NONE;
    assert_eq!(
        event_action(&app, KeyCode::Esc, none),
        Some(Action::CloseModal)
    );
    assert_eq!(
        event_action(&app, KeyCode::Char('c'), KeyModifiers::CONTROL),
        Some(Action::Quit)
    );
    for c in ['f', 'm', 'B', 'j', 'x', 'Q'] {
        assert_eq!(mapped(&app, c), None, "{c}");
    }
    assert_eq!(event_action(&app, KeyCode::Enter, none), None);
}

// --- follow on a profile --------------------------------------------------------------------------

#[test]
fn f_follows_at_once_in_the_profile_the_counts_and_the_aside_and_asks_the_server() {
    let mut app = plain_alice();

    let effects = key(&mut app, 'f');

    assert_eq!(follow_effects(&effects), [("alice".to_string(), true)]);
    assert_eq!(effects.len(), 1, "{effects:?}");
    assert_eq!(held(&app, "alice").is_following, Some(true));
    assert_eq!(counts(&app, "alice"), (Some(87), Some(3648)));
    assert_eq!(me_counts(&app), (11, 12));
    assert_eq!(counts(&app, "myuron"), (Some(11), Some(12)));
}

#[test]
fn f_on_a_followed_profile_unfollows() {
    let mut app = alice_app(json!({}), json!({"is_following": true}));

    let effects = key(&mut app, 'f');

    assert_eq!(follow_effects(&effects), [("alice".to_string(), false)]);
    assert_eq!(held(&app, "alice").is_following, Some(false));
    assert_eq!(counts(&app, "alice"), (Some(87), Some(3646)));
    assert_eq!(me_counts(&app), (9, 12));
    assert_eq!(counts(&app, "myuron"), (Some(9), Some(12)));
}

#[test]
fn a_profile_that_does_not_say_whether_it_is_followed_counts_as_not_followed() {
    let mut app = alice_app(
        json!({"followers_count": null}),
        json!({"is_following": null}),
    );

    let effects = key(&mut app, 'f');

    assert_eq!(follow_effects(&effects), [("alice".to_string(), true)]);
    assert_eq!(held(&app, "alice").is_following, Some(true));
    assert_eq!(counts(&app, "alice"), (Some(87), None));
    assert_eq!(me_counts(&app), (11, 12));
}

#[test]
fn unfollowing_never_takes_a_count_below_zero() {
    let mut app = alice_app(json!({"followers_count": 0}), json!({"is_following": true}));

    key(&mut app, 'f');

    assert_eq!(held(&app, "alice").is_following, Some(false));
    assert_eq!(counts(&app, "alice"), (Some(87), Some(0)));
}

#[test]
fn the_answer_of_a_follow_changes_nothing_and_frees_the_key() {
    let mut app = plain_alice();
    let req = Write::Follow.req(&key(&mut app, 'f'));
    let before = (held(&app, "alice"), me_counts(&app), counts(&app, "myuron"));

    app.update(Write::Follow.done("alice", true, req));

    assert_eq!(
        (held(&app, "alice"), me_counts(&app), counts(&app, "myuron")),
        before
    );
    assert_eq!(toast(&app), None);
    let again = key(&mut app, 'f');
    assert_eq!(follow_effects(&again), [("alice".to_string(), false)]);
}

#[test]
fn a_refused_follow_puts_back_the_flag_and_every_count() {
    let mut app = plain_alice();
    let req = Write::Follow.req(&key(&mut app, 'f'));

    app.update(Write::Follow.failed("alice", true, req, "http_500"));

    assert_eq!(held(&app, "alice").is_following, Some(false));
    assert_eq!(counts(&app, "alice"), (Some(87), Some(3647)));
    assert_eq!(me_counts(&app), (10, 12));
    assert_eq!(counts(&app, "myuron"), (Some(10), Some(12)));
    assert_eq!(toast(&app), Some((ToastKind::Error, "http_500".into())));
}

#[test]
fn a_refused_unfollow_puts_back_the_flag_and_every_count() {
    let mut app = alice_app(json!({}), json!({"is_following": true}));
    let req = Write::Follow.req(&key(&mut app, 'f'));

    app.update(Write::Follow.failed("alice", false, req, "http_500"));

    assert_eq!(held(&app, "alice").is_following, Some(true));
    assert_eq!(counts(&app, "alice"), (Some(87), Some(3647)));
    assert_eq!(me_counts(&app), (10, 12));
    assert_eq!(counts(&app, "myuron"), (Some(10), Some(12)));
}

#[test]
fn a_refusal_puts_back_only_the_copies_that_still_hold_what_the_press_set() {
    let mut app = plain_alice();
    let req = Write::Follow.req(&key(&mut app, 'f'));
    // The profile was loaded again before the refusal and, as the server had not acted yet, says "not followed".
    let reload = key(&mut app, 'R');
    app.update(Action::UserLoaded {
        username: "alice".into(),
        req: user_req(&reload),
        response: Box::new(resp_of(
            "alice",
            json!({"followers_count": 3700}),
            json!({}),
        )),
    });

    app.update(Write::Follow.failed("alice", true, req, "http_500"));

    assert_eq!(held(&app, "alice").is_following, Some(false));
    assert_eq!(counts(&app, "alice"), (Some(87), Some(3700)));
    assert_eq!(me_counts(&app), (10, 12));
}

// --- mute on a profile ----------------------------------------------------------------------------

#[test]
fn m_mutes_at_once_and_leaves_the_counts_alone() {
    let mut app = plain_alice();

    let effects = key(&mut app, 'm');

    assert_eq!(mute_effects(&effects), [("alice".to_string(), true)]);
    assert_eq!(effects.len(), 1, "{effects:?}");
    assert_eq!(held(&app, "alice").is_muting, Some(true));
    assert_eq!(counts(&app, "alice"), (Some(87), Some(3647)));
    assert_eq!(me_counts(&app), (10, 12));
}

#[test]
fn m_on_a_muted_profile_unmutes() {
    let mut app = alice_app(json!({}), json!({"is_muting": true}));

    let effects = key(&mut app, 'm');

    assert_eq!(mute_effects(&effects), [("alice".to_string(), false)]);
    assert_eq!(held(&app, "alice").is_muting, Some(false));
}

#[test]
fn the_answer_of_a_mute_sets_the_flag_and_keeps_everything_else_that_is_held() {
    let mut app = plain_alice();
    let follow = Write::Follow.req(&key(&mut app, 'f'));
    app.update(Write::Follow.done("alice", true, follow));
    let req = Write::Mute.req(&key(&mut app, 'm'));
    let before = held(&app, "alice");

    // The answer carries a person without counts and with another bio.
    app.update(Action::MuteDone {
        username: "alice".into(),
        on: true,
        req,
        response: Box::new(MuteResponse {
            data: person("alice", json!({"bio": "replaced"})),
            is_muting: true,
        }),
    });

    let after = held(&app, "alice");
    assert_eq!(after.is_muting, Some(true));
    assert_eq!(after.is_following, Some(true));
    assert_eq!(after.data, before.data);
    assert_eq!(counts(&app, "alice"), (Some(87), Some(3648)));
    assert_eq!(toast(&app), None);
}

#[test]
fn the_flag_of_a_mute_answer_wins_over_the_press() {
    let mut app = plain_alice();
    let req = Write::Mute.req(&key(&mut app, 'm'));

    app.update(Write::Mute.done("alice", false, req));

    assert_eq!(held(&app, "alice").is_muting, Some(false));
}

#[test]
fn a_refused_mute_puts_the_flag_back() {
    let mut app = plain_alice();
    let req = Write::Mute.req(&key(&mut app, 'm'));

    app.update(Write::Mute.failed("alice", true, req, "http_500"));

    assert_eq!(held(&app, "alice").is_muting, Some(false));
    assert_eq!(toast(&app), Some((ToastKind::Error, "http_500".into())));
}

#[test]
fn muting_leaves_the_commits_of_every_list_where_they_are() {
    let mut app = started_with_alice_in_the_log();
    let req = Write::Mute.req(&key(&mut app, 'm'));

    app.update(Write::Mute.done("alice", true, req));

    assert_eq!(ids(&app, FOLLOWING), ["c0", "c1"]);
    assert_eq!(app.profile("alice").map(|p| p.commits.items.len()), Some(3));
}

/// Following holds a commit of alice and one of bob, and alice's profile is open over it.
fn started_with_alice_in_the_log() -> App {
    let (mut app, effects) = started(120, 40);
    deliver(
        &mut app,
        FOLLOWING,
        &effects,
        feed_page(vec![card("c0", "alice"), card("c1", "bob")]),
    );
    profile_with(
        app,
        "alice",
        resp_of("alice", json!({}), json!({})),
        posts("p", 0, 3, "alice"),
        None,
    )
}

// --- the same request twice, and one after the other ----------------------------------------------

#[test]
fn a_second_press_while_the_request_is_out_sends_nothing_for_every_kind() {
    for write in WRITES {
        let mut app = plain_alice();
        write.req(&write.press(&mut app));
        let before = held(&app, "alice");

        let again = write.press(&mut app);

        assert_eq!(write.request(&again), None, "{write:?}");
        assert_eq!(app.modal(), None, "{write:?}");
        assert_eq!(held(&app, "alice"), before, "{write:?}");
    }
}

#[test]
fn a_press_after_the_answer_sends_the_opposite_for_follow_and_mute() {
    for write in [Write::Follow, Write::Mute] {
        let mut app = plain_alice();
        let req = write.req(&write.press(&mut app));
        app.update(write.done("alice", true, req));

        let again = write.press(&mut app);

        assert_eq!(
            write.request(&again).map(|(name, on, _)| (name, on)),
            Some(("alice".to_string(), false)),
            "{write:?}"
        );
    }
}

#[test]
fn follow_and_mute_of_the_same_person_can_be_out_together() {
    let mut app = plain_alice();

    let follow = key(&mut app, 'f');
    let mute = key(&mut app, 'm');

    assert_eq!(follow_effects(&follow).len(), 1);
    assert_eq!(mute_effects(&mute).len(), 1);
}

#[test]
fn while_a_block_is_out_the_same_person_takes_neither_f_nor_m_nor_another_b() {
    let mut app = plain_alice();
    let req = Write::Block.req(&Write::Block.press(&mut app));

    assert_eq!(key(&mut app, 'f'), Vec::<Effect>::new());
    assert_eq!(key(&mut app, 'm'), Vec::<Effect>::new());
    assert_eq!(key(&mut app, 'B'), Vec::<Effect>::new());
    assert_eq!(app.modal(), None);
    assert_eq!(held(&app, "alice").is_following, Some(false));
    assert_eq!(held(&app, "alice").is_muting, Some(false));

    app.update(Write::Block.failed("alice", true, req, "http_500"));

    let freed = key(&mut app, 'f');
    assert_eq!(follow_effects(&freed), [("alice".to_string(), true)]);
}

#[test]
fn the_requests_in_flight_are_told_by_the_name_in_lower_case() {
    let mut app = signed_in(120, 40);
    let effects = open_profile(&mut app, "Alice");
    deliver_user(
        &mut app,
        &effects,
        "Alice",
        resp_of("alice", json!({}), json!({})),
    );
    assert_eq!(follow_effects(&key(&mut app, 'f')).len(), 1);
    let effects = open_profile(&mut app, "bob");
    deliver_user(
        &mut app,
        &effects,
        "bob",
        resp_of("bob", json!({}), json!({})),
    );
    let effects = key(&mut app, '3');
    deliver_conns(
        &mut app,
        &effects,
        "bob",
        ConnTab::Followers,
        vec![person("alice", json!({"is_followed": false}))],
        None,
    );

    let again = key(&mut app, 'f');

    assert_eq!(again, Vec::<Effect>::new());
    assert_eq!(
        row_followed(&app, "bob", ConnTab::Followers, "alice"),
        Some(false)
    );
}

// --- block ----------------------------------------------------------------------------------------

#[test]
fn b_asks_first_and_sends_nothing() {
    let mut app = plain_alice();
    let before = held(&app, "alice");

    let effects = key(&mut app, 'B');

    assert_eq!(effects, Vec::<Effect>::new());
    assert_eq!(app.modal(), Some(Modal::ConfirmBlock));
    assert_eq!(held(&app, "alice"), before);
}

#[test]
fn n_and_esc_cancel_and_nothing_goes_out() {
    for cancel in [KeyCode::Char('n'), KeyCode::Esc] {
        let mut app = plain_alice();
        let before = held(&app, "alice");
        key(&mut app, 'B');

        let effects = press(&mut app, cancel);

        assert_eq!(user_writes(&effects), 0, "{cancel:?}");
        assert_eq!(app.modal(), None, "{cancel:?}");
        assert_eq!(held(&app, "alice"), before, "{cancel:?}");
        assert_eq!(app.screen(), Screen::Profile("alice".into()), "{cancel:?}");
        // A new press asks again, so nothing is left out.
        key(&mut app, 'B');
        assert_eq!(app.modal(), Some(Modal::ConfirmBlock), "{cancel:?}");
    }
}

#[test]
fn y_blocks_and_the_profile_waits_for_the_answer() {
    let mut app = plain_alice();
    key(&mut app, 'B');
    let before = held(&app, "alice");

    let effects = key(&mut app, 'y');

    assert_eq!(block_effects(&effects), [("alice".to_string(), true)]);
    assert_eq!(effects.len(), 1, "{effects:?}");
    assert_eq!(app.modal(), None);
    assert_eq!(held(&app, "alice"), before);
    assert!(!shows_block(&app, "alice"));
}

#[test]
fn a_second_confirmation_after_the_dialog_closed_sends_nothing() {
    let mut app = plain_alice();
    Write::Block.req(&Write::Block.press(&mut app));

    assert_eq!(app.update(Action::ConfirmBlock), Vec::<Effect>::new());
}

#[test]
fn the_answer_shows_the_profile_as_blocked_with_the_person_the_server_sent() {
    let mut app = plain_alice();
    let req = Write::Block.req(&Write::Block.press(&mut app));
    let sent = person("alice", json!({"bio": "from the answer"}));

    app.update(Action::BlockDone {
        username: "alice".into(),
        on: true,
        req,
        response: Box::new(BlockResponse {
            data: sent.clone(),
            is_blocking: true,
        }),
    });

    let state = app.profile("alice").cloned().unwrap_or_default();
    assert_eq!(state.error.as_deref(), Some("user_blocked"));
    assert_eq!(state.blocked_user, Some(sent));
    assert!(!state.shows_list());
    assert_eq!(toast(&app), None);
}

#[test]
fn a_blocked_profile_takes_no_list_key_and_no_f_or_m() {
    let mut app = blocked_alice(false, false);

    for c in ['j', 'f', 'm', 's', 'r'] {
        assert_eq!(key(&mut app, c), Vec::<Effect>::new(), "{c}");
    }
    assert_eq!(press(&mut app, KeyCode::Enter), Vec::<Effect>::new());
    assert_eq!(app.screen(), Screen::Profile("alice".into()));
    assert_eq!(app.modal(), None);
}

#[test]
fn the_answer_of_a_block_takes_the_follow_back_in_both_directions() {
    // (you follow them, they follow you) -> (following, followers) of you
    let cases = [
        (true, true, (9, 11)),
        (true, false, (9, 12)),
        (false, true, (10, 11)),
        (false, false, (10, 12)),
    ];
    for (is_following, follows_you, want) in cases {
        let app = blocked_alice(is_following, follows_you);

        assert_eq!(me_counts(&app), want, "{is_following} {follows_you}");
        assert_eq!(
            counts(&app, "myuron"),
            (Some(u64::from(want.0)), Some(u64::from(want.1))),
            "{is_following} {follows_you}"
        );
    }
}

#[test]
fn the_answer_of_a_block_clears_the_follow_mark_on_the_rows_of_the_lists() {
    let mut app = signed_in(120, 40);
    let effects = open_profile(&mut app, "bob");
    deliver_user(
        &mut app,
        &effects,
        "bob",
        resp_of("bob", json!({}), json!({})),
    );
    let effects = key(&mut app, '3');
    deliver_conns(
        &mut app,
        &effects,
        "bob",
        ConnTab::Followers,
        vec![
            person("alice", json!({"is_followed": true})),
            person("carol", json!({"is_followed": true})),
        ],
        None,
    );
    let effects = press(&mut app, KeyCode::Enter);
    deliver_user(
        &mut app,
        &effects,
        "alice",
        resp_of("alice", json!({}), json!({"is_following": true})),
    );
    assert_eq!(app.screen(), Screen::Profile("alice".into()));
    let req = Write::Block.req(&Write::Block.press(&mut app));

    app.update(Write::Block.done("alice", true, req));

    assert_eq!(
        row_followed(&app, "bob", ConnTab::Followers, "alice"),
        Some(false)
    );
    assert_eq!(
        row_followed(&app, "bob", ConnTab::Followers, "carol"),
        Some(true)
    );
}

#[test]
fn an_answer_with_the_flag_off_for_a_block_changes_nothing() {
    let mut app = started_with_alice_in_the_log();
    let req = Write::Block.req(&Write::Block.press(&mut app));
    let before = held(&app, "alice");

    app.update(Write::Block.done("alice", false, req));

    assert_eq!(held(&app, "alice"), before);
    assert!(!shows_block(&app, "alice"));
    assert_eq!(ids(&app, FOLLOWING), ["c0", "c1"]);
    assert_eq!(me_counts(&app), (10, 12));
    // The block did not happen, so the key asks again.
    let again = Write::Block.press(&mut app);
    assert_eq!(block_effects(&again), [("alice".to_string(), true)]);
}

// --- what a block takes out of the lists ----------------------------------------------------------

fn ids(app: &App, key: FeedKey) -> Vec<String> {
    app.feed(key).items.iter().map(|c| c.id.clone()).collect()
}

fn suggested(app: &App, key: FeedKey) -> Vec<String> {
    app.feed(key)
        .suggestions
        .iter()
        .map(|c| c.id.clone())
        .collect()
}

fn selected_id(app: &App, key: FeedKey) -> Option<String> {
    let feed = app.feed(key);
    feed.items.get(feed.selected).map(|c| c.id.clone())
}

/// Every list holds commits of alice and of others, with a selection in each; alice's profile is open over Saved.
///
/// Following `c0 c1 c2 c3 c4`, `c3` selected; Latest `l0 l1 l2 l3` with suggestions `s0 s1`, `l3` selected;
/// Trending `t0 t1`, `t1` selected; Saved `v0 v1 v2`, `v0` selected. The commits of alice are the odd `c`, the
/// even `l`, `s0`, `t1`, `v0` and `v2`.
fn crowded() -> App {
    let (mut app, effects) = started(120, 40);
    let following = vec![
        card("c0", "bob"),
        card("c1", "alice"),
        card("c2", "bob"),
        card("c3", "alice"),
        card("c4", "carol"),
    ];
    deliver(&mut app, FOLLOWING, &effects, feed_page(following));
    down(&mut app, 3);
    let fetch = type_str(&mut app, "2");
    let latest = FeedPage {
        items: vec![
            card("l0", "alice"),
            card("l1", "bob"),
            card("l2", "alice"),
            card("l3", "bob"),
        ],
        next_cursor: None,
        is_ranked: false,
        suggestions: vec![card("s0", "alice"), card("s1", "bob")],
    };
    deliver(&mut app, LATEST, &fetch, latest);
    down(&mut app, 3);
    let fetch = type_str(&mut app, "3");
    deliver(
        &mut app,
        TRENDING,
        &fetch,
        feed_page(vec![card("t0", "carol"), card("t1", "alice")]),
    );
    down(&mut app, 1);
    let fetch = app.update(Action::Open(Screen::Saved));
    let saved = vec![card("v0", "alice"), card("v1", "bob"), card("v2", "alice")];
    deliver(&mut app, FeedKey::Saved, &fetch, feed_page(saved));
    profile_with(
        app,
        "alice",
        resp_of("alice", json!({}), json!({})),
        posts("p", 0, 2, "alice"),
        None,
    )
}

#[test]
fn the_answer_of_a_block_takes_the_commits_out_of_every_log_tab_its_suggestions_and_saved() {
    let mut app = crowded();
    assert_eq!(selected_id(&app, FOLLOWING).as_deref(), Some("c3"));
    assert_eq!(selected_id(&app, LATEST).as_deref(), Some("l3"));
    let req = Write::Block.req(&Write::Block.press(&mut app));

    app.update(Write::Block.done("alice", true, req));

    assert_eq!(ids(&app, FOLLOWING), ["c0", "c2", "c4"]);
    assert_eq!(ids(&app, LATEST), ["l1", "l3"]);
    assert_eq!(suggested(&app, LATEST), ["s1"]);
    assert_eq!(ids(&app, TRENDING), ["t0"]);
    assert_eq!(ids(&app, FeedKey::Saved), ["v1"]);
}

#[test]
fn the_selection_of_every_list_stays_on_its_card_or_moves_to_the_one_that_takes_its_place() {
    let mut app = crowded();
    let req = Write::Block.req(&Write::Block.press(&mut app));

    app.update(Write::Block.done("alice", true, req));

    // c1 above and c3 itself went: the next card, c4, takes the place.
    assert_eq!(selected_id(&app, FOLLOWING).as_deref(), Some("c4"));
    // l0 and l2 above went: the card is the same.
    assert_eq!(selected_id(&app, LATEST).as_deref(), Some("l3"));
    // the last row went: the new last row.
    assert_eq!(selected_id(&app, TRENDING).as_deref(), Some("t0"));
    // the first row went: the card that comes next.
    assert_eq!(selected_id(&app, FeedKey::Saved).as_deref(), Some("v1"));
}

#[test]
fn commits_of_other_people_stay_in_every_list() {
    let mut app = crowded();
    let req = Write::Block.req(&Write::Block.press(&mut app));

    app.update(Write::Block.done("alice", true, req));

    for key in FeedKey::ALL {
        let feed = app.feed(key);
        assert!(
            feed.items
                .iter()
                .chain(&feed.suggestions)
                .all(|c| c.author.username != "alice"),
            "{key:?}"
        );
    }
    assert_eq!(app.feed(FeedKey::Saved).items.len(), 1);
}

#[test]
fn the_other_screens_keep_the_commits_until_they_are_loaded_again() {
    let (mut app, effects) = started(120, 40);
    let items = vec![card(MAIN, "alice"), card("a1", "alice"), card("r0", "bob")];
    deliver(&mut app, FOLLOWING, &effects, feed_page(items));
    let effects = open_thread(&mut app, MAIN);
    deliver_thread(&mut app, &effects, vec![ancestor("a1", "alice")], 2);
    deliver_replies(&mut app, &effects, MAIN, vec![], None);
    let effects = open_profile(&mut app, "alice");
    deliver_user(
        &mut app,
        &effects,
        "alice",
        resp_of("alice", json!({}), json!({})),
    );
    deliver_commits(
        &mut app,
        &effects,
        "alice",
        ProfileTab::Commits,
        posts("p", 0, 2, "alice"),
        None,
    );
    let req = Write::Block.req(&Write::Block.press(&mut app));

    app.update(Write::Block.done("alice", true, req));

    assert_eq!(ids(&app, FOLLOWING), ["r0"]);
    let thread = state(&app, MAIN);
    assert_eq!(thread.ancestors.len(), 1);
    assert!(thread.main_commit().is_some());
    assert_eq!(app.profile("alice").map(|p| p.commits.items.len()), Some(2));
}

#[test]
fn an_answer_that_arrives_after_leaving_the_profile_still_takes_the_commits_out() {
    let mut app = crowded();
    let req = Write::Block.req(&Write::Block.press(&mut app));
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.screen(), Screen::Saved);

    app.update(Write::Block.done("alice", true, req));

    assert_eq!(ids(&app, FeedKey::Saved), ["v1"]);
    assert_eq!(ids(&app, FOLLOWING), ["c0", "c2", "c4"]);
}

// --- a refused block ------------------------------------------------------------------------------

#[test]
fn a_refused_block_leaves_everything_as_it_was_and_the_block_can_be_asked_again() {
    let mut app = crowded();
    let before = (
        held(&app, "alice"),
        ids(&app, FOLLOWING),
        ids(&app, FeedKey::Saved),
        me_counts(&app),
    );
    let req = Write::Block.req(&Write::Block.press(&mut app));

    app.update(Write::Block.failed("alice", true, req, "http_500"));

    let after = (
        held(&app, "alice"),
        ids(&app, FOLLOWING),
        ids(&app, FeedKey::Saved),
        me_counts(&app),
    );
    assert_eq!(after, before);
    assert!(!shows_block(&app, "alice"));
    assert_eq!(toast(&app), Some((ToastKind::Error, "http_500".into())));
    key(&mut app, 'B');
    assert_eq!(app.modal(), Some(Modal::ConfirmBlock));
}

#[test]
fn a_refusal_shows_the_message_of_the_server_and_the_wait_of_a_rate_limit_for_every_kind() {
    let unauthorized = Failure {
        code: "http_403".into(),
        retry_after: None,
        message: Some("This action is unauthorized.".into()),
    };
    let limited = Failure {
        code: "rate_limited".into(),
        retry_after: Some(9),
        message: Some("Too Many Attempts.".into()),
    };
    let cases = [
        (unauthorized, "This action is unauthorized."),
        (limited, "rate_limited: retry in 9s"),
    ];
    for write in WRITES {
        for (failure, text) in &cases {
            let mut app = plain_alice();
            let (on, req) = write.sent(&write.press(&mut app));

            app.update(write.failed_with("alice", on, req, failure.clone()));

            assert_eq!(
                toast(&app),
                Some((ToastKind::Error, (*text).to_string())),
                "{write:?} {failure:?}"
            );
        }
    }
}

// --- unblock --------------------------------------------------------------------------------------

#[test]
fn b_on_a_blocked_profile_unblocks_without_asking() {
    let mut app = blocked_bob_app();

    let effects = key(&mut app, 'B');

    assert_eq!(block_effects(&effects), [("bob".to_string(), false)]);
    assert_eq!(effects.len(), 1, "{effects:?}");
    assert_eq!(app.modal(), None);
    assert!(shows_block(&app, "bob"));
}

#[test]
fn b_on_the_profile_that_was_just_blocked_unblocks_it() {
    let mut app = blocked_alice(false, false);

    let effects = key(&mut app, 'B');

    assert_eq!(block_effects(&effects), [("alice".to_string(), false)]);
    assert_eq!(app.modal(), None);
}

#[test]
fn a_second_b_while_the_unblock_is_out_sends_nothing() {
    let mut app = blocked_bob_app();
    Write::Block.req(&key(&mut app, 'B'));

    assert_eq!(key(&mut app, 'B'), Vec::<Effect>::new());
}

#[test]
fn the_answer_of_an_unblock_loads_the_profile_again() {
    let mut app = blocked_bob_app();
    let req = Write::Block.req(&key(&mut app, 'B'));

    let effects = app.update(Write::Block.done("bob", false, req));

    assert_eq!(user_fetches(&effects), ["bob"]);
    let reload = user_req(&effects);
    app.update(Action::UserLoaded {
        username: "bob".into(),
        req: reload,
        response: Box::new(resp_of("bob", json!({}), json!({}))),
    });
    assert!(!shows_block(&app, "bob"));
    assert_eq!(held(&app, "bob").data.username, "bob");
    assert_eq!(toast(&app), None);
}

#[test]
fn an_unblock_answer_that_still_says_blocked_leaves_the_block_shown() {
    let mut app = blocked_bob_app();
    let req = Write::Block.req(&key(&mut app, 'B'));

    let effects = app.update(Write::Block.done("bob", true, req));

    assert_eq!(user_fetches(&effects), Vec::<String>::new());
    assert!(shows_block(&app, "bob"));
}

#[test]
fn a_refused_unblock_leaves_the_block_shown_and_says_why() {
    let mut app = blocked_bob_app();
    let req = Write::Block.req(&key(&mut app, 'B'));

    app.update(Write::Block.failed("bob", false, req, "http_500"));

    assert!(shows_block(&app, "bob"));
    assert_eq!(toast(&app), Some((ToastKind::Error, "http_500".into())));
    let again = key(&mut app, 'B');
    assert_eq!(block_effects(&again), [("bob".to_string(), false)]);
}

#[test]
fn f_and_m_do_nothing_on_a_blocked_profile() {
    let mut app = blocked_bob_app();

    assert_eq!(bound_key(&mut app, 'f'), Vec::<Effect>::new());
    assert_eq!(bound_key(&mut app, 'm'), Vec::<Effect>::new());
}

// --- nothing to write to --------------------------------------------------------------------------

#[test]
fn f_m_and_b_do_nothing_on_a_profile_that_is_unavailable_or_still_loading() {
    let mut unavailable = signed_in(120, 40);
    let effects = open_profile(&mut unavailable, "bob");
    unavailable.update(Action::UserFailed {
        username: "bob".into(),
        req: user_req(&effects),
        failure: Failure::new("user_unavailable"),
        blocked_user: None,
    });
    let mut loading = signed_in(120, 40);
    open_profile(&mut loading, "bob");
    for (name, mut app) in [("unavailable", unavailable), ("loading", loading)] {
        for c in ['f', 'm', 'B'] {
            assert_eq!(bound_key(&mut app, c), Vec::<Effect>::new(), "{name} {c}");
            assert_eq!(app.modal(), None, "{name} {c}");
        }
    }
}

#[test]
fn f_m_and_b_do_nothing_on_your_own_profile_whatever_the_case_of_the_name() {
    for typed in ["myuron", "MyUron"] {
        let mut app = signed_in(120, 40);
        let effects = open_profile(&mut app, typed);
        deliver_user(&mut app, &effects, typed, own_resp());
        for c in ['f', 'm', 'B'] {
            assert_eq!(bound_key(&mut app, c), Vec::<Effect>::new(), "{typed} {c}");
            assert_eq!(app.modal(), None, "{typed} {c}");
        }
        assert_eq!(me_counts(&app), (10, 12), "{typed}");
    }
}

#[test]
fn a_guest_is_asked_to_sign_in_by_f_m_and_b_and_nothing_is_written() {
    for c in ['f', 'm', 'B'] {
        let mut app = guest_profile(vec![card("c0", "alice")]);
        let before = app.profile("alice").and_then(|p| p.user.clone());

        let effects = key(&mut app, c);

        assert_eq!(app.modal(), Some(Modal::SignIn), "{c}");
        assert_eq!(user_writes(&effects), 0, "{c}");
        assert_eq!(
            app.profile("alice").and_then(|p| p.user.clone()),
            before,
            "{c}"
        );
    }
}

#[test]
fn a_guest_is_asked_to_sign_in_by_f_on_followers() {
    let mut app = dismissed_guest_on_followers();

    let effects = key(&mut app, 'f');

    assert_eq!(app.modal(), Some(Modal::SignIn));
    assert_eq!(user_writes(&effects), 0);
}

fn dismissed_guest_on_followers() -> App {
    let mut app = guest(120, 40);
    app.update(Action::CloseModal);
    let effects = app.update(Action::Open(Screen::Connections(
        "alice".into(),
        ConnTab::Followers,
    )));
    deliver_conns(
        &mut app,
        &effects,
        "alice",
        ConnTab::Followers,
        vec![person("bob", json!({}))],
        None,
    );
    app
}

#[test]
fn a_session_being_restored_does_nothing_for_f_m_and_b() {
    let mut app = App::new(Session::Restoring, NOW, time::UtcOffset::UTC);
    app.update(Action::Resize(120, 40));
    app.update(Action::Init);
    let effects = open_profile(&mut app, "alice");
    deliver_user(
        &mut app,
        &effects,
        "alice",
        resp_of("alice", json!({}), json!({})),
    );
    for (c, action) in [
        ('f', Action::ToggleFollow),
        ('m', Action::ToggleMute),
        ('B', Action::Block),
    ] {
        assert_eq!(mapped(&app, c), Some(action.clone()), "{c}");
        assert_eq!(
            app.update(action.clone()),
            Vec::<Effect>::new(),
            "{action:?}"
        );
        assert_eq!(app.modal(), None, "{action:?}");
    }
    assert_eq!(held(&app, "alice").is_following, Some(false));
}

// --- Followers / Following ------------------------------------------------------------------------

#[test]
fn f_on_the_selected_row_follows_that_person_and_counts_you_one_more() {
    let mut app = people_app();
    key(&mut app, 'j');

    let effects = key(&mut app, 'f');

    assert_eq!(follow_effects(&effects), [("carol".to_string(), true)]);
    assert_eq!(effects.len(), 1, "{effects:?}");
    assert_eq!(row_state(&app, ConnTab::Followers, "carol"), Some(true));
    assert_eq!(row_state(&app, ConnTab::Followers, "dave"), Some(false));
    assert_eq!(me_counts(&app), (11, 12));
}

#[test]
fn f_on_a_followed_row_unfollows_that_person() {
    let mut app = people_app();

    let effects = key(&mut app, 'f');

    assert_eq!(follow_effects(&effects), [("bob".to_string(), false)]);
    assert_eq!(row_state(&app, ConnTab::Followers, "bob"), Some(false));
    assert_eq!(me_counts(&app), (9, 12));
}

#[test]
fn f_on_your_own_row_does_nothing() {
    let mut app = people_app();
    key(&mut app, 'G');

    assert_eq!(bound_key(&mut app, 'f'), Vec::<Effect>::new());
    assert_eq!(row_state(&app, ConnTab::Followers, "myuron"), Some(false));
    assert_eq!(me_counts(&app), (10, 12));
}

#[test]
fn f_on_an_empty_or_loading_list_does_nothing() {
    let mut empty = followers(120, 30, "alice", vec![], None);
    assert_eq!(bound_key(&mut empty, 'f'), Vec::<Effect>::new());
    let mut loading = signed_in(120, 30);
    loading.update(Action::Open(Screen::Connections(
        "alice".into(),
        ConnTab::Followers,
    )));
    assert_eq!(bound_key(&mut loading, 'f'), Vec::<Effect>::new());
}

#[test]
fn a_refused_follow_from_a_list_puts_the_row_and_the_counts_back() {
    let mut app = people_app();
    key(&mut app, 'j');
    let req = Write::Follow.req(&key(&mut app, 'f'));

    app.update(Write::Follow.failed("carol", true, req, "http_500"));

    assert_eq!(row_state(&app, ConnTab::Followers, "carol"), Some(false));
    assert_eq!(me_counts(&app), (10, 12));
    assert_eq!(toast(&app), Some((ToastKind::Error, "http_500".into())));
}

#[test]
fn the_answer_of_a_follow_from_a_list_changes_nothing() {
    let mut app = people_app();
    key(&mut app, 'j');
    let req = Write::Follow.req(&key(&mut app, 'f'));

    app.update(Write::Follow.done("carol", true, req));

    assert_eq!(row_state(&app, ConnTab::Followers, "carol"), Some(true));
    assert_eq!(me_counts(&app), (11, 12));
    assert_eq!(toast(&app), None);
}

#[test]
fn a_second_f_on_the_same_row_while_the_request_is_out_sends_nothing() {
    let mut app = people_app();
    key(&mut app, 'j');
    key(&mut app, 'f');

    assert_eq!(key(&mut app, 'f'), Vec::<Effect>::new());
    assert_eq!(me_counts(&app), (11, 12));
}

#[test]
fn f_on_another_row_while_one_is_out_follows_that_person_too() {
    let mut app = people_app();
    key(&mut app, 'j');
    key(&mut app, 'f');
    key(&mut app, 'j');

    let effects = key(&mut app, 'f');

    assert_eq!(follow_effects(&effects), [("dave".to_string(), true)]);
    assert_eq!(me_counts(&app), (12, 12));
}

#[test]
fn a_follow_reaches_the_profile_and_the_rows_of_both_lists_where_the_person_is_held() {
    let mut app = carol_everywhere();
    assert_eq!(
        app.screen(),
        Screen::Connections("bob".into(), ConnTab::Following)
    );

    let req = Write::Follow.req(&key(&mut app, 'f'));

    assert_eq!(held(&app, "carol").is_following, Some(true));
    assert_eq!(counts(&app, "carol").1, Some(51));
    assert_eq!(
        row_followed(&app, "bob", ConnTab::Following, "carol"),
        Some(true)
    );
    assert_eq!(
        row_followed(&app, "bob", ConnTab::Followers, "carol"),
        Some(true)
    );
    assert_eq!(me_counts(&app), (11, 12));

    app.update(Write::Follow.failed("carol", true, req, "http_500"));

    assert_eq!(held(&app, "carol").is_following, Some(false));
    assert_eq!(counts(&app, "carol").1, Some(50));
    assert_eq!(
        row_followed(&app, "bob", ConnTab::Following, "carol"),
        Some(false)
    );
    assert_eq!(
        row_followed(&app, "bob", ConnTab::Followers, "carol"),
        Some(false)
    );
    assert_eq!(me_counts(&app), (10, 12));
}

// --- the session ----------------------------------------------------------------------------------

#[test]
fn a_rejected_device_in_an_answer_returns_to_guest_for_every_kind() {
    for write in WRITES {
        let mut app = plain_alice();
        let (on, req) = write.sent(&write.press(&mut app));

        app.update(write.failed("alice", on, req, NOT_APPROVED));

        assert_eq!(app.session(), &Session::Guest, "{write:?}");
        assert_eq!(app.modal(), Some(Modal::SignIn), "{write:?}");
        assert!(app.profile("alice").is_none(), "{write:?}");
        assert_eq!(
            toast(&app),
            Some((
                ToastKind::Error,
                "this device is no longer approved — sign in again".into()
            )),
            "{write:?}"
        );
    }
}

#[test]
fn a_rejection_for_a_guest_changes_nothing_for_every_kind() {
    for write in WRITES {
        let mut app = guest(120, 40);
        app.update(Action::CloseModal);

        app.update(write.failed("alice", true, ReqId(3), NOT_APPROVED));

        assert_eq!(app.modal(), None, "{write:?}");
        assert_eq!(toast(&app), None, "{write:?}");
        assert_eq!(app.session(), &Session::Guest, "{write:?}");
    }
}

#[test]
fn an_answer_from_before_the_new_sign_in_changes_nothing_for_every_kind() {
    for write in WRITES {
        let mut app = plain_alice();
        let (on, req) = write.sent(&write.press(&mut app));
        sign_in_again(&mut app);
        assert!(matches!(app.session(), Session::SignedIn(_)), "{write:?}");
        let shown = toast(&app);
        let me = me_of(&app);

        app.update(write.done("alice", on, req));
        app.update(write.failed("alice", on, req, "http_500"));
        app.update(write.failed("alice", on, req, NOT_APPROVED));

        assert!(matches!(app.session(), Session::SignedIn(_)), "{write:?}");
        assert_eq!(me_of(&app), me, "{write:?}");
        assert_eq!(toast(&app), shown, "{write:?}");
    }
}

#[test]
fn after_signing_in_again_the_same_person_can_be_written_to_again_for_every_kind() {
    for write in WRITES {
        let mut app = plain_alice();
        let old = write.req(&write.press(&mut app));
        sign_in_again(&mut app);
        let mut app = profile_with(
            app,
            "alice",
            resp_of("alice", json!({}), json!({})),
            posts("p", 0, 1, "alice"),
            None,
        );

        let effects = write.press(&mut app);

        let req = write.request(&effects).map(|(_, _, req)| req);
        assert!(req.is_some_and(|r| r.0 > old.0), "{write:?}: {effects:?}");
    }
}

#[test]
fn a_rejection_with_a_newer_request_id_after_the_new_sign_in_still_ends_the_session() {
    let mut app = plain_alice();
    Write::Follow.req(&key(&mut app, 'f'));
    sign_in_again(&mut app);
    let mut app = profile_with(
        app,
        "alice",
        resp_of("alice", json!({}), json!({})),
        posts("p", 0, 1, "alice"),
        None,
    );
    let req = Write::Follow.req(&key(&mut app, 'f'));

    app.update(Write::Follow.failed("alice", true, req, NOT_APPROVED));

    assert_eq!(app.session(), &Session::Guest);
}

// --- the Safety lists -------------------------------------------------------------------------------

/// Settings on the Safety tab with both lists loaded and empty.
fn safety_loaded() -> App {
    let mut app = signed_in(120, 40);
    let mut effects = app.update(Action::Goto(Screen::Settings));
    effects.extend(key(&mut app, '3'));
    for effect in effects {
        match effect {
            Effect::FetchMuted { req, .. } => {
                app.update(Action::MutedLoaded {
                    req,
                    page: crate::profile_tests::upage(vec![], None),
                });
            }
            Effect::FetchBlocked { req, .. } => {
                app.update(Action::BlockedLoaded {
                    req,
                    page: crate::profile_tests::upage(vec![], None),
                });
            }
            _ => {}
        }
    }
    app
}

/// The fetches that opening Settings again makes.
fn reopened_settings(app: &mut App) -> (usize, usize) {
    let effects = app.update(Action::Open(Screen::Settings));
    let muted = effects
        .iter()
        .filter(|e| matches!(e, Effect::FetchMuted { .. }))
        .count();
    let blocked = effects
        .iter()
        .filter(|e| matches!(e, Effect::FetchBlocked { .. }))
        .count();
    (muted, blocked)
}

#[test]
fn a_mute_or_an_unmute_that_succeeds_makes_settings_load_the_muted_list_again() {
    for muted in [false, true] {
        let mut app = profile_with(
            safety_loaded(),
            "alice",
            resp_of("alice", json!({}), json!({"is_muting": muted})),
            posts("p", 0, 1, "alice"),
            None,
        );
        let req = Write::Mute.req(&key(&mut app, 'm'));
        app.update(Write::Mute.done("alice", !muted, req));

        let (muted_fetches, _) = reopened_settings(&mut app);

        assert_eq!(muted_fetches, 1, "was muted: {muted}");
    }
}

#[test]
fn a_block_that_succeeds_makes_settings_load_the_blocked_list_again() {
    let mut app = profile_with(
        safety_loaded(),
        "alice",
        resp_of("alice", json!({}), json!({})),
        posts("p", 0, 1, "alice"),
        None,
    );
    let req = Write::Block.req(&Write::Block.press(&mut app));
    app.update(Write::Block.done("alice", true, req));

    let (_, blocked_fetches) = reopened_settings(&mut app);

    assert_eq!(blocked_fetches, 1);
}

#[test]
fn an_unblock_that_succeeds_makes_settings_load_the_blocked_list_again() {
    let mut app = safety_loaded();
    let effects = open_profile(&mut app, "bob");
    app.update(Action::UserFailed {
        username: "bob".into(),
        req: user_req(&effects),
        failure: Failure::new("user_blocked"),
        blocked_user: Some(Box::new(person("bob", json!({})))),
    });
    let req = Write::Block.req(&key(&mut app, 'B'));
    app.update(Write::Block.done("bob", false, req));

    let (_, blocked_fetches) = reopened_settings(&mut app);

    assert_eq!(blocked_fetches, 1);
}

#[test]
fn a_refused_write_leaves_the_safety_lists_as_they_are() {
    for write in [Write::Mute, Write::Block] {
        let mut app = profile_with(
            safety_loaded(),
            "alice",
            resp_of("alice", json!({}), json!({})),
            posts("p", 0, 1, "alice"),
            None,
        );
        let (on, req) = write.sent(&write.press(&mut app));
        app.update(write.failed("alice", on, req, "http_500"));

        assert_eq!(reopened_settings(&mut app), (0, 0), "{write:?}");
    }
}

// --- more cases -------------------------------------------------------------------------------------

#[test]
fn f_on_a_row_that_does_not_say_whether_it_is_followed_does_nothing() {
    let mut app = followers(120, 30, "alice", vec![person("erin", json!({}))], None);

    assert_eq!(bound_key(&mut app, 'f'), Vec::<Effect>::new());
    assert_eq!(me_counts(&app), (10, 12));
}

#[test]
fn a_refused_follow_puts_back_a_profile_that_was_opened_with_another_case_of_the_name() {
    let mut app = signed_in(120, 40);
    let effects = open_profile(&mut app, "Alice");
    deliver_user(
        &mut app,
        &effects,
        "Alice",
        resp_of("alice", json!({}), json!({})),
    );
    let req = Write::Follow.req(&key(&mut app, 'f'));
    assert_eq!(held(&app, "Alice").is_following, Some(true));

    app.update(Write::Follow.failed("alice", true, req, "http_500"));

    assert_eq!(held(&app, "Alice").is_following, Some(false));
    assert_eq!(counts(&app, "Alice").1, Some(3647));
    assert_eq!(me_counts(&app), (10, 12));
}

#[test]
fn while_a_block_is_out_f_on_a_row_of_the_same_person_does_nothing() {
    let mut app = signed_in(120, 40);
    let effects = open_profile(&mut app, "alice");
    deliver_user(
        &mut app,
        &effects,
        "alice",
        resp_of("alice", json!({}), json!({})),
    );
    Write::Block.req(&Write::Block.press(&mut app));
    let effects = open_profile(&mut app, "bob");
    deliver_user(
        &mut app,
        &effects,
        "bob",
        resp_of("bob", json!({}), json!({})),
    );
    let effects = key(&mut app, '3');
    deliver_conns(
        &mut app,
        &effects,
        "bob",
        ConnTab::Followers,
        vec![person("alice", json!({"is_followed": false}))],
        None,
    );

    assert_eq!(bound_key(&mut app, 'f'), Vec::<Effect>::new());
    assert_eq!(
        row_followed(&app, "bob", ConnTab::Followers, "alice"),
        Some(false)
    );
}

#[test]
fn the_answer_of_a_follow_that_comes_after_the_block_succeeded_changes_nothing() {
    for refused in [false, true] {
        let mut app = plain_alice();
        let follow = Write::Follow.req(&key(&mut app, 'f'));
        let block = Write::Block.req(&Write::Block.press(&mut app));
        app.update(Write::Block.done("alice", true, block));
        // The block took the follow back, so the counts are the block's.
        assert_eq!(me_counts(&app), (10, 12));
        let before = (
            me_counts(&app),
            counts(&app, "myuron"),
            app.profile("alice")
                .cloned()
                .unwrap_or_default()
                .blocked_user,
            toast(&app),
        );

        if refused {
            app.update(Write::Follow.failed("alice", true, follow, "http_500"));
        } else {
            app.update(Write::Follow.done("alice", true, follow));
        }

        let after = (
            me_counts(&app),
            counts(&app, "myuron"),
            app.profile("alice")
                .cloned()
                .unwrap_or_default()
                .blocked_user,
            toast(&app),
        );
        assert_eq!(after, before, "refused: {refused}");
        assert!(shows_block(&app, "alice"), "refused: {refused}");
    }
}

// --- review follow-ups ------------------------------------------------------------------------------

#[test]
fn a_load_of_the_profile_that_left_before_the_block_does_not_hide_the_block() {
    let mut app = plain_alice();
    let reload = key(&mut app, 'R');
    let req = Write::Block.req(&Write::Block.press(&mut app));
    app.update(Write::Block.done("alice", true, req));

    app.update(Action::UserLoaded {
        username: "alice".into(),
        req: user_req(&reload),
        response: Box::new(resp_of("alice", json!({}), json!({}))),
    });

    assert!(shows_block(&app, "alice"));
}

#[test]
fn a_block_that_succeeds_makes_settings_load_the_muted_list_again_when_a_mute_was_out() {
    let mut app = profile_with(
        safety_loaded(),
        "alice",
        resp_of("alice", json!({}), json!({})),
        posts("p", 0, 1, "alice"),
        None,
    );
    Write::Mute.req(&key(&mut app, 'm'));
    let req = Write::Block.req(&Write::Block.press(&mut app));
    app.update(Write::Block.done("alice", true, req));

    let (muted, blocked) = reopened_settings(&mut app);

    assert_eq!((muted, blocked), (1, 1));
}

#[test]
fn b_does_nothing_while_the_profile_is_loading_again_after_an_unblock() {
    let mut app = blocked_bob_app();
    let req = Write::Block.req(&key(&mut app, 'B'));
    let effects = app.update(Write::Block.done("bob", false, req));
    assert_eq!(user_fetches(&effects), ["bob"]);

    assert_eq!(key(&mut app, 'B'), Vec::<Effect>::new());
    assert!(shows_block(&app, "bob"));
}

#[test]
fn a_follow_from_a_row_reaches_the_profile_only_when_it_holds_the_value_from_before_the_press() {
    let mut app = signed_in(120, 40);
    let effects = open_profile(&mut app, "carol");
    // The profile already says "followed" while the row, loaded earlier, says "not followed".
    deliver_user(
        &mut app,
        &effects,
        "carol",
        resp_of(
            "carol",
            json!({"followers_count": 50}),
            json!({"is_following": true}),
        ),
    );
    let effects = open_profile(&mut app, "bob");
    deliver_user(
        &mut app,
        &effects,
        "bob",
        resp_of("bob", json!({}), json!({})),
    );
    let effects = key(&mut app, '3');
    deliver_conns(
        &mut app,
        &effects,
        "bob",
        ConnTab::Followers,
        vec![person("carol", json!({"is_followed": false}))],
        None,
    );

    Write::Follow.req(&bound_key(&mut app, 'f'));

    assert_eq!(held(&app, "carol").is_following, Some(true));
    assert_eq!(counts(&app, "carol").1, Some(50));
    assert_eq!(
        row_followed(&app, "bob", ConnTab::Followers, "carol"),
        Some(true)
    );
}

#[test]
fn a_refused_unblock_leaves_the_safety_lists_as_they_are() {
    let mut app = safety_loaded();
    let effects = open_profile(&mut app, "bob");
    app.update(Action::UserFailed {
        username: "bob".into(),
        req: user_req(&effects),
        failure: Failure::new("user_blocked"),
        blocked_user: Some(Box::new(person("bob", json!({})))),
    });
    let req = Write::Block.req(&key(&mut app, 'B'));
    app.update(Write::Block.failed("bob", false, req, "http_500"));

    assert_eq!(reopened_settings(&mut app), (0, 0));
}
