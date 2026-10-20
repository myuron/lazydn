//! How a starred and a saved card look, and where the new keys show in the help.

use crossterm::event::KeyCode;

use crate::action::Action;
use crate::app::{App, Prefs};
use crate::feed_tests::{FOLLOWING, deliver, page, started};
use crate::test_support::*;
use crate::ui::theme::PRIMARY;

fn log_app() -> App {
    let (mut app, effects) = started(120, 40);
    deliver(&mut app, FOLLOWING, &effects, page(0, 3, None));
    app
}

/// The first card, starred with `s` and saved with `b`, before the server has answered.
fn starred_and_saved(ascii: bool) -> App {
    let mut app = log_app();
    app.set_prefs(Prefs {
        ascii,
        ..Prefs::default()
    });
    press(&mut app, KeyCode::Char('s'));
    press(&mut app, KeyCode::Char('b'));
    app
}

fn cell_fg(app: &App, symbol: &str) -> Option<ratatui::style::Color> {
    let terminal = render(app);
    let buffer = terminal.backend().buffer().clone();
    let (w, h) = (buffer.area.width, buffer.area.height);
    (0..h)
        .flat_map(|y| (0..w).map(move |x| (x, y)))
        .find(|&(x, y)| buffer[(x, y)].symbol() == symbol)
        .map(|(x, y)| buffer[(x, y)].fg)
}

#[test]
fn the_star_icon_is_filled_and_in_the_primary_color_once_the_card_is_starred() {
    let mut app = log_app();
    assert_eq!(cell_fg(&app, "★"), None, "no card is starred yet");

    press(&mut app, KeyCode::Char('s'));

    assert_eq!(cell_fg(&app, "★"), Some(PRIMARY));
}

#[test]
fn the_save_icon_is_the_checked_box_once_the_card_is_saved() {
    let mut app = log_app();
    assert_eq!(cell_fg(&app, "⊠"), None, "no card is saved yet");

    press(&mut app, KeyCode::Char('b'));

    assert!(cell_fg(&app, "⊠").is_some());
}

#[test]
fn the_help_lists_the_key_that_opens_the_card_in_the_browser() {
    let mut app = log_app();
    app.update(Action::OpenHelp);
    let terminal = render(&app);
    let buffer = terminal.backend().buffer().clone();

    let rows: Vec<String> = (0..buffer.area.height)
        .map(|y| row_text(&buffer, y))
        .collect();

    let row = rows.iter().find(|r| r.contains("open in browser"));
    assert!(
        row.is_some_and(|r| r.split_whitespace().any(|w| w == "O")),
        "{rows:#?}"
    );
}

// --- snapshots --------------------------------------------------------------

macro_rules! snapshot {
    ($name:literal, $app:expr) => {{
        let terminal = render(&$app);
        insta::assert_snapshot!($name, terminal.backend());
    }};
}

#[test]
fn st1_card_starred_saved() {
    snapshot!("card_starred_saved", starred_and_saved(false));
}

#[test]
fn st2_card_starred_saved_ascii() {
    snapshot!("card_starred_saved_ascii", starred_and_saved(true));
}

#[test]
fn the_list_of_the_ascii_screen_holds_only_ascii() {
    let terminal = render(&starred_and_saved(true));
    assert_eq!(
        non_ascii_cells(&terminal, (21, 87), (3, 38)),
        Vec::<String>::new()
    );
}
