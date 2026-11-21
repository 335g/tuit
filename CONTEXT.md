# tuit

A TUI git log viewer built with ratatui. Displays a scrollable list of commits and allows selecting one to view its full details.

## Language

**Repository Path**:
The filesystem path of the Git repository that tuit opens. Defaults to the current working directory; may be overridden when launching the application.

**Commit**:
A git commit, identified by its abbreviated hash (7 chars), with an author, relative timestamp, and a one-line subject message.

**Commit List**:
The primary view — a scrollable, selectable list of commits sorted chronologically (newest first).

**Commit Detail**:
An overlay shown when a commit is selected, displaying the abbreviated hash (7 chars), author, date, commit body, and diff output.

**Live Sync**:
The mechanism by which the commit list and detail view are kept up-to-date with the underlying git repository while the application is running. Implemented via timer-based polling.

**Poll Interval**:
The time between successive git state checks, expressed in milliseconds. Configurable; defaults to 2000 ms.

**HEAD Change**:
An event that occurs when the repository's HEAD resolves to a different OID than the previously recorded value. Causes the commit list to reload and the selection to be re-anchored via OID tracking (falling back to index 0 if the OID is not found).

**Commit Deletion (Deletion)**:
An event that occurs when a commit object selected for detail view no longer exists in the repository while HEAD has not changed. Displayed as a modal alert requiring user acknowledgment.

**OID**:
The full 40-character hex string of a git object ID, used as the canonical stable identifier for commits.

**OID Tracking**:
The strategy of preserving the user's selection across commit list reloads by locating the same OID in the new list.

**Screen**:
One of: Loading, List, Detail, Error, Alert. The Alert screen carries an AlertKind to distinguish between HEAD Change (informational, auto-dismiss) and Commit Deletion (requires acknowledgment).

**AlertKind**:
A discriminated union identifying the reason for a modal alert. Currently: `CommitDeleted { oid }`.

**Notification**:
A transient non-blocking message displayed in the footer. The entire bottom row is filled with the theme's accent colour and the message text is centred. Used for HEAD Change announcements and Copy to Clipboard confirmations.

**Notification Timeout**:
The duration (in milliseconds) that a footer notification is displayed before automatically dismissing. Configurable; defaults to 3000 ms.

**Copy to Clipboard**:
The action of copying a commit's full 40-character OID hex string to the system clipboard, triggered by pressing `c` in either the Commit List or the Commit Detail screen. Uses the system-native clipboard API via the `arboard` crate.
_Avoid_: Short hash copy, abbreviated copy

**Worktree Dashboard**:
A screen (`w` from the Commit List) showing every worktree of the repository — the main worktree plus all linked worktrees created with `git worktree add`. The left pane renders a `git log --graph`-style topology of how each branch diverges from the base; the right pane lists one card per worktree.

**Worktree**:
A checkout of the repository at a specific branch. Each has a name (`"main"` or the linked-worktree folder name), a checkout path, a checked-out branch, and a HEAD commit. Summarized as ahead/behind counts relative to the base, a 14-day commit-activity histogram, and a dirty flag.

**Topology Graph**:
The `git log --graph`-style lane diagram in the Worktree Dashboard. Built from a bounded multi-head revision walk (up to 120 commits), assigned columns with a simplified git-like algorithm, and rendered with Unicode box-drawing connectors (`●` `│` `╲` `╱`).

**Base (ahead/behind base)**:
The reference HEAD for ahead/behind counts: the main worktree's HEAD, falling back to the HEAD of the repository tuit was opened at when the main repository is bare.

**Ahead / Behind**:
Commits reachable from a worktree's HEAD but not the base (ahead), and vice versa (behind), computed via the merge base.
