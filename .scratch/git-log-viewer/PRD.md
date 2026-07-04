> Status: ready-for-agent

# PRD: ratty — TUI Git Log Viewer

## Problem Statement

Terminal users inspect git history by running commands like `git log --oneline` and `git show`, but these commands are read-only and require switching contexts between different invocations. There is no lightweight, keyboard-driven TUI that lets a user open a repository, scroll through the current branch's commits, and inspect a selected commit's full message and diff from a single session.

## Solution

Build ratty as a terminal UI application that opens the current git repository, shows a scrollable Commit List of the current branch, and lets the user open a Commit Detail overlay for the selected commit. The tool is driven entirely by the keyboard, starts with sensible defaults, and can be customized via a config file and color theme.

## User Stories

1. As a terminal user, I want to run `ratty` inside a git repository and immediately see the current branch's commits, so that I can browse history without typing `git log` commands.
2. As a user, I want the Commit List to show each commit's abbreviated hash, subject message, author name, and relative timestamp, so that I can scan the history at a glance.
3. As a user, I want to press `↑` and `↓` to move the cursor through the Commit List, so that I can select the commit I care about.
4. As a user, I want to press `Enter` on a commit to open a Commit Detail overlay, so that I can read the full commit body and diff.
5. As a user, I want to press `Esc` to close the Commit Detail overlay and return to the Commit List, so that I can continue browsing without losing my place.
6. As a user, I want to press `q` to quit ratty from the Commit List, so that I can return to my shell.
7. As a user, I want ratty to show a clear error message when I run it outside a git repository, so that I understand why it cannot start.
8. As a user, I want ratty to show a clear message when the repository has no commits, so that I know the repository is empty.
9. As a user, I want ratty to handle a corrupt `.git` directory or permission error gracefully, so that the application does not panic.
10. As a user, I want ratty to work correctly in a detached HEAD state, so that I can still inspect the commit at HEAD.
11. As a user, I want to customize ratty's colors by dropping a TOML theme file into `~/.config/ratty/themes/`, so that the tool matches my terminal colorscheme.
12. As a user, I want ratty to fall back to a built-in default theme when no config or theme file exists, so that the tool works out of the box.
13. As a user, I want the selected commit row to be highlighted, so that I always know which commit is active.
14. As a user, I want diff lines to be color-coded (added, deleted, context), so that I can read patches quickly.
15. As a maintainer, I want a single end-to-end test seam that exercises the whole application against a temporary git repository, so that the UI behaves correctly without requiring manual terminal interaction.

## Implementation Decisions

- **Data source.** ratty uses `gix` (gitoxide) to read the local `.git` object store. This avoids a runtime dependency on the `git` CLI and provides typed access to commits and diffs. See ADR-0001.
- **Commit scope.** Phase 1 only displays the current branch (the commit graph reachable from HEAD). Displaying all branches, graphs, and interactive git operations are deferred.
- **Eager loading.** On startup, ratty loads all commits from the current branch into memory. This keeps the rendering logic simple; lazy loading is deferred.
- **Module responsibilities.** The application is split into four modules:
  - a state machine that owns the current screen, selected index, loaded commits, and parsed colors;
  - a git module that opens the repository and extracts commits and diffs;
  - a config module that resolves `~/.config/ratty/config.toml` and loads theme files;
  - a UI module that renders the Commit List, Commit Detail overlay, and error screen using ratatui.
- **Screen states.** The state machine supports four screens: Loading, List, Detail, and Error. From List the user can navigate, select a commit, or quit. From Detail the user can return to List. From Error the only action is exit.
- **Commit Detail content.** The overlay displays author, date, subject, body, and a unified diff. If the diff does not fit, it is truncated; scrollable detail is out of scope for Phase 1.
- **Color system.** A `Colors` value is produced from either a user-supplied theme file or a hard-coded default palette. UI code applies these colors per field (hash, message, author, date, diff add/del/context, etc.).
- **Path resolution.** The configuration directory is resolved with the `directories` crate; ratty does not accept a path argument and always operates on the current working directory.
- **Test seam.** The application exposes a generic entry point that accepts any ratatui `Backend` and an iterator of key events. Production wiring uses `CrosstermBackend` and real crossterm events; tests use `TestBackend` and a scripted event list.
- **Error handling.** All git and config errors are surfaced to the user as a rendered Error screen rather than panicking or dumping a backtrace.

## Testing Decisions

- **One high-level seam.** All functional tests run the full application through the generic entry point using `TestBackend` and a temporary git repository created in a test fixture.
- **What a good test looks like.** A test sets up a repo with known commits, runs ratty with a scripted key sequence, and asserts on the rendered terminal buffer (e.g., that a specific commit hash appears, that the cursor moves, that the detail overlay opens, or that an error screen appears). Tests only inspect external behavior, not module internals.
- **No isolated unit tests.** Individual modules (`git`, `config`, `ui`) are not tested in isolation. The single seam gives enough confidence for Phase 1 and avoids over-specifying implementation details.
- **No tests for the binary entry point.** The `main` function only wires real backend/events/cleanup and is exercised manually or by integration tooling outside the PRD scope.

## Out of Scope

- Displaying all branches, remotes, tags, or stashes.
- Commit graph / DAG visualization.
- Interactive git operations (checkout, rebase, cherry-pick, staging, etc.).
- File-level history browser.
- Path argument to open a repository other than the current directory.
- Scrollable detail overlay.
- Theme hot reload, theme listing command, or 256-color fallback.
- Searching/filtering commits by text, author, or date.
- Mouse support.
- Performance guarantees for repositories larger than tens of thousands of commits.

## Further Notes

- The default theme should be a dark palette that is legible in most terminals. It should define colors for the cursor row, list fields, overlay border, overlay headings/values, and diff lines.
- The date display uses relative time strings (`3d ago`, `2h ago`, `30m ago`, `3mo ago`, `1y ago`). The exact cutoff points are left to the implementation, but the format must stay within the 10-character width allocated in the Commit List.
- Merge commits in the detail overlay are diffed against the first parent only.
- Future phases can introduce branch switching, all-branches view, and scrolling detail without changing the core state machine shape.
