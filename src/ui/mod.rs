pub mod aside;
pub mod card;
pub mod compose;
pub mod connections;
pub mod dialogs;
pub mod feed;
pub mod help;
pub mod layout;
pub mod nav;
pub mod notifications;
pub mod profile;
pub mod screens;
pub mod settings;
pub mod signin;
pub mod statusline;
pub mod theme;
pub mod thread;
pub mod toast;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::Paragraph;

use crate::app::{App, Modal};
use crate::layout::LayoutMode;

/// Centers a modal and shrinks it to fit, so small terminals never index out of bounds.
pub(crate) fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let w = width.min(area.width);
    let h = height.min(area.height);
    Rect::new(
        area.x + (area.width - w) / 2,
        area.y + (area.height - h) / 2,
        w,
        h,
    )
}

pub fn draw(app: &App, frame: &mut Frame) {
    let area = frame.area();
    let mode = app.layout_mode();
    if mode == LayoutMode::TooSmall {
        frame.render_widget(Paragraph::new("terminal too small (need 20 rows)"), area);
        return;
    }
    let panes = layout::split(area, mode);
    nav::render(app, frame, panes.nav);
    screens::render(app, frame, panes.main);
    if let Some(aside) = panes.aside {
        aside::render(app, frame, aside);
    }
    statusline::render(app, frame, panes.status);
    match app.modal() {
        Some(Modal::Help) => help::render(frame, area),
        Some(Modal::SignIn) => signin::render(app, frame, area),
        Some(Modal::Composer) => compose::render_modal(app, frame, area),
        Some(Modal::ConfirmDelete) => {
            if let Some(confirm) = app.delete_confirm() {
                dialogs::render_confirm(confirm, frame);
            }
        }
        Some(Modal::Report) => {
            if let Some(report) = app.report() {
                dialogs::render_report(report, frame);
            }
        }
        None => {}
    }
    toast::render(app, frame, area, panes.status);
}

#[cfg(test)]
mod card_tests;
#[cfg(test)]
mod compose_tests;
#[cfg(test)]
mod feed_tests;
#[cfg(test)]
mod notifications_tests;
#[cfg(test)]
mod profile_tests;
#[cfg(test)]
mod removal_tests;
#[cfg(test)]
mod reply_tests;
#[cfg(test)]
mod settings_tests;
#[cfg(test)]
mod signin_tests;
#[cfg(test)]
mod status_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod thread_tests;
#[cfg(test)]
mod write_tests;
