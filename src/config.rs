use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::Deserialize;

/// Resolved color palette used throughout the UI.
#[derive(Clone, Debug)]
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
    // Conventional-commit type colours
    pub cc_type_feat: String,
    pub cc_type_fix: String,
    pub cc_type_docs: String,
    pub cc_type_style: String,
    pub cc_type_refactor: String,
    pub cc_type_perf: String,
    pub cc_type_test: String,
    pub cc_type_chore: String,
}

/// Application-level configuration.
#[derive(Clone, Debug)]
pub struct Config {
    pub theme: String,
    pub colors: Colors,
    pub poll_interval_ms: u64,
    pub notification_timeout_ms: u64,
}

/// Raw deserialization target for `config.toml`.
#[derive(Deserialize)]
struct ConfigFile {
    theme: Option<String>,
    poll_interval_ms: Option<u64>,
    notification_timeout_ms: Option<u64>,
}

/// Raw deserialization target for a theme file.
#[derive(Deserialize)]
struct ThemeFile {
    #[allow(dead_code)]
    name: Option<String>,
    colors: ThemeColors,
}

#[derive(Deserialize)]
struct ThemeColors {
    cursor_bg: Option<String>,
    cursor_fg: Option<String>,
    list_bg: Option<String>,
    hash: Option<String>,
    message: Option<String>,
    author: Option<String>,
    date: Option<String>,
    detail_bg: Option<String>,
    detail_border: Option<String>,
    detail_heading: Option<String>,
    detail_value: Option<String>,
    diff_add: Option<String>,
    diff_del: Option<String>,
    diff_normal: Option<String>,
    cc_type_feat: Option<String>,
    cc_type_fix: Option<String>,
    cc_type_docs: Option<String>,
    cc_type_style: Option<String>,
    cc_type_refactor: Option<String>,
    cc_type_perf: Option<String>,
    cc_type_test: Option<String>,
    cc_type_chore: Option<String>,
}

/// Default color palette (Catppuccin Mocha inspired).
pub fn default_colors() -> Colors {
    Colors {
        cursor_bg: "#89b4fa".into(),
        cursor_fg: "#1e1e2e".into(),
        list_bg: "#1e1e2e".into(),
        hash: "#a6e3a1".into(),
        message: "#cdd6f4".into(),
        author: "#f5c2e7".into(),
        date: "#6c7086".into(),
        detail_bg: "#181825".into(),
        detail_border: "#89b4fa".into(),
        detail_heading: "#f9e2af".into(),
        detail_value: "#cdd6f4".into(),
        diff_add: "#a6e3a1".into(),
        diff_del: "#f38ba8".into(),
        diff_normal: "#bac2de".into(),
        cc_type_feat: "#89b4fa".into(),     // blue
        cc_type_fix: "#f38ba8".into(),      // red
        cc_type_docs: "#cba6f7".into(),     // mauve
        cc_type_style: "#f5c2e7".into(),    // pink
        cc_type_refactor: "#fab387".into(), // peach
        cc_type_perf: "#f9e2af".into(),     // yellow
        cc_type_test: "#94e2d5".into(),     // teal
        cc_type_chore: "#6c7086".into(),    // gray
    }
}

/// Resolve the config directory (`~/.config/tuit/`) using the `directories` crate.
fn config_dir() -> PathBuf {
    directories::BaseDirs::new()
        .map(|d| d.config_dir().join("tuit"))
        .unwrap_or_else(|| {
            // Fallback: use $XDG_CONFIG_HOME/tuit or ~/.config/tuit
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
            PathBuf::from(home).join(".config").join("tuit")
        })
}

/// Load configuration and colors.
///
/// 1. Reads `~/.config/tuit/config.toml` (optional).
/// 2. Reads the theme file specified in config (optional).
/// 3. Falls back to hard-coded defaults for anything missing.
pub fn load() -> Result<Config> {
    let cfg_dir = config_dir();
    let cfg_path = cfg_dir.join("config.toml");

    // 1. Read optional config file
    let (theme_name, poll_interval_ms, notification_timeout_ms) = if cfg_path.exists() {
        let content = fs::read_to_string(&cfg_path)
            .with_context(|| format!("Failed to read config file: {}", cfg_path.display()))?;
        let parsed: ConfigFile = toml::from_str(&content)
            .with_context(|| format!("Failed to parse config file: {}", cfg_path.display()))?;
        (
            parsed.theme.unwrap_or_else(|| "default".into()),
            parsed.poll_interval_ms.unwrap_or(2000),
            parsed.notification_timeout_ms.unwrap_or(3000),
        )
    } else {
        ("default".into(), 2000, 3000)
    };

    // 2. Load theme file (optional)
    let theme_path = cfg_dir.join("themes").join(format!("{}.toml", theme_name));
    let theme_colors = if theme_path.exists() {
        let content = fs::read_to_string(&theme_path)
            .with_context(|| format!("Failed to read theme file: {}", theme_path.display()))?;
        let parsed: ThemeFile = toml::from_str(&content)
            .with_context(|| format!("Failed to parse theme file: {}", theme_path.display()))?;
        Some(parsed.colors)
    } else {
        None
    };

    // 3. Merge with defaults
    let colors = merge_colors(default_colors(), theme_colors);

    Ok(Config {
        theme: theme_name,
        colors,
        poll_interval_ms,
        notification_timeout_ms,
    })
}

/// Merge optional theme colors over the defaults.
fn merge_colors(default: Colors, theme: Option<ThemeColors>) -> Colors {
    match theme {
        None => default,
        Some(t) => Colors {
            cursor_bg: t.cursor_bg.unwrap_or(default.cursor_bg),
            cursor_fg: t.cursor_fg.unwrap_or(default.cursor_fg),
            list_bg: t.list_bg.unwrap_or(default.list_bg),
            hash: t.hash.unwrap_or(default.hash),
            message: t.message.unwrap_or(default.message),
            author: t.author.unwrap_or(default.author),
            date: t.date.unwrap_or(default.date),
            detail_bg: t.detail_bg.unwrap_or(default.detail_bg),
            detail_border: t.detail_border.unwrap_or(default.detail_border),
            detail_heading: t.detail_heading.unwrap_or(default.detail_heading),
            detail_value: t.detail_value.unwrap_or(default.detail_value),
            diff_add: t.diff_add.unwrap_or(default.diff_add),
            diff_del: t.diff_del.unwrap_or(default.diff_del),
            diff_normal: t.diff_normal.unwrap_or(default.diff_normal),
            cc_type_feat: t.cc_type_feat.unwrap_or(default.cc_type_feat),
            cc_type_fix: t.cc_type_fix.unwrap_or(default.cc_type_fix),
            cc_type_docs: t.cc_type_docs.unwrap_or(default.cc_type_docs),
            cc_type_style: t.cc_type_style.unwrap_or(default.cc_type_style),
            cc_type_refactor: t.cc_type_refactor.unwrap_or(default.cc_type_refactor),
            cc_type_perf: t.cc_type_perf.unwrap_or(default.cc_type_perf),
            cc_type_test: t.cc_type_test.unwrap_or(default.cc_type_test),
            cc_type_chore: t.cc_type_chore.unwrap_or(default.cc_type_chore),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_colors_have_expected_values() {
        let colors = default_colors();
        assert_eq!(colors.cursor_bg, "#89b4fa");
        assert_eq!(colors.hash, "#a6e3a1");
        assert_eq!(colors.message, "#cdd6f4");
        assert_eq!(colors.author, "#f5c2e7");
        assert_eq!(colors.date, "#6c7086");
        assert_eq!(colors.diff_add, "#a6e3a1");
        assert_eq!(colors.diff_del, "#f38ba8");
        assert_eq!(colors.diff_normal, "#bac2de");
        assert_eq!(colors.cc_type_feat, "#89b4fa");
        assert_eq!(colors.cc_type_fix, "#f38ba8");
        assert_eq!(colors.cc_type_docs, "#cba6f7");
        assert_eq!(colors.cc_type_style, "#f5c2e7");
        assert_eq!(colors.cc_type_refactor, "#fab387");
        assert_eq!(colors.cc_type_perf, "#f9e2af");
        assert_eq!(colors.cc_type_test, "#94e2d5");
        assert_eq!(colors.cc_type_chore, "#6c7086");
    }

    #[test]
    fn merge_colors_with_none_returns_default() {
        let defaults = default_colors();
        let merged = merge_colors(default_colors(), None);
        assert_eq!(merged.cursor_bg, defaults.cursor_bg);
        assert_eq!(merged.hash, defaults.hash);
    }

    #[test]
    fn merge_colors_with_theme_overrides_values() {
        let theme = ThemeColors {
            cursor_bg: Some("#ff0000".into()),
            cursor_fg: None,
            list_bg: None,
            hash: Some("#00ff00".into()),
            message: None,
            author: None,
            date: None,
            detail_bg: None,
            detail_border: None,
            detail_heading: None,
            detail_value: None,
            diff_add: None,
            diff_del: None,
            diff_normal: None,
            cc_type_feat: None,
            cc_type_fix: None,
            cc_type_docs: None,
            cc_type_style: None,
            cc_type_refactor: None,
            cc_type_perf: None,
            cc_type_test: None,
            cc_type_chore: None,
        };
        let merged = merge_colors(default_colors(), Some(theme));
        assert_eq!(merged.cursor_bg, "#ff0000");
        assert_eq!(merged.hash, "#00ff00");
        // Unset fields should remain default
        assert_eq!(merged.author, "#f5c2e7");
    }

    #[test]
    fn malformed_toml_content_parse_fails() {
        let result: std::result::Result<ConfigFile, _> = toml::from_str("<<< not toml >>>");
        assert!(result.is_err());
    }
}
