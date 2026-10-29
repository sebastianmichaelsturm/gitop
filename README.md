# `gitop` *(synonym: `gittop`)*

A high-performance, real-time terminal dashboard and **interactive full Git client** designed to recursively discover, monitor, explore, and operate on Git repositories across your workspace.

Built with **Rust**, [`ratatui`](https://ratatui.rs/), [`git2`](https://crates.io/crates/git2) (libgit2), [`crossterm`](https://crates.io/crates/crossterm), and [`notify`](https://crates.io/crates/notify).

---

## Key Features

- **Instant Non-Blocking Startup (< 25ms)**: Launches immediately without waiting for disk traversal. Repositories stream in progressively from a background thread with an animated scanning indicator.
- **Hybrid Git Engine**:
  - **In-Memory Parallel Scan (`git2` + `rayon`)**: Reads dozens of repositories concurrently in RAM to compute commit timestamps, dirty states, and ahead/behind counts with zero subprocess overhead.
  - **Native Multi-Branch Graph (`git` CLI)**: Produces authentic, pixel-perfect branch graphs with branch and tag labels (`git log --graph --all`).
  - **Native Git Operations Engine**: All staging, committing, branching, stashing, and remote actions execute via the native `git` CLI, inheriting your `.gitconfig`, SSH keys/agents, GPG signing, credentials, and Git hooks.
- **Fleet-Wide Activity Ranking**: Repositories are strictly ordered by the timestamp of their latest commit, keeping your most active projects at the very top.
- **Real-Time Status Indicators**:
  - **Dirty State**: Displays uncommitted modifications and untracked files (e.g. `* [+2 ~1]`).
  - **Sync State**: Shows commits ahead and behind upstream tracking branches (`↑1 ↓0`).
  - **Active Branch**: Color-coded branch badge.
- **Dedicated Repository Tabs**:
  - **`[1] Log & Graph` (Default)**: Topological multi-branch graph with commit hashes, authors, relative dates, commit details, and changed files inspector.
  - **`[2] Status & Staging`**: Three-panel working tree manager (Staged Changes, Unstaged Changes, Untracked Files) with instant inline live diff preview.
  - **`[3] Branches`**: Complete local and remote branch list, active branch indicator, ahead/behind sync tracking, checkout, creation, and deletion.
  - **`[4] Stashes`**: Interactive stash stack with save, pop, drop, and diff inspection.
- **Interactive Commits**:
  - Built-in In-TUI modal for quick commits with visual cursor and Amend toggle.
  - Press <kbd>Ctrl+E</kbd> inside the modal to seamlessly launch your configured `$EDITOR` (`nvim`, `vim`, `nano`, etc.) for long, structured commit messages.
- **In-Terminal Diff Viewer Modal**:
  - Inspect diffs for commits, staged changes, working-tree modifications, untracked files, and stashes.
  - Color-coded additions (Green `+`), deletions (Red `-`), and hunks (Cyan `@@`).
  - **Diff-Only Toggle** (<kbd>Tab</kbd> / <kbd>d</kbd>): Switch between viewing only modified lines or the full file with line numbers.
- **Targeted File Watcher**:
  - Dynamically attaches OS watchers specifically to discovered `.git` directories.
  - Zero background noise from browser caches, compiler artifacts, or `AppData`.
- **Smart Tree Pruning**:
  - Built-in ignore lists for package caches (`.rustup`, `.cargo`, `node_modules`, `target`, `.nuget`, `.gradle`, `.venv`).
  - Automatically skips cloud hydration stalls (`OneDrive`, `SynologyDrive`, `Dropbox`) and Windows junction loops.
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
| <kbd>Enter</kbd> | **Inspect repository** (opens Tab 1: Log & Graph) |
| <kbd>r</kbd> | Force manual rescan |
| <kbd>q</kbd> | Quit `gitop` |

---

### 2. Repository Client (All Tabs)

| Key | Action |
| :--- | :--- |
| <kbd>1</kbd> | Switch to **Tab 1: Log & Graph** |
| <kbd>2</kbd> | Switch to **Tab 2: Status & Staging** |
| <kbd>3</kbd> | Switch to **Tab 3: Branches** |
| <kbd>4</kbd> | Switch to **Tab 4: Stashes** |
| <kbd>[</kbd> / <kbd>]</kbd> | Cycle through tabs |
| <kbd>P</kbd> | **git push** (push current branch to upstream) |
| <kbd>p</kbd> | **git pull** (pull current branch from upstream, except in Stash tab) |
| <kbd>f</kbd> / <kbd>F</kbd> | **git fetch** (`git fetch --all --prune`) |
| <kbd>r</kbd> | Refresh all repository data |
| <kbd>?</kbd> | **Open Keybindings Help Modal** |
| <kbd>Esc</kbd> or <kbd>q</kbd> | Return to Fleet Overview |

---

### 3. Tab 1: Log & Commit Graph

| Key | Action |
| :--- | :--- |
| <kbd>↑</kbd> / <kbd>k</kbd> | Select previous commit / file |
| <kbd>↓</kbd> / <kbd>j</kbd> | Select next commit / file |
| <kbd>Tab</kbd> | **Switch pane focus** (Commit Graph ⇄ Changed Files List) |
| <kbd>Enter</kbd> | **Open Diff Viewer** for selected commit or file |

---

### 4. Tab 2: Status & Staging

| Key | Action |
| :--- | :--- |
| <kbd>Tab</kbd> | **Cycle section focus** (Staged Changes ⇄ Unstaged Changes ⇄ Untracked Files) |
| <kbd>Space</kbd> | **Toggle Stage/Unstage** for selected file (`git add` / `git restore --staged`) |
| <kbd>a</kbd> | **Stage all** changes (`git add -A`) |
| <kbd>A</kbd> | **Unstage all** changes (`git restore --staged .`) |
| <kbd>c</kbd> | **Open Commit Modal** (commit staged changes) |
| <kbd>C</kbd> | **Open Commit Modal in Amend mode** (`git commit --amend`) |
| <kbd>Ctrl+E</kbd> | *(In Commit Modal)* **Launch `$EDITOR`** (`nano`, `vim`, `nvim`) |
| <kbd>Enter</kbd> | **Open Full-Screen Diff Viewer** for selected file |
| <kbd>x</kbd> | **Discard changes** for selected file (with confirmation dialog) |

---

### 5. Tab 3: Branches

| Key | Action |
| :--- | :--- |
| <kbd>Enter</kbd> or <kbd>Space</kbd> | **Checkout branch** (`git checkout <branch>`) |
| <kbd>n</kbd> | **Create new branch** (with optional immediate checkout) |
| <kbd>d</kbd> | **Delete branch** (with safety confirmation dialog) |

---

### 6. Tab 4: Stashes

| Key | Action |
| :--- | :--- |
| <kbd>s</kbd> | **Save working tree to stash** (`git stash push -u`) |
| <kbd>p</kbd> | **Pop selected stash** (`git stash pop stash@{n}`) |
| <kbd>d</kbd> | **Drop selected stash** (`git stash drop stash@{n}`) |
| <kbd>Enter</kbd> | **View stash diff** |

---

### 7. In-Terminal Diff Viewer Modal

| Key | Action |
| :--- | :--- |
| <kbd>Tab</kbd> or <kbd>d</kbd> | **Toggle Diff Mode** (Diff-Only vs Full-File with context) |
| <kbd>↑</kbd> / <kbd>k</kbd> | Scroll up 1 line |
| <kbd>↓</kbd> / <kbd>j</kbd> | Scroll down 1 line |
| <kbd>PgUp</kbd> / <kbd>b</kbd> | Scroll up 1 page |
| <kbd>PgDn</kbd> | Scroll down 1 page |
| <kbd>Home</kbd> / <kbd>g</kbd> | Jump to top of diff |
| <kbd>End</kbd> / <kbd>G</kbd> | Jump to bottom of diff |
| <kbd>Esc</kbd>, <kbd>q</kbd>, or <kbd>Enter</kbd> | Close Diff Viewer |
