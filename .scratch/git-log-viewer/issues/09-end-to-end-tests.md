Status: ready-for-agent
Type: task

# 09: End-to-end tests through the single TestBackend seam
> 
> ## Goal
> 
> Implement the end-to-end test seam described in the PRD and add tests that exercise the full application against temporary git repositories.
> 
> ## Test seam
> 
> The application exposes `run_app<B: Backend, I: Iterator<Item = KeyEvent>>(terminal, events, working_dir)`. Tests call this with:
> 
> - `ratatui::backend::TestBackend`
> - A `Vec<KeyEvent>` or iterator of scripted key events
> - A temporary git repository path
> 
> The test inspects the terminal buffer after the event sequence completes.
> 
> ## Test cases
> 
> 1. **Happy path: commit list**
>    - Create a repo with 3 commits.
>    - Run `tuit` and immediately quit.
>    - Assert that the buffer contains the abbreviated hashes and subject lines of all 3 commits.
> 
> 2. **Cursor movement**
>    - Create a repo with 3 commits.
>    - Send `↓` once.
>    - Assert that the second commit row is highlighted (e.g., contains the `❯` marker at the expected position).
> 
> 3. **Commit Detail overlay**
>    - Create a repo with at least 1 commit.
>    - Send `Enter`.
>    - Assert that the buffer contains "Commit {hash}" and the author line.
>    - Send `Esc`.
>    - Assert that the overlay is gone and the Commit List is visible again.
> 
> 4. **Non-git directory**
>    - Run `tuit` in a directory with no `.git`.
>    - Assert that the buffer contains the error message "tuit は git リポジトリの中で実行してください。".
> 
> 5. **Empty repository**
>    - Create an empty git repo (no commits).
>    - Run `tuit`.
>    - Assert that the buffer contains "このリポジトリにはまだコミットがありません。".
> 
> 6. **Custom theme**
>    - Create a temporary `~/.config/tuit/config.toml` referencing a custom theme and a matching theme file in `~/.config/tuit/themes/`.
>    - Run `tuit` with 1 commit.
>    - Assert that the rendered colors match the theme (using `TestBackend::buffer()` cell colors).
> 
> ## Implementation notes
> 
> - Use a helper function in `tests/` such as `fn run_with_events(repo: &Path, events: Vec<KeyEvent>) -> TestBackend`.
> - Override the config directory in tests via an environment variable or by passing a config path to `run_app` if the seam is extended. If that is not available, set `XDG_CONFIG_HOME` to a temp directory for the test process.
> - Temporary git repositories can be created with `std::process::Command` running `git init` and `git commit` in a temp dir, or via a small helper.
> 
> ## Acceptance
> 
> - All test cases above pass with `cargo test`.
> - Tests only inspect rendered terminal output and never call internal module functions directly.
> - `cargo test` runs in CI without a real terminal.
