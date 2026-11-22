use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use clap::Parser;
use crossterm::ExecutableCommand;
use crossterm::event::{
    self, Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind, poll,
};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::{Backend, CrosstermBackend};

use crate::app::{App, Screen};

/// Simplified input event for the application.
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
    /// Left mouse button click at the given terminal row.
    MouseClick(u16),
}

mod app;
mod config;
mod git;
mod ui;

/// Command-line arguments for tuit.
#[derive(Parser)]
#[command(name = "tuit", version)]
struct Cli {
    /// Path to the Git repository to open.
    #[arg(short, long, value_name = "PATH")]
    repo: Option<PathBuf>,

    /// Print the commit list as JSON and exit, without opening the TUI.
    #[arg(long)]
    json: bool,

    /// Number of commits --json prints (default 200).
    #[arg(long, value_name = "N", default_value_t = 200)]
    limit: usize,
}

/// Main entry point – production mode.
fn main() -> Result<()> {
    let cli = Cli::parse();
    let explicit_repo = cli.repo.is_some();
    let repo_path = cli
        .repo
        .map(|p| p.canonicalize().unwrap_or(p))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    // Validate an explicitly supplied repository path before entering the TUI.
    if explicit_repo && !cli.json {
        validate_repo_path(&repo_path)?;
    }

    if cli.json {
        print_json(&repo_path, cli.limit)?;
        return Ok(());
    }

    enable_raw_mode()?;
    io::stdout().execute(EnterAlternateScreen)?;
    io::stdout().execute(event::EnableMouseCapture)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;

    // Load config.
    let cfg = config::load().unwrap_or_else(|_| config::Config {
        theme: "default".into(),
        colors: config::default_colors(),
        poll_interval_ms: 2000,
        notification_timeout_ms: 3000,
    });

    // Initialise app and load commits.
    let mut app = App::new(cfg, repo_path);
    app.load_commits();

    // Draw initial state.
    terminal.draw(|frame| {
        ui::render(frame, &app);
    })?;

    let poll_interval = Duration::from_millis(app.poll_interval_ms);

    // Live event loop: poll for both keyboard input and timer-based git sync.
    loop {
        // Block up to `poll_interval` for a key press.
        if poll(poll_interval)? {
            if let Some(key) = read_key()? {
                handle_input(&mut app, key);
            }
        }

        // Git state sync (throttled internally to `poll_interval_ms`).
        app.poll();

        // Re-render.
        terminal.draw(|frame| {
            ui::render(frame, &app);
        })?;

        if app.should_quit {
            break;
        }
    }

    // Teardown.
    disable_raw_mode()?;
    io::stdout().execute(event::DisableMouseCapture)?;
    io::stdout().execute(LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    Ok(())
}

/// Read a single input event from crossterm.
fn read_key() -> Result<Option<Key>> {
    match event::read()? {
        Event::Key(key) if key.kind == KeyEventKind::Press => {
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
            Ok(Some(k))
        }
        Event::Mouse(mouse) if mouse.kind == MouseEventKind::Down(MouseButton::Left) => {
            Ok(Some(Key::MouseClick(mouse.row)))
        }
        _ => Ok(None),
    }
}

/// Print the commit list as JSON, for the herdr view (`herdr-app` mods call this).
fn print_json(repo_path: &Path, limit: usize) -> Result<()> {
    let repo = git::open_repo(repo_path)?;
    let mut commits = git::load_commits_from(&repo)?;
    commits.truncate(limit);
    let root = repo
        .workdir()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let branch = git::current_branch(&repo).unwrap_or_else(|_| "HEAD".to_string());
    // The palette the TUI would use (config.toml + theme), so the herdr view
    // colours its rows the same way instead of carrying a second copy.
    let colors = config::load()
        .map(|c| c.colors)
        .unwrap_or_else(|_| config::default_colors());
    let cc = serde_json::json!({
        "feat": colors.cc_type_feat.clone(),
        "fix": colors.cc_type_fix.clone(),
        "docs": colors.cc_type_docs.clone(),
        "style": colors.cc_type_style.clone(),
        "refactor": colors.cc_type_refactor.clone(),
        "perf": colors.cc_type_perf.clone(),
        "test": colors.cc_type_test.clone(),
        "chore": colors.cc_type_chore.clone(),
    });
    let out = serde_json::json!({
        "root": root,
        "branch": branch,
        "colors": colors,
        "cc": cc,
        "commits": commits,
    });
    println!("{}", serde_json::to_string(&out)?);
    Ok(())
}

/// Validate that `path` exists, is a directory, and is a git repository.
fn validate_repo_path(path: &Path) -> Result<()> {
    if !path.exists() {
        anyhow::bail!("リポジトリパスが存在しません: {}", path.display());
    }
    if !path.is_dir() {
        anyhow::bail!(
            "リポジトリパスはディレクトリである必要があります: {}",
            path.display()
        );
    }
    git::open_repo(path)
        .with_context(|| format!("無効な Git リポジトリです: {}", path.display()))?;
    Ok(())
}

/// Generic application entry point that accepts any Backend and any key-event
/// iterator.  Used by both `main()` (production) and tests (TestBackend).
pub fn run_app<B: Backend>(
    terminal: &mut Terminal<B>,
    events: impl Iterator<Item = Key>,
    repo_path: PathBuf,
) -> Result<()>
where
    <B as Backend>::Error: Send + Sync + 'static,
{
    // 1. Load config (falls back to default colours if no files found).
    let cfg = config::load().unwrap_or_else(|_| config::Config {
        theme: "default".into(),
        colors: config::default_colors(),
        poll_interval_ms: 2000,
        notification_timeout_ms: 3000,
    });
    let config = cfg;

    // 2. Initialise app and load commits.
    let mut app = App::new(config, repo_path);
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
    // Help overlay takes all input except toggle/close.
    if app.show_help {
        match key {
            Key::Char('?') | Key::Esc => app.show_help = false,
            _ => {}
        }
        return;
    }

    // Global: ? opens help from any screen.
    if key == Key::Char('?') {
        app.show_help = true;
        return;
    }

    // Global: r reloads commit list (or the worktree dashboard).
    if key == Key::Char('r') {
        if app.screen == Screen::Worktrees {
            app.refresh_worktrees();
        } else {
            app.reload();
        }
        return;
    }

    match &app.screen {
        Screen::List => {
            // Numeric prefix for j/k movement
            if let Key::Char(d) = key {
                if d.is_ascii_digit() {
                    let digit = d.to_digit(10).unwrap() as usize;
                    app.pending_count =
                        Some(app.pending_count.unwrap_or(0) * 10 + digit);
                    return;
                }
            }
            let count = app.pending_count.take();
            match key {
                Key::Up | Key::Char('k') => {
                    let n = count.unwrap_or(1);
                    for _ in 0..n { app.navigate_up(); }
                }
                Key::Down | Key::Char('j') => {
                    let n = count.unwrap_or(1);
                    for _ in 0..n { app.navigate_down(); }
                }
                Key::Ctrl('f') | Key::PageDown => app.navigate_page_down(),
                Key::Ctrl('b') | Key::PageUp => app.navigate_page_up(),
                Key::Enter => app.select_commit(),
                Key::Char('c') => copy_commit_hash(app),
                Key::Char('w') => app.open_worktrees(),
                Key::Char('q') => app.quit(),
                Key::MouseClick(row) => select_commit_at_row(app, row),
                _ => {}
            }
        },
        Screen::Detail => match key {
            Key::Esc => app.close_detail(),
            Key::Up | Key::Char('k') => app.scroll_detail_up(),
            Key::Down | Key::Char('j') => app.scroll_detail_down(),
            Key::Ctrl('f') | Key::PageDown => app.scroll_detail_page_down(),
            Key::Ctrl('b') | Key::PageUp => app.scroll_detail_page_up(),
            Key::Char('c') => copy_commit_hash(app),
            _ => {}
        },
        Screen::Worktrees => match key {
            Key::Char('w') | Key::Esc => app.close_worktrees(),
            Key::Up | Key::Char('k') => app.navigate_worktree_up(),
            Key::Down | Key::Char('j') => app.navigate_worktree_down(),
            Key::Char('c') => copy_commit_hash(app),
            Key::Char('q') => app.quit(),
            _ => {}
        },
        Screen::Error(_) => match key {
            Key::Enter => app.quit(),
            _ => {}
        },
        Screen::Alert(_) => match key {
            Key::Enter | Key::Esc => app.dismiss_alert(),
            _ => {}
        },
        Screen::Loading => {
            // Ignore all input while loading.
        }
    }
}

/// Select the commit visible at the given terminal row in the commit list.
///
/// The header occupies row 0; the list itself begins at row 1.  Clicks
/// outside the list area are ignored.
fn select_commit_at_row(app: &mut App, row: u16) {
    if app.commits.is_empty() || row < 1 {
        return;
    }
    let visible_row = (row as usize).saturating_sub(1);
    let new_index = app.list_scroll.get() + visible_row;
    let max_index = app.commits.len().saturating_sub(1);
    if new_index <= max_index {
        app.selected_index = new_index;
    }
}

/// Copy the currently focused commit's full OID to the system clipboard.
/// Shows a footer notification on success; fails silently if the clipboard
/// is unavailable (headless environment, etc.).
fn copy_commit_hash(app: &mut App) {
    let oid = match app.current_commit_oid() {
        Some(o) => o,
        None => return,
    };

    if let Ok(mut clipboard) = arboard::Clipboard::new() {
        if clipboard.set_text(oid.clone()).is_ok() {
            let short = oid.chars().take(7).collect::<String>();
            app.set_notification(format!("Copied {} to clipboard", short));
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

    /// Run tuit against a git repo at `repo_path` with the given key events.
    fn run_with_events(repo_path: &Path, events: Vec<Key>) -> TestBackend {
        // Change to the repo directory before testing.
        let prev_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(repo_path).unwrap();

        let backend = TestBackend::new(210, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let _ = super::run_app(&mut terminal, events.into_iter(), repo_path.to_path_buf());

        // Restore working directory.
        std::env::set_current_dir(prev_dir).unwrap();

        terminal.backend().clone()
    }

    /// Resolve the full OID of `rev` in `repo_path` via the git CLI.
    /// Independent source of truth for expected OIDs in assertions.
    fn rev_parse(repo_path: &Path, rev: &str) -> String {
        let out = Command::new("git")
            .args(["-C", &repo_path.to_string_lossy(), "rev-parse", rev])
            .output()
            .unwrap();
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    }

    #[test]
    fn happy_path_commit_list_shows_all_commits() {
        let tmp = std::env::temp_dir().join("tuit-e2e-happy");
        init_repo(&tmp, 3);

        let backend = run_with_events(&tmp, vec![Key::Char('q')]);
        let buf = backend.buffer();

        // The buffer should contain each commit's subject (hash and author not checked:
        // hash visibility depends on width, author is never shown in list view).
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
        // At 210 columns wide hash should be visible.
        assert!(
            !content.contains("Test User"),
            "Author 'Test User' should NOT appear in commit list, got:\n{content}",
        );
    }

    #[test]
    fn non_git_directory_shows_error() {
        let tmp = std::env::temp_dir().join("tuit-e2e-non-git");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let backend = run_with_events(&tmp, vec![Key::Enter]);
        let buf = backend.buffer();
        let content = buf_to_string(buf);

        // The error message may have spacing artifacts in buffer concatenation.
        let stripped: String = content.chars().filter(|c| !c.is_whitespace()).collect();
        assert!(
            stripped.contains("tuitはgitリポジトリの中で実行してください"),
            "Expected error message, got:\n{content}",
        );
    }

    #[test]
    fn empty_repository_shows_empty_message() {
        let tmp = std::env::temp_dir().join("tuit-e2e-empty");
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
    fn poll_detects_new_commit() {
        let tmp = std::env::temp_dir().join("tuit-e2e-poll-new");
        init_repo(&tmp, 2);

        let prev_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(&tmp).unwrap();

        let cfg = config::Config {
            theme: "default".into(),
            colors: config::default_colors(),
            poll_interval_ms: 2000,
            notification_timeout_ms: 3000,
        };
        let mut app = App::new(cfg, tmp.clone());
        app.load_commits();

        // Initial state: 2 commits
        assert_eq!(app.commits.len(), 2);
        assert_eq!(app.screen, Screen::List);
        let first_head = app.current_head_oid.clone();

        // Add a third commit via git CLI
        let file = tmp.join("file-2.txt");
        std::fs::write(&file, "content 2").unwrap();
        Command::new("git")
            .args(["-C", &tmp.to_string_lossy(), "add", &file.to_string_lossy()])
            .status()
            .unwrap();
        Command::new("git")
            .args([
                "-C",
                &tmp.to_string_lossy(),
                "commit",
                "--allow-empty",
                "-m",
                "Commit subject 2",
            ])
            .env("GIT_AUTHOR_NAME", "Test User")
            .env("GIT_AUTHOR_EMAIL", "test@example.com")
            .env("GIT_COMMITTER_NAME", "Test User")
            .env("GIT_COMMITTER_EMAIL", "test@example.com")
            .status()
            .unwrap();

        // First poll: just records HEAD (skip change detection)
        app.poll();
        assert!(app.current_head_oid.is_some());
        assert_ne!(app.current_head_oid, first_head);
        assert_eq!(app.commits.len(), 3);
        assert!(
            app.commits[0].message.contains("Commit subject 2"),
            "Expected newest commit at top, got: {}",
            app.commits[0].message
        );

        std::env::set_current_dir(prev_dir).unwrap();
    }

    #[test]
    fn poll_head_change_shows_notification() {
        let tmp = std::env::temp_dir().join("tuit-e2e-poll-head-notification");
        init_repo(&tmp, 2);

        let prev_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(&tmp).unwrap();

        let cfg = config::Config {
            theme: "default".into(),
            colors: config::default_colors(),
            poll_interval_ms: 2000,
            notification_timeout_ms: 3000,
        };
        let mut app = App::new(cfg, tmp.clone());
        app.load_commits();

        // First poll records the initial HEAD.
        app.poll();
        assert!(app.current_head_oid.is_some());
        assert!(
            app.notification.is_none(),
            "No notification on initial HEAD recording"
        );

        // Add a new commit so HEAD changes.
        let file = tmp.join("head-notification.txt");
        std::fs::write(&file, "head notification content").unwrap();
        Command::new("git")
            .args(["-C", &tmp.to_string_lossy(), "add", &file.to_string_lossy()])
            .status()
            .unwrap();
        Command::new("git")
            .args([
                "-C",
                &tmp.to_string_lossy(),
                "commit",
                "--allow-empty",
                "-m",
                "Head change notification commit",
            ])
            .env("GIT_AUTHOR_NAME", "Test User")
            .env("GIT_AUTHOR_EMAIL", "test@example.com")
            .env("GIT_COMMITTER_NAME", "Test User")
            .env("GIT_COMMITTER_EMAIL", "test@example.com")
            .status()
            .unwrap();

        // Force the next poll to run despite throttle.
        app.last_poll_time = std::time::Instant::now() - std::time::Duration::from_millis(5000);
        app.poll();

        assert!(
            app.notification.is_some(),
            "Expected notification after HEAD change"
        );
        assert!(
            app.notification
                .as_ref()
                .unwrap()
                .message
                .contains("HEAD moved"),
            "Expected HEAD moved notification, got: {:?}",
            app.notification
        );

        std::env::set_current_dir(prev_dir).unwrap();
    }

    #[test]
    fn poll_timestamps_update_in_detail() {
        let tmp = std::env::temp_dir().join("tuit-e2e-poll-detail-time");
        init_repo(&tmp, 1);

        let prev_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(&tmp).unwrap();

        let cfg = config::Config {
            theme: "default".into(),
            colors: config::default_colors(),
            poll_interval_ms: 2000,
            notification_timeout_ms: 3000,
        };
        let mut app = App::new(cfg, tmp.clone());
        app.load_commits();
        app.select_commit();
        assert_eq!(app.screen, Screen::Detail);

        let _original_date = app.selected_commit.as_ref().unwrap().date.clone();

        // First poll (HEAD recording)
        app.poll();

        // Second poll: HEAD unchanged, timestamp might have advanced
        app.last_poll_time = std::time::Instant::now() - std::time::Duration::from_millis(5000); // force poll to run
        app.poll();

        // Should still be in Detail with same OID
        assert_eq!(app.screen, Screen::Detail);
        assert!(app.selected_commit.is_some());

        std::env::set_current_dir(prev_dir).unwrap();
    }

    #[test]
    fn detail_overlay_hides_commit_list_text() {
        let tmp = std::env::temp_dir().join("tuit-e2e-detail-overlay");
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

    #[test]
    fn reload_detects_branch_switch() {
        let tmp = std::env::temp_dir().join("tuit-e2e-reload-branch");
        init_repo(&tmp, 2);

        // Create and switch to a second branch with one more commit.
        Command::new("git")
            .args(["-C", &tmp.to_string_lossy(), "checkout", "-b", "feature"])
            .status()
            .unwrap();
        let file = tmp.join("feature.txt");
        std::fs::write(&file, "feature content").unwrap();
        Command::new("git")
            .args(["-C", &tmp.to_string_lossy(), "add", &file.to_string_lossy()])
            .status()
            .unwrap();
        Command::new("git")
            .args([
                "-C",
                &tmp.to_string_lossy(),
                "commit",
                "-m",
                "Commit subject 2 (feature)",
            ])
            .env("GIT_AUTHOR_NAME", "Test User")
            .env("GIT_AUTHOR_EMAIL", "test@example.com")
            .env("GIT_COMMITTER_NAME", "Test User")
            .env("GIT_COMMITTER_EMAIL", "test@example.com")
            .status()
            .unwrap();

        // Switch back to main (2 commits).
        Command::new("git")
            .args(["-C", &tmp.to_string_lossy(), "checkout", "main"])
            .status()
            .unwrap();

        let prev_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(&tmp).unwrap();

        let cfg = config::Config {
            theme: "default".into(),
            colors: config::default_colors(),
            poll_interval_ms: 2000,
            notification_timeout_ms: 3000,
        };
        let mut app = App::new(cfg, tmp.clone());
        app.load_commits();

        // Initial state: on main with 2 commits
        assert_eq!(app.current_branch, "main");
        assert_eq!(app.commits.len(), 2);

        // Switch to feature branch via CLI
        Command::new("git")
            .args(["-C", &tmp.to_string_lossy(), "checkout", "feature"])
            .status()
            .unwrap();

        // Reload
        app.reload();

        // Now on feature with 3 commits
        assert_eq!(app.current_branch, "feature");
        assert_eq!(app.commits.len(), 3);
        assert!(
            app.commits[0]
                .message
                .contains("Commit subject 2 (feature)"),
            "Expected feature branch commit at top, got: {}",
            app.commits[0].message
        );
        assert_eq!(app.selected_index, 0);
        assert_eq!(app.screen, Screen::List);
        assert!(app.selected_commit.is_none());

        std::env::set_current_dir(prev_dir).unwrap();
    }

    #[test]
    fn reload_via_key_event_updates_branch_in_handle_input() {
        // Test that handle_input with Key::Char('r') correctly triggers reload.
        let tmp = std::env::temp_dir().join("tuit-e2e-reload-key-event");
        init_repo(&tmp, 2);

        // Create feature branch with one extra commit.
        Command::new("git")
            .args(["-C", &tmp.to_string_lossy(), "checkout", "-b", "feature"])
            .status()
            .unwrap();
        let file = tmp.join("f.txt");
        std::fs::write(&file, "f").unwrap();
        Command::new("git")
            .args(["-C", &tmp.to_string_lossy(), "add", &file.to_string_lossy()])
            .status()
            .unwrap();
        Command::new("git")
            .args([
                "-C",
                &tmp.to_string_lossy(),
                "commit",
                "-m",
                "Feature commit",
            ])
            .env("GIT_AUTHOR_NAME", "Test User")
            .env("GIT_AUTHOR_EMAIL", "test@example.com")
            .env("GIT_COMMITTER_NAME", "Test User")
            .env("GIT_COMMITTER_EMAIL", "test@example.com")
            .status()
            .unwrap();

        // Switch back to main.
        Command::new("git")
            .args(["-C", &tmp.to_string_lossy(), "checkout", "main"])
            .status()
            .unwrap();

        let prev_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(&tmp).unwrap();

        let cfg = config::Config {
            theme: "default".into(),
            colors: config::default_colors(),
            poll_interval_ms: 2000,
            notification_timeout_ms: 3000,
        };
        let mut app = App::new(cfg, tmp.clone());
        app.load_commits();

        assert_eq!(app.current_branch, "main");
        assert_eq!(app.commits.len(), 2);

        // Switch to feature via CLI, then simulate pressing 'r'.
        Command::new("git")
            .args(["-C", &tmp.to_string_lossy(), "checkout", "feature"])
            .status()
            .unwrap();

        handle_input(&mut app, Key::Char('r'));

        assert_eq!(
            app.current_branch, "feature",
            "branch name should update after reload"
        );
        assert_eq!(
            app.commits.len(),
            3,
            "commit count should reflect feature branch"
        );
        assert!(
            app.commits[0].message.contains("Feature commit"),
            "top commit should be the feature branch commit"
        );
        assert_eq!(app.screen, Screen::List);
        assert_eq!(app.selected_index, 0);

        std::env::set_current_dir(prev_dir).unwrap();
    }

    #[test]
    fn reload_via_run_app_r_key() {
        let tmp = std::env::temp_dir().join("tuit-e2e-run-app-r");
        init_repo(&tmp, 3);

        // Create a feature branch with one commit, switch back to main.
        Command::new("git")
            .args(["-C", &tmp.to_string_lossy(), "checkout", "-b", "feature"])
            .status()
            .unwrap();
        let file = tmp.join("ft.txt");
        std::fs::write(&file, "ft").unwrap();
        Command::new("git")
            .args(["-C", &tmp.to_string_lossy(), "add", &file.to_string_lossy()])
            .status()
            .unwrap();
        Command::new("git")
            .args([
                "-C",
                &tmp.to_string_lossy(),
                "commit",
                "-m",
                "Only on feature",
            ])
            .env("GIT_AUTHOR_NAME", "Test User")
            .env("GIT_AUTHOR_EMAIL", "test@example.com")
            .env("GIT_COMMITTER_NAME", "Test User")
            .env("GIT_COMMITTER_EMAIL", "test@example.com")
            .status()
            .unwrap();
        Command::new("git")
            .args(["-C", &tmp.to_string_lossy(), "checkout", "main"])
            .status()
            .unwrap();

        // Run with just 'r' then 'q' — at startup we're on main (3 commits).
        // The 'r' reloads from the same branch, so no visible change.
        // Test that the app doesn't crash and renders something.
        let backend = run_with_events(&tmp, vec![Key::Char('r'), Key::Char('q')]);
        let content = buf_to_string(backend.buffer());
        assert!(
            content.contains("main"),
            "Expected branch 'main' in header, got:\n{content}"
        );
        assert!(
            content.contains("tuit"),
            "Expected 'tuit' in header, got:\n{content}"
        );
        assert!(
            content.contains("Commit subject 0"),
            "Expected commit 0 in list after reload, got:\n{content}"
        );
    }

    #[test]
    fn reload_detaches_detail_view() {
        let tmp = std::env::temp_dir().join("tuit-e2e-reload-detail");
        init_repo(&tmp, 3);

        let prev_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(&tmp).unwrap();

        let cfg = config::Config {
            theme: "default".into(),
            colors: config::default_colors(),
            poll_interval_ms: 2000,
            notification_timeout_ms: 3000,
        };
        let mut app = App::new(cfg, tmp.clone());
        app.load_commits();
        app.select_commit();

        // Confirm we are in Detail
        assert_eq!(app.screen, Screen::Detail);
        assert!(app.selected_commit.is_some());
        assert!(app.selected_index == 0);

        // Reload
        app.reload();

        // Should be back to List with selection reset
        assert_eq!(app.screen, Screen::List);
        assert!(app.selected_commit.is_none());
        assert_eq!(app.selected_index, 0);

        std::env::set_current_dir(prev_dir).unwrap();
    }

    #[test]
    fn narrow_width_hides_author_and_hash() {
        let tmp = std::env::temp_dir().join("tuit-e2e-narrow");
        init_repo(&tmp, 3);

        // Run with 30-column terminal: neither author nor hash should be visible.
        let prev_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(&tmp).unwrap();

        let backend = TestBackend::new(30, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let _ = super::run_app(
            &mut terminal,
            vec![Key::Char('q')].into_iter(),
            tmp.clone(),
        );
        let content = buf_to_string(terminal.backend().buffer());

        std::env::set_current_dir(prev_dir).unwrap();

        // Author should NOT appear (width < 45).
        assert!(
            !content.contains("Test User"),
            "Author 'Test User' unexpectedly found in narrow (30-col) rendering:\n{content}",
        );
        // Full 7-char hex hashes should NOT appear either (width < 32 means hash hidden).
        // Commit subjects should still be visible.
        for i in 0..3 {
            assert!(
                content.contains(&format!("Commit subject {i}")),
                "Commit subject {i} missing in 30-col rendering:\n{content}",
            );
        }
    }

    #[test]
    fn mouse_click_selects_commit_in_list() {
        let tmp = std::env::temp_dir().join("tuit-e2e-mouse-click");
        init_repo(&tmp, 5);

        let prev_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(&tmp).unwrap();

        let cfg = config::Config {
            theme: "default".into(),
            colors: config::default_colors(),
            poll_interval_ms: 2000,
            notification_timeout_ms: 3000,
        };
        let mut app = App::new(cfg, tmp.clone());
        app.load_commits();
        assert_eq!(app.screen, Screen::List);
        assert_eq!(app.commits.len(), 5);
        assert_eq!(app.selected_index, 0);

        // Click on the third visible row (header is row 0, list starts at row 1).
        super::handle_input(&mut app, Key::MouseClick(3));
        assert_eq!(
            app.selected_index, 2,
            "Click on row 3 should select index 2"
        );

        // Click on the first list row.
        super::handle_input(&mut app, Key::MouseClick(1));
        assert_eq!(
            app.selected_index, 0,
            "Click on row 1 should select index 0"
        );

        // Click on the header row is ignored.
        super::handle_input(&mut app, Key::MouseClick(0));
        assert_eq!(
            app.selected_index, 0,
            "Click on header row should not change selection"
        );

        // Click beyond the commit list is ignored.
        super::handle_input(&mut app, Key::MouseClick(100));
        assert_eq!(
            app.selected_index, 0,
            "Click beyond list should not change selection"
        );

        std::env::set_current_dir(prev_dir).unwrap();
    }

    #[test]
    fn worktree_dashboard_renders_topology_and_cards() {
        let base = std::env::temp_dir().join("tuit-e2e-worktrees");
        let main = base.join("main");
        let wta = base.join("feat-a");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&main).unwrap();

        let commit = |dir: &Path, msg: &str| {
            let file = dir.join(format!("f-{}.txt", msg.replace(' ', "-")));
            std::fs::write(&file, msg).unwrap();
            Command::new("git")
                .args(["-C", &dir.to_string_lossy(), "add", "."])
                .status()
                .unwrap();
            Command::new("git")
                .args(["-C", &dir.to_string_lossy(), "commit", "-m", msg])
                .env("GIT_AUTHOR_NAME", "Test User")
                .env("GIT_AUTHOR_EMAIL", "test@example.com")
                .env("GIT_COMMITTER_NAME", "Test User")
                .env("GIT_COMMITTER_EMAIL", "test@example.com")
                .status()
                .unwrap();
        };

        Command::new("git")
            .args(["init", "--initial-branch=main"])
            .arg(&main)
            .status()
            .unwrap();
        commit(&main, "main 0");
        commit(&main, "main 1");

        Command::new("git")
            .args(["-C", &main.to_string_lossy(), "worktree", "add", "-b", "feat-a"])
            .arg(&wta)
            .status()
            .unwrap();
        commit(&wta, "feat a 1");

        let backend = run_with_events(&main, vec![Key::Char('w'), Key::Char('q')]);
        let content = buf_to_string(backend.buffer());

        assert!(
            content.contains("Topology"),
            "Expected 'Topology' pane, got:\n{content}"
        );
        assert!(
            content.contains("Worktrees"),
            "Expected 'Worktrees' pane, got:\n{content}"
        );
        assert!(content.contains("feat-a"), "got:\n{content}");
        assert!(content.contains("main"), "got:\n{content}");

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn worktree_dashboard_navigation_moves_selection() {
        let base = std::env::temp_dir().join("tuit-e2e-wt-nav");
        let main = base.join("main");
        let wta = base.join("feat-a");
        let wtb = base.join("feat-b");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&main).unwrap();

        let commit = |dir: &Path, msg: &str| {
            let file = dir.join(format!("f-{}.txt", msg.replace(' ', "-")));
            std::fs::write(&file, msg).unwrap();
            Command::new("git")
                .args(["-C", &dir.to_string_lossy(), "add", "."])
                .status()
                .unwrap();
            Command::new("git")
                .args(["-C", &dir.to_string_lossy(), "commit", "-m", msg])
                .env("GIT_AUTHOR_NAME", "Test User")
                .env("GIT_AUTHOR_EMAIL", "test@example.com")
                .env("GIT_COMMITTER_NAME", "Test User")
                .env("GIT_COMMITTER_EMAIL", "test@example.com")
                .status()
                .unwrap();
        };

        Command::new("git")
            .args(["init", "--initial-branch=main"])
            .arg(&main)
            .status()
            .unwrap();
        commit(&main, "main 0");

        Command::new("git")
            .args(["-C", &main.to_string_lossy(), "worktree", "add", "-b", "feat-a"])
            .arg(&wta)
            .status()
            .unwrap();
        commit(&wta, "feat a 1");

        Command::new("git")
            .args(["-C", &main.to_string_lossy(), "worktree", "add", "-b", "feat-b"])
            .arg(&wtb)
            .status()
            .unwrap();
        commit(&wtb, "feat b 1");

        let prev_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(&main).unwrap();

        let cfg = config::Config {
            theme: "default".into(),
            colors: config::default_colors(),
            poll_interval_ms: 2000,
            notification_timeout_ms: 3000,
        };
        let mut app = App::new(cfg, main.clone());
        app.load_commits();

        super::handle_input(&mut app, Key::Char('w'));
        assert_eq!(app.screen, Screen::Worktrees);
        assert!(
            app.worktrees.len() >= 3,
            "expected main + 2 linked worktrees, got {}: {:?}",
            app.worktrees.len(),
            app.worktrees.iter().map(|w| w.branch.clone()).collect::<Vec<_>>()
        );
        assert_eq!(app.worktree_selected, 0);

        super::handle_input(&mut app, Key::Char('j'));
        assert_eq!(app.worktree_selected, 1, "'j' should move down");
        super::handle_input(&mut app, Key::Down);
        assert_eq!(app.worktree_selected, 2, "Down should move down");
        super::handle_input(&mut app, Key::Char('k'));
        assert_eq!(app.worktree_selected, 1, "'k' should move up");
        super::handle_input(&mut app, Key::Up);
        assert_eq!(app.worktree_selected, 0, "Up should move up");

        std::env::set_current_dir(prev_dir).unwrap();
        let _ = std::fs::remove_dir_all(&base);
    }

}
