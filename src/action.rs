use crate::app::{LogTab, Screen};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Quit,
    Resize(u16, u16),
    /// Pushes onto the history so that Esc can return.
    Open(Screen),
    /// Replaces the screen and clears the history (g keys, Nav).
    Goto(Screen),
    Back,
    StartGPrefix,
    CancelGPrefix,
    FocusNext,
    FocusPrev,
    NavStep(i8),
    NavEnter,
    SwitchTab(LogTab),
    OpenHelp,
    OpenSignIn,
    CloseModal,
    Commit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    Quit,
}
