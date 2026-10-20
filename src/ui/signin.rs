use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};

use crate::ui::centered;

const LINES: [&str; 6] = [
    "",
    " 1. Key            – not created",
    " 2. Approve        (pending)",
    " 3. Code           ________________________",
    "",
    "                   Enter submit    Esc continue as guest",
];

pub fn render(frame: &mut Frame, area: Rect) {
    let rect = centered(area, 64, 8);
    frame.render_widget(Clear, rect);
    let block = Block::bordered()
        .border_type(BorderType::Double)
        .title(" Sign in to /dev/null ");
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    frame.render_widget(Paragraph::new(LINES.join("\n")), inner);
}
