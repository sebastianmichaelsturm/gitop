use crate::app::App;
use crate::model::{ActiveView, DiffLineKind};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Row, Table, Wrap},
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
}

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

fn render_repo_detail(f: &mut Frame, app: &mut App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Percentage(60), // Graph & Commits
            Constraint::Percentage(35), // Details & Files
            Constraint::Length(1), // Footer
        ])
        .split(area);

    let repo_name = app.current_selected_repo_name().to_string();
    let header_widget = Paragraph::new(Line::from(vec![
        Span::styled(" REPO DETAIL ", Style::default().fg(Color::Black).bg(Color::Green).add_modifier(Modifier::BOLD)),
        Span::raw("  "),
        Span::styled(repo_name, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        Span::raw(" │ All Branches & Commit Graph"),
    ]))
    .block(Block::default().borders(Borders::ALL).title(" Repository Inspector "));
    f.render_widget(header_widget, chunks[0]);

    // Graph Area
    let graph_rows: Vec<Row> = app
        .graph_rows
        .iter()
        .map(|row| {
            let prefix_span = Span::styled(row.graph_prefix.clone(), Style::default().fg(Color::Cyan));
            let hash_span = Span::styled(row.hash.clone(), Style::default().fg(Color::Yellow));
            
            let refs_str = if row.refs.is_empty() {
                String::new()
            } else {
                format!("({}) ", row.refs.join(", "))
            };
            let refs_span = Span::styled(refs_str, Style::default().fg(Color::Green).add_modifier(Modifier::BOLD));
            
            let author_span = Span::styled(row.author.clone(), Style::default().fg(Color::LightBlue));
            let date_span = Span::styled(row.relative_time.clone(), Style::default().fg(Color::DarkGray));
            let summary_span = Span::styled(row.summary.clone(), Style::default().fg(Color::White));

            Row::new(vec![
                prefix_span,
                hash_span,
                refs_span,
                summary_span,
                author_span,
                date_span,
            ])
        })
        .collect();

    let graph_header = Row::new(vec!["GRAPH", "HASH", "BRANCHES/TAGS", "SUMMARY", "AUTHOR", "DATE"])
        .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD));

    let graph_widths = [
        Constraint::Length(12),
        Constraint::Length(9),
        Constraint::Length(22),
        Constraint::Percentage(40),
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

    f.render_stateful_widget(graph_table, chunks[1], &mut app.graph_table_state);

    // Bottom Split: Commit Info Left 55%, Files Right 45%
    let bottom_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(chunks[2]);

    if let Some(detail) = &app.commit_detail {
        let info_lines = vec![
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
            Line::from(""),
            Line::from(Span::styled(&detail.full_message, Style::default().fg(Color::White))),
        ];

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
        let empty_widget = Paragraph::new("Loading commit details...")
            .block(Block::default().borders(Borders::ALL).title(" Commit Details "));
        f.render_widget(empty_widget, bottom_chunks[0]);
    }

    // Footer
    let footer_text = vec![
        Span::styled("[Enter] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw("View Diff  "),
        Span::styled("[Tab] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw("Switch Focus (Graph ⇄ Files)  "),
        Span::styled("[Esc/q] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw("Back to Fleet Overview  "),
        Span::styled("[↑/↓] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw("Navigate"),
    ];
    let footer_widget = Paragraph::new(Line::from(footer_text));
    f.render_widget(footer_widget, chunks[3]);
}

fn render_diff_modal(f: &mut Frame, app: &App, area: Rect) {
    let viewer = match &app.diff_viewer {
        Some(v) => v,
        None => return,
    };

    let modal_area = centered_rect(88, 85, area);
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
        Span::styled(" Commit Diff: ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        Span::styled(&viewer.commit_hash[..7.min(viewer.commit_hash.len())], Style::default().fg(Color::Yellow)),
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

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
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
