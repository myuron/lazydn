use color_eyre::Result;
use crossterm::event::{self, Event};
use lazydn::action::{Action, Effect};
use lazydn::app::{App, Session};
use lazydn::{keymap, ui};
use ratatui::DefaultTerminal;

fn main() -> Result<()> {
    color_eyre::install()?;
    // Enter raw mode and the alternate screen; restore them even if run() fails.
    let terminal = ratatui::init();
    let result = run(terminal);
    ratatui::restore();
    result
}

fn run(mut terminal: DefaultTerminal) -> Result<()> {
    let mut app = App::new(Session::Guest);
    let size = terminal.size()?;
    app.update(Action::Resize(size.width, size.height));
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
