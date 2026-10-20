//! Drawing of the composer: the dialog, and the field inside the Log list.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::app::{App, Screen};
use crate::body::body_len;
use crate::compose::{Compose, Counter, CounterTone, Surface, counter};
use crate::drafts::DraftKey;
use crate::editor::wrap;
use crate::layout::LayoutMode;
use crate::ui::card::{LIST_LEAD, clean_inline, quoted_card};
use crate::ui::centered;
use crate::ui::feed::{render_ctx, truncate};
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

/// The frame of a reply box takes two columns and the spaces inside it two more.
const BOX_PADDING: usize = 4;
/// Most text rows a reply box shows.
const BOX_MAX_ROWS: usize = 8;

/// Columns of the card under which a box sits, from the left edge of the list to the card's right edge.
fn card_width(app: &App) -> usize {
    usize::from(main_area(app).width.saturating_sub(2)).saturating_sub(1)
}

/// Columns the text of a composer on `surface` may take before it wraps.
pub fn compose_text_width(app: &App, surface: Surface) -> usize {
    match surface {
        Surface::Inline => usize::from(main_area(app).width.saturating_sub(2)).saturating_sub(5),
        Surface::ReplyBox => {
            let lead = match app.screen() {
                Screen::Thread(id) => crate::ui::thread::box_lead_width(app, &id),
                _ => None,
            };
            card_width(app)
                .saturating_sub(lead.unwrap_or(LIST_LEAD))
                .saturating_sub(BOX_PADDING)
        }
        Surface::Modal => usize::from(dialog_width(app)).saturating_sub(7),
    }
}

/// What a dialog draws besides the text, shrunk to the screen: the text field gives way first, then the body of the
/// quoted card, then the card, and last the row with the keys. Only a quote has a card to draw; the dialog of a new
/// commit gives up nothing but its keys row, on a screen too low for it.
struct ModalPlan {
    frame: Vec<Line<'static>>,
    keys_row: bool,
}

fn modal_plan(app: &App, compose: &Compose) -> ModalPlan {
    let allowed = usize::from(app.size().1).saturating_sub(4);
    let quoted = compose
        .target
        .as_ref()
        .filter(|_| matches!(compose.key, DraftKey::Quote(_)));
    // A border pair, the blank row, one text row and the two rows under the text.
    let fixed = 2 + 1 + 1 + FOOTER_ROWS;
    if let Some(target) = quoted {
        let cw = usize::from(dialog_width(app)).saturating_sub(4);
        let ctx = render_ctx(app);
        for body_rows in [3, 1] {
            let frame = quoted_card(target, &ctx, cw, body_rows);
            if fixed + frame.len() <= allowed {
                return ModalPlan {
                    frame,
                    keys_row: true,
                };
            }
        }
    }
    ModalPlan {
        frame: Vec::new(),
        keys_row: fixed <= allowed,
    }
}

/// Most text rows the field may show on this screen.
fn max_shown(app: &App, compose: &Compose, plan: Option<&ModalPlan>) -> usize {
    match compose.surface {
        Surface::ReplyBox => {
            let tabs = if matches!(app.screen(), Screen::Log(_)) {
                2
            } else {
                0
            };
            let view = usize::from(main_area(app).height.saturating_sub(2 + tabs));
            // A frame row above, the row with the keys and a frame row below.
            view.saturating_sub(3).clamp(1, BOX_MAX_ROWS)
        }
        Surface::Inline => {
            // Under the main pane's border and the two rows of tabs.
            let list = usize::from(main_area(app).height.saturating_sub(4));
            list.saturating_sub(1 + FOOTER_ROWS)
                .clamp(1, INLINE_MAX_ROWS)
        }
        Surface::Modal => {
            let own;
            let plan = match plan {
                Some(plan) => plan,
                None => {
                    own = modal_plan(app, compose);
                    &own
                }
            };
            let footer = if plan.keys_row { FOOTER_ROWS } else { 1 };
            usize::from(app.size().1)
                .saturating_sub(4 + 3 + footer + plan.frame.len())
                .max(1)
        }
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
    metrics_at(app, compose, compose_text_width(app, compose.surface), None)
}

/// Like `metrics` for text that wraps at `text_width`, and a dialog laid out as `plan` says when that is known.
fn metrics_at(
    app: &App,
    compose: &Compose,
    text_width: usize,
    plan: Option<&ModalPlan>,
) -> Metrics {
    static EMPTY: [String; 1] = [String::new()];
    let (lines, cursor) = match app.drafts().get(&compose.key) {
        Some(editor) => (editor.lines(), editor.cursor()),
        None => (&EMPTY[..], (0, 0)),
    };
    let wrapped = wrap(lines, cursor, text_width);
    let cap = max_shown(app, compose, plan);
    let floor = if compose.surface == Surface::ReplyBox {
        1
    } else {
        cap.min(3)
    };
    let shown = wrapped.rows.len().clamp(floor, cap);
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

/// The keys hint at the right of the last row of a composer.
fn keys_text(app: &App, compose: &Compose) -> String {
    if compose.submitting {
        "Esc cancel  Committing…".to_string()
    } else {
        format!(
            "Esc cancel  {} {}",
            app.keyboard().submit_hint(),
            action_word(&compose.key)
        )
    }
}

fn keys_row(app: &App, compose: &Compose, field: usize, rail: bool) -> Line<'static> {
    let keys = keys_text(app, compose);
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

fn action_word(key: &DraftKey) -> &'static str {
    match key {
        DraftKey::New => "Commit",
        DraftKey::Reply(_) => "Reply",
        DraftKey::Quote(_) => "Quote",
    }
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
    let plan = modal_plan(app, compose);
    let m = metrics_at(
        app,
        compose,
        compose_text_width(app, Surface::Modal),
        Some(&plan),
    );
    let top = drawn_top(&m, compose);
    let footer = if plan.keys_row { FOOTER_ROWS } else { 1 };
    let height = u16::try_from(m.shown + footer + plan.frame.len() + 3).unwrap_or(u16::MAX);
    let rect = centered(area, dialog_width(app), height);
    frame.render_widget(Clear, rect);
    let title = match (&compose.key, &compose.target) {
        (DraftKey::Quote(_), Some(target)) => {
            format!(" Quote @{} ", clean_inline(&target.author.username))
        }
        _ => TITLE.to_string(),
    };
    let block = Block::bordered()
        .border_type(BorderType::Double)
        .title(title);
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    let field = usize::from(inner.width);
    let mut lines = vec![Line::raw("")];
    lines.extend(text_rows(&m, top));
    for line in plan.frame {
        let mut spans = vec![Span::raw(" ")];
        spans.extend(line.spans);
        lines.push(Line::from(spans));
    }
    lines.push(error_row(app, compose, field, false));
    if plan.keys_row {
        lines.push(keys_row(app, compose, field, false));
    }
    frame.render_widget(Paragraph::new(lines), inner);
    let x = inner.x + 4 + u16::try_from(m.cursor.1).unwrap_or(0);
    let y = inner.y + 1 + u16::try_from(m.cursor.0.saturating_sub(top)).unwrap_or(0);
    if x < inner.x + inner.width && y < inner.y + inner.height {
        frame.set_cursor_position((x, y));
    }
}

/// Shortens `text` to `width` columns, ending in `…` when something was cut.
fn cut_with_ellipsis(text: &str, width: usize) -> String {
    if text.width() <= width {
        return text.to_string();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in text.chars() {
        let w = c.width().unwrap_or(0);
        if used + w + 1 > width {
            break;
        }
        used += w;
        out.push(c);
    }
    if width > 0 {
        out.push('…');
    }
    out
}

/// The double-framed box of a reply, drawn to the right of `lead` (the columns of the card it sits under), and where
/// the cursor goes in it (row of the returned lines, column from the left edge of the list).
pub fn reply_box(
    app: &App,
    compose: &Compose,
    lead: &[Span<'static>],
    card_w: usize,
) -> (Vec<Line<'static>>, (usize, usize)) {
    let lead_w: usize = lead.iter().map(|s| s.content.width()).sum();
    let box_w = card_w.saturating_sub(lead_w);
    let text_w = box_w.saturating_sub(BOX_PADDING);
    let m = metrics_at(app, compose, text_w, None);
    let top = drawn_top(&m, compose);
    let line = |body: Vec<Span<'static>>| {
        let mut spans = lead.to_vec();
        spans.extend(body);
        Line::from(spans)
    };

    let handle = compose
        .target
        .as_ref()
        .map(|c| clean_inline(&c.author.username))
        .unwrap_or_default();
    let title = format!(" Reply to @{handle} ");
    let fill = "═".repeat(box_w.saturating_sub(2 + title.width()));
    let mut lines = vec![line(vec![Span::raw(format!("╔{title}{fill}╗"))])];
    for k in 0..m.shown {
        let row = m.rows.get(top + k).map_or("", String::as_str);
        lines.push(line(vec![Span::raw(format!(
            "║ {} ║",
            pad_to(row, text_w)
        ))]));
    }

    let keys = keys_text(app, compose);
    let counter = counter(body_len(&body_text(app, compose)));
    let counter_w = counter
        .as_ref()
        .map_or(0, |c| c.remaining.to_string().width() + 2);
    let right_w = counter_w + keys.width();
    // The keys keep their place, then the counter; the error gets what is left.
    let room = text_w.saturating_sub(right_w + 1);
    let error = compose
        .error
        .as_deref()
        .map(|e| cut_with_ellipsis(&format!("✗ {e}"), room))
        .unwrap_or_default();
    let mut footer = vec![
        Span::raw("║ "),
        Span::raw(pad_to(&error, text_w.saturating_sub(right_w))),
    ];
    if let Some(c) = &counter {
        footer.push(counter_span(c));
        footer.push(Span::raw("  "));
    }
    footer.push(Span::raw(keys));
    footer.push(Span::raw(" ║"));
    lines.push(line(footer));
    lines.push(line(vec![Span::raw(format!(
        "╚{}╝",
        "═".repeat(box_w.saturating_sub(2))
    ))]));

    let row = 1 + m.cursor.0.saturating_sub(top);
    (lines, (row, lead_w + 2 + m.cursor.1))
}
