use ratatui::Frame;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};
use unicode_width::UnicodeWidthStr;

use crate::dialogs::{DETAILS_MAX, DeleteConfirm, MoreMenu, REASONS, ReportFocus, ReportState};
use crate::editor::wrap;
use crate::ui::card::clean_inline;
use crate::ui::centered;
use crate::ui::compose::cut_with_ellipsis;
use crate::ui::theme;

const WIDTH: u16 = 44;
/// Columns between the borders.
const INNER: usize = 42;
/// Columns of text in the details field.
const FIELD_TEXT: usize = 36;
const FIELD_ROWS_MAX: usize = 4;
/// Rows of the report dialog besides those of the details field.
const REPORT_ROWS: usize = 13;
/// Where the cursor sits among the rows: the details field starts at this inner row.
const FIELD_TOP: u16 = 8;

fn pad(text: &str, width: usize) -> String {
    let fill = width.saturating_sub(text.width());
    format!("{text}{}", " ".repeat(fill))
}

fn frame_of(frame: &mut Frame, rect: ratatui::layout::Rect, title: String, rows: Vec<String>) {
    frame.render_widget(Clear, rect);
    let block = Block::bordered()
        .border_type(BorderType::Double)
        .title(title);
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    let lines: Vec<Line> = rows.into_iter().map(Line::raw).collect();
    frame.render_widget(Paragraph::new(lines), inner);
}

pub fn render_confirm(confirm: &DeleteConfirm, frame: &mut Frame) {
    let hash = clean_inline(&confirm.short_hash);
    let rows = vec![
        String::new(),
        format!(" Delete {hash}?"),
        " Replies will stay as a tombstone.".to_string(),
        String::new(),
        "            [y] Delete      [n] Cancel".to_string(),
        String::new(),
    ];
    let rect = centered(frame.area(), WIDTH, 8);
    frame_of(frame, rect, " Revert commit ".to_string(), rows);
}

const BLOCK_WIDTH: u16 = 58;

/// Pads on the left so that the text sits in the middle of `width` columns.
fn centered_text(text: &str, width: usize) -> String {
    format!(
        "{}{text}",
        " ".repeat(width.saturating_sub(text.width()) / 2)
    )
}

pub fn render_block_confirm(username: &str, frame: &mut Frame) {
    let name = clean_inline(username);
    let rows = vec![
        String::new(),
        format!(" Block @{name}?"),
        " They won't be able to follow you or see your commits.".to_string(),
        String::new(),
        centered_text("[y] Block  [n] Cancel", usize::from(BLOCK_WIDTH) - 2),
        String::new(),
    ];
    let rect = centered(frame.area(), BLOCK_WIDTH, 8);
    frame_of(frame, rect, " Block user ".to_string(), rows);
}

pub fn render_report(report: &ReportState, frame: &mut Frame) {
    let (text_rows, cursor, top) = field_rows(report);
    let shown: Vec<String> = text_rows
        .iter()
        .skip(top)
        .take(FIELD_ROWS_MAX)
        .cloned()
        .collect();
    let field = shown.len().max(1);
    let hash = clean_inline(&report.short_hash);

    let mut rows = vec![String::new()];
    for reason in REASONS {
        let chosen = reason == report.reason;
        let mark = if chosen && report.focus == ReportFocus::Reason {
            '▌'
        } else {
            ' '
        };
        let dot = if chosen { "(•)" } else { "( )" };
        rows.push(format!(" {mark}{dot} {}", reason.as_str()));
    }
    rows.push(String::new());
    rows.push(" details (optional, 500 chars)".to_string());
    rows.push(format!(" ┌{}┐", "─".repeat(INNER - 4)));
    for k in 0..field {
        let line = shown.get(k).map_or("", String::as_str);
        rows.push(format!(" │ {} │", pad(line, FIELD_TEXT)));
    }
    rows.push(format!(" └{}┘", "─".repeat(INNER - 4)));
    let counter = format!("{}/{DETAILS_MAX}", report.details.text().chars().count());
    let room = INNER - 3 - counter.width();
    let left = report
        .error
        .as_deref()
        .map(|e| cut_with_ellipsis(&format!("✗ {}", clean_inline(e)), room))
        .unwrap_or_default();
    rows.push(format!(
        " {}{counter} ",
        pad(&left, INNER - 2 - counter.width())
    ));
    let keys = if report.sending {
        "Esc cancel   sending…"
    } else {
        "Esc cancel   Enter submit"
    };
    rows.push(format!("           {keys}"));

    let height = u16::try_from(REPORT_ROWS + field).unwrap_or(u16::MAX);
    let rect = centered(frame.area(), WIDTH, height);
    frame_of(frame, rect, format!(" Report {hash} "), rows);
    if report.focus == ReportFocus::Details {
        let x = rect.x + 4 + u16::try_from(cursor.1).unwrap_or(0);
        let y = rect.y + 1 + FIELD_TOP + u16::try_from(cursor.0.saturating_sub(top)).unwrap_or(0);
        if x < rect.x + rect.width && y < rect.y + rect.height {
            frame.set_cursor_position((x, y));
        }
    }
}

/// The wrapped rows of the details, where the cursor is in them, and the first row shown.
fn field_rows(report: &ReportState) -> (Vec<String>, (usize, usize), usize) {
    let wrapped = wrap(report.details.lines(), report.details.cursor(), FIELD_TEXT);
    let top = wrapped.cursor.0.saturating_sub(FIELD_ROWS_MAX - 1);
    let rows = wrapped.rows.into_iter().map(|r| r.text).collect();
    (rows, wrapped.cursor, top)
}

const MENU_WIDTH: u16 = 26;
/// Columns between the borders.
const MENU_INNER: usize = 24;

pub fn render_menu(menu: &MoreMenu, truecolor: bool, frame: &mut Frame) {
    let height = u16::try_from(menu.items.len() + 2).unwrap_or(u16::MAX);
    let rect = centered(frame.area(), MENU_WIDTH, height);
    frame.render_widget(Clear, rect);
    let hash = clean_inline(&menu.short_hash);
    let block = Block::bordered()
        .border_type(BorderType::Double)
        .title(format!(" {hash} "));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    let chosen = Style::new().bg(theme::cursor_bg(truecolor));
    let lines: Vec<Line> = menu
        .items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let on = i == menu.selected;
            let mark = if on { '▌' } else { ' ' };
            let text = pad(&format!(" {mark}{}  {}", item.key, item.label), MENU_INNER);
            if on {
                Line::styled(text, chosen)
            } else {
                Line::raw(text)
            }
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), inner);
}
