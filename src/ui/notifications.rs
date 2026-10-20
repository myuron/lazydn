use ratatui::style::Style;
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use crate::app::App;
use crate::feed::LoadState;
use crate::model::{Notification, NotificationType, ThreadItem, User};
use crate::notifications::{Filter, Filters, GroupList};
use crate::timefmt::{day_label, relative};
use crate::ui::card::{clean, clean_inline};
use crate::ui::feed::{CardSpan, center_offset, plain, retry_hint, rule, truncate};
use crate::ui::theme;

#[derive(Debug, Clone, Default)]
pub struct NotificationRows {
    /// The scrolling part; the filter rows are drawn above it by [`filter_rows`].
    pub lines: Vec<Line<'static>>,
    /// One entry per grouped row of the list, in order.
    pub cards: Vec<CardSpan>,
}

const HINT: &str = "(1-6 toggle, a all/none)";
/// Columns taken by the rail and the indent before the text of a second row.
const INDENT: usize = 6;
const REJECTED_NOTE: &str = "Only you can see this update and your rejected commit.";

fn pad_to(text: &str, width: usize) -> String {
    format!("{text}{}", " ".repeat(width.saturating_sub(text.width())))
}

/// Cuts by display width and marks the cut, unlike `truncate`, so that a clipped sentence is visible.
fn fit(text: &str, avail: usize) -> String {
    if text.width() <= avail {
        text.to_string()
    } else if avail == 0 {
        String::new()
    } else {
        format!("{}…", truncate(text, avail - 1))
    }
}

/// The fixed rows above the list: the toggles, wrapped by item, and the key hint.
pub fn filter_rows(filters: &Filters, width: u16) -> Vec<Line<'static>> {
    let limit = usize::from(width).saturating_sub(1);
    let mut rows: Vec<String> = Vec::new();
    for filter in Filter::ALL {
        let mark = if filters.is_on(filter) { 'x' } else { ' ' };
        let item = format!("[{mark}] {}", filter.label());
        match rows.last_mut() {
            Some(last) if last.width() + 2 + item.width() <= limit => {
                last.push_str("  ");
                last.push_str(&item);
            }
            _ => rows.push(format!(" {item}")),
        }
    }
    let hint_start = limit.saturating_sub(HINT.width());
    match rows.last_mut() {
        Some(last) if last.width() + 2 <= hint_start => {
            *last = format!("{}{HINT}", pad_to(last, hint_start));
        }
        _ => rows.push(format!("{}{HINT}", " ".repeat(hint_start))),
    }
    rows.into_iter()
        .map(|r| Line::raw(truncate(&r, limit)))
        .collect()
}

fn glyph(kind: NotificationType) -> &'static str {
    match kind {
        NotificationType::Starred => "★",
        NotificationType::Replied => "↩",
        NotificationType::Quoted => "❝",
        NotificationType::Mentioned => "@",
        NotificationType::Followed => "+",
        NotificationType::Approved => "✓",
        NotificationType::Rejected => "✗",
        NotificationType::Unknown => "?",
    }
}

fn action(kind: NotificationType) -> &'static str {
    match kind {
        NotificationType::Starred => "starred your commit",
        NotificationType::Replied => "replied to your commit",
        NotificationType::Quoted => "quoted your commit",
        NotificationType::Mentioned => "mentioned you",
        NotificationType::Followed => "followed you",
        NotificationType::Approved => "Your commit was approved.",
        NotificationType::Rejected => "Your commit was rejected.",
        NotificationType::Unknown => "",
    }
}

fn noun(kind: NotificationType) -> &'static str {
    match kind {
        NotificationType::Followed => "followers",
        NotificationType::Starred => "stars",
        _ => "updates",
    }
}

fn name_of(user: &User) -> String {
    clean_inline(user.name.as_deref().unwrap_or(&user.username))
}

fn phrase(n: &Notification) -> String {
    let verb = action(n.kind);
    if matches!(
        n.kind,
        NotificationType::Approved | NotificationType::Rejected
    ) {
        return verb.to_string();
    }
    let Some(actor) = &n.actor else {
        let mut chars = verb.chars();
        return chars
            .next()
            .map(|c| c.to_uppercase().chain(chars).collect())
            .unwrap_or_default();
    };
    let name = name_of(actor);
    let grouped = matches!(
        n.kind,
        NotificationType::Starred | NotificationType::Followed
    );
    if grouped && n.actors_count >= 2 {
        let others = n.actors_count - 1;
        let word = if others == 1 { "other" } else { "others" };
        format!("{name} and {others} {word} {verb}")
    } else {
        format!("{name} @{} {verb}", clean_inline(&actor.username))
    }
}

/// Whether the server had not yet marked this one read at the time of the response.
fn is_new(n: &Notification, state_read_at: Option<time::OffsetDateTime>) -> bool {
    match (n.read_at, state_read_at) {
        (Some(item), Some(top)) => item >= top,
        _ => true,
    }
}

struct Ctx {
    limit: usize,
    truecolor: bool,
}

impl Ctx {
    /// The rail and indent that start every row under a headline.
    fn sub(&self, rail: Style, content: Vec<Span<'static>>) -> Line<'static> {
        let mut spans = vec![Span::raw(" "), Span::styled("│", rail), Span::raw("    ")];
        spans.extend(content);
        Line::from(spans)
    }

    fn avail(&self) -> usize {
        self.limit.saturating_sub(INDENT)
    }

    fn text_row(&self, rail: Style, text: &str) -> Line<'static> {
        self.sub(rail, vec![Span::raw(fit(text, self.avail()))])
    }

    fn timed_row(&self, rail: Style, text: &str, time: &str) -> Line<'static> {
        let t = time.width();
        let text = fit(text, self.avail().saturating_sub(t + 1));
        let gap = self.avail().saturating_sub(text.width() + t);
        self.sub(
            rail,
            vec![Span::raw(format!("{text}{}{time}", " ".repeat(gap)))],
        )
    }

    fn boxed(&self, rail: Style, body: &str) -> Vec<Line<'static>> {
        let box_w = self.avail();
        if box_w < 6 {
            return Vec::new();
        }
        let inner = box_w - 4;
        let text = pad_to(&fit(&clean_inline(body), inner), inner);
        let bar = "─".repeat(box_w - 2);
        vec![
            self.sub(rail, vec![Span::raw(format!("┌{bar}┐"))]),
            self.sub(rail, vec![Span::raw(format!("│ {text} │"))]),
            self.sub(rail, vec![Span::raw(format!("└{bar}┘"))]),
        ]
    }

    /// At most two rows of the body; a cut after the second row is marked on that row.
    fn body_rows(&self, rail: Style, body: &str) -> Vec<Line<'static>> {
        let cleaned = clean(body);
        let mut lines = cleaned.lines();
        let shown: Vec<&str> = lines.by_ref().take(2).collect();
        let more = lines.next().is_some();
        let last = shown.len().saturating_sub(1);
        shown
            .iter()
            .enumerate()
            .map(|(i, l)| {
                let text = if more && i == last {
                    format!("{l}…")
                } else {
                    (*l).to_string()
                };
                self.text_row(rail, &text)
            })
            .collect()
    }

    fn group_rows(
        &self,
        rail: Style,
        n: &Notification,
        group: Option<&GroupList>,
        open: bool,
        now: time::OffsetDateTime,
        offset: time::UtcOffset,
    ) -> Vec<Line<'static>> {
        let word = noun(n.kind);
        let mark = if open { '⌃' } else { '⌄' };
        let mut out = vec![self.text_row(rail, &format!("{mark} {} {word}", n.event_count))];
        if !open {
            return out;
        }
        let Some(group) = group else {
            return out;
        };
        if group.pending.is_some() {
            out.push(self.text_row(rail, "┊ (loading...)"));
            return out;
        }
        if matches!(group.load, LoadState::Failed { .. }) {
            out.push(self.text_row(rail, &format!("fatal: could not load {word}")));
            return out;
        }
        let count = group.items.len();
        for (i, child) in group.items.iter().enumerate() {
            let connector = if i + 1 == count { "└─" } else { "├─" };
            let name = child.actor.as_ref().map(name_of).unwrap_or_default();
            let time = relative(child.created_at, now, offset);
            out.push(self.timed_row(rail, &format!("{connector} {name}"), &time));
        }
        let rest = (n.event_count as usize).saturating_sub(count);
        if rest > 0 {
            out.push(self.text_row(rail, &format!("┊ {rest} more not shown")));
        }
        out
    }

    fn headline(&self, n: &Notification, new: bool, time: &str) -> Line<'static> {
        let t = time.width();
        let text = fit(&phrase(n), self.limit.saturating_sub(t + 1 + INDENT));
        let gap = self.limit.saturating_sub(INDENT + text.width() + t);
        let node = if new {
            Span::styled("◉", theme::current_style())
        } else {
            Span::raw("○")
        };
        Line::from(vec![
            Span::raw(" "),
            node,
            Span::raw(format!(" {}  ", glyph(n.kind))),
            Span::raw(text),
            Span::raw(" ".repeat(gap)),
            Span::raw(time.to_string()),
        ])
    }
}

fn empty_command(filters: &Filters) -> String {
    let on: Vec<&str> = Filter::ALL
        .into_iter()
        .filter(|f| filters.is_on(*f))
        .map(Filter::name)
        .collect();
    if filters.all_on() {
        " $ cat /dev/notifications".to_string()
    } else if on.is_empty() {
        " $ cat /dev/notifications | grep -E '^$'".to_string()
    } else {
        format!(" $ cat /dev/notifications | grep -E '{}'", on.join("|"))
    }
}

pub fn rows(app: &App, width: u16) -> NotificationRows {
    let state = app.notifications();
    let w = usize::from(width);
    let ctx = Ctx {
        limit: w.saturating_sub(1),
        truecolor: app.prefs().truecolor,
    };
    let (now, offset) = (app.now(), app.local_offset());
    let mut out = NotificationRows::default();

    if state.new_count > 0 {
        let text = if state.new_count == 1 {
            "↑ 1 new notification".to_string()
        } else {
            format!("↑ {} new notifications", state.new_count)
        };
        let pad = center_offset(&text, w);
        out.lines.push(Line::from(vec![
            Span::raw(" ".repeat(pad)),
            Span::styled(
                truncate(&text, w.saturating_sub(pad)),
                theme::current_style(),
            ),
        ]));
    }

    if state.items.is_empty() {
        if state.pending.is_some() {
            out.lines.push(plain(" ┊ (loading...)", ctx.limit));
        } else if let LoadState::Failed { code, .. } = &state.load {
            out.lines.push(retry_hint(code, w));
        } else if state.loaded_once && state.next_cursor.is_none() {
            out.lines
                .push(plain(&empty_command(&state.filters), ctx.limit));
        }
        return out;
    }

    let mut seen_new = false;
    let mut head_done = false;
    let mut previous: Option<&Notification> = None;
    for n in &state.items {
        let new = is_new(n, state.read_at);
        if !new && seen_new && !head_done {
            out.lines.push(rule(" ╌╌ HEAD@{last visit} ", "╌", w));
            head_done = true;
        }
        seen_new |= new;
        let day = n.created_at.to_offset(offset).date();
        if previous.is_none_or(|p| p.created_at.to_offset(offset).date() != day) {
            let label = day_label(n.created_at, now, offset);
            out.lines.push(rule(&format!(" ╎ ── {label} "), "─", w));
        }

        let time = relative(n.created_at, now, offset);
        let rail = n
            .actor
            .as_ref()
            .map(|a| Style::new().fg(theme::lane_color(a.effective_lane(), ctx.truecolor)))
            .unwrap_or_default();
        let mut card = vec![ctx.headline(n, new, &time)];
        let own_commit = match &n.commit {
            Some(ThreadItem::Commit(c)) => Some(c.body.as_str()),
            _ => None,
        };
        match n.kind {
            NotificationType::Starred | NotificationType::Approved | NotificationType::Rejected => {
                if let Some(body) = own_commit {
                    card.extend(ctx.boxed(rail, body));
                }
            }
            NotificationType::Replied | NotificationType::Mentioned | NotificationType::Quoted => {
                if let Some(body) = own_commit {
                    card.extend(ctx.body_rows(rail, body));
                }
            }
            NotificationType::Followed | NotificationType::Unknown => {}
        }
        if n.kind == NotificationType::Rejected {
            if let Some(reason) = n.reason.as_deref().filter(|r| !r.is_empty()) {
                card.push(ctx.text_row(rail, &format!("reason: {}", clean_inline(reason))));
            }
            card.push(ctx.text_row(rail, REJECTED_NOTE));
        }
        if n.event_count > 1 {
            let open = state.expanded.contains(&n.group_id);
            card.extend(ctx.group_rows(rail, n, state.groups.get(&n.group_id), open, now, offset));
        }
        out.cards.push(CardSpan {
            start: out.lines.len(),
            height: card.len(),
        });
        out.lines.extend(card);
        previous = Some(n);
    }

    if state.pending.as_ref().is_some_and(|p| p.append) {
        out.lines.push(plain(" ┊ (loading more...)", w));
    } else if let LoadState::Failed { code, append: true } = &state.load {
        out.lines.push(retry_hint(code, w));
    }
    out
}
