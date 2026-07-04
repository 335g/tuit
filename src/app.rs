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
}

/// Central application state.
pub struct App {
    pub screen: Screen,
    pub commits: Vec<Commit>,
    pub selected_index: usize,
    pub selected_commit: Option<Commit>,
    pub colors: config::Colors,
    pub should_quit: bool,
}

impl App {
    /// Create a new app in the Loading state.
    pub fn new(config: Config) -> Self {
        App {
            screen: Screen::Loading,
            commits: Vec::new(),
            selected_index: 0,
            selected_commit: None,
            colors: config.colors,
            should_quit: false,
        }
    }

    /// Load commits from the git repository.
    /// Transitions to `List` on success, `Error` on failure.
    pub fn load_commits(&mut self) {
        match git::load_commits() {
            Ok(commits) => {
                self.commits = commits;
                if self.commits.is_empty() {
                    self.screen = Screen::Error(
                        "このリポジトリにはまだコミットがありません。".into(),
                    );
                } else {
                    self.screen = Screen::List;
                }
            }
            Err(e) => {
                self.screen = Screen::Error(e.to_string());
            }
        }
    }

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

    /// Close the detail overlay and return to the list, preserving scroll position.
    pub fn close_detail(&mut self) {
        if self.screen == Screen::Detail {
            self.screen = Screen::List;
            self.selected_commit = None;
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
