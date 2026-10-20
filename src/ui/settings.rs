use ratatui::style::Style;
use ratatui::text::{Line, Span};
use time::{Duration, OffsetDateTime};
use unicode_width::UnicodeWidthStr;

use crate::app::{App, Me, SettingsTab, Theme};
use crate::feed::LoadState;
use crate::model::{Lane, Rgb, User};
use crate::settings::{ProfileRow, UserList, parse_hex_color, saved_lane_choice};
use crate::timefmt::relative;
use crate::ui::card::clean_inline;
use crate::ui::feed::{plain, retry_hint, truncate};
use crate::ui::theme;

const COLOR_NAMES: [&str; 6] = ["orange", "green", "violet", "teal", "rose", "ochre"];

/// Rows below the tab row and its underline; each is at most `width - 1` wide.
pub fn rows(app: &App, width: u16) -> Vec<Line<'static>> {
    let limit = usize::from(width).saturating_sub(1);
    let tab = app.settings().tab;
    if tab != SettingsTab::Appearance && matches!(app.session(), crate::app::Session::Guest) {
        return vec![plain(
            " Sign in to change these settings. [L] Sign in",
            limit,
        )];
    }
    match tab {
        SettingsTab::Profile => profile(app, limit),
        SettingsTab::Appearance => appearance(app, limit),
        SettingsTab::Safety => safety(app, limit),
    }
}

fn on_off(on: bool) -> &'static str {
    if on { "[x] on" } else { "[ ] off" }
}

/// Keeps the right part whole and gives up the left one when the width is short.
fn pair(left: &str, right: &str, limit: usize) -> Line<'static> {
    if left.width() + 1 + right.width() <= limit {
        let gap = limit - left.width() - right.width();
        return Line::raw(format!("{left}{}{right}", " ".repeat(gap)));
    }
    let room = limit.saturating_sub(right.width() + 1);
    let head = truncate(left, room);
    let gap = room - head.width();
    plain(&format!("{head}{} {right}", " ".repeat(gap)), limit)
}

fn fit(spans: Vec<Span<'static>>, limit: usize) -> Line<'static> {
    let mut left = limit;
    let mut out = Vec::new();
    for span in spans {
        let text = truncate(&span.content, left);
        left -= text.width();
        out.push(Span::styled(text, span.style));
    }
    Line::from(out)
}

// --- Profile ----------------------------------------------------------------

fn hex(rgb: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", rgb.0, rgb.1, rgb.2)
}

fn radio(on: bool) -> &'static str {
    if on { "(•) " } else { "( ) " }
}

/// Replaces the first column of a line by the mark of the focused row.
fn marked(text: &str, on: bool) -> String {
    if on {
        format!("▌{}", text.chars().skip(1).collect::<String>())
    } else {
        text.to_string()
    }
}

/// The lane that the choice under the cursor would give; a color that is not typed in full changes nothing.
fn cursor_lane(me: &Me, cursor: Option<usize>, typed: Option<&str>) -> Lane {
    match cursor {
        None => me.lane,
        Some(0) => Lane::Index(me.automatic_lane),
        Some(n @ 1..=6) => Lane::Index(n as u8 - 1),
        Some(_) => typed
            .and_then(parse_hex_color)
            .map(Lane::Custom)
            .unwrap_or(me.lane),
    }
}

fn radios(me: &Me, truecolor: bool, typed: Option<&str>) -> Vec<Vec<Span<'static>>> {
    let auto = COLOR_NAMES[usize::from(me.automatic_lane).min(5)];
    let mut items = vec![vec![Span::raw(format!(
        "{}Auto ({auto})",
        radio(me.preferred_lane.is_none())
    ))]];
    for (i, name) in COLOR_NAMES.iter().enumerate() {
        let on = me.preferred_lane == Some(Lane::Index(i as u8));
        items.push(vec![
            Span::raw(radio(on)),
            Span::styled(
                "●",
                Style::new().fg(theme::lane_color(Lane::Index(i as u8), truecolor)),
            ),
            Span::raw(format!(" {name}")),
        ]);
    }
    let saved_custom = matches!(me.preferred_lane, Some(Lane::Custom(_)));
    let shown = match (typed, me.preferred_lane) {
        (Some(digits), _) => format!("#{digits}{}", "_".repeat(6 - digits.len().min(6))),
        (None, Some(Lane::Custom(rgb))) => hex(rgb),
        (None, _) => "#______".to_string(),
    };
    let custom = (saved_custom, shown);
    items.push(vec![Span::raw(format!(
        "{}custom {}",
        radio(custom.0),
        custom.1
    ))]);
    items
}

fn spans_width(spans: &[Span<'static>]) -> usize {
    spans.iter().map(Span::width).sum()
}

/// Dates stand for themselves; only the relative forms read as "ago".
fn synced_text(t: OffsetDateTime, app: &App) -> String {
    let age = app.now() - t;
    if age < Duration::minutes(1) {
        return "synced just now".to_string();
    }
    let shown = relative(t, app.now(), app.local_offset());
    if age <= Duration::days(7) {
        format!("synced {shown} ago")
    } else {
        format!("synced {shown}")
    }
}

fn profile(app: &App, limit: usize) -> Vec<Line<'static>> {
    let crate::app::Session::SignedIn(me) = app.session() else {
        return Vec::new();
    };
    let synced = match me.github_synced_at {
        Some(t) => synced_text(t, app),
        None => "never synced".to_string(),
    };
    let state = app.settings();
    let row = state.profile_row;
    let typed = state.hex_input.as_deref();
    let mirrored = format!(" Profile mirrored from GitHub   ({synced})");
    let mut out = Vec::new();
    if mirrored.width() + "   [S] Sync".width() <= limit {
        out.push(plain(
            &marked(&format!("{mirrored}   [S] Sync"), row == ProfileRow::Sync),
            limit,
        ));
    } else {
        out.push(plain(&marked(&mirrored, row == ProfileRow::Sync), limit));
        out.push(plain("  [S] Sync", limit));
    }
    out.push(Line::raw(""));
    out.push(plain(
        &marked(" Lane color", row == ProfileRow::Lane),
        limit,
    ));

    let cursor = state
        .lane_cursor
        .unwrap_or_else(|| saved_lane_choice(me.preferred_lane));
    let mut current: Vec<Span<'static>> = Vec::new();
    for (i, item) in radios(me, app.prefs().truecolor, typed)
        .into_iter()
        .enumerate()
    {
        let joined = spans_width(&current) + 2 + spans_width(&item);
        if !current.is_empty() && joined > limit {
            out.push(fit(std::mem::take(&mut current), limit));
        }
        let gap = if row == ProfileRow::Lane && i == cursor {
            " ▌"
        } else {
            "  "
        };
        current.push(Span::raw(gap));
        current.extend(item);
    }
    out.push(fit(current, limit));

    let shown = cursor_lane(me, state.lane_cursor, typed);
    let lane_style = Style::new().fg(theme::lane_color(shown, app.prefs().truecolor));
    out.push(fit(
        vec![
            Span::raw("  preview:  "),
            Span::styled("◉──●──●", lane_style),
            Span::raw(format!("  {}", clean_inline(&me.handle))),
        ],
        limit,
    ));
    out.push(Line::raw(""));
    out.push(pair(
        &marked(" Pasteis mode", row == ProfileRow::Pasteis),
        on_off(me.pasteis_mode),
        limit,
    ));
    // Why not `pasteis_resets_at`: the plan fixes the reset time as plain text.
    out.push(plain("  10 per day, resets 00:00 UTC", limit));
    out.push(Line::raw(""));
    out.push(plain(" Danger zone", limit));
    out.push(plain(
        &marked("  [D] Delete account", row == ProfileRow::Delete),
        limit,
    ));
    out
}

// --- Appearance -------------------------------------------------------------

fn appearance(app: &App, limit: usize) -> Vec<Line<'static>> {
    let prefs = app.prefs();
    let theme_name = match prefs.theme {
        Theme::Dark => "dark",
        Theme::Light => "light",
    };
    vec![
        pair(" Theme", theme_name, limit),
        pair(" Collapse nav", on_off(prefs.collapse_nav), limit),
        pair(" Lane dimming", on_off(prefs.lane_dimming), limit),
        pair(
            " Reply previews",
            &prefs.reply_preview_limit.to_string(),
            limit,
        ),
        pair(" ASCII mode", on_off(prefs.ascii), limit),
    ]
}

// --- Safety -----------------------------------------------------------------

fn user_row(user: &User, button: &str, limit: usize, selected: Option<Style>) -> Line<'static> {
    let name = clean_inline(user.name.as_deref().unwrap_or(&user.username));
    let mark = if selected.is_some() { '▌' } else { ' ' };
    let line = pair(
        &format!("{mark} ◉ {name} @{}", clean_inline(&user.username)),
        button,
        limit,
    );
    match selected {
        Some(style) => line.style(style),
        None => line,
    }
}

fn section(
    out: &mut Vec<Line<'static>>,
    heading: &str,
    list: &UserList,
    button: &str,
    empty: &str,
    limit: usize,
    selected: Option<(usize, Style)>,
) {
    out.push(plain(heading, limit));
    if list.users.is_empty() {
        if list.pending.is_some() {
            out.push(plain(" ┊ (loading...)", limit));
        } else if let LoadState::Failed { code, .. } = &list.load {
            out.push(retry_hint(code, limit));
        } else if list.loaded_once {
            out.push(plain(empty, limit));
        }
        return;
    }
    out.extend(list.users.iter().enumerate().map(|(i, u)| {
        let style = selected.filter(|(at, _)| *at == i).map(|(_, s)| s);
        user_row(u, button, limit, style)
    }));
    if list.next_cursor.is_some() {
        out.push(plain(" ┊ more not shown", limit));
    }
}

fn safety(app: &App, limit: usize) -> Vec<Line<'static>> {
    let state = app.settings();
    let cursor = Style::new().bg(theme::cursor_bg(app.prefs().truecolor));
    let muted_len = state.muted.users.len();
    let at = state.safety_selected;
    let mut out = Vec::new();
    section(
        &mut out,
        " Muted",
        &state.muted,
        "[u] Unmute",
        "  No muted users.",
        limit,
        (at < muted_len).then_some((at, cursor)),
    );
    out.push(Line::raw(""));
    section(
        &mut out,
        " Blocked",
        &state.blocked,
        "[u] Unblock",
        "  No blocked users.",
        limit,
        (at >= muted_len).then(|| (at - muted_len, cursor)),
    );
    out
}
