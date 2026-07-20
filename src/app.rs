use std::cell::Cell;
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::config::{self, Config};
use crate::git::{self, Commit};

/// Possible screens the application can be in.
#[derive(Clone, Debug, PartialEq)]
pub enum Screen {
    /// Initial state while git data is being fetched.
    Loading,
    /// Commit list (main screen).
    List,
    /// Commit detail overlay.
    Detail,
    /// Error screen with a message.
    Error(String),
    /// Modal alert that requires user acknowledgment.
    Alert(AlertKind),
}

/// Discriminated reason for a modal alert.
#[derive(Clone, Debug, PartialEq)]
pub enum AlertKind {
    /// The commit being viewed in Detail no longer exists in the repository.
    CommitDeleted { oid: String },
}

/// A transient non-blocking notification displayed in the footer.
#[derive(Clone, Debug)]
pub struct Notification {
    pub message: String,
    pub expires_at: Instant,
}

/// Outcome of an Open in Hunk launch, reported back to the app by the
/// event loop that performed the side effects.
#[derive(Clone, Debug, PartialEq)]
pub enum HunkOutcome {
    /// Hunk ran and exited successfully — resume silently.
    Success,
    /// Hunk could not be started or exited non-zero. Carries a
    /// user-facing message for the footer notification.
    Failed(String),
}

/// Specification for a pending Hunk launch.
/// The event loop reads this and dispatches the appropriate hunk command.
#[derive(Clone, Debug, PartialEq)]
pub enum HunkLaunch {
    /// Launch `hunk show <oid>` — review a single commit (all files).
    ShowCommit(String),
    /// Launch `hunk show <oid> -- <paths...>` — review selected files of a commit.
    ShowCommitFiltered(String, Vec<String>),
    /// Launch `hunk diff <older>..<newer>` — review a range of commits (all files).
    ShowRange(String, String),
    /// Launch `hunk diff <older>..<newer> -- <paths...>` — review selected files in range.
    ShowRangeFiltered(String, String, Vec<String>),
}

/// Central application state.
pub struct App {
    pub screen: Screen,
    pub commits: Vec<Commit>,
    pub selected_index: usize,
    pub list_scroll: Cell<usize>,
    /// Height of the list viewport (updated by render for page-scroll calculations).
    pub list_content_height: Cell<usize>,
    pub selected_commit: Option<Commit>,
    pub colors: config::Colors,
    pub should_quit: bool,
    pub detail_scroll: Cell<usize>,
    /// Height of the scrollable content area (updated by render for page-scroll calculations).
    pub detail_content_height: Cell<usize>,
    /// Whether the keybindings help overlay is shown.
    pub show_help: bool,
    /// Current git branch name (e.g. `"main"`).
    pub current_branch: String,

    // ── Live-sync state ──────────────────────────────────────────
    /// Transient footer notification.
    pub notification: Option<Notification>,
    /// Once true, the polling loop stops permanently (fatal repo error).
    pub polling_stopped: bool,
    /// The HEAD commit OID from the last successful poll (used for change detection).
    pub current_head_oid: Option<String>,
    /// Polling interval in milliseconds.
    pub poll_interval_ms: u64,
    /// How long a notification stays visible before auto-dismiss (ms).
    pub notification_timeout_ms: u64,
    /// Timestamp of the last poll (for throttling).
    pub last_poll_time: Instant,
    /// Filesystem path of the Git repository being viewed.
    pub repo_path: PathBuf,
        /// Pending Open in Hunk launch, set by `open_in_hunk` and consumed by
    /// the event loop which performs the actual launch.
    pub pending_hunk_launch: Option<HunkLaunch>,
    /// When set, marks the commit at this index as the start of a range.
    /// Pressing `h` while a range start is set launches
    /// `hunk diff <older>..<newer>` instead of `hunk show <oid>`.
    pub range_start: Option<usize>,
    /// Active file selection overlay, if any.
    pub file_selection: Option<FileSelection>,
}

impl App {
    /// Create a new app in the Loading state.
    pub fn new(config: Config, repo_path: PathBuf) -> Self {
        // Best-effort branch detection — fall back to "?" on failure.
        let branch = git::open_repo(&repo_path)
            .and_then(|repo| git::current_branch(&repo))
            .unwrap_or_else(|_| "?".to_string());

        App {
            screen: Screen::Loading,
            commits: Vec::new(),
            selected_index: 0,
            list_scroll: Cell::new(0),
            list_content_height: Cell::new(0),
            selected_commit: None,
            colors: config.colors,
            should_quit: false,
            detail_scroll: Cell::new(0),
            detail_content_height: Cell::new(0),
            current_branch: branch,
            show_help: false,
            notification: None,
            polling_stopped: false,
            current_head_oid: None,
            poll_interval_ms: config.poll_interval_ms,
            notification_timeout_ms: config.notification_timeout_ms,
            range_start: None,
            file_selection: None,
            // Initialise to the past so the first poll always runs.
            last_poll_time: Instant::now() - Duration::from_millis(config.poll_interval_ms + 1),
            repo_path,
            pending_hunk_launch: None,
        }
    }

    /// Load commits from the git repository.
    /// Transitions to `List` on success, `Error` on failure.
    pub fn load_commits(&mut self) {
        match git::load_commits(&self.repo_path) {
            Ok(commits) => {
                self.commits = commits;
                if self.commits.is_empty() {
                    self.screen =
                        Screen::Error("このリポジトリにはまだコミットがありません。".into());
                } else {
                    self.screen = Screen::List;
                }
            }
            Err(e) => {
                self.screen = Screen::Error(e.to_string());
            }
        }
    }

    /// Run one polling cycle.
    ///
    /// 1. Throttle to `poll_interval_ms`.
    /// 2. Open repository and check HEAD OID.
    /// 3. Reload commits.
    /// 4. If HEAD changed → notification + close detail + reload list.
    /// 5. If HEAD unchanged + in Detail → update timestamp / detect deletion.
    /// 6. Replace commit list while OID-tracking the selection.
    pub fn poll(&mut self) {
        if self.polling_stopped {
            self.tick_notification();
            return;
        }

        let now = Instant::now();

        // Throttle: don't poll more often than the configured interval.
        if now - self.last_poll_time < Duration::from_millis(self.poll_interval_ms) {
            self.tick_notification();
            return;
        }
        self.last_poll_time = now;
        self.tick_notification();

        // 1. Open repository
        let repo = match git::open_repo(&self.repo_path) {
            Ok(r) => r,
            Err(e) => {
                self.screen = Screen::Error(e.to_string());
                self.polling_stopped = true;
                return;
            }
        };

        // 2. Get current HEAD OID
        let new_head_oid = match git::current_head_oid(&repo) {
            Ok(oid) => oid,
            Err(e) => {
                self.screen = Screen::Error(e.to_string());
                self.polling_stopped = true;
                return;
            }
        };

        // 3. Load commits
        let new_commits = match git::load_commits_from(&repo) {
            Ok(c) => c,
            Err(e) => {
                self.screen = Screen::Error(e.to_string());
                self.polling_stopped = true;
                return;
            }
        };

        // 4. First poll after startup: just record HEAD + branch, no change detection.
        if self.current_head_oid.is_none() {
            self.current_head_oid = Some(new_head_oid.clone());
            self.current_branch = git::current_branch(&repo).unwrap_or_else(|_| "?".to_string());
            self.replace_commits(new_commits);
            return;
        }

        // 5. HEAD change detection
        let old_head = self.current_head_oid.clone();
        let head_changed = old_head.as_ref() != Some(&new_head_oid);

        if head_changed {
            // Build notification BEFORE moving new_head_oid.
            let old_short = old_head
                .as_ref()
                .map(|o| &o[..7.min(o.len())])
                .unwrap_or("?");
            let new_short = new_head_oid[..7.min(new_head_oid.len())].to_string();
            self.current_head_oid = Some(new_head_oid);
            self.current_branch = git::current_branch(&repo).unwrap_or_else(|_| "?".to_string());
            self.set_notification(format!("HEAD moved: {} → {}", old_short, new_short));

            // Close Detail if it was open
            if matches!(self.screen, Screen::Detail) {
                self.screen = Screen::List;
                self.selected_commit = None;
                self.detail_scroll.set(0);
            }

            self.replace_commits(new_commits);
        } else {
            // HEAD unchanged — still update timestamps
            if let (Screen::Detail, Some(sel)) = (&self.screen, self.selected_commit.clone()) {
                let oid = sel.oid.clone();
                if let Some(nc) = new_commits.iter().find(|c| c.oid == oid) {
                    // Update timestamp on the detail commit
                    if let Some(ref mut sc) = self.selected_commit {
                        sc.date = nc.date.clone();
                        sc.author = nc.author.clone();
                    }
                } else {
                    // Selected commit no longer reachable — check if object exists
                    let gone = git::object_exists(&repo, &oid)
                        .map(|exists| !exists)
                        .unwrap_or(true);
                    if gone {
                        self.screen = Screen::Alert(AlertKind::CommitDeleted { oid: oid.clone() });
                        self.commits = new_commits;
                        return;
                    }
                }
            }

            self.replace_commits(new_commits);
        }
    }

    // ── Notification helpers ────────────────────────────────────

    /// Create or overwrite the footer notification with a timeout.
    pub fn set_notification(&mut self, message: String) {
        let timeout_ms = self.notification_timeout_ms.max(100);
        self.notification = Some(Notification {
            message,
            expires_at: Instant::now() + Duration::from_millis(timeout_ms),
        });
    }

    /// Clear expired notification based on real time.
    fn tick_notification(&mut self) {
        if let Some(ref notif) = self.notification {
            if Instant::now() >= notif.expires_at {
                self.notification = None;
            }
        }
    }

    // ── Commit list management ───────────────────────────────────

    /// Replace the commit list while OID-tracking the current selection.
    fn replace_commits(&mut self, new_commits: Vec<Commit>) {
        let target_oid = self.commits.get(self.selected_index).map(|c| c.oid.clone());
        self.commits = new_commits;
        self.selected_index = target_oid
            .and_then(|oid| self.commits.iter().position(|c| c.oid == oid))
            .unwrap_or(0);
    }

    // ── Navigation ───────────────────────────────────────────────

    /// Move selection up (towards older commits).
    pub fn navigate_up(&mut self) {
        if self.screen == Screen::List && !self.commits.is_empty() {
            if self.selected_index > 0 {
                self.selected_index -= 1;
            }
        }
    }

    /// Move selection down (towards newer commits).
    pub fn navigate_down(&mut self) {
        if self.screen == Screen::List && !self.commits.is_empty() {
            if self.selected_index < self.commits.len().saturating_sub(1) {
                self.selected_index += 1;
            }
        }
    }

    /// Move selection up by one page in the commit list.
    pub fn navigate_page_up(&mut self) {
        if self.screen == Screen::List && !self.commits.is_empty() {
            let page = self.list_content_height.get().max(1);
            self.selected_index = self.selected_index.saturating_sub(page);
        }
    }

    /// Move selection down by one page in the commit list.
    pub fn navigate_page_down(&mut self) {
        if self.screen == Screen::List && !self.commits.is_empty() {
            let page = self.list_content_height.get().max(1);
            let max_index = self.commits.len().saturating_sub(1);
            self.selected_index = (self.selected_index + page).min(max_index);
        }
    }

    /// Select the current commit and load its detail (body + diff).
    /// Transitions to `Detail` on success, stays on `List` on error.
    pub fn select_commit(&mut self) {
        if self.screen != Screen::List {
            return;
        }
        if self.commits.is_empty() || self.selected_index >= self.commits.len() {
            return;
        }

        let selected = &self.commits[self.selected_index];
        let oid = selected.oid.clone();

        self.detail_scroll.set(0);

        match git::load_diff(&self.repo_path, &oid) {
            Ok((body, diff)) => {
                let mut commit = selected.clone();
                commit.body = body;
                commit.diff = diff;
                self.selected_commit = Some(commit);
                self.screen = Screen::Detail;
            }
            Err(_e) => {
                // On error, stay on list (diff loading failed silently).
                let mut commit = selected.clone();
                commit.body = String::new();
                commit.diff = String::new();
                self.selected_commit = Some(commit);
                self.screen = Screen::Detail;
            }
        }
    }

    /// Close the detail overlay and return to the list.
    pub fn close_detail(&mut self) {
        if self.screen == Screen::Detail {
            self.screen = Screen::List;
            self.selected_commit = None;
            self.detail_scroll.set(0);
        }
    }

    /// Dismiss the alert and return to the commit list.
    pub fn dismiss_alert(&mut self) {
        if matches!(self.screen, Screen::Alert(_)) {
            self.clear_range();
            self.screen = Screen::List;
            self.selected_commit = None;
            self.detail_scroll.set(0);
        }
    }

    /// Scroll up in the detail view (towards earlier content).
    pub fn scroll_detail_up(&mut self) {
        if self.screen == Screen::Detail && self.detail_scroll.get() > 0 {
            self.detail_scroll.set(self.detail_scroll.get() - 1);
        }
    }

    /// Scroll down in the detail view (towards later content).
    pub fn scroll_detail_down(&mut self) {
        // Ceiling is enforced in the render function where we know content height.
        if self.screen == Screen::Detail {
            self.detail_scroll.set(self.detail_scroll.get() + 1);
        }
    }

    /// Scroll up by one page in the detail view.
    pub fn scroll_detail_page_up(&mut self) {
        if self.screen == Screen::Detail && self.detail_scroll.get() > 0 {
            let page = self.detail_content_height.get().max(1);
            let new = self.detail_scroll.get().saturating_sub(page);
            // Clamp: don't overshoot the start when `page` is large.
            self.detail_scroll.set(if new > self.detail_scroll.get() {
                0
            } else {
                new
            });
        }
    }

    /// Scroll down by one page in the detail view.
    pub fn scroll_detail_page_down(&mut self) {
        // Ceiling is enforced in the render function where we know content height.
        if self.screen == Screen::Detail {
            let page = self.detail_content_height.get().max(1);
            self.detail_scroll
                .set(self.detail_scroll.get().saturating_add(page));
        }
    }

    /// Return the full OID of the currently focused commit, if any.
    ///
    /// Works in both `List` (selected commit in the list) and `Detail`
    /// (the commit whose detail is being viewed). Returns `None` in any
    /// other screen or when no commit is available.
    pub fn current_commit_oid(&self) -> Option<String> {
        match &self.screen {
            Screen::List => self.commits.get(self.selected_index).map(|c| c.oid.clone()),
            Screen::Detail => self.selected_commit.as_ref().map(|c| c.oid.clone()),
            _ => None,
        }
    }

    /// Set the quit flag.
    pub fn quit(&mut self) {
        self.should_quit = true;
    }

    /// Record an Open in Hunk launch for the currently focused commit or range.
    ///
    /// Only records the intent; the event loop performs the actual launch
    /// (suspend the TUI, run hunk, resume) and reports the result back via
    /// `on_hunk_finished`. Does nothing on screens without a focused commit.
    ///
    /// When a range start is set (List view only), launches
    /// `hunk diff <older>..<newer>` covering all commits between the mark
    /// and the current selection.  If mark and selection are the same commit
    /// it falls back to a single-commit show.
    pub fn open_in_hunk(&mut self) {
        match &self.screen {
            Screen::List => {
                if let Some(oid) = self.current_commit_oid() {
                    if let Some(start_idx) = self.range_start.take() {
                        let older_idx = start_idx.max(self.selected_index);
                        let newer_idx = start_idx.min(self.selected_index);

                        if older_idx == newer_idx {
                            // Same commit → fall back to single
                            self.pending_hunk_launch =
                                Some(HunkLaunch::ShowCommit(oid));
                        } else {
                            let older_oid = self.commits[older_idx].oid.clone();
                            let newer_oid = self.commits[newer_idx].oid.clone();
                            self.pending_hunk_launch =
                                Some(HunkLaunch::ShowRange(older_oid, newer_oid));
                        }
                    } else {
                        self.pending_hunk_launch =
                            Some(HunkLaunch::ShowCommit(oid));
                    }
                }
            }
            Screen::Detail => {
                if let Some(oid) = self.current_commit_oid() {
                    self.pending_hunk_launch =
                        Some(HunkLaunch::ShowCommit(oid));
                }
            }
            _ => {}
        }
    }

    /// Open the file selection overlay for the currently focused commit.
    /// Loads the changed file list from git.  Silently ignores errors
    /// (the user can still use plain hunk launch).
    pub fn open_file_selection(&mut self) {
        // Don't re-open if already selecting
        if self.file_selection.is_some() {
            return;
        }
        let oid = match self.current_commit_oid() {
            Some(o) => o,
            None => return,
        };

        match git::load_changed_files(&self.repo_path, &oid) {
            Ok(files) if !files.is_empty() => {
                self.file_selection = Some(FileSelection::new(oid, files));
            }
            _ => {}
        }
    }

    /// Close the file selection overlay without launching hunk.
    pub fn close_file_selection(&mut self) {
        self.file_selection = None;
    }

    /// Confirm the current file selection and launch hunk with the
    /// chosen files.  If all files are selected (the default) the
    /// unfiltered HunkLaunch variant is used (plain `hunk show` / `diff`).
    /// If no files are selected the overlay stays open with a notification.
    pub fn confirm_file_selection(&mut self) {
        let fs = match self.file_selection.take() {
            Some(fs) => fs,
            None => return,
        };

        if fs.selected.is_empty() {
            self.file_selection = Some(fs);
            self.set_notification(
                "No files selected — select at least one file".into(),
            );
            return;
        }

        let paths = fs.selected_paths();

        if let Some(start_idx) = self.range_start.take() {
            // Range mode
            let older_idx = start_idx.max(self.selected_index);
            let newer_idx = start_idx.min(self.selected_index);
            let older_oid = self.commits[older_idx].oid.clone();
            let newer_oid = self.commits[newer_idx].oid.clone();
            self.pending_hunk_launch = match paths {
                Some(p) => Some(HunkLaunch::ShowRangeFiltered(
                    older_oid, newer_oid, p,
                )),
                None => Some(HunkLaunch::ShowRange(older_oid, newer_oid)),
            };
        } else {
            // Single commit mode — use the OID captured when file
            // selection was opened.
            let oid = fs.commit_oid;
            self.pending_hunk_launch = match paths {
                Some(p) => Some(HunkLaunch::ShowCommitFiltered(oid, p)),
                None => Some(HunkLaunch::ShowCommit(oid)),
            };
        }
    }

    /// Apply the outcome of a finished Open in Hunk launch.
    /// Failures surface as a footer notification; success resumes silently.
    pub fn on_hunk_finished(&mut self, outcome: HunkOutcome) {
        if let HunkOutcome::Failed(message) = outcome {
            self.set_notification(message);
        }
    }

    /// Reload the commit list from the current branch.
    ///
    /// Re-opens the repository, re-fetches the branch name and commits,
    /// closes the detail view, and resets the selection to the top.
    pub fn reload(&mut self) {
        self.clear_range();

        let repo = match git::open_repo(&self.repo_path) {
            Ok(r) => r,
            Err(e) => {
                self.screen = Screen::Error(e.to_string());
                self.polling_stopped = true;
                return;
            }
        };

        // Update branch name.
        self.current_branch = git::current_branch(&repo).unwrap_or_else(|_| "?".to_string());

        // Update HEAD OID.
        self.current_head_oid = git::current_head_oid(&repo).ok();

        // Reload commits.
        match git::load_commits_from(&repo) {
            Ok(commits) => {
                self.commits = commits;
                if self.commits.is_empty() {
                    self.screen =
                        Screen::Error("このリポジトリにはまだコミットがありません。".into());
                } else {
                    self.screen = Screen::List;
                }
            }
            Err(e) => {
                self.screen = Screen::Error(e.to_string());
                self.polling_stopped = true;
                return;
            }
        }

        // Reset selection and scroll state.
        self.selected_index = 0;
        self.list_scroll.set(0);
        self.selected_commit = None;
        self.detail_scroll.set(0);
        self.detail_content_height.set(0);
    }

    /// Dismiss the error screen (equivalent to quitting).
    pub fn error_dismiss(&mut self) {
        if matches!(self.screen, Screen::Error(_)) {
            self.quit();
        }
    }

    /// Toggle the range start mark at the current selection.
    ///
    /// - If the current commit is already marked → unmark.
    /// - Otherwise → mark the current commit (moves the mark if one
    ///   was already set elsewhere).
    /// Does nothing outside of List view.
    pub fn toggle_range_start(&mut self) {
        if self.screen != Screen::List {
            return;
        }
        if self.range_start == Some(self.selected_index) {
            self.range_start = None;
        } else {
            self.range_start = Some(self.selected_index);
        }
    }

    /// Clear the range start mark, if any.
    pub fn clear_range(&mut self) {
        self.range_start = None;
    }
}

// ── File selection ──────────────────────────────────────────────────

/// Tracks the in-progress file selection overlay that lets the user
/// pick which changed files to forward to hunk.
#[derive(Clone, Debug)]
pub struct FileSelection {
    /// OID of the commit whose file list is shown.
    pub commit_oid: String,
    /// All changed file paths for that commit (sorted).
    pub files: Vec<String>,
    /// Indices of currently selected files.
    pub selected: HashSet<usize>,
    /// Cursor index into `files`.
    pub cursor: usize,
    /// Scroll offset for the file list viewport.
    pub scroll: Cell<usize>,
}

impl FileSelection {
    /// Create a new selection with all files pre-selected.
    pub fn new(commit_oid: String, files: Vec<String>) -> Self {
        let count = files.len();
        let all: HashSet<usize> = (0..count).collect();
        FileSelection {
            commit_oid,
            files,
            selected: all,
            cursor: 0,
            scroll: Cell::new(0),
        }
    }

    /// Return the list of selected file paths, or `None` if all files
    /// are selected (meaning "no filter needed").
    pub fn selected_paths(&self) -> Option<Vec<String>> {
        if self.selected.len() == self.files.len() || self.selected.is_empty() {
            return None;
        }
        Some(
            self.files
                .iter()
                .enumerate()
                .filter(|(i, _)| self.selected.contains(i))
                .map(|(_, p)| p.clone())
                .collect(),
        )
    }

    /// Toggle selection of the current cursor position.
    pub fn toggle_current(&mut self) {
        if self.selected.contains(&self.cursor) {
            self.selected.remove(&self.cursor);
        } else {
            self.selected.insert(self.cursor);
        }
    }

    /// Select all files.
    pub fn select_all(&mut self) {
        self.selected = (0..self.files.len()).collect();
    }

    /// Deselect all files.
    pub fn select_none(&mut self) {
        self.selected.clear();
    }

    /// Move cursor up; auto-scroll if needed.
    pub fn navigate_up(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            let scroll = self.scroll.get();
            if self.cursor < scroll {
                self.scroll.set(scroll.saturating_sub(1));
            }
        }
    }

    /// Move cursor down; auto-scroll is handled during render.
    pub fn navigate_down(&mut self) {
        if self.cursor + 1 < self.files.len() {
            self.cursor += 1;
        }
    }
}

#[cfg(test)]
mod file_selection_tests {
    use super::*;

    fn make_fs() -> FileSelection {
        FileSelection::new("abc".into(), vec![
            "src/main.rs".into(),
            "src/lib.rs".into(),
            "Cargo.toml".into(),
        ])
    }

    #[test]
    fn new_selects_all_files() {
        let fs = make_fs();
        assert_eq!(fs.selected.len(), 3);
        assert_eq!(fs.cursor, 0);
    }

    #[test]
    fn selected_paths_returns_none_when_all_selected() {
        let fs = make_fs();
        assert!(fs.selected_paths().is_none());
    }

    #[test]
    fn selected_paths_returns_none_when_none_selected() {
        let mut fs = make_fs();
        fs.select_none();
        assert!(fs.selected_paths().is_none());
    }

    #[test]
    fn selected_paths_returns_filtered_when_partial() {
        let mut fs = make_fs();
        fs.select_none();
        fs.selected.insert(0);
        fs.selected.insert(2);
        let paths = fs.selected_paths().unwrap();
        assert_eq!(paths, vec!["src/main.rs", "Cargo.toml"]);
    }

    #[test]
    fn toggle_current_deselects_when_selected() {
        let mut fs = make_fs();
        assert!(fs.selected.contains(&0));
        fs.toggle_current();
        assert!(!fs.selected.contains(&0));
    }

    #[test]
    fn toggle_current_selects_when_not_selected() {
        let mut fs = make_fs();
        fs.select_none();
        fs.toggle_current();
        assert!(fs.selected.contains(&0));
    }

    #[test]
    fn select_all_selects_everything() {
        let mut fs = make_fs();
        fs.select_none();
        fs.select_all();
        assert_eq!(fs.selected.len(), 3);
    }

    #[test]
    fn select_none_clears_all() {
        let mut fs = make_fs();
        fs.select_none();
        assert!(fs.selected.is_empty());
    }

    #[test]
    fn navigate_up_moves_cursor() {
        let mut fs = make_fs();
        fs.cursor = 2;
        fs.navigate_up();
        assert_eq!(fs.cursor, 1);
        fs.navigate_up();
        assert_eq!(fs.cursor, 0);
        fs.navigate_up(); // at top — no-op
        assert_eq!(fs.cursor, 0);
    }

    #[test]
    fn navigate_down_moves_cursor() {
        let mut fs = make_fs();
        fs.navigate_down();
        assert_eq!(fs.cursor, 1);
        fs.navigate_down();
        assert_eq!(fs.cursor, 2);
        fs.navigate_down(); // at bottom — no-op
        assert_eq!(fs.cursor, 2);
    }

    #[test]
    fn navigate_up_auto_scrolls_when_cursor_passes_above_scroll() {
        let mut fs = make_fs();
        fs.cursor = 2;
        fs.scroll.set(2);
        // Move up: cursor becomes 1, which is < scroll(2) → scroll drops to 1
        fs.navigate_up();
        assert_eq!(fs.scroll.get(), 1);
        // Move up again: cursor becomes 0, < scroll(1) → scroll drops to 0
        fs.navigate_up();
        assert_eq!(fs.scroll.get(), 0);
        // At top — no-op
        fs.navigate_up();
        assert_eq!(fs.cursor, 0);
        assert_eq!(fs.scroll.get(), 0);
    }
}
