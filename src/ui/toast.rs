use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};
use unicode_width::UnicodeWidthStr;

use crate::app::{App, ToastKind};
use crate::ui::theme::PRIMARY;

/// Draws the toast right above the status line, flush right with a one column margin.
pub fn render(app: &App, frame: &mut Frame, area: Rect, status: Rect) {
    let Some(toast) = app.toast() else {
        return;
    };
    let (symbol, color) = match toast.kind {
        ToastKind::Success => ("✓", Color::Green),
        ToastKind::Error => ("✗", Color::Red),
        ToastKind::Info => ("i", PRIMARY),
    };
    let content = format!(" {}", toast.text);
    let wanted = u16::try_from(symbol.width() + content.width() + 4).unwrap_or(u16::MAX);
    let width = wanted.min(area.width.saturating_sub(2));
    let y = status.y.saturating_sub(3);
    if width == 0 || y < area.y {
        return;
    }
    let rect = Rect::new(area.x + area.width - 1 - width, y, width, 3);
    let line = Line::from(vec![
        Span::styled(symbol, Style::new().fg(color)),
        Span::raw(content),
    ]);
    let block = Block::bordered().padding(ratatui::widgets::Padding::horizontal(1));
    let inner = block.inner(rect);
    frame.render_widget(Clear, rect);
    frame.render_widget(block, rect);
    frame.render_widget(Paragraph::new(line), inner);
}
