use ratatui::style::{Color, Style};
use ratatui::widgets::BorderType;

pub const PRIMARY: Color = Color::Cyan;
pub const CURSOR_BG: Color = Color::DarkGray;

pub fn border_type(focused: bool) -> BorderType {
    if focused {
        BorderType::Thick
    } else {
        BorderType::Plain
    }
}

pub fn current_style() -> Style {
    Style::new().fg(PRIMARY)
}

pub fn cursor_style() -> Style {
    Style::new().bg(CURSOR_BG)
}
