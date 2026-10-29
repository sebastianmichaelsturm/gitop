use crate::git_actions;
use crate::git_engine::{self, ScannerMessage};
use crate::model::{
    ActiveModal, ActiveView, BranchItem, CommitDetail, CommitGraphRow, ConfirmAction,
    DiffSource, DiffViewerState, InGraphOp, MergeRebaseOp, MergeStrategy, RefKind, RepoOpState, RepoRecord, RepoStatus,
    RepoTab, StashItem, StatusFileItem, StatusSection,
};
use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::widgets::TableState;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;

pub enum ModalEvent {
    None,
    Handled,
    LaunchEditor {
        repo_path: PathBuf,
        initial_msg: String,
    },
}

pub struct App {
    pub root_path: PathBuf,
    pub max_commits: usize,
    pub max_depth: usize,
    pub active_view: ActiveView,

    // Fleet Overview
    pub repos: Vec<RepoRecord>,
    pub selected_repo_v_idx: usize,
    pub filter_query: String,
    pub is_filtering: bool,
    pub fleet_table_state: TableState,

    // Background Scanning State
    pub is_scanning: bool,
    pub scan_spinner_frame: usize,
    pub scanner_rx: Option<Receiver<ScannerMessage>>,
    pub total_scanned: usize,

    // Repo Detail
    pub current_repo_path: Option<PathBuf>,
    pub current_repo_name: String,
    pub current_tab: RepoTab,

    // Tab 1: Graph & Commits (Default)
    pub graph_rows: Vec<CommitGraphRow>,
    pub selected_commit_idx: usize,
    pub commit_detail: Option<CommitDetail>,
    pub pane_focus_files: bool,
    pub selected_file_idx: usize,
    pub graph_table_state: TableState,
    pub files_table_state: TableState,

    // Tab 2: Status & Staging
    pub status_section: StatusSection,
    pub selected_staged_idx: usize,
    pub selected_unstaged_idx: usize,
    pub selected_untracked_idx: usize,
    pub repo_status: RepoStatus,
    pub staged_table_state: TableState,
    pub unstaged_table_state: TableState,
    pub untracked_table_state: TableState,
    pub status_preview_diff: Option<DiffViewerState>,

    // Tab 3: Branches
    pub branches: Vec<BranchItem>,
    pub selected_branch_idx: usize,
    pub branches_table_state: TableState,

    // Tab 4: Stashes
    pub stashes: Vec<StashItem>,
    pub selected_stash_idx: usize,
    pub stashes_table_state: TableState,

    // Modals & Notifications
    pub active_modal: Option<ActiveModal>,
    pub status_message: Option<(String, bool)>, // (message, is_error)
    pub repo_op_state: RepoOpState,
    pub in_graph_op: Option<InGraphOp>,

    // Diff Viewer
    pub diff_viewer: Option<DiffViewerState>,
}

impl App {
    pub fn new(root_path: PathBuf, max_commits: usize, max_depth: usize) -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        git_engine::start_background_scan(root_path.clone(), max_depth, tx);

        Self {
            root_path,
            max_commits,
            max_depth,
            active_view: ActiveView::FleetOverview,

            repos: Vec::new(),
            selected_repo_v_idx: 0,
            filter_query: String::new(),
            is_filtering: false,
            fleet_table_state: TableState::default(),

            is_scanning: true,
            scan_spinner_frame: 0,
            scanner_rx: Some(rx),
            total_scanned: 0,

            current_repo_path: None,
            current_repo_name: String::new(),
            current_tab: RepoTab::Graph,

            graph_rows: Vec::new(),
            selected_commit_idx: 0,
            commit_detail: None,
            pane_focus_files: false,
            selected_file_idx: 0,
            graph_table_state: TableState::default(),
            files_table_state: TableState::default(),

            status_section: StatusSection::Unstaged,
            selected_staged_idx: 0,
            selected_unstaged_idx: 0,
            selected_untracked_idx: 0,
            repo_status: RepoStatus::default(),
            staged_table_state: TableState::default(),
            unstaged_table_state: TableState::default(),
            untracked_table_state: TableState::default(),
            status_preview_diff: None,

            branches: Vec::new(),
            selected_branch_idx: 0,
            branches_table_state: TableState::default(),

            stashes: Vec::new(),
            selected_stash_idx: 0,
            stashes_table_state: TableState::default(),

            active_modal: None,
            status_message: None,
            repo_op_state: RepoOpState::Normal,
            in_graph_op: None,

            diff_viewer: None,
        }
    }

    /// Polls messages from background scanner and streams repo records
    pub fn poll_scanner(&mut self) -> Vec<PathBuf> {
        let mut messages = Vec::new();
        if let Some(rx) = &self.scanner_rx {
            while let Ok(msg) = rx.try_recv() {
                messages.push(msg);
            }
        }

        let mut new_paths = Vec::new();
        let mut needs_sort = false;

        for msg in messages {
            match msg {
                ScannerMessage::Batch(records) => {
                    for rec in records {
                        new_paths.push(rec.path.clone());
                        if let Some(existing) = self.repos.iter_mut().find(|r| r.path == rec.path) {
                            *existing = rec;
                        } else {
                            self.repos.push(rec);
                        }
                    }
                    needs_sort = true;
                }
                ScannerMessage::Finished(total) => {
                    self.is_scanning = false;
                    self.total_scanned = total;
                }
            }
        }

        if needs_sort {
            git_engine::sort_repo_records(&mut self.repos);
            self.sync_table_states();
        }

        new_paths
    }

    pub fn rescan(&mut self) {
        let (tx, rx) = std::sync::mpsc::channel();
        self.is_scanning = true;
        git_engine::start_background_scan(self.root_path.clone(), self.max_depth, tx);
        self.scanner_rx = Some(rx);
    }

    pub fn set_status_message(&mut self, msg: String, is_error: bool) {
        self.status_message = Some((msg, is_error));
    }

    pub fn clear_status_message(&mut self) {
        self.status_message = None;
    }

    pub fn sync_table_states(&mut self) {
        // Fleet
        let visible = self.visible_repo_indices();
        if visible.is_empty() {
            self.fleet_table_state.select(None);
        } else {
            if self.selected_repo_v_idx >= visible.len() {
                self.selected_repo_v_idx = visible.len().saturating_sub(1);
            }
            self.fleet_table_state.select(Some(self.selected_repo_v_idx));
        }

        // Tab 1: Graph & files
        if self.graph_rows.is_empty() {
            self.graph_table_state.select(None);
        } else {
            self.graph_table_state.select(Some(self.selected_commit_idx));
        }

        if let Some(detail) = &self.commit_detail {
            if detail.files.is_empty() {
                self.files_table_state.select(None);
            } else {
                self.files_table_state.select(Some(self.selected_file_idx));
            }
        } else {
            self.files_table_state.select(None);
        }

        // Tab 2: Status sections
        if self.repo_status.staged.is_empty() {
            self.staged_table_state.select(None);
        } else {
            if self.selected_staged_idx >= self.repo_status.staged.len() {
                self.selected_staged_idx = self.repo_status.staged.len().saturating_sub(1);
            }
            self.staged_table_state.select(Some(self.selected_staged_idx));
        }

        if self.repo_status.unstaged.is_empty() {
            self.unstaged_table_state.select(None);
        } else {
            if self.selected_unstaged_idx >= self.repo_status.unstaged.len() {
                self.selected_unstaged_idx = self.repo_status.unstaged.len().saturating_sub(1);
            }
            self.unstaged_table_state.select(Some(self.selected_unstaged_idx));
        }

        if self.repo_status.untracked.is_empty() {
            self.untracked_table_state.select(None);
        } else {
            if self.selected_untracked_idx >= self.repo_status.untracked.len() {
                self.selected_untracked_idx = self.repo_status.untracked.len().saturating_sub(1);
            }
            self.untracked_table_state.select(Some(self.selected_untracked_idx));
        }

        // Tab 3: Branches
        if self.branches.is_empty() {
            self.branches_table_state.select(None);
        } else {
            if self.selected_branch_idx >= self.branches.len() {
                self.selected_branch_idx = self.branches.len().saturating_sub(1);
            }
            self.branches_table_state.select(Some(self.selected_branch_idx));
        }

        // Tab 4: Stashes
        if self.stashes.is_empty() {
            self.stashes_table_state.select(None);
        } else {
            if self.selected_stash_idx >= self.stashes.len() {
                self.selected_stash_idx = self.stashes.len().saturating_sub(1);
            }
            self.stashes_table_state.select(Some(self.selected_stash_idx));
        }
    }

    pub fn visible_repo_indices(&self) -> Vec<usize> {
        if self.filter_query.is_empty() {
            (0..self.repos.len()).collect()
        } else {
            let q = self.filter_query.to_lowercase();
            self.repos
                .iter()
                .enumerate()
                .filter(|(_, r)| {
                    r.name.to_lowercase().contains(&q)
                        || r.rel_path.to_lowercase().contains(&q)
                        || r.current_branch.to_lowercase().contains(&q)
                        || r.last_commit_message.to_lowercase().contains(&q)
                })
                .map(|(idx, _)| idx)
                .collect()
        }
    }

    pub fn current_selected_repo_idx(&self) -> Option<usize> {
        let visible = self.visible_repo_indices();
        visible.get(self.selected_repo_v_idx).copied()
    }

    pub fn current_selected_repo_name(&self) -> &str {
        if !self.current_repo_name.is_empty() {
            &self.current_repo_name
        } else if let Some(idx) = self.current_selected_repo_idx() {
            &self.repos[idx].name
        } else {
            "Repository"
        }
    }

    // -------------------------------------------------------------
    // Tab Switching & Data Loading
    // -------------------------------------------------------------

    pub fn set_repo_tab(&mut self, tab: RepoTab) {
        self.current_tab = tab;
        match tab {
            RepoTab::Graph => {
                // If graph rows are empty, reload them
                if self.graph_rows.is_empty() {
                    if let Some(path) = &self.current_repo_path {
                        if let Ok(rows) = git_engine::load_commit_graph(path, self.max_commits) {
                            self.graph_rows = rows;
                            self.on_commit_selection_changed();
                        }
                    }
                }
            }
            RepoTab::Status => {
                self.refresh_repo_status();
            }
            RepoTab::Branches => {
                self.refresh_branches();
            }
            RepoTab::Stash => {
                self.refresh_stashes();
            }
        }
        self.sync_table_states();
    }

    pub fn next_repo_tab(&mut self) {
        self.set_repo_tab(self.current_tab.next());
    }

    pub fn prev_repo_tab(&mut self) {
        self.set_repo_tab(self.current_tab.prev());
    }

    pub fn refresh_repo_status(&mut self) {
        let path = match &self.current_repo_path {
            Some(p) => p.clone(),
            None => return,
        };

        self.repo_op_state = git_actions::check_repo_op_state(&path);

        if let Ok(status) = git_actions::load_repo_status(&path) {
            self.repo_status = status;
            // Update live preview diff
            self.update_status_preview_diff();
            // Also sync dirty state in repos list
            if let Some(rec) = self.repos.iter_mut().find(|r| r.path == path) {
                rec.is_dirty = self.repo_status.total_count() > 0;
                rec.modified_count = self.repo_status.staged.len() + self.repo_status.unstaged.len();
                rec.untracked_count = self.repo_status.untracked.len();
            }
        }
        self.sync_table_states();
    }

    pub fn update_status_preview_diff(&mut self) {
        let path = match &self.current_repo_path {
            Some(p) => p.clone(),
            None => return,
        };

        let current_item = self.current_selected_status_item();
        match current_item {
            Some(item) => {
                let diff_res = match self.status_section {
                    StatusSection::Staged => {
                        git_engine::load_working_tree_diff(&path, &item.path, true)
                    }
                    StatusSection::Unstaged => {
                        git_engine::load_working_tree_diff(&path, &item.path, false)
                    }
                    StatusSection::Untracked => {
                        git_engine::load_untracked_diff(&path, &item.path)
                    }
                };

                if let Ok((full, diff_only)) = diff_res {
                    self.status_preview_diff = Some(DiffViewerState::new(
                        item.path.clone(),
                        "Working Tree".to_string(),
                        full,
                        diff_only,
                    ));
                } else {
                    self.status_preview_diff = None;
                }
            }
            None => {
                self.status_preview_diff = None;
            }
        }
    }

    pub fn refresh_branches(&mut self) {
        let path = match &self.current_repo_path {
            Some(p) => p.clone(),
            None => return,
        };

        if let Ok(branches) = git_actions::load_branches(&path) {
            self.branches = branches;
            if self.selected_branch_idx >= self.branches.len() {
                self.selected_branch_idx = self.branches.len().saturating_sub(1);
            }
        }
        self.sync_table_states();
    }

    pub fn refresh_stashes(&mut self) {
        let path = match &self.current_repo_path {
            Some(p) => p.clone(),
            None => return,
        };

        if let Ok(stashes) = git_actions::load_stashes(&path) {
            self.stashes = stashes;
            if self.selected_stash_idx >= self.stashes.len() {
                self.selected_stash_idx = self.stashes.len().saturating_sub(1);
            }
        }
        self.sync_table_states();
    }

    pub fn refresh_all_repo_data(&mut self) {
        let path = match &self.current_repo_path {
            Some(p) => p.clone(),
            None => return,
        };

        self.repo_op_state = git_actions::check_repo_op_state(&path);

        if let Ok(rows) = git_engine::load_commit_graph(&path, self.max_commits) {
            self.graph_rows = rows;
            self.on_commit_selection_changed();
        }
        self.refresh_repo_status();
        self.refresh_branches();
        self.refresh_stashes();

        // Update branch name in record
        if let Some(branch) = self.branches.iter().find(|b| b.is_current) {
            if let Some(rec) = self.repos.iter_mut().find(|r| r.path == path) {
                rec.current_branch = branch.name.clone();
                rec.ahead = branch.ahead;
                rec.behind = branch.behind;
            }
        }

        self.sync_table_states();
    }

    pub fn current_selected_status_item(&self) -> Option<StatusFileItem> {
        match self.status_section {
            StatusSection::Staged => self.repo_status.staged.get(self.selected_staged_idx).cloned(),
            StatusSection::Unstaged => self.repo_status.unstaged.get(self.selected_unstaged_idx).cloned(),
            StatusSection::Untracked => self.repo_status.untracked.get(self.selected_untracked_idx).cloned(),
        }
    }

    // -------------------------------------------------------------
    // Git Client Actions
    // -------------------------------------------------------------

    pub fn toggle_stage_selected(&mut self) {
        let path = match &self.current_repo_path {
            Some(p) => p.clone(),
            None => return,
        };

        let item = match self.current_selected_status_item() {
            Some(it) => it,
            None => return,
        };

        match self.status_section {
            StatusSection::Staged => {
                if let Err(e) = git_actions::unstage_file(&path, &item.path) {
                    self.set_status_message(format!("Unstage error: {}", e), true);
                }
            }
            StatusSection::Unstaged | StatusSection::Untracked => {
                if let Err(e) = git_actions::stage_file(&path, &item.path) {
                    self.set_status_message(format!("Stage error: {}", e), true);
                }
            }
        }

        self.refresh_repo_status();
    }

    pub fn stage_all(&mut self) {
        let path = match &self.current_repo_path {
            Some(p) => p.clone(),
            None => return,
        };

        if let Err(e) = git_actions::stage_all(&path) {
            self.set_status_message(format!("Stage all error: {}", e), true);
        } else {
            self.set_status_message("Staged all changes".to_string(), false);
        }
        self.refresh_repo_status();
    }

    pub fn unstage_all(&mut self) {
        let path = match &self.current_repo_path {
            Some(p) => p.clone(),
            None => return,
        };

        if let Err(e) = git_actions::unstage_all(&path) {
            self.set_status_message(format!("Unstage all error: {}", e), true);
        } else {
            self.set_status_message("Unstaged all changes".to_string(), false);
        }
        self.refresh_repo_status();
    }

    pub fn open_commit_modal(&mut self, amend: bool) {
        if self.repo_status.staged.is_empty() && !amend {
            self.set_status_message("Nothing staged to commit (use Space or 'a' to stage files)".to_string(), true);
            return;
        }

        self.active_modal = Some(ActiveModal::Commit {
            message: String::new(),
            amend,
            cursor_pos: 0,
        });
    }

    pub fn open_discard_confirm(&mut self) {
        let item = match self.current_selected_status_item() {
            Some(it) => it,
            None => return,
        };

        let is_untracked = self.status_section == StatusSection::Untracked;
        let prompt = if is_untracked {
            format!("Permanently delete untracked file '{}'?", item.path)
        } else {
            format!("Discard all uncommitted changes in '{}'?", item.path)
        };

        self.active_modal = Some(ActiveModal::Confirm {
            title: "Discard Changes".to_string(),
            prompt,
            action: ConfirmAction::DiscardFile {
                path: item.path,
                is_untracked,
            },
        });
    }

    pub fn checkout_selected_branch(&mut self) {
        let path = match &self.current_repo_path {
            Some(p) => p.clone(),
            None => return,
        };

        let branch = match self.branches.get(self.selected_branch_idx) {
            Some(b) => b,
            None => return,
        };

        if branch.is_current {
            self.set_status_message(format!("Already on branch '{}'", branch.name), false);
            return;
        }

        let branch_name = branch.name.clone();
        match git_actions::checkout_branch(&path, &branch_name) {
            Ok(msg) => {
                self.set_status_message(msg, false);
                self.refresh_all_repo_data();
            }
            Err(err) => {
                self.set_status_message(format!("Checkout failed: {}", err), true);
            }
        }
    }

    pub fn open_new_branch_modal(&mut self) {
        self.active_modal = Some(ActiveModal::NewBranch {
            name: String::new(),
            checkout: true,
        });
    }

    pub fn open_delete_branch_confirm(&mut self) {
        let branch = match self.branches.get(self.selected_branch_idx) {
            Some(b) => b,
            None => return,
        };

        if branch.is_current {
            self.set_status_message("Cannot delete the currently checked out branch".to_string(), true);
            return;
        }

        self.active_modal = Some(ActiveModal::Confirm {
            title: "Delete Branch".to_string(),
            prompt: format!("Are you sure you want to delete branch '{}'?", branch.name),
            action: ConfirmAction::DeleteBranch {
                name: branch.name.clone(),
                is_remote: branch.is_remote,
            },
        });
    }

    pub fn open_stash_save_modal(&mut self) {
        let path = match &self.current_repo_path {
            Some(p) => p.clone(),
            None => return,
        };

        match git_actions::stash_save(&path, None) {
            Ok(msg) => {
                self.set_status_message(msg, false);
                self.refresh_stashes();
                self.refresh_repo_status();
            }
            Err(e) => {
                self.set_status_message(format!("Stash error: {}", e), true);
            }
        }
    }

    pub fn pop_selected_stash(&mut self) {
        let path = match &self.current_repo_path {
            Some(p) => p.clone(),
            None => return,
        };

        let stash = match self.stashes.get(self.selected_stash_idx) {
            Some(s) => s,
            None => return,
        };

        match git_actions::stash_pop(&path, stash.index) {
            Ok(msg) => {
                self.set_status_message(msg, false);
                self.refresh_stashes();
                self.refresh_repo_status();
            }
            Err(e) => {
                self.set_status_message(format!("Stash pop error: {}", e), true);
            }
        }
    }

    pub fn open_drop_stash_confirm(&mut self) {
        let stash = match self.stashes.get(self.selected_stash_idx) {
            Some(s) => s,
            None => return,
        };

        self.active_modal = Some(ActiveModal::Confirm {
            title: "Drop Stash".to_string(),
            prompt: format!("Are you sure you want to drop {} ('{}')?", stash.selector, stash.message),
            action: ConfirmAction::DropStash {
                index: stash.index,
            },
        });
    }

    pub fn push_repo(&mut self) {
        let path = match &self.current_repo_path {
            Some(p) => p.clone(),
            None => return,
        };

        self.set_status_message("Pushing changes to remote...".to_string(), false);
        match git_actions::push(&path) {
            Ok(msg) => {
                let display_msg = msg
                    .lines()
                    .rev()
                    .find(|l| !l.trim().is_empty())
                    .unwrap_or(&msg)
                    .trim();
                self.set_status_message(format!("✓ {}", display_msg), false);
                self.refresh_all_repo_data();
            }
            Err(e) => {
                self.active_modal = Some(ActiveModal::CommandResult {
                    title: "Push Failed".to_string(),
                    output: e,
                    is_error: true,
                    scroll: 0,
                });
            }
        }
    }

    pub fn pull_repo(&mut self) {
        let path = match &self.current_repo_path {
            Some(p) => p.clone(),
            None => return,
        };

        self.set_status_message("Pulling changes from remote...".to_string(), false);
        match git_actions::pull(&path) {
            Ok(msg) => {
                self.set_status_message(format!("✓ {}", msg), false);
                self.refresh_all_repo_data();
            }
            Err(e) => {
                self.active_modal = Some(ActiveModal::CommandResult {
                    title: "Pull Failed".to_string(),
                    output: e,
                    is_error: true,
                    scroll: 0,
                });
            }
        }
    }

    pub fn fetch_repo(&mut self) {
        let path = match &self.current_repo_path {
            Some(p) => p.clone(),
            None => return,
        };

        self.set_status_message("Fetching remotes...".to_string(), false);
        match git_actions::fetch(&path) {
            Ok(msg) => {
                self.set_status_message(format!("✓ {}", msg), false);
                self.refresh_branches();
            }
            Err(e) => {
                self.active_modal = Some(ActiveModal::CommandResult {
                    title: "Fetch Failed".to_string(),
                    output: e,
                    is_error: true,
                    scroll: 0,
                });
            }
        }
    }

    pub fn extract_target_from_row(&self, row: &CommitGraphRow, current_branch: &str) -> Option<(String, String)> {
        // Priority: Mainline > LocalBranch > RemoteBranch > Tag > Hash
        for r in &row.parsed_refs {
            if let RefKind::Mainline(name) = &r.kind {
                if name != current_branch {
                    return Some((name.clone(), r.display_text.clone()));
                }
            }
        }
        for r in &row.parsed_refs {
            if let RefKind::LocalBranch(name) = &r.kind {
                if name != current_branch {
                    return Some((name.clone(), r.display_text.clone()));
                }
            }
        }
        for r in &row.parsed_refs {
            if let RefKind::RemoteBranch(name) = &r.kind {
                if name != current_branch {
                    return Some((name.clone(), r.display_text.clone()));
                }
            }
        }
        for r in &row.parsed_refs {
            if let RefKind::Tag(name) = &r.kind {
                return Some((name.clone(), r.display_text.clone()));
            }
        }

        // Check if row has HEAD ref matching current_branch -> then it's HEAD
        let is_head = row.parsed_refs.iter().any(|r| match &r.kind {
            RefKind::Head(h) => h == current_branch,
            RefKind::Mainline(m) => m == current_branch,
            RefKind::LocalBranch(b) => b == current_branch,
            _ => false,
        });
        if is_head {
            return None;
        }

        if !row.hash.is_empty() {
            Some((row.hash.clone(), row.hash.clone()))
        } else {
            None
        }
    }

    pub fn start_in_graph_op(&mut self, op: MergeRebaseOp) {
        if self.repo_op_state != RepoOpState::Normal {
            self.set_status_message("Repository is already in merge/rebase state (press 'A' to abort)".to_string(), true);
            return;
        }

        let current_branch = self
            .branches
            .iter()
            .find(|b| b.is_current)
            .map(|b| b.name.clone())
            .unwrap_or_else(|| "HEAD".to_string());

        match self.current_tab {
            RepoTab::Graph => {
                if let Some(row) = self.graph_rows.get(self.selected_commit_idx) {
                    let target_info = self.extract_target_from_row(row, &current_branch);
                    match target_info {
                        Some((target_ref, target_display)) => {
                            self.in_graph_op = Some(InGraphOp {
                                op,
                                target_ref,
                                target_display,
                                commit_hash: row.hash.clone(),
                                commit_summary: row.summary.clone(),
                                strategy: MergeStrategy::Default,
                                autostash: true,
                            });
                        }
                        None => {
                            self.set_status_message(
                                "Cannot merge/rebase current branch into itself. Navigate ↑/↓ to a target commit (or press 'm' for main).".to_string(),
                                true,
                            );
                        }
                    }
                }
            }
            RepoTab::Branches => {
                if let Some(branch) = self.branches.get(self.selected_branch_idx) {
                    if branch.is_current {
                        self.set_status_message(
                            "Cannot merge/rebase current branch into itself. Select a different branch.".to_string(),
                            true,
                        );
                    } else {
                        self.in_graph_op = Some(InGraphOp {
                            op,
                            target_ref: branch.name.clone(),
                            target_display: branch.name.clone(),
                            commit_hash: String::new(),
                            commit_summary: branch.summary.clone(),
                            strategy: MergeStrategy::Default,
                            autostash: true,
                        });
                    }
                }
            }
            _ => {}
        }
    }

    pub fn sync_in_graph_op_target(&mut self) {
        let op = match &self.in_graph_op {
            Some(o) => o.op,
            None => return,
        };
        let strat = self.in_graph_op.as_ref().map(|o| o.strategy).unwrap_or(MergeStrategy::Default);
        let stash = self.in_graph_op.as_ref().map(|o| o.autostash).unwrap_or(true);

        let current_branch = self
            .branches
            .iter()
            .find(|b| b.is_current)
            .map(|b| b.name.clone())
            .unwrap_or_else(|| "HEAD".to_string());

        if let Some(row) = self.graph_rows.get(self.selected_commit_idx) {
            if let Some((target_ref, target_display)) = self.extract_target_from_row(row, &current_branch) {
                self.in_graph_op = Some(InGraphOp {
                    op,
                    target_ref,
                    target_display,
                    commit_hash: row.hash.clone(),
                    commit_summary: row.summary.clone(),
                    strategy: strat,
                    autostash: stash,
                });
            }
        }
    }

    pub fn toggle_in_graph_op_option(&mut self) {
        if let Some(op) = &mut self.in_graph_op {
            match op.op {
                MergeRebaseOp::Merge => {
                    op.strategy = op.strategy.next();
                }
                MergeRebaseOp::Rebase => {
                    op.autostash = !op.autostash;
                }
            }
        }
    }

    pub fn execute_in_graph_op(&mut self) {
        let op = match self.in_graph_op.take() {
            Some(o) => o,
            None => return,
        };

        let path = match &self.current_repo_path {
            Some(p) => p.clone(),
            None => return,
        };

        let current_branch = self
            .branches
            .iter()
            .find(|b| b.is_current)
            .map(|b| b.name.clone())
            .unwrap_or_else(|| "HEAD".to_string());

        match op.op {
            MergeRebaseOp::Merge => {
                self.set_status_message(format!("Merging '{}' into '{}'...", op.target_display, current_branch), false);
                match git_actions::merge(&path, &op.target_ref, op.strategy) {
                    Ok(msg) => {
                        let display_msg = msg.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or(&msg).trim();
                        self.set_status_message(format!("✓ {}", display_msg), false);
                        self.refresh_all_repo_data();
                    }
                    Err(err) => {
                        self.refresh_all_repo_data();
                        self.active_modal = Some(ActiveModal::CommandResult {
                            title: format!("Merge '{}' Failed", op.target_display),
                            output: err,
                            is_error: true,
                            scroll: 0,
                        });
                    }
                }
            }
            MergeRebaseOp::Rebase => {
                self.set_status_message(format!("Rebasing '{}' onto '{}'...", current_branch, op.target_display), false);
                match git_actions::rebase(&path, &op.target_ref, op.autostash) {
                    Ok(msg) => {
                        let display_msg = msg.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or(&msg).trim();
                        self.set_status_message(format!("✓ {}", display_msg), false);
                        self.refresh_all_repo_data();
                    }
                    Err(err) => {
                        self.refresh_all_repo_data();
                        self.active_modal = Some(ActiveModal::CommandResult {
                            title: format!("Rebase onto '{}' Failed", op.target_display),
                            output: err,
                            is_error: true,
                            scroll: 0,
                        });
                    }
                }
            }
        }
    }

    pub fn abort_current_op(&mut self) {
        let path = match &self.current_repo_path {
            Some(p) => p.clone(),
            None => return,
        };

        match self.repo_op_state {
            RepoOpState::Merging => {
                match git_actions::merge_abort(&path) {
                    Ok(msg) => {
                        self.set_status_message(format!("✓ {}", msg), false);
                        self.refresh_all_repo_data();
                    }
                    Err(e) => {
                        self.set_status_message(format!("Merge abort error: {}", e), true);
                    }
                }
            }
            RepoOpState::Rebasing => {
                match git_actions::rebase_abort(&path) {
                    Ok(msg) => {
                        self.set_status_message(format!("✓ {}", msg), false);
                        self.refresh_all_repo_data();
                    }
                    Err(e) => {
                        self.set_status_message(format!("Rebase abort error: {}", e), true);
                    }
                }
            }
            RepoOpState::Normal => {
                self.set_status_message("No merge or rebase currently in progress".to_string(), false);
            }
        }
    }

    pub fn continue_rebase_op(&mut self) {
        let path = match &self.current_repo_path {
            Some(p) => p.clone(),
            None => return,
        };

        if self.repo_op_state == RepoOpState::Rebasing {
            match git_actions::rebase_continue(&path) {
                Ok(msg) => {
                    self.set_status_message(format!("✓ {}", msg), false);
                    self.refresh_all_repo_data();
                }
                Err(e) => {
                    self.refresh_all_repo_data();
                    self.active_modal = Some(ActiveModal::CommandResult {
                        title: "Rebase Continue Failed".to_string(),
                        output: e,
                        is_error: true,
                        scroll: 0,
                    });
                }
            }
        }
    }

    pub fn open_help_modal(&mut self) {
        self.active_modal = Some(ActiveModal::Help);
    }

    // -------------------------------------------------------------
    // Modal Key Handling
    // -------------------------------------------------------------

    pub fn handle_modal_key(&mut self, code: KeyCode, modifiers: KeyModifiers) -> ModalEvent {
        let modal = match &mut self.active_modal {
            Some(m) => m,
            None => return ModalEvent::None,
        };

        match modal {
            ActiveModal::Commit {
                message,
                amend,
                cursor_pos,
            } => {
                if modifiers.contains(KeyModifiers::CONTROL) && code == KeyCode::Char('e') {
                    if let Some(path) = &self.current_repo_path {
                        return ModalEvent::LaunchEditor {
                            repo_path: path.clone(),
                            initial_msg: message.clone(),
                        };
                    }
                }

                match code {
                    KeyCode::Esc => {
                        self.active_modal = None;
                        ModalEvent::Handled
                    }
                    KeyCode::Tab => {
                        *amend = !*amend;
                        ModalEvent::Handled
                    }
                    KeyCode::Enter => {
                        let msg = message.clone();
                        let do_amend = *amend;
                        let path = self.current_repo_path.clone();
                        self.active_modal = None;

                        if let Some(repo_path) = path {
                            match git_actions::commit(&repo_path, &msg, do_amend) {
                                Ok(res) => {
                                    self.set_status_message(format!("✓ {}", res), false);
                                    self.refresh_all_repo_data();
                                }
                                Err(err) => {
                                    self.active_modal = Some(ActiveModal::CommandResult {
                                        title: "Commit Failed".to_string(),
                                        output: err,
                                        is_error: true,
                                        scroll: 0,
                                    });
                                }
                            }
                        }
                        ModalEvent::Handled
                    }
                    KeyCode::Left => {
                        *cursor_pos = cursor_pos.saturating_sub(1);
                        ModalEvent::Handled
                    }
                    KeyCode::Right => {
                        if *cursor_pos < message.len() {
                            *cursor_pos += 1;
                        }
                        ModalEvent::Handled
                    }
                    KeyCode::Backspace => {
                        if *cursor_pos > 0 && !message.is_empty() {
                            message.remove(*cursor_pos - 1);
                            *cursor_pos -= 1;
                        }
                        ModalEvent::Handled
                    }
                    KeyCode::Char(c) => {
                        message.insert(*cursor_pos, c);
                        *cursor_pos += 1;
                        ModalEvent::Handled
                    }
                    _ => ModalEvent::Handled,
                }
            }
            ActiveModal::NewBranch { name, checkout } => match code {
                KeyCode::Esc => {
                    self.active_modal = None;
                    ModalEvent::Handled
                }
                KeyCode::Tab => {
                    *checkout = !*checkout;
                    ModalEvent::Handled
                }
                KeyCode::Enter => {
                    let branch_name = name.trim().to_string();
                    let do_checkout = *checkout;
                    let path = self.current_repo_path.clone();
                    self.active_modal = None;

                    if branch_name.is_empty() {
                        self.set_status_message("Branch name cannot be empty".to_string(), true);
                        return ModalEvent::Handled;
                    }

                    if let Some(repo_path) = path {
                        match git_actions::create_branch(&repo_path, &branch_name, do_checkout) {
                            Ok(res) => {
                                self.set_status_message(format!("✓ {}", res), false);
                                self.refresh_all_repo_data();
                            }
                            Err(err) => {
                                self.active_modal = Some(ActiveModal::CommandResult {
                                    title: "Create Branch Failed".to_string(),
                                    output: err,
                                    is_error: true,
                                    scroll: 0,
                                });
                            }
                        }
                    }
                    ModalEvent::Handled
                }
                KeyCode::Backspace => {
                    name.pop();
                    ModalEvent::Handled
                }
                KeyCode::Char(c) => {
                    name.push(c);
                    ModalEvent::Handled
                }
                _ => ModalEvent::Handled,
            },
            ActiveModal::Confirm { action, .. } => match code {
                KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                    let act = action.clone();
                    let path = self.current_repo_path.clone();
                    self.active_modal = None;

                    if let Some(repo_path) = path {
                        match act {
                            ConfirmAction::DiscardFile { path: file_path, is_untracked } => {
                                match git_actions::discard_file(&repo_path, &file_path, is_untracked) {
                                    Ok(_) => {
                                        self.set_status_message(format!("Discarded '{}'", file_path), false);
                                        self.refresh_repo_status();
                                    }
                                    Err(e) => {
                                        self.set_status_message(format!("Discard error: {}", e), true);
                                    }
                                }
                            }
                            ConfirmAction::DiscardAll => {}
                            ConfirmAction::DeleteBranch { name, .. } => {
                                match git_actions::delete_branch(&repo_path, &name, true) {
                                    Ok(res) => {
                                        self.set_status_message(format!("✓ {}", res), false);
                                        self.refresh_branches();
                                    }
                                    Err(e) => {
                                        self.set_status_message(format!("Delete error: {}", e), true);
                                    }
                                }
                            }
                            ConfirmAction::DropStash { index } => {
                                match git_actions::stash_drop(&repo_path, index) {
                                    Ok(res) => {
                                        self.set_status_message(format!("✓ {}", res), false);
                                        self.refresh_stashes();
                                    }
                                    Err(e) => {
                                        self.set_status_message(format!("Drop error: {}", e), true);
                                    }
                                }
                            }
                        }
                    }
                    ModalEvent::Handled
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    self.active_modal = None;
                    ModalEvent::Handled
                }
                _ => ModalEvent::Handled,
            },
            ActiveModal::CommandResult { scroll, .. } => match code {
                KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => {
                    self.active_modal = None;
                    ModalEvent::Handled
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    *scroll = scroll.saturating_sub(1);
                    ModalEvent::Handled
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    *scroll = scroll.saturating_add(1);
                    ModalEvent::Handled
                }
                _ => ModalEvent::Handled,
            },
            ActiveModal::Help => match code {
                KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') | KeyCode::Char('?') => {
                    self.active_modal = None;
                    ModalEvent::Handled
                }
                _ => ModalEvent::Handled,
            },
        }
    }

    // -------------------------------------------------------------
    // Navigation Handlers
    // -------------------------------------------------------------

    pub fn move_up(&mut self) {
        match self.active_view {
            ActiveView::FleetOverview => {
                if self.selected_repo_v_idx > 0 {
                    self.selected_repo_v_idx -= 1;
                }
            }
            ActiveView::RepoDetail => match self.current_tab {
                RepoTab::Graph => {
                    if self.pane_focus_files {
                        if self.selected_file_idx > 0 {
                            self.selected_file_idx -= 1;
                        }
                    } else if self.selected_commit_idx > 0 {
                        self.selected_commit_idx -= 1;
                        self.on_commit_selection_changed();
                    }
                }
                RepoTab::Status => {
                    match self.status_section {
                        StatusSection::Staged => {
                            if self.selected_staged_idx > 0 {
                                self.selected_staged_idx -= 1;
                            }
                        }
                        StatusSection::Unstaged => {
                            if self.selected_unstaged_idx > 0 {
                                self.selected_unstaged_idx -= 1;
                            }
                        }
                        StatusSection::Untracked => {
                            if self.selected_untracked_idx > 0 {
                                self.selected_untracked_idx -= 1;
                            }
                        }
                    }
                    self.update_status_preview_diff();
                }
                RepoTab::Branches => {
                    if self.selected_branch_idx > 0 {
                        self.selected_branch_idx -= 1;
                    }
                }
                RepoTab::Stash => {
                    if self.selected_stash_idx > 0 {
                        self.selected_stash_idx -= 1;
                    }
                }
            },
            ActiveView::DiffViewer => {
                if let Some(viewer) = &mut self.diff_viewer {
                    viewer.scroll_up(1);
                }
            }
        }
        self.sync_table_states();
    }

    pub fn move_down(&mut self) {
        match self.active_view {
            ActiveView::FleetOverview => {
                let max = self.visible_repo_indices().len().saturating_sub(1);
                if self.selected_repo_v_idx < max {
                    self.selected_repo_v_idx += 1;
                }
            }
            ActiveView::RepoDetail => match self.current_tab {
                RepoTab::Graph => {
                    if self.pane_focus_files {
                        if let Some(detail) = &self.commit_detail {
                            let max = detail.files.len().saturating_sub(1);
                            if self.selected_file_idx < max {
                                self.selected_file_idx += 1;
                            }
                        }
                    } else {
                        let max = self.graph_rows.len().saturating_sub(1);
                        if self.selected_commit_idx < max {
                            self.selected_commit_idx += 1;
                            self.on_commit_selection_changed();
                        }
                    }
                }
                RepoTab::Status => {
                    match self.status_section {
                        StatusSection::Staged => {
                            let max = self.repo_status.staged.len().saturating_sub(1);
                            if self.selected_staged_idx < max {
                                self.selected_staged_idx += 1;
                            }
                        }
                        StatusSection::Unstaged => {
                            let max = self.repo_status.unstaged.len().saturating_sub(1);
                            if self.selected_unstaged_idx < max {
                                self.selected_unstaged_idx += 1;
                            }
                        }
                        StatusSection::Untracked => {
                            let max = self.repo_status.untracked.len().saturating_sub(1);
                            if self.selected_untracked_idx < max {
                                self.selected_untracked_idx += 1;
                            }
                        }
                    }
                    self.update_status_preview_diff();
                }
                RepoTab::Branches => {
                    let max = self.branches.len().saturating_sub(1);
                    if self.selected_branch_idx < max {
                        self.selected_branch_idx += 1;
                    }
                }
                RepoTab::Stash => {
                    let max = self.stashes.len().saturating_sub(1);
                    if self.selected_stash_idx < max {
                        self.selected_stash_idx += 1;
                    }
                }
            },
            ActiveView::DiffViewer => {
                if let Some(viewer) = &mut self.diff_viewer {
                    viewer.scroll_down(1);
                }
            }
        }
        self.sync_table_states();
    }

    pub fn move_page_up(&mut self) {
        match self.active_view {
            ActiveView::FleetOverview => {
                self.selected_repo_v_idx = self.selected_repo_v_idx.saturating_sub(10);
            }
            ActiveView::RepoDetail => match self.current_tab {
                RepoTab::Graph => {
                    self.selected_commit_idx = self.selected_commit_idx.saturating_sub(10);
                    self.on_commit_selection_changed();
                }
                RepoTab::Status => {
                    match self.status_section {
                        StatusSection::Staged => self.selected_staged_idx = self.selected_staged_idx.saturating_sub(10),
                        StatusSection::Unstaged => self.selected_unstaged_idx = self.selected_unstaged_idx.saturating_sub(10),
                        StatusSection::Untracked => self.selected_untracked_idx = self.selected_untracked_idx.saturating_sub(10),
                    }
                    self.update_status_preview_diff();
                }
                RepoTab::Branches => {
                    self.selected_branch_idx = self.selected_branch_idx.saturating_sub(10);
                }
                RepoTab::Stash => {
                    self.selected_stash_idx = self.selected_stash_idx.saturating_sub(10);
                }
            },
            ActiveView::DiffViewer => {
                if let Some(viewer) = &mut self.diff_viewer {
                    viewer.scroll_up(15);
                }
            }
        }
        self.sync_table_states();
    }

    pub fn move_page_down(&mut self) {
        match self.active_view {
            ActiveView::FleetOverview => {
                let max = self.visible_repo_indices().len().saturating_sub(1);
                self.selected_repo_v_idx = (self.selected_repo_v_idx + 10).min(max);
            }
            ActiveView::RepoDetail => match self.current_tab {
                RepoTab::Graph => {
                    let max = self.graph_rows.len().saturating_sub(1);
                    self.selected_commit_idx = (self.selected_commit_idx + 10).min(max);
                    self.on_commit_selection_changed();
                }
                RepoTab::Status => {
                    match self.status_section {
                        StatusSection::Staged => {
                            let max = self.repo_status.staged.len().saturating_sub(1);
                            self.selected_staged_idx = (self.selected_staged_idx + 10).min(max);
                        }
                        StatusSection::Unstaged => {
                            let max = self.repo_status.unstaged.len().saturating_sub(1);
                            self.selected_unstaged_idx = (self.selected_unstaged_idx + 10).min(max);
                        }
                        StatusSection::Untracked => {
                            let max = self.repo_status.untracked.len().saturating_sub(1);
                            self.selected_untracked_idx = (self.selected_untracked_idx + 10).min(max);
                        }
                    }
                    self.update_status_preview_diff();
                }
                RepoTab::Branches => {
                    let max = self.branches.len().saturating_sub(1);
                    self.selected_branch_idx = (self.selected_branch_idx + 10).min(max);
                }
                RepoTab::Stash => {
                    let max = self.stashes.len().saturating_sub(1);
                    self.selected_stash_idx = (self.selected_stash_idx + 10).min(max);
                }
            },
            ActiveView::DiffViewer => {
                if let Some(viewer) = &mut self.diff_viewer {
                    viewer.scroll_down(15);
                }
            }
        }
        self.sync_table_states();
    }

    pub fn move_to_top(&mut self) {
        match self.active_view {
            ActiveView::FleetOverview => self.selected_repo_v_idx = 0,
            ActiveView::RepoDetail => match self.current_tab {
                RepoTab::Graph => {
                    self.selected_commit_idx = 0;
                    self.on_commit_selection_changed();
                }
                RepoTab::Status => {
                    match self.status_section {
                        StatusSection::Staged => self.selected_staged_idx = 0,
                        StatusSection::Unstaged => self.selected_unstaged_idx = 0,
                        StatusSection::Untracked => self.selected_untracked_idx = 0,
                    }
                    self.update_status_preview_diff();
                }
                RepoTab::Branches => self.selected_branch_idx = 0,
                RepoTab::Stash => self.selected_stash_idx = 0,
            },
            ActiveView::DiffViewer => {
                if let Some(viewer) = &mut self.diff_viewer {
                    viewer.scroll_to_top();
                }
            }
        }
        self.sync_table_states();
    }

    pub fn move_to_bottom(&mut self) {
        match self.active_view {
            ActiveView::FleetOverview => {
                self.selected_repo_v_idx = self.visible_repo_indices().len().saturating_sub(1);
            }
            ActiveView::RepoDetail => match self.current_tab {
                RepoTab::Graph => {
                    self.selected_commit_idx = self.graph_rows.len().saturating_sub(1);
                    self.on_commit_selection_changed();
                }
                RepoTab::Status => {
                    match self.status_section {
                        StatusSection::Staged => self.selected_staged_idx = self.repo_status.staged.len().saturating_sub(1),
                        StatusSection::Unstaged => self.selected_unstaged_idx = self.repo_status.unstaged.len().saturating_sub(1),
                        StatusSection::Untracked => self.selected_untracked_idx = self.repo_status.untracked.len().saturating_sub(1),
                    }
                    self.update_status_preview_diff();
                }
                RepoTab::Branches => {
                    self.selected_branch_idx = self.branches.len().saturating_sub(1);
                }
                RepoTab::Stash => {
                    self.selected_stash_idx = self.stashes.len().saturating_sub(1);
                }
            },
            ActiveView::DiffViewer => {
                if let Some(viewer) = &mut self.diff_viewer {
                    viewer.scroll_to_bottom();
                }
            }
        }
        self.sync_table_states();
    }

    pub fn on_enter(&mut self) {
        match self.active_view {
            ActiveView::FleetOverview => {
                if let Some(idx) = self.current_selected_repo_idx() {
                    let (repo_path, repo_name) = {
                        let repo = &self.repos[idx];
                        (repo.path.clone(), repo.name.clone())
                    };
                    self.current_repo_path = Some(repo_path.clone());
                    self.current_repo_name = repo_name;

                    // ALWAYS start on Tab 1: Log & Graph (primary monitoring tool behavior)
                    self.current_tab = RepoTab::Graph;

                    if let Ok(rows) = git_engine::load_commit_graph(&repo_path, self.max_commits) {
                        self.graph_rows = rows;
                        self.selected_commit_idx = 0;
                        self.pane_focus_files = false;
                        self.selected_file_idx = 0;
                        self.on_commit_selection_changed();
                    }

                    // Preload status & branches in background
                    let _ = git_actions::load_repo_status(&repo_path).map(|st| self.repo_status = st);
                    let _ = git_actions::load_branches(&repo_path).map(|br| self.branches = br);
                    self.repo_op_state = git_actions::check_repo_op_state(&repo_path);

                    self.sync_table_states();
                    self.active_view = ActiveView::RepoDetail;
                }
            }
            ActiveView::RepoDetail => match self.current_tab {
                RepoTab::Graph => {
                    let repo_path = match &self.current_repo_path {
                        Some(p) => p.clone(),
                        None => return,
                    };

                    let active_commit_hash = self.current_selected_commit_hash();
                    if active_commit_hash.is_empty() {
                        return;
                    }

                    let target_file = if self.pane_focus_files {
                        self.commit_detail
                            .as_ref()
                            .and_then(|d| d.files.get(self.selected_file_idx).map(|f| f.path.clone()))
                    } else {
                        None
                    };

                    let title = target_file.clone().unwrap_or_else(|| "All Changes in Commit".to_string());

                    if let Ok((full, diff_only)) = git_engine::load_commit_diff(&repo_path, &active_commit_hash, target_file.as_deref()) {
                        self.diff_viewer = Some(DiffViewerState::new(
                            title,
                            active_commit_hash.clone(),
                            full,
                            diff_only,
                        ).with_source(DiffSource::Commit { hash: active_commit_hash, file: target_file }));
                        self.active_view = ActiveView::DiffViewer;
                    }
                }
                RepoTab::Status => {
                    let repo_path = match &self.current_repo_path {
                        Some(p) => p.clone(),
                        None => return,
                    };

                    let item = match self.current_selected_status_item() {
                        Some(it) => it,
                        None => return,
                    };

                    let diff_res = match self.status_section {
                        StatusSection::Staged => git_engine::load_working_tree_diff(&repo_path, &item.path, true),
                        StatusSection::Unstaged => git_engine::load_working_tree_diff(&repo_path, &item.path, false),
                        StatusSection::Untracked => git_engine::load_untracked_diff(&repo_path, &item.path),
                    };

                    if let Ok((full, diff_only)) = diff_res {
                        let is_staged = self.status_section == StatusSection::Staged;
                        let src = if self.status_section == StatusSection::Untracked {
                            DiffSource::Untracked { file: item.path.clone() }
                        } else {
                            DiffSource::WorkingTree { file: item.path.clone(), staged: is_staged }
                        };

                        let mode_desc = match self.status_section {
                            StatusSection::Staged => "Staged (Index vs HEAD)",
                            StatusSection::Unstaged => "Unstaged (Working Tree vs Index)",
                            StatusSection::Untracked => "Untracked File",
                        };

                        self.diff_viewer = Some(DiffViewerState::new(
                            item.path.clone(),
                            mode_desc.to_string(),
                            full,
                            diff_only,
                        ).with_source(src));
                        self.active_view = ActiveView::DiffViewer;
                    }
                }
                RepoTab::Branches => {
                    self.checkout_selected_branch();
                }
                RepoTab::Stash => {
                    let repo_path = match &self.current_repo_path {
                        Some(p) => p.clone(),
                        None => return,
                    };

                    let stash = match self.stashes.get(self.selected_stash_idx) {
                        Some(s) => s,
                        None => return,
                    };

                    if let Ok((full, diff_only)) = git_engine::load_stash_diff(&repo_path, &stash.selector) {
                        self.diff_viewer = Some(DiffViewerState::new(
                            format!("Stash: {}", stash.message),
                            stash.selector.clone(),
                            full,
                            diff_only,
                        ).with_source(DiffSource::Stash { selector: stash.selector.clone() }));
                        self.active_view = ActiveView::DiffViewer;
                    }
                }
            },
            ActiveView::DiffViewer => {
                self.active_view = ActiveView::RepoDetail;
                self.diff_viewer = None;
            }
        }
    }

    pub fn on_esc(&mut self) {
        if self.in_graph_op.take().is_some() {
            return;
        }

        if self.active_modal.is_some() {
            self.active_modal = None;
            return;
        }

        if self.is_filtering {
            self.is_filtering = false;
            self.filter_query.clear();
            self.sync_table_states();
            return;
        }

        match self.active_view {
            ActiveView::DiffViewer => {
                self.active_view = ActiveView::RepoDetail;
                self.diff_viewer = None;
            }
            ActiveView::RepoDetail => {
                self.active_view = ActiveView::FleetOverview;
                self.graph_rows.clear();
                self.commit_detail = None;
                self.current_repo_path = None;
                self.status_preview_diff = None;
                self.sync_table_states();
            }
            ActiveView::FleetOverview => {}
        }
    }

    pub fn switch_pane(&mut self) {
        match self.active_view {
            ActiveView::RepoDetail => match self.current_tab {
                RepoTab::Graph => {
                    self.pane_focus_files = !self.pane_focus_files;
                }
                RepoTab::Status => {
                    self.status_section = self.status_section.next();
                    self.update_status_preview_diff();
                }
                _ => {}
            },
            ActiveView::DiffViewer => {
                if let Some(viewer) = &mut self.diff_viewer {
                    viewer.toggle_mode();
                }
            }
            _ => {}
        }
    }

    pub fn toggle_diff_mode(&mut self) {
        if let Some(viewer) = &mut self.diff_viewer {
            viewer.toggle_mode();
        }
    }

    pub fn jump_to_main(&mut self) {
        if self.active_view != ActiveView::RepoDetail || self.current_tab != RepoTab::Graph {
            return;
        }

        let main_idx = self.graph_rows.iter().position(|r| {
            r.parsed_refs.iter().any(|pr| matches!(pr.kind, crate::model::RefKind::Mainline(_)))
                || r.refs.iter().any(|s| s == "main" || s == "master" || s == "origin/main")
        });

        if let Some(idx) = main_idx {
            self.selected_commit_idx = idx;
            self.on_commit_selection_changed();
            let hash = &self.graph_rows[idx].hash;
            let short_hash = if hash.len() > 7 { &hash[..7] } else { hash };
            self.set_status_message(format!("✓ Jumped to main ({})", short_hash), false);
        } else {
            self.set_status_message("No commit labeled 'main' or 'master' found in history".to_string(), true);
        }
        self.sync_table_states();
    }

    pub fn jump_to_head(&mut self) {
        if self.active_view != ActiveView::RepoDetail || self.current_tab != RepoTab::Graph {
            return;
        }

        let head_idx = self.graph_rows.iter().position(|r| {
            r.parsed_refs.iter().any(|pr| matches!(pr.kind, crate::model::RefKind::Head(_)))
                || r.refs.iter().any(|s| s.starts_with("HEAD"))
        }).unwrap_or(0);

        if head_idx < self.graph_rows.len() {
            self.selected_commit_idx = head_idx;
            self.on_commit_selection_changed();
            let hash = &self.graph_rows[head_idx].hash;
            let short_hash = if hash.len() > 7 { &hash[..7] } else { hash };
            self.set_status_message(format!("✓ Jumped to HEAD ({})", short_hash), false);
        }
        self.sync_table_states();
    }

    fn current_selected_commit_hash(&self) -> String {
        if let Some(row) = self.graph_rows.get(self.selected_commit_idx) {
            if !row.hash.is_empty() {
                return row.hash.clone();
            }
        }
        for row in &self.graph_rows[self.selected_commit_idx..] {
            if !row.hash.is_empty() {
                return row.hash.clone();
            }
        }
        String::new()
    }

    fn on_commit_selection_changed(&mut self) {
        let hash = self.current_selected_commit_hash();
        if hash.is_empty() {
            return;
        }

        if let Some(path) = &self.current_repo_path {
            if let Ok(detail) = git_engine::load_commit_details(path, &hash) {
                self.commit_detail = Some(detail);
                self.selected_file_idx = 0;
            }
        }

        if self.in_graph_op.is_some() {
            self.sync_in_graph_op_target();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_repo(name: &str, branch: &str, msg: &str) -> RepoRecord {
        RepoRecord {
            path: PathBuf::from(name),
            name: name.to_string(),
            rel_path: name.to_string(),
            current_branch: branch.to_string(),
            last_commit_time: None,
            last_commit_hash: "1234567".to_string(),
            last_commit_author: "Alice".to_string(),
            last_commit_message: msg.to_string(),
            is_dirty: false,
            modified_count: 0,
            untracked_count: 0,
            ahead: 0,
            behind: 0,
        }
    }

    #[test]
    fn test_filter_repos() {
        let mut app = App::new(PathBuf::from("."), 200, 5);
        app.is_scanning = false;
        app.scanner_rx = None;
        app.repos = vec![
            create_test_repo("filetop", "main", "feat: viewer"),
            create_test_repo("control", "dev", "fix: logic"),
            create_test_repo("gitop", "feat/init", "init project"),
        ];

        assert_eq!(app.visible_repo_indices().len(), 3);

        app.filter_query = "git".to_string();
        let visible = app.visible_repo_indices();
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0], 2);

        app.filter_query = "viewer".to_string();
        let visible = app.visible_repo_indices();
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0], 0);

        app.filter_query = "nonexistent".to_string();
        assert_eq!(app.visible_repo_indices().len(), 0);
    }

    #[test]
    fn test_navigation_bounds() {
        let mut app = App::new(PathBuf::from("."), 200, 5);
        app.is_scanning = false;
        app.scanner_rx = None;
        app.repos = vec![
            create_test_repo("repo1", "main", "msg"),
            create_test_repo("repo2", "main", "msg"),
        ];

        app.move_up();
        assert_eq!(app.selected_repo_v_idx, 0);

        app.move_down();
        assert_eq!(app.selected_repo_v_idx, 1);

        app.move_down();
        assert_eq!(app.selected_repo_v_idx, 1);

        app.move_to_top();
        assert_eq!(app.selected_repo_v_idx, 0);

        app.move_to_bottom();
        assert_eq!(app.selected_repo_v_idx, 1);
    }

    #[test]
    fn test_tab_switching() {
        let mut app = App::new(PathBuf::from("."), 200, 5);
        app.active_view = ActiveView::RepoDetail;
        assert_eq!(app.current_tab, RepoTab::Graph);

        app.next_repo_tab();
        assert_eq!(app.current_tab, RepoTab::Status);

        app.next_repo_tab();
        assert_eq!(app.current_tab, RepoTab::Branches);

        app.next_repo_tab();
        assert_eq!(app.current_tab, RepoTab::Stash);

        app.next_repo_tab();
        assert_eq!(app.current_tab, RepoTab::Graph);

        app.prev_repo_tab();
        assert_eq!(app.current_tab, RepoTab::Stash);
    }

    #[test]
    fn test_modal_commit_typing() {
        let mut app = App::new(PathBuf::from("."), 200, 5);
        app.active_modal = Some(ActiveModal::Commit {
            message: "feat: init".to_string(),
            amend: false,
            cursor_pos: 10,
        });

        // Type '!'
        app.handle_modal_key(KeyCode::Char('!'), KeyModifiers::NONE);
        if let Some(ActiveModal::Commit { message, cursor_pos, .. }) = &app.active_modal {
            assert_eq!(message, "feat: init!");
            assert_eq!(*cursor_pos, 11);
        } else {
            panic!("Expected commit modal");
        }

        // Backspace
        app.handle_modal_key(KeyCode::Backspace, KeyModifiers::NONE);
        if let Some(ActiveModal::Commit { message, cursor_pos, .. }) = &app.active_modal {
            assert_eq!(message, "feat: init");
            assert_eq!(*cursor_pos, 10);
        } else {
            panic!("Expected commit modal");
        }

        // Toggle amend
        app.handle_modal_key(KeyCode::Tab, KeyModifiers::NONE);
        if let Some(ActiveModal::Commit { amend, .. }) = &app.active_modal {
            assert!(*amend);
        } else {
            panic!("Expected commit modal");
        }
    }

    #[test]
    fn test_jump_to_main() {
        let mut app = App::new(PathBuf::from("."), 200, 5);
        app.active_view = ActiveView::RepoDetail;
        app.current_tab = RepoTab::Graph;

        app.graph_rows = vec![
            CommitGraphRow {
                graph_prefix: "*".into(),
                hash: "1111111".into(),
                refs: vec!["HEAD -> feat/test".into()],
                parsed_refs: vec![crate::model::CommitRef {
                    kind: crate::model::RefKind::Head("feat/test".into()),
                    display_text: "HEAD -> feat/test".into(),
                    is_paired_upstream: false,
                }],
                author: "Dev".into(),
                relative_time: "1m ago".into(),
                summary: "wip".into(),
            },
            CommitGraphRow {
                graph_prefix: "*".into(),
                hash: "2222222".into(),
                refs: vec!["main".into(), "origin/main".into()],
                parsed_refs: vec![crate::model::CommitRef {
                    kind: crate::model::RefKind::Mainline("main".into()),
                    display_text: "main(=)".into(),
                    is_paired_upstream: true,
                }],
                author: "Dev".into(),
                relative_time: "1d ago".into(),
                summary: "stable release".into(),
            },
        ];

        app.selected_commit_idx = 0;
        app.jump_to_main();
        assert_eq!(app.selected_commit_idx, 1);

        app.jump_to_head();
        assert_eq!(app.selected_commit_idx, 0);
    }

    #[test]
    fn test_in_graph_op_direct_selection_and_cycle() {
        let mut app = App::new(PathBuf::from("."), 200, 5);
        app.active_view = ActiveView::RepoDetail;
        app.current_tab = RepoTab::Graph;
        app.current_repo_path = Some(PathBuf::from("."));

        app.branches = vec![
            BranchItem {
                name: "feature-branch".into(),
                is_current: true,
                is_remote: false,
                upstream: None,
                ahead: 0,
                behind: 0,
                summary: "feat".into(),
            },
            BranchItem {
                name: "main".into(),
                is_current: false,
                is_remote: false,
                upstream: None,
                ahead: 0,
                behind: 0,
                summary: "main branch".into(),
            },
            BranchItem {
                name: "hotfix".into(),
                is_current: false,
                is_remote: false,
                upstream: None,
                ahead: 0,
                behind: 0,
                summary: "fix".into(),
            },
        ];

        // 0. Current HEAD commit row (cannot merge into itself)
        // 1. Commit with "hotfix"
        // 2. Commit with "main"
        // 3. Intermediate commit without branch tags (hash only)
        app.graph_rows = vec![
            CommitGraphRow {
                graph_prefix: "*".into(),
                hash: "1111111".into(),
                refs: vec!["HEAD -> feature-branch".into()],
                parsed_refs: vec![crate::model::CommitRef {
                    kind: crate::model::RefKind::Head("feature-branch".into()),
                    display_text: "HEAD -> feature-branch".into(),
                    is_paired_upstream: false,
                }],
                author: "Dev".into(),
                relative_time: "10m ago".into(),
                summary: "feat in progress".into(),
            },
            CommitGraphRow {
                graph_prefix: "*".into(),
                hash: "2222222".into(),
                refs: vec!["hotfix".into()],
                parsed_refs: vec![crate::model::CommitRef {
                    kind: crate::model::RefKind::LocalBranch("hotfix".into()),
                    display_text: "hotfix".into(),
                    is_paired_upstream: false,
                }],
                author: "Dev".into(),
                relative_time: "1h ago".into(),
                summary: "quick fix".into(),
            },
            CommitGraphRow {
                graph_prefix: "*".into(),
                hash: "3333333".into(),
                refs: vec!["main".into(), "origin/main".into()],
                parsed_refs: vec![crate::model::CommitRef {
                    kind: crate::model::RefKind::Mainline("main".into()),
                    display_text: "main(=)".into(),
                    is_paired_upstream: true,
                }],
                author: "Dev".into(),
                relative_time: "1d ago".into(),
                summary: "stable release".into(),
            },
            CommitGraphRow {
                graph_prefix: "*".into(),
                hash: "4444444".into(),
                refs: vec![],
                parsed_refs: vec![],
                author: "Dev".into(),
                relative_time: "2d ago".into(),
                summary: "historical commit".into(),
            },
        ];

        // On HEAD (row 0): pressing M should be intercepted
        app.selected_commit_idx = 0;
        app.start_in_graph_op(MergeRebaseOp::Merge);
        assert!(app.in_graph_op.is_none());
        assert!(app.status_message.as_ref().unwrap().0.contains("Cannot merge/rebase"));

        // Navigate to row 1 ("hotfix") and press M: should select "hotfix"
        app.selected_commit_idx = 1;
        app.start_in_graph_op(MergeRebaseOp::Merge);
        assert!(app.in_graph_op.is_some());
        let op = app.in_graph_op.as_ref().unwrap();
        assert_eq!(op.op, MergeRebaseOp::Merge);
        assert_eq!(op.target_ref, "hotfix");
        assert_eq!(op.target_display, "hotfix");
        assert_eq!(op.strategy, MergeStrategy::Default);

        // Test Tab toggles strategy: Default -> --no-ff -> --ff-only -> Default
        app.toggle_in_graph_op_option();
        assert_eq!(app.in_graph_op.as_ref().unwrap().strategy, MergeStrategy::NoFastForward);
        app.toggle_in_graph_op_option();
        assert_eq!(app.in_graph_op.as_ref().unwrap().strategy, MergeStrategy::FastForwardOnly);
        app.toggle_in_graph_op_option();
        assert_eq!(app.in_graph_op.as_ref().unwrap().strategy, MergeStrategy::Default);

        // Move cursor down to row 2 ("main(=)"): target dynamically updates to main(=)!
        app.selected_commit_idx = 2;
        app.on_commit_selection_changed();
        let op = app.in_graph_op.as_ref().unwrap();
        assert_eq!(op.target_ref, "main");
        assert_eq!(op.target_display, "main(=)");

        // Move cursor down to row 3 (intermediate commit without branch tag): target dynamically updates to commit hash!
        app.selected_commit_idx = 3;
        app.on_commit_selection_changed();
        let op = app.in_graph_op.as_ref().unwrap();
        assert_eq!(op.target_ref, "4444444");
        assert_eq!(op.target_display, "4444444");

        // Test Rebase with autostash
        app.in_graph_op = None;
        app.selected_commit_idx = 2; // main
        app.start_in_graph_op(MergeRebaseOp::Rebase);
        assert!(app.in_graph_op.is_some());
        let op = app.in_graph_op.as_ref().unwrap();
        assert_eq!(op.op, MergeRebaseOp::Rebase);
        assert_eq!(op.target_ref, "main");
        assert_eq!(op.autostash, true);

        // Tab toggles autostash: true -> false -> true
        app.toggle_in_graph_op_option();
        assert_eq!(app.in_graph_op.as_ref().unwrap().autostash, false);
        app.toggle_in_graph_op_option();
        assert_eq!(app.in_graph_op.as_ref().unwrap().autostash, true);

        // Esc cancels
        app.on_esc();
        assert!(app.in_graph_op.is_none());
    }
}
