use color_eyre::Result;
use crossterm::event::{self, Event};
use lazydn::action::{Action, Effect};
use lazydn::app::{App, Session};
use lazydn::{keymap, ui};
use ratatui::DefaultTerminal;
use time::{OffsetDateTime, UtcOffset};

fn main() -> Result<()> {
    // Must run before any thread exists; the time crate refuses to read the offset afterwards.
    let local_offset = UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC);
    color_eyre::install()?;
    // Enter raw mode and the alternate screen; restore them even if run() fails.
    let terminal = ratatui::init();
    let result = run(terminal, local_offset);
    ratatui::restore();
    result
}

fn run(mut terminal: DefaultTerminal, local_offset: UtcOffset) -> Result<()> {
    let mut app = App::new(Session::Guest, OffsetDateTime::now_utc(), local_offset);
    let size = terminal.size()?;
    app.update(Action::Resize(size.width, size.height));
    // TODO(M5): execute the effects returned here; until then no data is fetched.
    app.update(Action::Init);
    loop {
        terminal.draw(|frame| ui::draw(&app, frame))?;
        let action = match event::read()? {
            Event::Key(key) => keymap::map_key(&app, key),
            Event::Resize(w, h) => Some(Action::Resize(w, h)),
            _ => None,
        };
        if let Some(action) = action
            && app.update(action).contains(&Effect::Quit)
        {
            return Ok(());
        }
    }
}
