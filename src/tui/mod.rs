use crossterm::{
    event::{poll, read, DisableMouseCapture, EnableMouseCapture, Event},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use tui::{backend::CrosstermBackend, Terminal};

use std::io::stdout;

mod event;

use crate::Args;
use crate::Result;

pub(crate) async fn start_tui(_args: Args) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let events = event::Events::new();

    loop {
        match events.next() {
            event::Event::Input(input) => match input {
                _ => continue,
            },
            event::Event::Quit => break,
            event::Event::Tick => continue,
        };
    }
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor();
    Ok(())
}
