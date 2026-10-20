use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use url::Url;

use super::external::{Clipboard, Launcher};
use super::run::{ClientKind, Control, Exec, Runtime, Store};
use crate::action::{Action, Effect};
use crate::app::{LogTab, ToastKind};
use crate::feed::ReqId;
use crate::feed_tests::{FOLLOWING, deliver, started};
use crate::test_support::signed_in;

const BASE: &str = "https://dev-null.sh";

type Shared<T> = Rc<RefCell<Vec<T>>>;

struct FakeExec(Shared<(ClientKind, Effect)>);

impl Exec for FakeExec {
    fn spawn(&mut self, client: ClientKind, effect: Effect) {
        self.0.borrow_mut().push((client, effect));
    }
}

struct FakeLauncher(Shared<String>);

impl Launcher for FakeLauncher {
    fn open(&self, url: &Url) -> Result<(), String> {
        self.0.borrow_mut().push(url.to_string());
        Ok(())
    }
}

struct FakeClipboard {
    copied: Shared<String>,
    answer: Result<(), String>,
}

impl Clipboard for FakeClipboard {
    fn copy(&self, text: &str) -> Result<(), String> {
        self.copied.borrow_mut().push(text.into());
        self.answer.clone()
    }
}

struct FakeStore {
    saved: Shared<HashMap<LogTab, String>>,
    answer: Result<(), String>,
}

impl Store for FakeStore {
    fn save_last_visits(&mut self, loaded: &HashMap<LogTab, String>) -> Result<(), String> {
        self.saved.borrow_mut().push(loaded.clone());
        self.answer.clone()
    }
}

type TestRuntime = Runtime<FakeExec, FakeLauncher, FakeClipboard, FakeStore>;

/// A runtime on a fresh signed-in app, and handles to everything the fakes recorded.
struct Rig {
    runtime: TestRuntime,
    spawned: Shared<(ClientKind, Effect)>,
    opened: Shared<String>,
    copied: Shared<String>,
    saved: Shared<HashMap<LogTab, String>>,
}

fn rig_with(client: ClientKind, copy: Result<(), String>, save: Result<(), String>) -> Rig {
    let app = signed_in(120, 40);
    let spawned = Shared::default();
    let opened = Shared::default();
    let copied = Shared::default();
    let saved = Shared::default();
    let runtime = Runtime {
        app,
        client,
        exec: FakeExec(spawned.clone()),
        launcher: FakeLauncher(opened.clone()),
        clipboard: FakeClipboard {
            copied: copied.clone(),
            answer: copy,
        },
        store: FakeStore {
            saved: saved.clone(),
            answer: save,
        },
        base_url: BASE.into(),
    };
    Rig {
        runtime,
        spawned,
        opened,
        copied,
        saved,
    }
}

fn rig() -> Rig {
    rig_with(ClientKind::Guest, Ok(()), Ok(()))
}

fn fetch(n: u64) -> Effect {
    Effect::FetchLog {
        tab: LogTab::Latest,
        cursor: None,
        req: ReqId(n),
    }
}

// --- effect order -------------------------------------------------------------

#[test]
fn effects_are_spawned_in_order_with_the_client_in_force_when_they_come() {
    let mut rig = rig();
    let control = rig.runtime.apply(vec![
        fetch(1),
        Effect::UseSignedClient,
        fetch(2),
        Effect::UseGuestClient,
        fetch(3),
    ]);
    assert_eq!(control, Control::Continue);
    assert_eq!(
        *rig.spawned.borrow(),
        vec![
            (ClientKind::Guest, fetch(1)),
            (ClientKind::Signed, fetch(2)),
            (ClientKind::Guest, fetch(3)),
        ]
    );
}

#[test]
fn the_client_swap_stays_for_later_calls() {
    let mut rig = rig();
    rig.runtime.apply(vec![Effect::UseSignedClient]);
    rig.runtime.apply(vec![fetch(1)]);
    assert_eq!(rig.runtime.client, ClientKind::Signed);
    assert_eq!(*rig.spawned.borrow(), vec![(ClientKind::Signed, fetch(1))]);
}

#[test]
fn effects_from_an_action_are_spawned_with_the_current_client() {
    let mut rig = rig_with(ClientKind::Signed, Ok(()), Ok(()));
    let control = rig.runtime.handle(Action::Init);
    assert_eq!(control, Control::Continue);
    let spawned = rig.spawned.borrow();
    assert!(!spawned.is_empty());
    assert!(
        spawned
            .iter()
            .all(|(client, _)| *client == ClientKind::Signed)
    );
    assert!(spawned.iter().any(|(_, effect)| matches!(
        effect,
        Effect::FetchLog {
            tab: LogTab::Following,
            ..
        }
    )));
}

// --- quit ---------------------------------------------------------------------

#[test]
fn quit_stops_and_saves_the_heads_of_the_loaded_tabs() {
    let mut rig = rig();
    let (app, effects) = started(120, 40);
    rig.runtime.app = app;
    let page = crate::feed_tests::page(0, 3, None);
    deliver(&mut rig.runtime.app, FOLLOWING, &effects, page);
    let head = rig.runtime.app.feed(FOLLOWING).items[0].id.clone();

    let control = rig.runtime.handle(Action::Quit);

    assert_eq!(control, Control::Quit { save_error: None });
    let saved = rig.saved.borrow();
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0], HashMap::from([(LogTab::Following, head)]));
}

#[test]
fn quit_spawns_nothing_after_it_but_keeps_what_came_before() {
    let mut rig = rig();
    let control = rig.runtime.apply(vec![fetch(1), Effect::Quit, fetch(2)]);
    assert!(matches!(control, Control::Quit { .. }));
    assert_eq!(*rig.spawned.borrow(), vec![(ClientKind::Guest, fetch(1))]);
}

#[test]
fn a_failed_save_is_reported_with_the_quit() {
    let mut rig = rig_with(ClientKind::Guest, Ok(()), Err("disk full".into()));
    let control = rig.runtime.handle(Action::Quit);
    assert_eq!(
        control,
        Control::Quit {
            save_error: Some("disk full".into())
        }
    );
}

#[test]
fn nothing_is_saved_while_the_app_keeps_running() {
    let mut rig = rig();
    rig.runtime.handle(Action::Init);
    assert!(rig.saved.borrow().is_empty());
}

// --- browser and clipboard ----------------------------------------------------

#[test]
fn open_url_reaches_the_launcher_and_not_the_executor() {
    let mut rig = rig();
    rig.runtime.apply(vec![Effect::OpenUrl(
        "https://dev-null.sh/approve?x=1".into(),
    )]);
    assert_eq!(
        *rig.opened.borrow(),
        vec!["https://dev-null.sh/approve?x=1".to_string()]
    );
    assert!(rig.spawned.borrow().is_empty());
}

#[test]
fn a_url_off_the_approval_host_is_refused_with_a_toast() {
    let mut rig = rig();
    rig.runtime
        .apply(vec![Effect::OpenUrl("https://evil.example/x".into())]);
    assert!(rig.opened.borrow().is_empty());
    let toast = rig.runtime.app.toast().expect("a toast");
    assert_eq!(toast.kind, ToastKind::Error);
}

#[test]
fn copy_reaches_the_clipboard() {
    let mut rig = rig();
    rig.runtime.apply(vec![Effect::CopyToClipboard(
        "https://dev-null.sh/a".into(),
    )]);
    assert_eq!(
        *rig.copied.borrow(),
        vec!["https://dev-null.sh/a".to_string()]
    );
    assert!(rig.spawned.borrow().is_empty());
    assert!(rig.runtime.app.toast().is_none());
}

#[test]
fn a_failed_copy_shows_a_toast() {
    let mut rig = rig_with(ClientKind::Guest, Err("no tty".into()), Ok(()));
    rig.runtime
        .apply(vec![Effect::CopyToClipboard("text".into())]);
    let toast = rig.runtime.app.toast().expect("a toast");
    assert_eq!(toast.kind, ToastKind::Error);
    assert!(toast.text.contains("could not copy"), "{}", toast.text);
}

// --- sign-in effects ----------------------------------------------------------

#[test]
fn sign_in_effects_go_to_the_executor() {
    let mut rig = rig();
    let prepare = Effect::PrepareSignIn { create: true };
    let register = Effect::RegisterDevice {
        code: "c".into(),
        verifier: "v".into(),
    };
    rig.runtime.apply(vec![prepare.clone(), register.clone()]);
    assert_eq!(
        *rig.spawned.borrow(),
        vec![(ClientKind::Guest, prepare), (ClientKind::Guest, register)]
    );
}
