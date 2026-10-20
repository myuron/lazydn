use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::Mutex;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use url::{Host, Url};

use crate::action::Action;
use crate::app::ToastKind;
use crate::text::is_unsafe_char;

/// Starts the user's browser. Tests give a fake, so that they start no process.
pub trait Launcher {
    fn open(&self, url: &Url) -> Result<(), String>;
}

/// Puts text on the user's clipboard. Tests give a fake.
pub trait Clipboard {
    fn copy(&self, text: &str) -> Result<(), String>;
}

/// Server and OS messages are cut here so that one cannot fill the toast.
const REASON_MAX: usize = 64;

fn clean(text: &str) -> String {
    text.chars()
        .filter(|c| !is_unsafe_char(*c))
        .take(REASON_MAX)
        .collect()
}

fn error_notice(text: String) -> Vec<Action> {
    vec![Action::Notice {
        kind: ToastKind::Error,
        text,
    }]
}

fn is_loopback(url: &Url) -> bool {
    match url.host() {
        Some(Host::Domain(name)) => name == "localhost",
        Some(Host::Ipv4(ip)) => ip.is_loopback(),
        Some(Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    }
}

/// The URL to open, or `None` when it is not on the origin of `base_url`
/// over https (or plain http to a loopback host).
fn approval_url(raw: &str, base_url: &str) -> Option<Url> {
    let url = Url::parse(raw).ok()?;
    let base = Url::parse(base_url).ok()?;
    let secure = match url.scheme() {
        "https" => true,
        "http" => is_loopback(&url),
        _ => false,
    };
    let plain = url.username().is_empty() && url.password().is_none();
    (secure && plain && url.origin() == base.origin()).then_some(url)
}

/// Opens `raw` through `launcher` after checking that it is on the origin of `base_url`:
/// `https`, or `http` to a loopback host. Returns a `Notice` when it refuses or the launch fails.
pub fn open_url(launcher: &dyn Launcher, base_url: &str, raw: &str) -> Vec<Action> {
    let Some(url) = approval_url(raw, base_url) else {
        return error_notice("refusing to open a URL that is not on the approval host".into());
    };
    match launcher.open(&url) {
        Ok(()) => Vec::new(),
        Err(reason) => error_notice(format!("could not open the browser: {}", clean(&reason))),
    }
}

/// Copies `text` through `clipboard`. Returns a `Notice` when the copy fails.
pub fn copy_text(clipboard: &dyn Clipboard, text: &str) -> Vec<Action> {
    match clipboard.copy(text) {
        Ok(()) => Vec::new(),
        Err(reason) => error_notice(format!("could not copy: {}", clean(&reason))),
    }
}

/// The escape sequence that asks a terminal to set its clipboard to `text`.
pub fn osc52_sequence(text: &str) -> String {
    format!("\x1b]52;c;{}\x07", STANDARD.encode(text))
}

/// Copies by writing OSC 52 to the terminal. The terminal never says whether it took it.
pub struct Osc52Clipboard<W: Write> {
    out: Mutex<W>,
}

impl<W: Write> Osc52Clipboard<W> {
    pub fn new(out: W) -> Self {
        Self {
            out: Mutex::new(out),
        }
    }
}

impl<W: Write> Clipboard for Osc52Clipboard<W> {
    fn copy(&self, text: &str) -> Result<(), String> {
        let mut out = self
            .out
            .lock()
            .map_err(|_| "output is poisoned".to_string())?;
        out.write_all(osc52_sequence(text).as_bytes())
            .and_then(|()| out.flush())
            .map_err(|e| e.to_string())
    }
}

/// The command that opens `url` in the desktop's browser; the URL is one argument, never a shell string.
pub fn open_command(url: &Url) -> Command {
    let program = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let mut command = Command::new(program);
    command.arg(url.as_str());
    command
}

/// Starts `open_command` with stdio closed and reaps the child.
pub struct SystemLauncher;

impl Launcher for SystemLauncher {
    fn open(&self, url: &Url) -> Result<(), String> {
        let mut child = open_command(url)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| e.to_string())?;
        let reap = move || {
            let _ = child.wait();
        };
        // The launcher may run outside a runtime, where spawn_blocking would panic.
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => drop(handle.spawn_blocking(reap)),
            Err(_) => drop(std::thread::spawn(reap)),
        }
        Ok(())
    }
}
