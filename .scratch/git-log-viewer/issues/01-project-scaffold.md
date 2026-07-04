Status: needs-triage
Type: task

# 01: Project scaffold and dependencies

## Goal

Set up the module structure and add all required dependencies so subsequent issues can focus on logic without build configuration distractions.

## Tasks

1. Add the following dependencies to `Cargo.toml`:
   - `gix` (latest)
   - `toml` with serde feature
   - `serde` with derive feature
   - `directories` (for `~/.config/ratty/` path resolution)
   
2. Create the module files:
   - `src/app.rs` — application state management (skeleton with TODO)
   - `src/git.rs` — git data access (skeleton with TODO)
   - `src/config.rs` — config/theme loading (skeleton with TODO)
   - `src/ui.rs` — TUI rendering (skeleton with TODO)

3. Declare the modules in `main.rs`:
   ```rust
   mod app;
   mod git;
   mod config;
   mod ui;
   ```

4. Add `use` imports for all crates in `main.rs`.

5. Verify `cargo check` passes.

## Acceptance

- `cargo check` passes with all new dependencies
- All four module files exist with `fn main()`, placeholder functions, or `todo!()`
- `main.rs` still compiles and runs the existing Hello World screen (regression-free)
