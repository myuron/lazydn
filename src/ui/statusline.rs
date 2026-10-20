use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::widgets::Paragraph;

use crate::app::{App, Screen};
use crate::layout::LayoutMode;
use crate::thread::short_hash;
use crate::ui::screens::{screen_name, tab_label};

pub fn breadcrumb(app: &App) -> String {
    match app.screen() {
        Screen::Log(tab) => format!("Log > {}", tab_label(tab)),
        Screen::Thread(id) => format!("Commit {}", short_hash(&id)),
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

pub fn right(mode: LayoutMode) -> String {
    match mode {
        LayoutMode::Wide => "API -/-  ○ connecting".to_string(),
        LayoutMode::TooSmall => String::new(),
        _ => "-/-".to_string(),
    }
}

pub fn render(app: &App, frame: &mut Frame, area: Rect) {
    let mode = app.layout_mode();
    frame.render_widget(
        Paragraph::new(format!(" {}", left(mode, &breadcrumb(app)))),
        area,
    );
    frame.render_widget(
        Paragraph::new(format!("{} ", right(mode))).alignment(Alignment::Right),
        area,
    );
}
