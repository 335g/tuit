Status: needs-triage
Type: task

# 08: Wire everything in main.rs

## Goal

Replace the existing Hello World TUI loop in `main.rs` with the full ratty application, connecting all modules.

## Main flow

```rust
fn main() -> Result<()> {
    // 1. Load config
    let config = config::load()?;

    // 2. Initialize app state
    let mut app = App::new(config);

    // 3. Load commits (async or sync before entering TUI loop)
    if let Err(e) = app.load_commits() {
        // App transitions to Error screen internally
    }

    // 4. Enter TUI loop (terminal setup as before)
    enable_raw_mode()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;

    while !app.should_quit {
        // 5. Handle input
        handle_input(&mut app)?;

        // 6. Render
        terminal.draw(|frame| {
            render(&app, frame);
        })?;
    }

    // 7. Cleanup (same as before)
    disable_raw_mode()?;
    // ...
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
