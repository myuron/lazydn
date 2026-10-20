use std::cell::RefCell;
use std::io::{self, Write};
use std::sync::{Arc, Mutex};

use url::Url;

use super::external::{
    Clipboard, Launcher, Osc52Clipboard, copy_text, open_command, open_url, osc52_sequence,
};
use crate::action::Action;
use crate::app::ToastKind;

const BASE: &str = "https://dev-null.sh";

/// Records the URLs it was asked to open and answers as told.
struct FakeLauncher {
    opened: RefCell<Vec<String>>,
    answer: Result<(), String>,
}

impl FakeLauncher {
    fn ok() -> Self {
        Self {
            opened: RefCell::new(Vec::new()),
            answer: Ok(()),
        }
    }

    fn failing(reason: &str) -> Self {
        Self {
            opened: RefCell::new(Vec::new()),
            answer: Err(reason.into()),
        }
    }

    fn opened(&self) -> Vec<String> {
        self.opened.borrow().clone()
    }
}

impl Launcher for FakeLauncher {
    fn open(&self, url: &Url) -> Result<(), String> {
        self.opened.borrow_mut().push(url.to_string());
        self.answer.clone()
    }
}

struct FakeClipboard {
    copied: RefCell<Vec<String>>,
    answer: Result<(), String>,
}

impl Clipboard for FakeClipboard {
    fn copy(&self, text: &str) -> Result<(), String> {
        self.copied.borrow_mut().push(text.into());
        self.answer.clone()
    }
}

fn is_error_notice(actions: &[Action]) -> bool {
    matches!(
        actions,
        [Action::Notice {
            kind: ToastKind::Error,
            ..
        }]
    )
}

// --- the browser is started only for the approval host ---------------------------------------------

#[test]
fn an_https_url_on_the_configured_host_is_opened_and_nothing_is_reported() {
    let launcher = FakeLauncher::ok();
    let url = "https://dev-null.sh/mobile/authorize?state=abc";

    let actions = open_url(&launcher, BASE, url);

    assert_eq!(actions, Vec::<Action>::new());
    assert_eq!(launcher.opened(), vec![url.to_string()]);
}

#[test]
fn an_http_url_on_a_loopback_host_is_opened_when_the_base_is_that_host() {
    for base in [
        "http://127.0.0.1:8080",
        "http://localhost:3000",
        "http://[::1]:9000",
    ] {
        let launcher = FakeLauncher::ok();
        let url = format!("{base}/mobile/authorize?state=abc");

        let actions = open_url(&launcher, base, &url);

        assert_eq!(actions, Vec::<Action>::new(), "{base}");
        assert_eq!(launcher.opened(), vec![url], "{base}");
    }
}

#[test]
fn a_url_the_app_would_never_have_built_is_refused_and_nothing_is_started() {
    let refused = [
        ("https://dev-null.sh", "http://dev-null.sh/mobile/authorize"),
        (
            "https://dev-null.sh",
            "https://evil.example/mobile/authorize",
        ),
        ("https://dev-null.sh", "https://dev-null.sh.evil.example/"),
        ("https://dev-null.sh", "http://127.0.0.1:9/mobile/authorize"),
        (
            "http://127.0.0.1:8080",
            "http://localhost:8080/mobile/authorize",
        ),
        ("https://dev-null.sh", "file:///etc/passwd"),
        ("https://dev-null.sh", "javascript:alert(1)"),
        ("https://dev-null.sh", "ftp://dev-null.sh/x"),
        (
            "https://dev-null.sh",
            "https://dev-null.sh:8443/mobile/authorize",
        ),
        (
            "http://127.0.0.1:8080",
            "http://127.0.0.1:9090/mobile/authorize",
        ),
        ("https://dev-null.sh", "--help"),
        ("https://dev-null.sh", ""),
    ];
    for (base, url) in refused {
        let launcher = FakeLauncher::ok();

        let actions = open_url(&launcher, base, url);

        assert!(is_error_notice(&actions), "{url}: {actions:?}");
        assert_eq!(launcher.opened(), Vec::<String>::new(), "{url}");
    }
}

#[test]
fn a_launch_that_fails_is_reported_with_the_reason() {
    let launcher = FakeLauncher::failing("xdg-open not found");

    let actions = open_url(&launcher, BASE, "https://dev-null.sh/mobile/authorize");

    assert!(is_error_notice(&actions), "{actions:?}");
    let Some(Action::Notice { text, .. }) = actions.first() else {
        panic!("{actions:?}");
    };
    assert!(text.contains("xdg-open not found"), "{text}");
}

#[test]
fn the_browser_command_gets_the_url_as_one_argument_and_no_shell() {
    let url = Url::parse("https://dev-null.sh/a?x=1&y=2;rm%20-rf").unwrap();

    let command = open_command(&url);

    let expected = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    assert_eq!(command.get_program(), expected);
    let args: Vec<_> = command.get_args().collect();
    assert_eq!(args, vec![std::ffi::OsStr::new(url.as_str())]);
}

// --- the clipboard -------------------------------------------------------------------------------

#[test]
fn copying_hands_the_text_to_the_clipboard_and_reports_nothing() {
    let clipboard = FakeClipboard {
        copied: RefCell::new(Vec::new()),
        answer: Ok(()),
    };

    let actions = copy_text(&clipboard, "https://dev-null.sh/x");

    assert_eq!(actions, Vec::<Action>::new());
    assert_eq!(*clipboard.copied.borrow(), vec!["https://dev-null.sh/x"]);
}

#[test]
fn a_copy_that_fails_is_reported_with_the_reason() {
    let clipboard = FakeClipboard {
        copied: RefCell::new(Vec::new()),
        answer: Err("broken pipe".into()),
    };

    let actions = copy_text(&clipboard, "x");

    assert!(is_error_notice(&actions), "{actions:?}");
    let Some(Action::Notice { text, .. }) = actions.first() else {
        panic!("{actions:?}");
    };
    assert!(text.contains("broken pipe"), "{text}");
}

#[test]
fn osc52_sets_the_clipboard_selection_with_base64_text() {
    assert_eq!(osc52_sequence("hello"), "\x1b]52;c;aGVsbG8=\x07");
}

/// A writer that several owners can read back.
#[derive(Clone, Default)]
struct Shared(Arc<Mutex<Vec<u8>>>);

impl Write for Shared {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn the_terminal_clipboard_writes_the_sequence_to_its_output() {
    let out = Shared::default();
    let clipboard = Osc52Clipboard::new(out.clone());

    let result = clipboard.copy("hello");

    assert_eq!(result, Ok(()));
    assert_eq!(*out.0.lock().unwrap(), b"\x1b]52;c;aGVsbG8=\x07");
}

struct BrokenWriter;

impl Write for BrokenWriter {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::Error::new(io::ErrorKind::BrokenPipe, "gone"))
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn the_terminal_clipboard_reports_a_write_that_fails() {
    let clipboard = Osc52Clipboard::new(BrokenWriter);

    assert!(clipboard.copy("hello").is_err());
}

#[test]
fn the_reason_of_a_failed_launch_loses_control_characters_and_is_cut_at_64() {
    let launcher = FakeLauncher::failing(&format!("bad\x1b[0m{}", "x".repeat(100)));

    let actions = open_url(&launcher, BASE, "https://dev-null.sh/mobile/authorize");

    let Some(Action::Notice { text, .. }) = actions.first() else {
        panic!("{actions:?}");
    };
    assert!(!text.chars().any(char::is_control), "{text:?}");
    assert!(
        text.contains(&format!("bad[0m{}", "x".repeat(58))),
        "{text}"
    );
    assert!(!text.contains(&"x".repeat(59)), "{text}");
}
