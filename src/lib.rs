pub mod app;
pub mod git_actions;
pub mod git_engine;
pub mod model;
pub mod ui;
pub mod watcher;

use app::{App, ModalEvent};
use clap::Parser;
use crossterm::{
    event::{self, Event as CEvent, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use model::{ActiveModal, ActiveView, RepoOpState, RepoTab};
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

    /// Maximum directory depth to search for git repositories (default: 5, 0 = unlimited)
    #[arg(short, long, default_value_t = 5)]
    pub depth: usize,
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

    // Start targeted background watcher
    let mut watcher = RepoWatcher::new()?;

    // Initialize terminal immediately (< 25ms startup)
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(root_path, args.max_commits, args.depth);

    let tick_rate = Duration::from_millis(150);

    loop {
        // Poll background scanner and attach watcher to any newly found repos
        let new_repos = app.poll_scanner();
        for p in new_repos {
            watcher.watch_repo(p);
        }

        app.scan_spinner_frame = app.scan_spinner_frame.wrapping_add(1);

        terminal.draw(|f| ui::render(f, &mut app))?;

        // Handle keyboard events
        if event::poll(tick_rate)? {
            if let CEvent::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    // Universal Ctrl+C exit
                    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                        break;
                    }

                    // 1. If a modal is active, route keys to the modal
                    if app.active_modal.is_some() {
                        let modal_ev = app.handle_modal_key(key.code, key.modifiers);
                        if let ModalEvent::LaunchEditor { repo_path, initial_msg } = modal_ev {
                            // Suspend terminal raw mode and leave alternate screen
                            disable_raw_mode()?;
                            execute!(std::io::stdout(), LeaveAlternateScreen)?;
                            terminal.show_cursor()?;

                            let editor_res = git_actions::open_editor_for_commit(&repo_path, &initial_msg);

                            // Restore terminal
                            enable_raw_mode()?;
                            execute!(std::io::stdout(), EnterAlternateScreen)?;
                            terminal.hide_cursor()?;
                            terminal.clear()?;

                            match editor_res {
                                Ok(msg) => {
                                    if let Some(ActiveModal::Commit { message, cursor_pos, .. }) = &mut app.active_modal {
                                        *message = msg.clone();
                                        *cursor_pos = msg.len();
                                    }
                                }
                                Err(err) => {
                                    app.set_status_message(format!("Editor error: {}", err), true);
                                }
                            }
                        }
                        continue;
                    }

                    // 2. If filtering in Fleet Overview
                    if app.is_filtering {
                        match key.code {
                            KeyCode::Esc => {
                                app.is_filtering = false;
                                app.filter_query.clear();
                                app.sync_table_states();
                            }
                            KeyCode::Enter => {
                                app.is_filtering = false;
                            }
                            KeyCode::Backspace => {
                                app.filter_query.pop();
                                app.sync_table_states();
                            }
                            KeyCode::Char(c) => {
                                app.filter_query.push(c);
                                app.selected_repo_v_idx = 0;
                                app.sync_table_states();
                            }
                            _ => {}
                        }
                        continue;
                    }

                    // 3. Navigation & Actions by View
                    match app.active_view {
                        ActiveView::FleetOverview => match key.code {
                            KeyCode::Char('q') => break,
                            KeyCode::Char('/') => app.is_filtering = true,
                            KeyCode::Char('r') => app.rescan(),
                            KeyCode::Enter => app.on_enter(),
                            KeyCode::Up | KeyCode::Char('k') => app.move_up(),
                            KeyCode::Down | KeyCode::Char('j') => app.move_down(),
                            KeyCode::PageUp | KeyCode::Char('b') => app.move_page_up(),
                            KeyCode::PageDown | KeyCode::Char('f') => app.move_page_down(),
                            KeyCode::Home | KeyCode::Char('g') => app.move_to_top(),
                            KeyCode::End | KeyCode::Char('G') => app.move_to_bottom(),
                            _ => {}
                        },
                        ActiveView::DiffViewer => match key.code {
                            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Enter => app.on_esc(),
                            KeyCode::Tab | KeyCode::Char('d') | KeyCode::Char('t') => app.toggle_diff_mode(),
                            KeyCode::Up | KeyCode::Char('k') => app.move_up(),
                            KeyCode::Down | KeyCode::Char('j') => app.move_down(),
                            KeyCode::PageUp | KeyCode::Char('b') => app.move_page_up(),
                            KeyCode::PageDown | KeyCode::Char('f') => app.move_page_down(),
                            KeyCode::Home | KeyCode::Char('g') => app.move_to_top(),
                            KeyCode::End | KeyCode::Char('G') => app.move_to_bottom(),
                            _ => {}
                        },
                        ActiveView::RepoDetail => {
                            // Clear any previous transient status message on new user key
                            app.clear_status_message();

                            match key.code {
                                KeyCode::Esc | KeyCode::Char('q') => app.on_esc(),
                                KeyCode::Char('?') => app.open_help_modal(),

                                // Tab switching: 1, 2, 3, 4, [, ]
                                KeyCode::Char('1') => app.set_repo_tab(RepoTab::Graph),
                                KeyCode::Char('2') => app.set_repo_tab(RepoTab::Status),
                                KeyCode::Char('3') => app.set_repo_tab(RepoTab::Branches),
                                KeyCode::Char('4') => app.set_repo_tab(RepoTab::Stash),
                                KeyCode::Char('[') => app.prev_repo_tab(),
                                KeyCode::Char(']') => app.next_repo_tab(),

                                // Global git remotes actions
                                KeyCode::Char('P') => app.push_repo(),
                                KeyCode::Char('p') if app.current_tab != RepoTab::Stash => app.pull_repo(),
                                KeyCode::Char('f') | KeyCode::Char('F') => app.fetch_repo(),
                                KeyCode::Char('r') => app.refresh_all_repo_data(),

                                // View-specific keybindings
                                KeyCode::Tab => app.switch_pane(),
                                KeyCode::Enter => app.on_enter(),

                                // Merge & Rebase (Tab 1 Graph & Tab 3 Branches)
                                KeyCode::Char('M') if app.current_tab == RepoTab::Graph || app.current_tab == RepoTab::Branches => {
                                    app.open_merge_modal();
                                }
                                KeyCode::Char('R') if app.current_tab == RepoTab::Graph || app.current_tab == RepoTab::Branches => {
                                    app.open_rebase_modal();
                                }
                                // Conflict handling / Abort / Continue
                                KeyCode::Char('A') if app.repo_op_state != RepoOpState::Normal && app.current_tab != RepoTab::Status => {
                                    app.abort_current_op();
                                }
                                KeyCode::Char('C') if app.repo_op_state == RepoOpState::Rebasing && app.current_tab != RepoTab::Status => {
                                    app.continue_rebase_op();
                                }

                                // Tab 1: Graph keys
                                KeyCode::Char('m') if app.current_tab == RepoTab::Graph => {
                                    app.jump_to_main();
                                }
                                KeyCode::Char('H') if app.current_tab == RepoTab::Graph => {
                                    app.jump_to_head();
                                }

                                // Tab 2: Status & Staging keys
                                KeyCode::Char(' ') if app.current_tab == RepoTab::Status => {
                                    app.toggle_stage_selected();
                                }
                                KeyCode::Char('a') if app.current_tab == RepoTab::Status => {
                                    app.stage_all();
                                }
                                KeyCode::Char('A') if app.current_tab == RepoTab::Status => {
                                    app.unstage_all();
                                }
                                KeyCode::Char('c') if app.current_tab == RepoTab::Status => {
                                    app.open_commit_modal(false);
                                }
                                KeyCode::Char('C') if app.current_tab == RepoTab::Status => {
                                    app.open_commit_modal(true);
                                }
                                KeyCode::Char('x') if app.current_tab == RepoTab::Status => {
                                    app.open_discard_confirm();
                                }

                                // Tab 3: Branches keys
                                KeyCode::Char('n') if app.current_tab == RepoTab::Branches => {
                                    app.open_new_branch_modal();
                                }
                                KeyCode::Char('d') if app.current_tab == RepoTab::Branches => {
                                    app.open_delete_branch_confirm();
                                }
                                KeyCode::Char(' ') if app.current_tab == RepoTab::Branches => {
                                    app.checkout_selected_branch();
                                }

                                // Tab 4: Stash keys
                                KeyCode::Char('s') if app.current_tab == RepoTab::Stash => {
                                    app.open_stash_save_modal();
                                }
                                KeyCode::Char('p') if app.current_tab == RepoTab::Stash => {
                                    app.pop_selected_stash();
                                }
                                KeyCode::Char('d') if app.current_tab == RepoTab::Stash => {
                                    app.open_drop_stash_confirm();
                                }

                                // Navigation
                                KeyCode::Up | KeyCode::Char('k') => app.move_up(),
                                KeyCode::Down | KeyCode::Char('j') => app.move_down(),
                                KeyCode::PageUp | KeyCode::Char('b') => app.move_page_up(),
                                KeyCode::PageDown => app.move_page_down(),
                                KeyCode::Home | KeyCode::Char('g') => app.move_to_top(),
                                KeyCode::End | KeyCode::Char('G') => app.move_to_bottom(),
                                _ => {}
                            }
                        }
                    }
                }
            }
        }

        // Process targeted file watcher events
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
