use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Paragraph};

use crate::app::{App, ConnState, Me, Session};

const KEYS: &str =
    " j/k  move     ↵  open\n c    commit   r  reply\n s    star     o  replies\n ?    all keys";

fn boxed(frame: &mut Frame, area: Rect, y: &mut u16, height: u16, title: &str, text: String) {
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

fn me_text(me: &Me) -> String {
    format!(
        " ◉ {}\n ● @{}\n\n {} commits\n {} following  {} followers\n ◓ {}/{} pasteis left\n\n {}",
        me.name,
        me.handle,
        me.commits,
        me.following,
        me.followers,
        me.pasteis_left,
        me.pasteis_max,
        me.bio
    )
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
            boxed(frame, area, &mut y, 3, "Session", text.into());
        }
        Session::Guest => boxed(
            frame,
            area,
            &mut y,
            7,
            "New to /dev/null?",
            " Sign in with an SSH key\n to commit, star and\n follow developers.\n\n [L] Sign in"
                .into(),
        ),
        Session::SignedIn(me) => {
            boxed(frame, area, &mut y, 10, "Me", me_text(me));
            boxed(frame, area, &mut y, 6, "Keys", KEYS.into());
        }
    }
}
