use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::text::is_unsafe_char;

/// Most characters an editor holds, before the server's normalization; twice the server's limit.
pub const MAX_CHARS: usize = 8000;

/// Spaces a Tab inserts.
pub const TAB_WIDTH: usize = 4;

/// A multi-line text buffer whose cursor moves by grapheme.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Editor {
    lines: Vec<String>,
    row: usize,
    col: usize,
}

impl Default for Editor {
    fn default() -> Self {
        Self {
            lines: vec![String::new()],
            row: 0,
            col: 0,
        }
    }
}

/// One screen row of wrapped text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrapRow {
    pub text: String,
}

/// Wrapped text and where the cursor lands in it; `col` counts terminal cells.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wrapped {
    pub rows: Vec<WrapRow>,
    pub cursor: (usize, usize),
}

/// The graphemes `start..end` of a line that share one screen row, and the cells they take.
struct Span {
    start: usize,
    end: usize,
    cells: usize,
}

fn glen(line: &str) -> usize {
    line.graphemes(true).count()
}

fn byte_at(line: &str, col: usize) -> usize {
    line.grapheme_indices(true)
        .nth(col)
        .map_or(line.len(), |(i, _)| i)
}

fn col_at(line: &str, byte: usize) -> usize {
    glen(&line[..byte])
}

fn layout(line: &str, width: usize) -> Vec<Span> {
    let width = width.max(1);
    let mut spans = vec![Span {
        start: 0,
        end: 0,
        cells: 0,
    }];
    for (i, g) in line.graphemes(true).enumerate() {
        let w = g.width();
        let last = spans.last_mut().expect("one span at least");
        if last.cells > 0 && last.cells + w > width {
            spans.push(Span {
                start: i,
                end: i + 1,
                cells: w,
            });
        } else {
            last.end = i + 1;
            last.cells += w;
        }
    }
    spans
}

/// The row within the line that holds the cursor; one past the last when it follows a full row.
fn place(spans: &[Span], col: usize, width: usize) -> usize {
    if let Some(r) = spans.iter().position(|s| s.start <= col && col < s.end) {
        return r;
    }
    let last = spans.len() - 1;
    if spans[last].cells >= width.max(1) {
        spans.len()
    } else {
        last
    }
}

fn cell_col(line: &str, spans: &[Span], r: usize, col: usize) -> usize {
    line.graphemes(true)
        .skip(spans[r].start)
        .take(col - spans[r].start)
        .map(|g| g.width())
        .sum()
}

/// Breaks `lines` into rows of at most `width` cells, by grapheme; a grapheme that does not fit moves to the
/// next row whole. `cursor` is (line, grapheme index). A cursor that follows a full row sits at column 0 of an
/// extra empty row.
pub fn wrap(lines: &[String], cursor: (usize, usize), width: usize) -> Wrapped {
    let mut rows = Vec::new();
    let mut at = (0, 0);
    for (n, line) in lines.iter().enumerate() {
        let spans = layout(line, width);
        if n == cursor.0 {
            let r = place(&spans, cursor.1, width);
            let cell = if r < spans.len() && cursor.1 < spans[r].end {
                cell_col(line, &spans, r, cursor.1)
            } else if r < spans.len() {
                spans[r].cells
            } else {
                0
            };
            at = (rows.len() + r, cell);
        }
        let graphemes: Vec<&str> = line.graphemes(true).collect();
        for span in &spans {
            rows.push(WrapRow {
                text: graphemes[span.start..span.end].concat(),
            });
        }
        if n == cursor.0 && at.0 == rows.len() {
            rows.push(WrapRow {
                text: String::new(),
            });
        }
    }
    Wrapped { rows, cursor: at }
}

impl Editor {
    pub fn new() -> Self {
        Self::default()
    }

    /// An editor holding `text`, with the cursor at its end.
    pub fn from_text(text: &str) -> Self {
        let lines: Vec<String> = text.split('\n').map(str::to_string).collect();
        let row = lines.len() - 1;
        let col = glen(&lines[row]);
        Self { lines, row, col }
    }

    /// An editor holding `text` with the cursor at (line, grapheme index).
    pub fn from_text_at(text: &str, row: usize, col: usize) -> Self {
        let mut editor = Self::from_text(text);
        editor.row = row.min(editor.lines.len() - 1);
        editor.col = col.min(glen(&editor.lines[editor.row]));
        editor
    }

    /// The lines joined by `\n`.
    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    /// (line, grapheme index).
    pub fn cursor(&self) -> (usize, usize) {
        (self.row, self.col)
    }

    pub fn is_empty(&self) -> bool {
        self.lines.len() == 1 && self.lines[0].is_empty()
    }

    /// Characters held, newlines included.
    pub fn char_count(&self) -> usize {
        self.lines.iter().map(|l| l.chars().count()).sum::<usize>() + self.lines.len() - 1
    }

    fn room(&self) -> usize {
        MAX_CHARS.saturating_sub(self.char_count())
    }

    fn insert_str(&mut self, s: &str) {
        let line = &mut self.lines[self.row];
        let at = byte_at(line, self.col);
        line.insert_str(at, s);
        self.col = col_at(line, at + s.len());
    }

    /// Returns false when the character is dropped: unsafe, or the buffer is full.
    pub fn insert_char(&mut self, c: char) -> bool {
        if is_unsafe_char(c) || self.room() == 0 {
            return false;
        }
        self.insert_str(c.encode_utf8(&mut [0; 4]));
        true
    }

    pub fn newline(&mut self) {
        if self.room() == 0 {
            return;
        }
        let at = byte_at(&self.lines[self.row], self.col);
        let tail = self.lines[self.row].split_off(at);
        self.row += 1;
        self.col = 0;
        self.lines.insert(self.row, tail);
    }

    pub fn insert_tab(&mut self) {
        if self.room() >= TAB_WIDTH {
            self.insert_str(&" ".repeat(TAB_WIDTH));
        }
    }

    fn remove_graphemes(&mut self, from: usize, to: usize) {
        let line = &mut self.lines[self.row];
        let (a, b) = (byte_at(line, from), byte_at(line, to));
        line.replace_range(a..b, "");
        self.col = from;
    }

    pub fn backspace(&mut self) {
        if self.col > 0 {
            self.remove_graphemes(self.col - 1, self.col);
        } else if self.row > 0 {
            let tail = self.lines.remove(self.row);
            self.row -= 1;
            self.col = glen(&self.lines[self.row]);
            self.lines[self.row].push_str(&tail);
        }
    }

    pub fn delete(&mut self) {
        if self.col < glen(&self.lines[self.row]) {
            self.remove_graphemes(self.col, self.col + 1);
        } else if self.row + 1 < self.lines.len() {
            let tail = self.lines.remove(self.row + 1);
            self.lines[self.row].push_str(&tail);
        }
    }

    pub fn left(&mut self) {
        if self.col > 0 {
            self.col -= 1;
        } else if self.row > 0 {
            self.row -= 1;
            self.col = glen(&self.lines[self.row]);
        }
    }

    pub fn right(&mut self) {
        if self.col < glen(&self.lines[self.row]) {
            self.col += 1;
        } else if self.row + 1 < self.lines.len() {
            self.row += 1;
            self.col = 0;
        }
    }

    /// The grapheme index in span `r` of line `row` whose left edge is nearest to `cell` without passing it.
    fn col_in(&self, row: usize, spans: &[Span], r: usize, cell: usize) -> usize {
        let span = &spans[r];
        let mut cum = 0;
        for (i, g) in self.lines[row]
            .graphemes(true)
            .enumerate()
            .skip(span.start)
            .take(span.end - span.start)
        {
            let w = g.width();
            if cell < cum + w {
                return i;
            }
            cum += w;
        }
        if r + 1 == spans.len() {
            span.end
        } else {
            span.end - 1
        }
    }

    /// Moves one wrapped row up at `width` cells, keeping the cell column where it can.
    pub fn up(&mut self, width: usize) {
        let spans = layout(&self.lines[self.row], width);
        let r = place(&spans, self.col, width);
        let cell = self.cell_now(&spans, r);
        if r > 0 {
            self.col = self.col_in(self.row, &spans, r.min(spans.len()) - 1, cell);
        } else if self.row > 0 {
            self.row -= 1;
            let spans = layout(&self.lines[self.row], width);
            self.col = self.col_in(self.row, &spans, spans.len() - 1, cell);
        }
    }

    pub fn down(&mut self, width: usize) {
        let spans = layout(&self.lines[self.row], width);
        let r = place(&spans, self.col, width);
        let cell = self.cell_now(&spans, r);
        if r + 1 < spans.len() {
            self.col = self.col_in(self.row, &spans, r + 1, cell);
        } else if self.row + 1 < self.lines.len() {
            self.row += 1;
            let spans = layout(&self.lines[self.row], width);
            self.col = self.col_in(self.row, &spans, 0, cell);
        }
    }

    fn cell_now(&self, spans: &[Span], r: usize) -> usize {
        if r < spans.len() {
            cell_col(&self.lines[self.row], spans, r, self.col)
        } else {
            0
        }
    }

    /// To the start of the line, not of the wrapped row.
    pub fn home(&mut self) {
        self.col = 0;
    }

    pub fn end(&mut self) {
        self.col = glen(&self.lines[self.row]);
    }

    /// Ctrl+U: deletes back to the start of the line.
    pub fn kill_to_line_start(&mut self) {
        self.remove_graphemes(0, self.col);
    }

    /// Ctrl+W: deletes the word before the cursor.
    pub fn delete_word_back(&mut self) {
        let graphemes: Vec<&str> = self.lines[self.row].graphemes(true).collect();
        let blank = |g: &&str| g.chars().all(char::is_whitespace);
        let mut i = self.col;
        while i > 0 && blank(&graphemes[i - 1]) {
            i -= 1;
        }
        while i > 0 && !blank(&graphemes[i - 1]) {
            i -= 1;
        }
        self.remove_graphemes(i, self.col);
    }

    /// Inserts pasted text at once; returns true when the 8000-character cap cut it.
    pub fn paste(&mut self, text: &str) -> bool {
        let cleaned: String = text
            .replace("\r\n", "\n")
            .replace('\r', "\n")
            .replace('\t', &" ".repeat(TAB_WIDTH))
            .chars()
            .filter(|&c| c == '\n' || !is_unsafe_char(c))
            .collect();
        let room = self.room();
        let mut kept = String::new();
        let mut used = 0;
        let mut truncated = false;
        for g in cleaned.graphemes(true) {
            let n = g.chars().count();
            if used + n > room {
                truncated = true;
                break;
            }
            used += n;
            kept.push_str(g);
        }
        let mut parts = kept.split('\n');
        let first = parts.next().unwrap_or_default();
        self.insert_str(first);
        for part in parts {
            let at = byte_at(&self.lines[self.row], self.col);
            let tail = self.lines[self.row].split_off(at);
            self.row += 1;
            self.lines.insert(self.row, format!("{part}{tail}"));
            self.col = col_at(&self.lines[self.row], part.len());
        }
        truncated
    }
}
