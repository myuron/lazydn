use ratatui::style::Style;
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use crate::app::{App, Session};
use crate::compose::Surface;
use crate::drafts::Drafts;
use crate::feed::LoadState;
use crate::model::{Commit, ThreadItem};
use crate::thread::{Slot, SlotKind, ThreadState, item_id, short_hash};
use crate::ui::card::{
    CardInput, CardOptions, Node, Prefix, RenderCtx, Variant, apply_selection, body_lead,
    card_lines,
};
use crate::ui::compose::reply_box;
use crate::ui::feed::{CardSpan, plain, render_ctx, truncate};
use crate::ui::theme;

#[derive(Debug, Clone, Default)]
pub struct ThreadRows {
    pub lines: Vec<Line<'static>>,
    /// One entry per selectable item, in selection order.
    pub entries: Vec<CardSpan>,
    /// Where the terminal cursor goes (row of `lines`, column) while a reply box is open in this thread.
    pub cursor: Option<(usize, usize)>,
    /// The last row of that box.
    pub box_end: Option<usize>,
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

/// The thing a card is drawn from.
enum CardRef<'a> {
    Item(&'a ThreadItem),
    Commit(&'a Commit),
}

enum Drawn<'a> {
    Card {
        card: CardRef<'a>,
        opts: CardOptions,
        prefix: Prefix,
    },
    Plain(Vec<Line<'static>>),
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

    /// What a slot draws: a card with its options and prefix, or ready rows.
    fn draw_slot<'a>(&self, slot: &'a Slot, main: &'a Commit, drafts: &Drafts) -> Drawn<'a> {
        let card = |card, opts: CardOptions, id: &str, prefix| Drawn::Card {
            card,
            opts: CardOptions {
                has_draft: drafts.has_reply(id),
                ..opts
            },
            prefix,
        };
        match &slot.kind {
            SlotKind::Ancestor(item) => card(
                CardRef::Item(item),
                CardOptions::default(),
                item_id(item),
                Prefix::default(),
            ),
            SlotKind::Main { pos } => card(
                CardRef::Commit(main),
                CardOptions {
                    variant: Variant::ThreadMain,
                    thread_pos: *pos,
                    ..CardOptions::default()
                },
                &main.id,
                Prefix::default(),
            ),
            SlotKind::Continuation(c) => card(
                CardRef::Commit(c),
                CardOptions {
                    variant: Variant::Continuation,
                    node: Node::SameAuthor,
                    thread_pos: Some((2, 2)),
                    ..CardOptions::default()
                },
                &c.id,
                Prefix::default(),
            ),
            SlotKind::Reply {
                item,
                lasts,
                replying_to,
                quote,
                has_children,
            } => card(
                CardRef::Item(item),
                CardOptions {
                    variant: Variant::Reply,
                    replying_to: replying_to.clone(),
                    quotes_parent: *quote,
                    has_children: *has_children,
                    ..CardOptions::default()
                },
                item_id(item),
                prefix_of(lasts, &self.ctx),
            ),
            SlotKind::Rail => Drawn::Plain(vec![self.rail(main)]),
            SlotKind::Hint { lasts, count } => {
                let text = if *count == 1 {
                    "1 reply (o)".to_string()
                } else {
                    format!("{count} replies (o)")
                };
                Drawn::Plain(self.note(lasts, &text))
            }
            SlotKind::Note { lasts, text } => Drawn::Plain(self.note(lasts, text)),
            SlotKind::Error {
                lasts,
                code,
                retry_key,
            } => {
                let body = format!(
                    "{}error: {code} — press {retry_key} to retry",
                    guides(lasts, &self.ctx)
                );
                Drawn::Plain(vec![Line::from(vec![
                    Span::raw(" "),
                    Span::raw(truncate(&body, self.width.saturating_sub(2))),
                ])])
            }
            SlotKind::SignIn => Drawn::Plain(vec![plain(
                " ┊ sign in to see replies",
                self.width.saturating_sub(1),
            )]),
        }
    }

    fn slot_lines(&self, slot: &Slot, main: &Commit, drafts: &Drafts) -> Vec<Line<'static>> {
        match self.draw_slot(slot, main, drafts) {
            Drawn::Card {
                card: CardRef::Item(item),
                opts,
                prefix,
            } => self.item_card(item, opts, &prefix),
            Drawn::Card {
                card: CardRef::Commit(c),
                opts,
                prefix,
            } => self.commit_card(c, opts, &prefix),
            Drawn::Plain(lines) => lines,
        }
    }

    /// The columns left of the body text of the card in `slot`, when it is the card of a commit.
    fn lead_of(&self, slot: &Slot, main: &Commit, drafts: &Drafts) -> Option<Vec<Span<'static>>> {
        let Drawn::Card { card, opts, prefix } = self.draw_slot(slot, main, drafts) else {
            return None;
        };
        let commit: &Commit = match card {
            CardRef::Commit(c) => c,
            CardRef::Item(ThreadItem::Commit(c)) => c,
            CardRef::Item(ThreadItem::Tombstone(_)) => return None,
        };
        Some(body_lead(commit, &opts, &self.ctx, &prefix))
    }
}

/// The id of the commit that a selectable slot stands for.
fn slot_item_id<'a>(slot: &'a Slot, main: &'a Commit) -> Option<&'a str> {
    match &slot.kind {
        SlotKind::Ancestor(item) | SlotKind::Reply { item, .. } => Some(item_id(item)),
        SlotKind::Main { .. } => Some(&main.id),
        SlotKind::Continuation(c) => Some(&c.id),
        _ => None,
    }
}

/// Columns left of the body of the card that has the open reply box, for the width of its text.
pub fn box_lead_width(app: &App, id: &str) -> Option<usize> {
    let state = app.thread(id)?;
    let main = state.main.as_ref()?;
    let parent = app.compose()?.key.reply_to()?;
    let painter = Painter {
        ctx: render_ctx(app),
        own: None,
        width: 0,
    };
    state
        .slots(id, app.is_guest())
        .iter()
        .filter(|slot| slot_item_id(slot, main) == Some(parent))
        .find_map(|slot| painter.lead_of(slot, main, app.drafts()))
        .map(|lead| lead.iter().map(|s| s.content.width()).sum())
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
            ..ThreadRows::default()
        };
    };
    let main = match &state.main {
        Some(main) if !state.main_failed() => main,
        _ => {
            return ThreadRows {
                lines: vec![status_line(state, id, width)],
                ..ThreadRows::default()
            };
        }
    };
    let guest = matches!(app.session(), Session::Guest);
    let painter = Painter {
        ctx: render_ctx(app),
        own: match app.session() {
            Session::SignedIn(me) => Some(me.handle.as_str()),
            Session::Guest | Session::Restoring => None,
        },
        width,
    };
    let mut out = ThreadRows::default();
    out.lines.push(header(id, width));
    let mut index = 0;
    let open_box = app
        .compose()
        .filter(|c| c.surface == Surface::ReplyBox)
        .and_then(|c| c.key.reply_to().map(|parent| (c, parent)));
    for slot in state.slots(id, guest) {
        let mut lines = painter.slot_lines(&slot, main, app.drafts());
        if slot.entry.is_some() {
            if index == state.selected {
                apply_selection(&mut lines, &painter.ctx, painter.card_w());
            }
            if let Some((compose, parent)) = &open_box
                && slot_item_id(&slot, main) == Some(*parent)
                && let Some(lead) = painter.lead_of(&slot, main, app.drafts())
            {
                let (boxed, cursor) = reply_box(app, compose, &lead, usize::from(painter.card_w()));
                out.cursor = Some((out.lines.len() + lines.len() + cursor.0, cursor.1));
                out.box_end = Some(out.lines.len() + lines.len() + boxed.len() - 1);
                lines.extend(boxed);
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
