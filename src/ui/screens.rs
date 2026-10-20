use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::{Block, Paragraph};

use crate::app::{App, LogTab, Pane, Screen, Session};
use crate::feed::FeedKey;
use crate::ui::feed::rows;
use crate::ui::theme;
use crate::ui::thread::rows as thread_rows;

pub fn tab_label(tab: LogTab) -> &'static str {
    match tab {
        LogTab::Following => "Following",
        LogTab::Latest => "Latest",
        LogTab::Trending => "Trending",
    }
}

fn title(screen: &Screen) -> &'static str {
    match screen {
        Screen::Thread(_) => "← Commit",
        other => screen_name(other),
    }
}

pub fn screen_name(screen: &Screen) -> &'static str {
    match screen {
        Screen::Log(_) => "Log",
        Screen::Notifications => "Notifications",
        Screen::Profile(_) => "Profile",
        Screen::Saved => "Saved",
        Screen::Settings => "Settings",
        Screen::Thread(_) => "Thread",
    }
}

/// `Paragraph::centered` rounds odd leftovers up, which would disagree with the snapshots.
fn floor_centered(cell: Rect, len: usize) -> Rect {
    let offset = (cell.width as usize).saturating_sub(len) / 2;
    Rect {
        x: cell.x + offset as u16,
        width: cell.width - offset as u16,
        ..cell
    }
}

/// Cassowary-based `Constraint::Ratio(1, 3)` splits 66 columns unevenly, which shifts the tab labels off-center.
fn thirds(row: Rect) -> [Rect; 3] {
    let edge = |i: u32| (u32::from(row.width) * i + 1) / 3;
    std::array::from_fn(|i| {
        let start = edge(i as u32) as u16;
        let end = edge(i as u32 + 1) as u16;
        Rect::new(row.x + start, row.y, end - start, row.height)
    })
}

pub fn render(app: &App, frame: &mut Frame, area: Rect) {
    let block = Block::bordered()
        .border_type(theme::border_type(app.focus() == Pane::Main))
        .title(format!(" {} ", title(&app.screen())));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    match app.screen() {
        Screen::Log(tab) => render_log(app, frame, inner, tab),
        Screen::Saved => draw_list(app, frame, inner, FeedKey::Saved),
        Screen::Thread(id) => draw_thread(app, frame, inner, &id),
        other => frame.render_widget(
            Paragraph::new(format!(" {} (not implemented yet)", screen_name(&other))),
            inner,
        ),
    }
}

fn render_log(app: &App, frame: &mut Frame, inner: Rect, selected: LogTab) {
    let screen_rows: Vec<Rect> = inner.rows().collect();
    let Some(tab_row) = screen_rows.first() else {
        return;
    };
    let cells = thirds(*tab_row);
    for (cell, tab) in cells
        .iter()
        .zip([LogTab::Following, LogTab::Latest, LogTab::Trending])
    {
        let is_selected = tab == selected;
        let label = tab_label(tab);
        let label_width = Span::raw(label).width();
        let style = if is_selected {
            theme::current_style().add_modifier(Modifier::BOLD)
        } else {
            Style::new()
        };
        frame.render_widget(
            Paragraph::new(label).style(style),
            floor_centered(*cell, label_width),
        );
        if is_selected && let Some(underline_row) = screen_rows.get(1) {
            let rect = floor_centered(
                Rect {
                    x: cell.x,
                    width: cell.width,
                    ..*underline_row
                },
                label_width,
            );
            frame.render_widget(
                Paragraph::new("━".repeat(label_width)).style(theme::current_style()),
                rect,
            );
        }
    }

    let list = Rect {
        y: tab_row.y + 2,
        height: inner.height.saturating_sub(2),
        ..inner
    };
    draw_list(app, frame, list, FeedKey::Log(selected));

    if matches!(app.session(), Session::Guest)
        && let Some(row) = screen_rows.get(3)
    {
        frame.render_widget(Paragraph::new(" sign in to see the log  [L] Sign in"), *row);
    }
}

fn draw_thread(app: &App, frame: &mut Frame, area: Rect, id: &str) {
    let scroll = app.thread(id).map_or(0, |t| usize::from(t.scroll));
    let visible = thread_rows(app, id, area.width)
        .lines
        .into_iter()
        .skip(scroll)
        .take(usize::from(area.height));
    for (row, line) in area.rows().zip(visible) {
        frame.render_widget(Paragraph::new(line), row);
    }
}

/// One `rows` line per screen row, never wrapped, so the scroll maths in `App` stays exact.
fn draw_list(app: &App, frame: &mut Frame, area: Rect, key: FeedKey) {
    let feed = app.feed(key);
    let list_rows = rows(feed, key, app, area.width);
    let visible = list_rows
        .lines
        .into_iter()
        .skip(usize::from(feed.scroll))
        .take(usize::from(area.height));
    for (row, line) in area.rows().zip(visible) {
        frame.render_widget(Paragraph::new(line), row);
    }
}
