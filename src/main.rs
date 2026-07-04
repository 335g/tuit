use std::io;

use anyhow::Result;
use crossterm::ExecutableCommand;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::{Backend, CrosstermBackend};

use crate::app::{App, Screen};

/// Simplified key event for the application.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Key {
    Up,
    Down,
    Enter,
    Esc,
    PageUp,
    PageDown,
    Char(char),
    Ctrl(char),
}

mod app;
mod config;
mod git;
mod ui;

/// Main entry point – production mode.
fn main() -> Result<()> {
    enable_raw_mode()?;
    io::stdout().execute(EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    let res = run_app(
        &mut terminal,
        std::iter::from_fn(|| read_key().ok()).flatten(),
    );

    disable_raw_mode()?;
    io::stdout().execute(LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    if let Err(e) = res {
        eprintln!("Error: {e}");
    }

    Ok(())
}

/// Read a single key event from crossterm.
fn read_key() -> Result<Option<Key>> {
    if let Event::Key(key) = event::read()? {
        if key.kind == KeyEventKind::Press {
            let k = match key.code {
                KeyCode::Esc => Key::Esc,
                KeyCode::Enter => Key::Enter,
                KeyCode::Up => Key::Up,
                KeyCode::Down => Key::Down,
                KeyCode::PageUp => Key::PageUp,
                KeyCode::PageDown => Key::PageDown,
                KeyCode::Char(c) if key.modifiers.contains(KeyModifiers::CONTROL) => Key::Ctrl(c),
                KeyCode::Char(c) => Key::Char(c),
                _ => return Ok(None),
            };
            return Ok(Some(k));
        }
    }
    Ok(None)
}

/// Generic application entry point that accepts any Backend and any key-event
/// iterator.  Used by both `main()` (production) and tests (TestBackend).
pub fn run_app<B: Backend>(
    terminal: &mut Terminal<B>,
    events: impl Iterator<Item = Key>,
) -> Result<()>
where
    <B as Backend>::Error: Send + Sync + 'static,
{
    // 1. Load config (falls back to default colours if no files found).
    let cfg = config::load().unwrap_or_else(|_| config::Config {
        theme: "default".into(),
        colors: config::default_colors(),
    });
    let config = cfg;

    // 2. Initialise app and load commits.
    let mut app = App::new(config);
    app.load_commits();

    // Draw initial state before waiting for input.
    terminal.draw(|frame| {
        ui::render(frame, &app);
    })?;

    // 3. Event loop.
    for key in events {
        handle_input(&mut app, key);

        // Draw current state.
        terminal.draw(|frame| {
            ui::render(frame, &app);
        })?;

        if app.should_quit {
            break;
        }
    }

    // Final draw
    let _ = terminal.draw(|frame| {
        ui::render(frame, &app);
    });

    Ok(())
}

/// Process a single key-press against the current app state.
pub fn handle_input(app: &mut App, key: Key) {
    match &app.screen {
        Screen::List => match key {
            Key::Up => app.navigate_up(),
            Key::Down => app.navigate_down(),
            Key::Enter => app.select_commit(),
            Key::Char('q') => app.quit(),
            _ => {}
        },
        Screen::Detail => match key {
            Key::Esc => app.close_detail(),
            Key::Up | Key::Char('k') => app.scroll_detail_up(),
            Key::Down | Key::Char('j') => app.scroll_detail_down(),
            Key::Ctrl('f') | Key::PageDown => app.scroll_detail_page_down(),
            Key::Ctrl('b') | Key::PageUp => app.scroll_detail_page_up(),
            _ => {}
        },
        Screen::Error(_) => match key {
            Key::Enter => app.quit(),
            _ => {}
        },
        Screen::Loading => {
            // Ignore all input while loading.
        }
    }
}

// ── End-to-end tests ──────────────────────────────────────────────────

#[cfg(test)]
mod e2e_tests {
    use std::path::Path;
    use std::process::Command;

    use ratatui::backend::TestBackend;

    use super::*;

    /// Helper: create a temporary git repository at `path` with `n` commits.
    /// Each commit adds a unique file (commit-0, commit-1, …) with a known subject line.
    fn init_repo(path: &Path, n: usize) {
        let _ = std::fs::remove_dir_all(path);
        std::fs::create_dir_all(path).unwrap();

        Command::new("git")
            .args(["init", "--initial-branch=main"])
            .arg(path)
            .status()
            .unwrap();

        for i in 0..n {
            let file = path.join(format!("file-{i}.txt"));
            std::fs::write(&file, format!("content {i}")).unwrap();
            Command::new("git")
                .args([
                    "-C",
                    &path.to_string_lossy(),
                    "add",
                    &file.to_string_lossy(),
                ])
                .status()
                .unwrap();
            Command::new("git")
                .args([
                    "-C",
                    &path.to_string_lossy(),
                    "commit",
                    "-m",
                    &format!("Commit subject {i}"),
                    "--allow-empty",
                ])
                .env("GIT_AUTHOR_NAME", "Test User")
                .env("GIT_AUTHOR_EMAIL", "test@example.com")
                .env("GIT_COMMITTER_NAME", "Test User")
                .env("GIT_COMMITTER_EMAIL", "test@example.com")
                .status()
                .unwrap();
        }
    }

    /// Create an empty git repo (no commits) at `path`.
    fn init_empty_repo(path: &Path) {
        let _ = std::fs::remove_dir_all(path);
        std::fs::create_dir_all(path).unwrap();
        Command::new("git")
            .args(["init", "--initial-branch=main"])
            .arg(path)
            .status()
            .unwrap();
    }

    /// Run ratty against a git repo at `repo_path` with the given key events.
    fn run_with_events(repo_path: &Path, events: Vec<Key>) -> TestBackend {
        // Change to the repo directory before testing.
        let prev_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(repo_path).unwrap();

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        // Override XDG_CONFIG_HOME so the test doesn't read the user's real config.
        let config_home = repo_path.join(".ratty-config");
        std::fs::create_dir_all(&config_home).unwrap();
        // We can't easily override this in-process, but config::load() will
        // fall back to defaults when no files exist, which is fine.

        let _ = super::run_app(&mut terminal, events.into_iter());

        // Restore working directory.
        std::env::set_current_dir(prev_dir).unwrap();

        terminal.backend().clone()
    }

    #[test]
    fn happy_path_commit_list_shows_all_commits() {
        let tmp = std::env::temp_dir().join("ratty-e2e-happy");
        init_repo(&tmp, 3);

        let backend = run_with_events(&tmp, vec![Key::Char('q')]);
        let buf = backend.buffer();

        // The buffer should contain each commit's abbreviated hash and subject.
        let content = buf_to_string(buf);
        assert!(
            content.contains("Commit subject 0"),
            "Expected 'Commit subject 0' in buffer, got:\n{content}",
        );
        assert!(
            content.contains("Commit subject 1"),
            "Expected 'Commit subject 1' in buffer, got:\n{content}",
        );
        assert!(
            content.contains("Commit subject 2"),
            "Expected 'Commit subject 2' in buffer, got:\n{content}",
        );
        // Test User should appear for each commit.
        assert!(
            content.contains("Test User"),
            "Expected 'Test User' in buffer, got:\n{content}",
        );
    }

    #[test]
    fn non_git_directory_shows_error() {
        let tmp = std::env::temp_dir().join("ratty-e2e-non-git");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let backend = run_with_events(&tmp, vec![Key::Enter]);
        let buf = backend.buffer();
        let content = buf_to_string(buf);

        // The error message may have spacing artifacts in buffer concatenation.
        let stripped: String = content.chars().filter(|c| !c.is_whitespace()).collect();
        assert!(
            stripped.contains("rattyはgitリポジトリの中で実行してください"),
            "Expected error message, got:\n{content}",
        );
    }

    #[test]
    fn empty_repository_shows_empty_message() {
        let tmp = std::env::temp_dir().join("ratty-e2e-empty");
        init_empty_repo(&tmp);

        let backend = run_with_events(&tmp, vec![Key::Enter]);
        let buf = backend.buffer();
        let content = buf_to_string(buf);

        let stripped: String = content.chars().filter(|c| !c.is_whitespace()).collect();
        assert!(
            stripped.contains("このリポジトリにはまだコミットがありません"),
            "Expected empty repo message, got:\n{content}",
        );
    }

    /// Convert a TestBackend buffer cells to a plain string for easy assertion.
    /// Skips cells that are hidden (empty symbols) and collapses runs of spaces.
    fn buf_to_string(buf: &ratatui::buffer::Buffer) -> String {
        let mut s = String::new();
        let area = buf.area;
        for y in 0..area.height {
            let mut prev_was_space = false;
            for x in 0..area.width {
                let cell = buf.cell((x, y)).unwrap();
                let sym = cell.symbol();
                if sym.is_empty() {
                    continue;
                }
                if sym == " " {
                    if prev_was_space {
                        continue;
                    }
                    prev_was_space = true;
                } else {
                    prev_was_space = false;
                }
                s.push_str(sym);
            }
            if y + 1 < area.height {
                s.push('\n');
            }
        }
        s
    }

    #[test]
    fn detail_overlay_hides_commit_list_text() {
        let tmp = std::env::temp_dir().join("ratty-e2e-detail-overlay");
        init_repo(&tmp, 5);
        let backend = run_with_events(&tmp, vec![Key::Enter]);
        let content = buf_to_string(backend.buffer());

        // When detail is open for the first (most recent) commit, only its subject
        // should be visible; the other list rows must not leak through the popup.
        assert!(
            content.contains("Commit subject 4"),
            "Expected selected commit subject in buffer, got:\n{content}",
        );
        for i in 0..4 {
            assert!(
                !content.contains(&format!("Commit subject {i}")),
                "Commit subject {i} leaked through popup:\n{content}",
            );
        }
    }
}
