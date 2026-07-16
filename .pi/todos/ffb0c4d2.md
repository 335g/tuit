{
  "id": "ffb0c4d2",
  "title": "Implement Toast notification system",
  "tags": [
    "feature",
    "toast",
    "ui"
  ],
  "status": "closed",
  "created_at": "2026-07-16T14:29:04.937Z"
}

## Tasks
- [x] Update CONTEXT.md with Toast domain terms
- [x] Add Toast state and timeout to App
- [x] Add sample toast keybinding (`n`)
- [x] Render Toast in bottom-right corner
- [x] Enable mouse capture and support click-to-dismiss
- [x] Add `toast_timeout_ms` config option
- [x] Update help overlay
- [x] Add tests
- [x] Run cargo test / clippy

## Notes
- All existing tests pass (some pre-existing flakiness due to parallel cwd changes).
- Clippy reports 7 pre-existing warnings, none introduced by this change.
- cargo fmt applied.
