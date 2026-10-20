use ratatui::style::Style;
use ratatui::text::{Line, Span};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::app::{App, ConnState, LogTab, Session};
use crate::compose::Surface;
use crate::feed::{Feed, FeedKey, LoadState};
use crate::model::{Commit, ThreadItem};
use crate::timefmt::day_label;
use crate::ui::card::{
    CardInput, CardOptions, Node, Prefix, RenderCtx, apply_selection, body_lead, card_lines,
};
use crate::ui::compose::reply_box;
use crate::ui::theme;

/// Where one card sits inside [`FeedRows::lines`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CardSpan {
    pub start: usize,
    pub height: usize,
}

/// Start and height of card `index`; the last card owns the closing rows (loading, error, root), so they scroll into view too.
pub fn card_extent(cards: &[CardSpan], index: usize, lines: usize) -> Option<(usize, usize)> {
    let card = cards.get(index)?;
    let height = if index + 1 == cards.len() {
        lines - card.start
    } else {
        card.height
    };
    Some((card.start, height))
}

/// Where a reply box sits among the rows its card returned.
#[derive(Debug, Clone, Copy)]
pub struct BoxPlace {
    /// Row and column of the cursor.
    pub cursor: (usize, usize),
    /// The last row of the box.
    pub end: usize,
}

#[derive(Debug, Clone, Default)]
pub struct FeedRows {
    pub lines: Vec<Line<'static>>,
    /// One entry per item of the feed, in order.
    pub cards: Vec<CardSpan>,
    /// Where the terminal cursor goes, as (row of `lines`, column), while a composer on this list has the focus.
    pub cursor: Option<(usize, usize)>,
    /// The last row of that box.
    pub box_end: Option<usize>,
}

pub fn render_ctx(app: &App) -> RenderCtx {
    let prefs = app.prefs();
    RenderCtx {
        now: app.now(),
        local_offset: app.local_offset(),
        truecolor: prefs.truecolor,
        ascii: prefs.ascii,
        pasteis_mode: match app.session() {
            Session::SignedIn(me) => me.pasteis_mode,
            Session::Guest | Session::Restoring => false,
        },
        reply_preview_limit: prefs.reply_preview_limit,
    }
}

const HINT: &str = "[c] Write a commit";
const HINT_COL_MAX: usize = 38;
const HINT_COL_MIN: usize = 3;

/// Cuts by display width because the fixed texts hold wide glyphs; no ellipsis, to keep rows plain.
pub(crate) fn truncate(text: &str, width: usize) -> String {
    let mut used = 0;
    let mut out = String::new();
    for c in text.chars() {
        let w = c.width().unwrap_or(0);
        if used + w > width {
            break;
        }
        used += w;
        out.push(c);
    }
    out
}

pub(crate) fn plain(text: &str, width: usize) -> Line<'static> {
    Line::raw(truncate(text, width))
}

pub(crate) fn center_offset(text: &str, width: usize) -> usize {
    width.saturating_sub(text.width()) / 2
}

fn centered(text: &str, width: usize) -> Line<'static> {
    plain(
        &format!("{}{text}", " ".repeat(center_offset(text, width))),
        width,
    )
}

/// A rule that stops one column short of the edge, which stays free like the cards' right margin.
pub(crate) fn rule(head: &str, fill: &str, width: usize) -> Line<'static> {
    let limit = width.saturating_sub(1);
    let fill_len = limit.saturating_sub(head.width());
    plain(&format!("{head}{}", fill.repeat(fill_len)), limit)
}

pub(crate) fn retry_hint(code: &str, width: usize) -> Line<'static> {
    plain(&format!(" error: {code} — press R to retry"), width)
}

fn composer(width: usize, draft: Option<String>) -> Vec<Line<'static>> {
    let hint_col = HINT_COL_MAX.min(width.saturating_sub(1 + HINT.len()));
    let hint_row = if hint_col >= HINT_COL_MIN {
        format!(" ┆{}{HINT}", " ".repeat(hint_col - 2))
    } else {
        " ┆".to_string()
    };
    vec![
        plain(" ┆", width),
        match &draft {
            Some(first) => plain(&format!(" ◌  {first}"), width.saturating_sub(1)),
            None => plain(" ◌  What did you ship?", width),
        },
        plain(&hint_row, width),
        match draft {
            Some(_) => plain(" ┆  stash@{0} draft saved", width),
            None => plain(" ┆", width),
        },
    ]
}

fn rail(commit: &Commit, ctx: &RenderCtx, width: usize) -> Line<'static> {
    let color = theme::lane_color(commit.author.effective_lane(), ctx.truecolor);
    let limit = width.saturating_sub(1);
    if limit < 2 {
        return plain(" ", limit);
    }
    Line::from(vec![
        Span::raw(" "),
        Span::styled("│", Style::new().fg(color)),
    ])
}

fn new_commits_banner(count: u32, width: usize) -> Line<'static> {
    let text = if count == 1 {
        "↑ 1 new commit".to_string()
    } else {
        format!("↑ {count} new commits")
    };
    let offset = center_offset(&text, width);
    Line::from(vec![
        Span::raw(" ".repeat(offset)),
        Span::styled(
            truncate(&text, width.saturating_sub(offset)),
            theme::current_style(),
        ),
    ])
}

/// What the row builders share, so that suggestions and listed cards are drawn alike.
pub(crate) struct Env<'a> {
    pub app: &'a App,
    pub ctx: RenderCtx,
    pub own: Option<&'a str>,
    pub width: usize,
    /// Where the reply box of the last card drew itself, in the rows that card returned.
    pub placed: std::cell::Cell<Option<BoxPlace>>,
}

impl Env<'_> {
    /// The rail row followed by the card; the card is everything after the first row.
    pub(crate) fn card_rows(
        &self,
        commit: &Commit,
        node: Node,
        selected: bool,
        replies_expanded: bool,
    ) -> Vec<Line<'static>> {
        let card_w = u16::try_from(self.width.saturating_sub(1)).unwrap_or(u16::MAX);
        let opts = CardOptions {
            node,
            is_own: self.own == Some(commit.author.username.as_str()),
            replies_expanded,
            replying_to: match commit.parent.as_deref() {
                Some(ThreadItem::Commit(p)) => Some(p.author.username.clone()),
                _ => None,
            },
            has_draft: self.app.drafts().has_reply(&commit.id),
            ..CardOptions::default()
        };
        let input = if self.app.is_deleting(&commit.id) {
            CardInput::Deleting {
                short_hash: &commit.short_hash,
            }
        } else {
            CardInput::Commit(commit)
        };
        let mut card = card_lines(input, &opts, &self.ctx, card_w, &Prefix::default());
        if selected {
            apply_selection(&mut card, &self.ctx, card_w);
        }
        let mut rows = vec![rail(commit, &self.ctx, self.width)];
        rows.extend(card);
        if let Some(compose) = self.app.compose().filter(|c| {
            c.surface == Surface::ReplyBox && c.key.reply_to() == Some(commit.id.as_str())
        }) {
            let lead = body_lead(commit, &opts, &self.ctx, &Prefix::default());
            let (boxed, cursor) = reply_box(self.app, compose, &lead, usize::from(card_w));
            self.placed.set(Some(BoxPlace {
                cursor: (rows.len() + cursor.0, cursor.1),
                end: rows.len() + boxed.len() - 1,
            }));
            rows.extend(boxed);
        }
        rows
    }
}

/// Builds the list rows shared by drawing and scroll maths, so both agree on card heights.
pub fn rows(feed: &Feed, key: FeedKey, app: &App, width: u16) -> FeedRows {
    let own = match app.session() {
        Session::SignedIn(me) => Some(me.handle.as_str()),
        Session::Guest | Session::Restoring => None,
    };
    let env = Env {
        app,
        ctx: render_ctx(app),
        own,
        width: usize::from(width),
        placed: std::cell::Cell::new(None),
    };
    let w = env.width;
    let is_log = matches!(key, FeedKey::Log(_));
    let chronological = is_log && !feed.is_ranked;
    let mut out = FeedRows::default();

    if matches!(app.session(), Session::Restoring) && is_log {
        let text = if app.conn_state() == ConnState::Offline {
            " offline — retrying"
        } else {
            " restoring session…"
        };
        out.lines.push(plain(text, w));
        return out;
    }
    if is_log && own.is_some() {
        match crate::ui::compose::inline_field(app, w) {
            Some((field, cursor)) => {
                out.cursor = Some((out.lines.len() + cursor.0, cursor.1));
                out.lines.extend(field);
            }
            None => out
                .lines
                .extend(composer(w, crate::ui::compose::draft_preview(app))),
        }
    }

    if chronological && feed.new_commits > 0 {
        out.lines.push(new_commits_banner(feed.new_commits, w));
    }

    if feed.items.is_empty() {
        out.lines.extend(empty_state(feed, key, &env));
        return out;
    }

    let offset = env.ctx.local_offset;
    let mut previous: Option<&Commit> = None;
    for (i, commit) in feed.items.iter().enumerate() {
        let mut boundary = false;
        if chronological {
            let day = commit.created_at.to_offset(offset).date();
            if previous.is_none_or(|p| p.created_at.to_offset(offset).date() != day) {
                let label = day_label(commit.created_at, env.ctx.now, offset);
                out.lines.push(rule(&format!(" ╎ ── {label} "), "─", w));
                boundary = true;
            }
            if i > 0
                && matches!(key, FeedKey::Log(tab) if app.last_visit_of(tab) == Some(commit.id.as_str()))
            {
                out.lines.push(rule(" ╌╌ HEAD@{last visit} ", "╌", w));
                boundary = true;
            }
        }
        let same_author = chronological
            && !boundary
            && previous.is_some_and(|p| p.author.username == commit.author.username);
        let node = if same_author {
            Node::SameAuthor
        } else {
            Node::Normal
        };
        let rows = env.card_rows(
            commit,
            node,
            i == feed.selected,
            feed.expanded.contains(&commit.id),
        );
        out.cards.push(CardSpan {
            start: out.lines.len() + 1,
            height: rows.len() - 1,
        });
        if let Some(place) = env.placed.take() {
            out.cursor = Some((out.lines.len() + place.cursor.0, place.cursor.1));
            out.box_end = Some(out.lines.len() + place.end);
        }
        out.lines.extend(rows);
        previous = Some(commit);
    }

    out.lines.extend(list_tail(feed, w));
    out
}

/// The row of a list that has nothing to show yet because it is loading or failed.
pub(crate) fn pending_or_failed_row(feed: &Feed, w: usize) -> Option<Line<'static>> {
    if feed.pending.is_some() {
        return Some(plain(" ┊ (loading...)", w));
    }
    match &feed.load {
        LoadState::Failed { code, .. } => Some(retry_hint(code, w)),
        LoadState::Idle => None,
    }
}

/// The closing row under the last card: more loading, a failed page, or the end of history.
pub(crate) fn list_tail(feed: &Feed, w: usize) -> Option<Line<'static>> {
    if feed.pending.as_ref().is_some_and(|p| p.append) {
        Some(plain(" ┊ (loading more...)", w))
    } else if let LoadState::Failed { code, append: true } = &feed.load {
        Some(retry_hint(code, w))
    } else if feed.loaded_once && feed.next_cursor.is_none() && feed.pending.is_none() {
        Some(plain(
            " ○ That's the root commit. There's nothing before it.",
            w,
        ))
    } else {
        None
    }
}

fn empty_state(feed: &Feed, key: FeedKey, env: &Env) -> Vec<Line<'static>> {
    let w = env.width;
    if let Some(row) = pending_or_failed_row(feed, w) {
        return vec![row];
    }
    if !feed.loaded_once {
        return Vec::new();
    }
    match key {
        FeedKey::Saved => vec![
            Line::raw(""),
            Line::raw(""),
            centered("fatal: no saved commits yet", w),
            Line::raw(""),
            centered("press b on any commit to save it", w),
        ],
        FeedKey::Log(LogTab::Following) if !feed.suggestions.is_empty() => {
            // Suggestions never enter `cards`, so they cannot be selected.
            let mut lines = vec![plain(" $ who --should-i-follow", w)];
            for commit in &feed.suggestions {
                lines.extend(env.card_rows(commit, Node::Normal, false, false));
            }
            lines
        }
        FeedKey::Log(_) => vec![plain(" fatal: your log does not have any commits yet", w)],
    }
}
