use chrono::{DateTime, Utc};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActiveView {
    FleetOverview,
    RepoDetail,
    DiffViewer,
}

#[derive(Debug, Clone)]
pub struct RepoRecord {
    pub path: PathBuf,
    pub name: String,
    pub rel_path: String,
    pub current_branch: String,
    pub last_commit_time: Option<DateTime<Utc>>,
    pub last_commit_hash: String,
    pub last_commit_author: String,
    pub last_commit_message: String,
    pub is_dirty: bool,
    pub modified_count: usize,
    pub untracked_count: usize,
    pub ahead: usize,
    pub behind: usize,
}

impl RepoRecord {
    pub fn formatted_time(&self) -> String {
        match self.last_commit_time {
            Some(t) => format_relative_time(t),
            None => "No commits".to_string(),
        }
    }

    pub fn status_badge(&self) -> String {
        if !self.is_dirty {
            "clean".to_string()
        } else {
            let mut parts = Vec::new();
            if self.modified_count > 0 {
                parts.push(format!("~{}", self.modified_count));
            }
            if self.untracked_count > 0 {
                parts.push(format!("+{}", self.untracked_count));
            }
            if parts.is_empty() {
                "* dirty".to_string()
            } else {
                format!("* {}", parts.join(" "))
            }
        }
    }

    pub fn sync_badge(&self) -> String {
        if self.ahead == 0 && self.behind == 0 {
            "✓".to_string()
        } else {
            let mut s = String::new();
            if self.ahead > 0 {
                s.push_str(&format!("↑{}", self.ahead));
            }
            if self.behind > 0 {
                if !s.is_empty() {
                    s.push(' ');
                }
                s.push_str(&format!("↓{}", self.behind));
            }
            s
        }
    }
}

pub fn format_relative_time(time: DateTime<Utc>) -> String {
    let now = Utc::now();
    let duration = now.signed_duration_since(time);

    if duration.num_seconds() < 60 {
        format!("{}s ago", duration.num_seconds().max(0))
    } else if duration.num_minutes() < 60 {
        format!("{}m ago", duration.num_minutes())
    } else if duration.num_hours() < 24 {
        format!("{}h ago", duration.num_hours())
    } else if duration.num_days() < 30 {
        format!("{}d ago", duration.num_days())
    } else {
        time.format("%Y-%m-%d").to_string()
    }
}

#[derive(Debug, Clone)]
pub struct CommitGraphRow {
    pub graph_prefix: String,
    pub hash: String,
    pub refs: Vec<String>,
    pub author: String,
    pub relative_time: String,
    pub summary: String,
}

#[derive(Debug, Clone)]
pub struct ChangedFile {
    pub status: String, // e.g. "M", "A", "D"
    pub path: String,
}

#[derive(Debug, Clone)]
pub struct CommitDetail {
    pub hash: String,
    pub author: String,
    pub date: String,
    pub full_message: String,
    pub files: Vec<ChangedFile>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffLineKind {
    Hunk,
    Addition,
    Deletion,
    Context,
    Info,
}

#[derive(Debug, Clone)]
pub struct DiffLine {
    pub kind: DiffLineKind,
    pub content: String,
    pub line_number: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct DiffViewerState {
    pub file_title: String,
    pub commit_hash: String,
    pub full_lines: Vec<DiffLine>,
    pub diff_only_lines: Vec<DiffLine>,
    pub diff_only: bool,
    pub scroll: usize,
}

impl DiffViewerState {
    pub fn new(file_title: String, commit_hash: String, full_lines: Vec<DiffLine>, diff_only_lines: Vec<DiffLine>) -> Self {
        let has_diffs = diff_only_lines.iter().any(|l| l.kind == DiffLineKind::Addition || l.kind == DiffLineKind::Deletion);
        Self {
            file_title,
            commit_hash,
            full_lines,
            diff_only_lines,
            diff_only: has_diffs,
            scroll: 0,
        }
    }

    pub fn active_lines(&self) -> &[DiffLine] {
        if self.diff_only {
            &self.diff_only_lines
        } else {
            &self.full_lines
        }
    }

    pub fn toggle_mode(&mut self) {
        self.diff_only = !self.diff_only;
        let max_scroll = self.active_lines().len().saturating_sub(1);
        if self.scroll > max_scroll {
            self.scroll = max_scroll;
        }
    }

    pub fn scroll_down(&mut self, amount: usize) {
        let total = self.active_lines().len();
        if self.scroll + amount < total {
            self.scroll += amount;
        } else {
            self.scroll = total.saturating_sub(1);
        }
    }

    pub fn scroll_up(&mut self, amount: usize) {
        self.scroll = self.scroll.saturating_sub(amount);
    }

    pub fn scroll_to_top(&mut self) {
        self.scroll = 0;
    }

    pub fn scroll_to_bottom(&mut self) {
        self.scroll = self.active_lines().len().saturating_sub(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn test_format_relative_time() {
        let now = Utc::now();
        assert_eq!(format_relative_time(now - Duration::seconds(15)), "15s ago");
        assert_eq!(format_relative_time(now - Duration::minutes(5)), "5m ago");
        assert_eq!(format_relative_time(now - Duration::hours(3)), "3h ago");
        assert_eq!(format_relative_time(now - Duration::days(4)), "4d ago");
    }

    #[test]
    fn test_diff_viewer_state_toggle_and_scroll() {
        let full = vec![
            DiffLine { kind: DiffLineKind::Context, content: "Line 1".into(), line_number: Some(1) },
            DiffLine { kind: DiffLineKind::Addition, content: "+ Line 2".into(), line_number: Some(2) },
            DiffLine { kind: DiffLineKind::Context, content: "Line 3".into(), line_number: Some(3) },
        ];
        let diff_only = vec![
            DiffLine { kind: DiffLineKind::Addition, content: "+ Line 2".into(), line_number: Some(2) },
        ];

        let mut viewer = DiffViewerState::new("test.rs".into(), "1234567".into(), full, diff_only);
        assert!(viewer.diff_only);
        assert_eq!(viewer.active_lines().len(), 1);

        viewer.toggle_mode();
        assert!(!viewer.diff_only);
        assert_eq!(viewer.active_lines().len(), 3);

        viewer.scroll_down(2);
        assert_eq!(viewer.scroll, 2);

        viewer.toggle_mode();
        // Since diff_only has 1 item, scroll should clamp to 0
        assert_eq!(viewer.scroll, 0);
    }
}
