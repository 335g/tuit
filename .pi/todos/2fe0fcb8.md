{
  "id": "2fe0fcb8",
  "title": "Show toast after copying commit hash",
  "tags": [
    "feature",
    "toast",
    "clipboard"
  ],
  "status": "closed",
  "created_at": "2026-07-16T14:43:23.148Z"
}

## Tasks
- [x] Change copy_commit_hash to take &mut App
- [x] Show success toast with short hash
- [x] Update tests if needed
- [x] Run cargo test / fmt / clippy

## Notes
- Tests pass with `--test-threads=1` (parallel flakiness is pre-existing due to cwd changes).
- Clippy reports 7 pre-existing warnings, none introduced by this change.
