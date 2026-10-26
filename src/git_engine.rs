use crate::model::{ChangedFile, CommitDetail, CommitGraphRow, DiffLine, DiffLineKind, RepoRecord};
use chrono::DateTime;
use git2::{BranchType, Repository, StatusOptions};
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::process::Command;
use walkdir::WalkDir;

const IGNORED_DIRS: &[&str] = &[
    "target",
    "node_modules",
    ".cargo",
    "AppData",
    ".gemini",
    ".cache",
    ".idea",
    ".vscode",
    "__pycache__",
    "vendor",
    "dist",
    "build",
    "tmp",
    "temp",
];

/// Recursively scans for git repositories starting from `root`.
/// Prunes recursion when a `.git` directory is encountered.
pub fn discover_repos(root: &Path) -> Vec<PathBuf> {
    let mut repos = Vec::new();
    let mut it = WalkDir::new(root).follow_links(false).into_iter();

    while let Some(entry) = it.next() {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        let file_type = entry.file_type();
        if file_type.is_dir() {
            let file_name = entry.file_name().to_string_lossy();

            // Skip known huge/noise non-repo directories
            if IGNORED_DIRS.contains(&file_name.as_ref()) {
                it.skip_current_dir();
                continue;
            }

            // Check if this directory is a .git directory
            if file_name == ".git" {
                if let Some(parent) = entry.path().parent() {
                    repos.push(parent.to_path_buf());
                }
                // Do not descend into .git itself
                it.skip_current_dir();
                continue;
            }

            // Check if directory directly contains a .git folder or file
            let git_sub = entry.path().join(".git");
            if git_sub.exists() {
                repos.push(entry.path().to_path_buf());
                it.skip_current_dir();
                continue;
            }
        }
    }

    repos.sort();
    repos.dedup();
    repos
}

/// Inspects a single repository in-memory using git2
pub fn inspect_repo_in_memory(path: &Path, root: &Path) -> Option<RepoRecord> {
    let repo = Repository::open(path).ok()?;

    let name = path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "unnamed".to_string());

    let rel_path = path
        .strip_prefix(root)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| name.clone());

    let (current_branch, head_commit_oid) = match repo.head() {
        Ok(head) => {
            let branch = if head.is_branch() {
                head.shorthand().unwrap_or("HEAD").to_string()
            } else {
                format!(
                    "detached:{}",
                    head.target()
                        .map(|t| t.to_string()[..7.min(t.to_string().len())].to_string())
                        .unwrap_or_else(|| "unknown".to_string())
                )
            };
            (branch, head.target())
        }
        Err(_) => ("(no branch)".to_string(), None),
    };

    let mut last_commit_time = None;
    let mut last_commit_hash = String::new();
    let mut last_commit_author = String::new();
    let mut last_commit_message = String::new();

    if let Some(oid) = head_commit_oid {
        if let Ok(commit) = repo.find_commit(oid) {
            let secs = commit.time().seconds();
            last_commit_time = DateTime::from_timestamp(secs, 0);
            last_commit_hash = oid.to_string();
            last_commit_author = commit.author().name().unwrap_or("").to_string();
            last_commit_message = commit.summary().unwrap_or("").to_string();
        }
    }

    // Ahead / Behind calculation
    let mut ahead = 0;
    let mut behind = 0;
    if let (Some(head_oid), Ok(branch)) = (head_commit_oid, repo.find_branch(&current_branch, BranchType::Local)) {
        if let Ok(upstream) = branch.upstream() {
            if let Some(upstream_oid) = upstream.get().target() {
                if let Ok((a, b)) = repo.graph_ahead_behind(head_oid, upstream_oid) {
                    ahead = a;
                    behind = b;
                }
            }
        }
    }

    // Status inspection
    let mut is_dirty = false;
    let mut modified_count = 0;
    let mut untracked_count = 0;

    let mut status_opts = StatusOptions::new();
    status_opts
        .include_untracked(true)
        .recurse_untracked_dirs(false)
        .exclude_submodules(true);

    if let Ok(statuses) = repo.statuses(Some(&mut status_opts)) {
        for s in statuses.iter() {
            let st = s.status();
            if st.is_wt_new() || st.is_index_new() {
                untracked_count += 1;
                is_dirty = true;
            } else if st.is_wt_modified()
                || st.is_index_modified()
                || st.is_wt_deleted()
                || st.is_index_deleted()
                || st.is_wt_renamed()
                || st.is_index_renamed()
            {
                modified_count += 1;
                is_dirty = true;
            }
        }
    }

    Some(RepoRecord {
        path: path.to_path_buf(),
        name,
        rel_path,
        current_branch,
        last_commit_time,
        last_commit_hash,
        last_commit_author,
        last_commit_message,
        is_dirty,
        modified_count,
        untracked_count,
        ahead,
        behind,
    })
}

/// Scans all repositories in parallel and sorts strictly by latest commit descending
pub fn scan_all_repos_parallel(repos: &[PathBuf], root: &Path) -> Vec<RepoRecord> {
    let mut records: Vec<RepoRecord> = repos
        .par_iter()
        .filter_map(|p| inspect_repo_in_memory(p, root))
        .collect();

    records.sort_by(|a, b| match (&a.last_commit_time, &b.last_commit_time) {
        (Some(ta), Some(tb)) => tb.cmp(ta),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a.name.cmp(&b.name),
    });

    records
}

/// Loads git log with full branch graph via `git log --graph --all`
pub fn load_commit_graph(
    repo_path: &Path,
    max_commits: usize,
) -> Result<Vec<CommitGraphRow>, Box<dyn std::error::Error>> {
    let output = Command::new("git")
        .args([
            "-C",
            &repo_path.to_string_lossy(),
            "log",
            "--graph",
            "--all",
            "--color=never",
            "--date=relative",
            &format!("--format=format:COMMIT_ROW%x09%h%x09%d%x09%an%x09%cr%x09%s"),
            &format!("-n{}", max_commits),
        ])
        .output()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut rows = Vec::new();

    for line in stdout.lines() {
        if line.trim().is_empty() {
            continue;
        }

        if let Some(pos) = line.find("COMMIT_ROW\t") {
            let graph_prefix = line[..pos].to_string();
            let payload = &line[pos + "COMMIT_ROW\t".len()..];
            let parts: Vec<&str> = payload.split('\t').collect();

            let hash = parts.first().copied().unwrap_or("").to_string();
            let refs_raw = parts.get(1).copied().unwrap_or("").trim();
            let author = parts.get(2).copied().unwrap_or("").to_string();
            let relative_time = parts.get(3).copied().unwrap_or("").to_string();
            let summary = parts.get(4).copied().unwrap_or("").to_string();

            let refs: Vec<String> = if !refs_raw.is_empty() {
                refs_raw
                    .trim_start_matches('(')
                    .trim_end_matches(')')
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            } else {
                Vec::new()
            };

            rows.push(CommitGraphRow {
                graph_prefix,
                hash,
                refs,
                author,
                relative_time,
                summary,
            });
        } else {
            // Pure graph structure line (e.g. `| \ ` or `| | ` without commit)
            rows.push(CommitGraphRow {
                graph_prefix: line.to_string(),
                hash: String::new(),
                refs: Vec::new(),
                author: String::new(),
                relative_time: String::new(),
                summary: String::new(),
            });
        }
    }

    Ok(rows)
}

/// Loads full details for a commit including changed files list
pub fn load_commit_details(
    repo_path: &Path,
    commit_hash: &str,
) -> Result<CommitDetail, Box<dyn std::error::Error>> {
    let output = Command::new("git")
        .args([
            "-C",
            &repo_path.to_string_lossy(),
            "show",
            "--name-status",
            "--color=never",
            "--format=format:INFO%x09%H%x09%an%x09%ad%x09%B---SPLIT_FILES---",
            commit_hash,
        ])
        .output()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut hash = commit_hash.to_string();
    let mut author = String::new();
    let mut date = String::new();
    let mut full_message = String::new();
    let mut files = Vec::new();

    if let Some(split_pos) = stdout.find("---SPLIT_FILES---") {
        let header_part = &stdout[..split_pos];
        let files_part = &stdout[split_pos + "---SPLIT_FILES---".len()..];

        if let Some(info_payload) = header_part.strip_prefix("INFO\t") {
            let mut parts = info_payload.splitn(4, '\t');
            if let Some(h) = parts.next() {
                hash = h.to_string();
            }
            if let Some(a) = parts.next() {
                author = a.to_string();
            }
            if let Some(d) = parts.next() {
                date = d.to_string();
            }
            if let Some(m) = parts.next() {
                full_message = m.trim().to_string();
            }
        }

        for file_line in files_part.lines() {
            let line = file_line.trim();
            if line.is_empty() {
                continue;
            }
            let mut cols = line.split_whitespace();
            let status = cols.next().unwrap_or("M").to_string();
            let path = cols.next().unwrap_or("").to_string();
            if !path.is_empty() {
                files.push(ChangedFile { status, path });
            }
        }
    } else {
        full_message = stdout.to_string();
    }

    Ok(CommitDetail {
        hash,
        author,
        date,
        full_message,
        files,
    })
}

/// Loads diff for a commit (optionally limited to a specific file)
pub fn load_commit_diff(
    repo_path: &Path,
    commit_hash: &str,
    file: Option<&str>,
) -> Result<(Vec<DiffLine>, Vec<DiffLine>), Box<dyn std::error::Error>> {
    let mut cmd = Command::new("git");
    cmd.args([
        "-C",
        &repo_path.to_string_lossy(),
        "show",
        "--color=never",
        commit_hash,
    ]);

    if let Some(f) = file {
        cmd.arg("--").arg(f);
    }

    let output = cmd.output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);

    let mut full_lines = Vec::new();
    let mut diff_only_lines = Vec::new();
    let mut line_counter = 1;

    for line in stdout.lines() {
        if line.starts_with("@@") {
            let dl = DiffLine {
                kind: DiffLineKind::Hunk,
                content: line.to_string(),
                line_number: None,
            };
            full_lines.push(dl.clone());
            diff_only_lines.push(dl);
        } else if line.starts_with('+') && !line.starts_with("+++") {
            let dl = DiffLine {
                kind: DiffLineKind::Addition,
                content: line.to_string(),
                line_number: Some(line_counter),
            };
            full_lines.push(dl.clone());
            diff_only_lines.push(dl);
            line_counter += 1;
        } else if line.starts_with('-') && !line.starts_with("---") {
            let dl = DiffLine {
                kind: DiffLineKind::Deletion,
                content: line.to_string(),
                line_number: None,
            };
            full_lines.push(dl.clone());
            diff_only_lines.push(dl);
        } else if line.starts_with("diff --git")
            || line.starts_with("index ")
            || line.starts_with("--- ")
            || line.starts_with("+++ ")
        {
            let dl = DiffLine {
                kind: DiffLineKind::Info,
                content: line.to_string(),
                line_number: None,
            };
            full_lines.push(dl);
        } else {
            let dl = DiffLine {
                kind: DiffLineKind::Context,
                content: line.to_string(),
                line_number: Some(line_counter),
            };
            full_lines.push(dl);
            line_counter += 1;
        }
    }

    Ok((full_lines, diff_only_lines))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_discover_repos_pruning() {
        let temp_dir = std::env::temp_dir().join("gittop_test_discover");
        let _ = fs::remove_dir_all(&temp_dir);

        let repo1 = temp_dir.join("project_a");
        let repo1_git = repo1.join(".git");
        let _repo1_sub = repo1.join("src").join(".git"); // fake sub .git, should be ignored

        let repo2 = temp_dir.join("group").join("project_b");
        let repo2_git = repo2.join(".git");

        let ignored_target = temp_dir.join("project_c").join("target").join(".git");

        fs::create_dir_all(&repo1_git).unwrap();
        fs::create_dir_all(repo1.join("src")).unwrap();
        fs::create_dir_all(&repo2_git).unwrap();
        fs::create_dir_all(&ignored_target).unwrap();

        let repos = discover_repos(&temp_dir);

        assert!(repos.contains(&repo1));
        assert!(repos.contains(&repo2));
        assert!(!repos.contains(&repo1.join("src")));
        assert!(!repos.contains(&temp_dir.join("project_c")));

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_repo_record_sorting_order() {
        use chrono::Utc;

        let now = Utc::now();
        let r1 = RepoRecord {
            path: PathBuf::from("old"),
            name: "old".into(),
            rel_path: "old".into(),
            current_branch: "main".into(),
            last_commit_time: Some(now - chrono::Duration::hours(5)),
            last_commit_hash: "1".into(),
            last_commit_author: "A".into(),
            last_commit_message: "old".into(),
            is_dirty: false,
            modified_count: 0,
            untracked_count: 0,
            ahead: 0,
            behind: 0,
        };
        let mut r2 = r1.clone();
        r2.name = "newest".into();
        r2.last_commit_time = Some(now - chrono::Duration::minutes(5));

        let mut r3 = r1.clone();
        r3.name = "none".into();
        r3.last_commit_time = None;

        let mut records = vec![r1, r3, r2];
        records.sort_by(|a, b| match (&a.last_commit_time, &b.last_commit_time) {
            (Some(ta), Some(tb)) => tb.cmp(ta),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a.name.cmp(&b.name),
        });

        assert_eq!(records[0].name, "newest");
        assert_eq!(records[1].name, "old");
        assert_eq!(records[2].name, "none");
    }
}
