use ratatui::style::Style;
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use crate::app::{App, Session};
use crate::feed::LoadState;
use crate::model::{Commit, ThreadItem};
use crate::thread::{Slot, SlotKind, ThreadState, short_hash};
use crate::ui::card::{
    CardInput, CardOptions, Node, Prefix, RenderCtx, Variant, apply_selection, card_lines,
};
use crate::ui::feed::{CardSpan, plain, render_ctx, truncate};
use crate::ui::theme;

#[derive(Debug, Clone, Default)]
pub struct ThreadRows {
    pub lines: Vec<Line<'static>>,
    /// One entry per selectable item, in selection order.
    pub entries: Vec<CardSpan>,
}

const COPY_LINK: &str = "y copy link";

/// Guide columns of the levels above (and including) the row: a rail while siblings follow.
fn guides(lasts: &[bool], ctx: &RenderCtx) -> String {
    let rail = if ctx.ascii { "| " } else { "│ " };
    lasts
        .iter()
        .map(|last| if *last { "  " } else { rail })
        .collect()
}

fn prefix_of(lasts: &[bool], ctx: &RenderCtx) -> Prefix {
    let (own, up) = match lasts.split_last() {
        Some((own, up)) => (*own, up),
        None => return Prefix::default(),
    };
    let above = guides(up, ctx);
    let branch = match (own, ctx.ascii) {
        (false, false) => "├─",
        (true, false) => "└─",
        (false, true) => "+-",
        (true, true) => "`-",
    };
    Prefix {
        first: vec![Span::raw(format!("{above}{branch}"))],
        rest: vec![Span::raw(guides(lasts, ctx))],
    }
}

/// Shows no reply count: the thread opens replies through the hint rows instead.
fn without_count(c: &Commit) -> Commit {
    Commit {
        replies_count: None,
        ..c.clone()
    }
}

struct Painter<'a> {
    ctx: RenderCtx,
    own: Option<&'a str>,
    width: usize,
}

impl Painter<'_> {
    fn card_w(&self) -> u16 {
        u16::try_from(self.width.saturating_sub(1)).unwrap_or(u16::MAX)
    }

    fn rail(&self, lane_of: &Commit) -> Line<'static> {
        let color = theme::lane_color(lane_of.author.effective_lane(), self.ctx.truecolor);
        Line::from(vec![
            Span::raw(" "),
            Span::styled("│", Style::new().fg(color)),
        ])
    }

    fn commit_card(&self, c: &Commit, opts: CardOptions, prefix: &Prefix) -> Vec<Line<'static>> {
        let opts = CardOptions {
            is_own: self.own == Some(c.author.username.as_str()),
            ..opts
        };
        card_lines(
            CardInput::Commit(&without_count(c)),
            &opts,
            &self.ctx,
            self.card_w(),
            prefix,
        )
    }

    fn item_card(
        &self,
        item: &ThreadItem,
        opts: CardOptions,
        prefix: &Prefix,
    ) -> Vec<Line<'static>> {
        match item {
            ThreadItem::Commit(c) => self.commit_card(c, opts, prefix),
            ThreadItem::Tombstone(t) => card_lines(
                CardInput::Tombstone(t),
                &opts,
                &self.ctx,
                self.card_w(),
                prefix,
            ),
        }
    }

    fn note(&self, lasts: &[bool], text: &str) -> Vec<Line<'static>> {
        let dot = if self.ctx.ascii { ":" } else { "┊" };
        let body = format!("{}{dot} {text}", guides(lasts, &self.ctx));
        vec![Line::from(vec![
            Span::raw(" "),
            Span::raw(truncate(&body, self.width.saturating_sub(2))),
        ])]
    }

    fn slot_lines(&self, slot: &Slot, main: &Commit) -> Vec<Line<'static>> {
        match &slot.kind {
            SlotKind::Rail => vec![self.rail(main)],
            SlotKind::Ancestor(item) => {
                self.item_card(item, CardOptions::default(), &Prefix::default())
            }
            SlotKind::Main { pos } => self.commit_card(
                main,
                CardOptions {
                    variant: Variant::ThreadMain,
                    thread_pos: *pos,
                    ..CardOptions::default()
                },
                &Prefix::default(),
            ),
            SlotKind::Continuation(c) => self.commit_card(
                c,
                CardOptions {
                    variant: Variant::Continuation,
                    node: Node::SameAuthor,
                    thread_pos: Some((2, 2)),
                    ..CardOptions::default()
                },
                &Prefix::default(),
            ),
            SlotKind::Reply {
                item,
                lasts,
                replying_to,
                quote,
                has_children,
            } => self.item_card(
                item,
                CardOptions {
                    variant: Variant::Reply,
                    replying_to: replying_to.clone(),
                    quotes_parent: *quote,
                    has_children: *has_children,
                    ..CardOptions::default()
                },
                &prefix_of(lasts, &self.ctx),
            ),
            SlotKind::Hint { lasts, count } => {
                let text = if *count == 1 {
                    "1 reply (o)".to_string()
                } else {
                    format!("{count} replies (o)")
                };
                self.note(lasts, &text)
            }
            SlotKind::Note { lasts, text } => self.note(lasts, text),
            SlotKind::Error {
                lasts,
                code,
                retry_key,
            } => {
                let body = format!(
                    "{}error: {code} — press {retry_key} to retry",
                    guides(lasts, &self.ctx)
                );
                vec![Line::from(vec![
                    Span::raw(" "),
                    Span::raw(truncate(&body, self.width.saturating_sub(2))),
                ])]
            }
            SlotKind::SignIn => vec![plain(
                " ┊ sign in to see replies",
                self.width.saturating_sub(1),
            )],
        }
    }
}

fn header(id: &str, width: usize) -> Line<'static> {
    let text = format!("{}  {COPY_LINK}", short_hash(id));
    let pad = width.saturating_sub(1).saturating_sub(text.width());
    Line::raw(format!("{}{text}", " ".repeat(pad)))
}

fn status_line(state: &ThreadState, id: &str, width: usize) -> Line<'static> {
    let width = width.saturating_sub(1);
    match &state.main_load {
        LoadState::Failed { code, .. } if code == "commit_unavailable" || code == "http_404" => {
            plain(
                &format!(" fatal: commit {} is not available", short_hash(id)),
                width,
            )
        }
        LoadState::Failed { code, .. } => {
            plain(&format!(" error: {code} — press R to retry"), width)
        }
        LoadState::Idle => plain(" ┊ (loading...)", width),
    }
}

pub fn rows(app: &App, id: &str, width: u16) -> ThreadRows {
    let width = usize::from(width);
    let Some(state) = app.thread(id) else {
        return ThreadRows {
            lines: vec![plain(" ┊ (loading...)", width)],
            entries: Vec::new(),
        };
    };
    let main = match &state.main {
        Some(main) if !state.main_failed() => main,
        _ => {
            return ThreadRows {
                lines: vec![status_line(state, id, width)],
                entries: Vec::new(),
            };
        }
    };
    let guest = matches!(app.session(), Session::Guest);
    let painter = Painter {
        ctx: render_ctx(app),
        own: match app.session() {
            Session::SignedIn(me) => Some(me.handle.as_str()),
            Session::Guest => None,
        },
        width,
    };
    let mut out = ThreadRows::default();
    out.lines.push(header(id, width));
    let mut index = 0;
    for slot in state.slots(id, guest) {
        let mut lines = painter.slot_lines(&slot, main);
        if slot.entry.is_some() {
            if index == state.selected {
                apply_selection(&mut lines, &painter.ctx, painter.card_w());
            }
            index += 1;
            out.entries.push(CardSpan {
                start: out.lines.len(),
                height: lines.len(),
            });
        }
        out.lines.extend(lines);
    }
    out
}
