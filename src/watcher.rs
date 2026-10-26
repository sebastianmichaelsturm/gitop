use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};

pub struct RepoWatcher {
    watcher: RecommendedWatcher,
    pub rx: Receiver<notify::Result<Event>>,
    watched_paths: HashSet<PathBuf>,
}

impl RepoWatcher {
    pub fn new() -> notify::Result<Self> {
        let (tx, rx) = channel();

        let watcher = RecommendedWatcher::new(
            move |res| {
                let _ = tx.send(res);
            },
            Config::default(),
        )?;

        Ok(Self {
            watcher,
            rx,
            watched_paths: HashSet::new(),
        })
    }

    /// Attaches the watcher specifically to a repository's .git directory
    pub fn watch_repo<P: AsRef<Path>>(&mut self, repo_path: P) {
        let p = repo_path.as_ref();
        let git_dir = if p.ends_with(".git") {
            p.to_path_buf()
        } else {
            p.join(".git")
        };

        if git_dir.exists() && !self.watched_paths.contains(&git_dir) {
            let _ = self.watcher.watch(&git_dir, RecursiveMode::Recursive);
            self.watched_paths.insert(git_dir);
        }
    }

    /// Helper to determine if an event warrants refreshing repository state
    pub fn is_relevant_event(event: &Event) -> bool {
        for path in &event.paths {
            let path_str = path.to_string_lossy();
            if path_str.contains(".git") {
                return true;
            }
        }
        false
    }
}
