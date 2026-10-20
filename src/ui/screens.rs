use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};

use crate::app::{App, ConnTab, LogTab, Pane, Screen, Session, SettingsTab};
use crate::feed::FeedKey;
use crate::model::User;
use crate::ui::card::apply_selection;
use crate::ui::feed::{render_ctx, rows};
use crate::ui::notifications::{filter_rows, rows as notification_rows};
use crate::ui::profile::display_name;
use crate::ui::theme;
use crate::ui::thread::rows as thread_rows;
use crate::ui::{connections, profile, settings};

pub fn tab_label(tab: LogTab) -> &'static str {
    match tab {
        LogTab::Following => "Following",
        LogTab::Latest => "Latest",
        LogTab::Trending => "Trending",
    }
}

fn title(app: &App) -> String {
    match app.screen() {
        Screen::Thread(_) => "← Commit".to_string(),
        Screen::Profile(name) => profile_title(app, &name),
        Screen::Connections(name, _) => format!("@{name}"),
        other => screen_name(&other).to_string(),
    }
}

/// Counts are known only for a loaded user; a blocked user comes without them.
fn profile_title(app: &App, name: &str) -> String {
    let Some(state) = app.profile(name) else {
        return format!("@{name}");
    };
    let display = |u: &User| display_name(u).to_string();
    if state.error.is_some() {
        return state
            .blocked_user
            .as_ref()
            .map_or_else(|| format!("@{name}"), display);
    }
    match state.user.as_ref().map(|r| &r.data) {
        Some(user) => match user.commits_count {
            Some(n) => format!("{} · {n} commits", display(user)),
            None => display(user),
        },
        None => format!("@{name}"),
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
        Screen::Connections(..) => "Connections",
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
        .title(format!(" {} ", title(app)));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    match app.screen() {
        Screen::Log(tab) => render_log(app, frame, inner, tab),
        Screen::Saved => draw_list(app, frame, inner, FeedKey::Saved),
        Screen::Notifications => draw_notifications(app, frame, inner),
        Screen::Thread(id) => draw_thread(app, frame, inner, &id),
        Screen::Profile(name) => draw_profile(app, frame, inner, &name),
        Screen::Connections(name, tab) => draw_connections(app, frame, inner, &name, tab),
        Screen::Settings => draw_settings(app, frame, inner),
    }
}

fn render_log(app: &App, frame: &mut Frame, inner: Rect, selected: LogTab) {
    let screen_rows: Vec<Rect> = inner.rows().collect();
    let labels = [LogTab::Following, LogTab::Latest, LogTab::Trending].map(tab_label);
    let active = [LogTab::Following, LogTab::Latest, LogTab::Trending]
        .iter()
        .position(|t| *t == selected)
        .unwrap_or(0);
    draw_three_tabs(frame, inner, labels, active);

    let list = Rect {
        y: inner.y + 2,
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

/// Labels centered in thirds of the first row, with a line under the active one.
fn draw_three_tabs(frame: &mut Frame, inner: Rect, labels: [&str; 3], active: usize) {
    let screen_rows: Vec<Rect> = inner.rows().collect();
    let Some(tab_row) = screen_rows.first() else {
        return;
    };
    let cells = thirds(*tab_row);
    for (i, (cell, label)) in cells.iter().zip(labels).enumerate() {
        let is_selected = i == active;
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
}

/// The tab row, its underline and a blank row stay put; the settings below them do not scroll.
fn draw_settings(app: &App, frame: &mut Frame, inner: Rect) {
    let labels = ["Profile", "Appearance", "Safety"];
    let active = match app.settings().tab {
        SettingsTab::Profile => 0,
        SettingsTab::Appearance => 1,
        SettingsTab::Safety => 2,
    };
    draw_three_tabs(frame, inner, labels, active);
    let body = Rect {
        y: inner.y + 3,
        height: inner.height.saturating_sub(3),
        ..inner
    };
    for (row, line) in body.rows().zip(settings::rows(app, body.width)) {
        frame.render_widget(Paragraph::new(line), row);
    }
}

/// The filter rows stay put; only the list below them scrolls.
fn draw_notifications(app: &App, frame: &mut Frame, inner: Rect) {
    let all: Vec<Rect> = inner.rows().collect();
    let fixed = filter_rows(&app.notifications().filters, inner.width);
    for (row, line) in all.iter().zip(&fixed) {
        frame.render_widget(Paragraph::new(line.clone()), *row);
    }
    let scroll = usize::from(app.notifications().scroll);
    let view = notification_rows(app, inner.width);
    let mut visible: Vec<Line<'static>> = view.lines.into_iter().skip(scroll).collect();
    // Marked here, not in rows(), because the row tests compare unselected text.
    if let Some(card) = view.cards.get(app.notifications().selected) {
        let from = card.start.saturating_sub(scroll).min(visible.len());
        let to = (card.start + card.height)
            .saturating_sub(scroll)
            .min(visible.len());
        let width = inner.width.saturating_sub(1);
        apply_selection(&mut visible[from..to], &render_ctx(app), width);
    }
    for (row, line) in all.iter().skip(fixed.len()).zip(visible) {
        frame.render_widget(Paragraph::new(line), *row);
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

fn draw_profile(app: &App, frame: &mut Frame, area: Rect, name: &str) {
    let scroll = app.profile(name).map_or(0, |s| usize::from(s.scroll()));
    let visible = profile::rows(app, name, area.width)
        .lines
        .into_iter()
        .skip(scroll)
        .take(usize::from(area.height));
    for (row, line) in area.rows().zip(visible) {
        frame.render_widget(Paragraph::new(line), row);
    }
}

fn draw_connections(app: &App, frame: &mut Frame, inner: Rect, name: &str, tab: ConnTab) {
    let active = usize::from(tab == ConnTab::Following);
    let [labels, underline] =
        profile::two_tabs(["Followers", "Following"], active, usize::from(inner.width));
    let mut screen_rows = inner.rows();
    if let Some(row) = screen_rows.next() {
        frame.render_widget(Paragraph::new(labels), row);
    }
    if let Some(row) = screen_rows.next() {
        frame.render_widget(Paragraph::new(underline).style(theme::current_style()), row);
    }
    let list = Rect {
        y: inner.y + 3,
        height: inner.height.saturating_sub(3),
        ..inner
    };
    let scroll = app
        .connections(name, tab)
        .map_or(0, |s| usize::from(s.scroll));
    let visible = connections::rows(app, name, tab, list.width)
        .lines
        .into_iter()
        .skip(scroll)
        .take(usize::from(list.height));
    for (row, line) in list.rows().zip(visible) {
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
