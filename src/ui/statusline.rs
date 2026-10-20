use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::widgets::Paragraph;

use crate::api::client::RateLimit;
use crate::app::{App, ConnState, ConnTab, Screen};
use crate::layout::LayoutMode;
use crate::thread::short_hash;
use crate::ui::screens::{screen_name, tab_label};

pub fn breadcrumb(app: &App) -> String {
    match app.screen() {
        Screen::Log(tab) => format!("Log > {}", tab_label(tab)),
        Screen::Thread(id) => format!("Commit {}", short_hash(&id)),
        Screen::Connections(_, ConnTab::Followers) => "Profile > Followers".to_string(),
        Screen::Connections(_, ConnTab::Following) => "Profile > Following".to_string(),
        other => screen_name(&other).to_string(),
    }
}

pub fn left(mode: LayoutMode, breadcrumb: &str) -> String {
    match mode {
        LayoutMode::Wide => {
            format!("{breadcrumb}  j/k move  ↵ open  c commit  Tab pane  ? help")
        }
        LayoutMode::Medium => format!("{breadcrumb}  j/k move  ↵ open  c commit  ? help"),
        LayoutMode::Narrow => "j/k ↵ c ?".to_string(),
        LayoutMode::TooSmall => String::new(),
    }
}

/// The right end before any response has arrived.
pub fn right(mode: LayoutMode) -> String {
    right_with(mode, None, ConnState::Connecting, None)
}

/// The right end of the status line.
/// `host` is shown only when the base URL is not the default.
pub fn right_with(
    mode: LayoutMode,
    rate: Option<RateLimit>,
    conn: ConnState,
    host: Option<&str>,
) -> String {
    let count = rate.map_or_else(
        || "-/-".to_string(),
        |r| format!("{}/{}", r.remaining, r.limit),
    );
    let base = match mode {
        LayoutMode::TooSmall => return String::new(),
        LayoutMode::Wide => {
            let state = match conn {
                ConnState::Connecting => "○ connecting",
                ConnState::Online => "● online",
                ConnState::Offline => "○ offline",
            };
            format!("API {count}  {state}")
        }
        LayoutMode::Medium | LayoutMode::Narrow if conn == ConnState::Offline => {
            format!("○ {count}")
        }
        LayoutMode::Medium | LayoutMode::Narrow => count,
    };
    match host {
        Some(host) => format!("{base}  {host}"),
        None => base,
    }
}

pub fn render(app: &App, frame: &mut Frame, area: Rect) {
    let mode = app.layout_mode();
    frame.render_widget(
        Paragraph::new(format!(" {}", left(mode, &breadcrumb(app)))),
        area,
    );
    frame.render_widget(
        Paragraph::new(format!(
            "{} ",
            right_with(mode, app.rate(), app.conn_state(), app.host_label())
        ))
        .alignment(Alignment::Right),
        area,
    );
}
