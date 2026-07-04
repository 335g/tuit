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
}

/// Application-level configuration.
#[derive(Clone, Debug)]
pub struct Config {
    pub theme: String,
    pub colors: Colors,
}

/// Raw deserialization target for `config.toml`.
#[derive(Deserialize)]
struct ConfigFile {
    theme: Option<String>,
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
}

/// Default color palette (Catppuccin Mocha inspired).
fn default_colors() -> Colors {
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
    }
}

/// Resolve the config directory (`~/.config/ratty/`) using the `directories` crate.
fn config_dir() -> PathBuf {
    directories::BaseDirs::new()
        .map(|d| d.config_dir().join("ratty"))
        .unwrap_or_else(|| {
            // Fallback: use $XDG_CONFIG_HOME/ratty or ~/.config/ratty
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
            PathBuf::from(home).join(".config").join("ratty")
        })
}

/// Load configuration and colors.
///
/// 1. Reads `~/.config/ratty/config.toml` (optional).
/// 2. Reads the theme file specified in config (optional).
/// 3. Falls back to hard-coded defaults for anything missing.
pub fn load() -> Result<Config> {
    let cfg_dir = config_dir();
    let cfg_path = cfg_dir.join("config.toml");

    // 1. Read optional config file
    let theme_name = if cfg_path.exists() {
        let content = fs::read_to_string(&cfg_path)
            .with_context(|| format!("Failed to read config file: {}", cfg_path.display()))?;
        let parsed: ConfigFile = toml::from_str(&content)
            .with_context(|| format!("Failed to parse config file: {}", cfg_path.display()))?;
        parsed.theme.unwrap_or_else(|| "default".into())
    } else {
        "default".into()
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
    let defaults = default_colors();
    let colors = merge_colors(defaults, theme_colors);

    Ok(Config {
        theme: theme_name,
        colors,
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
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_colors_loads_when_no_files_exist() {
        // Run in a temp dir with no config files
        let prev = std::env::var("HOME").ok();
        let tmp = std::env::temp_dir().join("ratty-test-no-config");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        std::env::set_var("HOME", &tmp);
        std::env::set_var("XDG_CONFIG_HOME", &tmp.join(".config"));

        let config = load().unwrap();
        assert_eq!(config.theme, "default");
        assert_eq!(config.colors.cursor_bg, "#89b4fa");

        if let Some(h) = prev {
            std::env::set_var("HOME", h);
        }
    }

    #[test]
    fn malformed_toml_returns_error() {
        let prev = std::env::var("HOME").ok();
        let tmp = std::env::temp_dir().join("ratty-test-malformed");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp.join(".config").join("ratty")).unwrap();
        fs::write(
            tmp.join(".config").join("ratty").join("config.toml"),
            "<<< not toml >>>",
        )
        .unwrap();
        std::env::set_var("HOME", &tmp);
        std::env::set_var("XDG_CONFIG_HOME", &tmp.join(".config"));

        let result = load();
        assert!(result.is_err());

        if let Some(h) = prev {
            std::env::set_var("HOME", h);
        }
    }
}
