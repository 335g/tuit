Status: ready-for-agent
Type: task

# 05: Commit list UI rendering

## Goal

Implement the main commit list screen in `src/ui.rs` using ratatui. This is the primary view the user sees on startup.

## Layout

```
┌─────────────────────────────────────────────────────┐
│  ratty                                          q:quit │
├─────────────────────────────────────────────────────┤
│                                                       │
│   abc1234 Add feature X                        Alice  3d ago │
│  ❯ def5678 Fix bug Y                          Bob    2h ago │
│    abcdead Update README                       Alice  1h ago │
│                                                       │
├─────────────────────────────────────────────────────┤
│  ↑↓ navigate  Enter detail  q quit                    │
└─────────────────────────────────────────────────────┘
```

## Implementation

### Header bar
- Left: "ratty" (styled with app name emphasis)
- Right: "q:quit" (dimmed)
- Use `ratatui::widgets::Paragraph` or inline `Span`s

### Commit list
- Each row: `{❯| } {hash:7} {message} {author:>12} {date:>10}`
- Use `ratatui::widgets::List` with a custom render function or `Paragraph` rows
- The selected row gets `cursor_bg`/`cursor_fg` colors from the theme
- Other rows use the per-field colors (`hash`, `message`, `author`, `date`)
- If message exceeds available width, truncate with `…`

### Footer bar
- Centered keybinding hints: "↑↓ navigate · Enter detail · q quit"
- Dimmed style

### Color application
Map the `Colors` struct to ratatui `Style` values:
```rust
fn field_color(colors: &Colors, field: &str) -> Style {
    // Parse hex string to ratatui::Color
}
```

## Acceptance

- List renders with correct formatting for each row
- Selected row is visually distinct (background highlight)
- Each field (hash, message, author, date) uses its configured color
- Header and footer display correctly
- Truncation works when terminal is too narrow
- Empty commit list shows a placeholder message ("No commits found")
