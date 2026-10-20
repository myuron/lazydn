use crate::action::{Action, Effect};
use crate::layout::LayoutMode;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Me {
    pub name: String,
    pub handle: String,
    pub commits: u32,
    pub following: u32,
    pub followers: u32,
    pub pasteis_left: u32,
    pub pasteis_max: u32,
    pub bio: String,
    pub unread: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Session {
    Guest,
    SignedIn(Me),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogTab {
    Following,
    Latest,
    Trending,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Log(LogTab),
    Notifications,
    Profile,
    Saved,
    Settings,
    Thread,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Nav,
    Main,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modal {
    Help,
    SignIn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavItem {
    Log,
    Notifications,
    Profile,
    Saved,
    Settings,
}

impl NavItem {
    pub fn of_screen(screen: Screen) -> Option<NavItem> {
        match screen {
            Screen::Log(_) => Some(NavItem::Log),
            Screen::Notifications => Some(NavItem::Notifications),
            Screen::Profile => Some(NavItem::Profile),
            Screen::Saved => Some(NavItem::Saved),
            Screen::Settings => Some(NavItem::Settings),
            Screen::Thread => None,
        }
    }

    fn index(self) -> usize {
        self as usize
    }

    pub const ALL: [NavItem; 5] = [
        NavItem::Log,
        NavItem::Notifications,
        NavItem::Profile,
        NavItem::Saved,
        NavItem::Settings,
    ];
}

#[derive(Debug, Clone)]
pub struct App {
    session: Session,
    screen: Screen,
    history: Vec<Screen>,
    focus: Pane,
    modal: Option<Modal>,
    nav_cursor: NavItem,
    g_pending: bool,
    size: (u16, u16),
}

impl App {
    pub fn new(session: Session) -> Self {
        let guest = matches!(session, Session::Guest);
        Self {
            screen: Screen::Log(if guest {
                LogTab::Latest
            } else {
                LogTab::Following
            }),
            modal: guest.then_some(Modal::SignIn),
            session,
            history: Vec::new(),
            focus: Pane::Main,
            nav_cursor: NavItem::Log,
            g_pending: false,
            size: (0, 0),
        }
    }

    pub fn update(&mut self, action: Action) -> Vec<Effect> {
        match action {
            Action::Quit => return vec![Effect::Quit],
            Action::Resize(w, h) => self.size = (w, h),
            Action::Open(screen) => {
                if self.guard_sign_in(screen) {
                    self.history.push(self.screen);
                    self.show(screen);
                }
            }
            Action::Goto(screen) => {
                self.g_pending = false;
                self.goto(screen);
            }
            Action::Back => {
                if let Some(previous) = self.history.pop() {
                    self.show(previous);
                }
            }
            Action::StartGPrefix => self.g_pending = true,
            Action::CancelGPrefix => self.g_pending = false,
            Action::FocusNext | Action::FocusPrev => {
                self.focus = match self.focus {
                    Pane::Nav => Pane::Main,
                    Pane::Main => {
                        self.sync_cursor();
                        Pane::Nav
                    }
                };
            }
            Action::NavStep(step) => {
                let last = NavItem::ALL.len() - 1;
                let next = self
                    .nav_cursor
                    .index()
                    .saturating_add_signed(step as isize)
                    .min(last);
                self.nav_cursor = NavItem::ALL[next];
            }
            Action::NavEnter => {
                let target = match self.nav_cursor {
                    NavItem::Log => match self.screen {
                        Screen::Log(_) => self.screen,
                        _ => Screen::Log(self.home_tab()),
                    },
                    NavItem::Notifications => Screen::Notifications,
                    NavItem::Profile => Screen::Profile,
                    NavItem::Saved => Screen::Saved,
                    NavItem::Settings => Screen::Settings,
                };
                if self.goto(target) {
                    self.focus = Pane::Main;
                }
            }
            Action::SwitchTab(tab) => {
                if matches!(self.screen, Screen::Log(_)) {
                    self.goto_tab(tab);
                }
            }
            Action::OpenHelp => self.modal = Some(Modal::Help),
            Action::OpenSignIn => self.modal = Some(Modal::SignIn),
            Action::CloseModal => self.modal = None,
            Action::Commit => {
                if matches!(self.session, Session::Guest) {
                    self.modal = Some(Modal::SignIn);
                }
            }
        }
        Vec::new()
    }

    pub fn home_tab(&self) -> LogTab {
        match self.session {
            Session::Guest => LogTab::Latest,
            Session::SignedIn(_) => LogTab::Following,
        }
    }

    fn needs_sign_in(&self, screen: Screen) -> bool {
        matches!(self.session, Session::Guest)
            && matches!(
                screen,
                Screen::Log(LogTab::Following)
                    | Screen::Notifications
                    | Screen::Profile
                    | Screen::Saved
                    | Screen::Settings
            )
    }

    /// Returns false after opening the sign-in dialog when a guest targets a protected screen.
    fn guard_sign_in(&mut self, screen: Screen) -> bool {
        if self.needs_sign_in(screen) {
            self.modal = Some(Modal::SignIn);
            return false;
        }
        true
    }

    /// Replaces the screen and clears the history; returns false when the sign-in dialog opened instead.
    fn goto(&mut self, screen: Screen) -> bool {
        if !self.guard_sign_in(screen) {
            return false;
        }
        self.history.clear();
        self.show(screen);
        true
    }

    fn goto_tab(&mut self, tab: LogTab) {
        let screen = Screen::Log(tab);
        if self.guard_sign_in(screen) {
            self.screen = screen;
        }
    }

    fn show(&mut self, screen: Screen) {
        self.screen = screen;
        self.sync_cursor();
    }

    fn sync_cursor(&mut self) {
        if let Some(item) = NavItem::of_screen(self.screen) {
            self.nav_cursor = item;
        }
    }

    pub fn session(&self) -> &Session {
        &self.session
    }

    pub fn screen(&self) -> Screen {
        self.screen
    }

    pub fn focus(&self) -> Pane {
        self.focus
    }

    pub fn modal(&self) -> Option<Modal> {
        self.modal
    }

    pub fn nav_cursor(&self) -> NavItem {
        self.nav_cursor
    }

    pub fn g_pending(&self) -> bool {
        self.g_pending
    }

    pub fn size(&self) -> (u16, u16) {
        self.size
    }

    pub fn layout_mode(&self) -> LayoutMode {
        LayoutMode::from_size(self.size.0, self.size.1)
    }
}
