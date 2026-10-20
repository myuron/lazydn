use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use crate::app::{App, ConnTab};
use crate::feed::LoadState;
use crate::ui::card::apply_selection;
use crate::ui::feed::{CardSpan, plain, render_ctx, retry_hint, truncate};

#[derive(Debug, Clone, Default)]
pub struct ConnectionRows {
    /// The user rows only; the tab row and the blank row below it are drawn outside the scroll.
    pub lines: Vec<Line<'static>>,
    /// One entry per user, in order.
    pub users: Vec<CardSpan>,
}

const BIO_INDENT: &str = "  ";

pub fn rows(app: &App, username: &str, tab: ConnTab, width: u16) -> ConnectionRows {
    let w = usize::from(width);
    let mut out = ConnectionRows::default();
    let Some(state) = app.connections(username, tab) else {
        return out;
    };
    let ctx = render_ctx(app);
    let signed = !app.is_guest();
    let card_w = u16::try_from(w.saturating_sub(1)).unwrap_or(u16::MAX);

    if state.users.is_empty() {
        if state.pending.is_some() {
            out.lines.push(plain(" ┊ (loading...)", w));
        } else if let LoadState::Failed { code, .. } = &state.load {
            out.lines.push(retry_hint(code, w));
        } else if state.loaded_once {
            out.lines.push(plain(
                match tab {
                    ConnTab::Followers => " fatal: no followers yet",
                    ConnTab::Following => " fatal: not following anyone yet",
                },
                w,
            ));
        }
        return out;
    }

    for (i, user) in state.users.iter().enumerate() {
        let name = user.name.as_deref().unwrap_or(&user.username);
        let mut left = format!("◉ {name} @{}", user.username);
        if signed && user.follows_you == Some(true) {
            left.push_str("  [Follows you]");
        }
        let button = match user.is_followed {
            Some(true) if signed && !app.is_own(&user.username) => Some("[Following]"),
            Some(false) if signed && !app.is_own(&user.username) => Some("[ Follow ]"),
            _ => None,
        };
        // The marker column is the first span so that the selection can overwrite it.
        let limit = w.saturating_sub(1);
        let text = match button {
            Some(button) if limit >= left.width() + 1 + 1 + button.width() => {
                let gap = limit - 1 - left.width() - button.width();
                format!("{left}{}{button}", " ".repeat(gap))
            }
            _ => truncate(&left, limit.saturating_sub(1)),
        };
        let mut lines = vec![Line::from(vec![Span::raw(" "), Span::raw(text)])];
        let bio = user
            .bio
            .as_deref()
            .map(|b| b.split_whitespace().collect::<Vec<_>>().join(" "))
            .filter(|b| !b.is_empty());
        if let Some(bio) = bio {
            lines.push(Line::from(vec![
                Span::raw(" "),
                Span::raw(truncate(
                    &format!("{BIO_INDENT}{bio}"),
                    limit.saturating_sub(1),
                )),
            ]));
        }
        if i == state.selected {
            apply_selection(&mut lines, &ctx, card_w);
        }
        out.users.push(CardSpan {
            start: out.lines.len(),
            height: lines.len(),
        });
        out.lines.extend(lines);
    }

    if state.pending.as_ref().is_some_and(|p| p.append) {
        out.lines.push(plain(" ┊ (loading more...)", w));
    } else if let LoadState::Failed { code, append: true } = &state.load {
        out.lines.push(retry_hint(code, w));
    }
    out
}
