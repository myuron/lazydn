use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::action::Action;
use crate::app::{App, ConnTab, LogTab, Modal, Pane, ProfileTab, Screen, Session, SettingsTab};
use crate::layout::LayoutMode;
use crate::notifications::Filter;

/// Maps any terminal event to an action; a paste only reaches the sign-in dialog or an open composer.
pub fn map_event(app: &App, event: &Event) -> Option<Action> {
    match event {
        Event::Key(key) => map_key(app, *key),
        Event::Paste(text)
            if app.modal() == Some(Modal::SignIn)
                || app.composer_focused()
                || app.report_input_focused() =>
        {
            Some(Action::Paste(text.clone()))
        }
        Event::Resize(w, h) => Some(Action::Resize(*w, *h)),
        _ => None,
    }
}

/// Handled before the shared Ctrl and Press filters: the dialog reads a held Backspace, and Ctrl+letter means
/// something here that it means nowhere else.
fn map_sign_in_key(key: KeyEvent) -> Option<Action> {
    let pressed = key.kind == KeyEventKind::Press;
    let typed = key.kind != KeyEventKind::Release;
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        let KeyCode::Char(c) = key.code else {
            return None;
        };
        return match c {
            'c' if pressed => Some(Action::Quit),
            'o' if pressed => Some(Action::SignInOpenBrowser),
            'y' if pressed => Some(Action::SignInCopyUrl),
            'u' if typed => Some(Action::SignInClear),
            'r' if pressed => Some(Action::SignInRefresh),
            _ => None,
        };
    }
    match key.code {
        KeyCode::Esc if pressed => Some(Action::CloseModal),
        KeyCode::Enter if pressed => Some(Action::SignInSubmit),
        KeyCode::Backspace if typed => Some(Action::SignInBackspace),
        KeyCode::Char(c) if typed && !key.modifiers.contains(KeyModifiers::ALT) => {
            Some(Action::SignInInput(c))
        }
        _ => None,
    }
}

/// A text field reads everything as text, so that no global key fires while typing; only Esc, Enter, Backspace and
/// Ctrl+C mean something else.
fn map_text_key(key: KeyEvent, esc: Action) -> Option<Action> {
    let pressed = key.kind == KeyEventKind::Press;
    let typed = key.kind != KeyEventKind::Release;
    let text = |t| Some(Action::SettingsText(t));
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return (pressed && key.code == KeyCode::Char('c')).then_some(Action::Quit);
    }
    match key.code {
        KeyCode::Esc if pressed => Some(esc),
        KeyCode::Enter if pressed => text(SettingsText::Submit),
        KeyCode::Backspace if typed => text(SettingsText::Backspace),
        KeyCode::Char(c) if typed && !key.modifiers.contains(KeyModifiers::ALT) => {
            text(SettingsText::Insert(c))
        }
        _ => None,
    }
}

/// The keys of the Settings tabs while Main has the focus; the tab keys `1`..`3` are handled before.
fn map_settings_key(app: &App, key: KeyEvent) -> Option<Action> {
    let step = |n: i8| Some(Action::SettingsStep(n));
    match (app.settings().tab, key.code) {
        (SettingsTab::Appearance, _) => None,
        (_, KeyCode::Char('j') | KeyCode::Down) => Some(Action::MoveSelection(1)),
        (_, KeyCode::Char('k') | KeyCode::Up) => Some(Action::MoveSelection(-1)),
        (_, KeyCode::Enter) => Some(Action::OpenSelected),
        (SettingsTab::Profile, KeyCode::Char('l') | KeyCode::Right) => step(1),
        (SettingsTab::Profile, KeyCode::Char('h') | KeyCode::Left) => step(-1),
        (SettingsTab::Profile, KeyCode::Char(' ')) => Some(Action::SettingsToggle),
        (SettingsTab::Profile, KeyCode::Char('S')) => Some(Action::SyncGithub),
        (SettingsTab::Profile, KeyCode::Char('D')) => Some(Action::OpenAccountDelete),
        (SettingsTab::Safety, KeyCode::Char('G')) => Some(Action::SelectLast),
        (SettingsTab::Safety, KeyCode::Char('u')) => Some(Action::ReleaseSelected),
        _ => None,
    }
}

/// The confirmation reads `y`, `n`, Esc and Ctrl+C; every other key is ignored so that nothing slips past it.
fn map_confirm_key(key: KeyEvent, yes: Action) -> Option<Action> {
    if key.kind != KeyEventKind::Press {
        return None;
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return (key.code == KeyCode::Char('c')).then_some(Action::Quit);
    }
    match key.code {
        KeyCode::Char('y') => Some(yes),
        KeyCode::Char('n') | KeyCode::Esc => Some(Action::CloseModal),
        _ => None,
    }
}

/// With the details focused every key but Esc, Tab, Enter and Ctrl+C is text, so that no global key fires.
fn map_report_key(app: &App, key: KeyEvent) -> Option<Action> {
    let pressed = key.kind == KeyEventKind::Press;
    let typed = key.kind != KeyEventKind::Release;
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return (pressed && key.code == KeyCode::Char('c')).then_some(Action::Quit);
    }
    let details = app.report_input_focused();
    let report = |k| Some(Action::ReportKey(k));
    match key.code {
        KeyCode::Esc if pressed => Some(Action::CloseModal),
        KeyCode::Tab | KeyCode::BackTab if pressed => report(ReportKey::Tab),
        KeyCode::Enter if pressed => report(ReportKey::Submit),
        KeyCode::Char(c) if details && typed && !key.modifiers.contains(KeyModifiers::ALT) => {
            report(ReportKey::Insert(c))
        }
        KeyCode::Backspace if details && typed => report(ReportKey::Backspace),
        KeyCode::Delete if details && typed => report(ReportKey::Delete),
        KeyCode::Left if details && typed => report(ReportKey::Left),
        KeyCode::Right if details && typed => report(ReportKey::Right),
        KeyCode::Home if details && typed => report(ReportKey::Home),
        KeyCode::End if details && typed => report(ReportKey::End),
        KeyCode::Char('j') | KeyCode::Down if !details && pressed => report(ReportKey::Down),
        KeyCode::Char('k') | KeyCode::Up if !details && pressed => report(ReportKey::Up),
        _ => None,
    }
}

/// The menu reads the choice keys, the keys of its items, Esc and Ctrl+C; the rest is ignored so that nothing
/// slips past it.
fn map_menu_key(app: &App, key: KeyEvent) -> Option<Action> {
    if key.kind != KeyEventKind::Press {
        return None;
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return (key.code == KeyCode::Char('c')).then_some(Action::Quit);
    }
    let menu = |k| Some(Action::MenuKey(k));
    match key.code {
        KeyCode::Esc => Some(Action::CloseModal),
        KeyCode::Enter => menu(MenuKey::Select),
        KeyCode::Char('j') | KeyCode::Down => menu(MenuKey::Down),
        KeyCode::Char('k') | KeyCode::Up => menu(MenuKey::Up),
        KeyCode::Char(c)
            if !key.modifiers.contains(KeyModifiers::ALT)
                && app
                    .menu()
                    .is_some_and(|m| m.items.iter().any(|i| i.key == c)) =>
        {
            menu(MenuKey::Item(c))
        }
        _ => None,
    }
}

pub fn map_key(app: &App, key: KeyEvent) -> Option<Action> {
    if app.modal() == Some(Modal::SignIn) {
        return map_sign_in_key(key);
    }
    if app.composer_focused() {
        return map_composer_key(key).map(Action::Compose);
    }
    match app.modal() {
        Some(Modal::ConfirmDelete) => return map_confirm_key(key, Action::ConfirmDelete),
        Some(Modal::ConfirmBlock) => return map_confirm_key(key, Action::ConfirmBlock),
        Some(Modal::ConfirmAccountDelete) => return map_text_key(key, Action::CloseModal),
        Some(Modal::Report) => return map_report_key(app, key),
        Some(Modal::Menu) => return map_menu_key(app, key),
        _ => {}
    }
    if app.settings_typing() && app.modal().is_none() {
        return map_text_key(key, Action::SettingsText(SettingsText::Cancel));
    }
    if key.kind != KeyEventKind::Press {
        return None;
    }
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    if ctrl {
        return (key.code == KeyCode::Char('c')).then_some(Action::Quit);
    }
    if app.modal() == Some(Modal::Help) {
        return match key.code {
            KeyCode::Esc | KeyCode::Char('?') => Some(Action::CloseModal),
            KeyCode::Char('Q') => Some(Action::Quit),
            _ => None,
        };
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
    let safety_list = app.focus() == Pane::Main
        && matches!(app.screen(), Screen::Settings)
        && app.settings().tab == SettingsTab::Safety;
    if app.g_pending() {
        if (list || safety_list) && key.code == KeyCode::Char('g') {
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
            (Screen::Settings, KeyCode::Char(c @ '1'..='3')) => {
                return Some(Action::SwitchSettingsTab(
                    [
                        SettingsTab::Profile,
                        SettingsTab::Appearance,
                        SettingsTab::Safety,
                    ][usize::from(c as u8 - b'1')],
                ));
            }
            (Screen::Settings, KeyCode::Char('R')) => return Some(Action::Refresh),
            (Screen::Connections(..), KeyCode::Char('1')) => {
                return Some(Action::SwitchConnTab(ConnTab::Followers));
            }
            (Screen::Connections(..), KeyCode::Char('2')) => {
                return Some(Action::SwitchConnTab(ConnTab::Following));
            }
            _ => {}
        }
    }
    if app.focus() == Pane::Main
        && matches!(app.screen(), Screen::Settings)
        && let Some(action) = map_settings_key(app, key)
    {
        return Some(action);
    }
    let commit_cards = matches!(
        app.screen(),
        Screen::Log(_) | Screen::Saved | Screen::Thread(_) | Screen::Profile(_)
    );
    if list {
        let action = match key.code {
            KeyCode::Char('j') | KeyCode::Down => Some(Action::MoveSelection(1)),
            KeyCode::Char('k') | KeyCode::Up => Some(Action::MoveSelection(-1)),
            KeyCode::Char('G') => Some(Action::SelectLast),
            KeyCode::Enter => Some(Action::OpenSelected),
            KeyCode::Char('o') => Some(Action::ToggleReplies),
            KeyCode::Char('R') => Some(Action::Refresh),
            KeyCode::Char('r') if commit_cards => Some(Action::Reply),
            KeyCode::Char('q') if commit_cards => Some(Action::Quote),
            KeyCode::Char('s') if commit_cards => Some(Action::ToggleStar),
            KeyCode::Char('b') if commit_cards => Some(Action::ToggleSave),
            KeyCode::Char('p') if commit_cards => Some(Action::GivePastel),
            KeyCode::Char('y') if commit_cards => Some(Action::CopyLink),
            KeyCode::Char('O') if commit_cards => Some(Action::OpenInBrowser),
            KeyCode::Char('d') if commit_cards => Some(Action::Delete),
            KeyCode::Char('!') if commit_cards => Some(Action::Report),
            KeyCode::Char('.') if commit_cards => Some(Action::OpenMenu),
            KeyCode::Char('f')
                if matches!(app.screen(), Screen::Profile(_) | Screen::Connections(..)) =>
            {
                Some(Action::ToggleFollow)
            }
            KeyCode::Char('m') if matches!(app.screen(), Screen::Profile(_)) => {
                Some(Action::ToggleMute)
            }
            KeyCode::Char('B') if matches!(app.screen(), Screen::Profile(_)) => Some(Action::Block),
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

/// A key typed into the hex field of the lane color or into the username field of the account deletion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsText {
    Insert(char),
    Backspace,
    /// Enter.
    Submit,
    /// Esc in the hex field; the deletion dialog closes with `CloseModal`.
    Cancel,
}

/// A key typed in the more menu; Esc and Ctrl+C are `CloseModal` and `Quit` instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuKey {
    Up,
    Down,
    /// Enter: runs the chosen item.
    Select,
    /// The key an item starts with: runs that item.
    Item(char),
}

/// A key typed in the report dialog; Esc and Ctrl+C are `CloseModal` and `Quit` instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportKey {
    Up,
    Down,
    /// Tab: moves the focus between the reasons and the details.
    Tab,
    /// Enter: sends the report.
    Submit,
    Insert(char),
    Backspace,
    Delete,
    Left,
    Right,
    Home,
    End,
}

/// An editing or control key typed while a composer has the focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComposerKey {
    Insert(char),
    Newline,
    /// Tab: spaces, not a focus change.
    Tab,
    Backspace,
    Delete,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    /// Ctrl+U.
    KillToLineStart,
    /// Ctrl+W.
    DeleteWordBack,
    /// Ctrl+Enter or Ctrl+S.
    Submit,
    /// Esc.
    Close,
    /// Ctrl+C.
    Quit,
}

/// Every key a composer reads, so that no global binding fires while typing. Edits repeat when held;
/// submit, close and quit act on the press only.
pub fn map_composer_key(key: KeyEvent) -> Option<ComposerKey> {
    let pressed = key.kind == KeyEventKind::Press;
    if key.kind == KeyEventKind::Release {
        return None;
    }
    let mods = key.modifiers;
    let ctrl = mods.contains(KeyModifiers::CONTROL);
    let alt = mods.contains(KeyModifiers::ALT);
    match key.code {
        KeyCode::Char(c) if ctrl && alt => {
            (!c.is_ascii_alphabetic()).then_some(ComposerKey::Insert(c))
        }
        KeyCode::Char(c) if ctrl => match c {
            'c' if pressed => Some(ComposerKey::Quit),
            's' if pressed => Some(ComposerKey::Submit),
            'j' => Some(ComposerKey::Newline),
            'u' => Some(ComposerKey::KillToLineStart),
            'w' => Some(ComposerKey::DeleteWordBack),
            _ => None,
        },
        KeyCode::Char(c) if !alt => Some(ComposerKey::Insert(c)),
        KeyCode::Enter if ctrl => pressed.then_some(ComposerKey::Submit),
        KeyCode::Enter => Some(ComposerKey::Newline),
        KeyCode::Esc if pressed => Some(ComposerKey::Close),
        KeyCode::Tab => Some(ComposerKey::Tab),
        KeyCode::Backspace => Some(ComposerKey::Backspace),
        KeyCode::Delete => Some(ComposerKey::Delete),
        KeyCode::Left => Some(ComposerKey::Left),
        KeyCode::Right => Some(ComposerKey::Right),
        KeyCode::Up => Some(ComposerKey::Up),
        KeyCode::Down => Some(ComposerKey::Down),
        KeyCode::Home => Some(ComposerKey::Home),
        KeyCode::End => Some(ComposerKey::End),
        _ => None,
    }
}
