//! How the reply box and the quote dialog look, with the layout they are held to:
//!
//! - The reply box is a double frame under the last row of its card. It starts at the column where the card's body
//!   text starts, ends at the card's right edge (the list is two columns wider), and repeats the rail of the card
//!   to its left; the selection mark column is blank. The title is `Reply to @handle`.
//! - Inside, one to eight text rows, each starting with `║ `, then a row with the keys at the right edge (and the
//!   counter before them), then the bottom frame. The text wraps four columns short of the frame's width.
//! - The quote dialog is the dialog of a new commit, titled `Quote @handle`, with the quoted card in its frame
//!   between the text rows and the row of the error and the counter.
//! - Both put the terminal cursor at the end of the text.

use crossterm::event::KeyCode;
use ratatui::style::Color;

use crate::feed_tests::{FOLLOWING, deliver, page, started};
use crate::test_support::*;
use crate::thread_tests::{item, opened};

macro_rules! snapshot {
    ($name:literal, $app:expr) => {{
        let terminal = render(&$app);
        insta::assert_snapshot!($name, terminal.backend());
    }};
}

fn write(app: &mut crate::app::App, text: &str) {
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            press(app, KeyCode::Enter);
        }
        type_str(app, line);
    }
}

/// The Following list with three commits and the second one selected.
fn log_scene(w: u16, h: u16) -> crate::app::App {
    let (mut app, effects) = started(w, h);
    deliver(&mut app, FOLLOWING, &effects, page(0, 3, None));
    press(&mut app, KeyCode::Char('j'));
    app
}

fn thread_scene() -> crate::app::App {
    let mut app = opened(
        120,
        40,
        vec![],
        2,
        vec![item("r1", "bob"), item("r2", "carol")],
        None,
    );
    press(&mut app, KeyCode::Char('j'));
    app
}

fn cursor_of(app: &crate::app::App) -> (u16, u16) {
    let mut terminal = render(app);
    let position = terminal.get_cursor_position().unwrap_or_default();
    (position.x, position.y)
}

// --- the reply box -----------------------------------------------------------------

#[test]
fn reply_inline_log() {
    let mut app = log_scene(120, 40);
    type_str(&mut app, "r");
    write(&mut app, "thanks, shipping it\ntoday");

    snapshot!("reply_inline_log", app);
    assert_eq!(cursor_of(&app), (31, 20));
}

#[test]
fn reply_inline_thread() {
    let mut app = thread_scene();
    type_str(&mut app, "r");
    write(&mut app, "ssh-key has SshSig");

    snapshot!("reply_inline_thread", app);
    assert_eq!(cursor_of(&app), (48, 12));
}

#[test]
fn reply_inline_narrow() {
    let mut app = log_scene(60, 30);
    type_str(&mut app, "r");
    write(
        &mut app,
        "thanks for sharing this, it helps a lot with my setup",
    );

    snapshot!("reply_inline_narrow", app);
    assert_eq!(cursor_of(&app), (9, 23));
}

#[test]
fn the_cursor_of_an_empty_box_is_at_the_start_of_its_first_text_row() {
    let mut app = log_scene(120, 40);

    type_str(&mut app, "r");

    // The box starts on the row after the actions of the selected card; its text starts after `║ `.
    assert_eq!(cursor_of(&app), (21 + 3 + 2, 19));
}

#[test]
fn the_counter_of_the_box_is_yellow_from_20_left_and_red_when_over() {
    let footer = |app: &crate::app::App, counter: &str| -> Option<Color> {
        let terminal = render(app);
        let buffer = terminal.backend().buffer().clone();
        let (_, h) = app.size();
        let y = (0..h).find(|y| row_text(&buffer, *y).contains("Esc cancel"))?;
        let row = row_text(&buffer, y);
        let x = row.find(&format!("{counter}  Esc cancel"))?;
        let x = row[..x].chars().count();
        Some(buffer[(u16::try_from(x).ok()?, y)].fg)
    };
    for (count, counter, color) in [(3980, "20", Color::Yellow), (4010, "-10", Color::Red)] {
        let mut app = log_scene(120, 40);
        type_str(&mut app, "r");
        app.update(crate::action::Action::Paste("a".repeat(count)));

        assert_eq!(footer(&app, counter), Some(color), "{counter}");
    }
}

// --- the quote dialog --------------------------------------------------------------

#[test]
fn quote_modal() {
    let mut app = log_scene(120, 40);
    type_str(&mut app, "q");
    write(&mut app, "great idea,\nquoting this");

    snapshot!("quote_modal", app);
    assert_eq!(cursor_of(&app), (45, 17));
}

#[test]
fn quote_modal_narrow() {
    let mut app = log_scene(60, 30);
    type_str(&mut app, "q");
    write(
        &mut app,
        "a quick note that is long enough to wrap in a narrow dialog",
    );

    snapshot!("quote_modal_narrow", app);
    assert_eq!(cursor_of(&app), (17, 12));
}
