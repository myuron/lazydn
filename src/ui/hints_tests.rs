//! The key hints of the status line and the Keys box of the Aside, per screen and per dialog.
//!
//! - Every screen and dialog has its own hints; Settings does not offer `j/k move  ↵ open`, because nothing there opens.
//! - Wide shows every hint, Medium leaves out `Tab pane` and then drops hints from the end (before `? help`)
//!   until the left part and the right end fit with two blank columns between, Narrow shows the keys only.
//! - Dialogs and text fields show their own keys and no `? help`, since `?` is a character there.
//! - ASCII mode writes `Enter` where the hints have `↵`.
//! - The Keys box of the Aside lays the pairs out in two columns, four rows at most.
//!
//! The expected strings are written out from docs/screen-design.md, not taken from the output of the code.

use crossterm::event::KeyCode;
use serde_json::json;

use crate::action::{Action, Failure};
use crate::app::{App, ConnTab, Prefs, Screen};
use crate::profile_tests::{deliver_user, open_profile, user_req, user_resp};
use crate::settings_tests::person;
use crate::test_support::*;
use crate::write_tests::{card, key, log_app_of};

/// (context, width, height, ascii, the left part of the status line without the right end)
const STATUS: &[(&str, u16, u16, bool, &str)] = &[
    (
        "log",
        120,
        40,
        false,
        " Log > Following  j/k move  ↵ open  c commit  Tab pane  ? help",
    ),
    (
        "log",
        80,
        24,
        false,
        " Log > Following  j/k move  ↵ open  c commit  ? help",
    ),
    ("log", 60, 24, false, " j/k ↵ c ?"),
    (
        "log",
        120,
        40,
        true,
        " Log > Following  j/k move  Enter open  c commit  Tab pane  ? help",
    ),
    (
        "log",
        80,
        24,
        true,
        " Log > Following  j/k move  Enter open  c commit  ? help",
    ),
    ("log", 60, 24, true, " j/k Enter c ?"),
    (
        "saved",
        120,
        40,
        false,
        " Saved  j/k move  ↵ open  b unsave  Tab pane  ? help",
    ),
    (
        "saved",
        80,
        24,
        false,
        " Saved  j/k move  ↵ open  b unsave  ? help",
    ),
    ("saved", 60, 24, false, " j/k ↵ b ?"),
    (
        "saved",
        120,
        40,
        true,
        " Saved  j/k move  Enter open  b unsave  Tab pane  ? help",
    ),
    (
        "saved",
        80,
        24,
        true,
        " Saved  j/k move  Enter open  b unsave  ? help",
    ),
    ("saved", 60, 24, true, " j/k Enter b ?"),
    (
        "thread",
        120,
        40,
        false,
        " Commit m1  j/k move  ↵ open  r reply  Esc back  Tab pane  ? help",
    ),
    (
        "thread",
        80,
        24,
        false,
        " Commit m1  j/k move  ↵ open  r reply  Esc back  ? help",
    ),
    ("thread", 60, 24, false, " j/k ↵ r Esc ?"),
    (
        "thread",
        120,
        40,
        true,
        " Commit m1  j/k move  Enter open  r reply  Esc back  Tab pane  ? help",
    ),
    (
        "thread",
        80,
        24,
        true,
        " Commit m1  j/k move  Enter open  r reply  Esc back  ? help",
    ),
    ("thread", 60, 24, true, " j/k Enter r Esc ?"),
    (
        "profile_other",
        120,
        40,
        false,
        " Profile  j/k move  ↵ open  1-4 tab  f follow  Esc back  Tab pane  ? help",
    ),
    (
        "profile_other",
        80,
        24,
        false,
        " Profile  j/k move  ↵ open  1-4 tab  f follow  Esc back  ? help",
    ),
    ("profile_other", 60, 24, false, " j/k ↵ 1-4 f Esc ?"),
    (
        "profile_other",
        120,
        40,
        true,
        " Profile  j/k move  Enter open  1-4 tab  f follow  Esc back  Tab pane  ? help",
    ),
    (
        "profile_other",
        80,
        24,
        true,
        " Profile  j/k move  Enter open  1-4 tab  f follow  Esc back  ? help",
    ),
    ("profile_other", 60, 24, true, " j/k Enter 1-4 f Esc ?"),
    (
        "profile_own",
        120,
        40,
        false,
        " Profile  j/k move  ↵ open  1-4 tab  Esc back  Tab pane  ? help",
    ),
    (
        "profile_own",
        80,
        24,
        false,
        " Profile  j/k move  ↵ open  1-4 tab  Esc back  ? help",
    ),
    ("profile_own", 60, 24, false, " j/k ↵ 1-4 Esc ?"),
    (
        "profile_own",
        120,
        40,
        true,
        " Profile  j/k move  Enter open  1-4 tab  Esc back  Tab pane  ? help",
    ),
    (
        "profile_own",
        80,
        24,
        true,
        " Profile  j/k move  Enter open  1-4 tab  Esc back  ? help",
    ),
    ("profile_own", 60, 24, true, " j/k Enter 1-4 Esc ?"),
    (
        "connections",
        120,
        40,
        false,
        " Profile > Followers  j/k move  ↵ open  1-2 tab  f follow  Esc back  Tab pane  ? help",
    ),
    (
        "connections",
        80,
        24,
        false,
        " Profile > Followers  j/k move  ↵ open  1-2 tab  f follow  ? help",
    ),
    ("connections", 60, 24, false, " j/k ↵ 1-2 f Esc ?"),
    (
        "connections",
        120,
        40,
        true,
        " Profile > Followers  j/k move  Enter open  1-2 tab  f follow  Esc back  Tab pane  ? help",
    ),
    (
        "connections",
        80,
        24,
        true,
        " Profile > Followers  j/k move  Enter open  1-2 tab  f follow  ? help",
    ),
    ("connections", 60, 24, true, " j/k Enter 1-2 f Esc ?"),
    (
        "notifications",
        120,
        40,
        false,
        " Notifications  j/k move  ↵ open  1-6 filter  a all  Tab pane  ? help",
    ),
    (
        "notifications",
        80,
        24,
        false,
        " Notifications  j/k move  ↵ open  1-6 filter  a all  ? help",
    ),
    ("notifications", 60, 24, false, " j/k ↵ 1-6 a ?"),
    (
        "notifications",
        120,
        40,
        true,
        " Notifications  j/k move  Enter open  1-6 filter  a all  Tab pane  ? help",
    ),
    (
        "notifications",
        80,
        24,
        true,
        " Notifications  j/k move  Enter open  1-6 filter  a all  ? help",
    ),
    ("notifications", 60, 24, true, " j/k Enter 1-6 a ?"),
    (
        "settings_profile",
        120,
        40,
        false,
        " Settings  j/k row  h/l choose  S sync  D delete  1-3 tab  Tab pane  ? help",
    ),
    (
        "settings_profile",
        80,
        24,
        false,
        " Settings  j/k row  h/l choose  S sync  D delete  1-3 tab  ? help",
    ),
    ("settings_profile", 60, 24, false, " j/k h/l S D 1-3 ?"),
    (
        "settings_profile",
        120,
        40,
        true,
        " Settings  j/k row  h/l choose  S sync  D delete  1-3 tab  Tab pane  ? help",
    ),
    (
        "settings_profile",
        80,
        24,
        true,
        " Settings  j/k row  h/l choose  S sync  D delete  1-3 tab  ? help",
    ),
    ("settings_profile", 60, 24, true, " j/k h/l S D 1-3 ?"),
    (
        "settings_appearance",
        120,
        40,
        false,
        " Settings  j/k row  h/l change  1-3 tab  Tab pane  ? help",
    ),
    (
        "settings_appearance",
        80,
        24,
        false,
        " Settings  j/k row  h/l change  1-3 tab  ? help",
    ),
    ("settings_appearance", 60, 24, false, " j/k h/l 1-3 ?"),
    (
        "settings_appearance",
        120,
        40,
        true,
        " Settings  j/k row  h/l change  1-3 tab  Tab pane  ? help",
    ),
    (
        "settings_appearance",
        80,
        24,
        true,
        " Settings  j/k row  h/l change  1-3 tab  ? help",
    ),
    ("settings_appearance", 60, 24, true, " j/k h/l 1-3 ?"),
    (
        "settings_safety",
        120,
        40,
        false,
        " Settings  j/k move  u release  R reload  1-3 tab  Tab pane  ? help",
    ),
    (
        "settings_safety",
        80,
        24,
        false,
        " Settings  j/k move  u release  R reload  1-3 tab  ? help",
    ),
    ("settings_safety", 60, 24, false, " j/k u R 1-3 ?"),
    (
        "settings_safety",
        120,
        40,
        true,
        " Settings  j/k move  u release  R reload  1-3 tab  Tab pane  ? help",
    ),
    (
        "settings_safety",
        80,
        24,
        true,
        " Settings  j/k move  u release  R reload  1-3 tab  ? help",
    ),
    ("settings_safety", 60, 24, true, " j/k u R 1-3 ?"),
    (
        "settings_guest",
        120,
        40,
        false,
        " Settings  L sign in  1-3 tab  Tab pane  ? help",
    ),
    (
        "settings_guest",
        80,
        24,
        false,
        " Settings  L sign in  1-3 tab  ? help",
    ),
    ("settings_guest", 60, 24, false, " L 1-3 ?"),
    (
        "settings_guest",
        120,
        40,
        true,
        " Settings  L sign in  1-3 tab  Tab pane  ? help",
    ),
    (
        "settings_guest",
        80,
        24,
        true,
        " Settings  L sign in  1-3 tab  ? help",
    ),
    ("settings_guest", 60, 24, true, " L 1-3 ?"),
    (
        "help",
        120,
        40,
        false,
        " Log > Following  Esc close  Q quit",
    ),
    ("help", 80, 24, false, " Log > Following  Esc close  Q quit"),
    ("help", 60, 24, false, " Esc Q"),
    ("help", 120, 40, true, " Log > Following  Esc close  Q quit"),
    ("help", 80, 24, true, " Log > Following  Esc close  Q quit"),
    ("help", 60, 24, true, " Esc Q"),
    (
        "signin",
        120,
        40,
        false,
        " Log > Latest  ↵ submit  Esc close  ^O browser  ^Y copy  ^U clear  ^R refresh",
    ),
    (
        "signin",
        80,
        24,
        false,
        " Log > Latest  ↵ submit  Esc close  ^O browser  ^Y copy  ^U clear",
    ),
    ("signin", 60, 24, false, " ↵ Esc ^O ^Y ^U ^R"),
    (
        "signin",
        120,
        40,
        true,
        " Log > Latest  Enter submit  Esc close  ^O browser  ^Y copy  ^U clear  ^R refresh",
    ),
    (
        "signin",
        80,
        24,
        true,
        " Log > Latest  Enter submit  Esc close  ^O browser  ^Y copy  ^U clear",
    ),
    ("signin", 60, 24, true, " Enter Esc ^O ^Y ^U ^R"),
    (
        "composer",
        120,
        40,
        false,
        " Log > Following  ^S send  ↵ newline  Esc close",
    ),
    (
        "composer",
        80,
        24,
        false,
        " Log > Following  ^S send  ↵ newline  Esc close",
    ),
    ("composer", 60, 24, false, " ^S ↵ Esc"),
    (
        "composer",
        120,
        40,
        true,
        " Log > Following  ^S send  Enter newline  Esc close",
    ),
    (
        "composer",
        80,
        24,
        true,
        " Log > Following  ^S send  Enter newline  Esc close",
    ),
    ("composer", 60, 24, true, " ^S Enter Esc"),
    (
        "confirm_delete",
        120,
        40,
        false,
        " Log > Following  y delete  n/Esc cancel",
    ),
    (
        "confirm_delete",
        80,
        24,
        false,
        " Log > Following  y delete  n/Esc cancel",
    ),
    ("confirm_delete", 60, 24, false, " y n/Esc"),
    (
        "confirm_delete",
        120,
        40,
        true,
        " Log > Following  y delete  n/Esc cancel",
    ),
    (
        "confirm_delete",
        80,
        24,
        true,
        " Log > Following  y delete  n/Esc cancel",
    ),
    ("confirm_delete", 60, 24, true, " y n/Esc"),
    (
        "confirm_block",
        120,
        40,
        false,
        " Profile  y block  n/Esc cancel",
    ),
    (
        "confirm_block",
        80,
        24,
        false,
        " Profile  y block  n/Esc cancel",
    ),
    ("confirm_block", 60, 24, false, " y n/Esc"),
    (
        "confirm_block",
        120,
        40,
        true,
        " Profile  y block  n/Esc cancel",
    ),
    (
        "confirm_block",
        80,
        24,
        true,
        " Profile  y block  n/Esc cancel",
    ),
    ("confirm_block", 60, 24, true, " y n/Esc"),
    (
        "confirm_account",
        120,
        40,
        false,
        " Settings  ↵ delete  Esc close",
    ),
    (
        "confirm_account",
        80,
        24,
        false,
        " Settings  ↵ delete  Esc close",
    ),
    ("confirm_account", 60, 24, false, " ↵ Esc"),
    (
        "confirm_account",
        120,
        40,
        true,
        " Settings  Enter delete  Esc close",
    ),
    (
        "confirm_account",
        80,
        24,
        true,
        " Settings  Enter delete  Esc close",
    ),
    ("confirm_account", 60, 24, true, " Enter Esc"),
    (
        "report",
        120,
        40,
        false,
        " Log > Following  j/k reason  Tab details  ↵ send  Esc close",
    ),
    (
        "report",
        80,
        24,
        false,
        " Log > Following  j/k reason  Tab details  ↵ send  Esc close",
    ),
    ("report", 60, 24, false, " j/k Tab ↵ Esc"),
    (
        "report",
        120,
        40,
        true,
        " Log > Following  j/k reason  Tab details  Enter send  Esc close",
    ),
    (
        "report",
        80,
        24,
        true,
        " Log > Following  j/k reason  Tab details  Enter send  Esc close",
    ),
    ("report", 60, 24, true, " j/k Tab Enter Esc"),
    (
        "menu",
        120,
        40,
        false,
        " Log > Following  j/k move  ↵ run  Esc close",
    ),
    (
        "menu",
        80,
        24,
        false,
        " Log > Following  j/k move  ↵ run  Esc close",
    ),
    ("menu", 60, 24, false, " j/k ↵ Esc"),
    (
        "menu",
        120,
        40,
        true,
        " Log > Following  j/k move  Enter run  Esc close",
    ),
    (
        "menu",
        80,
        24,
        true,
        " Log > Following  j/k move  Enter run  Esc close",
    ),
    ("menu", 60, 24, true, " j/k Enter Esc"),
    (
        "hex",
        120,
        40,
        false,
        " Settings  0-9a-f type  ↵ set  Esc cancel",
    ),
    (
        "hex",
        80,
        24,
        false,
        " Settings  0-9a-f type  ↵ set  Esc cancel",
    ),
    ("hex", 60, 24, false, " 0-9a-f ↵ Esc"),
    (
        "hex",
        120,
        40,
        true,
        " Settings  0-9a-f type  Enter set  Esc cancel",
    ),
    (
        "hex",
        80,
        24,
        true,
        " Settings  0-9a-f type  Enter set  Esc cancel",
    ),
    ("hex", 60, 24, true, " 0-9a-f Enter Esc"),
    (
        "profile_blocked",
        120,
        40,
        false,
        " Profile  B unblock  Esc back  Tab pane  ? help",
    ),
    (
        "profile_blocked",
        80,
        24,
        false,
        " Profile  B unblock  Esc back  ? help",
    ),
    ("profile_blocked", 60, 24, false, " B Esc ?"),
    (
        "profile_blocked",
        120,
        40,
        true,
        " Profile  B unblock  Esc back  Tab pane  ? help",
    ),
    (
        "profile_blocked",
        80,
        24,
        true,
        " Profile  B unblock  Esc back  ? help",
    ),
    ("profile_blocked", 60, 24, true, " B Esc ?"),
    (
        "settings_guest_appearance",
        120,
        40,
        false,
        " Settings  j/k row  h/l change  1-3 tab  Tab pane  ? help",
    ),
    (
        "settings_guest_appearance",
        80,
        24,
        false,
        " Settings  j/k row  h/l change  1-3 tab  ? help",
    ),
    ("settings_guest_appearance", 60, 24, false, " j/k h/l 1-3 ?"),
    (
        "settings_guest_appearance",
        120,
        40,
        true,
        " Settings  j/k row  h/l change  1-3 tab  Tab pane  ? help",
    ),
    (
        "settings_guest_appearance",
        80,
        24,
        true,
        " Settings  j/k row  h/l change  1-3 tab  ? help",
    ),
    ("settings_guest_appearance", 60, 24, true, " j/k h/l 1-3 ?"),
];

/// (context, ascii, the four content rows of the Keys box without trailing blanks)
const KEYS: &[(&str, bool, &[&str])] = &[
    (
        "log",
        false,
        &[
            " j/k  move     ↵  open",
            " c    commit   r  reply",
            " s    star     o  replies",
            " ?    all keys",
        ],
    ),
    (
        "log",
        true,
        &[
            " j/k  move     Enter  open",
            " c    commit   r      reply",
            " s    star     o      replies",
            " ?    all keys",
        ],
    ),
    (
        "saved",
        false,
        &[
            " j/k  move     ↵  open",
            " b    unsave   r  reply",
            " s    star     o  replies",
            " ?    all keys",
        ],
    ),
    (
        "saved",
        true,
        &[
            " j/k  move     Enter  open",
            " b    unsave   r      reply",
            " s    star     o      replies",
            " ?    all keys",
        ],
    ),
    (
        "thread",
        false,
        &[
            " j/k  move    ↵  open",
            " r    reply   q  quote",
            " s    star    o  replies",
            " Esc  back    ?  all keys",
        ],
    ),
    (
        "thread",
        true,
        &[
            " j/k  move    Enter  open",
            " r    reply   q      quote",
            " s    star    o      replies",
            " Esc  back    ?      all keys",
        ],
    ),
    (
        "profile_other",
        false,
        &[
            " j/k  move   ↵  open",
            " 1-4  tab    f  follow",
            " m    mute   B  block",
            " Esc  back   ?  all keys",
        ],
    ),
    (
        "profile_other",
        true,
        &[
            " j/k  move   Enter  open",
            " 1-4  tab    f      follow",
            " m    mute   B      block",
            " Esc  back   ?      all keys",
        ],
    ),
    (
        "profile_own",
        false,
        &[
            " j/k  move   ↵  open",
            " 1-4  tab    s  star",
            " Esc  back   ?  all keys",
        ],
    ),
    (
        "profile_own",
        true,
        &[
            " j/k  move   Enter  open",
            " 1-4  tab    s      star",
            " Esc  back   ?      all keys",
        ],
    ),
    (
        "connections",
        false,
        &[
            " j/k  move   ↵  open",
            " 1-2  tab    f  follow",
            " Esc  back   ?  all keys",
        ],
    ),
    (
        "connections",
        true,
        &[
            " j/k  move   Enter  open",
            " 1-2  tab    f      follow",
            " Esc  back   ?      all keys",
        ],
    ),
    (
        "notifications",
        false,
        &[
            " j/k  move     ↵  open",
            " 1-6  filter   a  all",
            " ?    all keys",
        ],
    ),
    (
        "notifications",
        true,
        &[
            " j/k  move     Enter  open",
            " 1-6  filter   a      all",
            " ?    all keys",
        ],
    ),
    (
        "settings_profile",
        false,
        &[
            " j/k  row    h/l  choose",
            " S    sync   D    delete",
            " 1-3  tab    ?    all keys",
        ],
    ),
    (
        "settings_profile",
        true,
        &[
            " j/k  row    h/l  choose",
            " S    sync   D    delete",
            " 1-3  tab    ?    all keys",
        ],
    ),
    (
        "settings_appearance",
        false,
        &[" j/k  row   h/l  change", " 1-3  tab   ?    all keys"],
    ),
    (
        "settings_appearance",
        true,
        &[" j/k  row   h/l  change", " 1-3  tab   ?    all keys"],
    ),
    (
        "settings_safety",
        false,
        &[
            " j/k  move     u    release",
            " R    reload   1-3  tab",
            " ?    all keys",
        ],
    ),
    (
        "settings_safety",
        true,
        &[
            " j/k  move     u    release",
            " R    reload   1-3  tab",
            " ?    all keys",
        ],
    ),
    ("help", false, &[" Esc  close   Q  quit"]),
    ("help", true, &[" Esc  close   Q  quit"]),
    (
        "composer",
        false,
        &[" ^S   send   ↵  newline", " Esc  close"],
    ),
    (
        "composer",
        true,
        &[" ^S   send   Enter  newline", " Esc  close"],
    ),
    ("confirm_delete", false, &[" y  delete   n/Esc  cancel"]),
    ("confirm_delete", true, &[" y  delete   n/Esc  cancel"]),
    ("confirm_block", false, &[" y  block   n/Esc  cancel"]),
    ("confirm_block", true, &[" y  block   n/Esc  cancel"]),
    ("confirm_account", false, &[" ↵  delete   Esc  close"]),
    ("confirm_account", true, &[" Enter  delete   Esc  close"]),
    (
        "report",
        false,
        &[" j/k  reason   Tab  details", " ↵    send     Esc  close"],
    ),
    (
        "report",
        true,
        &[
            " j/k    reason   Tab  details",
            " Enter  send     Esc  close",
        ],
    ),
    ("menu", false, &[" j/k  move   ↵  run", " Esc  close"]),
    ("menu", true, &[" j/k  move   Enter  run", " Esc  close"]),
    ("hex", false, &[" 0-9a-f  type   ↵  set", " Esc     cancel"]),
    (
        "hex",
        true,
        &[" 0-9a-f  type   Enter  set", " Esc     cancel"],
    ),
    (
        "profile_blocked",
        false,
        &[" B  unblock   Esc  back", " ?  all keys"],
    ),
    (
        "profile_blocked",
        true,
        &[" B  unblock   Esc  back", " ?  all keys"],
    ),
];

/// (context, the left part at 80 columns when the right end is `-/-  127.0.0.1:8080`)
const TRUNCATED: &[(&str, &str)] = &[
    (
        "connections",
        " Profile > Followers  j/k move  ↵ open  1-2 tab  ? help",
    ),
    (
        "profile_other",
        " Profile  j/k move  ↵ open  1-4 tab  f follow  ? help",
    ),
    (
        "settings_profile",
        " Settings  j/k row  h/l choose  S sync  D delete  ? help",
    ),
    (
        "settings_safety",
        " Settings  j/k move  u release  R reload  1-3 tab  ? help",
    ),
    (
        "signin",
        " Log > Latest  ↵ submit  Esc close  ^O browser  ^Y copy",
    ),
];

/// The context on a 120x40 terminal, before the size and the ASCII mode are set.
fn build(ctx: &str) -> App {
    let me = fixture_me();
    match ctx {
        "log" => signed_in(120, 40),
        "saved" => {
            let mut app = signed_in(120, 40);
            type_str(&mut app, "gv");
            app
        }
        "thread" => {
            let mut app = signed_in(120, 40);
            app.update(Action::Open(Screen::Thread("m1".into())));
            app
        }
        "profile_other" => {
            let mut app = signed_in(120, 40);
            open_profile(&mut app, "alice");
            app
        }
        "profile_own" => {
            let mut app = signed_in(120, 40);
            open_profile(&mut app, "myuron");
            app
        }
        "connections" => {
            let mut app = signed_in(120, 40);
            app.update(Action::Open(Screen::Connections(
                "alice".into(),
                ConnTab::Followers,
            )));
            app
        }
        "notifications" => {
            let mut app = signed_in(120, 40);
            type_str(&mut app, "gn");
            app
        }
        "settings_profile" => {
            let mut app = signed_in(120, 40);
            type_str(&mut app, "gs");
            app
        }
        "settings_appearance" => {
            let mut app = signed_in(120, 40);
            type_str(&mut app, "gs2");
            app
        }
        "settings_safety" => {
            let mut app = signed_in(120, 40);
            type_str(&mut app, "gs3");
            app
        }
        "settings_guest_appearance" => {
            let mut app = guest(120, 40);
            press(&mut app, KeyCode::Esc);
            type_str(&mut app, "gs2");
            app
        }
        "profile_blocked" => {
            let mut app = signed_in(120, 40);
            let opened = open_profile(&mut app, "alice");
            app.update(Action::UserFailed {
                username: "alice".into(),
                req: user_req(&opened),
                failure: Failure::new("user_blocked"),
                blocked_user: Some(Box::new(person("alice"))),
            });
            app
        }
        "settings_guest" => {
            let mut app = guest(120, 40);
            press(&mut app, KeyCode::Esc);
            type_str(&mut app, "gs");
            app
        }
        "help" => {
            let mut app = signed_in(120, 40);
            type_str(&mut app, "?");
            app
        }
        "signin" => {
            let mut app = guest(120, 40);
            app.update(Action::OpenSignIn);
            app
        }
        "composer" => {
            let mut app = signed_in(120, 40);
            type_str(&mut app, "c");
            app
        }
        "confirm_delete" => {
            let mut app = log_app_of(me, vec![card("c0", "myuron")]);
            key(&mut app, 'd');
            app
        }
        "confirm_block" => {
            let mut app = signed_in(120, 40);
            let opened = open_profile(&mut app, "alice");
            deliver_user(&mut app, &opened, "alice", user_resp("alice", json!({})));
            key(&mut app, 'B');
            app
        }
        "confirm_account" => {
            let mut app = signed_in(120, 40);
            type_str(&mut app, "gs");
            key(&mut app, 'D');
            app
        }
        "report" => {
            let mut app = log_app_of(me, vec![card("c0", "alice")]);
            key(&mut app, '!');
            app
        }
        "menu" => {
            let mut app = log_app_of(me, vec![card("c0", "alice")]);
            key(&mut app, '.');
            app
        }
        "hex" => {
            let mut app = signed_in(120, 40);
            type_str(&mut app, "gsj");
            for _ in 0..7 {
                key(&mut app, 'l');
            }
            press(&mut app, KeyCode::Enter);
            app
        }
        other => panic!("no scene for {other}"),
    }
}

fn scene(ctx: &str, w: u16, h: u16, ascii: bool) -> App {
    let mut app = build(ctx);
    app.update(Action::Resize(w, h));
    app.set_prefs(Prefs {
        ascii,
        ..Prefs::default()
    });
    app
}

/// The status line of the drawn app.
fn status_row(app: &App) -> String {
    let terminal = render(app);
    let buffer = terminal.backend().buffer();
    row_text(buffer, buffer.area.height - 1)
}

/// The rows inside the Keys box of the Aside, without trailing blanks; none when the Aside has no such box.
fn keys_box(app: &App) -> Option<Vec<String>> {
    let terminal = render(app);
    let buffer = terminal.backend().buffer();
    let x0 = buffer.area.width.checked_sub(32)?;
    let cell = |x: u16, y: u16| buffer[(x, y)].symbol().to_string();
    let top = (0..buffer.area.height)
        .find(|y| (0..5).map(|dx| cell(x0 + dx, *y)).collect::<String>() == "┌ Key")?;
    Some(
        (top + 1..top + 5)
            .map(|y| {
                (x0 + 1..x0 + 31)
                    .map(|x| cell(x, y))
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect(),
    )
}

/// Whether `row` starts with `left` and leaves a blank column after it.
fn shows(row: &str, left: &str) -> bool {
    row.strip_prefix(left)
        .is_some_and(|rest| rest.starts_with(' '))
}

// --- the status line ---------------------------------------------------------------------------------

#[test]
fn the_status_line_shows_the_hints_of_the_screen_or_dialog_in_every_layout() {
    let mut wrong = Vec::new();
    for (ctx, w, h, ascii, left) in STATUS {
        let row = status_row(&scene(ctx, *w, *h, *ascii));
        if !shows(&row, left) {
            wrong.push(format!(
                "{ctx} {w}x{h} ascii={ascii}: want {left:?}, got {row:?}"
            ));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn medium_drops_hints_from_the_end_before_help_when_the_right_end_is_long() {
    let mut wrong = Vec::new();
    for (ctx, left) in TRUNCATED {
        let mut app = scene(ctx, 80, 24, false);
        app.set_host_label(Some("127.0.0.1:8080".into()));
        let row = status_row(&app);
        if !shows(&row, left) || !row.trim_end().ends_with("-/-  127.0.0.1:8080") {
            wrong.push(format!("{ctx}: want {left:?}, got {row:?}"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn settings_never_offers_to_open_anything() {
    for ctx in [
        "settings_profile",
        "settings_appearance",
        "settings_safety",
        "settings_guest",
        "settings_guest_appearance",
    ] {
        for (w, h) in [(120, 40), (80, 24), (60, 24)] {
            for ascii in [false, true] {
                let row = status_row(&scene(ctx, w, h, ascii));
                assert!(!row.contains("↵ open"), "{ctx} {w}: {row:?}");
                assert!(!row.contains("Enter open"), "{ctx} {w}: {row:?}");
            }
        }
        if let Some(rows) = keys_box(&scene(ctx, 160, 40, false)) {
            assert!(rows.iter().all(|r| !r.contains("open")), "{ctx}: {rows:?}");
        }
    }
}

#[test]
fn ascii_mode_writes_enter_in_place_of_the_return_symbol_in_every_hint() {
    for (ctx, w, h, ascii, _) in STATUS {
        if !ascii {
            continue;
        }
        let row = status_row(&scene(ctx, *w, *h, true));
        assert!(!row.contains('↵'), "{ctx} {w}: {row:?}");
    }
    for (ctx, ascii, _) in KEYS {
        if !ascii {
            continue;
        }
        let rows = keys_box(&scene(ctx, 160, 40, true)).unwrap_or_default();
        assert!(rows.iter().all(|r| !r.contains('↵')), "{ctx}: {rows:?}");
    }
}

#[test]
fn the_hints_follow_the_screen_when_it_changes() {
    let mut app = scene("log", 120, 40, false);
    app.update(Action::Open(Screen::Settings));
    assert!(shows(
        &status_row(&app),
        " Settings  j/k row  h/l choose  S sync  D delete  1-3 tab  Tab pane  ? help"
    ));
    type_str(&mut app, "2");
    assert!(shows(
        &status_row(&app),
        " Settings  j/k row  h/l change  1-3 tab  Tab pane  ? help"
    ));
    press(&mut app, KeyCode::Esc);
    assert!(shows(
        &status_row(&app),
        " Log > Following  j/k move  ↵ open  c commit  Tab pane  ? help"
    ));
}

#[test]
fn closing_a_dialog_brings_the_hints_of_the_screen_back() {
    let mut app = scene("log", 120, 40, false);
    type_str(&mut app, "?");
    assert!(shows(
        &status_row(&app),
        " Log > Following  Esc close  Q quit"
    ));
    press(&mut app, KeyCode::Esc);
    assert!(shows(
        &status_row(&app),
        " Log > Following  j/k move  ↵ open  c commit  Tab pane  ? help"
    ));
}

#[test]
fn a_profile_of_your_own_does_not_offer_to_follow() {
    let own = status_row(&scene("profile_own", 120, 40, false));
    let other = status_row(&scene("profile_other", 120, 40, false));
    assert!(!own.contains("follow"), "{own:?}");
    assert!(other.contains("f follow"), "{other:?}");
}

// --- the Keys box of the Aside -----------------------------------------------------------------------

#[test]
fn the_keys_box_shows_the_hints_of_the_screen_or_dialog() {
    let mut wrong = Vec::new();
    for (ctx, ascii, want) in KEYS {
        let want: Vec<String> = (0..4)
            .map(|i| want.get(i).map_or_else(String::new, |s| s.to_string()))
            .collect();
        let got = keys_box(&scene(ctx, 160, 40, *ascii));
        if got.as_ref() != Some(&want) {
            wrong.push(format!("{ctx} ascii={ascii}: want {want:?}, got {got:?}"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn a_guest_has_no_keys_box() {
    assert_eq!(keys_box(&scene("settings_guest", 160, 40, false)), None);
}

// --- the help dialog ---------------------------------------------------------------------------------

#[test]
fn the_help_dialog_lists_the_people_the_settings_and_the_notification_keys() {
    let mut app = signed_in(120, 40);
    type_str(&mut app, "?");
    let terminal = render(&app);
    let buffer = terminal.backend().buffer();
    let screen: Vec<String> = (0..buffer.area.height)
        .map(|y| row_text(buffer, y))
        .collect();
    for needle in [
        " People ",
        "f follow",
        "m mute",
        "B block",
        " Settings ",
        "S sync",
        "D delete",
        "h/l change",
        "u release",
        " Notifications ",
        "1-6 filter",
        "a all / none",
    ] {
        assert!(
            screen.iter().any(|row| row.contains(needle)),
            "{needle:?} is missing from\n{}",
            screen.join("\n")
        );
    }
}

#[test]
fn the_help_dialog_fits_a_terminal_of_24_rows_and_leaves_the_status_line() {
    let mut app = signed_in(80, 24);
    type_str(&mut app, "?");
    let terminal = render(&app);
    let buffer = terminal.backend().buffer();
    assert!(row_text(buffer, 23).contains("Esc close  Q quit"));
    assert!(
        row_text(buffer, 22).contains('╝'),
        "{}",
        row_text(buffer, 22)
    );
}

#[test]
fn d_on_the_appearance_tab_opens_no_dialog_and_the_hints_stay() {
    let mut app = scene("settings_appearance", 120, 40, false);
    key(&mut app, 'D');
    assert_eq!(app.modal(), None);
    assert!(shows(
        &status_row(&app),
        " Settings  j/k row  h/l change  1-3 tab  Tab pane  ? help"
    ));
}

#[test]
fn a_blocked_profile_offers_unblock_and_neither_follow_nor_mute() {
    let row = status_row(&scene("profile_blocked", 120, 40, false));
    assert!(row.contains("B unblock"), "{row:?}");
    assert!(!row.contains("follow") && !row.contains("mute"), "{row:?}");
}
