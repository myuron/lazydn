//! The more menu that `.` opens on the selected card.
//!
//! - `.` opens the menu on the selected card of Log, Saved, a profile list or a thread, titled with the short
//!   hash. A guest is asked to sign in; a tombstone, an empty list, a card that is being deleted and a session
//!   being restored do nothing.
//! - The items, in this order, are `s` Star (Unstar once starred), `p` Give a pastel, `b` Save (Unsave once
//!   saved), `y` Copy link, `O` Open in browser, `!` Report and `d` Revert commit. Star and Report are left out
//!   on your own commit, Give a pastel when the pastéis mode is off or the commit is yours, and Revert commit on
//!   the commit of someone else.
//! - The first item is chosen when the menu opens. `j`, `k` and the arrows move the choice and stop at the first
//!   and the last item. Enter runs the chosen item and the key of an item runs it at once; an item that is not
//!   listed does nothing.
//! - A run does what the same key does on the card and closes the menu. Revert commit opens the confirmation
//!   and Report opens the report dialog in its place.
//! - Esc closes the menu. Ctrl+C quits. Every other key, `?` and `Q` included, is ignored.

use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};
use serde_json::json;
use time::UtcOffset;

use crate::action::{Action, Effect, Failure};
use crate::app::{App, ConnTab, LogTab, Me, Modal, ProfileTab, Screen, Session};
use crate::delete_tests::{delete, done, own, thread_by};
use crate::feed::ReqId;
use crate::feed_tests::{FOLLOWING, deliver, down, first_req, started};
use crate::keymap::{MenuKey, map_event, map_key};
use crate::profile_tests::{deliver_commits, deliver_user, open_profile, user_resp};
use crate::test_support::*;
use crate::thread_tests::{MAIN, commit_json, item_of, merge, opened, tomb};
use crate::write_tests::{
    card, card_with, feed_page, guest_profile, key, log_app_of, mapped, saved_app,
};

const NOT_APPROVED: &str = "device_not_approved";

// --- helpers ------------------------------------------------------------------

fn link_of(id: &str) -> String {
    format!("https://dev-null.sh/@alice/commits/{id}")
}

/// A commit of alice with the hash the docs show and a link to copy.
fn theirs(id: &str) -> crate::model::Commit {
    card_with(
        id,
        "alice",
        json!({"short_hash": "a1b2c3d", "html_url": link_of(id)}),
    )
}

/// Your commit with a link to copy.
fn mine(id: &str) -> crate::model::Commit {
    card_with(
        id,
        "myuron",
        json!({"short_hash": "a1b2c3d", "html_url": link_of(id)}),
    )
}

fn without_pasteis() -> Me {
    Me {
        pasteis_mode: false,
        ..fixture_me()
    }
}

fn log_of_theirs() -> App {
    log_app_of(fixture_me(), vec![theirs("c0"), card("c1", "bob")])
}

fn log_of_mine() -> App {
    log_app_of(fixture_me(), vec![mine("c0"), card("c1", "bob")])
}

fn open_menu(app: &mut App) {
    let effects = key(app, '.');
    assert_eq!(
        app.modal(),
        Some(Modal::Menu),
        ". did not open the menu: {effects:?}"
    );
    assert!(app.menu().is_some(), "no menu state is held");
}

fn items(app: &App) -> Vec<(char, &'static str)> {
    assert!(app.menu().is_some(), "no menu is open");
    app.menu()
        .map(|m| m.items.iter().map(|i| (i.key, i.label)).collect())
        .unwrap_or_default()
}

fn keys(app: &App) -> String {
    items(app).into_iter().map(|(k, _)| k).collect()
}

fn selected(app: &App) -> usize {
    assert!(app.menu().is_some(), "no menu is open");
    app.menu().map_or(usize::MAX, |m| m.selected)
}

fn is_closed(app: &App) -> bool {
    app.modal() != Some(Modal::Menu) && app.menu().is_none()
}

fn event_key(app: &App, code: KeyCode, modifiers: KeyModifiers) -> Option<Action> {
    map_key(app, key_event(code, modifiers, KeyEventKind::Press))
}

fn plain(app: &App, code: KeyCode) -> Option<Action> {
    event_key(app, code, KeyModifiers::NONE)
}

fn profile_of(items: Vec<crate::model::Commit>) -> App {
    let mut app = log_app_of(fixture_me(), vec![]);
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

fn restoring() -> App {
    let mut app = App::new(Session::Restoring, NOW, UtcOffset::UTC);
    app.update(Action::Resize(120, 40));
    app.update(Action::Init);
    app
}

// --- keymap -------------------------------------------------------------------

#[test]
fn dot_opens_the_menu_on_the_screens_with_commit_cards_only() {
    let log = log_of_theirs();
    let saved = saved_app(1);
    let thread = thread_by("alice", 0, vec![], vec![]);
    let profile = profile_of(vec![theirs("c0")]);
    for (name, app) in [
        ("log", &log),
        ("saved", &saved),
        ("thread", &thread),
        ("profile", &profile),
    ] {
        assert_eq!(mapped(app, '.'), Some(Action::OpenMenu), "{name}");
    }

    let mut others = signed_in(120, 40);
    for screen in [
        Screen::Notifications,
        Screen::Settings,
        Screen::Connections("alice".into(), ConnTab::Followers),
    ] {
        others.update(Action::Open(screen.clone()));
        assert_eq!(mapped(&others, '.'), None, "{screen:?}");
    }
}

#[test]
fn the_open_menu_reads_the_choice_keys_the_item_keys_esc_and_ctrl_c() {
    let mut app = log_of_theirs();
    open_menu(&mut app);

    let down_key = Some(Action::MenuKey(MenuKey::Down));
    let up_key = Some(Action::MenuKey(MenuKey::Up));
    assert_eq!(mapped(&app, 'j'), down_key);
    assert_eq!(plain(&app, KeyCode::Down), down_key);
    assert_eq!(mapped(&app, 'k'), up_key);
    assert_eq!(plain(&app, KeyCode::Up), up_key);
    assert_eq!(
        plain(&app, KeyCode::Enter),
        Some(Action::MenuKey(MenuKey::Select))
    );
    assert_eq!(mapped(&app, 's'), Some(Action::MenuKey(MenuKey::Item('s'))));
    assert_eq!(mapped(&app, '!'), Some(Action::MenuKey(MenuKey::Item('!'))));
    assert_eq!(plain(&app, KeyCode::Esc), Some(Action::CloseModal));
    assert_eq!(
        event_key(&app, KeyCode::Char('c'), KeyModifiers::CONTROL),
        Some(Action::Quit)
    );
}

#[test]
fn the_open_menu_maps_neither_help_nor_quit_nor_a_paste() {
    let mut app = log_of_theirs();
    open_menu(&mut app);

    assert_eq!(mapped(&app, '?'), None);
    assert_eq!(mapped(&app, 'Q'), None);
    assert_eq!(plain(&app, KeyCode::Tab), None);
    assert_eq!(plain(&app, KeyCode::BackTab), None);
    assert_eq!(map_event(&app, &Event::Paste("text".into())), None);
}

// --- the items ----------------------------------------------------------------

#[test]
fn the_commit_of_someone_else_lists_every_item_but_the_revert() {
    let mut app = log_of_theirs();
    open_menu(&mut app);

    assert_eq!(
        items(&app),
        [
            ('s', "Star"),
            ('p', "Give a pastel"),
            ('b', "Save"),
            ('y', "Copy link"),
            ('O', "Open in browser"),
            ('!', "Report"),
        ]
    );
}

#[test]
fn give_a_pastel_is_left_out_when_the_pasteis_mode_is_off() {
    let mut app = log_app_of(without_pasteis(), vec![theirs("c0")]);
    open_menu(&mut app);

    assert_eq!(keys(&app), "sbyO!");
}

#[test]
fn your_own_commit_lists_save_the_links_and_the_revert() {
    let mut app = log_of_mine();
    open_menu(&mut app);

    assert_eq!(
        items(&app),
        [
            ('b', "Save"),
            ('y', "Copy link"),
            ('O', "Open in browser"),
            ('d', "Revert commit"),
        ]
    );
}

#[test]
fn your_own_commit_lists_the_same_items_with_the_pasteis_mode_off() {
    let mut app = log_app_of(without_pasteis(), vec![mine("c0")]);
    open_menu(&mut app);

    assert_eq!(keys(&app), "byOd");
}

#[test]
fn the_menu_is_titled_with_the_short_hash_of_the_card() {
    let mut app = log_of_theirs();
    open_menu(&mut app);

    let menu = app.menu().cloned();
    assert_eq!(menu.as_ref().map(|m| m.id.as_str()), Some("c0"));
    assert_eq!(
        menu.as_ref().map(|m| m.short_hash.as_str()),
        Some("a1b2c3d")
    );
}

#[test]
fn a_starred_and_saved_commit_offers_unstar_and_unsave() {
    let both = card_with("c0", "alice", json!({"starred": true, "is_saved": true}));
    let mut app = log_app_of(fixture_me(), vec![both]);
    open_menu(&mut app);

    let listed = items(&app);
    assert_eq!(listed[0], ('s', "Unstar"));
    assert_eq!(listed[2], ('b', "Unsave"));
}

#[test]
fn a_commit_that_is_neither_starred_nor_saved_offers_star_and_save() {
    let plain_card = card_with("c0", "alice", json!({"starred": false, "is_saved": false}));
    let mut app = log_app_of(fixture_me(), vec![plain_card]);
    open_menu(&mut app);

    let listed = items(&app);
    assert_eq!(listed[0], ('s', "Star"));
    assert_eq!(listed[2], ('b', "Save"));
}

#[test]
fn the_words_follow_a_star_and_a_save_that_are_out() {
    let mut app = log_of_theirs();
    open_menu(&mut app);
    key(&mut app, 's');
    open_menu(&mut app);
    key(&mut app, 'b');
    open_menu(&mut app);

    let listed = items(&app);
    assert_eq!(listed[0], ('s', "Unstar"));
    assert_eq!(listed[2], ('b', "Unsave"));
}

#[test]
fn y_and_o_are_listed_for_a_commit_without_a_link_and_do_nothing() {
    let mut app = log_app_of(fixture_me(), vec![card("c0", "alice")]);
    open_menu(&mut app);
    assert_eq!(keys(&app), "spbyO!");

    assert_eq!(key(&mut app, 'y'), Vec::<Effect>::new());
    assert!(is_closed(&app));
    open_menu(&mut app);
    assert_eq!(key(&mut app, 'O'), Vec::<Effect>::new());
    assert!(is_closed(&app));
}

#[test]
fn the_menu_opens_on_every_kind_of_card() {
    let mut main = thread_by("alice", 1, vec![], vec![]);
    let mut reply = thread_by(
        "alice",
        1,
        vec![],
        vec![item_of(merge(
            commit_json("r0", "bob", 600),
            json!({"parent_id": MAIN}),
        ))],
    );
    down(&mut reply, 1);
    let mut profile = profile_of(vec![theirs("c0")]);
    let mut saved = saved_app(2);
    let cases = [
        ("thread main", &mut main, MAIN),
        ("thread reply", &mut reply, "r0"),
        ("profile", &mut profile, "c0"),
        ("saved", &mut saved, "s0"),
    ];
    for (name, app, id) in cases {
        open_menu(app);
        assert_eq!(app.menu().map(|m| m.id.as_str()), Some(id), "{name}");
    }
    // A saved commit is offered the way to undo it.
    assert_eq!(items(&saved)[2], ('b', "Unsave"));
    assert_eq!(items(&reply)[0], ('s', "Star"));
}

#[test]
fn the_menu_of_your_own_commit_in_a_thread_offers_the_revert() {
    let mut app = thread_by("myuron", 0, vec![], vec![]);
    open_menu(&mut app);

    assert_eq!(keys(&app), "byOd");
}

// --- the choice ---------------------------------------------------------------

#[test]
fn the_first_item_is_chosen_when_the_menu_opens() {
    for mut app in [log_of_theirs(), log_of_mine()] {
        open_menu(&mut app);
        assert_eq!(selected(&app), 0);
    }
}

#[test]
fn j_and_k_and_the_arrows_move_the_choice_and_stop_at_both_ends() {
    let mut app = log_of_mine();
    open_menu(&mut app);
    let last = items(&app).len() - 1;

    press(&mut app, KeyCode::Up);
    key(&mut app, 'k');
    assert_eq!(selected(&app), 0, "stops at the first item");
    key(&mut app, 'j');
    assert_eq!(selected(&app), 1);
    press(&mut app, KeyCode::Down);
    assert_eq!(selected(&app), 2);
    key(&mut app, 'k');
    assert_eq!(selected(&app), 1);
    for _ in 0..10 {
        key(&mut app, 'j');
    }
    assert_eq!(selected(&app), last, "stops at the last item");
    press(&mut app, KeyCode::Down);
    assert_eq!(selected(&app), last);
    assert_eq!(app.modal(), Some(Modal::Menu));
}

#[test]
fn moving_the_choice_leaves_the_list_behind_it_alone() {
    let mut app = log_of_theirs();
    open_menu(&mut app);

    let mut effects = type_str(&mut app, "jjjkk");
    effects.extend(press(&mut app, KeyCode::Down));

    assert_eq!(effects, Vec::<Effect>::new());
    assert_eq!(app.feed(FOLLOWING).selected, 0);
}

// --- running an item ----------------------------------------------------------

/// A name, a way to build the app and the keys of the items its menu lists.
type Case = (&'static str, fn() -> App, &'static str);

/// What a run leaves behind that the card key leaves as well.
fn outcome(app: &App) -> (Option<Modal>, Vec<crate::model::Commit>, Option<String>) {
    (
        app.modal(),
        app.feed(FOLLOWING).items.clone(),
        app.delete_confirm().map(|c| c.id.clone()),
    )
}

#[test]
fn enter_runs_each_item_as_its_key_does_on_the_card_and_closes_the_menu() {
    let cases: [Case; 2] = [
        ("theirs", log_of_theirs, "spbyO!"),
        ("mine", log_of_mine, "byOd"),
    ];
    for (name, make, listed) in cases {
        for (at, item) in listed.chars().enumerate() {
            let mut direct = make();
            let want = key(&mut direct, item);

            let mut app = make();
            open_menu(&mut app);
            for _ in 0..at {
                key(&mut app, 'j');
            }
            let got = press(&mut app, KeyCode::Enter);

            assert_eq!(got, want, "{name} {item}: effects");
            assert_eq!(outcome(&app), outcome(&direct), "{name} {item}: state");
            assert!(is_closed(&app), "{name} {item}: the menu is still open");
        }
    }
}

#[test]
fn the_key_of_an_item_runs_it_as_the_key_does_on_the_card_and_closes_the_menu() {
    let cases: [Case; 2] = [
        ("theirs", log_of_theirs, "spbyO!"),
        ("mine", log_of_mine, "byOd"),
    ];
    for (name, make, listed) in cases {
        for item in listed.chars() {
            let mut direct = make();
            let want = key(&mut direct, item);

            let mut app = make();
            open_menu(&mut app);
            let got = key(&mut app, item);

            assert_eq!(got, want, "{name} {item}: effects");
            assert_eq!(outcome(&app), outcome(&direct), "{name} {item}: state");
            assert!(is_closed(&app), "{name} {item}: the menu is still open");
        }
    }
}

#[test]
fn the_items_send_the_requests_of_the_card_keys() {
    let mut app = log_of_theirs();
    open_menu(&mut app);
    let star = key(&mut app, 's');
    assert!(
        matches!(star.as_slice(), [Effect::StarCommit { id, on: true, .. }] if id == "c0"),
        "{star:?}"
    );
    open_menu(&mut app);
    let save = key(&mut app, 'b');
    assert!(
        matches!(save.as_slice(), [Effect::BookmarkCommit { id, on: true, .. }] if id == "c0"),
        "{save:?}"
    );
    open_menu(&mut app);
    let pastel = key(&mut app, 'p');
    assert!(
        matches!(pastel.as_slice(), [Effect::GivePastel { id, .. }] if id == "c0"),
        "{pastel:?}"
    );
    open_menu(&mut app);
    assert_eq!(key(&mut app, 'y'), [Effect::CopyLink(link_of("c0"))]);
    open_menu(&mut app);
    assert_eq!(key(&mut app, 'O'), [Effect::OpenUrl(link_of("c0"))]);
}

#[test]
fn the_revert_item_opens_the_confirmation_in_place_of_the_menu() {
    let mut app = log_of_mine();
    open_menu(&mut app);

    let effects = key(&mut app, 'd');

    assert_eq!(effects, Vec::<Effect>::new());
    assert_eq!(app.modal(), Some(Modal::ConfirmDelete));
    assert_eq!(app.menu(), None);
    assert_eq!(app.delete_confirm().map(|c| c.id.as_str()), Some("c0"));
    // The confirmation reads `y` as the confirmation of the delete.
    let sent = key(&mut app, 'y');
    assert!(
        matches!(sent.as_slice(), [Effect::DeleteCommit { id, .. }] if id == "c0"),
        "{sent:?}"
    );
}

#[test]
fn the_report_item_opens_the_report_dialog_in_place_of_the_menu() {
    let mut app = log_of_theirs();
    open_menu(&mut app);

    let effects = key(&mut app, '!');

    assert_eq!(effects, Vec::<Effect>::new());
    assert_eq!(app.modal(), Some(Modal::Report));
    assert_eq!(app.menu(), None);
    assert_eq!(app.report().map(|r| r.id.as_str()), Some("c0"));
}

#[test]
fn a_run_acts_on_the_card_the_menu_was_opened_on() {
    let mut app = log_app_of(fixture_me(), vec![theirs("c0"), theirs("c1")]);
    down(&mut app, 1);
    open_menu(&mut app);

    let effects = key(&mut app, 's');

    assert!(
        matches!(effects.as_slice(), [Effect::StarCommit { id, .. }] if id == "c1"),
        "{effects:?}"
    );
}

#[test]
fn an_item_that_is_not_listed_does_nothing_and_keeps_the_menu_open() {
    let mut mine = log_of_mine();
    let mut no_pasteis = log_app_of(without_pasteis(), vec![theirs("c0")]);
    let mut theirs_ = log_of_theirs();
    let cases = [
        ("own star", &mut mine, "sp!"),
        ("pasteis off", &mut no_pasteis, "pd"),
        ("others revert", &mut theirs_, "d"),
    ];
    for (name, app, typed) in cases {
        open_menu(app);
        let effects = type_str(app, typed);
        assert_eq!(effects, Vec::<Effect>::new(), "{name}");
        assert_eq!(app.modal(), Some(Modal::Menu), "{name}");
        assert_eq!(selected(app), 0, "{name}");
    }
}

// --- closing ------------------------------------------------------------------

#[test]
fn esc_closes_the_menu_without_leaving_the_screen() {
    let mut app = thread_by("alice", 0, vec![], vec![]);
    open_menu(&mut app);

    let effects = press(&mut app, KeyCode::Esc);

    assert_eq!(effects, Vec::<Effect>::new());
    assert!(is_closed(&app));
    assert_eq!(app.screen(), Screen::Thread(MAIN.into()));
}

#[test]
fn the_menu_opens_again_after_it_was_closed() {
    let mut app = log_of_theirs();
    open_menu(&mut app);
    key(&mut app, 'j');
    press(&mut app, KeyCode::Esc);

    open_menu(&mut app);

    assert_eq!(selected(&app), 0);
}

#[test]
fn every_other_key_including_help_and_quit_is_ignored_and_ctrl_c_quits() {
    let mut app = log_of_theirs();
    open_menu(&mut app);

    let mut effects = type_str(&mut app, "?Qgcrq@R1Llh.Go");
    effects.extend(press(&mut app, KeyCode::Tab));
    effects.extend(press(&mut app, KeyCode::BackTab));
    effects.extend(ctrl(&mut app, 'd'));
    effects.extend(ctrl(&mut app, 'u'));
    effects.extend(press(&mut app, KeyCode::Char(' ')));

    assert_eq!(effects, Vec::<Effect>::new());
    assert_eq!(app.modal(), Some(Modal::Menu));
    assert_eq!(selected(&app), 0);
    assert_eq!(app.screen(), Screen::Log(LogTab::Following));
    assert!(quit_requested(&ctrl(&mut app, 'c')));
}

#[test]
fn a_paste_does_not_reach_the_menu() {
    let mut app = log_of_theirs();
    open_menu(&mut app);

    if let Some(action) = map_event(&app, &Event::Paste("s".into())) {
        app.update(action);
    }

    assert_eq!(app.modal(), Some(Modal::Menu));
}

// --- when there is nothing to open --------------------------------------------

#[test]
fn a_guest_is_asked_to_sign_in_and_gets_no_menu() {
    let mut with_card = guest_profile(vec![card("c0", "alice")]);
    let mut without_card = crate::write_tests::guest_log();
    for (name, app) in [("card", &mut with_card), ("no card", &mut without_card)] {
        assert_eq!(app.modal(), None, "{name}");

        let effects = key(app, '.');

        assert_eq!(effects, Vec::<Effect>::new(), "{name}");
        assert_eq!(app.modal(), Some(Modal::SignIn), "{name}");
        assert_eq!(app.menu(), None, "{name}");
    }
}

#[test]
fn a_session_being_restored_does_nothing() {
    let mut app = restoring();

    assert_eq!(app.update(Action::OpenMenu), Vec::<Effect>::new());
    assert_eq!(app.modal(), None);
    assert_eq!(app.menu(), None);
}

#[test]
fn a_tombstone_and_an_empty_list_open_no_menu() {
    let mut thread = thread_by("alice", 1, vec![], vec![tomb("r0")]);
    down(&mut thread, 1);
    let mut empty = log_app_of(fixture_me(), vec![]);
    // The main commit above the tombstone takes the menu, so the silence below is that of the tombstone.
    let mut main = thread_by("alice", 1, vec![], vec![tomb("r0")]);
    open_menu(&mut main);

    for (name, app) in [("tombstone", &mut thread), ("empty", &mut empty)] {
        assert_eq!(key(app, '.'), Vec::<Effect>::new(), "{name}");
        assert_eq!(app.modal(), None, "{name}");
        assert_eq!(app.menu(), None, "{name}");
    }
}

#[test]
fn a_card_that_is_being_deleted_opens_no_menu() {
    let mut app = log_of_mine();
    delete(&mut app);
    assert!(app.is_deleting("c0"));

    let effects = key(&mut app, '.');

    assert_eq!(effects, Vec::<Effect>::new());
    assert_eq!(app.modal(), None);
    assert_eq!(app.menu(), None);
    // Another card is not held back by it.
    let mut other = log_app_of(fixture_me(), vec![own("c0"), card("c1", "alice")]);
    delete(&mut other);
    down(&mut other, 1);
    open_menu(&mut other);
}

#[test]
fn a_return_to_guest_closes_the_menu() {
    // The routing never lets a return to guest meet an open menu; what is closed here is the defence in
    // `reset_account_data`, reached by the answer of a request that was out before the menu opened.
    let mut app = log_of_theirs();
    open_menu(&mut app);

    app.update(Action::FeedFailed {
        key: FOLLOWING,
        req: ReqId(1),
        failure: Failure::new(NOT_APPROVED),
    });

    assert_eq!(app.modal(), Some(Modal::SignIn), "the guest is asked");
    assert_eq!(app.menu(), None);
}

// --- the card the menu was opened on ------------------------------------------

/// Following with `c0` selected, a refresh out, and the menu open on `c0`.
fn menu_over_a_refresh() -> (App, ReqId) {
    let (mut app, effects) = started(120, 40);
    deliver(
        &mut app,
        FOLLOWING,
        &effects,
        feed_page(vec![theirs("c0"), theirs("c1")]),
    );
    let refresh = key(&mut app, 'R');
    let req = first_req(&refresh);
    open_menu(&mut app);
    (app, req)
}

fn load_again(app: &mut App, req: ReqId, ids: &[&str]) {
    let items = ids.iter().map(|id| theirs(id)).collect();
    app.update(Action::FeedLoaded {
        key: FOLLOWING,
        req,
        page: feed_page(items),
    });
}

fn under_the_selection(app: &App) -> Option<String> {
    let feed = app.feed(FOLLOWING);
    feed.items.get(feed.selected).map(|c| c.id.clone())
}

#[test]
fn a_run_acts_on_the_card_of_the_menu_when_another_card_took_the_selected_place() {
    let (mut app, req) = menu_over_a_refresh();
    load_again(&mut app, req, &["c9", "c0"]);
    assert_eq!(
        under_the_selection(&app).as_deref(),
        Some("c9"),
        "the load did not put another card under the selection"
    );

    let effects = key(&mut app, 's');

    assert!(
        matches!(effects.as_slice(), [Effect::StarCommit { id, on: true, .. }] if id == "c0"),
        "{effects:?}"
    );
    assert!(is_closed(&app));
}

#[test]
fn a_run_on_a_card_that_left_the_screen_does_nothing_and_closes_the_menu() {
    let (mut app, req) = menu_over_a_refresh();
    load_again(&mut app, req, &["c9", "c8"]);

    for item in ['s', 'b', 'p', 'y', 'O', '!'] {
        let mut gone = app.clone();
        let effects = key(&mut gone, item);
        assert_eq!(effects, Vec::<Effect>::new(), "{item}");
        assert!(is_closed(&gone), "{item}");
        assert_eq!(gone.modal(), None, "{item}");
    }
}

#[test]
fn an_item_chosen_while_its_request_is_out_does_nothing_and_closes_the_menu() {
    let mut app = log_of_theirs();
    key(&mut app, 's');
    open_menu(&mut app);
    assert_eq!(items(&app)[0], ('s', "Unstar"));

    let effects = key(&mut app, 's');

    assert_eq!(effects, Vec::<Effect>::new());
    assert!(is_closed(&app));
}

// --- more cards that open no menu ---------------------------------------------

#[test]
fn a_tombstone_main_commit_opens_no_menu() {
    let mut app = thread_by("myuron", 0, vec![], vec![]);
    open_menu(&mut app);
    press(&mut app, KeyCode::Esc);
    let req = delete(&mut app);
    app.update(done(MAIN, req));

    assert_eq!(key(&mut app, '.'), Vec::<Effect>::new());
    assert_eq!(app.modal(), None);
    assert_eq!(app.menu(), None);
}

#[test]
fn a_profile_without_cards_or_not_showing_its_list_opens_no_menu() {
    let mut app = log_app_of(fixture_me(), vec![]);
    let effects = open_profile(&mut app, "alice");
    assert_eq!(key(&mut app, '.'), Vec::<Effect>::new(), "not loaded");
    assert_eq!(app.menu(), None, "not loaded");

    deliver_user(&mut app, &effects, "alice", user_resp("alice", json!({})));
    deliver_commits(
        &mut app,
        &effects,
        "alice",
        ProfileTab::Commits,
        vec![],
        None,
    );
    assert_eq!(key(&mut app, '.'), Vec::<Effect>::new(), "no cards");
    assert_eq!(app.menu(), None, "no cards");

    // The same profile with a card is what the silence above is measured against.
    let mut with_card = profile_of(vec![theirs("c0")]);
    open_menu(&mut with_card);
}

#[test]
fn a_show_more_row_opens_no_menu() {
    let replies = vec![item_of(merge(
        commit_json("r1", "bob", 600),
        json!({"parent_id": MAIN}),
    ))];
    let mut app = opened(120, 40, vec![], 3, replies, Some("next"));
    // The main commit takes the menu, so the silence below is that of the row.
    open_menu(&mut app);
    press(&mut app, KeyCode::Esc);
    // Entries: the main commit, r1, "Show more".
    down(&mut app, 2);

    assert_eq!(key(&mut app, '.'), Vec::<Effect>::new());
    assert_eq!(app.modal(), None);
    assert_eq!(app.menu(), None);
}
