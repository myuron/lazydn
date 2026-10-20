use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};

use crate::app::{App, Me, NavItem, Pane, Session};
use crate::layout::LayoutMode;
use crate::ui::theme;

pub fn badge(count: u32) -> Option<String> {
    match count {
        0 => None,
        1..=99 => Some(count.to_string()),
        _ => Some("99+".to_string()),
    }
}

pub fn narrow_notifs_label(count: u32) -> String {
    match badge(count) {
        Some(b) => format!("Notifs({b})"),
        None => "Notifs".to_string(),
    }
}

fn icon(item: NavItem) -> &'static str {
    match item {
        NavItem::Log => "◉",
        NavItem::Notifications => "◎",
        NavItem::Profile => "◯",
        NavItem::Saved => "⊡",
        NavItem::Settings => "⚙",
    }
}

fn label(item: NavItem) -> &'static str {
    match item {
        NavItem::Log => "Log",
        NavItem::Notifications => "Notifications",
        NavItem::Profile => "Profile",
        NavItem::Saved => "Saved",
        NavItem::Settings => "Settings",
    }
}

fn me(app: &App) -> Option<&Me> {
    match app.session() {
        Session::SignedIn(me) => Some(me),
        Session::Guest => None,
    }
}

/// Returns the marker span and the style of the rest of the item.
fn item_styles(app: &App, item: NavItem) -> (Span<'static>, Style) {
    let is_current = app.nav_item() == Some(item);
    let is_cursor = app.focus() == Pane::Nav && app.nav_cursor() == item;
    let mut style = Style::new();
    if is_current {
        style = style.patch(theme::current_style());
    }
    if is_cursor {
        style = style.patch(theme::cursor_style());
    }
    let marker = if is_current { "▌" } else { " " };
    (Span::styled(marker, style), style)
}

pub fn render(app: &App, frame: &mut Frame, area: Rect) {
    match app.layout_mode() {
        LayoutMode::Wide => render_wide(app, frame, area),
        LayoutMode::Medium => render_medium(app, frame, area),
        _ => render_narrow(app, frame, area),
    }
}

fn render_wide(app: &App, frame: &mut Frame, area: Rect) {
    let focused = app.focus() == Pane::Nav;
    let block = Block::bordered()
        .border_type(theme::border_type(focused))
        .title(" /dev/null ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let width = inner.width as usize;
    let mut lines: Vec<Line> = vec![
        Line::raw(" a social network"),
        Line::raw(" for devs"),
        Line::raw(""),
    ];
    for item in NavItem::ALL {
        let (marker, style) = item_styles(app, item);
        let text = format!("{} {}", icon(item), label(item));
        let rest = format!("{:<w$}", text, w = width.saturating_sub(1));
        lines.push(Line::from(vec![marker, Span::styled(rest, style)]));
        if item == NavItem::Notifications
            && let Some(b) = me(app).and_then(|m| badge(m.unread))
        {
            lines.push(Line::raw(format!("{b:>width$}")));
        }
    }
    lines.push(Line::raw(""));
    lines.push(Line::raw(match app.session() {
        Session::Guest => " [ L  Sign in ]",
        Session::SignedIn(_) => " [ c  Commit ]",
    }));
    frame.render_widget(Paragraph::new(lines), inner);

    if let Some(me) = me(app)
        && let Some(last) = inner.rows().next_back()
    {
        frame.render_widget(Paragraph::new(format!(" @{}", me.handle)), last);
    }
}

fn render_medium(app: &App, frame: &mut Frame, area: Rect) {
    let block = Block::bordered().border_type(theme::border_type(app.focus() == Pane::Nav));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines = vec![Line::raw("")];
    for item in NavItem::ALL {
        let (marker, style) = item_styles(app, item);
        lines.push(Line::from(vec![marker, Span::styled(icon(item), style)]));
    }
    lines.push(Line::raw(""));
    lines.push(Line::raw(match app.session() {
        Session::Guest => " L",
        Session::SignedIn(_) => " +",
    }));
    frame.render_widget(Paragraph::new(lines), inner);
}

fn render_narrow(app: &App, frame: &mut Frame, area: Rect) {
    let block = Block::bordered()
        .border_type(theme::border_type(app.focus() == Pane::Nav))
        .title(" /dev/null ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut spans: Vec<Span> = Vec::new();
    for item in NavItem::ALL {
        if !spans.is_empty() {
            spans.push(Span::raw(" "));
        }
        let (marker, style) = item_styles(app, item);
        let text = match item {
            NavItem::Notifications => narrow_notifs_label(me(app).map_or(0, |m| m.unread)),
            other => label(other).to_string(),
        };
        spans.push(marker);
        spans.push(Span::styled(text, style));
    }
    let used: usize = spans.iter().map(Span::width).sum();
    frame.render_widget(Paragraph::new(Line::from(spans)), inner);

    const HINT: &str = "[L] Sign in";
    // Dropped when it would collide with the items instead of wrapping.
    if matches!(app.session(), Session::Guest) && used + HINT.len() < inner.width as usize {
        frame.render_widget(Paragraph::new(HINT).alignment(Alignment::Right), inner);
    }
}
