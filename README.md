# `gitop` *(synonym: `gittop`)*

A high-performance, real-time terminal dashboard (like `top` or `htop` for processes) designed to recursively discover, monitor, and explore Git repositories across your workspace.

Built with **Rust**, [`ratatui`](https://ratatui.rs/), [`git2`](https://crates.io/crates/git2) (libgit2), [`crossterm`](https://crates.io/crates/crossterm), and [`notify`](https://crates.io/crates/notify).

---

## Key Features

- **Instant Non-Blocking Startup (< 25ms)**: Launches immediately without waiting for disk traversal. Repositories stream in progressively from a background thread with an animated scanning indicator.
- **Hybrid Git Engine**:
  - **In-Memory Parallel Scan (`git2` + `rayon`)**: Reads dozens of repositories concurrently in RAM to compute commit timestamps, dirty states, and ahead/behind counts with zero subprocess overhead.
  - **Native Multi-Branch Graph (`git` CLI)**: Produces authentic, pixel-perfect branch graphs with branch and tag labels (`git log --graph --all`).
- **Fleet-Wide Activity Ranking**: Repositories are strictly ordered by the timestamp of their latest commit, keeping your most active projects at the very top.
- **Real-Time Status Indicators**:
  - **Dirty State**: Displays uncommitted modifications and untracked files (e.g. `* [+2 ~1]`).
  - **Sync State**: Shows commits ahead and behind upstream tracking branches (`↑1 ↓0`).
  - **Active Branch**: Color-coded branch badge.
- **Interactive Commit Graph & Inspector**:
  - Full-screen drill-down into any repository (<kbd>Enter</kbd>).
  - Top pane: Topological multi-branch graph with commit hashes, authors, and dates.
  - Bottom pane: Full commit details (author name, email, commit date, full message) and interactive changed files list (<kbd>Tab</kbd> to focus).
- **In-Terminal Diff Viewer Modal**:
  - Opens file or commit diffs directly inside the terminal.
  - Color-coded additions (Green `+`), deletions (Red `-`), and hunks (Cyan `@@`).
  - **Diff-Only Toggle** (<kbd>Tab</kbd> / <kbd>d</kbd>): Instantly switch between viewing only modified lines or the full file with line numbers.
- **Targeted File Watcher**:
  - Dynamically attaches OS watchers specifically to discovered `.git` directories.
  - Zero background noise from browser caches, compiler artifacts, or `AppData`.
- **Smart Tree Pruning**:
  - Built-in ignore lists for package caches (`.rustup`, `.cargo`, `node_modules`, `target`, `.nuget`, `.gradle`, `.venv`).
  - Automatically skips cloud hydration stalls (`OneDrive`, `SynologyDrive`, `Dropbox`) and Windows junction loops (`Anwendungsdaten`, `Lokale Einstellungen`).
  - Configurable search depth limit (default: 5).

---

## Installation

### From Local Source
```bash
git clone https://github.com/sebastianmichaelsturm/gitop.git
cd gitop
cargo install --path . --force
```

*(Note: Both executable names **`gitop`** and **`gittop`** are installed simultaneously.)*

---

## Usage

```bash
# Monitor repositories in the current directory
gitop

# Monitor a specific workspace or development folder
gitop C:\Users\s.sturm\development

# Limit search depth (e.g., max 3 directories deep)
gitop ~/dev --depth 3

# Unlimited search depth
gitop ~/dev --depth 0

# Limit commits shown in the branch graph (default: 200)
gitop --max-commits 500

# Use the synonym executable name
gittop
```

---

## Keybindings

### 1. Fleet Overview Table

| Key | Action |
| :--- | :--- |
| <kbd>↑</kbd> / <kbd>k</kbd> | Navigate to previous repository |
| <kbd>↓</kbd> / <kbd>j</kbd> | Navigate to next repository |
| <kbd>PgUp</kbd> / <kbd>b</kbd> | Page up (10 repositories) |
| <kbd>PgDn</kbd> / <kbd>f</kbd> | Page down (10 repositories) |
| <kbd>Home</kbd> / <kbd>g</kbd> | Jump to first repository |
| <kbd>End</kbd> / <kbd>G</kbd> | Jump to last repository |
| <kbd>/</kbd> | **Interactive search/filter** by name, path, branch, or message |
| <kbd>Enter</kbd> | **Inspect repository** (opens Branch Graph & Commit Inspector) |
| <kbd>r</kbd> | Force manual rescan |
| <kbd>q</kbd> | Quit `gitop` |

### 2. Repository Detail (Branch Graph & Commits)

| Key | Action |
| :--- | :--- |
| <kbd>↑</kbd> / <kbd>k</kbd> | Select previous commit / file |
| <kbd>↓</kbd> / <kbd>j</kbd> | Select next commit / file |
| <kbd>Tab</kbd> | **Switch pane focus** (Commit Graph ⇄ Changed Files List) |
| <kbd>Enter</kbd> | **Open Diff Viewer** for selected commit or file |
| <kbd>Esc</kbd> or <kbd>q</kbd> | Return to Fleet Overview |

### 3. In-Terminal Diff Viewer Modal

| Key | Action |
| :--- | :--- |
| <kbd>Tab</kbd> or <kbd>d</kbd> | **Toggle Diff Mode** (Diff-Only vs Full-File) |
| <kbd>↑</kbd> / <kbd>k</kbd> | Scroll up 1 line |
| <kbd>↓</kbd> / <kbd>j</kbd> | Scroll down 1 line |
| <kbd>PgUp</kbd> / <kbd>b</kbd> | Scroll up 1 page |
| <kbd>PgDn</kbd> / <kbd>f</kbd> | Scroll down 1 page |
| <kbd>Home</kbd> / <kbd>g</kbd> | Jump to top of diff |
| <kbd>End</kbd> / <kbd>G</kbd> | Jump to bottom of diff |
| <kbd>Esc</kbd>, <kbd>q</kbd>, or <kbd>Enter</kbd> | Close Diff Viewer |
