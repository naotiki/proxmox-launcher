mod app;
mod command;
mod config;
mod proxmox;
mod ui;
mod viewer;

use std::{io, panic, time::Duration};

use anyhow::Result;
use app::App;
use config::Config;
use crossterm::{
    event::{self, Event, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

fn main() -> Result<()> {
    let config = Config::load();
    viewer::cleanup_temp_dir(&config)?;

    let mut terminal = setup_terminal()?;
    install_panic_restore_hook();

    let mut app = App::new(config);
    app.bootstrap();

    let result = run_app(&mut terminal, &mut app);
    restore_terminal()?;

    if let Err(error) = &result {
        eprintln!("error: {error:#}");
    }

    result
}

fn setup_terminal() -> Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    Ok(Terminal::new(backend)?)
}

fn restore_terminal() -> Result<()> {
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen)?;
    Ok(())
}

fn install_panic_restore_hook() {
    let original_hook = panic::take_hook();
    panic::set_hook(Box::new(move |panic_info| {
        let _ = restore_terminal();
        original_hook(panic_info);
    }));
}

fn run_app(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, app: &mut App) -> Result<()> {
    let tick_rate = Duration::from_millis(200);

    while !app.should_quit {
        terminal.draw(|frame| ui::draw(frame, app))?;

        if event::poll(tick_rate)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    app.handle_key(key);
                }
            }
        }

        app.tick();
    }

    Ok(())
}
