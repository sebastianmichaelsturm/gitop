use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::Path;
use std::sync::mpsc::{channel, Receiver};

pub struct RepoWatcher {
    _watcher: RecommendedWatcher,
    pub rx: Receiver<notify::Result<Event>>,
}

impl RepoWatcher {
    pub fn new<P: AsRef<Path>>(root: P) -> notify::Result<Self> {
        let (tx, rx) = channel();

        let mut watcher = RecommendedWatcher::new(
            move |res| {
                let _ = tx.send(res);
            },
            Config::default(),
        )?;

        watcher.watch(root.as_ref(), RecursiveMode::Recursive)?;

        Ok(Self {
            _watcher: watcher,
            rx,
        })
    }

    /// Helper to determine if an event warrants refreshing repository state
    pub fn is_relevant_event(event: &Event) -> bool {
        for path in &event.paths {
            let path_str = path.to_string_lossy();
            // Ignore build artifacts and noisy folders
            if path_str.contains("target")
                || path_str.contains("node_modules")
                || path_str.contains(".cache")
                || path_str.contains(".gemini")
                || path_str.contains(".tmp")
            {
                continue;
            }

            // High relevance: Git metadata modified
            if path_str.contains(".git") {
                return true;
            }
        }
        false
    }
}
