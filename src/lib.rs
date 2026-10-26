pub mod app;
pub mod git_engine;
pub mod model;
pub mod ui;
pub mod watcher;

use app::App;
use clap::Parser;
use crossterm::{
    event::{self, Event as CEvent, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use model::ActiveView;
use ratatui::{backend::CrosstermBackend, Terminal};
use std::{
    fs,
    io::stdout,
    path::PathBuf,
    time::Duration,
};
use watcher::RepoWatcher;

#[derive(Parser, Debug)]
#[command(
    name = "gitop",
    about = "A real-time top-like terminal dashboard for git repositories with branch graph and commit inspector",
    version = "0.1.0"
)]
pub struct Args {
    /// Root directory to scan and monitor (defaults to current directory)
    #[arg(default_value = ".")]
    pub path: PathBuf,

    /// Maximum commits to display in the repo branch graph
    #[arg(short, long, default_value_t = 200)]
    pub max_commits: usize,
}

fn setup_panic_hook() {
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = disable_raw_mode();
        let _ = execute!(stdout(), LeaveAlternateScreen);
        original_hook(panic_info);
    }));
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let root_path = fs::canonicalize(&args.path)?;

    // Set panic hook so terminal is always restored
    setup_panic_hook();

    // Start background watcher
    let watcher = RepoWatcher::new(&root_path)?;

    // Initialize terminal
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(root_path, args.max_commits);

    let tick_rate = Duration::from_millis(150);

    loop {
        terminal.draw(|f| ui::render(f, &mut app))?;

        // Handle keyboard events
        if event::poll(tick_rate)? {
            if let CEvent::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    // Universal Ctrl+C exit
                    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                        break;
                    }

                    if app.is_filtering {
                        match key.code {
                            KeyCode::Esc => {
                                app.is_filtering = false;
                                app.filter_query.clear();
                            }
                            KeyCode::Enter => {
                                app.is_filtering = false;
                            }
                            KeyCode::Backspace => {
                                app.filter_query.pop();
                            }
                            KeyCode::Char(c) => {
                                app.filter_query.push(c);
                                app.selected_repo_v_idx = 0;
                            }
                            _ => {}
                        }
                    } else {
                        match key.code {
                            KeyCode::Char('q') => match app.active_view {
                                ActiveView::FleetOverview => break,
                                _ => app.on_esc(),
                            },
                            KeyCode::Esc => app.on_esc(),
                            KeyCode::Char('/') => {
                                if app.active_view == ActiveView::FleetOverview {
                                    app.is_filtering = true;
                                }
                            }
                            KeyCode::Char('r') => {
                                app.rescan();
                            }
                            KeyCode::Tab => {
                                app.switch_pane();
                            }
                            KeyCode::Char('d') | KeyCode::Char('t') => {
                                app.toggle_diff_mode();
                            }
                            KeyCode::Enter => {
                                app.on_enter();
                            }
                            KeyCode::Up | KeyCode::Char('k') => {
                                app.move_up();
                            }
                            KeyCode::Down | KeyCode::Char('j') => {
                                app.move_down();
                            }
                            KeyCode::PageUp | KeyCode::Char('b') => {
                                app.move_page_up();
                            }
                            KeyCode::PageDown | KeyCode::Char('f') => {
                                app.move_page_down();
                            }
                            KeyCode::Home | KeyCode::Char('g') => {
                                app.move_to_top();
                            }
                            KeyCode::End | KeyCode::Char('G') => {
                                app.move_to_bottom();
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        // Process file watcher events
        let mut needs_refresh = false;
        while let Ok(res) = watcher.rx.try_recv() {
            if let Ok(event) = res {
                if RepoWatcher::is_relevant_event(&event) {
                    needs_refresh = true;
                }
            }
        }

        if needs_refresh {
            app.rescan();
        }
    }

    // Clean up terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    Ok(())
}
