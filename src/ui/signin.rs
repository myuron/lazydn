use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::app::{App, KeyStatus, SignInStep};
use crate::text::is_unsafe_char;
use crate::ui::centered;

const MAX_WIDTH: u16 = 100;
const LABEL_WIDTH: usize = 13;
const LABELS: [&str; 3] = [" 1. Key      ", " 2. Approve  ", " 3. Code     "];
const CURSOR: &str = "▏";

/// Cuts at `width` columns with no regard for words, since URLs and paths have none.
fn hard_wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = vec![String::new()];
    let mut used = 0;
    for c in text.chars() {
        let w = c.width().unwrap_or(0);
        if used + w > width.max(1) && used > 0 {
            lines.push(String::new());
            used = 0;
        }
        if let Some(line) = lines.last_mut() {
            line.push(c);
        }
        used += w;
    }
    lines
}

fn word_wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for word in text.split(' ') {
        match lines.last_mut() {
            Some(line) if line.width() + 1 + word.width() <= width => {
                line.push(' ');
                line.push_str(word);
            }
            _ => lines.push(word.to_string()),
        }
    }
    lines
}

/// The first characters that fit in `width` columns.
fn head(text: &str, width: usize) -> String {
    let mut used = 0;
    text.chars()
        .take_while(|c| {
            used += c.width().unwrap_or(0);
            used <= width
        })
        .collect()
}

/// Keeps the end of the text, where the user is typing.
fn tail(text: &str, width: usize) -> String {
    if text.width() <= width {
        return text.to_string();
    }
    let mut kept = Vec::new();
    let mut used = 1;
    for c in text.chars().rev() {
        let w = c.width().unwrap_or(0);
        if used + w > width {
            break;
        }
        used += w;
        kept.push(c);
    }
    kept.reverse();
    format!("…{}", kept.into_iter().collect::<String>())
}

/// A line of the dialog body: a label column, then text; `dim` marks hints.
struct Row {
    text: String,
    dim: bool,
}

struct Body {
    rows: Vec<Row>,
}

impl Body {
    fn push(&mut self, label: &str, texts: Vec<String>, dim: bool) {
        for (i, text) in texts.into_iter().enumerate() {
            let prefix = if i == 0 {
                label.to_string()
            } else {
                " ".repeat(LABEL_WIDTH)
            };
            self.rows.push(Row {
                text: format!("{prefix}{text}"),
                dim,
            });
        }
    }

    fn cont(&mut self, texts: Vec<String>, dim: bool) {
        self.push(&" ".repeat(LABEL_WIDTH), texts, dim);
    }
}

fn key_section(app: &App, body: &mut Body, width: usize) {
    let state = app.sign_in();
    if let Some(problem) = app.key_problem() {
        let mut lines = word_wrap(&format!("✗ {problem}"), width);
        lines.push("Ctrl+R read the key again".into());
        body.push(LABELS[0], lines, false);
        return;
    }
    let path: String = app
        .key_path()
        .display()
        .to_string()
        .chars()
        .filter(|c| !is_unsafe_char(*c))
        .collect();
    let status = match app.key_status() {
        KeyStatus::Absent => "press Enter to create",
        _ if state.preparing => "preparing…",
        _ if state.created => "✓ created",
        _ => "✓ ready",
    };
    let one_line = format!("{path}  {status}");
    let mut lines = if one_line.width() <= width {
        vec![one_line]
    } else {
        let mut lines = hard_wrap(&path, width);
        lines.push(status.into());
        lines
    };
    if let Some(fingerprint) = &state.fingerprint {
        lines.extend(hard_wrap(fingerprint, width));
    }
    body.push(LABELS[0], lines, false);
}

/// `url_rows` is how many rows the URL may use; it is cut only when the screen is too low for all of it.
fn approve_section(app: &App, body: &mut Body, width: usize, url_rows: usize) {
    let state = app.sign_in();
    let Some(url) = &state.url else {
        let head = match app.key_status() {
            KeyStatus::Present if state.preparing => "preparing…",
            KeyStatus::Present => "(not ready)",
            KeyStatus::Absent | KeyStatus::Unreadable { .. } => "(waiting for the key)",
        };
        body.push(LABELS[1], vec![head.into()], false);
        return;
    };
    body.push(
        LABELS[1],
        vec!["open this URL in your browser:".into()],
        false,
    );
    let mut lines = hard_wrap(url, width);
    if lines.len() > url_rows {
        lines.truncate(url_rows.max(1));
        let last = lines.pop().unwrap_or_default();
        lines.push(format!("{}…", head(&last, width.saturating_sub(1))));
    }
    body.cont(lines, false);
    body.cont(vec!["Ctrl+O open browser   Ctrl+Y copy URL".into()], true);
}

fn code_section(app: &App, body: &mut Body, width: usize) {
    let state = app.sign_in();
    let accepting = state.url.is_some() && state.step != SignInStep::Submitting;
    let field = if accepting {
        format!("{}{CURSOR}", tail(&state.code, width.saturating_sub(1)))
    } else {
        tail(&state.code, width)
    };
    body.push(LABELS[2], vec![field], false);
    body.cont(vec!["paste the devnull:// URL or the code".into()], true);
}

fn footer(app: &App, body: &mut Body, width: usize) {
    let state = app.sign_in();
    if state.step == SignInStep::Submitting {
        body.cont(vec!["Signing in…".into()], false);
    } else if let Some(error) = &state.error {
        body.cont(word_wrap(&format!("✗ {error}"), width), false);
    } else {
        body.rows.push(Row {
            text: String::new(),
            dim: false,
        });
    }
    let keys = match app.key_status() {
        KeyStatus::Absent => "Enter create key    Esc continue as guest",
        KeyStatus::Unreadable { .. } => "Enter retry    Esc continue as guest",
        KeyStatus::Present if state.url.is_some() && state.step != SignInStep::Submitting => {
            "Enter submit    Esc continue as guest"
        }
        KeyStatus::Present if !state.preparing && state.url.is_none() => {
            "Enter retry    Esc continue as guest"
        }
        KeyStatus::Present => "Esc continue as guest",
    };
    body.cont(vec![keys.into()], true);
}

fn build(app: &App, width: usize, url_rows: usize) -> Body {
    let mut body = Body {
        rows: vec![Row {
            text: String::new(),
            dim: false,
        }],
    };
    key_section(app, &mut body, width);
    approve_section(app, &mut body, width, url_rows);
    code_section(app, &mut body, width);
    footer(app, &mut body, width);
    body
}

pub fn render(app: &App, frame: &mut Frame, area: Rect) {
    let outer = usize::from(area.width.min(MAX_WIDTH));
    let width = outer.saturating_sub(2 + LABEL_WIDTH + 1);
    let mut body = build(app, width, usize::MAX);
    let rows_wanted = body.rows.len() + 2;
    let screen_rows = usize::from(area.height);
    if rows_wanted > screen_rows
        && let Some(url) = &app.sign_in().url
    {
        // Only the URL gives way, since it is the one part that wraps to any length.
        let url_rows = hard_wrap(url, width).len();
        body = build(
            app,
            width,
            url_rows.saturating_sub(rows_wanted - screen_rows),
        );
    }

    let height = u16::try_from(body.rows.len() + 2).unwrap_or(u16::MAX);
    let rect = centered(area, MAX_WIDTH, height);
    frame.render_widget(Clear, rect);
    let block = Block::bordered()
        .border_type(BorderType::Double)
        .title(" Sign in to /dev/null ");
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    let lines: Vec<Line> = body
        .rows
        .into_iter()
        .map(|row| {
            let style = if row.dim {
                Style::new().add_modifier(Modifier::DIM)
            } else {
                Style::new()
            };
            Line::styled(row.text, style)
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), inner);
}
