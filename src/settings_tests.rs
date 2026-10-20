use crossterm::event::KeyCode;

use crate::action::{Action, Effect, Failure};
use crate::app::{App, Me, Modal, Prefs, Screen, SettingsTab, Theme};
use crate::feed::{LoadState, ReqId};
use crate::model::Page;
use crate::profile_tests::{upage, user_of};
use crate::test_support::*;
use crate::thread_tests::user_json;

// --- fixtures ---------------------------------------------------------------

pub(crate) fn person(name: &str) -> crate::model::User {
    user_of(user_json(name))
}

pub(crate) fn people(names: &[&str]) -> Vec<crate::model::User> {
    names.iter().map(|n| person(n)).collect()
}

pub(crate) fn page_of(names: &[&str], cursor: Option<&str>) -> Page<crate::model::User> {
    upage(people(names), cursor)
}

/// A signed-in app on the Settings screen, Profile tab.
pub(crate) fn settings_app(w: u16, h: u16) -> App {
    let mut app = signed_in(w, h);
    type_str(&mut app, "gs");
    app
}

pub(crate) fn settings_app_for(me: Me, w: u16, h: u16) -> App {
    let mut app = App::new(crate::app::Session::SignedIn(me), NOW, time::UtcOffset::UTC);
    app.update(Action::Resize(w, h));
    type_str(&mut app, "gs");
    app
}

/// Opens the Safety tab and returns the effects it produced.
pub(crate) fn open_safety(app: &mut App) -> Vec<Effect> {
    type_str(app, "3")
}

pub(crate) fn muted_fetches(effects: &[Effect]) -> Vec<Option<String>> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::FetchMuted { cursor, .. } => Some(cursor.clone()),
            _ => None,
        })
        .collect()
}

pub(crate) fn blocked_fetches(effects: &[Effect]) -> Vec<Option<String>> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::FetchBlocked { cursor, .. } => Some(cursor.clone()),
            _ => None,
        })
        .collect()
}

/// Falls back to a bogus id so that a missing effect fails the assertions, not an unwrap.
pub(crate) fn muted_req(effects: &[Effect]) -> ReqId {
    effects
        .iter()
        .find_map(|e| match e {
            Effect::FetchMuted { req, .. } => Some(*req),
            _ => None,
        })
        .unwrap_or(ReqId(u64::MAX))
}

pub(crate) fn blocked_req(effects: &[Effect]) -> ReqId {
    effects
        .iter()
        .find_map(|e| match e {
            Effect::FetchBlocked { req, .. } => Some(*req),
            _ => None,
        })
        .unwrap_or(ReqId(u64::MAX))
}

pub(crate) fn load_muted(app: &mut App, effects: &[Effect], names: &[&str], cursor: Option<&str>) {
    app.update(Action::MutedLoaded {
        req: muted_req(effects),
        page: page_of(names, cursor),
    });
}

pub(crate) fn load_blocked(
    app: &mut App,
    effects: &[Effect],
    names: &[&str],
    cursor: Option<&str>,
) {
    app.update(Action::BlockedLoaded {
        req: blocked_req(effects),
        page: page_of(names, cursor),
    });
}

fn usernames(users: &[crate::model::User]) -> Vec<&str> {
    users.iter().map(|u| u.username.as_str()).collect()
}

fn fetch_count(effects: &[Effect]) -> usize {
    muted_fetches(effects).len() + blocked_fetches(effects).len()
}

// --- 1. tabs ----------------------------------------------------------------

#[test]
fn g_s_opens_settings_on_the_profile_tab() {
    let app = settings_app(120, 30);
    assert_eq!(app.screen(), Screen::Settings);
    assert_eq!(app.settings().tab, SettingsTab::Profile);
}

#[test]
fn keys_1_2_3_switch_the_settings_tab() {
    let mut app = settings_app(120, 30);
    type_str(&mut app, "2");
    assert_eq!(app.settings().tab, SettingsTab::Appearance);
    type_str(&mut app, "3");
    assert_eq!(app.settings().tab, SettingsTab::Safety);
    type_str(&mut app, "1");
    assert_eq!(app.settings().tab, SettingsTab::Profile);
}

#[test]
fn the_tab_keys_do_nothing_while_nav_has_the_focus() {
    let mut app = settings_app(120, 30);
    press(&mut app, KeyCode::Tab);
    let effects = type_str(&mut app, "23");
    assert_eq!(app.settings().tab, SettingsTab::Profile);
    assert_eq!(fetch_count(&effects), 0);
}

#[test]
fn the_tab_actions_are_applied_directly_too() {
    let mut app = settings_app(120, 30);
    app.update(Action::SwitchSettingsTab(SettingsTab::Appearance));
    assert_eq!(app.settings().tab, SettingsTab::Appearance);
}

#[test]
fn a_guest_cannot_open_settings() {
    let mut app = guest(120, 30);
    press(&mut app, KeyCode::Esc);
    type_str(&mut app, "gs");
    assert!(matches!(app.screen(), Screen::Log(_)));
    assert_eq!(app.modal(), Some(Modal::SignIn));
}

#[test]
fn the_default_prefs_are_dark_and_off_with_three_reply_previews() {
    assert_eq!(
        Prefs::default(),
        Prefs {
            ascii: false,
            truecolor: true,
            reply_preview_limit: 3,
            theme: Theme::Dark,
            collapse_nav: false,
            lane_dimming: false,
        }
    );
}

// --- 2. Safety fetches ------------------------------------------------------

#[test]
fn the_safety_tab_asks_for_muted_and_blocked_once_each() {
    let mut app = settings_app(120, 30);
    let effects = open_safety(&mut app);
    assert_eq!(muted_fetches(&effects), vec![None]);
    assert_eq!(blocked_fetches(&effects), vec![None]);
    assert_eq!(effects.len(), 2);
}

#[test]
fn the_other_tabs_fetch_nothing() {
    let mut app = settings_app(120, 30);
    assert_eq!(fetch_count(&type_str(&mut app, "2")), 0);
    assert_eq!(fetch_count(&type_str(&mut app, "1")), 0);
}

#[test]
fn reopening_the_safety_tab_fetches_nothing_while_the_first_request_is_pending() {
    let mut app = settings_app(120, 30);
    assert_eq!(fetch_count(&open_safety(&mut app)), 2);
    type_str(&mut app, "1");
    assert_eq!(fetch_count(&open_safety(&mut app)), 0);
}

#[test]
fn reopening_the_safety_tab_fetches_nothing_once_loaded() {
    let mut app = settings_app(120, 30);
    let effects = open_safety(&mut app);
    assert_eq!(fetch_count(&effects), 2);
    load_muted(&mut app, &effects, &["alice"], None);
    load_blocked(&mut app, &effects, &[], None);
    type_str(&mut app, "1");
    assert_eq!(fetch_count(&open_safety(&mut app)), 0);
}

// --- 3. responses -----------------------------------------------------------

#[test]
fn the_responses_fill_the_muted_and_blocked_lists() {
    let mut app = settings_app(120, 30);
    let effects = open_safety(&mut app);
    load_muted(&mut app, &effects, &["alice", "bob"], Some("c1"));
    load_blocked(&mut app, &effects, &["carol"], None);
    let state = app.settings();
    assert_eq!(usernames(&state.muted.users), vec!["alice", "bob"]);
    assert_eq!(state.muted.next_cursor.as_deref(), Some("c1"));
    assert!(state.muted.pending.is_none());
    assert!(state.muted.loaded_once);
    assert_eq!(usernames(&state.blocked.users), vec!["carol"]);
    assert_eq!(state.blocked.next_cursor, None);
    assert!(state.blocked.pending.is_none());
    assert!(state.blocked.loaded_once);
}

#[test]
fn a_response_for_another_request_is_dropped() {
    let mut app = settings_app(120, 30);
    let effects = open_safety(&mut app);
    let stale = |effects: &[Effect], by: u64| ReqId(muted_req(effects).0.wrapping_add(by));
    app.update(Action::MutedLoaded {
        req: stale(&effects, 100),
        page: page_of(&["alice"], None),
    });
    app.update(Action::BlockedLoaded {
        req: ReqId(blocked_req(&effects).0.wrapping_add(100)),
        page: page_of(&["carol"], None),
    });
    let state = app.settings();
    assert!(state.muted.users.is_empty());
    assert!(state.blocked.users.is_empty());
    assert!(state.muted.pending.is_some());
    assert!(state.blocked.pending.is_some());
    assert!(!state.muted.loaded_once);
    assert!(!state.blocked.loaded_once);
}

#[test]
fn a_response_to_a_superseded_request_is_dropped_after_a_refresh() {
    let mut app = settings_app(120, 30);
    let first = open_safety(&mut app);
    let second = press(&mut app, KeyCode::Char('R'));
    load_muted(&mut app, &first, &["old"], None);
    load_blocked(&mut app, &first, &["old"], None);
    assert!(app.settings().muted.users.is_empty());
    assert!(app.settings().blocked.users.is_empty());
    load_muted(&mut app, &second, &["new"], None);
    load_blocked(&mut app, &second, &["new"], None);
    assert_eq!(usernames(&app.settings().muted.users), vec!["new"]);
    assert_eq!(usernames(&app.settings().blocked.users), vec!["new"]);
}

#[test]
fn a_failure_is_kept_per_list_and_a_stale_failure_is_dropped() {
    let mut app = settings_app(120, 30);
    let effects = open_safety(&mut app);
    app.update(Action::MutedFailed {
        req: ReqId(muted_req(&effects).0.wrapping_add(100)),
        failure: Failure::new("http_500"),
    });
    assert_eq!(app.settings().muted.load, LoadState::Idle);
    app.update(Action::MutedFailed {
        req: muted_req(&effects),
        failure: Failure::new("http_500"),
    });
    assert_eq!(
        app.settings().muted.load,
        LoadState::Failed {
            code: "http_500".into(),
            append: false,
        }
    );
    assert!(app.settings().muted.pending.is_none());
    assert!(app.settings().blocked.pending.is_some());
    assert_eq!(app.settings().blocked.load, LoadState::Idle);
}

// --- Safety: R and Goto -----------------------------------------------------

#[test]
fn r_on_the_safety_tab_refetches_both_lists() {
    let mut app = settings_app(120, 30);
    let effects = open_safety(&mut app);
    load_muted(&mut app, &effects, &["alice"], None);
    load_blocked(&mut app, &effects, &["carol"], None);
    let effects = press(&mut app, KeyCode::Char('R'));
    assert_eq!(muted_fetches(&effects), vec![None]);
    assert_eq!(blocked_fetches(&effects), vec![None]);
    assert_eq!(effects.len(), 2);
}

#[test]
fn r_after_a_failure_retries_the_failed_list_from_the_first_page() {
    let mut app = settings_app(120, 30);
    let effects = open_safety(&mut app);
    app.update(Action::MutedFailed {
        req: muted_req(&effects),
        failure: Failure::new("http_500"),
    });
    let again = press(&mut app, KeyCode::Char('R'));
    assert_eq!(muted_fetches(&again), vec![None]);
    assert_eq!(app.settings().muted.load, LoadState::Idle);
}

#[test]
fn r_on_the_other_tabs_fetches_nothing() {
    let mut app = settings_app(120, 30);
    assert!(press(&mut app, KeyCode::Char('R')).is_empty());
    type_str(&mut app, "2");
    assert!(press(&mut app, KeyCode::Char('R')).is_empty());
}

#[test]
fn going_back_to_settings_uses_the_cached_lists() {
    let mut app = settings_app(120, 30);
    let effects = open_safety(&mut app);
    load_muted(&mut app, &effects, &["alice"], None);
    load_blocked(&mut app, &effects, &[], None);
    type_str(&mut app, "gl");
    let back = type_str(&mut app, "gs");
    assert_eq!(app.screen(), Screen::Settings);
    assert_eq!(fetch_count(&back), 0);
    assert_eq!(usernames(&app.settings().muted.users), vec!["alice"]);
    assert_eq!(app.settings().tab, SettingsTab::Safety);
    assert!(app.settings().blocked.loaded_once);
}

#[test]
fn the_safety_lists_have_no_selection_to_move() {
    let mut app = settings_app(120, 30);
    let effects = open_safety(&mut app);
    load_muted(&mut app, &effects, &["alice", "bob"], Some("c1"));
    load_blocked(&mut app, &effects, &["carol"], None);
    for key in [
        KeyCode::Char('j'),
        KeyCode::Char('k'),
        KeyCode::Char('G'),
        KeyCode::Enter,
    ] {
        assert!(press(&mut app, key).is_empty(), "{key:?}");
    }
    assert_eq!(app.screen(), Screen::Settings);
}

// --- Safety: a first fetch that failed is retried on re-entry ----------------

fn fail_muted(app: &mut App, effects: &[Effect]) {
    app.update(Action::MutedFailed {
        req: muted_req(effects),
        failure: Failure::new("http_500"),
    });
}

#[test]
fn reopening_the_safety_tab_retries_a_list_whose_first_fetch_failed() {
    let mut app = settings_app(120, 30);
    let effects = open_safety(&mut app);
    fail_muted(&mut app, &effects);
    type_str(&mut app, "1");
    let again = open_safety(&mut app);
    assert_eq!(muted_fetches(&again), vec![None]);
    assert_eq!(blocked_fetches(&again), Vec::<Option<String>>::new());
}

#[test]
fn going_back_to_settings_on_the_safety_tab_retries_a_failed_first_fetch() {
    let mut app = settings_app(120, 30);
    let effects = open_safety(&mut app);
    fail_muted(&mut app, &effects);
    type_str(&mut app, "gl");
    let back = type_str(&mut app, "gs");
    assert_eq!(app.settings().tab, SettingsTab::Safety);
    assert_eq!(muted_fetches(&back), vec![None]);
    assert_eq!(blocked_fetches(&back), Vec::<Option<String>>::new());
}

#[test]
fn a_failure_after_users_were_loaded_is_not_retried_on_re_entry() {
    let mut app = settings_app(120, 30);
    let effects = open_safety(&mut app);
    load_muted(&mut app, &effects, &["alice"], None);
    load_blocked(&mut app, &effects, &[], None);
    let refetch = press(&mut app, KeyCode::Char('R'));
    fail_muted(&mut app, &refetch);
    load_blocked(&mut app, &refetch, &[], None);
    type_str(&mut app, "1");
    assert_eq!(fetch_count(&open_safety(&mut app)), 0);
    assert_eq!(usernames(&app.settings().muted.users), vec!["alice"]);
}
