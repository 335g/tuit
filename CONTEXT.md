# tuit

A TUI git log viewer built with ratatui. Displays a scrollable list of commits and allows selecting one to view its full details.

## Language

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

**Toast**:
A transient non-blocking popup displayed in the bottom-right corner of the screen. Carries a `message: String` and an expiration time. Used for user-facing announcements such as HEAD Change alerts, copy confirmations, and future autocommit notifications.

**Toast Timeout**:
The duration (in milliseconds) that a Toast is displayed before automatically dismissing. Configurable; defaults to 3000 ms.

**Toast Dismissal**:
The act of closing a Toast, either by waiting for its timeout to expire or by clicking the Toast area with the mouse.

**Sample Toast**:
A Toast triggered manually for demonstration or debugging purposes (e.g. by pressing a dedicated key).

**Copy to Clipboard**:
The action of copying a commit's full 40-character OID hex string to the system clipboard, triggered by pressing `c` in either the Commit List or the Commit Detail screen. Uses the system-native clipboard API via the `arboard` crate.
_Avoid_: Short hash copy, abbreviated copy
