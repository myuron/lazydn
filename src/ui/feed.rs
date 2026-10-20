use ratatui::text::Line;

use crate::app::{App, Session};
use crate::feed::{Feed, FeedKey};
use crate::ui::card::{CardInput, CardOptions, Prefix, RenderCtx, card_lines};

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

/// Builds the list rows shared by drawing and scroll maths, so both agree on card heights.
pub fn rows(feed: &Feed, _key: FeedKey, app: &App, width: u16) -> FeedRows {
    let ctx = render_ctx(app);
    let own = match app.session() {
        Session::SignedIn(me) => Some(me.handle.as_str()),
        Session::Guest => None,
    };
    let mut out = FeedRows::default();
    for commit in &feed.items {
        out.lines.push(Line::raw("│"));
        let opts = CardOptions {
            is_own: own == Some(commit.author.username.as_str()),
            replies_expanded: feed.expanded.contains(&commit.id),
            ..CardOptions::default()
        };
        let lines = card_lines(
            CardInput::Commit(commit),
            &opts,
            &ctx,
            width,
            &Prefix::default(),
        );
        out.cards.push(CardSpan {
            start: out.lines.len(),
            height: lines.len(),
        });
        out.lines.extend(lines);
    }
    out
}
