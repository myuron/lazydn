use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use url::Url;

use super::external::{Clipboard, Launcher};
use super::run::{ClientKind, Control, Exec, Runtime, Store};
use crate::action::{Action, Effect};
use crate::app::{LogTab, ToastKind};
use crate::drafts::{DraftKey, Drafts};
use crate::editor::Editor;
use crate::feed::ReqId;
use crate::feed_tests::{FOLLOWING, deliver, started};
use crate::test_support::{NOW, ctrl, fixture_me, signed_in, type_str};

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
    saved_drafts: Shared<(String, Drafts)>,
    drafts_answer: Result<(), String>,
    loaded_accounts: Shared<String>,
    drafts_on_disk: Drafts,
}

impl Store for FakeStore {
    fn save_last_visits(&mut self, loaded: &HashMap<LogTab, String>) -> Result<(), String> {
        self.saved.borrow_mut().push(loaded.clone());
        self.answer.clone()
    }

    fn save_drafts(&mut self, account: &str, drafts: &Drafts) -> Result<(), String> {
        self.saved_drafts
            .borrow_mut()
            .push((account.to_string(), drafts.clone()));
        self.drafts_answer.clone()
    }

    fn load_drafts(&mut self, account: &str) -> Drafts {
        self.loaded_accounts.borrow_mut().push(account.to_string());
        self.drafts_on_disk.clone()
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
    saved_drafts: Shared<(String, Drafts)>,
    loaded_accounts: Shared<String>,
}

fn rig_with(client: ClientKind, copy: Result<(), String>, save: Result<(), String>) -> Rig {
    let app = signed_in(120, 40);
    let spawned = Shared::default();
    let opened = Shared::default();
    let copied = Shared::default();
    let saved = Shared::default();
    let saved_drafts = Shared::default();
    let loaded_accounts = Shared::default();
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
            saved_drafts: saved_drafts.clone(),
            drafts_answer: Ok(()),
            loaded_accounts: loaded_accounts.clone(),
            drafts_on_disk: Drafts::default(),
        },
        base_url: BASE.into(),
    };
    Rig {
        runtime,
        spawned,
        opened,
        copied,
        saved,
        saved_drafts,
        loaded_accounts,
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

// --- posting a commit -------------------------------------------------------------

#[test]
fn a_commit_to_post_goes_to_the_executor_with_the_client_in_force() {
    let mut rig = rig_with(ClientKind::Signed, Ok(()), Ok(()));
    let create = Effect::CreateCommit {
        body: "hello".into(),
        parent_id: None,
        quoted_commit_id: None,
        req: ReqId(3),
    };

    rig.runtime.apply(vec![create.clone()]);

    assert_eq!(*rig.spawned.borrow(), vec![(ClientKind::Signed, create)]);
}

// --- drafts -------------------------------------------------------------------------

const ACCOUNT: &str = "0190a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b";
const OTHER_ACCOUNT: &str = "0190ffff-0000-7000-8000-000000000001";

fn account_me() -> crate::app::Me {
    crate::app::Me {
        id: ACCOUNT.into(),
        ..fixture_me()
    }
}

fn drafts_with(text: &str) -> Drafts {
    let mut drafts = Drafts::default();
    drafts.set(DraftKey::New, Editor::from_text(text));
    drafts
}

/// A rig whose app is signed in as an account that can have a drafts file.
fn account_rig() -> Rig {
    let mut rig = rig_with(ClientKind::Signed, Ok(()), Ok(()));
    let mut app = crate::app::App::new(
        crate::app::Session::SignedIn(account_me()),
        NOW,
        time::UtcOffset::UTC,
    );
    app.update(Action::Resize(120, 40));
    rig.runtime.app = app;
    rig
}

#[test]
fn saving_drafts_goes_to_the_store_on_the_spot_and_not_to_the_executor() {
    let mut rig = account_rig();
    let drafts = drafts_with("hello");

    let control = rig.runtime.apply(vec![Effect::SaveDrafts {
        account: ACCOUNT.into(),
        drafts: drafts.clone(),
    }]);

    assert_eq!(control, Control::Continue);
    assert_eq!(
        *rig.saved_drafts.borrow(),
        vec![(ACCOUNT.to_string(), drafts)]
    );
    assert!(rig.spawned.borrow().is_empty());
    assert!(rig.runtime.app.toast().is_none());
}

#[test]
fn a_failed_draft_save_shows_a_toast() {
    let mut rig = account_rig();
    rig.runtime.store.drafts_answer = Err("disk full".into());

    let control = rig.runtime.apply(vec![Effect::SaveDrafts {
        account: ACCOUNT.into(),
        drafts: drafts_with("hello"),
    }]);

    assert_eq!(control, Control::Continue);
    let toast = rig.runtime.app.toast().expect("a toast");
    assert_eq!(toast.kind, ToastKind::Error);
    assert!(
        toast.text.contains("drafts could not be saved"),
        "{}",
        toast.text
    );
    assert!(toast.text.contains("disk full"), "{}", toast.text);
}

#[test]
fn a_failed_draft_save_just_before_quitting_is_reported_with_the_quit() {
    let mut rig = account_rig();
    rig.runtime.store.drafts_answer = Err("disk full".into());

    let control = rig.runtime.apply(vec![
        Effect::SaveDrafts {
            account: ACCOUNT.into(),
            drafts: drafts_with("hello"),
        },
        Effect::Quit,
    ]);

    match control {
        Control::Quit {
            save_error: Some(error),
        } => assert!(error.contains("disk full"), "{error}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_draft_save_that_worked_adds_no_error_to_the_quit() {
    let mut rig = account_rig();

    let control = rig.runtime.apply(vec![
        Effect::SaveDrafts {
            account: ACCOUNT.into(),
            drafts: drafts_with("hello"),
        },
        Effect::Quit,
    ]);

    assert_eq!(control, Control::Quit { save_error: None });
    assert_eq!(rig.saved_drafts.borrow().len(), 1);
}

#[test]
fn both_save_failures_are_reported_when_quitting() {
    let mut rig = account_rig();
    rig.runtime.store.answer = Err("state broke".into());
    rig.runtime.store.drafts_answer = Err("drafts broke".into());

    let control = rig.runtime.apply(vec![
        Effect::SaveDrafts {
            account: ACCOUNT.into(),
            drafts: drafts_with("hello"),
        },
        Effect::Quit,
    ]);

    match control {
        Control::Quit {
            save_error: Some(error),
        } => {
            assert!(error.contains("state broke"), "{error}");
            assert!(error.contains("drafts broke"), "{error}");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn ctrl_c_in_a_composer_writes_the_draft_through_the_store_and_quits() {
    let mut rig = account_rig();
    type_str(&mut rig.runtime.app, "c");
    type_str(&mut rig.runtime.app, "hello");
    assert!(rig.runtime.app.composer_focused());
    let effects = ctrl(&mut rig.runtime.app, 'c');

    let control = rig.runtime.apply(effects);

    assert_eq!(control, Control::Quit { save_error: None });
    let saved = rig.saved_drafts.borrow();
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].0, ACCOUNT);
    assert_eq!(saved[0].1.text(&DraftKey::New), Some("hello".to_string()));
}

#[test]
fn loading_drafts_reads_the_store_on_the_spot_and_the_app_takes_what_was_read() {
    let mut rig = account_rig();
    rig.runtime.store.drafts_on_disk = drafts_with("from disk");

    let control = rig.runtime.apply(vec![Effect::LoadDrafts {
        account: ACCOUNT.into(),
    }]);

    assert_eq!(control, Control::Continue);
    assert_eq!(*rig.loaded_accounts.borrow(), vec![ACCOUNT.to_string()]);
    assert_eq!(
        rig.runtime.app.drafts().text(&DraftKey::New),
        Some("from disk".to_string())
    );
    assert!(rig.spawned.borrow().is_empty());
}

#[test]
fn drafts_read_for_another_account_are_not_taken() {
    let mut rig = account_rig();
    rig.runtime.store.drafts_on_disk = drafts_with("not mine");

    rig.runtime.apply(vec![Effect::LoadDrafts {
        account: OTHER_ACCOUNT.into(),
    }]);

    assert!(rig.runtime.app.drafts().is_empty());
}

#[test]
fn a_restored_session_gets_its_drafts_as_soon_as_the_user_is_known() {
    let mut rig = rig_with(ClientKind::Signed, Ok(()), Ok(()));
    rig.runtime.app =
        crate::app::App::new(crate::app::Session::Restoring, NOW, time::UtcOffset::UTC);
    rig.runtime.store.drafts_on_disk = drafts_with("from disk");
    rig.runtime.handle(Action::Resize(120, 40));
    rig.runtime.handle(Action::Init);
    let req = rig
        .spawned
        .borrow()
        .iter()
        .find_map(|(_, e)| match e {
            Effect::FetchMe { req } => Some(*req),
            _ => None,
        })
        .expect("Init asks who the user is");
    let response = serde_json::from_value(crate::notifications_tests::me_json(
        0,
        serde_json::json!({"id": ACCOUNT}),
    ))
    .unwrap();

    rig.runtime.handle(Action::MeLoaded {
        req,
        response: Box::new(response),
    });

    assert_eq!(*rig.loaded_accounts.borrow(), vec![ACCOUNT.to_string()]);
    assert_eq!(
        rig.runtime.app.drafts().text(&DraftKey::New),
        Some("from disk".to_string())
    );
}

#[test]
fn the_file_store_keeps_drafts_in_the_state_dir_per_account() {
    let tmp = tempfile::tempdir().unwrap();
    let mut store = super::run::FileStore {
        dir: tmp.path().to_path_buf(),
    };

    store.save_drafts(ACCOUNT, &drafts_with("kept")).unwrap();

    assert!(tmp.path().join(format!("drafts-{ACCOUNT}.toml")).is_file());
    assert_eq!(
        store.load_drafts(ACCOUNT).text(&DraftKey::New),
        Some("kept".to_string())
    );
    assert!(store.load_drafts(OTHER_ACCOUNT).is_empty());
}

#[test]
fn the_file_store_refuses_an_account_id_that_cannot_name_a_file() {
    let tmp = tempfile::tempdir().unwrap();
    let mut store = super::run::FileStore {
        dir: tmp.path().to_path_buf(),
    };

    assert!(store.save_drafts("../evil", &drafts_with("x")).is_err());
    assert!(store.load_drafts("../evil").is_empty());
    assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 0);
}

#[test]
fn a_failed_write_is_told_to_the_app_so_that_quitting_writes_the_drafts_again() {
    let mut rig = account_rig();
    rig.runtime.store.drafts_answer = Err("disk full".into());
    type_str(&mut rig.runtime.app, "c");
    type_str(&mut rig.runtime.app, "hello");
    rig.runtime
        .handle(Action::Tick(NOW + time::Duration::seconds(2)));
    assert_eq!(rig.saved_drafts.borrow().len(), 1);

    let control = rig.runtime.handle(Action::Quit);

    assert_eq!(rig.saved_drafts.borrow().len(), 2);
    match control {
        Control::Quit {
            save_error: Some(error),
        } => assert!(error.contains("disk full"), "{error}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn when_both_writes_fail_each_error_is_named() {
    let mut rig = account_rig();
    rig.runtime.store.answer = Err("a".into());
    rig.runtime.store.drafts_answer = Err("b".into());

    let control = rig.runtime.apply(vec![
        Effect::SaveDrafts {
            account: ACCOUNT.into(),
            drafts: drafts_with("hello"),
        },
        Effect::Quit,
    ]);

    assert_eq!(
        control,
        Control::Quit {
            save_error: Some("state: a; drafts: b".into())
        }
    );
}

// --- links from the server, stars, saves and pastéis -----------------------------------------

#[test]
fn a_link_to_copy_reaches_the_clipboard_and_the_toast_says_copied_link() {
    let mut rig = rig();

    rig.runtime.apply(vec![Effect::CopyLink(
        "https://dev-null.sh/@alice/commits/c1".into(),
    )]);

    assert_eq!(
        *rig.copied.borrow(),
        vec!["https://dev-null.sh/@alice/commits/c1".to_string()]
    );
    assert!(rig.spawned.borrow().is_empty());
    let toast = rig.runtime.app.toast().expect("a toast");
    assert_eq!(
        (toast.kind, toast.text.as_str()),
        (ToastKind::Success, "Copied link")
    );
}

#[test]
fn a_link_from_another_origin_is_not_copied_and_an_error_toast_says_so() {
    let mut rig = rig();

    rig.runtime
        .apply(vec![Effect::CopyLink("https://evil.example/x".into())]);

    assert!(rig.copied.borrow().is_empty());
    assert!(rig.spawned.borrow().is_empty());
    let toast = rig.runtime.app.toast().expect("a toast");
    assert_eq!(toast.kind, ToastKind::Error);
}

#[test]
fn a_link_copy_that_fails_shows_the_error_and_not_copied_link() {
    let mut rig = rig_with(ClientKind::Guest, Err("no tty".into()), Ok(()));

    rig.runtime.apply(vec![Effect::CopyLink(
        "https://dev-null.sh/@alice/commits/c1".into(),
    )]);

    let toast = rig.runtime.app.toast().expect("a toast");
    assert_eq!(toast.kind, ToastKind::Error);
    assert!(toast.text.contains("could not copy"), "{}", toast.text);
}

#[test]
fn a_refused_open_and_a_refused_copy_say_the_same_thing() {
    let mut open = rig();
    let mut copy = rig();

    open.runtime
        .apply(vec![Effect::OpenUrl("https://evil.example/x".into())]);
    copy.runtime
        .apply(vec![Effect::CopyLink("https://evil.example/x".into())]);

    let text_of = |rig: &Rig| rig.runtime.app.toast().map(|t| t.text.clone());
    assert_eq!(
        text_of(&open),
        Some("refusing a link outside https://dev-null.sh".to_string())
    );
    assert_eq!(text_of(&open), text_of(&copy));
}

#[test]
fn star_save_and_pastel_go_to_the_executor_with_the_client_in_force() {
    let mut rig = rig_with(ClientKind::Signed, Ok(()), Ok(()));
    let effects = vec![
        Effect::StarCommit {
            id: "c1".into(),
            on: true,
            req: ReqId(4),
        },
        Effect::BookmarkCommit {
            id: "c1".into(),
            on: false,
            req: ReqId(5),
        },
        Effect::GivePastel {
            id: "c1".into(),
            req: ReqId(6),
        },
    ];

    rig.runtime.apply(effects.clone());

    let expected: Vec<_> = effects
        .into_iter()
        .map(|e| (ClientKind::Signed, e))
        .collect();
    assert_eq!(*rig.spawned.borrow(), expected);
}
