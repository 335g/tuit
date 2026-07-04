Status: ready-for-agent
Type: task

# 06: Commit detail overlay UI

## Goal

Implement the commit detail overlay in `src/ui.rs` — shown when the user presses Enter on a commit.

## Layout

```
┌────────── Commit abc1234 ────────────────────────────┐
│                                                        │
│  Author:  Alice                                        │
│  Date:    3 days ago                                   │
│                                                        │
│  Add feature X                                         │
│                                                        │
│  This is the full commit body.                         │
│  It can span multiple lines...                         │
│                                                        │
│  ─── diff (unified format) ────────────────────────── │
│  diff --git a/src/main.rs b/src/main.rs                │
│  @@ -1,5 +1,8 @@                                       │
│   use std::io;                                         │
│  +use gix;                                             │
│  ...                                                    │
│                                                        │
│  ──────────────────────────────────────────────────── │
│  [Esc] close                                            │
└────────────────────────────────────────────────────────┘
```

## Implementation

### Overlay rendering
- Render a bordered block covering ~90% x 90% of the terminal
- Use `ratatui::layout::Rect` with padding to create the overlay effect
- Title in the border: "Commit {hash}"

### Content sections
1. **Metadata**: Author, Date (using `detail_heading` and `detail_value` colors)
2. **Subject line**: Bold or emphasized
3. **Commit body**: Normal text
4. **Diff section**: Separated by a line marker. Color each diff line:
   - Lines starting with `+` → `diff_add` color
   - Lines starting with `-` → `diff_del` color
   - Context lines → `diff_normal` color
   - `diff --git` / `@@` lines → `diff_normal` color

### Truncation
If the content exceeds the overlay area, truncate and append `…` (scrollable detail is deferred to a future phase).

### Close
- Esc key closes the overlay (handled in main event loop, not in the renderer)
- Render function receives `show_detail: bool` or is called separately

## Acceptance

- Overlay renders on top of a paused list view
- All metadata (author, date, subject, body) is visible
- Diff lines are color-coded (+ green, - red, context neutral)
- Border title shows abbreviated commit hash
- Content fits within the overlay area (truncation if needed)
- [Esc] hint at bottom is legible
