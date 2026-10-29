use crate::app::App;
use crate::model::{
    ActiveModal, ActiveView, CommitRef, ConfirmAction, DiffLineKind, DiffViewerState,
    MergeRebaseOp, RefKind, RepoOpState, RepoTab, StatusSection,
};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, Wrap},
    Frame,
};

pub fn render(f: &mut Frame, app: &mut App) {
    match app.active_view {
        ActiveView::FleetOverview => render_fleet_overview(f, app, f.area()),
        ActiveView::RepoDetail => render_repo_detail(f, app, f.area()),
        ActiveView::DiffViewer => {
            render_repo_detail(f, app, f.area());
            render_diff_modal(f, app, f.area());
        }
    }

    if let Some(modal) = &app.active_modal {
        render_active_modal(f, modal, app, f.area());
    }
}

// =========================================================================
// 1. FLEET OVERVIEW
// =========================================================================

fn render_fleet_overview(f: &mut Frame, app: &mut App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Min(5),    // Table
            Constraint::Length(1), // Footer / Filter
        ])
        .split(area);

    // Header with animated scanning indicator
    let dirty_count = app.repos.iter().filter(|r| r.is_dirty).count();
    let spinner_chars = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    let spinner = spinner_chars[app.scan_spinner_frame % spinner_chars.len()];

    let mut header_text = vec![
        Span::styled(" gitop ", Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw("  "),
    ];

    if app.is_scanning {
        header_text.push(Span::styled(format!("{} Scanning... ", spinner), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)));
        header_text.push(Span::styled(format!("(Found: {}) ", app.repos.len()), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)));
        header_text.push(Span::raw("│ "));
    } else {
        header_text.push(Span::styled(format!("Total Repos: {} ", app.repos.len()), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)));
        header_text.push(Span::raw("│ "));
    }

    header_text.push(Span::styled(format!("Dirty: {} ", dirty_count), if dirty_count > 0 { Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD) } else { Style::default().fg(Color::Green) }));
    header_text.push(Span::raw("│ "));
    header_text.push(Span::styled(format!("Root: {} (depth: {})", app.root_path.to_string_lossy(), if app.max_depth == 0 { "unlimited".to_string() } else { app.max_depth.to_string() }), Style::default().fg(Color::DarkGray)));

    let header_widget = Paragraph::new(Line::from(header_text))
        .block(Block::default().borders(Borders::ALL).title(" Fleet Overview "));
    f.render_widget(header_widget, chunks[0]);

    // Table
    let header_row = Row::new(vec![
        "REPOSITORY / PATH",
        "BRANCH",
        "LAST COMMIT",
        "STATUS",
        "SYNC",
        "LAST MESSAGE",
    ])
    .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
    .bottom_margin(1);

    let visible_indices = app.visible_repo_indices();
    let rows: Vec<Row> = visible_indices
        .iter()
        .map(|&real_idx| {
            let r = &app.repos[real_idx];

            let name_cell = Span::raw(r.rel_path.clone());
            let branch_cell = Span::styled(r.current_branch.clone(), Style::default().fg(Color::Green));
            let time_cell = Span::styled(r.formatted_time(), Style::default().fg(Color::Yellow));
            
            let status_style = if r.is_dirty {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            let status_cell = Span::styled(r.status_badge(), status_style);

            let sync_style = if r.ahead > 0 || r.behind > 0 {
                Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Green)
            };
            let sync_cell = Span::styled(r.sync_badge(), sync_style);

            let msg_cell = Span::styled(r.last_commit_message.clone(), Style::default().fg(Color::Gray));

            Row::new(vec![
                name_cell,
                branch_cell,
                time_cell,
                status_cell,
                sync_cell,
                msg_cell,
            ])
        })
        .collect();

    let widths = [
        Constraint::Percentage(25),
        Constraint::Length(18),
        Constraint::Length(14),
        Constraint::Length(12),
        Constraint::Length(8),
        Constraint::Percentage(40),
    ];

    let table = Table::new(rows, widths)
        .header(header_row)
        .block(Block::default().borders(Borders::ALL).title(" Monitored Git Repositories (Sorted by Latest Commit) "))
        .row_highlight_style(
            Style::default()
                .bg(Color::Rgb(40, 50, 70))
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");

    f.render_stateful_widget(table, chunks[1], &mut app.fleet_table_state);

    // Footer
    let footer_text = if app.is_filtering {
        vec![
            Span::styled(" FILTER: ", Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::raw(" "),
            Span::styled(&app.filter_query, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled("█", Style::default().fg(Color::Yellow)),
            Span::styled("  [Esc] Clear & Exit", Style::default().fg(Color::DarkGray)),
        ]
    } else {
        vec![
            Span::styled("[Enter] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw("Inspect Repo  "),
            Span::styled("[/] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw("Filter  "),
            Span::styled("[r] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw("Rescan  "),
            Span::styled("[↑/↓/j/k] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw("Navigate  "),
            Span::styled("[q] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw("Quit"),
        ]
    };

    let footer_widget = Paragraph::new(Line::from(footer_text));
    f.render_widget(footer_widget, chunks[2]);
}

// =========================================================================
// 2. REPO DETAIL & TABS
// =========================================================================

fn render_repo_detail(f: &mut Frame, app: &mut App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Length(3), // Tab switcher bar
            Constraint::Min(5),    // Main Content
            Constraint::Length(2), // Status message + Keybinding footer
        ])
        .split(area);

    let repo_name = app.current_selected_repo_name().to_string();
    let current_branch = app.branches.iter().find(|b| b.is_current).map(|b| b.name.as_str()).unwrap_or("HEAD");
    let dirty_total = app.repo_status.total_count();

    let status_badge = if dirty_total > 0 {
        Span::styled(format!("* dirty ({})", dirty_total), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
    } else {
        Span::styled("clean", Style::default().fg(Color::Green))
    };

    let mut header_spans = vec![
        Span::styled(" GIT CLIENT ", Style::default().fg(Color::Black).bg(Color::Green).add_modifier(Modifier::BOLD)),
        Span::raw("  "),
        Span::styled(repo_name, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        Span::raw(" │ Branch: "),
        Span::styled(current_branch, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw(" │ Status: "),
        status_badge,
    ];

    match app.repo_op_state {
        RepoOpState::Normal => {}
        RepoOpState::Merging => {
            header_spans.push(Span::raw(" │ "));
            header_spans.push(Span::styled(
                " ⚠ MERGE CONFLICT (Press 'A' to Abort) ",
                Style::default().fg(Color::White).bg(Color::Red).add_modifier(Modifier::BOLD),
            ));
        }
        RepoOpState::Rebasing => {
            header_spans.push(Span::raw(" │ "));
            header_spans.push(Span::styled(
                " ⚠ REBASING ('A' to Abort, 'C' to Continue) ",
                Style::default().fg(Color::White).bg(Color::Red).add_modifier(Modifier::BOLD),
            ));
        }
    }

    let header_widget = Paragraph::new(Line::from(header_spans))
        .block(Block::default().borders(Borders::ALL).title(" Repository Inspector "));
    f.render_widget(header_widget, chunks[0]);

    // Tab Bar
    let tabs = [
        (RepoTab::Graph, format!(" [1] Log & Graph ({}) ", app.graph_rows.len())),
        (RepoTab::Status, format!(" [2] Status & Staging ({}) ", app.repo_status.total_count())),
        (RepoTab::Branches, format!(" [3] Branches ({}) ", app.branches.len())),
        (RepoTab::Stash, format!(" [4] Stashes ({}) ", app.stashes.len())),
    ];

    let mut tab_spans = Vec::new();
    for (tab, label) in tabs {
        if app.current_tab == tab {
            tab_spans.push(Span::styled(
                label,
                Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD),
            ));
        } else {
            tab_spans.push(Span::styled(
                label,
                Style::default().fg(Color::Gray).bg(Color::Rgb(30, 35, 45)),
            ));
        }
        tab_spans.push(Span::raw("  "));
    }
    tab_spans.push(Span::styled("  [1-4 / [ / ] to switch tabs]", Style::default().fg(Color::DarkGray)));

    let tab_bar_widget = Paragraph::new(Line::from(tab_spans))
        .block(Block::default().borders(Borders::ALL).title(" Views "));
    f.render_widget(tab_bar_widget, chunks[1]);

    // Main Content Chunk
    match app.current_tab {
        RepoTab::Graph => render_tab_graph(f, app, chunks[2]),
        RepoTab::Status => render_tab_status(f, app, chunks[2]),
        RepoTab::Branches => render_tab_branches(f, app, chunks[2]),
        RepoTab::Stash => render_tab_stash(f, app, chunks[2]),
    }

    // Footer & Status message
    let footer_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1)])
        .split(chunks[3]);

    if let Some((msg, is_error)) = &app.status_message {
        let style = if *is_error {
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
        };
        let msg_widget = Paragraph::new(Line::from(Span::styled(format!(" ▶ {}", msg), style)));
        f.render_widget(msg_widget, footer_chunks[0]);
    } else {
        let hint_line = match app.current_tab {
            RepoTab::Graph => "Tab 1: Log & Graph (Primary Monitor View)",
            RepoTab::Status => "Tab 2: Interactive Staging & Working Tree",
            RepoTab::Branches => "Tab 3: Branch Switching & Management",
            RepoTab::Stash => "Tab 4: Stash Stack",
        };
        let msg_widget = Paragraph::new(Line::from(Span::styled(format!(" {}", hint_line), Style::default().fg(Color::DarkGray))));
        f.render_widget(msg_widget, footer_chunks[0]);
    }

    let footer_text = match app.current_tab {
        RepoTab::Graph => {
            let mut items = vec![
                Span::styled("[Enter] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::raw("Diff  "),
                Span::styled("[Tab] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::raw("Focus  "),
                Span::styled("[m] ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::raw("Main  "),
                Span::styled("[M] ", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
                Span::raw("Merge  "),
                Span::styled("[R] ", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
                Span::raw("Rebase  "),
            ];
            if app.repo_op_state != RepoOpState::Normal {
                items.push(Span::styled("[A] ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)));
                items.push(Span::raw("Abort  "));
                if app.repo_op_state == RepoOpState::Rebasing {
                    items.push(Span::styled("[C] ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)));
                    items.push(Span::raw("Continue  "));
                }
            }
            items.extend(vec![
                Span::styled("[P/p/f] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::raw("Remotes  "),
                Span::styled("[?] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::raw("Help  "),
                Span::styled("[Esc/q] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::raw("Back"),
            ]);
            items
        }
        RepoTab::Status => vec![
            Span::styled("[Space] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw("Stage/Unstage  "),
            Span::styled("[a/A] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw("Stage/Unstage All  "),
            Span::styled("[c] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw("Commit  "),
            Span::styled("[Enter] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw("Full Diff  "),
            Span::styled("[x] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw("Discard  "),
            Span::styled("[Tab] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw("Section  "),
            Span::styled("[?] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw("Help"),
        ],
        RepoTab::Branches => {
            let mut items = vec![
                Span::styled("[Enter/Space] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::raw("Checkout  "),
                Span::styled("[M] ", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
                Span::raw("Merge  "),
                Span::styled("[R] ", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
                Span::raw("Rebase  "),
                Span::styled("[n] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::raw("New  "),
                Span::styled("[d] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::raw("Delete  "),
            ];
            if app.repo_op_state != RepoOpState::Normal {
                items.push(Span::styled("[A] ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)));
                items.push(Span::raw("Abort  "));
            }
            items.extend(vec![
                Span::styled("[P/p/f] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::raw("Remotes  "),
                Span::styled("[?] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::raw("Help"),
            ]);
            items
        }
        RepoTab::Stash => vec![
            Span::styled("[s] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw("Save Stash  "),
            Span::styled("[p] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw("Pop Stash  "),
            Span::styled("[d] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw("Drop Stash  "),
            Span::styled("[Enter] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw("Diff  "),
            Span::styled("[?] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw("Help"),
        ],
    };

    let footer_widget = Paragraph::new(Line::from(footer_text));
    f.render_widget(footer_widget, footer_chunks[1]);
}

pub fn render_ref_badge<'a>(r: &'a CommitRef) -> Span<'a> {
    match &r.kind {
        RefKind::Head(_) => Span::styled(
            format!("[{}]", r.display_text),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        RefKind::Mainline(_) => Span::styled(
            format!("[{}]", r.display_text),
            Style::default()
                .fg(Color::Black)
                .bg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        RefKind::LocalBranch(_) => Span::styled(
            format!("[{}]", r.display_text),
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        RefKind::Tag(_) => Span::styled(
            format!("[🏷 {}]", r.display_text),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        RefKind::RemoteBranch(_) => Span::styled(
            format!("[{}]", r.display_text),
            Style::default()
                .fg(Color::Magenta),
        ),
    }
}

// -------------------------------------------------------------------------
// Tab 1: Log & Commit Graph
// -------------------------------------------------------------------------

fn render_tab_graph(f: &mut Frame, app: &mut App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(62), // Graph & Commits
            Constraint::Percentage(38), // Details & Files
        ])
        .split(area);

    let graph_rows: Vec<Row> = app
        .graph_rows
        .iter()
        .map(|row| {
            let prefix_span = Span::styled(row.graph_prefix.clone(), Style::default().fg(Color::Cyan));
            let hash_span = Span::styled(row.hash.clone(), Style::default().fg(Color::Yellow));
            
            let mut summary_spans = Vec::new();
            for r in &row.parsed_refs {
                summary_spans.push(render_ref_badge(r));
                summary_spans.push(Span::raw(" "));
            }
            summary_spans.push(Span::styled(row.summary.clone(), Style::default().fg(Color::White)));
            let summary_cell = Line::from(summary_spans);

            let author_span = Span::styled(row.author.clone(), Style::default().fg(Color::LightBlue));
            let date_span = Span::styled(row.relative_time.clone(), Style::default().fg(Color::DarkGray));

            Row::new(vec![
                Cell::from(prefix_span),
                Cell::from(hash_span),
                Cell::from(summary_cell),
                Cell::from(author_span),
                Cell::from(date_span),
            ])
        })
        .collect();

    let graph_header = Row::new(vec!["GRAPH", "HASH", "COMMIT SUMMARY & REFS", "AUTHOR", "DATE"])
        .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD));

    let graph_widths = [
        Constraint::Length(12),
        Constraint::Length(9),
        Constraint::Percentage(55),
        Constraint::Length(16),
        Constraint::Length(14),
    ];

    let graph_table = Table::new(graph_rows, graph_widths)
        .header(graph_header)
        .block(Block::default().borders(Borders::ALL).title(" Git Graph (Topological) "))
        .row_highlight_style(
            Style::default()
                .bg(Color::Rgb(40, 50, 70))
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");

    f.render_stateful_widget(graph_table, chunks[0], &mut app.graph_table_state);

    // Bottom Split: Commit Info Left 55%, Files Right 45%
    let bottom_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(chunks[1]);

    if let Some(detail) = &app.commit_detail {
        let mut info_lines = vec![
            Line::from(vec![
                Span::styled("Commit:  ", Style::default().fg(Color::Yellow)),
                Span::raw(&detail.hash),
            ]),
            Line::from(vec![
                Span::styled("Author:  ", Style::default().fg(Color::Cyan)),
                Span::raw(&detail.author),
            ]),
            Line::from(vec![
                Span::styled("Date:    ", Style::default().fg(Color::Green)),
                Span::raw(&detail.date),
            ]),
        ];

        if !detail.refs.is_empty() {
            let mut ref_spans = vec![
                Span::styled("Refs:    ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            ];
            for r in &detail.refs {
                ref_spans.push(render_ref_badge(r));
                ref_spans.push(Span::raw(" "));
            }
            info_lines.push(Line::from(ref_spans));
        }

        info_lines.push(Line::from(""));
        info_lines.push(Line::from(Span::styled(&detail.full_message, Style::default().fg(Color::White))));

        let info_widget = Paragraph::new(info_lines)
            .block(Block::default().borders(Borders::ALL).title(" Commit Message "))
            .wrap(Wrap { trim: false });
        f.render_widget(info_widget, bottom_chunks[0]);

        // Changed Files List
        let file_rows: Vec<Row> = detail
            .files
            .iter()
            .map(|file| {
                let status_style = match file.status.as_str() {
                    "A" => Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
                    "D" => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                    _ => Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                };

                let status_span = Span::styled(format!("[{}]", file.status), status_style);
                let path_span = Span::raw(format!(" {}", file.path));

                Row::new(vec![status_span, path_span])
            })
            .collect();

        let files_title = if app.pane_focus_files {
            " Changed Files [FOCUSED - Tab to switch] "
        } else {
            " Changed Files [Tab to focus] "
        };

        let files_table = Table::new(file_rows, [Constraint::Length(5), Constraint::Percentage(90)])
            .block(Block::default().borders(Borders::ALL).title(files_title))
            .row_highlight_style(
                Style::default()
                    .bg(Color::Rgb(40, 50, 70))
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("▶ ");

        f.render_stateful_widget(files_table, bottom_chunks[1], &mut app.files_table_state);
    } else {
        let empty_widget = Paragraph::new("No commit selected")
            .block(Block::default().borders(Borders::ALL).title(" Commit Details "));
        f.render_widget(empty_widget, bottom_chunks[0]);
    }
}

// -------------------------------------------------------------------------
// Tab 2: Status & Staging
// -------------------------------------------------------------------------

fn render_tab_status(f: &mut Frame, app: &mut App, area: Rect) {
    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(42), Constraint::Percentage(58)])
        .split(area);

    // Left: Staged, Unstaged, Untracked sections
    let left_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(33),
            Constraint::Percentage(37),
            Constraint::Percentage(30),
        ])
        .split(main_chunks[0]);

    // 1. Staged
    let is_staged_focused = app.status_section == StatusSection::Staged;
    let staged_rows: Vec<Row> = app
        .repo_status
        .staged
        .iter()
        .map(|item| {
            let b = Span::styled(format!("[{}]", item.kind.badge()), Style::default().fg(Color::Green).add_modifier(Modifier::BOLD));
            let p = Span::styled(format!(" {}", item.path), Style::default().fg(Color::White));
            Row::new(vec![b, p])
        })
        .collect();

    let staged_title = format!(" Staged Changes ({}) [Space to unstage] ", app.repo_status.staged.len());
    let staged_block = Block::default()
        .borders(Borders::ALL)
        .border_style(if is_staged_focused { Style::default().fg(Color::Green).add_modifier(Modifier::BOLD) } else { Style::default().fg(Color::DarkGray) })
        .title(staged_title);

    let staged_table = Table::new(staged_rows, [Constraint::Length(5), Constraint::Percentage(90)])
        .block(staged_block)
        .row_highlight_style(
            Style::default()
                .bg(Color::Rgb(30, 60, 40))
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");
    f.render_stateful_widget(staged_table, left_chunks[0], &mut app.staged_table_state);

    // 2. Unstaged
    let is_unstaged_focused = app.status_section == StatusSection::Unstaged;
    let unstaged_rows: Vec<Row> = app
        .repo_status
        .unstaged
        .iter()
        .map(|item| {
            let b = Span::styled(format!("[{}]", item.kind.badge()), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
            let p = Span::styled(format!(" {}", item.path), Style::default().fg(Color::White));
            Row::new(vec![b, p])
        })
        .collect();

    let unstaged_title = format!(" Unstaged Changes ({}) [Space to stage] ", app.repo_status.unstaged.len());
    let unstaged_block = Block::default()
        .borders(Borders::ALL)
        .border_style(if is_unstaged_focused { Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD) } else { Style::default().fg(Color::DarkGray) })
        .title(unstaged_title);

    let unstaged_table = Table::new(unstaged_rows, [Constraint::Length(5), Constraint::Percentage(90)])
        .block(unstaged_block)
        .row_highlight_style(
            Style::default()
                .bg(Color::Rgb(60, 50, 30))
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");
    f.render_stateful_widget(unstaged_table, left_chunks[1], &mut app.unstaged_table_state);

    // 3. Untracked
    let is_untracked_focused = app.status_section == StatusSection::Untracked;
    let untracked_rows: Vec<Row> = app
        .repo_status
        .untracked
        .iter()
        .map(|item| {
            let b = Span::styled("[?]", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD));
            let p = Span::styled(format!(" {}", item.path), Style::default().fg(Color::White));
            Row::new(vec![b, p])
        })
        .collect();

    let untracked_title = format!(" Untracked Files ({}) [Space to track] ", app.repo_status.untracked.len());
    let untracked_block = Block::default()
        .borders(Borders::ALL)
        .border_style(if is_untracked_focused { Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD) } else { Style::default().fg(Color::DarkGray) })
        .title(untracked_title);

    let untracked_table = Table::new(untracked_rows, [Constraint::Length(5), Constraint::Percentage(90)])
        .block(untracked_block)
        .row_highlight_style(
            Style::default()
                .bg(Color::Rgb(50, 30, 60))
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");
    f.render_stateful_widget(untracked_table, left_chunks[2], &mut app.untracked_table_state);

    // Right: Live Diff Preview
    render_diff_preview(f, app.status_preview_diff.as_ref(), main_chunks[1]);
}

fn render_diff_preview(f: &mut Frame, preview: Option<&DiffViewerState>, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(match preview {
            Some(p) => format!(" Diff Preview: {} [Enter for Full View] ", p.file_title),
            None => " Diff Preview ".to_string(),
        });

    let inner = block.inner(area);
    f.render_widget(block, area);

    if let Some(viewer) = preview {
        let active_lines = viewer.active_lines();
        let height = inner.height as usize;
        let visible_slice = if active_lines.is_empty() {
            &[]
        } else {
            let start = viewer.scroll.min(active_lines.len().saturating_sub(1));
            let end = (start + height).min(active_lines.len());
            &active_lines[start..end]
        };

        let text_lines: Vec<Line> = visible_slice
            .iter()
            .map(|line| {
                let line_no_str = match line.line_number {
                    Some(n) => format!("{:4} │ ", n),
                    None => "     │ ".to_string(),
                };

                match line.kind {
                    DiffLineKind::Addition => Line::from(vec![
                        Span::styled(line_no_str, Style::default().fg(Color::DarkGray)),
                        Span::styled(&line.content, Style::default().fg(Color::Green)),
                    ]),
                    DiffLineKind::Deletion => Line::from(vec![
                        Span::styled(line_no_str, Style::default().fg(Color::DarkGray)),
                        Span::styled(&line.content, Style::default().fg(Color::Red)),
                    ]),
                    DiffLineKind::Hunk => Line::from(vec![
                        Span::styled(line_no_str, Style::default().fg(Color::DarkGray)),
                        Span::styled(&line.content, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                    ]),
                    DiffLineKind::Info => Line::from(vec![
                        Span::styled(line_no_str, Style::default().fg(Color::DarkGray)),
                        Span::styled(&line.content, Style::default().fg(Color::Yellow)),
                    ]),
                    DiffLineKind::Context => Line::from(vec![
                        Span::styled(line_no_str, Style::default().fg(Color::DarkGray)),
                        Span::styled(&line.content, Style::default().fg(Color::White)),
                    ]),
                }
            })
            .collect();

        let paragraph = Paragraph::new(text_lines);
        f.render_widget(paragraph, inner);
    } else {
        let empty = Paragraph::new("No file selected or working tree clean")
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center);
        f.render_widget(empty, inner);
    }
}

// -------------------------------------------------------------------------
// Tab 3: Branches
// -------------------------------------------------------------------------

fn render_tab_branches(f: &mut Frame, app: &mut App, area: Rect) {
    let header_row = Row::new(vec!["CURRENT", "BRANCH NAME", "UPSTREAM TRACKING", "SYNC", "LATEST COMMIT SUMMARY"])
        .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD));

    let rows: Vec<Row> = app
        .branches
        .iter()
        .map(|b| {
            let current_span = if b.is_current {
                Span::styled("*", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))
            } else {
                Span::raw(" ")
            };

            let name_style = if b.is_current {
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
            } else if b.is_remote {
                Style::default().fg(Color::Magenta)
            } else {
                Style::default().fg(Color::Cyan)
            };
            let name_span = Span::styled(&b.name, name_style);

            let upstream_span = Span::styled(
                b.upstream.as_deref().unwrap_or("-"),
                Style::default().fg(Color::DarkGray),
            );

            let sync_str = if b.ahead == 0 && b.behind == 0 {
                "✓".to_string()
            } else {
                let mut s = String::new();
                if b.ahead > 0 { s.push_str(&format!("↑{}", b.ahead)); }
                if b.behind > 0 {
                    if !s.is_empty() { s.push(' '); }
                    s.push_str(&format!("↓{}", b.behind));
                }
                s
            };
            let sync_style = if b.ahead > 0 || b.behind > 0 {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Green)
            };
            let sync_span = Span::styled(sync_str, sync_style);

            let summary_span = Span::styled(&b.summary, Style::default().fg(Color::Gray));

            Row::new(vec![current_span, name_span, upstream_span, sync_span, summary_span])
        })
        .collect();

    let widths = [
        Constraint::Length(8),
        Constraint::Length(26),
        Constraint::Length(26),
        Constraint::Length(10),
        Constraint::Percentage(45),
    ];

    let table = Table::new(rows, widths)
        .header(header_row)
        .block(Block::default().borders(Borders::ALL).title(" Local & Remote Branches [Enter/Space: Checkout, n: New, d: Delete] "))
        .row_highlight_style(
            Style::default()
                .bg(Color::Rgb(40, 50, 70))
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");

    f.render_stateful_widget(table, area, &mut app.branches_table_state);
}

// -------------------------------------------------------------------------
// Tab 4: Stashes
// -------------------------------------------------------------------------

fn render_tab_stash(f: &mut Frame, app: &mut App, area: Rect) {
    let header_row = Row::new(vec!["INDEX", "SELECTOR", "DATE", "MESSAGE"])
        .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD));

    let rows: Vec<Row> = app
        .stashes
        .iter()
        .map(|s| {
            let idx_span = Span::styled(format!("#{}", s.index), Style::default().fg(Color::DarkGray));
            let sel_span = Span::styled(&s.selector, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD));
            let date_span = Span::styled(&s.date, Style::default().fg(Color::Yellow));
            let msg_span = Span::styled(&s.message, Style::default().fg(Color::White));

            Row::new(vec![idx_span, sel_span, date_span, msg_span])
        })
        .collect();

    let widths = [
        Constraint::Length(7),
        Constraint::Length(14),
        Constraint::Length(16),
        Constraint::Percentage(65),
    ];

    let table = Table::new(rows, widths)
        .header(header_row)
        .block(Block::default().borders(Borders::ALL).title(" Stash Stack [s: Save Stash, p: Pop, d: Drop, Enter: View Diff] "))
        .row_highlight_style(
            Style::default()
                .bg(Color::Rgb(40, 50, 70))
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");

    f.render_stateful_widget(table, area, &mut app.stashes_table_state);
}

// =========================================================================
// 3. DIFF VIEWER MODAL
// =========================================================================

fn render_diff_modal(f: &mut Frame, app: &App, area: Rect) {
    let viewer = match &app.diff_viewer {
        Some(v) => v,
        None => return,
    };

    let modal_area = centered_rect(90, 85, area);
    f.render_widget(Clear, modal_area);

    let mode_badge = if viewer.diff_only {
        Span::styled(
            " [Mode: DIFF-ONLY (Tab / d to switch)] ",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(
            " [Mode: FULL-FILE (Tab / d to switch)] ",
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        )
    };

    let title_line = Line::from(vec![
        Span::styled(" Diff: ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        Span::styled(&viewer.commit_hash, Style::default().fg(Color::Yellow)),
        Span::raw(" │ "),
        Span::styled(&viewer.file_title, Style::default().fg(Color::Green)),
        mode_badge,
    ]);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(title_line)
        .title_alignment(Alignment::Left);

    let inner_area = block.inner(modal_area);
    f.render_widget(block, modal_area);

    let active_lines = viewer.active_lines();
    let height = inner_area.height as usize;
    let visible_slice = if active_lines.is_empty() {
        &[]
    } else {
        let start = viewer.scroll.min(active_lines.len().saturating_sub(1));
        let end = (start + height).min(active_lines.len());
        &active_lines[start..end]
    };

    let text_lines: Vec<Line> = visible_slice
        .iter()
        .map(|line| {
            let line_no_str = match line.line_number {
                Some(n) => format!("{:4} │ ", n),
                None => "     │ ".to_string(),
            };

            match line.kind {
                DiffLineKind::Addition => Line::from(vec![
                    Span::styled(line_no_str, Style::default().fg(Color::DarkGray)),
                    Span::styled(&line.content, Style::default().fg(Color::Green)),
                ]),
                DiffLineKind::Deletion => Line::from(vec![
                    Span::styled(line_no_str, Style::default().fg(Color::DarkGray)),
                    Span::styled(&line.content, Style::default().fg(Color::Red)),
                ]),
                DiffLineKind::Hunk => Line::from(vec![
                    Span::styled(line_no_str, Style::default().fg(Color::DarkGray)),
                    Span::styled(&line.content, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                ]),
                DiffLineKind::Info => Line::from(vec![
                    Span::styled(line_no_str, Style::default().fg(Color::DarkGray)),
                    Span::styled(&line.content, Style::default().fg(Color::Yellow)),
                ]),
                DiffLineKind::Context => Line::from(vec![
                    Span::styled(line_no_str, Style::default().fg(Color::DarkGray)),
                    Span::styled(&line.content, Style::default().fg(Color::White)),
                ]),
            }
        })
        .collect();

    let paragraph = Paragraph::new(text_lines);
    f.render_widget(paragraph, inner_area);
}

// =========================================================================
// 4. INTERACTIVE MODALS
// =========================================================================

fn render_active_modal(f: &mut Frame, modal: &ActiveModal, _app: &App, area: Rect) {
    match modal {
        ActiveModal::Commit {
            message,
            amend,
            cursor_pos,
        } => {
            let modal_area = centered_rect(65, 38, area);
            f.render_widget(Clear, modal_area);

            let block = Block::default()
                .borders(Borders::ALL)
                .title(" Commit Staged Changes ")
                .title_alignment(Alignment::Center)
                .border_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD));

            let inner = block.inner(modal_area);
            f.render_widget(block, modal_area);

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1), // prompt
                    Constraint::Length(4), // input box
                    Constraint::Length(2), // amend checkbox
                    Constraint::Length(2), // controls
                ])
                .split(inner);

            let prompt = Paragraph::new("Enter commit message:").style(Style::default().fg(Color::Yellow));
            f.render_widget(prompt, chunks[0]);

            // Text input box with cursor
            let mut display_text = message.clone();
            let c_pos = *cursor_pos;
            if c_pos <= display_text.len() {
                display_text.insert(c_pos, '█');
            } else {
                display_text.push('█');
            }

            let input_widget = Paragraph::new(display_text)
                .block(Block::default().borders(Borders::ALL).title(" Message "))
                .style(Style::default().fg(Color::White));
            f.render_widget(input_widget, chunks[1]);

            let amend_span = if *amend {
                Span::styled("[X] Amend previous commit (Tab to toggle)", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
            } else {
                Span::styled("[ ] Amend previous commit (Tab to toggle)", Style::default().fg(Color::DarkGray))
            };
            let amend_widget = Paragraph::new(Line::from(amend_span));
            f.render_widget(amend_widget, chunks[2]);

            let controls = Paragraph::new(Line::from(vec![
                Span::styled("<Enter> ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::raw("Commit  │  "),
                Span::styled("<Ctrl+E> ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::raw("Open in $EDITOR  │  "),
                Span::styled("<Esc> ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::raw("Cancel"),
            ]))
            .alignment(Alignment::Center);
            f.render_widget(controls, chunks[3]);
        }
        ActiveModal::NewBranch { name, checkout } => {
            let modal_area = centered_rect(55, 28, area);
            f.render_widget(Clear, modal_area);

            let block = Block::default()
                .borders(Borders::ALL)
                .title(" Create New Branch ")
                .title_alignment(Alignment::Center)
                .border_style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD));

            let inner = block.inner(modal_area);
            f.render_widget(block, modal_area);

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1),
                    Constraint::Length(3),
                    Constraint::Length(2),
                    Constraint::Length(1),
                ])
                .split(inner);

            f.render_widget(Paragraph::new("Branch name:").style(Style::default().fg(Color::Yellow)), chunks[0]);

            let input_text = format!("{}█", name);
            let input_box = Paragraph::new(input_text)
                .block(Block::default().borders(Borders::ALL))
                .style(Style::default().fg(Color::White));
            f.render_widget(input_box, chunks[1]);

            let chk_span = if *checkout {
                Span::styled("[X] Checkout branch after creation (Tab to toggle)", Style::default().fg(Color::Green))
            } else {
                Span::styled("[ ] Checkout branch after creation (Tab to toggle)", Style::default().fg(Color::DarkGray))
            };
            f.render_widget(Paragraph::new(Line::from(chk_span)), chunks[2]);

            let controls = Paragraph::new(Line::from(vec![
                Span::styled("<Enter> ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::raw("Create  │  "),
                Span::styled("<Esc> ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::raw("Cancel"),
            ]))
            .alignment(Alignment::Center);
            f.render_widget(controls, chunks[3]);
        }
        ActiveModal::Confirm { title, prompt, action } => {
            let modal_area = centered_rect(55, 24, area);
            f.render_widget(Clear, modal_area);

            let border_color = match action {
                ConfirmAction::DiscardFile { .. } | ConfirmAction::DiscardAll => Color::Red,
                ConfirmAction::DeleteBranch { .. } => Color::Yellow,
                ConfirmAction::DropStash { .. } => Color::Red,
            };

            let block = Block::default()
                .borders(Borders::ALL)
                .title(format!(" {} ", title))
                .title_alignment(Alignment::Center)
                .border_style(Style::default().fg(border_color).add_modifier(Modifier::BOLD));

            let inner = block.inner(modal_area);
            f.render_widget(block, modal_area);

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Percentage(60),
                    Constraint::Percentage(40),
                ])
                .split(inner);

            let prompt_widget = Paragraph::new(prompt.as_str())
                .style(Style::default().fg(Color::White).add_modifier(Modifier::BOLD))
                .alignment(Alignment::Center)
                .wrap(Wrap { trim: false });
            f.render_widget(prompt_widget, chunks[0]);

            let buttons = Paragraph::new(Line::from(vec![
                Span::styled(" [ <y / Enter> Confirm ] ", Style::default().fg(Color::Black).bg(Color::Red).add_modifier(Modifier::BOLD)),
                Span::raw("    "),
                Span::styled(" [ <n / Esc> Cancel ] ", Style::default().fg(Color::Black).bg(Color::White)),
            ]))
            .alignment(Alignment::Center);
            f.render_widget(buttons, chunks[1]);
        }
        ActiveModal::CommandResult {
            title,
            output,
            is_error,
            scroll,
        } => {
            let modal_area = centered_rect(75, 65, area);
            f.render_widget(Clear, modal_area);

            let border_color = if *is_error { Color::Red } else { Color::Green };
            let block = Block::default()
                .borders(Borders::ALL)
                .title(format!(" Output: {} ", title))
                .title_alignment(Alignment::Center)
                .border_style(Style::default().fg(border_color).add_modifier(Modifier::BOLD));

            let inner = block.inner(modal_area);
            f.render_widget(block, modal_area);

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(3),
                    Constraint::Length(1),
                ])
                .split(inner);

            let lines: Vec<Line> = output
                .lines()
                .skip(*scroll)
                .map(|l| Line::from(Span::styled(l, Style::default().fg(Color::White))))
                .collect();

            let paragraph = Paragraph::new(lines);
            f.render_widget(paragraph, chunks[0]);

            let footer = Paragraph::new(Line::from(vec![
                Span::styled("<Esc / Enter / q> ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::raw("Close  │  "),
                Span::styled("<↑/↓> ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::raw("Scroll"),
            ]))
            .alignment(Alignment::Center);
            f.render_widget(footer, chunks[1]);
        }
        ActiveModal::MergeRebase {
            op,
            current_branch,
            candidate_branches,
            selected_branch_idx,
            strategy,
            autostash,
        } => {
            let modal_area = centered_rect(68, 55, area);
            f.render_widget(Clear, modal_area);

            let (title, border_color, action_desc) = match op {
                MergeRebaseOp::Merge => (
                    format!(" Merge into '{}' ", current_branch),
                    Color::Magenta,
                    format!("Select branch to merge INTO current branch '{}':", current_branch),
                ),
                MergeRebaseOp::Rebase => (
                    format!(" Rebase '{}' onto branch ", current_branch),
                    Color::LightMagenta,
                    format!("Select branch to rebase '{}' ONTO:", current_branch),
                ),
            };

            let block = Block::default()
                .borders(Borders::ALL)
                .title(title)
                .title_alignment(Alignment::Center)
                .border_style(Style::default().fg(border_color).add_modifier(Modifier::BOLD));

            let inner = block.inner(modal_area);
            f.render_widget(block, modal_area);

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1), // action description prompt
                    Constraint::Min(4),    // candidate branches list
                    Constraint::Length(2), // strategy / autostash option
                    Constraint::Length(1), // controls footer
                ])
                .split(inner);

            // 1. Prompt
            f.render_widget(
                Paragraph::new(action_desc).style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                chunks[0],
            );

            // 2. Candidate branches list
            let visible_rows = chunks[1].height as usize;
            let scroll_start = if *selected_branch_idx >= visible_rows {
                *selected_branch_idx + 1 - visible_rows
            } else {
                0
            };

            let branch_lines: Vec<Line> = candidate_branches
                .iter()
                .enumerate()
                .skip(scroll_start)
                .take(visible_rows)
                .map(|(idx, b)| {
                    let is_sel = idx == *selected_branch_idx;
                    if is_sel {
                        Line::from(vec![
                            Span::styled(" ▶ ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                            Span::styled(
                                format!(" {} ", b),
                                Style::default()
                                    .fg(Color::Black)
                                    .bg(Color::Cyan)
                                    .add_modifier(Modifier::BOLD),
                            ),
                        ])
                    } else {
                        Line::from(vec![
                            Span::raw("   "),
                            Span::styled(b.as_str(), Style::default().fg(Color::White)),
                        ])
                    }
                })
                .collect();

            let branches_block = Block::default()
                .borders(Borders::ALL)
                .title(format!(" Candidate Branches ({}/{}) ", selected_branch_idx + 1, candidate_branches.len()))
                .border_style(Style::default().fg(Color::DarkGray));
            f.render_widget(Paragraph::new(branch_lines).block(branches_block), chunks[1]);

            // 3. Strategy / Autostash box
            let option_span = match op {
                MergeRebaseOp::Merge => Line::from(vec![
                    Span::styled("Strategy (<Tab> to cycle): ", Style::default().fg(Color::Yellow)),
                    Span::styled(
                        format!("[ {} ]", strategy.label()),
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                    ),
                ]),
                MergeRebaseOp::Rebase => {
                    let chk = if *autostash { "[X]" } else { "[ ]" };
                    let style = if *autostash {
                        Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    };
                    Line::from(vec![
                        Span::styled("Autostash (<Tab> to toggle): ", Style::default().fg(Color::Yellow)),
                        Span::styled(format!("{} --autostash (stash before & pop after rebase)", chk), style),
                    ])
                }
            };
            f.render_widget(Paragraph::new(option_span), chunks[2]);

            // 4. Controls footer
            let controls = Paragraph::new(Line::from(vec![
                Span::styled("<Enter> ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::raw("Execute  │  "),
                Span::styled("<Tab> ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::raw("Toggle Option  │  "),
                Span::styled("<↑/↓ / k/j> ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::raw("Select Branch  │  "),
                Span::styled("<Esc> ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::raw("Cancel"),
            ]))
            .alignment(Alignment::Center);
            f.render_widget(controls, chunks[3]);
        }
        ActiveModal::Help => {
            let modal_area = centered_rect(82, 85, area);
            f.render_widget(Clear, modal_area);

            let block = Block::default()
                .borders(Borders::ALL)
                .title(" gitop Keybindings & Cheatsheet ")
                .title_alignment(Alignment::Center)
                .border_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD));

            let inner = block.inner(modal_area);
            f.render_widget(block, modal_area);

            let help_text = vec![
                Line::from(Span::styled("--- Fleet Overview ---", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))),
                Line::from("  Enter               Inspect selected repository (opens Tab 1: Log & Graph)"),
                Line::from("  /                   Search and filter monitored repositories"),
                Line::from("  r                   Force manual rescan"),
                Line::from("  ↑/↓, k/j, PgUp/PgDn Navigate repository list"),
                Line::from("  q                   Quit gitop"),
                Line::from(""),
                Line::from(Span::styled("--- Repository Client Navigation ---", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))),
                Line::from("  1                   Switch to Tab 1: Log & Graph (Primary monitor view)"),
                Line::from("  2                   Switch to Tab 2: Status & Staging"),
                Line::from("  3                   Switch to Tab 3: Branches"),
                Line::from("  4                   Switch to Tab 4: Stashes"),
                Line::from("  [ / ]               Cycle through tabs"),
                Line::from("  m (in Tab 1)        Jump selection immediately to main/master"),
                Line::from("  H (in Tab 1)        Jump selection to HEAD"),
                Line::from("  M                   Merge branch into current branch"),
                Line::from("  R                   Rebase current branch onto branch"),
                Line::from("  A (if conflict)     Abort in-progress merge or rebase"),
                Line::from("  C (if rebasing)     Continue in-progress rebase after resolving"),
                Line::from("  P                   git push (push to tracking upstream)"),
                Line::from("  p                   git pull (pull from tracking upstream)"),
                Line::from("  f / F               git fetch --all --prune"),
                Line::from("  ?                   Toggle this help screen"),
                Line::from("  Esc / q             Return to Fleet Overview"),
                Line::from(""),
                Line::from(Span::styled("--- Tab 2: Status & Staging ---", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))),
                Line::from("  Tab                 Switch section focus (Staged ⇄ Unstaged ⇄ Untracked)"),
                Line::from("  Space               Toggle Stage/Unstage for selected file"),
                Line::from("  a                   Stage all changes (git add -A)"),
                Line::from("  c                   Open Commit Modal (commit staged changes)"),
                Line::from("  Ctrl+E (in modal)   Launch external $EDITOR for commit message"),
                Line::from("  Enter               Open Full-Screen Diff Viewer for selected file"),
                Line::from("  x                   Discard changes (with safety confirmation)"),
                Line::from(""),
                Line::from(Span::styled("--- Tab 3: Branches ---", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))),
                Line::from("  Enter / Space       Checkout highlighted branch"),
                Line::from("  n                   Create new branch"),
                Line::from("  d                   Delete highlighted branch (with confirmation)"),
                Line::from(""),
                Line::from(Span::styled("--- Tab 4: Stashes ---", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))),
                Line::from("  s                   Stash current changes"),
                Line::from("  p                   Pop highlighted stash"),
                Line::from("  d                   Drop highlighted stash (with confirmation)"),
                Line::from("  Enter               View stash diff"),
                Line::from(""),
                Line::from(Span::styled("--- Diff Viewer Modal ---", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))),
                Line::from("  Tab / d             Toggle Diff Mode (Diff-only vs Full-file)"),
                Line::from("  ↑/↓, k/j, PgUp/PgDn Scroll diff lines"),
                Line::from("  Esc / q / Enter     Close Diff Viewer"),
            ];

            let paragraph = Paragraph::new(help_text).wrap(Wrap { trim: false });
            f.render_widget(paragraph, inner);
        }
    }
}

pub fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
