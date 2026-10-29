use crate::model::{
    ChangedFile, CommitDetail, CommitGraphRow, CommitRef, DiffLine, DiffLineKind, RefKind,
    RepoRecord,
};
use chrono::DateTime;
use git2::{BranchType, Repository, StatusOptions};
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::Sender;
use walkdir::WalkDir;

pub const IGNORED_DIRS: &[&str] = &[
    // Build and package caches
    "target",
    "node_modules",
    ".cargo",
    "AppData",
    ".cache",
    ".gemini",
    ".idea",
    ".vscode",
    "__pycache__",
    "vendor",
    "dist",
    "build",
    "tmp",
    "temp",
    // Toolchains & Package managers
    ".rustup",
    ".nuget",
    ".gradle",
    ".m2",
    "scoop",
    ".local",
    ".docker",
    ".virtualenvs",
    ".conda",
    "venv",
    ".venv",
    "env",
    // Cloud sync clients (prevent cloud hydration / network stalls)
    "OneDrive",
    "OneDrive - CONENGA Group GmbH",
    "SynologyDrive",
    "Dropbox",
    "Google Drive",
    // Virtualization
    "VirtualBox VMs",
    ".VirtualBox",
    // Windows compatibility junctions & special folders
    "Anwendungsdaten",
    "Lokale Einstellungen",
    "Cookies",
    "Recent",
    "SendTo",
    "Startmenü",
    "Vorlagen",
    "Druckumgebung",
    "Netzwerkumgebung",
    "Eigene Dateien",
];

#[derive(Debug, Clone)]
pub enum ScannerMessage {
    Batch(Vec<RepoRecord>),
    Finished(usize),
}

/// Recursively scans for git repositories starting from `root` with `max_depth`.
/// Prunes recursion when a `.git` directory or ignored directory is encountered.
pub fn discover_repos(root: &Path, max_depth: usize) -> Vec<PathBuf> {
    let mut repos = Vec::new();
    let mut builder = WalkDir::new(root).follow_links(false);
    if max_depth > 0 {
        builder = builder.max_depth(max_depth);
    }
    let mut it = builder.into_iter();

    while let Some(entry) = it.next() {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        let file_type = entry.file_type();
        if file_type.is_dir() {
            let file_name = entry.file_name().to_string_lossy();

            // Skip known huge / noise / junction directories
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

/// Runs repository discovery and inspection in the background, streaming batches of records.
pub fn start_background_scan(root: PathBuf, max_depth: usize, tx: Sender<ScannerMessage>) {
    std::thread::spawn(move || {
        let mut builder = WalkDir::new(&root).follow_links(false);
        if max_depth > 0 {
            builder = builder.max_depth(max_depth);
        }
        let mut it = builder.into_iter();

        let mut discovered_batch = Vec::new();
        let mut total_found = 0;

        while let Some(entry) = it.next() {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };

            let file_type = entry.file_type();
            if file_type.is_dir() {
                let file_name = entry.file_name().to_string_lossy();

                if IGNORED_DIRS.contains(&file_name.as_ref()) {
                    it.skip_current_dir();
                    continue;
                }

                if file_name == ".git" {
                    if let Some(parent) = entry.path().parent() {
                        discovered_batch.push(parent.to_path_buf());
                    }
                    it.skip_current_dir();
                } else if entry.path().join(".git").exists() {
                    discovered_batch.push(entry.path().to_path_buf());
                    it.skip_current_dir();
                }

                // When batch reaches 5 or more, inspect and stream immediately
                if discovered_batch.len() >= 5 {
                    let records: Vec<RepoRecord> = discovered_batch
                        .par_iter()
                        .filter_map(|p| inspect_repo_in_memory(p, &root))
                        .collect();
                    total_found += records.len();
                    if tx.send(ScannerMessage::Batch(records)).is_err() {
                        return; // receiver dropped, exit thread
                    }
                    discovered_batch.clear();
                }
            }
        }

        // Flush remaining discovered repos
        if !discovered_batch.is_empty() {
            let records: Vec<RepoRecord> = discovered_batch
                .par_iter()
                .filter_map(|p| inspect_repo_in_memory(p, &root))
                .collect();
            total_found += records.len();
            let _ = tx.send(ScannerMessage::Batch(records));
        }

        let _ = tx.send(ScannerMessage::Finished(total_found));
    });
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

    sort_repo_records(&mut records);

    records
}

pub fn sort_repo_records(records: &mut [RepoRecord]) {
    records.sort_by(|a, b| match (&a.last_commit_time, &b.last_commit_time) {
        (Some(ta), Some(tb)) => tb.cmp(ta),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a.name.cmp(&b.name),
    });
}

/// Parses and intelligently prioritizes git ref decorations (%d)
pub fn parse_commit_refs(refs_raw: &str) -> Vec<CommitRef> {
    if refs_raw.trim().is_empty() {
        return Vec::new();
    }

    let trimmed = refs_raw
        .trim()
        .trim_start_matches('(')
        .trim_end_matches(')')
        .trim();

    if trimmed.is_empty() {
        return Vec::new();
    }

    let raw_tokens: Vec<&str> = trimmed
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    let mut remotes: Vec<String> = Vec::new();
    let mut non_remotes: Vec<&str> = Vec::new();

    for token in raw_tokens {
        // Skip noisy symbolic ref origin/HEAD
        if token == "origin/HEAD" || (token.ends_with("/HEAD") && !token.starts_with("HEAD")) {
            continue;
        }

        if (token.starts_with("origin/") || token.starts_with("upstream/") || token.starts_with("remotes/"))
            && !token.starts_with("HEAD")
        {
            remotes.push(token.to_string());
        } else {
            non_remotes.push(token);
        }
    }

    let mut result: Vec<CommitRef> = Vec::new();
    let mut consumed_remotes: Vec<String> = Vec::new();

    for token in non_remotes {
        if let Some(target) = token.strip_prefix("HEAD -> ") {
            let target_name = target.trim();
            let matching_remote = format!("origin/{}", target_name);
            let is_paired = remotes.contains(&matching_remote);
            if is_paired {
                consumed_remotes.push(matching_remote);
            }

            result.push(CommitRef {
                kind: RefKind::Head(target_name.to_string()),
                display_text: if is_paired {
                    format!("HEAD -> {}(=)", target_name)
                } else {
                    format!("HEAD -> {}", target_name)
                },
                is_paired_upstream: is_paired,
            });
        } else if token == "HEAD" {
            result.push(CommitRef {
                kind: RefKind::Head("HEAD".to_string()),
                display_text: "HEAD".to_string(),
                is_paired_upstream: false,
            });
        } else if let Some(tag_name) = token.strip_prefix("tag: ") {
            result.push(CommitRef {
                kind: RefKind::Tag(tag_name.trim().to_string()),
                display_text: tag_name.trim().to_string(),
                is_paired_upstream: false,
            });
        } else if token == "main" || token == "master" || token == "develop" || token == "trunk" {
            let matching_remote = format!("origin/{}", token);
            let is_paired = remotes.contains(&matching_remote);
            if is_paired {
                consumed_remotes.push(matching_remote);
            }

            result.push(CommitRef {
                kind: RefKind::Mainline(token.to_string()),
                display_text: if is_paired {
                    format!("{}(=)", token)
                } else {
                    token.to_string()
                },
                is_paired_upstream: is_paired,
            });
        } else {
            let matching_remote = format!("origin/{}", token);
            let is_paired = remotes.contains(&matching_remote);
            if is_paired {
                consumed_remotes.push(matching_remote);
            }

            result.push(CommitRef {
                kind: RefKind::LocalBranch(token.to_string()),
                display_text: if is_paired {
                    format!("{}(=)", token)
                } else {
                    token.to_string()
                },
                is_paired_upstream: is_paired,
            });
        }
    }

    for r in remotes {
        if !consumed_remotes.contains(&r) {
            result.push(CommitRef {
                kind: RefKind::RemoteBranch(r.clone()),
                display_text: r,
                is_paired_upstream: false,
            });
        }
    }

    result.sort_by(|a, b| {
        let rank = |k: &RefKind| match k {
            RefKind::Head(_) => 0,
            RefKind::Mainline(_) => 1,
            RefKind::LocalBranch(_) => 2,
            RefKind::Tag(_) => 3,
            RefKind::RemoteBranch(_) => 4,
        };
        let r_a = rank(&a.kind);
        let r_b = rank(&b.kind);
        if r_a != r_b {
            r_a.cmp(&r_b)
        } else {
            a.display_text.cmp(&b.display_text)
        }
    });

    result
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

            let parsed_refs = parse_commit_refs(refs_raw);

            rows.push(CommitGraphRow {
                graph_prefix,
                hash,
                refs,
                parsed_refs,
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
                parsed_refs: Vec::new(),
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
            "--format=format:INFO%x09%H%x09%an <%ae>%x09%ad%x09%d%x09%B---SPLIT_FILES---",
            commit_hash,
        ])
        .output()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut hash = commit_hash.to_string();
    let mut author = String::new();
    let mut date = String::new();
    let mut refs = Vec::new();
    let mut full_message = String::new();
    let mut files = Vec::new();

    if let Some(split_pos) = stdout.find("---SPLIT_FILES---") {
        let header_part = &stdout[..split_pos];
        let files_part = &stdout[split_pos + "---SPLIT_FILES---".len()..];

        if let Some(info_payload) = header_part.strip_prefix("INFO\t") {
            let mut parts = info_payload.splitn(5, '\t');
            if let Some(h) = parts.next() {
                hash = h.to_string();
            }
            if let Some(a) = parts.next() {
                author = a.to_string();
            }
            if let Some(d) = parts.next() {
                date = d.to_string();
            }
            if let Some(r) = parts.next() {
                refs = parse_commit_refs(r);
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
        refs,
        files,
    })
}

/// Parses git diff output into full lines and diff-only lines
pub fn parse_diff_output(stdout: &str) -> (Vec<DiffLine>, Vec<DiffLine>) {
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

    (full_lines, diff_only_lines)
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
    Ok(parse_diff_output(&stdout))
}

/// Loads working tree diff (staged or unstaged) for a file
pub fn load_working_tree_diff(
    repo_path: &Path,
    file: &str,
    staged: bool,
) -> Result<(Vec<DiffLine>, Vec<DiffLine>), Box<dyn std::error::Error>> {
    let mut cmd = Command::new("git");
    cmd.args([
        "-C",
        &repo_path.to_string_lossy(),
        "diff",
        "--color=never",
    ]);

    if staged {
        cmd.arg("--cached");
    }

    cmd.arg("--").arg(file);

    let output = cmd.output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(parse_diff_output(&stdout))
}

/// Loads diff for an untracked file (shows entire file as additions)
pub fn load_untracked_diff(
    repo_path: &Path,
    file: &str,
) -> Result<(Vec<DiffLine>, Vec<DiffLine>), Box<dyn std::error::Error>> {
    let full_path = repo_path.join(file);
    let content = match std::fs::read_to_string(&full_path) {
        Ok(c) => c,
        Err(_) => "(binary or inaccessible file)".to_string(),
    };

    let mut full_lines = Vec::new();
    let mut diff_only_lines = Vec::new();

    full_lines.push(DiffLine {
        kind: DiffLineKind::Info,
        content: format!("--- /dev/null (untracked)"),
        line_number: None,
    });
    full_lines.push(DiffLine {
        kind: DiffLineKind::Info,
        content: format!("+++ b/{}", file),
        line_number: None,
    });

    for (idx, line) in content.lines().enumerate() {
        let dl = DiffLine {
            kind: DiffLineKind::Addition,
            content: format!("+{}", line),
            line_number: Some(idx + 1),
        };
        full_lines.push(dl.clone());
        diff_only_lines.push(dl);
    }

    Ok((full_lines, diff_only_lines))
}

/// Loads diff for a stash
pub fn load_stash_diff(
    repo_path: &Path,
    selector: &str,
) -> Result<(Vec<DiffLine>, Vec<DiffLine>), Box<dyn std::error::Error>> {
    let output = Command::new("git")
        .args([
            "-C",
            &repo_path.to_string_lossy(),
            "stash",
            "show",
            "-p",
            "--color=never",
            selector,
        ])
        .output()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(parse_diff_output(&stdout))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_discover_repos_pruning() {
        let temp_dir = std::env::temp_dir().join("gitop_test_discover");
        let _ = fs::remove_dir_all(&temp_dir);

        let repo1 = temp_dir.join("project_a");
        let repo1_git = repo1.join(".git");
        let _repo1_sub = repo1.join("src").join(".git");

        let repo2 = temp_dir.join("group").join("project_b");
        let repo2_git = repo2.join(".git");

        let ignored_target = temp_dir.join("project_c").join("target").join(".git");
        let ignored_rustup = temp_dir.join(".rustup").join("toolchains").join(".git");

        fs::create_dir_all(&repo1_git).unwrap();
        fs::create_dir_all(repo1.join("src")).unwrap();
        fs::create_dir_all(&repo2_git).unwrap();
        fs::create_dir_all(&ignored_target).unwrap();
        fs::create_dir_all(&ignored_rustup).unwrap();

        let repos = discover_repos(&temp_dir, 5);

        assert!(repos.contains(&repo1));
        assert!(repos.contains(&repo2));
        assert!(!repos.contains(&repo1.join("src")));
        assert!(!repos.contains(&temp_dir.join("project_c")));
        assert!(!repos.contains(&temp_dir.join(".rustup")));

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_discover_repos_depth_limit() {
        let temp_dir = std::env::temp_dir().join("gitop_test_depth");
        let _ = fs::remove_dir_all(&temp_dir);

        let shallow_repo = temp_dir.join("shallow");
        let deep_repo = temp_dir.join("d1").join("d2").join("d3").join("d4").join("d5").join("d6");

        fs::create_dir_all(shallow_repo.join(".git")).unwrap();
        fs::create_dir_all(deep_repo.join(".git")).unwrap();

        // Max depth 3 should only find shallow
        let repos = discover_repos(&temp_dir, 3);
        assert!(repos.contains(&shallow_repo));
        assert!(!repos.contains(&deep_repo));

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
        sort_repo_records(&mut records);

        assert_eq!(records[0].name, "newest");
        assert_eq!(records[1].name, "old");
        assert_eq!(records[2].name, "none");
    }

    #[test]
    fn test_parse_commit_refs_prioritization_and_pairing() {
        // Raw string where main is hidden at the end after tags and remotes
        let raw = " (tag: v0.1.0, origin/main, origin/HEAD, main, feat/cargo-metadata)";
        let refs = parse_commit_refs(raw);

        // Expect 3 refs:
        // 1. main (paired with origin/main -> main(=))
        // 2. feat/cargo-metadata
        // 3. tag v0.1.0
        // (origin/HEAD should be pruned)
        assert_eq!(refs.len(), 3);

        // main must be FIRST!
        assert_eq!(refs[0].display_text, "main(=)");
        assert!(matches!(refs[0].kind, RefKind::Mainline(_)));
        assert!(refs[0].is_paired_upstream);

        assert_eq!(refs[1].display_text, "feat/cargo-metadata");
        assert!(matches!(refs[1].kind, RefKind::LocalBranch(_)));

        assert_eq!(refs[2].display_text, "v0.1.0");
        assert!(matches!(refs[2].kind, RefKind::Tag(_)));

        // Test HEAD priority
        let raw_head = " (HEAD -> feat/test, main, origin/main)";
        let head_refs = parse_commit_refs(raw_head);
        assert_eq!(head_refs.len(), 2);
        assert_eq!(head_refs[0].display_text, "HEAD -> feat/test");
        assert!(matches!(head_refs[0].kind, RefKind::Head(_)));
        assert_eq!(head_refs[1].display_text, "main(=)");
    }
}
