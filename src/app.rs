use std::cell::Cell;
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
    /// Transient footer notification (HEAD-move announcement).
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
}

impl App {
    /// Create a new app in the Loading state.
    pub fn new(config: Config) -> Self {
        // Best-effort branch detection — fall back to "?" on failure.
        let branch = git::open_repo()
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
            // Initialise to the past so the first poll always runs.
            last_poll_time: Instant::now()
                - Duration::from_millis(config.poll_interval_ms + 1),
        }
    }

    /// Load commits from the git repository.
    /// Transitions to `List` on success, `Error` on failure.
    pub fn load_commits(&mut self) {
        match git::load_commits() {
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
        let repo = match git::open_repo() {
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

        // 4. First poll after startup: just record HEAD, no change detection.
        if self.current_head_oid.is_none() {
            self.current_head_oid = Some(new_head_oid.clone());
            self.replace_commits(new_commits);
            return;
        }

        // 5. HEAD change detection
        let old_head = self.current_head_oid.clone();
        let head_changed = old_head.as_ref() != Some(&new_head_oid);

        if head_changed {
            // Build notification BEFORE moving new_head_oid.
            let old_short = old_head.as_ref().map(|o| &o[..7.min(o.len())]).unwrap_or("?");
            let new_short = new_head_oid[..7.min(new_head_oid.len())].to_string();
            self.current_head_oid = Some(new_head_oid);
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
                        self.screen =
                            Screen::Alert(AlertKind::CommitDeleted { oid: oid.clone() });
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
        let target_oid = self
            .commits
            .get(self.selected_index)
            .map(|c| c.oid.clone());
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

        match git::load_diff(&oid) {
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

    /// Set the quit flag.
    pub fn quit(&mut self) {
        self.should_quit = true;
    }

    /// Dismiss the error screen (equivalent to quitting).
    pub fn error_dismiss(&mut self) {
        if matches!(self.screen, Screen::Error(_)) {
            self.quit();
        }
    }
}
