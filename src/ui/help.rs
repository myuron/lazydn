use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};

use crate::ui::centered;

const LINES: [&str; 15] = [
    " Navigation               Commit                          ",
    "  j / k   next / prev      r   reply                      ",
    "  Enter   open             q   quote                      ",
    "  Esc     back             o   show / hide replies        ",
    "  Tab     next pane        s   star                       ",
    "  1-3     switch tab       p   give a pastel              ",
    "  g l     log              b   save                       ",
    "  g e     trending         y   copy link                  ",
    "  g n     notifications    .   more menu                  ",
    "  g p     profile          d   revert (own)               ",
    "  g s     settings        Global                          ",
    "  g v     saved            c   write a commit             ",
    "  g g / G top / bottom     R   refresh                    ",
    "  @       author profile   ?   this help                  ",
    "  3 / 4   follow lists     Q   quit                       ",
];

pub fn render(frame: &mut Frame, area: Rect) {
    let rect = centered(area, 60, 17);
    frame.render_widget(Clear, rect);
    let block = Block::bordered()
        .border_type(BorderType::Double)
        .title(" Keyboard shortcuts ");
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    frame.render_widget(Paragraph::new(LINES.join("\n")), inner);
}
