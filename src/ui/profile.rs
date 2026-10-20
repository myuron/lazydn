use ratatui::style::Style;
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use crate::app::{App, ProfileTab, Session};
use crate::model::{User, UserResponse};
use crate::profile::ProfileState;
use crate::ui::card::Node;
use crate::ui::feed::{
    CardSpan, Env, list_tail, pending_or_failed_row, plain, render_ctx, retry_hint, truncate,
};
use crate::ui::theme;

#[derive(Debug, Clone, Default)]
pub struct ProfileRows {
    /// Everything below the frame title: header, tab row and list, which scroll together.
    pub lines: Vec<Line<'static>>,
    /// One entry per selectable card, a pinned commit first.
    pub cards: Vec<CardSpan>,
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const ITEM_GAP: &str = "   ";

/// The label row and the underline row of a two-tab strip; each label sits in the middle of its half.
pub(crate) fn two_tabs(labels: [&str; 2], active: usize, width: usize) -> [String; 2] {
    let edge = |i: usize| (width * i).div_ceil(2);
    let mut label_row = String::new();
    let mut underline = String::new();
    for (i, label) in labels.iter().enumerate() {
        let (start, end) = (edge(i), edge(i + 1));
        let at = start + (end - start).saturating_sub(label.width()) / 2;
        label_row.push_str(&" ".repeat(at.saturating_sub(label_row.width())));
        label_row.push_str(label);
        if i == active {
            underline.push_str(&" ".repeat(at));
            underline.push_str(&"━".repeat(label.width()));
        }
    }
    [label_row, underline]
}

fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut out = Vec::new();
    for paragraph in text.lines() {
        let mut current = String::new();
        for word in paragraph.split_whitespace() {
            let word = truncate(word, width);
            if current.is_empty() {
                current = word;
            } else if current.width() + 1 + word.width() <= width {
                current.push(' ');
                current.push_str(&word);
            } else {
                out.push(std::mem::replace(&mut current, word));
            }
        }
        if !current.is_empty() {
            out.push(current);
        }
    }
    out
}

/// Wraps whole items, so an item is never split; only one wider than a row is cut.
fn item_rows(items: &[String], limit: usize) -> Vec<String> {
    let mut rows = Vec::new();
    let mut current = String::new();
    for item in items {
        let item = if item.width() + 1 > limit {
            format!("{}…", truncate(item, limit.saturating_sub(2)))
        } else {
            item.clone()
        };
        if current.is_empty() {
            current = format!(" {item}");
        } else if current.width() + ITEM_GAP.len() + item.width() <= limit {
            current.push_str(ITEM_GAP);
            current.push_str(&item);
        } else {
            rows.push(std::mem::replace(&mut current, format!(" {item}")));
        }
    }
    if !current.is_empty() {
        rows.push(current);
    }
    rows
}

fn right_aligned(left: &str, right: &str, limit: usize) -> String {
    match limit.checked_sub(left.width() + right.width()) {
        Some(gap) if gap >= 1 => format!("{left}{}{right}", " ".repeat(gap)),
        _ => left.to_string(),
    }
}

pub(crate) fn display_name(user: &User) -> &str {
    user.name.as_deref().unwrap_or(&user.username)
}

struct Who {
    hint: Option<String>,
    badges: Vec<&'static str>,
}

fn who(app: &App, username: &str, resp: Option<&UserResponse>) -> Who {
    let none = Who {
        hint: None,
        badges: Vec::new(),
    };
    let Some(resp) = resp else { return none };
    if app.is_guest() {
        return none;
    }
    if app.is_own(username) {
        return Who {
            hint: Some("[O] GitHub".into()),
            badges: Vec::new(),
        };
    }
    let follow = if resp.is_following == Some(true) {
        "[f] Unfollow"
    } else {
        "[f] Follow"
    };
    let mute = if resp.is_muting == Some(true) {
        "[m] Unmute"
    } else {
        "[m] Mute"
    };
    let mut badges = Vec::new();
    if resp.follows_you == Some(true) {
        badges.push("[Follows you]");
    }
    if resp.is_muting == Some(true) {
        badges.push("[Muted]");
    }
    Who {
        hint: Some(format!("{follow}  {mute}  [B] Block")),
        badges,
    }
}

/// Name, handle, bio and meta: what a blocked profile keeps of the header.
fn identity(app: &App, user: &User, who: &Who, w: usize, out: &mut Vec<Line<'static>>) {
    let limit = w.saturating_sub(1);
    let left = format!(" ◉ {}", display_name(user));
    let name = match &who.hint {
        Some(hint) => right_aligned(&left, hint, limit),
        None => truncate(&left, limit),
    };
    out.push(Line::raw(name));

    let color = theme::lane_color(user.effective_lane(), app.prefs().truecolor);
    let badges: String = who.badges.iter().map(|b| format!("  {b}")).collect();
    out.push(Line::from(vec![
        Span::raw(" "),
        Span::styled("●", Style::new().fg(color)),
        Span::raw(truncate(
            &format!(" @{}{badges}", user.username),
            limit.saturating_sub(2),
        )),
    ]));
    out.push(Line::raw(""));

    let bio = wrap(user.bio.as_deref().unwrap_or(""), w.saturating_sub(2));
    if !bio.is_empty() {
        out.extend(bio.into_iter().map(|l| Line::raw(format!(" {l}"))));
        out.push(Line::raw(""));
    }

    let local = user.created_at.to_offset(app.local_offset());
    let mut items: Vec<String> = Vec::new();
    if let Some(company) = &user.company {
        items.push(format!("⌂ {company}"));
    }
    if let Some(location) = &user.location {
        items.push(format!("⚲ {location}"));
    }
    if let Some(website) = &user.website {
        items.push(format!("⛓ {website}"));
    }
    items.push(format!(
        "Joined {} {}",
        MONTHS[usize::from(local.month() as u8 - 1)],
        local.year()
    ));
    out.extend(item_rows(&items, limit).into_iter().map(Line::raw));
}

fn stats(app: &App, resp: &UserResponse, w: usize) -> Option<String> {
    let user = &resp.data;
    let mut parts: Vec<String> = Vec::new();
    if let Some(n) = user.commits_count {
        parts.push(format!("{n} commits"));
    }
    if let Some(n) = user.following_count {
        parts.push(format!("{n} following"));
    }
    if let Some(n) = user.followers_count {
        parts.push(format!("{n} followers"));
    }
    let signed = !app.is_guest();
    if let Some(n) = resp
        .pasteis_received
        .filter(|_| signed && render_ctx(app).pasteis_mode)
    {
        parts.push(format!("◓ {n} received"));
    }
    (!parts.is_empty())
        .then(|| truncate(&format!(" {}", parts.join(ITEM_GAP)), w.saturating_sub(1)))
}

fn list_rows(app: &App, state: &ProfileState, env: &Env, out: &mut ProfileRows) {
    let w = env.width;
    let signed = !app.is_guest();
    let cards = state.cards(signed);
    let feed = state.feed(state.tab);
    let has_pinned = state.pinned(signed).is_some();
    for (i, commit) in cards.iter().enumerate() {
        let mut rows = env.card_rows(
            commit,
            Node::Normal,
            i == feed.selected,
            feed.expanded.contains(&commit.id),
        );
        let card = rows.split_off(1);
        out.lines.extend(rows);
        if i == 0 && has_pinned {
            out.lines.push(plain(" ⚲ Pinned", w));
        }
        out.cards.push(CardSpan {
            start: out.lines.len(),
            height: card.len(),
        });
        out.lines.extend(card);
    }
    // Emptiness is judged without the pinned commit, which is not part of the list being loaded.
    if feed.items.is_empty() {
        let row = pending_or_failed_row(feed, w).or_else(|| {
            feed.loaded_once.then(|| {
                plain(
                    match state.tab {
                        ProfileTab::Commits => " fatal: no commits yet",
                        ProfileTab::Replies => " fatal: no replies yet",
                    },
                    w,
                )
            })
        });
        out.lines.extend(row);
    } else {
        out.lines.extend(list_tail(feed, w));
    }
}

pub fn rows(app: &App, username: &str, width: u16) -> ProfileRows {
    let w = usize::from(width);
    let mut out = ProfileRows::default();
    let Some(state) = app.profile(username) else {
        return out;
    };
    out.lines.push(Line::raw(""));

    if let Some(code) = &state.error {
        match code.as_str() {
            "user_blocked" => {
                if let Some(user) = &state.blocked_user {
                    let none = Who {
                        hint: None,
                        badges: Vec::new(),
                    };
                    identity(app, user, &none, w, &mut out.lines);
                    out.lines.push(Line::raw(""));
                }
                out.lines
                    .push(plain(&format!(" You blocked @{username}. [B] Unblock"), w));
            }
            "user_unavailable" | "http_404" => out
                .lines
                .push(plain(&format!(" fatal: @{username} is not available"), w)),
            other => out.lines.push(retry_hint(other, w)),
        }
        return out;
    }
    let Some(resp) = &state.user else {
        out.lines.push(plain(" ┊ (loading...)", w));
        return out;
    };

    let who = who(app, username, Some(resp));
    identity(app, &resp.data, &who, w, &mut out.lines);
    out.lines.push(Line::raw(""));
    if let Some(line) = stats(app, resp, w) {
        out.lines.push(Line::raw(line));
        out.lines.push(Line::raw(""));
    }
    let active = usize::from(state.tab == ProfileTab::Replies);
    for row in two_tabs(["Commits", "Replies"], active, w) {
        out.lines.push(Line::raw(row));
    }

    let render = render_ctx(app);
    let own = match app.session() {
        Session::SignedIn(me) => Some(me.handle.as_str()),
        Session::Guest | Session::Restoring => None,
    };
    let env = Env {
        ctx: render,
        own,
        width: w,
    };
    list_rows(app, state, &env, &mut out);
    out
}
