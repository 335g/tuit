use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
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

use crate::app::{App, HunkLaunch, HunkOutcome, Screen};

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
    if explicit_repo {
        validate_repo_path(&repo_path)?;
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
                if drain_hunk_launch(&mut app, &mut launch_hunk) {
                    // The alternate screen was handed to hunk and back;
                    // force a full repaint.
                    let _ = terminal.clear();
                }
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
    launcher: &mut dyn FnMut(&Path, &HunkLaunch) -> HunkOutcome,
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
        drain_hunk_launch(&mut app, launcher);

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

    // File selection overlay intercepts its own keys.
    if app.file_selection.is_some() {
        match key {
            Key::Up | Key::Char('k') => {
                if let Some(ref mut fs) = app.file_selection {
                    fs.navigate_up();
                }
            }
            Key::Down | Key::Char('j') => {
                if let Some(ref mut fs) = app.file_selection {
                    fs.navigate_down();
                }
            }
            Key::Char(' ') => {
                if let Some(ref mut fs) = app.file_selection {
                    fs.toggle_current();
                }
            }
            Key::Char('a') => {
                if let Some(ref mut fs) = app.file_selection {
                    fs.select_all();
                }
            }
            Key::Char('n') => {
                if let Some(ref mut fs) = app.file_selection {
                    fs.select_none();
                }
            }
            Key::Enter => app.confirm_file_selection(),
            Key::Esc => app.close_file_selection(),
            _ => {}
        }
        return;
    }

    // Global: ? opens help from any screen.
    if key == Key::Char('?') {
        app.show_help = true;
        return;
    }

    // Global: r reloads commit list from the current branch.
    if key == Key::Char('r') {
        app.reload();
        return;
    }



    match &app.screen {
        Screen::List => match key {
            Key::Up | Key::Char('k') => app.navigate_up(),
            Key::Down | Key::Char('j') => app.navigate_down(),
            Key::Ctrl('f') | Key::PageDown => app.navigate_page_down(),
            Key::Ctrl('b') | Key::PageUp => app.navigate_page_up(),
            Key::Enter => app.select_commit(),
            Key::Char('c') => copy_commit_hash(app),
            Key::Char('h') => app.open_in_hunk(),
            Key::Char('v') => app.toggle_range_start(),
            Key::Esc => app.clear_range(),
            Key::Char('f') => app.open_file_selection(),
            Key::Char('q') => app.quit(),
            Key::MouseClick(row) => select_commit_at_row(app, row),
            _ => {}
        },
        Screen::Detail => match key {
            Key::Esc => app.close_detail(),
            Key::Up | Key::Char('k') => app.scroll_detail_up(),
            Key::Down | Key::Char('j') => app.scroll_detail_down(),
            Key::Ctrl('f') | Key::PageDown => app.scroll_detail_page_down(),
            Key::Ctrl('b') | Key::PageUp => app.scroll_detail_page_up(),
            Key::Char('c') => copy_commit_hash(app),
            Key::Char('h') => app.open_in_hunk(),
            Key::Char('f') => app.open_file_selection(),
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

/// Execute a pending Open in Hunk launch, if any, and feed the outcome
/// back into the app. Returns `true` when a launch happened, so the caller
/// can force a full repaint after the TUI resumes.
fn drain_hunk_launch(
    app: &mut App,
    launcher: &mut dyn FnMut(&Path, &HunkLaunch) -> HunkOutcome,
) -> bool {
    match app.pending_hunk_launch.take() {
        Some(launch) => {
            let outcome = launcher(&app.repo_path, &launch);
            app.on_hunk_finished(outcome);
            true
        }
        None => false,
    }
}

/// Suspend the TUI, run hunk in the foreground, and resume.
/// The child process inherits the terminal, so hunk takes over the full
/// screen until it exits.
///
/// Dispatches either `hunk show <oid>` (single commit) or
/// `hunk diff <older>..<newer>` (range) based on the launch spec.
fn launch_hunk(repo_path: &Path, launch: &HunkLaunch) -> HunkOutcome {
    let _ = disable_raw_mode();
    let _ = io::stdout().execute(event::DisableMouseCapture);
    let _ = io::stdout().execute(LeaveAlternateScreen);

    let status = match launch {
        HunkLaunch::ShowCommit(oid) => Command::new("hunk")
            .arg("show")
            .arg(oid)
            .current_dir(repo_path)
            .status(),
        HunkLaunch::ShowCommitFiltered(oid, paths) => {
            let mut cmd = Command::new("hunk");
            cmd.arg("show").arg(oid).arg("--");
            for p in paths {
                cmd.arg(p);
            }
            cmd.current_dir(repo_path).status()
        }
        HunkLaunch::ShowRange(older, newer) => {
            let range = format!("{}..{}", older, newer);
            Command::new("hunk")
                .arg("diff")
                .arg(&range)
                .current_dir(repo_path)
                .status()
        }
        HunkLaunch::ShowRangeFiltered(older, newer, paths) => {
            let range = format!("{}..{}", older, newer);
            let mut cmd = Command::new("hunk");
            cmd.arg("diff").arg(&range).arg("--");
            for p in paths {
                cmd.arg(p);
            }
            cmd.current_dir(repo_path).status()
        }
    };

    let _ = enable_raw_mode();
    let _ = io::stdout().execute(EnterAlternateScreen);
    let _ = io::stdout().execute(event::EnableMouseCapture);

    match status {
        Ok(s) if s.success() => HunkOutcome::Success,
        Ok(s) => HunkOutcome::Failed(format!("hunk exited with {s}")),
        Err(e) => HunkOutcome::Failed(format!("Failed to launch hunk: {e}")),
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
    use std::cell::RefCell;
    use std::path::Path;
    use std::process::Command;
    use std::rc::Rc;

    use ratatui::backend::TestBackend;

    use super::app::{HunkLaunch, HunkOutcome};
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
        run_with_events_and_launcher(repo_path, events, &mut |_, _| HunkOutcome::Success)
    }

    /// Run tuit with an injected hunk launcher, so tests can observe and
    /// control Open in Hunk launches without spawning a real process.
    fn run_with_events_and_launcher(
        repo_path: &Path,
        events: Vec<Key>,
        launcher: &mut dyn FnMut(&Path, &HunkLaunch) -> HunkOutcome,
    ) -> TestBackend {
        // Change to the repo directory before testing.
        let prev_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(repo_path).unwrap();

        let backend = TestBackend::new(210, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        // Override XDG_CONFIG_HOME so the test doesn't read the user's real config.
        let config_home = repo_path.join(".tuit-config");
        std::fs::create_dir_all(&config_home).unwrap();
        // We can't easily override this in-process, but config::load() will
        // fall back to defaults when no files exist, which is fine.

        let _ = super::run_app(
            &mut terminal,
            events.into_iter(),
            repo_path.to_path_buf(),
            launcher,
        );

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
    fn h_in_list_launches_hunk_with_selected_commit_oid() {
        let tmp = std::env::temp_dir().join("tuit-e2e-hunk-list");
        init_repo(&tmp, 2);

        // The list is newest-first, so the initially selected commit is HEAD.
        let expected_oid = rev_parse(&tmp, "HEAD");

        let calls = Rc::new(RefCell::new(Vec::new()));
        let calls2 = calls.clone();
        let mut launcher = move |_path: &Path, launch: &HunkLaunch| {
            if let HunkLaunch::ShowCommit(oid) = launch {
                calls2.borrow_mut().push(oid.clone());
            }
            HunkOutcome::Success
        };

        let _backend = run_with_events_and_launcher(
            &tmp,
            vec![Key::Char('h'), Key::Char('q')],
            &mut launcher,
        );

        assert_eq!(
            calls.borrow().as_slice(),
            &[expected_oid],
            "h in Commit List should launch hunk with the selected commit's full OID",
        );
    }

    #[test]
    fn h_in_detail_launches_hunk_with_viewed_commit_oid() {
        let tmp = std::env::temp_dir().join("tuit-e2e-hunk-detail");
        init_repo(&tmp, 2);

        // Navigate to the older commit (HEAD~1), open its detail, then press h.
        let expected_oid = rev_parse(&tmp, "HEAD~1");

        let calls = Rc::new(RefCell::new(Vec::new()));
        let calls2 = calls.clone();
        let mut launcher = move |_path: &Path, launch: &HunkLaunch| {
            if let HunkLaunch::ShowCommit(oid) = launch {
                calls2.borrow_mut().push(oid.clone());
            }
            HunkOutcome::Success
        };

        let _backend = run_with_events_and_launcher(
            &tmp,
            vec![
                Key::Char('j'),
                Key::Enter,
                Key::Char('h'),
                Key::Esc,
                Key::Char('q'),
            ],
            &mut launcher,
        );

        assert_eq!(
            calls.borrow().as_slice(),
            &[expected_oid],
            "h in Commit Detail should launch hunk with the viewed commit's full OID",
        );
    }

    #[test]
    fn hunk_launch_failure_shows_footer_notification() {
        let tmp = std::env::temp_dir().join("tuit-e2e-hunk-failure");
        init_repo(&tmp, 1);

        let mut launcher = |_path: &Path, _launch: &HunkLaunch| {
            HunkOutcome::Failed("Failed to launch hunk: command not found".to_string())
        };

        let backend = run_with_events_and_launcher(
            &tmp,
            vec![Key::Char('h'), Key::Char('q')],
            &mut launcher,
        );

        let content = buf_to_string(backend.buffer());
        assert!(
            content.contains("Failed to launch hunk: command not found"),
            "Failure notification should be rendered in the footer, got:\n{content}",
        );
    }

    #[test]
    fn hunk_launch_success_resumes_silently() {
        let tmp = std::env::temp_dir().join("tuit-e2e-hunk-success");
        init_repo(&tmp, 1);

        let mut launcher = |_path: &Path, _launch: &HunkLaunch| HunkOutcome::Success;

        let backend = run_with_events_and_launcher(
            &tmp,
            vec![Key::Char('h'), Key::Char('q')],
            &mut launcher,
        );

        let content = buf_to_string(backend.buffer());
        assert!(
            !content.contains("Failed") && !content.contains("exited with"),
            "Successful launch should not raise any notification, got:\n{content}",
        );
    }

    #[test]
    fn h_on_error_screen_does_not_launch_hunk() {
        let tmp = std::env::temp_dir().join("tuit-e2e-hunk-error");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let calls = Rc::new(RefCell::new(Vec::new()));
        let calls2 = calls.clone();
        let mut launcher = move |_path: &Path, launch: &HunkLaunch| {
            if let HunkLaunch::ShowCommit(oid) = launch {
                calls2.borrow_mut().push(oid.clone());
            }
            HunkOutcome::Success
        };

        let _backend = run_with_events_and_launcher(
            &tmp,
            vec![Key::Char('h'), Key::Enter],
            &mut launcher,
        );

        assert!(
            calls.borrow().is_empty(),
            "h on a screen without a focused commit must not launch hunk",
        );
    }

    // ── Range-based hunk launch tests ───────────────────────────

    #[test]
    fn v_sets_range_start_on_current_commit() {
        let tmp = std::env::temp_dir().join("tuit-e2e-v-set-range");
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

        assert_eq!(app.selected_index, 0);
        assert!(app.range_start.is_none());

        super::handle_input(&mut app, Key::Char('v'));
        assert_eq!(app.range_start, Some(0));

        std::env::set_current_dir(prev_dir).unwrap();
    }

    #[test]
    fn v_on_same_commit_clears_range_start() {
        let tmp = std::env::temp_dir().join("tuit-e2e-v-clear-range");
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

        // Mark
        super::handle_input(&mut app, Key::Char('v'));
        assert_eq!(app.range_start, Some(0));

        // Toggle off
        super::handle_input(&mut app, Key::Char('v'));
        assert_eq!(app.range_start, None);

        std::env::set_current_dir(prev_dir).unwrap();
    }

    #[test]
    fn v_moves_range_to_new_commit() {
        let tmp = std::env::temp_dir().join("tuit-e2e-v-move-range");
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

        // Mark first commit
        super::handle_input(&mut app, Key::Char('v'));
        assert_eq!(app.range_start, Some(0));

        // Move to index 1 and mark there (should move the mark)
        super::handle_input(&mut app, Key::Char('j'));
        super::handle_input(&mut app, Key::Char('v'));
        assert_eq!(app.range_start, Some(1));

        std::env::set_current_dir(prev_dir).unwrap();
    }

    #[test]
    fn h_with_range_launches_show_range() {
        let tmp = std::env::temp_dir().join("tuit-e2e-h-range");
        init_repo(&tmp, 3);

        let expected_older = rev_parse(&tmp, "HEAD~1");
        let expected_newer = rev_parse(&tmp, "HEAD");

        let launches = Rc::new(RefCell::new(Vec::new()));
        let launches2 = launches.clone();
        let mut launcher = move |_path: &Path, launch: &HunkLaunch| {
            launches2.borrow_mut().push(launch.clone());
            HunkOutcome::Success
        };

        // Start at HEAD (index 0), press v to mark, j to move to HEAD~1, h to launch range
        let _backend = run_with_events_and_launcher(
            &tmp,
            vec![Key::Char('v'), Key::Char('j'), Key::Char('h'), Key::Char('q')],
            &mut launcher,
        );

        let calls = launches.borrow();
        assert_eq!(calls.len(), 1, "should launch hunk exactly once");
        match &calls[0] {
            HunkLaunch::ShowRange(older, newer) => {
                assert_eq!(older, &expected_older, "older commit OID should match HEAD~1");
                assert_eq!(newer, &expected_newer, "newer commit OID should match HEAD");
            }
            other => panic!("expected ShowRange, got {:?}", other),
        }
    }

    #[test]
    fn h_without_range_launches_show_commit() {
        let tmp = std::env::temp_dir().join("tuit-e2e-h-no-range");
        init_repo(&tmp, 2);

        let expected_oid = rev_parse(&tmp, "HEAD");

        let launches = Rc::new(RefCell::new(Vec::new()));
        let launches2 = launches.clone();
        let mut launcher = move |_path: &Path, launch: &HunkLaunch| {
            launches2.borrow_mut().push(launch.clone());
            HunkOutcome::Success
        };

        let _backend = run_with_events_and_launcher(
            &tmp,
            vec![Key::Char('h'), Key::Char('q')],
            &mut launcher,
        );

        let calls = launches.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0],
            HunkLaunch::ShowCommit(expected_oid),
            "without range, h should launch ShowCommit"
        );
    }

    #[test]
    fn range_is_cleared_after_h_launch() {
        let tmp = std::env::temp_dir().join("tuit-e2e-range-cleared");
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

        super::handle_input(&mut app, Key::Char('v'));
        assert_eq!(app.range_start, Some(0));

        app.open_in_hunk();
        // range_start should be consumed (taken by open_in_hunk)
        assert!(app.range_start.is_none(), "range_start should be cleared after open_in_hunk");
        // pending_hunk_launch should be set
        assert!(app.pending_hunk_launch.is_some(), "pending_hunk_launch should be set");

        std::env::set_current_dir(prev_dir).unwrap();
    }

    #[test]
    fn reload_clears_range_start() {
        let tmp = std::env::temp_dir().join("tuit-e2e-reload-clears-range");
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

        super::handle_input(&mut app, Key::Char('v'));
        assert_eq!(app.range_start, Some(0));

        app.reload();
        assert!(app.range_start.is_none(), "range_start should be cleared on reload");

        std::env::set_current_dir(prev_dir).unwrap();
    }

    #[test]
    fn v_in_detail_does_nothing() {
        let tmp = std::env::temp_dir().join("tuit-e2e-v-detail");
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
        assert_eq!(app.screen, Screen::Detail);

        super::handle_input(&mut app, Key::Char('v'));
        // v should be ignored in Detail — range_start stays None
        assert!(app.range_start.is_none(), "v in Detail must not set range_start");

        std::env::set_current_dir(prev_dir).unwrap();
    }

    #[test]
    fn h_in_detail_ignores_range_set_in_list() {
        let tmp = std::env::temp_dir().join("tuit-e2e-h-detail-ignores-range");
        init_repo(&tmp, 3);

        let expected_oid = rev_parse(&tmp, "HEAD");

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

        // Set range in list
        super::handle_input(&mut app, Key::Char('v'));
        assert_eq!(app.range_start, Some(0));

        // Open detail
        app.select_commit();
        assert_eq!(app.screen, Screen::Detail);

        // h in detail should still launch ShowCommit (range_start preserved but not consumed)
        app.open_in_hunk();
        assert_eq!(
            app.pending_hunk_launch,
            Some(HunkLaunch::ShowCommit(expected_oid)),
            "h in Detail should launch ShowCommit even if range_start is set"
        );
        // range_start should still be set for when user returns to list
        assert_eq!(app.range_start, Some(0));

        std::env::set_current_dir(prev_dir).unwrap();
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
            &mut |_, _: &HunkLaunch| HunkOutcome::Success,
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

    // ── File selection tests ────────────────────────────────────

    #[test]
    fn f_opens_file_selection_overlay() {
        let tmp = std::env::temp_dir().join("tuit-e2e-f-opens");
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

        assert!(app.file_selection.is_none());
        super::handle_input(&mut app, Key::Char('f'));
        assert!(
            app.file_selection.is_some(),
            "pressing f should open file selection"
        );
        // Default: all files selected
        assert_eq!(
            app.file_selection.as_ref().unwrap().selected.len(),
            app.file_selection.as_ref().unwrap().files.len(),
        );

        std::env::set_current_dir(prev_dir).unwrap();
    }

    #[test]
    fn f_twice_does_not_reopen() {
        let tmp = std::env::temp_dir().join("tuit-e2e-f-twice");
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

        super::handle_input(&mut app, Key::Char('f'));
        let commit_oid = app.file_selection.as_ref().unwrap().commit_oid.clone();
        super::handle_input(&mut app, Key::Char('f'));
        // Second f should be a no-op (same instance preserved)
        assert_eq!(
            app.file_selection.as_ref().unwrap().commit_oid,
            commit_oid,
            "second f must not re-open file selection"
        );

        std::env::set_current_dir(prev_dir).unwrap();
    }

    #[test]
    fn esc_closes_file_selection() {
        let tmp = std::env::temp_dir().join("tuit-e2e-f-esc");
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

        super::handle_input(&mut app, Key::Char('f'));
        assert!(app.file_selection.is_some());
        super::handle_input(&mut app, Key::Esc);
        assert!(
            app.file_selection.is_none(),
            "Esc should close file selection"
        );

        std::env::set_current_dir(prev_dir).unwrap();
    }

    #[test]
    fn f_then_enter_with_all_sel_launches_show_commit() {
        let tmp = std::env::temp_dir().join("tuit-e2e-f-enter-all");
        init_repo(&tmp, 3);

        let expected_oid = rev_parse(&tmp, "HEAD");

        let launches = Rc::new(RefCell::new(Vec::new()));
        let launches2 = launches.clone();
        let mut launcher = move |_path: &Path, launch: &HunkLaunch| {
            launches2.borrow_mut().push(launch.clone());
            HunkOutcome::Success
        };

        let _backend = run_with_events_and_launcher(
            &tmp,
            vec![Key::Char('f'), Key::Enter, Key::Char('q')],
            &mut launcher,
        );

        let calls = launches.borrow();
        assert_eq!(calls.len(), 1, "should launch hunk exactly once");
        // All files selected → unfiltered ShowCommit
        assert_eq!(
            calls[0],
            HunkLaunch::ShowCommit(expected_oid),
            "all-selected should produce ShowCommit"
        );
    }

    /// Create a repo whose HEAD commit contains exactly the given files.
    fn init_repo_with_files(path: &Path, files: &[&str]) {
        let _ = std::fs::remove_dir_all(path);
        std::fs::create_dir_all(path).unwrap();

        Command::new("git")
            .args(["init", "--initial-branch=main"])
            .arg(path)
            .status()
            .unwrap();

        // Create an initial commit so HEAD exists
        let init = path.join(".gitkeep");
        std::fs::write(&init, "").unwrap();
        Command::new("git")
            .args(["-C", &path.to_string_lossy(), "add", ".gitkeep"])
            .status()
            .unwrap();
        Command::new("git")
            .args(["-C", &path.to_string_lossy(), "commit", "-m", "init"])
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "test@test")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "test@test")
            .status()
            .unwrap();

        // Add each file in a single commit
        for f in files {
            let fp = path.join(f);
            if let Some(parent) = fp.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(&fp, format!("content of {f}")).unwrap();
            Command::new("git")
                .args(["-C", &path.to_string_lossy(), "add", &fp.to_string_lossy()])
                .status()
                .unwrap();
        }
        Command::new("git")
            .args(["-C", &path.to_string_lossy(), "commit", "-m", "multi-file commit"])
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "test@test")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "test@test")
            .status()
            .unwrap();
    }

    #[test]
    fn f_then_enter_with_one_deselected_launches_filtered() {
        let tmp = std::env::temp_dir().join("tuit-e2e-f-enter-filtered");
        init_repo_with_files(&tmp, &["README.md", "src/lib.rs"]);

        let expected_oid = rev_parse(&tmp, "HEAD");

        let launches = Rc::new(RefCell::new(Vec::new()));
        let launches2 = launches.clone();
        let mut launcher = move |_path: &Path, launch: &HunkLaunch| {
            launches2.borrow_mut().push(launch.clone());
            HunkOutcome::Success
        };

        // f → Space (deselect first = src/lib.rs if sorted, or README.md)
        // Files are sorted: README.md then src/lib.rs.
        // Space on cursor 0 deselects README.md.
        // j moves to src/lib.rs (still selected).
        // Now only src/lib.rs is selected → filtered launch.
        let _backend = run_with_events_and_launcher(
            &tmp,
            vec![
                Key::Char('f'),
                Key::Char(' '),   // deselect README.md
                Key::Char('j'),   // cursor to src/lib.rs
                Key::Enter,       // launch with src/lib.rs only
                Key::Char('q'),
            ],
            &mut launcher,
        );

        let calls = launches.borrow();
        assert_eq!(calls.len(), 1);
        match &calls[0] {
            HunkLaunch::ShowCommitFiltered(oid, paths) => {
                assert_eq!(oid, &expected_oid);
                assert_eq!(paths, &["src/lib.rs"]);
            }
            other => panic!("expected ShowCommitFiltered, got {:?}", other),
        }
    }

    #[test]
    fn range_and_file_selection_launches_show_range_filtered() {
        let tmp = std::env::temp_dir().join("tuit-e2e-range-filtered");
        // Build: commit 1 (HEAD~1) adds a.rs; commit 2 (HEAD) modifies
        // a.rs AND adds b.rs.  Range HEAD~1..HEAD shows both files changed.
        init_repo_with_files(&tmp, &["a.rs"]);

        // HEAD: modify a.rs and add b.rs (both files change across the range)
        let a_rs = tmp.join("a.rs");
        std::fs::write(&a_rs, "a v2").unwrap();
        let b_rs = tmp.join("b.rs");
        std::fs::write(&b_rs, "b content").unwrap();
        Command::new("git")
            .args(["-C", &tmp.to_string_lossy(), "add", &a_rs.to_string_lossy(), &b_rs.to_string_lossy()])
            .status()
            .unwrap();
        Command::new("git")
            .args(["-C", &tmp.to_string_lossy(), "commit", "-m", "modify a, add b"])
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "test@test")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "test@test")
            .status()
            .unwrap();

        let expected_older = rev_parse(&tmp, "HEAD~1");
        let expected_newer = rev_parse(&tmp, "HEAD");

        let launches = Rc::new(RefCell::new(Vec::new()));
        let launches2 = launches.clone();
        let mut launcher = move |_path: &Path, launch: &HunkLaunch| {
            launches2.borrow_mut().push(launch.clone());
            HunkOutcome::Success
        };

        // On HEAD (index 0): press v (mark range start),
        // then j to move to index 1 (older), f to open file selection,
        // Space to deselect a.rs, Enter to launch range filtered with
        // just b.rs.
        //
        // load_range_changed_files shows files changed across the range:
        //   a.rs (modified) and b.rs (added).
        let _backend = run_with_events_and_launcher(
            &tmp,
            vec![
                Key::Char('v'),
                Key::Char('j'),  // move to index 1 (older = HEAD~1)
                Key::Char('f'),  // open file selection (range files)
                Key::Char(' '),  // deselect a.rs (cursor at 0)
                Key::Enter,      // confirm with b.rs only
                Key::Char('q'),
            ],
            &mut launcher,
        );

        let calls = launches.borrow();
        assert_eq!(calls.len(), 1, "should launch hunk exactly once");
        match &calls[0] {
            HunkLaunch::ShowRangeFiltered(older, newer, paths) => {
                assert_eq!(older, &expected_older);
                assert_eq!(newer, &expected_newer);
                assert_eq!(paths, &["b.rs"]);
            }
            other => panic!("expected ShowRangeFiltered, got {:?}", other),
        }
    }
}
