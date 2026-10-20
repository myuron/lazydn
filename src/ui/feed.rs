use ratatui::style::Style;
use ratatui::text::{Line, Span};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::app::{App, LogTab, Session};
use crate::feed::{Feed, FeedKey, LoadState};
use crate::model::{Commit, ThreadItem};
use crate::timefmt::day_label;
use crate::ui::card::{
    CardInput, CardOptions, Node, Prefix, RenderCtx, apply_selection, card_lines,
};
use crate::ui::theme;

/// Where one card sits inside [`FeedRows::lines`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CardSpan {
    pub start: usize,
    pub height: usize,
}

#[derive(Debug, Clone, Default)]
pub struct FeedRows {
    pub lines: Vec<Line<'static>>,
    /// One entry per item of the feed, in order.
    pub cards: Vec<CardSpan>,
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
            Session::Guest => false,
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

fn center_offset(text: &str, width: usize) -> usize {
    width.saturating_sub(text.width()) / 2
}

fn centered(text: &str, width: usize) -> Line<'static> {
    plain(
        &format!("{}{text}", " ".repeat(center_offset(text, width))),
        width,
    )
}

/// A rule that stops one column short of the edge, which stays free like the cards' right margin.
fn rule(head: &str, fill: &str, width: usize) -> Line<'static> {
    let limit = width.saturating_sub(1);
    let fill_len = limit.saturating_sub(head.width());
    plain(&format!("{head}{}", fill.repeat(fill_len)), limit)
}

pub(crate) fn retry_hint(code: &str, width: usize) -> Line<'static> {
    plain(&format!(" error: {code} — press R to retry"), width)
}

fn composer(width: usize) -> Vec<Line<'static>> {
    let hint_col = HINT_COL_MAX.min(width.saturating_sub(1 + HINT.len()));
    let hint_row = if hint_col >= HINT_COL_MIN {
        format!(" ┆{}{HINT}", " ".repeat(hint_col - 2))
    } else {
        " ┆".to_string()
    };
    vec![
        plain(" ┆", width),
        plain(" ◌  What did you ship?", width),
        plain(&hint_row, width),
        plain(" ┆", width),
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
    pub ctx: RenderCtx,
    pub own: Option<&'a str>,
    pub width: usize,
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
            ..CardOptions::default()
        };
        let mut card = card_lines(
            CardInput::Commit(commit),
            &opts,
            &self.ctx,
            card_w,
            &Prefix::default(),
        );
        if selected {
            apply_selection(&mut card, &self.ctx, card_w);
        }
        let mut rows = vec![rail(commit, &self.ctx, self.width)];
        rows.extend(card);
        rows
    }
}

/// Builds the list rows shared by drawing and scroll maths, so both agree on card heights.
pub fn rows(feed: &Feed, key: FeedKey, app: &App, width: u16) -> FeedRows {
    let own = match app.session() {
        Session::SignedIn(me) => Some(me.handle.as_str()),
        Session::Guest => None,
    };
    let env = Env {
        ctx: render_ctx(app),
        own,
        width: usize::from(width),
    };
    let w = env.width;
    let is_log = matches!(key, FeedKey::Log(_));
    let chronological = is_log && !feed.is_ranked;
    let mut out = FeedRows::default();

    if is_log && own.is_some() {
        out.lines.extend(composer(w));
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
            if i > 0 && app.last_visit() == Some(commit.id.as_str()) {
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
