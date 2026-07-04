---
Status: Accepted
Date: 2026-07-05
---

# Timer-based polling for live sync instead of filesystem watching

ratty needs to keep its commit list and detail view in sync with the underlying git repository while running. We chose a fixed-interval timer polling strategy (re-calling `gix::open` + revision walk every 2 s by default) over filesystem watching via `notify`.

## Why

- **Reliability across git operations.** `.git/refs/` files can vanish during `pack-refs` or `gc`, causing false negatives for an inotify-based watcher. Timer polling checks the actual object store every cycle, so it catches every mutation — amend, rebase, reset, fetch, gc — without gaps.
- **Zero new dependencies.** Switching the event loop from blocking `event::read()` to `event::poll(Duration)` is the only change needed — no async runtime, no watcher crate.
- **Acceptable cost.** A full `gix::open()` + revision walk of a 10 000-commit repo completes in well under 100 ms on modern hardware; polling at 2 s intervals uses negligible CPU and I/O.

## Trade-offs

- Polling never reaches true zero latency — a commit made milliseconds after a poll cycle won't appear until the next tick. For a TUI operated by a human, the gap is imperceptible.
- Battery-powered machines see a wake-up every poll interval; the cost is roughly one `gix` open + walk per tick (~50 ms of CPU every 2 s), which is comparable to background OS indexing.
- FS watching could theoretically be added as an optimisation later if polling proves too wasteful — but our benchmark data (see ADR companion) suggests it won't.

## Considered alternatives

1. **`notify` crate (inotify / kqueue / ReadDirectoryChanges).** Rejected because: (a) packed-refs and gc produce false signals the watcher must debounce; (b) cross-platform behaviour varies significantly (WSL, Docker mounts); (c) the added complexity of a background watcher thread with `AtomicBool` handshake isn't justified at this scale.
2. **Manual reload key (`r`).** Rejected because the user explicitly wants the view to stay up-to-date automatically.
