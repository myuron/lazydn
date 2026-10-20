use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::action::Action;
use crate::app::{App, LogTab, Modal, Pane, Screen, Session};
use crate::layout::LayoutMode;

pub fn map_key(app: &App, key: KeyEvent) -> Option<Action> {
    if key.kind != KeyEventKind::Press {
        return None;
    }
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    if ctrl {
        return (key.code == KeyCode::Char('c')).then_some(Action::Quit);
    }
    match app.modal() {
        Some(Modal::SignIn) => return (key.code == KeyCode::Esc).then_some(Action::CloseModal),
        Some(Modal::Help) => {
            return match key.code {
                KeyCode::Esc | KeyCode::Char('?') => Some(Action::CloseModal),
                KeyCode::Char('Q') => Some(Action::Quit),
                _ => None,
            };
        }
        None => {}
    }
    if key.code == KeyCode::Char('Q') {
        return Some(Action::Quit);
    }
    if app.g_pending() {
        return Some(match key.code {
            KeyCode::Char(c) => g_target(app, c).map_or(Action::CancelGPrefix, Action::Goto),
            _ => Action::CancelGPrefix,
        });
    }
    let narrow = app.layout_mode() == LayoutMode::Narrow;
    match key.code {
        KeyCode::Esc => return Some(Action::Back),
        KeyCode::Tab => return Some(Action::FocusNext),
        KeyCode::BackTab => return Some(Action::FocusPrev),
        KeyCode::Char('j') if app.focus() == Pane::Nav => return Some(Action::NavStep(1)),
        KeyCode::Char('k') if app.focus() == Pane::Nav => return Some(Action::NavStep(-1)),
        KeyCode::Char('l') if app.focus() == Pane::Nav && narrow => {
            return Some(Action::NavStep(1));
        }
        KeyCode::Char('h') if app.focus() == Pane::Nav && narrow => {
            return Some(Action::NavStep(-1));
        }
        KeyCode::Enter if app.focus() == Pane::Nav => return Some(Action::NavEnter),
        _ => {}
    }
    if app.focus() == Pane::Main && matches!(app.screen(), Screen::Log(_)) {
        match key.code {
            KeyCode::Char('1') => return Some(Action::SwitchTab(LogTab::Following)),
            KeyCode::Char('2') => return Some(Action::SwitchTab(LogTab::Latest)),
            KeyCode::Char('3') => return Some(Action::SwitchTab(LogTab::Trending)),
            _ => {}
        }
    }
    match key.code {
        KeyCode::Char('g') => Some(Action::StartGPrefix),
        KeyCode::Char('?') => Some(Action::OpenHelp),
        KeyCode::Char('L') if matches!(app.session(), Session::Guest) => Some(Action::OpenSignIn),
        KeyCode::Char('c') => Some(Action::Commit),
        _ => None,
    }
}

fn g_target(app: &App, c: char) -> Option<Screen> {
    match c {
        'l' => Some(Screen::Log(app.home_tab())),
        'e' => Some(Screen::Log(LogTab::Trending)),
        'n' => Some(Screen::Notifications),
        'p' => Some(Screen::Profile),
        's' => Some(Screen::Settings),
        'v' => Some(Screen::Saved),
        _ => None,
    }
}
