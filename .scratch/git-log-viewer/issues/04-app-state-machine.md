Status: ready-for-agent
Type: task

# 04: TUI application state machine

## Goal

Implement `src/app.rs` — the central application state that manages which screen is active, which commit is selected, and the data required for rendering.

## State model

```rust
pub enum Screen {
    Loading,     // Initial state while git data is being fetched
    List,        // Commit list (main screen)
    Detail,      // Commit detail overlay
    Error(String),  // Error screen with message
}

pub struct App {
    pub screen: Screen,
    pub commits: Vec<Commit>,
    pub selected_index: usize,      // Which commit is highlighted
    pub selected_commit: Option<Commit>,  // Full detail including diff
    pub colors: Colors,
    pub should_quit: bool,
}
```

## Methods

### `App::new(config: Config) -> Self`
Initialize with `Screen::Loading`, empty commits, and parsed colors from config.

### `App::load_commits(&mut self) -> Result<()>`
Call `git::load_commits()`, store results, transition to `Screen::List`.
On error, transition to `Screen::Error(message)`.

### `App::navigate_up(&mut self)` / `navigate_down(&mut self)`
Move `selected_index` within bounds. No-op on `Screen::Detail`.

### `App::select_commit(&mut self) -> Result<()>`
Called on Enter. Call `git::load_diff()` for the selected commit, populate `selected_commit`, transition to `Screen::Detail`.
On error, stay on `Screen::List` (log/ignore the error for now).

### `App::close_detail(&mut self)`
Transition from `Screen::Detail` back to `Screen::List`, preserve `selected_index`.

### `App::quit(&mut self)`
Set `should_quit = true`.

### `App::error_dismiss(&mut self)`
Transition from `Screen::Error` to quitting (the error screen's only action is exit).

## Acceptance

- State transitions follow the correct flow: `Loading → List ↔ Detail`, `Loading → Error`, `Error → quit`
- `navigate_up`/`navigate_down` clamp within `[0, commits.len()-1]`
- `select_commit` falls back gracefully if diff loading fails (stays on list, no crash)
- `close_detail` preserves scroll position
