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

### Hunk Integration

**Agent Review Loop**:
The development workflow the Hunk integration serves: a coding agent's changes are auto-committed in meaningful units, the user reviews each commit and writes comments in Hunk, then feeds those comments back to the agent for fixes.

**Hunk**:
An external terminal diff review tool (modem-dev/hunk) that tuit integrates with. Provides multi-file review with inline comments, used as the feedback channel in the Agent Review Loop.

**Range Start**:
A commit marked in the Commit List by pressing `v`, which acts as the base (older bound) or tip (newer bound) for a range review. When a Range Start is set and the user presses `h`, tuit launches `hunk diff <older_oid>..<newer_oid>` covering all commits between the mark and the cursor, instead of `hunk show <oid>`. Visually indicated by a `■` cursor on the marked row and a highlighted background across the range.
_Avoid_: Mark, Range marker

**File Selection**:
An overlay opened by pressing `f` in the Commit List or Commit Detail. Shows a check-list of changed files.
- **Single-commit mode** (no range start): shows files changed in the focused Commit.
- **Range mode** (range start set): shows the **union** of files changed across the range (`older..newer`), computed as `git diff --name-only <older>..<newer>`.
The user can toggle individual files with `Space`, select all with `a`, deselect all with `n`, then press `Enter` to launch Hunk with only the selected files. Default: all files selected (unfiltered launch). Cancelled with `Esc`.
_Avoid_: File picker, Filter files

**Open in Hunk**:
The action of launching Hunk against a Commit (single) or a Range of Commits (range). Triggered by pressing `h` in the Commit List or the Commit Detail screen. May be preceded by File Selection (`f`) to restrict which changed files are forwarded to hunk.
- Without a Range Start: runs `hunk show <oid>` (single Commit) or `hunk show <oid> -- <selected files>` (filtered).
- With a Range Start set in the Commit List: runs `hunk diff <older_oid>..<newer_oid>` (range) or `hunk diff <older_oid>..<newer_oid> -- <selected files>` (filtered range).
- The Range Start is automatically cleared after a range launch. File Selection is consumed after launch.
_Avoid_: Launch hunk, Review in hunk
