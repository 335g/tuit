Status: ready-for-agent
Type: task

# 08: Wire everything in main.rs

## Goal

Replace the existing Hello World TUI loop in `main.rs` with the full tuit application, connecting all modules.

## Main flow

Introduce a single generic entry point that can be used by both production and tests:

```rust
pub fn run_app<B: Backend, I: Iterator<Item = KeyEvent>>(
    terminal: &mut Terminal<B>,
    events: I,
    working_dir: &Path,
) -> Result<()>
```

This function:
1. Loads config and colors from `~/.config/tuit/`.
2. Initializes `App` state and loads commits from `working_dir`.
3. Runs the event loop: read the next event, update app state, draw the frame.
4. Returns when `app.should_quit` becomes true.

`main()` does the real terminal setup and wires `CrosstermBackend`, `crossterm::event::read()` (via a small adapter), and the current working directory:

```rust
fn main() -> Result<()> {
    enable_raw_mode()?;
    io::stdout().execute(EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    let res = run_app(&mut terminal, real_events(), current_dir());

    disable_raw_mode()?;
    io::stdout().execute(LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    if let Err(e) = res {
        eprintln!("Error: {e}");
    }
    Ok(())
}
```

## Event handling (`handle_input`)

```rust
fn handle_input(app: &mut App) -> Result<()> {
    match app.screen {
        Screen::List => {
            match key.code {
                KeyCode::Up => app.navigate_up(),
                KeyCode::Down => app.navigate_down(),
                KeyCode::Enter => app.select_commit()?,
                KeyCode::Char('q') => app.quit(),
                _ => {}
            }
        }
        Screen::Detail => {
            match key.code {
                KeyCode::Esc => app.close_detail(),
                _ => {}
            }
        }
        Screen::Error(_) => {
            match key.code {
                KeyCode::Enter => app.quit(),
                _ => {}
            }
        }
        Screen::Loading => {
            // ignore all input while loading
        }
    }
}
```

## Render dispatch

```rust
fn render(app: &App, frame: &mut Frame) {
    match &app.screen {
        Screen::Loading => render_loading(frame),
        Screen::List => render_commit_list(frame, app),
        Screen::Detail => {
            render_commit_list(frame, app);  // render list in background
            render_detail_overlay(frame, app);  // overlay on top
        }
        Screen::Error(msg) => render_error(frame, msg),
    }
}
```

*Note*: The `Screen::Detail` case draws the list first, then overlays the detail on top (ratatui renders widgets in order; later widgets paint over earlier ones).

## Acceptance

- Application starts, loads config, loads commits, and displays the list
- All keybindings work (↑↓, Enter, Esc, q)
- Non-git directory shows error screen
- Commits list renders with colors from the theme
- Overlay opens and closes correctly
- Application exits cleanly (raw mode disabled, cursor shown, alternate screen left)
