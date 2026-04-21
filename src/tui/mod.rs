mod app;
mod data;
mod ui;

use std::io::{self, Stdout};
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::{
    event::{self, Event},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use crate::tui::app::{AppAction, DashboardApp};

const DASHBOARD_DEFAULT_RANGE_DAYS: u32 = 7;

pub fn run_dashboard(db_path: &Path) -> Result<()> {
    let mut app = DashboardApp::load(db_path, DASHBOARD_DEFAULT_RANGE_DAYS, false)?;
    let mut terminal = init_terminal()?;
    let result = run_event_loop(&mut terminal, &mut app);
    restore_terminal(&mut terminal)?;
    result
}

fn init_terminal() -> Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.hide_cursor()?;
    Ok(terminal)
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

fn run_event_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut DashboardApp,
) -> Result<()> {
    let mut last_refresh = Instant::now();
    loop {
        terminal.draw(|frame| ui::render(frame, app))?;

        if last_refresh.elapsed() >= Duration::from_secs(5) {
            app.refresh();
            last_refresh = Instant::now();
        }

        if event::poll(Duration::from_millis(250))? {
            if let Event::Key(key) = event::read()? {
                match app.handle_key(key) {
                    AppAction::Quit => break,
                    AppAction::Refresh => {
                        app.refresh();
                        last_refresh = Instant::now();
                    }
                    AppAction::None => {}
                }
            }
        }
    }

    Ok(())
}
