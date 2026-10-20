use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};

use crate::ui::centered;

const LINES: [&str; 21] = [
    " Navigation               Commit                          ",
    "  j / k   next / prev      r   reply                      ",
    "  Enter   open             q   quote                      ",
    "  Esc     back             o   show / hide replies        ",
    "  Tab     next pane        s   star                       ",
    "  1-3     switch tab       p   give a pastel              ",
    "  g l     log              b   save                       ",
    "  g e     trending         y   copy link                  ",
    "  g n     notifications    O   open in browser            ",
    "  g p     profile          .   more menu                  ",
    "  g s     settings         d   revert (own)               ",
    "  g v     saved            !   report                     ",
    "  g g / G top / bottom    Global                          ",
    "  @       author profile   c   write a commit             ",
    "  3 / 4   follow lists     R   refresh                    ",
    "                           ?   this help                  ",
    "                           Q   quit                       ",
    "                                                          ",
    " People        f follow   m mute   B block                ",
    " Settings      S sync   D delete   h/l change   u release ",
    " Notifications 1-6 filter   a all / none                  ",
];

pub fn render(frame: &mut Frame, area: Rect) {
    let rect = centered(area, 60, 23);
    frame.render_widget(Clear, rect);
    let block = Block::bordered()
        .border_type(BorderType::Double)
        .title(" Keyboard shortcuts ");
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    frame.render_widget(Paragraph::new(LINES.join("\n")), inner);
}
