use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Paragraph};

use crate::app::{App, ConnState, Me, Session};
use crate::ui::theme;

fn boxed(
    frame: &mut Frame,
    area: Rect,
    y: &mut u16,
    height: u16,
    title: &str,
    text: impl Into<Text<'static>>,
) {
    let bottom = area.y + area.height;
    let top = area.y + *y;
    *y += height;
    if top >= bottom {
        return;
    }
    let rect = Rect::new(area.x, top, area.width, height.min(bottom - top));
    let block = Block::bordered().title(format!(" {title} "));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    frame.render_widget(Paragraph::new(text), inner);
}

fn me_text(me: &Me, truecolor: bool) -> Text<'static> {
    let dot = Style::new().fg(theme::lane_color(me.lane, truecolor));
    let head = vec![
        Line::raw(format!(" ◉ {}", me.name)),
        Line::from(vec![
            Span::raw(" "),
            Span::styled("●", dot),
            Span::raw(format!(" @{}", me.handle)),
        ]),
    ];
    let rest = format!(
        "\n {} commits\n {} following  {} followers\n ◓ {}/{} pasteis left\n\n {}",
        me.commits, me.following, me.followers, me.pasteis_left, me.pasteis_max, me.bio
    );
    let mut lines = head;
    lines.extend(rest.split('\n').map(|l| Line::raw(l.to_string())));
    Text::from(lines)
}

pub fn render(app: &App, frame: &mut Frame, area: Rect) {
    let mut y = 0;
    match app.session() {
        Session::Restoring => {
            let text = if app.conn_state() == ConnState::Offline {
                " offline — retrying"
            } else {
                " restoring session…"
            };
            boxed(frame, area, &mut y, 3, "Session", text);
        }
        Session::Guest => boxed(
            frame,
            area,
            &mut y,
            7,
            "New to /dev/null?",
            " Sign in with an SSH key\n to commit, star and\n follow developers.\n\n [L] Sign in",
        ),
        Session::SignedIn(me) => {
            boxed(
                frame,
                area,
                &mut y,
                10,
                "Me",
                me_text(me, app.prefs().truecolor),
            );
            let keys = crate::ui::hints::keys_rows(app)
                .unwrap_or_default()
                .join("\n");
            boxed(frame, area, &mut y, 6, "Keys", keys);
        }
    }
}
