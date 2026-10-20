use ratatui::layout::{Constraint, Layout, Rect};

// Not removed yet: logic_tests imports LayoutMode through this path; drop it once tests import crate::layout.
pub use crate::layout::LayoutMode;

/// `nav` is the top navbar in Narrow mode and the left column otherwise.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Panes {
    pub nav: Rect,
    pub main: Rect,
    pub aside: Option<Rect>,
    pub status: Rect,
}

pub fn split(area: Rect, mode: LayoutMode) -> Panes {
    if mode == LayoutMode::TooSmall || area.height == 0 {
        return Panes::default();
    }
    let body = Rect {
        height: area.height - 1,
        ..area
    };
    let status = Rect::new(area.x, area.y + body.height, area.width, 1);
    match mode {
        LayoutMode::Wide => {
            let [nav, main, aside] = Layout::horizontal([
                Constraint::Length(20),
                Constraint::Min(0),
                Constraint::Length(32),
            ])
            .areas(body);
            Panes {
                nav,
                main,
                aside: Some(aside),
                status,
            }
        }
        LayoutMode::Medium => {
            let [nav, main] =
                Layout::horizontal([Constraint::Length(4), Constraint::Min(0)]).areas(body);
            Panes {
                nav,
                main,
                aside: None,
                status,
            }
        }
        _ => {
            let [nav, main] =
                Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).areas(body);
            Panes {
                nav,
                main,
                aside: None,
                status,
            }
        }
    }
}
