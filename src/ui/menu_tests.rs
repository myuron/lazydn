//! How the more menu looks.
//!
//! - The menu is a double-framed box 26 columns wide, titled with the short hash, one row per item: a mark on the
//!   chosen row (`▌` in the second inner column), the key, then the label. It is as high as its items and sits in
//!   the middle of the screen over the page.
//! - The chosen row, its 24 inner columns, has the background of the selected card.
//! - The commit of someone else lists Star, Give a pastel, Save, Copy link, Open in browser and Report. Your own
//!   commit lists Save, Copy link, Open in browser and Revert commit.

use serde_json::json;
use time::UtcOffset;

use crate::action::Action;
use crate::app::{App, Session};
use crate::feed_tests::{FOLLOWING, deliver};
use crate::test_support::*;
use crate::write_tests::{card, card_with, feed_page, key};

macro_rules! snapshot {
    ($name:literal, $app:expr) => {{
        let terminal = render(&$app);
        insta::assert_snapshot!($name, terminal.backend());
    }};
}

fn log_sized(w: u16, h: u16, first: crate::model::Commit) -> App {
    let mut app = App::new(Session::SignedIn(fixture_me()), NOW, UtcOffset::UTC);
    let mut effects = app.update(Action::Init);
    effects.extend(app.update(Action::Resize(w, h)));
    let items = vec![first, card("c1", "bob"), card("c2", "carol")];
    deliver(&mut app, FOLLOWING, &effects, feed_page(items));
    app
}

fn theirs_first(w: u16, h: u16) -> App {
    log_sized(
        w,
        h,
        card_with("c0", "alice", json!({"short_hash": "a1b2c3d"})),
    )
}

fn own_first(w: u16, h: u16) -> App {
    log_sized(
        w,
        h,
        card_with("c0", "myuron", json!({"short_hash": "a1b2c3d"})),
    )
}

#[test]
fn mn1_more_menu() {
    let mut app = theirs_first(120, 40);
    key(&mut app, '.');

    snapshot!("more_menu", app);
}

#[test]
fn mn2_more_menu_own() {
    let mut app = own_first(60, 24);
    key(&mut app, '.');

    snapshot!("more_menu_own", app);
}

#[test]
fn mn1_the_chosen_row_has_the_background_of_the_selected_card() {
    let mut app = theirs_first(120, 40);
    key(&mut app, '.');

    let terminal = render(&app);
    let buffer = terminal.backend().buffer();
    for x in 48..72 {
        assert_eq!(
            buffer[(x, 17)].bg,
            crate::ui::theme::cursor_bg(true),
            "({x}, 17)"
        );
    }
    assert_ne!(buffer[(48, 18)].bg, crate::ui::theme::cursor_bg(true));
}
