Status: ready-for-agent
Type: task

# 02: Config and theme system

## Goal

Implement the config/theme loading layer (`src/config.rs`). The system reads `~/.config/ratty/config.toml` which specifies a `theme` name, then loads the corresponding theme from `~/.config/ratty/themes/<name>.toml`. If neither file exists, use hard-coded default colors.

## Details

### config.toml format

```toml
theme = "catppuccin"
```

### Theme file format

```toml
name = "Catppuccin Mocha"

[colors]
cursor_bg = "#89b4fa"
cursor_fg = "#1e1e2e"
list_bg = "#1e1e2e"
hash = "#a6e3a1"
message = "#cdd6f4"
author = "#f5c2e7"
date = "#6c7086"
detail_bg = "#181825"
detail_border = "#89b4fa"
detail_heading = "#f9e2af"
detail_value = "#cdd6f4"
diff_add = "#a6e3a1"
diff_del = "#f38ba8"
diff_normal = "#bac2de"
```

### Data structure

```rust
pub struct Colors {
    pub cursor_bg: String,
    pub cursor_fg: String,
    pub list_bg: String,
    pub hash: String,
    pub message: String,
    pub author: String,
    pub date: String,
    pub detail_bg: String,
    pub detail_border: String,
    pub detail_heading: String,
    pub detail_value: String,
    pub diff_add: String,
    pub diff_del: String,
    pub diff_normal: String,
}

pub struct Config {
    pub theme: String,
    pub colors: Colors,
}
```

### Behavior

1. Resolve `~/.config/ratty/` via the `directories` crate.
2. Try to read `config.toml` — if absent, use default values.
3. Try to read `themes/<name>.toml` — if absent, use default colors.
4. Return a `Config` struct with all resolved values.

### Default colors (fallback)

A reasonable light-on-dark scheme (e.g., inspired by Catppuccin Mocha as shown above).

## Acceptance

- `config::load()` returns a `Config` with populated `Colors`
- When no config files exist, returns default colors
- When a valid theme file exists, colors reflect that file
- When config.toml references a non-existent theme, falls back to default
- Malformed TOML files produce a clear error message (not a panic)

## Testing note

This module is exercised indirectly through the single end-to-end seam: tests run ratty in a temp directory with controlled `~/.config/ratty/` files and assert on rendered output colors.
