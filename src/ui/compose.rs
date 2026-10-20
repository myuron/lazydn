//! Drawing of the composer: the dialog, and the field inside the Log list.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};
use unicode_width::UnicodeWidthStr;

use crate::app::App;
use crate::body::body_len;
use crate::compose::{Compose, Counter, CounterTone, Surface, counter};
use crate::drafts::DraftKey;
use crate::editor::wrap;
use crate::layout::LayoutMode;
use crate::ui::centered;
use crate::ui::feed::truncate;
use crate::ui::layout::split;

const DIALOG_MAX_WIDTH: u16 = 64;
const TITLE: &str = " Write a commit ";
const DRAFT_LABEL: &str = "stash@{0} draft saved";
const DRAFT_LABEL_SHORT: &str = "stash@{0}";
/// The most text rows an inline field takes.
const INLINE_MAX_ROWS: usize = 10;
/// The rows of a field that are not text: the one with the error and counter, and the one with the keys.
const FOOTER_ROWS: usize = 2;

fn main_area(app: &App) -> Rect {
    let (w, h) = app.size();
    split(Rect::new(0, 0, w, h), LayoutMode::from_size(w, h)).main
}

fn dialog_width(app: &App) -> u16 {
    app.size().0.saturating_sub(4).min(DIALOG_MAX_WIDTH)
}

/// Columns the text of a composer on `surface` may take before it wraps.
pub fn compose_text_width(app: &App, surface: Surface) -> usize {
    match surface {
        Surface::Inline | Surface::ReplyBox => {
            usize::from(main_area(app).width.saturating_sub(2)).saturating_sub(5)
        }
        Surface::Modal => usize::from(dialog_width(app)).saturating_sub(7),
    }
}

/// Most text rows the field may show on this screen.
fn max_shown(app: &App, surface: Surface) -> usize {
    match surface {
        Surface::Inline | Surface::ReplyBox => {
            // Under the main pane's border and the two rows of tabs.
            let list = usize::from(main_area(app).height.saturating_sub(4));
            list.saturating_sub(1 + FOOTER_ROWS)
                .clamp(1, INLINE_MAX_ROWS)
        }
        Surface::Modal => usize::from(app.size().1).saturating_sub(9).max(1),
    }
}

/// How the text of an open composer lies on the screen.
pub struct Metrics {
    /// Wrapped rows of the whole text.
    pub rows: Vec<String>,
    /// Text rows the field shows.
    pub shown: usize,
    /// Wrapped row and column of the cursor.
    pub cursor: (usize, usize),
}

pub fn metrics(app: &App, compose: &Compose) -> Metrics {
    static EMPTY: [String; 1] = [String::new()];
    let (lines, cursor) = match app.drafts().get(&compose.key) {
        Some(editor) => (editor.lines(), editor.cursor()),
        None => (&EMPTY[..], (0, 0)),
    };
    let wrapped = wrap(lines, cursor, compose_text_width(app, compose.surface));
    let cap = max_shown(app, compose.surface);
    let shown = wrapped.rows.len().clamp(cap.min(3), cap);
    Metrics {
        rows: wrapped.rows.into_iter().map(|r| r.text).collect(),
        shown,
        cursor: wrapped.cursor,
    }
}

/// The first wrapped row to show: the current one moved just far enough to keep the cursor in view.
pub fn view_top_for(app: &App, compose: &Compose) -> usize {
    let m = metrics(app, compose);
    let mut top = compose.view_top;
    if m.cursor.0 < top {
        top = m.cursor.0;
    } else if m.cursor.0 >= top + m.shown {
        top = m.cursor.0 + 1 - m.shown;
    }
    top.min(m.rows.len().saturating_sub(m.shown))
}

/// The first wrapped row drawn: what `App` settled on, kept within the text.
fn drawn_top(m: &Metrics, compose: &Compose) -> usize {
    compose.view_top.min(m.rows.len().saturating_sub(m.shown))
}

/// Rows an inline field takes in the list, or `None` when no inline composer is open.
pub fn inline_height(app: &App) -> Option<usize> {
    let compose = app.compose().filter(|c| c.surface == Surface::Inline)?;
    Some(1 + metrics(app, compose).shown + FOOTER_ROWS)
}

fn pad_to(text: &str, width: usize) -> String {
    format!("{text}{}", " ".repeat(width.saturating_sub(text.width())))
}

fn counter_span(c: &Counter) -> Span<'static> {
    let style = match c.tone {
        CounterTone::Normal => Style::new(),
        CounterTone::Warn => Style::new().fg(Color::Yellow),
        CounterTone::Over => Style::new().fg(Color::Red),
    };
    Span::styled(c.remaining.to_string(), style)
}

fn error_row(app: &App, compose: &Compose, field: usize, rail: bool) -> Line<'static> {
    let col = if rail { 4 } else { 1 };
    let text = compose.error.as_deref().map(|e| format!("✗ {e}"));
    let counter = counter(body_len(&body_text(app, compose)));
    let (start, right) = match &counter {
        Some(c) => {
            let span = counter_span(c);
            (field.saturating_sub(1 + span.content.width()), span)
        }
        None => (field, Span::raw("")),
    };
    // The counter keeps its place, so the error gives up the columns it needs and one to breathe.
    let room = start.saturating_sub(col + 1);
    let left = text.map(|t| truncate(&t, room)).unwrap_or_default();
    two_sided(rail, &left, col, start, right)
}

/// A row with `left` from column `left_col`, padded to `start` where `right` begins; `rail` puts the line of the field in
/// front, as the text rows have it.
fn two_sided(
    rail: bool,
    left: &str,
    left_col: usize,
    start: usize,
    right: Span<'static>,
) -> Line<'static> {
    let head = if rail { " ┆" } else { "" };
    let pad = left_col.saturating_sub(head.width());
    let left = format!("{head}{}{left}", " ".repeat(pad));
    Line::from(vec![Span::raw(pad_to(&left, start)), right])
}

fn keys_row(app: &App, compose: &Compose, field: usize, rail: bool) -> Line<'static> {
    let keys = if compose.submitting {
        "Esc cancel  Committing…".to_string()
    } else {
        format!("Esc cancel  {} Commit", app.keyboard().submit_hint())
    };
    let col = if rail { 4 } else { 1 };
    let start = field.saturating_sub(1 + keys.width());
    let label = if app.drafts().get(&compose.key).is_some() {
        [DRAFT_LABEL, DRAFT_LABEL_SHORT]
            .into_iter()
            .find(|l| col + l.width() < start)
            .unwrap_or("")
    } else {
        ""
    };
    two_sided(rail, label, col, start, Span::raw(keys))
}

fn body_text(app: &App, compose: &Compose) -> String {
    app.drafts().text(&compose.key).unwrap_or_default()
}

/// The text rows of the window the field shows, each with its prefix.
fn text_rows(m: &Metrics, top: usize) -> Vec<Line<'static>> {
    (0..m.shown)
        .map(|k| match m.rows.get(top + k) {
            Some(row) => {
                let mark = if top + k == 0 { " ◌  " } else { " ┆  " };
                Line::raw(format!("{mark}{row}"))
            }
            None => Line::raw(" ┆"),
        })
        .collect()
}

/// The inline field and where its cursor goes in it (row, column), for the list to place.
pub fn inline_field(app: &App, width: usize) -> Option<(Vec<Line<'static>>, (usize, usize))> {
    let compose = app.compose().filter(|c| c.surface == Surface::Inline)?;
    let m = metrics(app, compose);
    let top = drawn_top(&m, compose);
    let mut lines = vec![Line::raw(" ┆")];
    lines.extend(text_rows(&m, top));
    lines.push(error_row(app, compose, width, true));
    lines.push(keys_row(app, compose, width, true));
    let cursor = (1 + m.cursor.0.saturating_sub(top), 4 + m.cursor.1);
    Some((lines, cursor))
}

/// The first line with text in the draft of New, for the closed inline composer.
pub fn draft_preview(app: &App) -> Option<String> {
    let editor = app.drafts().get(&DraftKey::New)?;
    editor
        .lines()
        .iter()
        .find(|line| !line.trim().is_empty())
        .cloned()
}

/// Draws the dialog of the composer over `area`.
pub fn render_modal(app: &App, frame: &mut Frame, area: Rect) {
    let Some(compose) = app.compose() else {
        return;
    };
    let m = metrics(app, compose);
    let top = drawn_top(&m, compose);
    let height = u16::try_from(m.shown + FOOTER_ROWS + 3).unwrap_or(u16::MAX);
    let rect = centered(area, dialog_width(app), height);
    frame.render_widget(Clear, rect);
    let block = Block::bordered()
        .border_type(BorderType::Double)
        .title(TITLE);
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    let field = usize::from(inner.width);
    let mut lines = vec![Line::raw("")];
    lines.extend(text_rows(&m, top));
    lines.push(error_row(app, compose, field, false));
    lines.push(keys_row(app, compose, field, false));
    frame.render_widget(Paragraph::new(lines), inner);
    let x = inner.x + 4 + u16::try_from(m.cursor.1).unwrap_or(0);
    let y = inner.y + 1 + u16::try_from(m.cursor.0.saturating_sub(top)).unwrap_or(0);
    if x < inner.x + inner.width && y < inner.y + inner.height {
        frame.set_cursor_position((x, y));
    }
}
