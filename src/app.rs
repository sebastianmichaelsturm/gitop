use crate::git_engine;
use crate::model::{ActiveView, CommitDetail, CommitGraphRow, DiffViewerState, RepoRecord};
use ratatui::widgets::TableState;
use std::path::PathBuf;

pub struct App {
    pub root_path: PathBuf,
    pub max_commits: usize,
    pub active_view: ActiveView,

    // Fleet Overview
    pub discovered_paths: Vec<PathBuf>,
    pub repos: Vec<RepoRecord>,
    pub selected_repo_v_idx: usize,
    pub filter_query: String,
    pub is_filtering: bool,
    pub fleet_table_state: TableState,

    // Repo Detail
    pub current_repo_path: Option<PathBuf>,
    pub current_repo_name: String,
    pub graph_rows: Vec<CommitGraphRow>,
    pub selected_commit_idx: usize,
    pub commit_detail: Option<CommitDetail>,
    pub pane_focus_files: bool,
    pub selected_file_idx: usize,
    pub graph_table_state: TableState,
    pub files_table_state: TableState,

    // Diff Viewer
    pub diff_viewer: Option<DiffViewerState>,
}

impl App {
    pub fn new(root_path: PathBuf, max_commits: usize) -> Self {
        let discovered = git_engine::discover_repos(&root_path);
        let repos = git_engine::scan_all_repos_parallel(&discovered, &root_path);

        let mut fleet_table_state = TableState::default();
        if !repos.is_empty() {
            fleet_table_state.select(Some(0));
        }

        Self {
            root_path,
            max_commits,
            active_view: ActiveView::FleetOverview,

            discovered_paths: discovered,
            repos,
            selected_repo_v_idx: 0,
            filter_query: String::new(),
            is_filtering: false,
            fleet_table_state,

            current_repo_path: None,
            current_repo_name: String::new(),
            graph_rows: Vec::new(),
            selected_commit_idx: 0,
            commit_detail: None,
            pane_focus_files: false,
            selected_file_idx: 0,
            graph_table_state: TableState::default(),
            files_table_state: TableState::default(),

            diff_viewer: None,
        }
    }

    pub fn sync_table_states(&mut self) {
        if self.visible_repo_indices().is_empty() {
            self.fleet_table_state.select(None);
        } else {
            self.fleet_table_state.select(Some(self.selected_repo_v_idx));
        }

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
    }

    pub fn rescan(&mut self) {
        self.discovered_paths = git_engine::discover_repos(&self.root_path);
        self.repos = git_engine::scan_all_repos_parallel(&self.discovered_paths, &self.root_path);
        let max_v = self.visible_repo_indices().len().saturating_sub(1);
        if self.selected_repo_v_idx > max_v {
            self.selected_repo_v_idx = max_v;
        }
        self.sync_table_states();
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

    pub fn move_up(&mut self) {
        match self.active_view {
            ActiveView::FleetOverview => {
                if self.selected_repo_v_idx > 0 {
                    self.selected_repo_v_idx -= 1;
                }
            }
            ActiveView::RepoDetail => {
                if self.pane_focus_files {
                    if self.selected_file_idx > 0 {
                        self.selected_file_idx -= 1;
                    }
                } else if self.selected_commit_idx > 0 {
                    self.selected_commit_idx -= 1;
                    self.on_commit_selection_changed();
                }
            }
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
            ActiveView::RepoDetail => {
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
            ActiveView::RepoDetail => {
                self.selected_commit_idx = self.selected_commit_idx.saturating_sub(10);
                self.on_commit_selection_changed();
            }
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
            ActiveView::RepoDetail => {
                let max = self.graph_rows.len().saturating_sub(1);
                self.selected_commit_idx = (self.selected_commit_idx + 10).min(max);
                self.on_commit_selection_changed();
            }
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
            ActiveView::RepoDetail => {
                self.selected_commit_idx = 0;
                self.on_commit_selection_changed();
            }
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
            ActiveView::RepoDetail => {
                self.selected_commit_idx = self.graph_rows.len().saturating_sub(1);
                self.on_commit_selection_changed();
            }
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
                    let repo = &self.repos[idx];
                    self.current_repo_path = Some(repo.path.clone());
                    self.current_repo_name = repo.name.clone();

                    if let Ok(rows) = git_engine::load_commit_graph(&repo.path, self.max_commits) {
                        self.graph_rows = rows;
                        self.selected_commit_idx = 0;
                        self.pane_focus_files = false;
                        self.selected_file_idx = 0;
                        self.on_commit_selection_changed();
                        self.sync_table_states();
                        self.active_view = ActiveView::RepoDetail;
                    }
                }
            }
            ActiveView::RepoDetail => {
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
                        active_commit_hash,
                        full,
                        diff_only,
                    ));
                    self.active_view = ActiveView::DiffViewer;
                }
            }
            ActiveView::DiffViewer => {
                self.active_view = ActiveView::RepoDetail;
                self.diff_viewer = None;
            }
        }
    }

    pub fn on_esc(&mut self) {
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
                self.sync_table_states();
            }
            ActiveView::FleetOverview => {}
        }
    }

    pub fn switch_pane(&mut self) {
        match self.active_view {
            ActiveView::RepoDetail => {
                self.pane_focus_files = !self.pane_focus_files;
            }
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
        let mut app = App {
            root_path: PathBuf::from("."),
            max_commits: 200,
            active_view: ActiveView::FleetOverview,
            discovered_paths: Vec::new(),
            repos: vec![
                create_test_repo("filetop", "main", "feat: viewer"),
                create_test_repo("control", "dev", "fix: logic"),
                create_test_repo("gittop", "feat/init", "init project"),
            ],
            selected_repo_v_idx: 0,
            filter_query: String::new(),
            is_filtering: false,
            fleet_table_state: TableState::default(),
            current_repo_path: None,
            current_repo_name: String::new(),
            graph_rows: Vec::new(),
            selected_commit_idx: 0,
            commit_detail: None,
            pane_focus_files: false,
            selected_file_idx: 0,
            graph_table_state: TableState::default(),
            files_table_state: TableState::default(),
            diff_viewer: None,
        };

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
        let mut app = App {
            root_path: PathBuf::from("."),
            max_commits: 200,
            active_view: ActiveView::FleetOverview,
            discovered_paths: Vec::new(),
            repos: vec![
                create_test_repo("repo1", "main", "msg"),
                create_test_repo("repo2", "main", "msg"),
            ],
            selected_repo_v_idx: 0,
            filter_query: String::new(),
            is_filtering: false,
            fleet_table_state: TableState::default(),
            current_repo_path: None,
            current_repo_name: String::new(),
            graph_rows: Vec::new(),
            selected_commit_idx: 0,
            commit_detail: None,
            pane_focus_files: false,
            selected_file_idx: 0,
            graph_table_state: TableState::default(),
            files_table_state: TableState::default(),
            diff_viewer: None,
        };

        app.move_up();
        assert_eq!(app.selected_repo_v_idx, 0);

        app.move_down();
        assert_eq!(app.selected_repo_v_idx, 1);

        app.move_down();
        assert_eq!(app.selected_repo_v_idx, 1); // clamped

        app.move_to_top();
        assert_eq!(app.selected_repo_v_idx, 0);

        app.move_to_bottom();
        assert_eq!(app.selected_repo_v_idx, 1);
    }
}
