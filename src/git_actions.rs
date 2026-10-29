use crate::model::{
    BranchItem, FileChangeKind, MergeStrategy, RepoOpState, RepoStatus, StashItem, StatusFileItem,
};
use std::fs;
use std::path::Path;
use std::process::Command;

/// Parses the output of `git status --porcelain=v1 -uall`
pub fn parse_status_porcelain_v1(stdout: &str) -> RepoStatus {
    let mut staged = Vec::new();
    let mut unstaged = Vec::new();
    let mut untracked = Vec::new();

    for line in stdout.lines() {
        if line.len() < 3 {
            continue;
        }

        let x = line.as_bytes()[0] as char;
        let y = line.as_bytes()[1] as char;
        let rest = line[3..].trim();

        if x == '?' && y == '?' {
            untracked.push(StatusFileItem {
                path: clean_path(rest),
                original_path: None,
                kind: FileChangeKind::Untracked,
                is_staged: false,
            });
            continue;
        }

        let (orig_path, path) = if let Some((orig, target)) = rest.split_once(" -> ") {
            (Some(clean_path(orig)), clean_path(target))
        } else {
            (None, clean_path(rest))
        };

        // Staged changes (Index)
        if x != ' ' && x != '?' && x != '!' {
            let kind = match x {
                'M' => FileChangeKind::Modified,
                'A' => FileChangeKind::Added,
                'D' => FileChangeKind::Deleted,
                'R' => FileChangeKind::Renamed,
                'T' => FileChangeKind::TypeChanged,
                _ => FileChangeKind::Modified,
            };
            staged.push(StatusFileItem {
                path: path.clone(),
                original_path: orig_path.clone(),
                kind,
                is_staged: true,
            });
        }

        // Unstaged changes (Working Tree)
        if y != ' ' && y != '?' && y != '!' {
            let kind = match y {
                'M' => FileChangeKind::Modified,
                'D' => FileChangeKind::Deleted,
                'T' => FileChangeKind::TypeChanged,
                _ => FileChangeKind::Modified,
            };
            unstaged.push(StatusFileItem {
                path,
                original_path: orig_path,
                kind,
                is_staged: false,
            });
        }
    }

    RepoStatus {
        staged,
        unstaged,
        untracked,
    }
}

fn clean_path(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2 {
        trimmed[1..trimmed.len() - 1].to_string()
    } else {
        trimmed.to_string()
    }
}

/// Loads the working tree and staging status of a repository
pub fn load_repo_status(repo: &Path) -> Result<RepoStatus, String> {
    let output = Command::new("git")
        .args(["-C", &repo.to_string_lossy(), "status", "--porcelain=v1", "-uall"])
        .output()
        .map_err(|e| format!("Failed to execute git status: {}", e))?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(parse_status_porcelain_v1(&stdout))
}

pub fn stage_file(repo: &Path, file: &str) -> Result<(), String> {
    let output = Command::new("git")
        .args(["-C", &repo.to_string_lossy(), "add", "--", file])
        .output()
        .map_err(|e| format!("Failed to stage file: {}", e))?;

    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

pub fn unstage_file(repo: &Path, file: &str) -> Result<(), String> {
    let output = Command::new("git")
        .args(["-C", &repo.to_string_lossy(), "restore", "--staged", "--", file])
        .output()
        .map_err(|e| format!("Failed to unstage file: {}", e))?;

    if output.status.success() {
        return Ok(());
    }

    // Fallback for older git versions
    let fallback = Command::new("git")
        .args(["-C", &repo.to_string_lossy(), "reset", "HEAD", "--", file])
        .output()
        .map_err(|e| format!("Failed to reset file: {}", e))?;

    if fallback.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

pub fn stage_all(repo: &Path) -> Result<(), String> {
    let output = Command::new("git")
        .args(["-C", &repo.to_string_lossy(), "add", "-A"])
        .output()
        .map_err(|e| format!("Failed to stage all: {}", e))?;

    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

pub fn unstage_all(repo: &Path) -> Result<(), String> {
    let output = Command::new("git")
        .args(["-C", &repo.to_string_lossy(), "restore", "--staged", "."])
        .output()
        .map_err(|e| format!("Failed to unstage all: {}", e))?;

    if output.status.success() {
        return Ok(());
    }

    let fallback = Command::new("git")
        .args(["-C", &repo.to_string_lossy(), "reset", "HEAD"])
        .output()
        .map_err(|e| format!("Failed to reset all: {}", e))?;

    if fallback.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

pub fn discard_file(repo: &Path, file: &str, is_untracked: bool) -> Result<(), String> {
    if is_untracked {
        let output = Command::new("git")
            .args(["-C", &repo.to_string_lossy(), "clean", "-f", "-d", "--", file])
            .output()
            .map_err(|e| format!("Failed to clean untracked file: {}", e))?;

        if output.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
        }
    } else {
        let output = Command::new("git")
            .args(["-C", &repo.to_string_lossy(), "restore", "--", file])
            .output()
            .map_err(|e| format!("Failed to restore file: {}", e))?;

        if output.status.success() {
            return Ok(());
        }

        let fallback = Command::new("git")
            .args(["-C", &repo.to_string_lossy(), "checkout", "--", file])
            .output()
            .map_err(|e| format!("Failed to checkout file: {}", e))?;

        if fallback.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
        }
    }
}

pub fn commit(repo: &Path, message: &str, amend: bool) -> Result<String, String> {
    if message.trim().is_empty() && !amend {
        return Err("Commit message cannot be empty".to_string());
    }

    let mut cmd = Command::new("git");
    cmd.args(["-C", &repo.to_string_lossy(), "commit"]);

    if amend && message.trim().is_empty() {
        cmd.arg("--amend").arg("--no-edit");
    } else {
        if amend {
            cmd.arg("--amend");
        }
        cmd.args(["-m", message]);
    }

    let output = cmd.output().map_err(|e| format!("Failed to execute git commit: {}", e))?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if stdout.is_empty() {
            Ok("Commit succeeded".to_string())
        } else {
            Ok(stdout)
        }
    } else {
        let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if err.is_empty() {
            Err(String::from_utf8_lossy(&output.stdout).trim().to_string())
        } else {
            Err(err)
        }
    }
}

/// Spawns the user's $EDITOR to compose a commit message
pub fn open_editor_for_commit(repo: &Path, initial_msg: &str) -> Result<String, String> {
    let editor = std::env::var("EDITOR")
        .or_else(|_| std::env::var("VISUAL"))
        .unwrap_or_else(|_| "nano".to_string());

    let temp_file = repo.join(".git").join("COMMIT_EDITMSG_GITOP");
    fs::write(&temp_file, initial_msg).map_err(|e| format!("Failed to write commit temp file: {}", e))?;

    let status = Command::new(&editor)
        .arg(&temp_file)
        .status()
        .map_err(|e| format!("Failed to launch editor '{}': {}", editor, e))?;

    if !status.success() {
        let _ = fs::remove_file(&temp_file);
        return Err(format!("Editor '{}' exited with non-zero status", editor));
    }

    let content = fs::read_to_string(&temp_file)
        .map_err(|e| format!("Failed to read back commit message from temp file: {}", e))?;
    let _ = fs::remove_file(&temp_file);

    // Filter out comment lines starting with #
    let filtered_lines: Vec<&str> = content
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .collect();

    Ok(filtered_lines.join("\n").trim().to_string())
}

/// Parses output of `git for-each-ref` for branches
pub fn parse_branches(stdout: &str) -> Vec<BranchItem> {
    let mut branches = Vec::new();

    for line in stdout.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Format: "%(refname)%09%(refname:short)%09%(HEAD)%09%(upstream:short)%09%(upstream:track,nobracket)%09%(subject)"
        let parts: Vec<&str> = trimmed.split('\t').collect();
        if parts.len() < 2 {
            continue;
        }

        let full_ref = parts[0];
        let short_name = parts[1].to_string();
        let is_current = parts.get(2).map(|s| s.trim() == "*").unwrap_or(false);
        let upstream_raw = parts.get(3).copied().unwrap_or("").trim();
        let upstream = if upstream_raw.is_empty() {
            None
        } else {
            Some(upstream_raw.to_string())
        };

        let track_info = parts.get(4).copied().unwrap_or("").trim();
        let mut ahead = 0;
        let mut behind = 0;

        for token in track_info.split(',') {
            let token = token.trim();
            if let Some(rest) = token.strip_prefix("ahead ") {
                ahead = rest.parse().unwrap_or(0);
            } else if let Some(rest) = token.strip_prefix("behind ") {
                behind = rest.parse().unwrap_or(0);
            }
        }

        let summary = parts.get(5).copied().unwrap_or("").trim().to_string();
        let is_remote = full_ref.starts_with("refs/remotes/");

        branches.push(BranchItem {
            name: short_name,
            is_current,
            is_remote,
            upstream,
            ahead,
            behind,
            summary,
        });
    }

    // Sort: current first, then local branches, then remotes
    branches.sort_by(|a, b| {
        if a.is_current {
            std::cmp::Ordering::Less
        } else if b.is_current {
            std::cmp::Ordering::Greater
        } else if a.is_remote != b.is_remote {
            a.is_remote.cmp(&b.is_remote)
        } else {
            a.name.cmp(&b.name)
        }
    });

    branches
}

pub fn load_branches(repo: &Path) -> Result<Vec<BranchItem>, String> {
    let format = "%(refname)\t%(refname:short)\t%(HEAD)\t%(upstream:short)\t%(upstream:track,nobracket)\t%(subject)";
    let output = Command::new("git")
        .args([
            "-C",
            &repo.to_string_lossy(),
            "for-each-ref",
            &format!("--format={}", format),
            "refs/heads",
            "refs/remotes",
        ])
        .output()
        .map_err(|e| format!("Failed to list branches: {}", e))?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(parse_branches(&stdout))
}

pub fn checkout_branch(repo: &Path, branch: &str) -> Result<String, String> {
    let output = Command::new("git")
        .args(["-C", &repo.to_string_lossy(), "checkout", branch])
        .output()
        .map_err(|e| format!("Failed to checkout branch: {}", e))?;

    if output.status.success() {
        let msg = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if msg.is_empty() {
            Ok(format!("Switched to branch '{}'", branch))
        } else {
            Ok(msg)
        }
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

pub fn create_branch(repo: &Path, branch: &str, checkout: bool) -> Result<String, String> {
    let mut cmd = Command::new("git");
    cmd.args(["-C", &repo.to_string_lossy()]);
    if checkout {
        cmd.args(["checkout", "-b", branch]);
    } else {
        cmd.args(["branch", branch]);
    }

    let output = cmd.output().map_err(|e| format!("Failed to create branch: {}", e))?;
    if output.status.success() {
        Ok(format!("Created branch '{}'", branch))
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

pub fn delete_branch(repo: &Path, branch: &str, force: bool) -> Result<String, String> {
    let flag = if force { "-D" } else { "-d" };
    let output = Command::new("git")
        .args(["-C", &repo.to_string_lossy(), "branch", flag, branch])
        .output()
        .map_err(|e| format!("Failed to delete branch: {}", e))?;

    if output.status.success() {
        Ok(format!("Deleted branch '{}'", branch))
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

/// Parses `git stash list --format="%gd%x09%cr%x09%gs"`
pub fn parse_stashes(stdout: &str) -> Vec<StashItem> {
    let mut stashes = Vec::new();

    for (idx, line) in stdout.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let parts: Vec<&str> = trimmed.split('\t').collect();
        let selector = parts.first().copied().unwrap_or("stash@{0}").to_string();
        let date = parts.get(1).copied().unwrap_or("").to_string();
        let message = parts.get(2).copied().unwrap_or("").to_string();

        stashes.push(StashItem {
            index: idx,
            selector,
            message,
            date,
        });
    }

    stashes
}

pub fn load_stashes(repo: &Path) -> Result<Vec<StashItem>, String> {
    let output = Command::new("git")
        .args([
            "-C",
            &repo.to_string_lossy(),
            "stash",
            "list",
            "--format=%gd\t%cr\t%gs",
        ])
        .output()
        .map_err(|e| format!("Failed to list stashes: {}", e))?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(parse_stashes(&stdout))
}

pub fn stash_save(repo: &Path, message: Option<&str>) -> Result<String, String> {
    let mut cmd = Command::new("git");
    cmd.args(["-C", &repo.to_string_lossy(), "stash", "push", "-u"]);

    if let Some(msg) = message {
        if !msg.trim().is_empty() {
            cmd.args(["-m", msg]);
        }
    }

    let output = cmd.output().map_err(|e| format!("Failed to stash changes: {}", e))?;
    if output.status.success() {
        let msg = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(if msg.is_empty() { "Saved working directory and index state".to_string() } else { msg })
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

pub fn stash_pop(repo: &Path, index: usize) -> Result<String, String> {
    let selector = format!("stash@{{{}}}", index);
    let output = Command::new("git")
        .args(["-C", &repo.to_string_lossy(), "stash", "pop", &selector])
        .output()
        .map_err(|e| format!("Failed to pop stash: {}", e))?;

    if output.status.success() {
        Ok(format!("Popped {}", selector))
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

pub fn stash_drop(repo: &Path, index: usize) -> Result<String, String> {
    let selector = format!("stash@{{{}}}", index);
    let output = Command::new("git")
        .args(["-C", &repo.to_string_lossy(), "stash", "drop", &selector])
        .output()
        .map_err(|e| format!("Failed to drop stash: {}", e))?;

    if output.status.success() {
        Ok(format!("Dropped {}", selector))
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

pub fn push(repo: &Path) -> Result<String, String> {
    let output = Command::new("git")
        .args(["-C", &repo.to_string_lossy(), "push"])
        .output()
        .map_err(|e| format!("Failed to push: {}", e))?;

    if output.status.success() {
        let out = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Ok(if out.is_empty() { "Everything up-to-date".to_string() } else { out })
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

pub fn pull(repo: &Path) -> Result<String, String> {
    let output = Command::new("git")
        .args(["-C", &repo.to_string_lossy(), "pull"])
        .output()
        .map_err(|e| format!("Failed to pull: {}", e))?;

    if output.status.success() {
        let out = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(if out.is_empty() { "Already up to date.".to_string() } else { out })
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

pub fn fetch(repo: &Path) -> Result<String, String> {
    let output = Command::new("git")
        .args(["-C", &repo.to_string_lossy(), "fetch", "--all", "--prune"])
        .output()
        .map_err(|e| format!("Failed to fetch: {}", e))?;

    if output.status.success() {
        let out = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Ok(if out.is_empty() { "Fetched all remotes".to_string() } else { out })
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

pub fn merge(repo: &Path, target: &str, strategy: MergeStrategy) -> Result<String, String> {
    let mut cmd = Command::new("git");
    cmd.args(["-C", &repo.to_string_lossy(), "merge"]);

    match strategy {
        MergeStrategy::Default => {}
        MergeStrategy::NoFastForward => {
            cmd.arg("--no-ff");
        }
        MergeStrategy::FastForwardOnly => {
            cmd.arg("--ff-only");
        }
    }

    cmd.arg(target);

    let output = cmd.output().map_err(|e| format!("Failed to execute git merge: {}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

    if output.status.success() {
        let msg = if stdout.is_empty() { stderr } else { stdout };
        Ok(if msg.is_empty() { format!("Merged '{}' successfully", target) } else { msg })
    } else {
        let err = if stderr.is_empty() { stdout } else { stderr };
        Err(err)
    }
}

pub fn merge_abort(repo: &Path) -> Result<String, String> {
    let output = Command::new("git")
        .args(["-C", &repo.to_string_lossy(), "merge", "--abort"])
        .output()
        .map_err(|e| format!("Failed to abort merge: {}", e))?;

    if output.status.success() {
        Ok("Merge aborted successfully".to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

pub fn rebase(repo: &Path, target: &str, autostash: bool) -> Result<String, String> {
    let mut cmd = Command::new("git");
    cmd.args(["-C", &repo.to_string_lossy(), "rebase"]);

    if autostash {
        cmd.arg("--autostash");
    }

    cmd.arg(target);

    let output = cmd.output().map_err(|e| format!("Failed to execute git rebase: {}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

    if output.status.success() {
        let msg = if stdout.is_empty() { stderr } else { stdout };
        Ok(if msg.is_empty() { format!("Rebased onto '{}' successfully", target) } else { msg })
    } else {
        let err = if stderr.is_empty() { stdout } else { stderr };
        Err(err)
    }
}

pub fn rebase_abort(repo: &Path) -> Result<String, String> {
    let output = Command::new("git")
        .args(["-C", &repo.to_string_lossy(), "rebase", "--abort"])
        .output()
        .map_err(|e| format!("Failed to abort rebase: {}", e))?;

    if output.status.success() {
        Ok("Rebase aborted successfully".to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

pub fn rebase_continue(repo: &Path) -> Result<String, String> {
    let output = Command::new("git")
        .args(["-C", &repo.to_string_lossy(), "rebase", "--continue"])
        .output()
        .map_err(|e| format!("Failed to continue rebase: {}", e))?;

    if output.status.success() {
        let msg = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(if msg.is_empty() { "Rebase continued".to_string() } else { msg })
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

pub fn check_repo_op_state(repo: &Path) -> RepoOpState {
    let git_dir = repo.join(".git");
    if git_dir.join("MERGE_HEAD").exists() {
        RepoOpState::Merging
    } else if git_dir.join("rebase-merge").exists() || git_dir.join("rebase-apply").exists() {
        RepoOpState::Rebasing
    } else {
        RepoOpState::Normal
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_status_porcelain() {
        let sample = "M  src/app.rs\n M src/lib.rs\nMM src/model.rs\n?? new_file.txt\nA  added.rs\nD  deleted.rs\nR  old.rs -> new.rs\n";
        let status = parse_status_porcelain_v1(sample);

        // Staged: M src/app.rs, M src/model.rs, A added.rs, D deleted.rs, R new.rs
        assert_eq!(status.staged.len(), 5);
        assert_eq!(status.staged[0].path, "src/app.rs");
        assert_eq!(status.staged[0].kind, FileChangeKind::Modified);
        assert_eq!(status.staged[4].path, "new.rs");
        assert_eq!(status.staged[4].original_path.as_deref(), Some("old.rs"));

        // Unstaged: M src/lib.rs, M src/model.rs
        assert_eq!(status.unstaged.len(), 2);
        assert_eq!(status.unstaged[0].path, "src/lib.rs");
        assert_eq!(status.unstaged[1].path, "src/model.rs");

        // Untracked: new_file.txt
        assert_eq!(status.untracked.len(), 1);
        assert_eq!(status.untracked[0].path, "new_file.txt");
    }

    #[test]
    fn test_parse_branches() {
        let sample = "refs/heads/main\tmain\t*\torigin/main\tahead 1, behind 2\tfeat: something\nrefs/heads/feature\tfeature\t \torigin/feature\t\twork in progress\nrefs/remotes/origin/main\torigin/main\t \t\t\tfeat: something\n";
        let branches = parse_branches(sample);

        assert_eq!(branches.len(), 3);
        assert_eq!(branches[0].name, "main");
        assert!(branches[0].is_current);
        assert_eq!(branches[0].ahead, 1);
        assert_eq!(branches[0].behind, 2);
        assert_eq!(branches[0].upstream.as_deref(), Some("origin/main"));

        assert_eq!(branches[1].name, "feature");
        assert!(!branches[1].is_current);
        assert!(!branches[1].is_remote);

        assert_eq!(branches[2].name, "origin/main");
        assert!(branches[2].is_remote);
    }

    #[test]
    fn test_parse_stashes() {
        let sample = "stash@{0}\t2 hours ago\tWIP on main: 1234567 fix bug\nstash@{1}\t2 days ago\ton dev: test\n";
        let stashes = parse_stashes(sample);

        assert_eq!(stashes.len(), 2);
        assert_eq!(stashes[0].selector, "stash@{0}");
        assert_eq!(stashes[0].date, "2 hours ago");
        assert_eq!(stashes[0].message, "WIP on main: 1234567 fix bug");

        assert_eq!(stashes[1].selector, "stash@{1}");
        assert_eq!(stashes[1].date, "2 days ago");
    }

    #[test]
    fn test_git_operations_lifecycle() {
        let temp_dir = std::env::temp_dir().join(format!("gitop_test_lifecycle_{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        // Initialize git repository
        let init_status = Command::new("git")
            .args(["init", "-b", "main"])
            .current_dir(&temp_dir)
            .status()
            .unwrap();
        assert!(init_status.success());

        // Configure dummy user for committing
        Command::new("git")
            .args(["config", "user.name", "Test User"])
            .current_dir(&temp_dir)
            .status()
            .unwrap();
        Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(&temp_dir)
            .status()
            .unwrap();

        // 1. Create untracked file
        fs::write(temp_dir.join("test.txt"), "hello world\n").unwrap();
        let status = load_repo_status(&temp_dir).unwrap();
        assert_eq!(status.untracked.len(), 1);
        assert_eq!(status.staged.len(), 0);

        // 2. Stage file
        stage_file(&temp_dir, "test.txt").unwrap();
        let status = load_repo_status(&temp_dir).unwrap();
        assert_eq!(status.untracked.len(), 0);
        assert_eq!(status.staged.len(), 1);

        // 3. Unstage file
        unstage_file(&temp_dir, "test.txt").unwrap();
        let status = load_repo_status(&temp_dir).unwrap();
        assert_eq!(status.staged.len(), 0);
        assert_eq!(status.untracked.len(), 1);

        // 4. Stage all and commit
        stage_all(&temp_dir).unwrap();
        let commit_res = commit(&temp_dir, "initial commit", false).unwrap();
        assert!(!commit_res.is_empty());
        let status = load_repo_status(&temp_dir).unwrap();
        assert_eq!(status.total_count(), 0);

        // 5. Branch operations
        create_branch(&temp_dir, "feat/login", true).unwrap();
        let branches = load_branches(&temp_dir).unwrap();
        assert!(branches.iter().any(|b| b.name == "feat/login" && b.is_current));

        checkout_branch(&temp_dir, "main").unwrap();
        let branches = load_branches(&temp_dir).unwrap();
        assert!(branches.iter().any(|b| b.name == "main" && b.is_current));

        // 6. Stash operations
        fs::write(temp_dir.join("test.txt"), "modified for stash\n").unwrap();
        let status = load_repo_status(&temp_dir).unwrap();
        assert_eq!(status.unstaged.len(), 1);

        stash_save(&temp_dir, Some("my test stash")).unwrap();
        let status = load_repo_status(&temp_dir).unwrap();
        assert_eq!(status.total_count(), 0);

        let stashes = load_stashes(&temp_dir).unwrap();
        assert_eq!(stashes.len(), 1);
        assert!(stashes[0].message.contains("my test stash"));

        stash_pop(&temp_dir, 0).unwrap();
        let status = load_repo_status(&temp_dir).unwrap();
        assert_eq!(status.unstaged.len(), 1);

        // Clean up
        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_merge_strategy_cycle() {
        let s = MergeStrategy::Default;
        assert_eq!(s.next(), MergeStrategy::NoFastForward);
        assert_eq!(s.next().next(), MergeStrategy::FastForwardOnly);
        assert_eq!(s.next().next().next(), MergeStrategy::Default);
    }

    #[test]
    fn test_merge_and_rebase_operations() {
        let temp_dir = std::env::temp_dir().join(format!("gitop_test_merge_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        let _ = Command::new("git").args(["init", "-b", "main"]).current_dir(&temp_dir).output();
        let _ = Command::new("git").args(["config", "user.name", "Test"]).current_dir(&temp_dir).output();
        let _ = Command::new("git").args(["config", "user.email", "test@test.com"]).current_dir(&temp_dir).output();

        assert_eq!(check_repo_op_state(&temp_dir), RepoOpState::Normal);

        // Commit on main
        fs::write(temp_dir.join("file1.txt"), "hello main\n").unwrap();
        stage_all(&temp_dir).unwrap();
        commit(&temp_dir, "initial on main", false).unwrap();

        // Branch feature-a
        create_branch(&temp_dir, "feature-a", true).unwrap();
        fs::write(temp_dir.join("file_a.txt"), "hello feature a\n").unwrap();
        stage_all(&temp_dir).unwrap();
        commit(&temp_dir, "commit on feature a", false).unwrap();

        // Back to main and merge feature-a
        checkout_branch(&temp_dir, "main").unwrap();
        let merge_res = merge(&temp_dir, "feature-a", MergeStrategy::Default).unwrap();
        assert!(merge_res.contains("Merged") || merge_res.contains("Fast-forward"));
        assert!(temp_dir.join("file_a.txt").exists());
        assert_eq!(check_repo_op_state(&temp_dir), RepoOpState::Normal);

        // Branch feature-b off main
        create_branch(&temp_dir, "feature-b", true).unwrap();
        fs::write(temp_dir.join("conflict.txt"), "line from feature b\n").unwrap();
        stage_all(&temp_dir).unwrap();
        commit(&temp_dir, "commit on feature b", false).unwrap();

        // Back to main and create conflicting commit
        checkout_branch(&temp_dir, "main").unwrap();
        fs::write(temp_dir.join("conflict.txt"), "line from main conflict\n").unwrap();
        stage_all(&temp_dir).unwrap();
        commit(&temp_dir, "commit on main conflicting", false).unwrap();

        // Attempt merge feature-b into main -> triggers conflict!
        let conflict_merge = merge(&temp_dir, "feature-b", MergeStrategy::Default);
        assert!(conflict_merge.is_err());
        assert_eq!(check_repo_op_state(&temp_dir), RepoOpState::Merging);

        // Abort merge
        let abort_res = merge_abort(&temp_dir).unwrap();
        assert!(abort_res.contains("aborted") || abort_res.contains("Merge aborted"));
        assert_eq!(check_repo_op_state(&temp_dir), RepoOpState::Normal);

        // Switch to feature-b and rebase onto main -> triggers rebase conflict!
        checkout_branch(&temp_dir, "feature-b").unwrap();
        let conflict_rebase = rebase(&temp_dir, "main", false);
        assert!(conflict_rebase.is_err());
        assert_eq!(check_repo_op_state(&temp_dir), RepoOpState::Rebasing);

        // Abort rebase
        let abort_rebase_res = rebase_abort(&temp_dir).unwrap();
        assert!(abort_rebase_res.contains("aborted") || abort_rebase_res.contains("Rebase aborted"));
        assert_eq!(check_repo_op_state(&temp_dir), RepoOpState::Normal);

        // Clean up
        let _ = fs::remove_dir_all(&temp_dir);
    }
}
