//! How the composer looks, with the layout it is held to:
//!
//! - Every text row starts with four columns, ` ┆  `, and the first row of the text with ` ◌  `. A row of the
//!   field that holds no text is ` ┆`. The text wraps at `field width - 5` columns, by grapheme and not by word.
//! - Under the text sits one row for the error (left) and the counter (right), then one row for `stash@{0} draft
//!   saved` (left, while a draft exists) and the keys (right). Both rows end their right-hand text two columns
//!   short of the field's edge.
//!   Inline, both rows start with the rail ` ┆` and their left-hand text starts at column 4, as the text does.
//! - When the width is short, the keys keep their place and `stash@{0} draft saved` shrinks to `stash@{0}`, and goes
//!   altogether when even that does not fit; the counter wins over the error.
//! - Inline, the field starts with one ` ┆` row and shows 3 to 10 text rows. Over a dialog it starts with one blank
//!   row, shows 3 text rows or as many as the screen allows (`height - 9`), and the dialog is `min(width - 4, 64)`
//!   wide and centred in the whole screen.
//! - The field scrolls just enough to keep the cursor row in view.

use crossterm::event::KeyCode;
use ratatui::layout::Rect;
use ratatui::style::Color;

use crate::action::{Action, Failure};
use crate::app::{App, Screen};
use crate::compose::Surface;
use crate::feed_tests::{FOLLOWING, deliver, page, started};
use crate::keyboard::KeyboardMode;
use crate::layout::LayoutMode;
use crate::test_support::*;
use crate::ui::compose::compose_text_width;
use crate::ui::feed::rows;
use crate::ui::layout::split;

macro_rules! snapshot {
    ($name:literal, $app:expr) => {{
        let terminal = render(&$app);
        insta::assert_snapshot!($name, terminal.backend());
    }};
}

// --- scenes ---------------------------------------------------------------------

/// Signed in on Log with three commits loaded; the composer is closed.
fn log_scene(w: u16, h: u16) -> App {
    let (mut app, effects) = started(w, h);
    deliver(&mut app, FOLLOWING, &effects, page(0, 3, None));
    app
}

/// Signed in on Saved, which has nothing to show yet; `c` opens the composer as a dialog there.
fn saved_scene(w: u16, h: u16) -> App {
    let mut app = signed_in(w, h);
    app.update(Action::Goto(Screen::Saved));
    app
}

fn thread_scene(w: u16, h: u16) -> App {
    crate::thread_tests::opened(
        w,
        h,
        vec![],
        2,
        vec![
            crate::thread_tests::item("r1", "bob"),
            crate::thread_tests::item("r2", "carol"),
        ],
        None,
    )
}

fn open(app: &mut App) {
    type_str(app, "c");
    assert!(app.composer_focused(), "c did not open the composer");
}

fn write(app: &mut App, text: &str) {
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            press(app, KeyCode::Enter);
        }
        type_str(app, line);
    }
}

fn digits(n: usize) -> String {
    (0..n).map(|i| char::from(b'0' + (i % 10) as u8)).collect()
}

fn lines_1_to(n: usize) -> String {
    (1..=n)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn long_line() -> &'static str {
    "a quick note that is long enough to wrap in a narrow dialog"
}

fn unfocus_toast(app: &mut App) {
    app.update(Action::Tick(NOW + time::Duration::seconds(10)));
}

// --- geometry --------------------------------------------------------------------

fn main_area(w: u16, h: u16) -> Rect {
    split(Rect::new(0, 0, w, h), LayoutMode::from_size(w, h)).main
}

/// Where the first row of the Log list is on screen.
fn list_origin(w: u16, h: u16) -> (u16, u16) {
    let main = main_area(w, h);
    (main.x + 1, main.y + 1 + 2)
}

fn list_width(w: u16, h: u16) -> usize {
    usize::from(main_area(w, h).width) - 2
}

fn cursor_of(app: &App) -> (u16, u16) {
    let mut terminal = render(app);
    let position = terminal.get_cursor_position().unwrap();
    (position.x, position.y)
}

fn screen_rows(app: &App) -> Vec<String> {
    let terminal = render(app);
    let buffer = terminal.backend().buffer().clone();
    (0..buffer.area.height)
        .map(|y| row_text(&buffer, y))
        .collect()
}

fn screen_text(app: &App) -> String {
    screen_rows(app).join("\n")
}

/// The text and colour of the counter, which sits in the row above the one with the keys; `x` and `width` bound the field.
fn counter_in(app: &App, x: u16, width: u16) -> Option<(String, Color)> {
    let terminal = render(app);
    let buffer = terminal.backend().buffer().clone();
    let keys = (0..buffer.area.height).find(|y| row_text(&buffer, *y).contains("Esc cancel"))?;
    let row = keys.checked_sub(1)?;
    let cells: Vec<(String, Color)> = (x..x + width)
        .map(|cx| (buffer[(cx, row)].symbol().to_string(), buffer[(cx, row)].fg))
        .collect();
    let text: String = cells
        .iter()
        .map(|(s, _)| s.as_str())
        .collect::<String>()
        .trim()
        // The inline rows start with the `┆` rail, which is not part of the counter.
        .trim_start_matches('┆')
        .trim()
        .to_string();
    let first = cells.iter().find(|(s, _)| {
        s.trim()
            .chars()
            .next()
            .is_some_and(|c| c != '║' && c != '┆')
    })?;
    (!text.is_empty()).then_some((text, first.1))
}

/// The counter of a dialog on a 120-column screen: 62 columns inside the 64-wide frame.
fn dialog_counter(app: &App) -> Option<(String, Color)> {
    counter_in(app, 29, 62)
}

// --- the width the text wraps at ------------------------------------------------------

#[test]
fn inline_text_wraps_five_columns_short_of_the_list() {
    for (w, h) in [(120, 40), (80, 24), (60, 24)] {
        let app = signed_in(w, h);
        assert_eq!(
            compose_text_width(&app, Surface::Inline),
            list_width(w, h) - 5,
            "{w}x{h}"
        );
    }
    assert_eq!(compose_text_width(&signed_in(120, 40), Surface::Inline), 61);
}

#[test]
fn dialog_text_wraps_seven_columns_short_of_the_dialog() {
    assert_eq!(compose_text_width(&signed_in(120, 40), Surface::Modal), 57);
    assert_eq!(compose_text_width(&signed_in(68, 24), Surface::Modal), 57);
    assert_eq!(compose_text_width(&signed_in(60, 24), Surface::Modal), 49);
    assert_eq!(compose_text_width(&signed_in(40, 24), Surface::Modal), 29);
}

#[test]
fn up_and_down_move_by_the_same_wrapped_rows_that_are_drawn() {
    let text = "a".repeat(150);

    let mut inline = signed_in(120, 40);
    open(&mut inline);
    inline.update(Action::Paste(text.clone()));
    press(&mut inline, KeyCode::Up);
    type_str(&mut inline, "X");
    assert_eq!(
        inline.drafts().text(&crate::drafts::DraftKey::New),
        Some(format!("{}X{}", "a".repeat(89), "a".repeat(61)))
    );

    let mut dialog = saved_scene(120, 40);
    open(&mut dialog);
    dialog.update(Action::Paste(text));
    press(&mut dialog, KeyCode::Up);
    type_str(&mut dialog, "X");
    assert_eq!(
        dialog.drafts().text(&crate::drafts::DraftKey::New),
        Some(format!("{}X{}", "a".repeat(93), "a".repeat(57)))
    );
}

// --- the terminal cursor -------------------------------------------------------------

#[test]
fn the_cursor_sits_after_the_prefix_of_an_empty_inline_composer() {
    let mut app = signed_in(120, 40);
    open(&mut app);

    let (x, y) = list_origin(120, 40);
    assert_eq!(cursor_of(&app), (x + 4, y + 1));
}

#[test]
fn the_cursor_follows_what_is_typed() {
    let mut app = signed_in(120, 40);
    open(&mut app);
    let (x, y) = list_origin(120, 40);

    write(&mut app, "ab");
    assert_eq!(cursor_of(&app), (x + 4 + 2, y + 1));

    write(&mut app, "\ncde");
    assert_eq!(cursor_of(&app), (x + 4 + 3, y + 2));

    press(&mut app, KeyCode::Left);
    assert_eq!(cursor_of(&app), (x + 4 + 2, y + 2));
}

#[test]
fn a_wide_character_that_does_not_fit_moves_the_cursor_to_the_next_row() {
    let mut app = signed_in(120, 40);
    open(&mut app);
    let width = compose_text_width(&app, Surface::Inline);
    let (x, y) = list_origin(120, 40);

    write(&mut app, &format!("{}あ", "a".repeat(width - 1)));

    assert_eq!(cursor_of(&app), (x + 4 + 2, y + 2));
}

#[test]
fn a_line_that_ends_exactly_at_the_edge_puts_the_cursor_at_the_start_of_the_next_row() {
    let mut app = signed_in(120, 40);
    open(&mut app);
    let width = compose_text_width(&app, Surface::Inline);
    let (x, y) = list_origin(120, 40);

    write(&mut app, &"a".repeat(width));

    assert_eq!(cursor_of(&app), (x + 4, y + 2));
}

#[test]
fn a_line_that_ends_exactly_at_the_edge_with_a_wide_character_does_the_same() {
    let mut app = signed_in(120, 40);
    open(&mut app);
    let width = compose_text_width(&app, Surface::Inline);
    let (x, y) = list_origin(120, 40);

    write(&mut app, &format!("{}あ", "a".repeat(width - 2)));

    assert_eq!(cursor_of(&app), (x + 4, y + 2));
}

#[test]
fn the_cursor_stays_in_the_window_when_the_text_is_taller_than_ten_rows() {
    let mut app = signed_in(120, 40);
    open(&mut app);
    let (x, y) = list_origin(120, 40);

    write(&mut app, &lines_1_to(15));

    assert_eq!(cursor_of(&app), (x + 4 + 7, y + 1 + 9));
}

#[test]
fn the_cursor_of_the_dialog_is_inside_the_dialog() {
    let mut app = saved_scene(120, 40);
    open(&mut app);
    write(&mut app, "ab\nc");

    // The dialog is 64 wide and 8 tall, centred in 120x40.
    assert_eq!(cursor_of(&app), (28 + 1 + 4 + 1, 16 + 1 + 1 + 1));
}

#[test]
fn the_cursor_of_a_scrolled_dialog_is_on_its_last_row() {
    let mut app = saved_scene(120, 30);
    open(&mut app);

    app.update(Action::Paste(digits(3982)));

    // 70 rows of 57 columns; 21 of them fit, so the dialog is 26 tall and the cursor is at the bottom of the field.
    assert_eq!(cursor_of(&app), (28 + 1 + 4 + 49, 2 + 1 + 21));
}

#[test]
fn rows_tell_where_the_cursor_goes_while_the_composer_has_the_focus() {
    let mut app = log_scene(120, 40);
    let width = list_width(120, 40) as u16;
    let before = rows(app.feed(FOLLOWING), FOLLOWING, &app, width);
    assert_eq!(before.cursor, None);

    open(&mut app);
    let empty = rows(app.feed(FOLLOWING), FOLLOWING, &app, width);
    write(&mut app, "ab\ncde");
    let typed = rows(app.feed(FOLLOWING), FOLLOWING, &app, width);
    press(&mut app, KeyCode::Esc);
    let after = rows(app.feed(FOLLOWING), FOLLOWING, &app, width);

    assert_eq!(empty.cursor, Some((1, 4)));
    assert_eq!(typed.cursor, Some((2, 4 + 3)));
    assert_eq!(after.cursor, None);
}

// --- what the field says --------------------------------------------------------------------

#[test]
fn the_placeholder_and_the_hint_give_way_to_the_field() {
    let mut app = log_scene(120, 40);
    assert!(screen_text(&app).contains("What did you ship?"));
    assert!(screen_text(&app).contains("[c] Write a commit"));

    open(&mut app);

    let text = screen_text(&app);
    assert!(!text.contains("What did you ship?"), "{text}");
    assert!(!text.contains("[c] Write a commit"), "{text}");
    assert!(text.contains("Esc cancel"), "{text}");
}

#[test]
fn the_hint_names_ctrl_s_unless_the_terminal_tells_ctrl_enter_apart() {
    let mut app = signed_in(120, 40);
    open(&mut app);
    write(&mut app, "x");
    let plain = screen_text(&app);
    assert!(plain.contains("Esc cancel  Ctrl+S Commit"), "{plain}");
    assert!(!plain.contains("Ctrl+Enter"), "{plain}");

    app.set_keyboard(KeyboardMode { enhanced: true });
    let enhanced = screen_text(&app);
    assert!(
        enhanced.contains("Esc cancel  Ctrl+Enter Commit"),
        "{enhanced}"
    );
    assert!(!enhanced.contains("Ctrl+S"), "{enhanced}");
}

#[test]
fn the_dialog_shows_the_same_hint() {
    let mut app = saved_scene(120, 40);
    open(&mut app);
    assert!(screen_text(&app).contains("Esc cancel  Ctrl+S Commit"));

    app.set_keyboard(KeyboardMode { enhanced: true });
    assert!(screen_text(&app).contains("Esc cancel  Ctrl+Enter Commit"));
}

#[test]
fn while_the_commit_is_out_the_hint_says_committing() {
    let mut app = signed_in(120, 40);
    open(&mut app);
    write(&mut app, "x");
    submit_key(&mut app);

    let text = screen_text(&app);

    assert!(text.contains("Esc cancel  Committing…"), "{text}");
    assert!(!text.contains("Ctrl+S Commit"), "{text}");
}

fn submit_key(app: &mut App) {
    ctrl(app, 's');
    assert!(
        app.compose().is_some_and(|c| c.submitting),
        "nothing was sent"
    );
}

#[test]
fn the_draft_label_shows_once_there_is_text() {
    let mut app = signed_in(120, 40);
    open(&mut app);
    assert!(!screen_text(&app).contains("stash@{0} draft saved"));

    write(&mut app, "x");

    assert!(screen_text(&app).contains("stash@{0} draft saved"));
}

#[test]
fn the_draft_label_goes_when_the_text_is_deleted() {
    let mut app = signed_in(120, 40);
    open(&mut app);
    write(&mut app, "x");
    press(&mut app, KeyCode::Backspace);

    assert!(!screen_text(&app).contains("stash@{0} draft saved"));
}

#[test]
fn a_refusal_is_written_under_the_text() {
    let mut app = signed_in(120, 40);
    open(&mut app);
    write(&mut app, "x");
    submit_key(&mut app);
    let req = app
        .in_flight()
        .map(|f| f.req)
        .unwrap_or(crate::feed::ReqId(u64::MAX));

    app.update(Action::CommitFailed {
        req,
        failure: Failure {
            message: Some("The body field is required.".into()),
            ..Failure::new("validation_failed")
        },
    });
    unfocus_toast(&mut app);

    let text = screen_text(&app);
    assert!(text.contains("✗ The body field is required."), "{text}");
}

// --- the counter ---------------------------------------------------------------------------------

fn dialog_with(len: usize) -> App {
    let mut app = saved_scene(120, 30);
    open(&mut app);
    app.update(Action::Paste("a".repeat(len)));
    app
}

fn shown(app: &App) -> Option<String> {
    dialog_counter(app).map(|(text, _)| text)
}

fn color(app: &App) -> Option<Color> {
    dialog_counter(app).map(|(_, color)| color)
}

#[test]
fn no_counter_while_more_than_200_are_left() {
    assert_eq!(shown(&dialog_with(0)), None);
    assert_eq!(shown(&dialog_with(3799)), None);
}

#[test]
fn the_counter_shows_the_characters_left_from_200() {
    let app = dialog_with(3800);

    assert_eq!(shown(&app), Some("200".to_string()));
    assert_ne!(color(&app), Some(Color::Yellow));
    assert_ne!(color(&app), Some(Color::Red));
    assert_eq!(shown(&dialog_with(3979)), Some("21".to_string()));
    assert_ne!(color(&dialog_with(3979)), Some(Color::Yellow));
}

#[test]
fn the_counter_is_yellow_from_20_left() {
    assert_eq!(shown(&dialog_with(3980)), Some("20".to_string()));
    assert_eq!(color(&dialog_with(3980)), Some(Color::Yellow));
    assert_eq!(shown(&dialog_with(3999)), Some("1".to_string()));
    assert_eq!(color(&dialog_with(3999)), Some(Color::Yellow));
}

#[test]
fn the_counter_is_red_at_zero_and_when_negative() {
    assert_eq!(shown(&dialog_with(4000)), Some("0".to_string()));
    assert_eq!(color(&dialog_with(4000)), Some(Color::Red));
    assert_eq!(shown(&dialog_with(4005)), Some("-5".to_string()));
    assert_eq!(color(&dialog_with(4005)), Some(Color::Red));
}

#[test]
fn the_counter_counts_the_text_the_way_the_server_will() {
    let mut app = saved_scene(120, 30);
    open(&mut app);
    app.update(Action::Paste(format!(
        "{}{}b",
        "a".repeat(3799),
        "\n".repeat(8)
    )));

    // 3799, the two line breaks that the blank lines collapse to, and the last character.
    assert_eq!(shown(&app), Some("198".to_string()));
}

#[test]
fn the_inline_composer_has_a_counter_too() {
    let mut app = signed_in(120, 40);
    open(&mut app);
    app.update(Action::Paste("a".repeat(3990)));

    let (x, _) = list_origin(120, 40);
    assert_eq!(
        counter_in(&app, x, list_width(120, 40) as u16),
        Some(("10".to_string(), Color::Yellow))
    );
}

// --- fitting the screen ------------------------------------------------------------------------------

#[test]
fn the_dialog_grows_with_the_text_up_to_four_rows_short_of_the_screen() {
    let mut small = saved_scene(120, 24);
    open(&mut small);
    let rows_small = screen_rows(&small);
    let top_small = rows_small.iter().position(|r| r.contains("Write a commit"));
    assert_eq!(top_small, Some(8));

    let mut tall = saved_scene(120, 24);
    open(&mut tall);
    write(&mut tall, &lines_1_to(30));
    let rows_tall = screen_rows(&tall);
    let top_tall = rows_tall.iter().position(|r| r.contains("Write a commit"));
    assert_eq!(top_tall, Some(2));
    let bottom = rows_tall.iter().rposition(|r| r.contains('╚'));
    assert_eq!(bottom, Some(21));
}

#[test]
fn the_whole_inline_composer_stays_on_screen_at_the_smallest_height() {
    let (mut app, effects) = started(120, 20);
    deliver(&mut app, FOLLOWING, &effects, page(0, 30, None));
    type_str(&mut app, "G");
    open(&mut app);

    write(&mut app, &lines_1_to(15));

    let text = screen_text(&app);
    assert!(text.contains("line 15"), "{text}");
    assert!(text.contains("stash@{0} draft saved"), "{text}");
    assert!(text.contains("Esc cancel"), "{text}");
    let (_, y) = cursor_of(&app);
    let (_, top) = list_origin(120, 20);
    assert!(y >= top && y < top + 15, "cursor row {y}");
}

#[test]
fn the_first_card_follows_the_inline_composer() {
    let mut app = log_scene(120, 40);
    let before = screen_rows(&app);
    let first_card = before.iter().position(|r| r.contains("commit 0")).unwrap();

    open(&mut app);
    write(&mut app, "ab");
    let after = screen_rows(&app);

    // Three text rows instead of the placeholder row and the hint row, and two more rows under them.
    let moved = after.iter().position(|r| r.contains("commit 0")).unwrap();
    assert_eq!(moved, first_card + 2);
}

// --- looks ---------------------------------------------------------------------------------------------------

#[test]
fn snapshot_cp1_inline_editing() {
    let mut app = log_scene(120, 40);
    open(&mut app);
    write(&mut app, "hello /dev/null\nsecond line");
    snapshot!("composer_inline_editing", app);
}

#[test]
fn snapshot_cp2_modal() {
    let mut app = thread_scene(120, 40);
    open(&mut app);
    write(
        &mut app,
        "hello /dev/null, this was written in lazydn.\n`cargo install lazydn`\n",
    );
    snapshot!("composer_modal", app);
}

#[test]
fn snapshot_cp3_counter_warning() {
    let mut app = saved_scene(120, 30);
    open(&mut app);
    app.update(Action::Paste(digits(3982)));
    snapshot!("composer_counter_warning", app);
}

#[test]
fn snapshot_cp4_submitting() {
    let mut app = log_scene(120, 30);
    open(&mut app);
    write(&mut app, "ship it");
    submit_key(&mut app);
    snapshot!("composer_submitting", app);
}

#[test]
fn snapshot_cp5_failed() {
    let mut app = log_scene(120, 30);
    open(&mut app);
    write(&mut app, "x");
    submit_key(&mut app);
    let req = app
        .in_flight()
        .map(|f| f.req)
        .unwrap_or(crate::feed::ReqId(u64::MAX));
    app.update(Action::CommitFailed {
        req,
        failure: Failure {
            message: Some("The body field is required.".into()),
            ..Failure::new("validation_failed")
        },
    });
    unfocus_toast(&mut app);
    snapshot!("composer_failed", app);
}

#[test]
fn snapshot_cp6_modal_narrow() {
    let mut app = saved_scene(60, 24);
    open(&mut app);
    write(&mut app, long_line());
    snapshot!("composer_modal_narrow", app);
}

#[test]
fn snapshot_counter_negative() {
    let mut app = saved_scene(120, 30);
    open(&mut app);
    app.update(Action::Paste(digits(4005)));
    snapshot!("composer_counter_negative", app);
}

#[test]
fn snapshot_scrolled_inline() {
    let mut app = log_scene(120, 30);
    open(&mut app);
    write(&mut app, &lines_1_to(15));
    snapshot!("composer_inline_scrolled", app);
}

#[test]
fn snapshot_unfocused_inline_with_a_draft() {
    let mut app = log_scene(120, 30);
    open(&mut app);
    write(&mut app, "hello /dev/null\nsecond line");
    press(&mut app, KeyCode::Esc);
    snapshot!("composer_inline_unfocused_draft", app);
}

// --- narrow dialogs ------------------------------------------------------------------------------------

#[test]
fn in_a_narrow_dialog_the_keys_keep_their_place_and_the_draft_label_goes() {
    let mut app = saved_scene(40, 20);
    open(&mut app);
    write(&mut app, "hi");

    let text = screen_text(&app);

    assert!(text.contains("Esc cancel  Ctrl+S Commit"), "{text}");
    assert!(!text.contains("stash@{0}"), "{text}");
}

#[test]
fn a_little_more_room_shows_the_label_in_short_form() {
    let mut app = saved_scene(44, 20);
    open(&mut app);
    write(&mut app, "hi");

    let text = screen_text(&app);

    assert!(text.contains("Esc cancel  Ctrl+S Commit"), "{text}");
    assert!(text.contains("stash@{0} "), "{text}");
    assert!(!text.contains("draft saved"), "{text}");
}

#[test]
fn the_cursor_of_a_dialog_on_a_narrow_low_screen_is_inside_it() {
    let mut app = saved_scene(40, 20);
    open(&mut app);
    write(&mut app, "ab");

    // The dialog is 36 wide and 8 tall, centred in 40x20.
    assert_eq!(cursor_of(&app), (2 + 1 + 4 + 2, 6 + 1 + 1));
}

#[test]
fn snapshot_narrow_low_dialog() {
    let mut app = saved_scene(40, 20);
    open(&mut app);
    write(&mut app, "hello narrow");
    snapshot!("composer_modal_40x20", app);
}

// --- low screens and crowded rows ----------------------------------------------------------------------

#[test]
fn with_the_nav_on_top_the_inline_field_shrinks_to_keep_the_keys_on_screen() {
    let (mut app, effects) = started(60, 20);
    deliver(&mut app, FOLLOWING, &effects, page(0, 30, None));
    open(&mut app);

    write(&mut app, &lines_1_to(15));

    let rows = screen_rows(&app);
    let text = rows.join("\n");
    assert!(text.contains("Esc cancel"), "{text}");
    let shown = rows.iter().filter(|r| r.contains("line ")).count();
    assert!((1..=9).contains(&shown), "{shown} text rows\n{text}");
    let (_, y) = cursor_of(&app);
    let (_, top) = list_origin(60, 20);
    assert!(y >= top && y < top + 12, "cursor row {y}");
}

#[test]
fn a_crowded_row_keeps_the_counter_and_cuts_the_error() {
    let mut app = saved_scene(40, 20);
    open(&mut app);
    app.update(Action::Paste("a".repeat(3985)));
    submit_key(&mut app);
    let req = app
        .in_flight()
        .map(|f| f.req)
        .unwrap_or(crate::feed::ReqId(u64::MAX));
    app.update(Action::CommitFailed {
        req,
        failure: Failure {
            message: Some("x".repeat(64)),
            ..Failure::new("validation_failed")
        },
    });
    unfocus_toast(&mut app);

    let terminal = render(&app);
    let buffer = terminal.backend().buffer().clone();
    let keys = (0..buffer.area.height)
        .find(|y| row_text(&buffer, *y).contains("Esc cancel"))
        .expect("the keys are shown");
    let y = keys - 1;
    let row = row_text(&buffer, y);
    // The dialog is 36 wide at x = 2.
    assert_eq!(buffer[(2, y)].symbol(), "║", "{row}");
    assert_eq!(buffer[(37, y)].symbol(), "║", "{row}");
    let at = row.find("15").expect("the counter stays");
    let col = row[..at].chars().count() as u16;
    assert_eq!(buffer[(col, y)].fg, Color::Yellow);
    assert!(row.contains("✗ x"), "{row}");
    assert!(!row.contains(&"x".repeat(64)), "{row}");
}

#[test]
fn a_closed_composer_previews_the_first_line_with_text_of_the_draft() {
    let mut app = log_scene(120, 40);
    open(&mut app);
    write(&mut app, "\n  \nreal start\nlater");
    press(&mut app, KeyCode::Esc);

    let text = screen_text(&app);

    assert!(text.contains("◌  real start"), "{text}");
    assert!(!text.contains("later"), "{text}");
}
