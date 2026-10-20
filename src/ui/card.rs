use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use time::{OffsetDateTime, UtcOffset};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use super::theme;
use crate::highlight::{HlClass, highlight};
use crate::model::{
    Commit, LinkPreview, Poll, Quoted, Segment, Status, ThreadItem, Tombstone, User,
};
use crate::timefmt;

#[derive(Debug, Clone)]
pub struct RenderCtx {
    pub now: OffsetDateTime,
    pub local_offset: UtcOffset,
    pub truecolor: bool,
    pub ascii: bool,
    pub pasteis_mode: bool,
    pub reply_preview_limit: usize,
}

#[derive(Debug, Clone, Copy)]
pub enum CardInput<'a> {
    Commit(&'a Commit),
    Tombstone(&'a Tombstone),
    Deleting { short_hash: &'a str },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Variant {
    #[default]
    List,
    ThreadMain,
    /// A nested reply in a thread: no hash, body indented under the node, minimal actions.
    Reply,
    /// The author's follow-up under the thread's main commit: header and body only.
    Continuation,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Node {
    #[default]
    Normal,
    SameAuthor,
}

#[derive(Debug, Clone, Default)]
pub struct CardOptions {
    pub variant: Variant,
    pub node: Node,
    pub thread_pos: Option<(u16, u16)>,
    pub is_own: bool,
    pub replies_expanded: bool,
    pub replying_to: Option<String>,
    /// Shows the "quoted this commit" marker where a reply would show `replying_to`.
    pub quotes_parent: bool,
    /// Draws the rail in the node column of a reply whose children follow below it.
    pub has_children: bool,
    pub has_draft: bool,
}

/// Nesting guides placed between the selection marker and the rail, e.g. thread branches.
///
/// `first` goes on the header line and `rest` on every other line. When the two differ in
/// width the narrower one is padded so the rail column stays aligned; the wider one is
/// subtracted from the width available to the card.
#[derive(Debug, Clone, Default)]
pub struct Prefix {
    pub first: Vec<Span<'static>>,
    pub rest: Vec<Span<'static>>,
}

const FOLD_CHARS: usize = 280;
const FOLD_LINES: usize = 8;
const TAB_AS_SPACES: &str = "    ";
const DELETED_TEXT: &str = "[deleted] This commit was deleted.";
const UNAVAILABLE_TEXT: &str = "[unavailable] This commit isn't available.";

type Row = Vec<Span<'static>>;
type Chars = Vec<(String, Style)>;

// --- text primitives --------------------------------------------------------

/// Drops control and bidi-override characters so server text cannot inject escapes or reorder a line.
fn clean(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\t' => out.push_str(TAB_AS_SPACES),
            '\n' => out.push('\n'),
            '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' => {}
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

/// For fields that must stay on one row; a newline would break the rail.
fn clean_inline(s: &str) -> String {
    clean(s).replace('\n', " ")
}

/// Measured per grapheme cluster so emoji sequences are never split or miscounted.
fn gw(g: &str) -> usize {
    g.width()
}

fn chars_width(cs: &[(String, Style)]) -> usize {
    cs.iter().map(|(g, _)| gw(g)).sum()
}

fn row_width(row: &[Span]) -> usize {
    row.iter().map(|s| s.content.width()).sum()
}

fn chars_of(s: &str, style: Style) -> Chars {
    s.graphemes(true).map(|g| (g.to_string(), style)).collect()
}

fn to_chars(row: &[Span]) -> Chars {
    row.iter()
        .flat_map(|s| {
            s.content
                .graphemes(true)
                .map(move |g| (g.to_string(), s.style))
        })
        .collect()
}

fn to_row(cs: &[(String, Style)]) -> Row {
    let mut row: Row = Vec::new();
    let mut buf = String::new();
    let mut cur: Option<Style> = None;
    for (g, st) in cs {
        if let Some(prev) = cur
            && prev != *st
        {
            row.push(Span::styled(std::mem::take(&mut buf), prev));
        }
        cur = Some(*st);
        buf.push_str(g);
    }
    if let Some(st) = cur {
        row.push(Span::styled(buf, st));
    }
    row
}

fn plain(s: &str) -> Row {
    vec![Span::raw(s.to_string())]
}

fn spaces(n: usize) -> Span<'static> {
    Span::raw(" ".repeat(n))
}

/// Wraps at spaces, keeping runs of inner spaces; a word wider than the line is split by cluster.
fn wrap(cs: &[(String, Style)], width: usize) -> Vec<Row> {
    let width = width.max(1);
    let mut out: Vec<Chars> = Vec::new();
    for para in cs.split(|(g, _)| g == "\n") {
        let mut cur: Chars = Vec::new();
        let mut cur_w = 0;
        let mut first_line = true;
        let mut i = 0;
        while i < para.len() {
            let s = i;
            while i < para.len() && para[i].0 == " " {
                i += 1;
            }
            let gap = &para[s..i];
            let ws = i;
            while i < para.len() && para[i].0 != " " {
                i += 1;
            }
            let word = &para[ws..i];
            if word.is_empty() {
                break;
            }
            let ww = chars_width(word);
            // An indent that would push the word off the line is dropped rather than the word.
            let keep_gap = if cur.is_empty() {
                first_line && gap.len() + ww <= width
            } else if cur_w + gap.len() + ww <= width {
                true
            } else {
                out.push(std::mem::take(&mut cur));
                cur_w = 0;
                first_line = false;
                false
            };
            if keep_gap {
                cur.extend_from_slice(gap);
                cur_w += gap.len();
            }
            for (g, st) in word {
                let w = gw(g);
                if cur_w + w > width && cur_w > 0 {
                    out.push(std::mem::take(&mut cur));
                    cur_w = 0;
                    first_line = false;
                }
                cur.push((g.clone(), *st));
                cur_w += w;
            }
        }
        out.push(cur);
    }
    while out.len() > 1 && out.last().is_some_and(Vec::is_empty) {
        out.pop();
    }
    out.iter().map(|l| to_row(l)).collect()
}

fn ellipsis(ctx: &RenderCtx) -> &'static str {
    if ctx.ascii { "..." } else { "…" }
}

/// Keeps as many clusters as fit in `max_w` together with `tail`; the tail itself is cut when `max_w` is tiny.
fn cut_to(row: &[Span], max_w: usize, tail: &str) -> Row {
    let cs = to_chars(row);
    let style = cs.last().map(|(_, s)| *s).unwrap_or_default();
    let tail_w = tail.width().min(max_w);
    let mut out: Chars = Vec::new();
    let mut w = 0;
    for (g, st) in cs {
        if w + gw(&g) > max_w - tail_w {
            break;
        }
        w += gw(&g);
        out.push((g, st));
    }
    let mut tw = 0;
    for g in tail.graphemes(true) {
        if tw + gw(g) > tail_w {
            break;
        }
        tw += gw(g);
        out.push((g.to_string(), style));
    }
    to_row(&out)
}

fn fit(row: Row, max_w: usize, ctx: &RenderCtx) -> Row {
    if row_width(&row) <= max_w {
        row
    } else {
        cut_to(&row, max_w, ellipsis(ctx))
    }
}

fn append_ellipsis(row: Row, max_w: usize, ctx: &RenderCtx) -> Row {
    let tail = ellipsis(ctx);
    if row_width(&row) + tail.width() <= max_w {
        let mut row = row;
        row.push(Span::raw(tail));
        row
    } else {
        cut_to(&row, max_w, tail)
    }
}

fn limit_rows(mut rows: Vec<Row>, n: usize, max_w: usize, ctx: &RenderCtx) -> Vec<Row> {
    if rows.len() > n {
        rows.truncate(n);
        if let Some(last) = rows.pop() {
            rows.push(append_ellipsis(last, max_w, ctx));
        }
    }
    rows
}

fn g(ctx: &RenderCtx, unicode: &'static str, ascii: &'static str) -> &'static str {
    if ctx.ascii { ascii } else { unicode }
}

fn dim() -> Style {
    Style::new().add_modifier(theme::DIM)
}

// --- boxes ------------------------------------------------------------------

struct Glyphs {
    top_left: &'static str,
    horizontal: &'static str,
    vertical: &'static str,
    top_right: &'static str,
    bottom_left: &'static str,
    bottom_right: &'static str,
}

fn glyphs(ctx: &RenderCtx, dotted: bool) -> Glyphs {
    let (top_left, horizontal, vertical, top_right, bottom_left, bottom_right) =
        match (ctx.ascii, dotted) {
            (false, false) => ("┌", "─", "│", "┐", "└", "┘"),
            (false, true) => ("┌", "╌", "╎", "┐", "└", "┘"),
            (true, false) => ("+", "-", "|", "+", "+", "+"),
            (true, true) => ("+", ".", ":", "+", "+", "+"),
        };
    Glyphs {
        top_left,
        horizontal,
        vertical,
        top_right,
        bottom_left,
        bottom_right,
    }
}

fn framed(
    ctx: &RenderCtx,
    dotted: bool,
    width: usize,
    title: Option<Row>,
    body: Vec<Row>,
    style: Style,
) -> Vec<Row> {
    let gl = glyphs(ctx, dotted);
    let (h, v) = (gl.horizontal, gl.vertical);
    let inner = width.saturating_sub(4);
    let mut top: Row = vec![Span::styled(gl.top_left, style)];
    match title {
        Some(t) => {
            let max_title = width.saturating_sub(5);
            let t = if row_width(&t) > max_title {
                cut_to(&t, max_title, ellipsis(ctx))
            } else {
                t
            };
            let used = 1 + 1 + row_width(&t) + 1;
            top.push(Span::styled(" ", style));
            top.extend(t);
            top.push(Span::styled(" ", style));
            top.push(Span::styled(
                h.repeat(width.saturating_sub(used + 1)),
                style,
            ));
        }
        None => top.push(Span::styled(h.repeat(width.saturating_sub(2)), style)),
    }
    top.push(Span::styled(gl.top_right, style));
    let mut out = vec![top];
    for row in body {
        let pad = inner.saturating_sub(row_width(&row));
        let mut r: Row = vec![Span::styled(format!("{v} "), style)];
        r.extend(row);
        r.push(Span::styled(format!("{} {v}", " ".repeat(pad)), style));
        out.push(r);
    }
    out.push(vec![Span::styled(
        format!(
            "{}{}{}",
            gl.bottom_left,
            h.repeat(width.saturating_sub(2)),
            gl.bottom_right
        ),
        style,
    )]);
    out
}

// --- frame ------------------------------------------------------------------

struct Frame<'a> {
    ctx: &'a RenderCtx,
    prefix: &'a Prefix,
    prefix_w: usize,
    cw: usize,
    rail: Style,
    lines: Vec<Line<'static>>,
}

impl<'a> Frame<'a> {
    fn new(ctx: &'a RenderCtx, prefix: &'a Prefix, width: u16, rail: Style) -> Self {
        let prefix_w = row_width(&prefix.first).max(row_width(&prefix.rest));
        Frame {
            ctx,
            prefix,
            prefix_w,
            cw: usize::from(width).saturating_sub(1 + prefix_w + 2),
            rail,
            lines: Vec::new(),
        }
    }

    /// Column 0 is the selection marker slot; the prefix is padded so every rail lines up.
    fn lead(&self) -> Row {
        let p = if self.lines.is_empty() {
            &self.prefix.first
        } else {
            &self.prefix.rest
        };
        let mut spans = vec![Span::raw(" ")];
        spans.extend(p.iter().cloned());
        spans.push(spaces(self.prefix_w - row_width(p)));
        spans
    }

    fn push_with(&mut self, symbol: &'static str, style: Style, row: Row) {
        let mut spans = self.lead();
        spans.push(Span::styled(symbol, style));
        spans.push(Span::raw(" "));
        spans.extend(fit(row, self.cw, self.ctx));
        self.lines.push(Line::from(spans));
    }

    fn push(&mut self, row: Row) {
        let rail = g(self.ctx, "│", "|");
        self.push_with(rail, self.rail, row);
    }
}

// --- entry points -----------------------------------------------------------

/// Renders one card as terminal lines; the line count is the card's height and depends on `width`.
///
/// Every line is at most `width` columns wide, prefix included, as long as `width` is at least
/// 3 plus the prefix width (marker, rail and gap); below that only "no panic" is guaranteed. Column 0 of every line is left
/// blank for the selection marker that [`apply_selection`] fills in. Separators and the rails
/// between cards belong to the list view, not to this function.
pub fn card_lines(
    input: CardInput<'_>,
    opts: &CardOptions,
    ctx: &RenderCtx,
    width: u16,
    prefix: &Prefix,
) -> Vec<Line<'static>> {
    match input {
        CardInput::Commit(c) => commit_card(c, opts, ctx, width, prefix),
        CardInput::Tombstone(_) => {
            let mut f = Frame::new(ctx, prefix, width, dim());
            let node = g(ctx, "◌", "x");
            f.push_with(node, dim(), vec![Span::styled(DELETED_TEXT, dim())]);
            f.lines
        }
        CardInput::Deleting { short_hash } => {
            let text = format!(
                "rm {} {} /dev/null",
                clean_inline(short_hash),
                g(ctx, "→", "->")
            );
            let mut f = Frame::new(ctx, prefix, width, Style::default());
            let box_w = (text.width() + 4).min(f.cw + 2);
            let rows = framed(
                ctx,
                false,
                box_w,
                None,
                vec![fit(plain(&text), box_w.saturating_sub(4), ctx)],
                Style::default(),
            );
            for row in rows {
                let mut spans = f.lead();
                spans.extend(row);
                f.lines.push(Line::from(spans));
            }
            f.lines
        }
    }
}

/// Marks lines as selected: replaces column 0 with the marker and paints the row up to `width`.
///
/// Call it after [`card_lines`] with the same `width`; the line count is unchanged. Rows are
/// padded with spaces because a line style alone does not paint past the end of the text.
pub fn apply_selection(lines: &mut [Line<'static>], ctx: &RenderCtx, width: u16) {
    let bg = Style::new().bg(theme::cursor_bg(ctx.truecolor));
    let marker = g(ctx, "▌", ">");
    for line in lines {
        line.style = line.style.patch(bg);
        let used: usize = line.spans.iter().map(|s| s.content.width()).sum();
        let pad = usize::from(width).saturating_sub(used);
        if let Some(first) = line.spans.first_mut() {
            *first = Span::styled(marker, bg);
        }
        for span in line.spans.iter_mut().skip(1) {
            span.style = span.style.patch(bg);
        }
        if pad > 0 {
            line.spans.push(Span::styled(" ".repeat(pad), bg));
        }
    }
}

// --- commit card ------------------------------------------------------------

fn lane_style(u: &User, ctx: &RenderCtx) -> Style {
    Style::new().fg(theme::lane_color(u.effective_lane(), ctx.truecolor))
}

fn display_name(u: &User) -> String {
    clean_inline(u.name.as_deref().unwrap_or(&u.username))
}

#[derive(Clone, Copy)]
enum HeaderKind {
    Full,
    NoTime,
    NameOnly,
}

fn header_left(
    ctx: &RenderCtx,
    author: &User,
    created_at: OffsetDateTime,
    kind: HeaderKind,
    pos: Option<(u16, u16)>,
    badge: Option<&str>,
    max_w: usize,
) -> Row {
    let dot = g(ctx, "·", "-");
    let handle = clean_inline(&author.username);
    let mut suffix = match kind {
        HeaderKind::Full => format!(
            " @{handle} {dot} {}",
            timefmt::relative(created_at, ctx.now, ctx.local_offset)
        ),
        HeaderKind::NoTime => format!(" @{handle}"),
        HeaderKind::NameOnly => String::new(),
    };
    if let Some((p, t)) = pos {
        suffix.push_str(&format!("  [{p}/{t}]"));
    }
    if let Some(b) = badge {
        suffix.push_str(if pos.is_some() { " " } else { "  " });
        suffix.push_str(&format!("[{b}]"));
    }
    let name = vec![Span::styled(
        display_name(author),
        Style::new().add_modifier(Modifier::BOLD),
    )];
    let avail = max_w.saturating_sub(suffix.width());
    let mut row = if row_width(&name) > avail {
        cut_to(&name, avail, ellipsis(ctx))
    } else {
        name
    };
    row.push(Span::raw(suffix));
    fit(row, max_w, ctx)
}

/// Header content with the hash flush right when the variant shows one.
fn header_row(c: &Commit, opts: &CardOptions, ctx: &RenderCtx, cw: usize) -> Row {
    let thread_main = opts.variant == Variant::ThreadMain;
    let kind = match (opts.node, thread_main) {
        (Node::SameAuthor, _) => HeaderKind::NameOnly,
        (_, true) => HeaderKind::NoTime,
        _ => HeaderKind::Full,
    };
    let badge = match (opts.is_own, c.status) {
        (true, Some(Status::Pending)) => Some("in review"),
        (true, Some(Status::Rejected)) => Some("rejected"),
        _ => None,
    };
    if thread_main || matches!(opts.variant, Variant::Reply | Variant::Continuation) {
        return header_left(
            ctx,
            &c.author,
            c.created_at,
            kind,
            opts.thread_pos,
            badge,
            cw,
        );
    }
    let hash = clean_inline(&c.short_hash);
    let left_max = cw.saturating_sub(hash.width() + 1);
    let mut head = header_left(
        ctx,
        &c.author,
        c.created_at,
        kind,
        opts.thread_pos,
        badge,
        left_max,
    );
    let pad = cw.saturating_sub(row_width(&head) + hash.width());
    head.push(spaces(pad.max(1)));
    head.push(Span::styled(hash, dim()));
    head
}

fn image_rows(c: &Commit, cw: usize) -> Vec<Row> {
    if c.images.is_empty() {
        return Vec::new();
    }
    let n = c.images.len();
    let mut text = String::new();
    for (i, img) in c.images.iter().enumerate() {
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str(&format!(
            "[image {}/{n} {}x{}]",
            i + 1,
            img.width,
            img.height
        ));
        if let Some(d) = &img.description {
            text.push(' ');
            text.push_str(&clean_inline(d));
        }
    }
    wrap(&chars_of(&text, Style::default()), cw)
}

// Each attachment is pushed in turn instead of being collected into one layout pass:
// every kind already yields finished rows, so a shared pass would only add indirection.
fn commit_card(
    c: &Commit,
    opts: &CardOptions,
    ctx: &RenderCtx,
    width: u16,
    prefix: &Prefix,
) -> Vec<Line<'static>> {
    let lane = lane_style(&c.author, ctx);
    let mut f = Frame::new(ctx, prefix, width, lane);
    let thread_main = opts.variant == Variant::ThreadMain;
    let reply = opts.variant == Variant::Reply;
    // A reply's body sits four columns right of its node, so its content is two columns narrower.
    let cw = if reply { f.cw.saturating_sub(2) } else { f.cw };

    let node = match (opts.variant, opts.node) {
        (Variant::Reply, _) => g(ctx, "●", "o"),
        (_, Node::Normal) => g(ctx, "◉", "o"),
        (_, Node::SameAuthor) => g(ctx, "•", "*"),
    };
    let header_w = f.cw;
    f.push_with(node, lane, header_row(c, opts, ctx, header_w));

    // The rail under a reply's node only continues when its children follow.
    let rail_sym = if opts.has_children {
        g(ctx, "│", "|")
    } else {
        " "
    };
    let put = |f: &mut Frame, row: Row| {
        if reply {
            let mut indented = vec![spaces(2)];
            indented.extend(row);
            f.push_with(rail_sym, lane, indented);
        } else {
            f.push(row);
        }
    };

    if opts.quotes_parent {
        put(
            &mut f,
            plain(&format!("{} quoted this commit", g(ctx, "❝", "\""))),
        );
    } else if let Some(who) = &opts.replying_to {
        let arrow = g(ctx, "↳", ">");
        put(
            &mut f,
            plain(&format!("{arrow} replying to @{}", clean_inline(who))),
        );
    }

    let (rows, show_more) = body_rows(c, opts.variant, ctx, cw);
    for r in rows {
        put(&mut f, r);
    }
    if show_more {
        put(&mut f, plain("Show more"));
    }
    if opts.variant == Variant::Continuation {
        return f.lines;
    }

    for r in image_rows(c, cw) {
        put(&mut f, r);
    }
    if let Some(poll) = &c.poll {
        for r in poll_rows(poll, ctx) {
            put(&mut f, r);
        }
    }
    for r in quote_rows(&c.quoted_commit, ctx, cw) {
        put(&mut f, r);
    }
    if let Some(lp) = &c.link_preview
        && !lp.is_pending
    {
        for r in link_rows(lp, ctx, cw) {
            put(&mut f, r);
        }
    }

    if thread_main {
        let dot = g(ctx, "·", "-");
        f.push(plain(&format!(
            "{} {dot} commit {}",
            timefmt::full(c.created_at, ctx.local_offset),
            clean_inline(&c.short_hash)
        )));
    }

    for r in action_rows(c, opts, ctx, cw) {
        put(&mut f, r);
    }

    if opts.replies_expanded {
        reply_preview(c, ctx, &mut f);
    }
    f.lines
}

// --- body -------------------------------------------------------------------

fn fold(text: &str, ctx: &RenderCtx) -> Option<String> {
    let lines: Vec<&str> = text.split('\n').collect();
    if text.chars().count() <= FOLD_CHARS && lines.len() <= FOLD_LINES {
        return None;
    }
    let mut kept = lines[..lines.len().min(FOLD_LINES)].join("\n");
    if kept.chars().count() > FOLD_CHARS {
        let cut: String = kept.chars().take(FOLD_CHARS).collect();
        let at_boundary = kept
            .chars()
            .nth(FOLD_CHARS)
            .is_some_and(char::is_whitespace);
        kept = match cut.rfind(char::is_whitespace) {
            Some(i) if !at_boundary => cut[..i].to_string(),
            _ => cut,
        };
    }
    let mut kept = kept.trim_end().to_string();
    kept.push_str(ellipsis(ctx));
    Some(kept)
}

fn body_rows(c: &Commit, variant: Variant, ctx: &RenderCtx, cw: usize) -> (Vec<Row>, bool) {
    let list = variant == Variant::List;
    match &c.body_segments {
        Some(segs) => match (&c.preview_segments, list) {
            (Some(preview), true) => (segment_rows(preview, ctx, cw), true),
            _ => (segment_rows(segs, ctx, cw), false),
        },
        None => {
            let text = clean(&c.body);
            let text = text.trim_end_matches('\n');
            if text.is_empty() {
                return (Vec::new(), false);
            }
            let folded = if list { fold(text, ctx) } else { None };
            let show_more = folded.is_some();
            let text = folded.unwrap_or_else(|| text.to_string());
            (wrap(&chars_of(&text, Style::default()), cw), show_more)
        }
    }
}

fn segment_rows(segs: &[Segment], ctx: &RenderCtx, cw: usize) -> Vec<Row> {
    let code_style = Style::new().bg(theme::code_bg(ctx.truecolor));
    let mut out = Vec::new();
    let mut inline: Chars = Vec::new();
    for seg in segs {
        match seg {
            Segment::Text { text } => inline.extend(chars_of(&clean(text), Style::default())),
            Segment::Code { code } => inline.extend(chars_of(&clean(code), code_style)),
            Segment::Block { language, html } => {
                out.extend(wrap_nonempty(&inline, cw));
                inline.clear();
                out.extend(code_block(language.as_deref(), html, ctx, cw));
            }
            Segment::Unknown => {}
        }
    }
    out.extend(wrap_nonempty(&inline, cw));
    out
}

fn wrap_nonempty(cs: &[(String, Style)], cw: usize) -> Vec<Row> {
    if cs.is_empty() {
        Vec::new()
    } else {
        wrap(cs, cw)
    }
}

fn code_block(language: Option<&str>, html: &str, ctx: &RenderCtx, cw: usize) -> Vec<Row> {
    let mut rows: Vec<Row> = highlight(html)
        .iter()
        .map(|line| {
            line.iter()
                .map(|(t, class)| {
                    let style = match class {
                        HlClass::Plain => Style::default(),
                        c => Style::new().fg(theme::hl_color(*c, ctx.truecolor)),
                    };
                    Span::styled(clean(t), style)
                })
                .collect()
        })
        .collect();
    while rows.len() > 1 && rows.last().is_some_and(|r| row_width(r) == 0) {
        rows.pop();
    }
    let label = language.map(clean_inline).filter(|l| !l.is_empty());
    let longest = rows.iter().map(|r| row_width(r)).max().unwrap_or(0);
    let label_min = label.as_ref().map_or(0, |l| l.width() + 5);
    let width = (longest + 4).max(label_min).min(cw);
    let rows = rows
        .into_iter()
        .map(|r| fit(r, width.saturating_sub(4), ctx))
        .collect();
    framed(
        ctx,
        false,
        width,
        label.map(|l| plain(&l)),
        rows,
        Style::default(),
    )
}

// --- attachments ------------------------------------------------------------

fn poll_rows(poll: &Poll, ctx: &RenderCtx) -> Vec<Row> {
    let mut out = Vec::new();
    for opt in &poll.options {
        let label = clean_inline(&opt.label);
        let text = if poll.total_votes.is_some() {
            let pct = u32::from(opt.percentage.unwrap_or(0));
            let filled = ((pct + 5) / 10).min(10) as usize;
            format!(
                "{}{} {pct:>3}% {label}",
                g(ctx, "▇", "#").repeat(filled),
                g(ctx, "░", ".").repeat(10 - filled)
            )
        } else {
            format!("{} {label}", g(ctx, "○", "o"))
        };
        out.push(plain(&text));
    }
    // The server flips is_closed lazily, so an expired deadline counts as closed too.
    let expired = poll.closes_at.is_some_and(|t| t <= ctx.now);
    if poll.is_closed || expired {
        out.push(plain("Final results"));
    } else if let Some(closes) = poll.closes_at {
        let left = closes - ctx.now;
        let text = if left.whole_days() >= 1 {
            format!("{}d left", left.whole_days())
        } else if left.whole_hours() >= 1 {
            format!("{}h left", left.whole_hours())
        } else if left.whole_minutes() >= 1 {
            format!("{}m left", left.whole_minutes())
        } else {
            "<1m left".to_string()
        };
        out.push(plain(&text));
    }
    out
}

fn text_box(ctx: &RenderCtx, cw: usize, text: &str) -> Vec<Row> {
    let body = wrap(&chars_of(text, Style::default()), cw.saturating_sub(4));
    framed(ctx, true, cw, None, body, dim())
}

/// An empty body yields no rows, since wrapping it would still draw one blank row.
fn excerpt(text: &str, max_rows: usize, width: usize, ctx: &RenderCtx) -> Vec<Row> {
    let text = clean(text);
    if text.is_empty() {
        return Vec::new();
    }
    limit_rows(
        wrap(&chars_of(&text, Style::default()), width),
        max_rows,
        width,
        ctx,
    )
}

fn quote_rows(q: &Quoted, ctx: &RenderCtx, cw: usize) -> Vec<Row> {
    match q {
        Quoted::NotLoaded => Vec::new(),
        Quoted::Unavailable => text_box(ctx, cw, UNAVAILABLE_TEXT),
        Quoted::Item(item) => match item.as_ref() {
            ThreadItem::Tombstone(_) => text_box(ctx, cw, DELETED_TEXT),
            ThreadItem::Commit(qc) => {
                let mut title = vec![Span::raw(format!("{} ", g(ctx, "❝", "\"")))];
                title.extend(header_left(
                    ctx,
                    &qc.author,
                    qc.created_at,
                    HeaderKind::Full,
                    None,
                    None,
                    cw.saturating_sub(7),
                ));
                let inner = cw.saturating_sub(4);
                let body = excerpt(&qc.body, 3, inner, ctx);
                framed(ctx, false, cw, Some(title), body, Style::default())
            }
        },
    }
}

fn host_of(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    authority
        .rsplit('@')
        .next()
        .unwrap_or(authority)
        .to_string()
}

fn link_rows(lp: &LinkPreview, ctx: &RenderCtx, cw: usize) -> Vec<Row> {
    // The server sends "" for missing text on some sites; treat it like null.
    let title = lp.title.as_deref().filter(|s| !s.is_empty());
    let description = lp.description.as_deref().filter(|s| !s.is_empty());
    if title.is_none() && description.is_none() {
        return vec![plain(&clean_inline(&lp.url))];
    }
    let inner = cw.saturating_sub(4);
    let mut body = Vec::new();
    for text in [title, description].into_iter().flatten() {
        let rows = wrap(&chars_of(&clean(text), Style::default()), inner);
        body.extend(limit_rows(rows, 2, inner, ctx));
    }
    let site = clean_inline(lp.site_name.as_deref().unwrap_or(&host_of(&lp.url)));
    framed(ctx, false, cw, Some(plain(&site)), body, Style::default())
}

// --- actions and replies ----------------------------------------------------

fn action_rows(c: &Commit, opts: &CardOptions, ctx: &RenderCtx, cw: usize) -> Vec<Row> {
    if opts.variant == Variant::Reply {
        return vec![reply_actions(c, opts, ctx)];
    }
    let starred = c.starred.unwrap_or(false);
    let star_icon = if starred {
        g(ctx, "★", "*")
    } else {
        g(ctx, "☆", "*")
    };
    let star_style = if starred {
        theme::current_style()
    } else {
        Style::default()
    };
    let mut items: Vec<Row> = vec![vec![Span::styled(
        format!("{star_icon} {}", c.stars_count.unwrap_or(0)),
        star_style,
    )]];
    if ctx.pasteis_mode {
        items.push(plain(&format!(
            "{} {}",
            g(ctx, "◓", "p"),
            c.pasteis_count.unwrap_or(0)
        )));
    }
    items.push(plain(&format!("{} Quote", g(ctx, "❝", "\""))));
    let reply = if opts.has_draft { "stash@{0}" } else { "Reply" };
    items.push(plain(&format!("{} {reply}", g(ctx, "↩", "<"))));
    match c.replies_count.unwrap_or(0) {
        0 => {}
        1 => items.push(plain(&format!("{} 1 reply", g(ctx, "⌄", "v")))),
        n => items.push(plain(&format!("{} {n} replies", g(ctx, "⌄", "v")))),
    }

    let mut lines: Vec<Row> = Vec::new();
    for item in items {
        match lines.last_mut() {
            Some(cur) if row_width(cur) + 2 + row_width(&item) <= cw => {
                cur.push(spaces(2));
                cur.extend(item);
            }
            _ => lines.push(item),
        }
    }

    let saved = if c.is_saved.unwrap_or(false) {
        g(ctx, "⊠", "[x]")
    } else {
        g(ctx, "⊡", "[ ]")
    };
    let right = format!("{saved} {}", g(ctx, "⚑", "!"));
    let rw = right.width();
    match lines.last_mut() {
        Some(cur) if row_width(cur) + 1 + rw <= cw => {
            let pad = cw - row_width(cur) - rw;
            cur.push(spaces(pad));
            cur.push(Span::raw(right));
        }
        _ => lines.push(vec![spaces(cw.saturating_sub(rw)), Span::raw(right)]),
    }
    lines
}

/// A reply only offers the star and the reply button, to keep a long thread quiet.
fn reply_actions(c: &Commit, opts: &CardOptions, ctx: &RenderCtx) -> Row {
    let starred = c.starred.unwrap_or(false);
    let star_icon = if starred {
        g(ctx, "★", "*")
    } else {
        g(ctx, "☆", "*")
    };
    let star_style = if starred {
        theme::current_style()
    } else {
        Style::default()
    };
    let label = if opts.has_draft { "stash@{0}" } else { "Reply" };
    vec![
        Span::styled(
            format!("{star_icon} {}", c.stars_count.unwrap_or(0)),
            star_style,
        ),
        spaces(2),
        Span::raw(format!("{} {label}", g(ctx, "↩", "<"))),
    ]
}

fn reply_preview(c: &Commit, ctx: &RenderCtx, f: &mut Frame) {
    let Some(preview) = &c.reply_preview else {
        return;
    };
    let cw = f.cw;
    let parent = lane_style(&c.author, ctx);
    let shown = preview.data.len().min(ctx.reply_preview_limit);
    for (i, item) in preview.data.iter().take(shown).enumerate() {
        let last = i + 1 == shown;
        let branch = match (last, ctx.ascii) {
            (false, false) => "├─",
            (true, false) => "└─",
            (false, true) => "+-",
            (true, true) => "`-",
        };
        let mut head: Row = vec![Span::styled(branch, parent)];
        let mut body = Vec::new();
        match item {
            ThreadItem::Commit(rc) => {
                head.push(Span::styled(g(ctx, "●", "o"), lane_style(&rc.author, ctx)));
                head.push(Span::raw(" "));
                head.extend(header_left(
                    ctx,
                    &rc.author,
                    rc.created_at,
                    HeaderKind::Full,
                    None,
                    None,
                    cw.saturating_sub(4),
                ));
                body = excerpt(&rc.body, 3, cw.saturating_sub(6), ctx);
            }
            ThreadItem::Tombstone(_) => {
                head.push(Span::raw(" "));
                head.push(Span::styled(g(ctx, "◌", "x"), dim()));
                head.push(Span::styled(format!(" {DELETED_TEXT}"), dim()));
            }
        }
        f.push(head);
        let guide = if last {
            Span::raw("      ")
        } else {
            let bar = g(ctx, "│", "|");
            Span::styled(format!("{bar}     "), parent)
        };
        for row in body {
            let mut r = vec![guide.clone()];
            r.extend(row);
            f.push(r);
        }
    }
    let remaining = (c.replies_count.unwrap_or(0) as usize).saturating_sub(shown);
    if remaining > 0 {
        f.push(plain(&format!("Show {remaining} more")));
    } else if preview.next_cursor.is_some() {
        f.push(plain("Show more replies"));
    }
}
