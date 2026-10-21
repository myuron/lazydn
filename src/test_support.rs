use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;

use crate::model::Lane;
use time::macros::datetime;
use time::{OffsetDateTime, UtcOffset};

use crate::action::{Action, Effect};
use crate::app::{App, Me, Session};
use crate::keymap::map_key;
use crate::ui;

/// Fixed clock so that relative times in every test are reproducible.
pub const NOW: OffsetDateTime = datetime!(2026-10-20 12:00 UTC);

/// Fixed account shown in the Aside and the Nav badge of every signed-in test.
pub fn fixture_me() -> Me {
    Me {
        id: "user-myuron".into(),
        name: "myuron".into(),
        handle: "myuron".into(),
        commits: 42,
        following: 10,
        followers: 12,
        pasteis_left: 7,
        pasteis_max: 10,
        bio: "Rust / TUI enjoyer".into(),
        unread: 12,
        pasteis_mode: true,
        lane: Lane::Index(1),
        preferred_lane: None,
        automatic_lane: 1,
        pasteis_resets_at: datetime!(2026-10-21 00:00 UTC),
        pasteis_received: 3,
        github_synced_at: Some(datetime!(2026-10-20 10:00 UTC)),
    }
}

pub fn signed_in(w: u16, h: u16) -> App {
    let mut app = App::new(Session::SignedIn(fixture_me()), NOW, UtcOffset::UTC);
    app.update(Action::Resize(w, h));
    app
}

pub fn guest(w: u16, h: u16) -> App {
    let mut app = App::new(Session::Guest, NOW, UtcOffset::UTC);
    app.update(Action::Resize(w, h));
    app
}

pub fn key_event(code: KeyCode, modifiers: KeyModifiers, kind: KeyEventKind) -> KeyEvent {
    KeyEvent {
        code,
        modifiers,
        kind,
        state: KeyEventState::NONE,
    }
}

/// Mimics the run loop: KeyEvent -> map_key -> update.
pub fn send(app: &mut App, event: KeyEvent) -> Vec<Effect> {
    match map_key(app, event) {
        Some(action) => app.update(action),
        None => Vec::new(),
    }
}

pub fn press(app: &mut App, code: KeyCode) -> Vec<Effect> {
    send(
        app,
        key_event(code, KeyModifiers::NONE, KeyEventKind::Press),
    )
}

pub fn ctrl(app: &mut App, c: char) -> Vec<Effect> {
    send(
        app,
        key_event(KeyCode::Char(c), KeyModifiers::CONTROL, KeyEventKind::Press),
    )
}

/// Types each character; uppercase letters carry SHIFT as a real terminal reports them.
pub fn type_str(app: &mut App, text: &str) -> Vec<Effect> {
    let mut effects = Vec::new();
    for c in text.chars() {
        let modifiers = if c.is_uppercase() {
            KeyModifiers::SHIFT
        } else {
            KeyModifiers::NONE
        };
        effects.extend(send(
            app,
            key_event(KeyCode::Char(c), modifiers, KeyEventKind::Press),
        ));
    }
    effects
}

pub fn quit_requested(effects: &[Effect]) -> bool {
    effects.contains(&Effect::Quit)
}

pub fn render(app: &App) -> Terminal<TestBackend> {
    let (w, h) = app.size();
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal.draw(|frame| ui::draw(app, frame)).unwrap();
    terminal
}

pub fn row_text(buffer: &Buffer, y: u16) -> String {
    (0..buffer.area.width)
        .map(|x| buffer[(x, y)].symbol())
        .collect()
}

/// The cells in `x0..x1` by `y0..y1` that hold anything but ASCII, as `x,y:symbol`.
pub fn non_ascii_cells(
    terminal: &Terminal<TestBackend>,
    (x0, x1): (u16, u16),
    (y0, y1): (u16, u16),
) -> Vec<String> {
    let buffer = terminal.backend().buffer();
    let mut found = Vec::new();
    for y in y0..y1 {
        for x in x0..x1 {
            let symbol = buffer[(x, y)].symbol();
            if !symbol.is_ascii() {
                found.push(format!("{x},{y}:{symbol}"));
            }
        }
    }
    found
}

/// `app` with the ASCII mode on.
pub fn with_ascii(mut app: App) -> App {
    app.set_prefs(crate::app::Prefs {
        ascii: true,
        ..crate::app::Prefs::default()
    });
    app
}
