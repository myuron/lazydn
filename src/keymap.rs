use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::action::Action;
use crate::app::{App, ConnTab, LogTab, Modal, Pane, ProfileTab, Screen, Session};
use crate::layout::LayoutMode;
use crate::notifications::Filter;

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
    let list = app.focus() == Pane::Main
        && matches!(
            app.screen(),
            Screen::Log(_)
                | Screen::Saved
                | Screen::Thread(_)
                | Screen::Profile(_)
                | Screen::Connections(..)
                | Screen::Notifications
        );
    if app.g_pending() {
        if list && key.code == KeyCode::Char('g') {
            return Some(Action::SelectFirst);
        }
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
    if app.focus() == Pane::Main {
        match (app.screen(), key.code) {
            (Screen::Profile(_), KeyCode::Char('1')) => {
                return Some(Action::SwitchProfileTab(ProfileTab::Commits));
            }
            (Screen::Profile(_), KeyCode::Char('2')) => {
                return Some(Action::SwitchProfileTab(ProfileTab::Replies));
            }
            (Screen::Profile(_), KeyCode::Char('3')) => {
                return Some(Action::OpenConnections(ConnTab::Followers));
            }
            (Screen::Profile(_), KeyCode::Char('4')) => {
                return Some(Action::OpenConnections(ConnTab::Following));
            }
            (Screen::Notifications, KeyCode::Char(c @ '1'..='6')) => {
                return Some(Action::ToggleFilter(
                    Filter::ALL[usize::from(c as u8 - b'1')],
                ));
            }
            (Screen::Notifications, KeyCode::Char('a')) => {
                return Some(Action::ToggleAllFilters);
            }
            (Screen::Connections(..), KeyCode::Char('1')) => {
                return Some(Action::SwitchConnTab(ConnTab::Followers));
            }
            (Screen::Connections(..), KeyCode::Char('2')) => {
                return Some(Action::SwitchConnTab(ConnTab::Following));
            }
            _ => {}
        }
    }
    if list {
        let action = match key.code {
            KeyCode::Char('j') | KeyCode::Down => Some(Action::MoveSelection(1)),
            KeyCode::Char('k') | KeyCode::Up => Some(Action::MoveSelection(-1)),
            KeyCode::Char('G') => Some(Action::SelectLast),
            KeyCode::Enter => Some(Action::OpenSelected),
            KeyCode::Char('o') => Some(Action::ToggleReplies),
            KeyCode::Char('R') => Some(Action::Refresh),
            KeyCode::Char('@') if !matches!(app.screen(), Screen::Connections(..)) => {
                Some(Action::OpenAuthor)
            }
            _ => None,
        };
        if action.is_some() {
            return action;
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
        'p' => Some(Screen::Profile(app.own_handle())),
        's' => Some(Screen::Settings),
        'v' => Some(Screen::Saved),
        _ => None,
    }
}
