---
Status: Accepted
Date: 2026-07-17
---

# Open in Hunk: delegate deep review to the external hunk process

tuit renders commit diffs in-process for quick inspection, but deep review — multi-file navigation, inline comments fed back to a coding agent — is delegated to Hunk, an external terminal diff review tool. Pressing `h` in the Commit List or the Commit Detail suspends tuit (leaving the alternate screen), runs `hunk show <oid>` as a foreground child process, and resumes tuit when hunk exits.

## Why

- **Role division in the Agent Review Loop.** The target workflow: a coding agent's changes are auto-committed per turn; tuit (via Live Sync) is the history navigator that surfaces those commits; Hunk is where comments are written and fed back to the agent. Re-implementing Hunk's review features in-process would duplicate a tool dedicated to exactly this.
- **Deliberate exception to ADR-0001.** ADR-0001 established "no external CLI dependencies" for reading git data. That principle still holds for tuit's core function — tuit is fully functional without hunk installed. Open in Hunk is an optional integration with the user's own tool, not a git-data dependency, and degrades gracefully (a footer Notification) when hunk is absent.
- **Suspend-and-resume is terminal-agnostic.** It works in any terminal without coupling tuit to a multiplexer. On resume, the existing Live Sync poll naturally picks up any commits that landed during the review (HEAD Change + OID Tracking), so no special catch-up logic is needed.

## Trade-offs

- While hunk runs, tuit is frozen (no visible live updates). Accepted: the review target is a fixed commit, and repository state catches up on resume.
- Spawning an external process introduces a new failure surface (missing binary, non-zero exit). Handled with transient Notifications rather than modal Alerts, and detected lazily at spawn time — no startup pre-check, keeping tuit stateless about hunk's availability.
- **Zero configuration by design.** The command (`hunk show <oid>`, resolved via `$PATH`) is not configurable in v1. A `[hunk]` config section (binary path, extra args) can be added later without breaking anything if real demand appears.

## Considered alternatives

1. **Multiplexer integration (e.g. tmux split-window).** Keeps both TUIs visible side by side, but couples tuit to a specific multiplexer and pane layout. Deferred; may be revisited if side-by-side review proves important in practice.
2. **In-process review features.** Full control and no external dependency, but means re-implementing multi-file review, annotations, and comment export — the exact scope Hunk already covers.
