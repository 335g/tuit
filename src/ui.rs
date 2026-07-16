use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};
use unicode_width::UnicodeWidthStr;

use crate::app::{AlertKind, App, Screen};
use crate::config::Colors;

// ── Conventional commit helpers ─────────────────────────────────────

/// Parse a conventional commit subject.
/// Returns `(type, scope, breaking, description)` or `None`.
fn parse_cc(message: &str) -> Option<(&str, Option<&str>, bool, &str)> {
    let colon = message.find(':')?;
    let prefix = message[..colon].trim_end();
    let desc = message[colon + 1..].trim_start();

    let breaking = prefix.ends_with('!');
    let clean = if breaking { &prefix[..prefix.len() - 1] } else { prefix };

    if let Some(open) = clean.find('(') {
        let close = clean.rfind(')')?;
        if close > open {
            let r#type = clean[..open].trim();
            let scope = clean[open + 1..close].trim();
            if !r#type.is_empty() {
                return Some((r#type, Some(scope), breaking, desc));
            }
        }
    }

    let r#type = clean.trim();
    if r#type.is_empty() { None } else { Some((r#type, None, breaking, desc)) }
}

/// Return the configured hex colour for a conventional-commit type.
fn cc_type_color<'a>(colors: &'a Colors, type_name: &str) -> &'a str {
    match type_name {
        "feat" => &colors.cc_type_feat,
        "fix" => &colors.cc_type_fix,
        "docs" => &colors.cc_type_docs,
        "style" => &colors.cc_type_style,
        "refactor" => &colors.cc_type_refactor,
        "perf" => &colors.cc_type_perf,
        "test" => &colors.cc_type_test,
        "chore" => &colors.cc_type_chore,
        // Unknown type: use chore colour so it still looks intentional
        _ => &colors.cc_type_chore,
    }
}

/// Convert a hex colour string (e.g. "#89b4fa") to a ratatui Color.
fn hex_color(hex: &str) -> Color {
    let hex = hex.trim_start_matches('#');
    if hex.len() != 6 {
        return Color::Reset;
    }
    match u32::from_str_radix(hex, 16) {
        Ok(rgb) => Color::Rgb(
            ((rgb >> 16) & 0xFF) as u8,
            ((rgb >> 8) & 0xFF) as u8,
            (rgb & 0xFF) as u8,
        ),
        Err(_) => Color::Reset,
    }
}

/// Helper: create a Style from a foreground hex colour string.
fn fg(hex: &str) -> Style {
    Style::default().fg(hex_color(hex))
}

/// Main render dispatch.
pub fn render(frame: &mut Frame, app: &App) {
    match &app.screen {
        Screen::Loading => render_loading(frame),
        Screen::List => render_commit_list(frame, app, false),
        Screen::Detail => {
            render_commit_list(frame, app, true);
            render_detail_overlay(frame, app);
        }
        Screen::Error(msg) => render_error(frame, msg),
        Screen::Alert(kind) => {
            render_commit_list(frame, app, true);
            render_alert_overlay(frame, app, kind);
        }
    }

    // Transient footer notification (HEAD-change announcement).
    if app.notification.is_some() {
        render_notification(frame, app);
    }

    // Help overlay on top of everything else.
    if app.show_help {
        render_help_overlay(frame, app);
    }
}

/// Loading placeholder screen.
fn render_loading(frame: &mut Frame) {
    let area = frame.area();
    let text = Paragraph::new("Loading commits...")
        .alignment(Alignment::Center)
        .block(Block::default().title(" tuit ").borders(Borders::ALL));
    frame.render_widget(text, area);
}

/// Render the main commit list screen.
pub fn render_commit_list(frame: &mut Frame, app: &App, grayed_out: bool) {
    let colors = &app.colors;
    let area = frame.area();

    // Layout: header, list, footer
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // header
            Constraint::Min(0),    // list
            Constraint::Length(1), // footer
        ])
        .split(area);

    // --- Header ---
    let header_line = Line::from(vec![
        Span::styled(" tuit ", Style::default().bold()),
        Span::styled(
            format!(" on {} ", app.current_branch),
            Style::default().fg(Color::DarkGray),
        ),
    ]);
    let header = Paragraph::new(header_line);
    frame.render_widget(header, layout[0]);

    // --- Commit list ---
    let list_area = layout[1];
    let list_bg = hex_color(&colors.list_bg);
    let muted_fg = Style::default().fg(Color::DarkGray);

    // ── Compute scroll offset to keep selected_index visible ────
    let viewport_height = list_area.height as usize;
    let commits_len = app.commits.len();
    let mut scroll = app.list_scroll.get();

    // If selection drifted above the viewport, bring scroll up.
    if app.selected_index < scroll {
        scroll = app.selected_index;
    }
    // If selection drifted past the bottom edge, push scroll down.
    if viewport_height > 0 && app.selected_index >= scroll + viewport_height {
        scroll = app.selected_index.saturating_add(1).saturating_sub(viewport_height);
    }

    // Clamp: don't let scroll go past the last page of commits.
    let max_scroll = commits_len.saturating_sub(viewport_height);
    if scroll > max_scroll {
        scroll = max_scroll;
    }

    app.list_scroll.set(scroll);
    app.list_content_height.set(viewport_height);

    // Build list rows
    let rows: Vec<Line> = app
        .commits
        .iter()
        .enumerate()
        .map(|(i, commit)| {
            let is_selected = i == app.selected_index;
            let cursor = if is_selected { "❯" } else { " " };
            let w = list_area.width as usize;
            let show_hash = w >= 140;

            // Date field (up to date_str.width() ≤ 9)
            let date_str = if commit.date.len() > 10 {
                let mut s: String = commit.date.chars().take(9).collect();
                s.push('.');
                s
            } else {
                commit.date.clone()
            };

            // Compute exact reserved width so non-message content never wraps.
            // cursor: 2 chars (" " + " " or "❯"),
            // hash:   9 chars (" " + 7-char hash + " ") [only when show_hash],
            // before-msg space, after-msg space, date space + date_str.
            let cursor_part = 2usize;
            let hash_part = if show_hash { 9usize } else { 0 };
            let before_msg = 1usize;
            let after_msg = 1usize;
            let date_part = 1usize + date_str.width(); // " " + date_str
            let reserved = cursor_part + hash_part + before_msg + after_msg + date_part;
            let max_msg_width = w.saturating_sub(reserved).max(5);
            let msg = if commit.message.len() > max_msg_width {
                let mut s: String = commit
                    .message
                    .chars()
                    .take(max_msg_width.saturating_sub(1))
                    .collect();
                s.push('.');
                s
            } else {
                commit.message.clone()
            };

            // ── Build row spans ────────────────────────────────────
            if is_selected {
                let bg = if grayed_out {
                    Color::Gray
                } else {
                    hex_color(&colors.cursor_bg)
                };
                let fg_color = if grayed_out {
                    Color::Black
                } else {
                    hex_color(&colors.cursor_fg)
                };
                let base = Style::default().bg(bg).fg(fg_color);
                let mut spans = vec![
                    Span::styled(format!(" {}", cursor), base),
                ];
                if show_hash {
                    spans.push(Span::styled(format!(" {} ", commit.hash), base));
                }
                // Message with conventional-commit highlighting
                spans.push(Span::styled(" ", base));
                if let Some((cc_type, cc_scope, cc_breaking, cc_desc)) = parse_cc(&msg) {
                    let type_color = if grayed_out {
                        Color::Black
                    } else {
                        hex_color(cc_type_color(&colors, cc_type))
                    };
                    spans.push(Span::styled(
                        cc_type.to_string(),
                        Style::default().bg(bg).fg(type_color).add_modifier(Modifier::BOLD),
                    ));
                    if let Some(scope) = cc_scope {
                        spans.push(Span::styled(format!("({})", scope), base));
                    }
                    if cc_breaking {
                        spans.push(Span::styled("!", base.fg(Color::Red)));
                    }
                    let rest = if cc_desc.is_empty() { String::new() } else { format!(": {}", cc_desc) };
                    spans.push(Span::styled(rest, base));
                } else {
                    spans.push(Span::styled(msg.clone(), base));
                }
                spans.push(Span::styled(" ", base));
                spans.push(Span::styled(format!(" {}", date_str), base));
                Line::from(spans)
            } else {
                let mut spans = vec![
                    Span::styled(format!(" {}", cursor), muted_fg),
                ];
                if show_hash {
                    spans.push(Span::styled(
                        format!(" {} ", commit.hash),
                        if grayed_out { muted_fg } else { fg(&colors.hash) },
                    ));
                }
                // Message with conventional-commit highlighting
                let msg_style = if grayed_out { muted_fg } else { fg(&colors.message) };
                spans.push(Span::styled(" ", msg_style));
                if let Some((cc_type, cc_scope, cc_breaking, cc_desc)) = parse_cc(&msg) {
                    let type_style = if grayed_out {
                        muted_fg
                    } else {
                        fg(cc_type_color(&colors, cc_type)).add_modifier(Modifier::BOLD)
                    };
                    spans.push(Span::styled(cc_type.to_string(), type_style));
                    if let Some(scope) = cc_scope {
                        let scope_style = if grayed_out {
                            muted_fg
                        } else {
                            Style::default().fg(Color::DarkGray)
                        };
                        spans.push(Span::styled(format!("({})", scope), scope_style));
                    }
                    if cc_breaking {
                        let br_style = if grayed_out {
                            muted_fg
                        } else {
                            fg(&colors.diff_del)
                        };
                        spans.push(Span::styled("!", br_style));
                    }
                    let rest = if cc_desc.is_empty() { String::new() } else { format!(": {}", cc_desc) };
                    spans.push(Span::styled(rest, msg_style));
                } else {
                    spans.push(Span::styled(msg.clone(), msg_style));
                }
                spans.push(Span::styled(" ", msg_style));
                spans.push(Span::styled(
                    format!(" {}", date_str),
                    if grayed_out { muted_fg } else { fg(&colors.date) },
                ));
                Line::from(spans)
            }
        })
        .collect();

    let list_block = Block::default().style(Style::default().bg(list_bg));
    let list = Paragraph::new(Text::from(rows))
        .block(list_block)
        .scroll((scroll as u16, 0))
        .wrap(Wrap { trim: false });
    frame.render_widget(list, layout[1]);

    // --- Footer ---
    let footer_text = " ^/k up | v/j down | Ctrl+f/b pg | c copy | r reload | Enter detail | q quit ";
    let footer = Paragraph::new(Line::from(Span::styled(
        footer_text,
        Style::default().fg(Color::DarkGray),
    )))
    .alignment(Alignment::Center);
    frame.render_widget(footer, layout[2]);
}

/// Render the commit detail overlay on top of the list.
pub fn render_detail_overlay(frame: &mut Frame, app: &App) {
    let colors = &app.colors;
    let area = frame.area();

    let commit = match &app.selected_commit {
        Some(c) => c,
        None => return,
    };

    // Overlay: centered block covering ~90% x 80%
    let overlay_w = (area.width as f64 * 0.9) as u16;
    let overlay_h = (area.height as f64 * 0.8) as u16;
    let overlay_x = (area.width - overlay_w) / 2;
    let overlay_y = (area.height - overlay_h) / 3;

    let overlay_area = Rect {
        x: overlay_x,
        y: overlay_y,
        width: overlay_w,
        height: overlay_h,
    };

    // Background dimming
    let dim_block = Block::default().style(Style::default().bg(Color::Black));
    frame.render_widget(dim_block, area);

    // Clear the popup area so the commit list text doesn't show through
    frame.render_widget(Clear, overlay_area);

    // Overlay border — author & date in the title line
    let border_color = hex_color(&colors.detail_border);
    let bg_color = hex_color(&colors.detail_bg);
    let overlay = Block::default()
        .title(format!(
            " {} | {} | {} ",
            commit.hash, commit.author, commit.date,
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .style(Style::default().bg(bg_color));
    let inner = overlay.inner(overlay_area);
    frame.render_widget(overlay, overlay_area);

    // Count commit-message lines, accounting for word-wrap, to size the header area.
    let body_lines: Vec<&str> = commit.body.lines().collect();
    let body_count = body_lines.len();
    let showed_all_body = body_count <= 8;
    let wrap_width = inner.width.max(1) as usize;

    // How many visual rows a line takes when wrapped (uses Unicode display width).
    let wrap_rows = |text: &str| -> u16 {
        let w = wrap_width.max(1);
        let vis = text.width();
        if vis == 0 { 1 } else { vis.div_ceil(w) as u16 }
    };

    let mut msg_height = wrap_rows(&commit.message); // subject line (may wrap)
    if body_count > 0 {
        msg_height += 1; // blank line before body
        for line in body_lines.iter().take(8) {
            msg_height += wrap_rows(line);
        }
        if !showed_all_body {
            msg_height += 1; // "... and N more" indicator
        }
    }

    // Cap at two thirds of the overlay so diff still has room.
    let max_msg = (inner.height.saturating_sub(3) * 2 / 3).max(1);
    msg_height = msg_height.min(max_msg);

    // Layout: commit message (fixed, wraps), separator, diff (scrollable), close hint
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(msg_height),
            Constraint::Length(1),  // separator ───────
            Constraint::Min(1),     // diff (scrollable)
            Constraint::Length(1),  // close hint
        ])
        .split(inner);

    // --- Commit message (fixed header) ---
    let value_style = Style::default().fg(hex_color(&colors.detail_value));

    let mut msg_lines: Vec<Line> = Vec::new();

    // Subject line (bold)
    msg_lines.push(Line::from(Span::styled(
        commit.message.clone(),
        value_style.add_modifier(Modifier::BOLD),
    )));

    // Body lines (capped at 8)
    if body_count > 0 {
        msg_lines.push(Line::from(Span::styled(String::new(), value_style)));
        for line in body_lines.iter().take(8) {
            msg_lines.push(Line::from(Span::styled(
                line.to_string(),
                value_style,
            )));
        }
        if !showed_all_body {
            let dim_style = Style::default().fg(Color::DarkGray);
            msg_lines.push(Line::from(Span::styled(
                format!(" (... and {} more lines)", body_count - 8),
                dim_style,
            )));
        }
    }

    let msg = Paragraph::new(Text::from(msg_lines))
        .wrap(Wrap { trim: false })
        .style(Style::default().bg(bg_color));
    frame.render_widget(msg, layout[0]);

    // --- Separator ---
    let sep_style = Style::default().fg(Color::DarkGray);
    let sep = Paragraph::new(Line::from(Span::styled(
        "-".repeat(layout[1].width.max(1) as usize),
        sep_style,
    )))
    .style(Style::default().bg(bg_color));
    frame.render_widget(sep, layout[1]);

    // --- Diff (scrollable) ---
    let diff_area = layout[2];
    let diff_width = diff_area.width.saturating_sub(2) as usize;

    let mut diff_lines: Vec<Line> = Vec::new();

    for line in commit.diff.lines() {
        let style = if line.starts_with('+') {
            fg(&colors.diff_add)
        } else if line.starts_with('-') {
            fg(&colors.diff_del)
        } else {
            fg(&colors.diff_normal)
        };
        let display = if line.len() > diff_width {
            let mut s: String = line
                .chars()
                .take(diff_width.saturating_sub(3))
                .collect();
            s.push_str(" ...");
            s
        } else {
            line.to_string()
        };
        diff_lines.push(Line::from(Span::styled(display, style)));
    }

    // Calculate scroll offset for diff only
    let diff_height = diff_area.height as usize;
    app.detail_content_height.set(diff_height);
    let total_diff_lines = diff_lines.len();
    let max_scroll = total_diff_lines.saturating_sub(diff_height);
    let scroll = app.detail_scroll.get().min(max_scroll);
    app.detail_scroll.set(scroll); // sync back so page-drift never accumulates

    let diff_para = Paragraph::new(Text::from(diff_lines))
        .style(Style::default().bg(bg_color))
        .scroll((scroll as u16, 0));
    frame.render_widget(diff_para, diff_area);

    // --- Close hint with optional scroll indicator ---
    let hint_text = if total_diff_lines > diff_height {
        let pct = if max_scroll > 0 {
            (scroll as f64 / max_scroll as f64 * 100.0) as u8
        } else {
            0
        };
        format!(" [Esc] close | c copy | ^/v scroll ({pct}%) ")
    } else {
        " [Esc] close | c copy ".to_string()
    };

    let hint = Paragraph::new(Line::from(Span::styled(
        hint_text,
        Style::default().fg(Color::DarkGray),
    )))
    .alignment(Alignment::Center)
    .style(Style::default().bg(bg_color));
    frame.render_widget(hint, layout[3]);
}

/// Render the keybindings help popup overlay.
fn render_help_overlay(frame: &mut Frame, app: &App) {
    let colors = &app.colors;
    let area = frame.area();

    // Darken the background
    let dim_block = Block::default().style(Style::default().bg(Color::Black));
    frame.render_widget(dim_block, area);

    // Popup size
    let popup_w = (area.width as f64 * 0.7).min(52.0).max(40.0) as u16;
    let popup_h = (area.height as f64 * 0.85).min(28.0) as u16;
    let popup_x = (area.width - popup_w) / 2;
    let popup_y = (area.height - popup_h) / 2;

    let popup_area = Rect {
        x: popup_x,
        y: popup_y,
        width: popup_w,
        height: popup_h,
    };

    frame.render_widget(Clear, popup_area);

    let border_color = hex_color(&colors.detail_border);
    let bg_color = hex_color(&colors.detail_bg);
    let overlay = Block::default()
        .title(" Keybindings ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .style(Style::default().bg(bg_color));
    let inner = overlay.inner(popup_area);
    frame.render_widget(overlay, popup_area);

    // Build content lines — context-sensitive: only the current screen's bindings.
    let heading_style = Style::default()
        .fg(hex_color(&colors.detail_heading))
        .add_modifier(Modifier::BOLD);
    let key_style = Style::default().fg(hex_color(&colors.detail_border));
    let desc_style = Style::default().fg(hex_color(&colors.detail_value));
    let dim_style = Style::default().fg(Color::DarkGray);

    let mut lines: Vec<Line> = Vec::new();

    let (section_title, section_bindings): (&str, &[(&str, &str)]) = match &app.screen {
        Screen::List => (
            "Commit List",
            &[
                ("^ / k", "Move cursor up"),
                ("v / j", "Move cursor down"),
                ("Ctrl+f", "Page down"),
                ("Ctrl+b", "Page up"),
                ("Enter", "Show commit details"),
                ("c", "Copy commit hash to clipboard"),
                ("r", "Reload from current branch"),
                ("q", "Quit tuit"),
            ],
        ),
        Screen::Detail => (
            "Commit Detail",
            &[
                ("^ / k", "Scroll up"),
                ("v / j", "Scroll down"),
                ("Ctrl+f", "Scroll down one page"),
                ("Ctrl+b", "Scroll up one page"),
                ("c", "Copy commit hash to clipboard"),
                ("r", "Reload from current branch"),
                ("Esc", "Close detail view"),
            ],
        ),
        Screen::Error(_) => (
            "Error",
            &[
                ("Enter", "Quit"),
            ],
        ),
        Screen::Alert(_) => (
            "Alert",
            &[
                ("Enter / Esc", "Dismiss"),
            ],
        ),
        Screen::Loading => return, // help should never appear during loading
    };

    lines.push(Line::from(Span::styled(
        format!(" {} ", section_title),
        heading_style,
    )));
    lines.push(Line::from(Span::styled(String::new(), dim_style)));

    for (key_str, desc) in section_bindings {
        lines.push(Line::from(vec![
            Span::styled(format!("   {:<12}", key_str), key_style),
            Span::styled(desc.to_string(), desc_style),
        ]));
    }

    // Close hint
    lines.push(Line::from(Span::styled(String::new(), dim_style)));
    lines.push(Line::from(Span::styled(
        " [Esc/?] close ",
        dim_style,
    )));

    let content = Paragraph::new(Text::from(lines))
        .style(Style::default().bg(bg_color));
    frame.render_widget(content, inner);
}

/// Render the commit-deletion alert overlay.
pub fn render_alert_overlay(frame: &mut Frame, app: &App, kind: &AlertKind) {
    let colors = &app.colors;
    let area = frame.area();

    // Background dimming
    let dim_block = Block::default().style(Style::default().bg(Color::Black));
    frame.render_widget(dim_block, area);

    // Popup size
    let popup_w = (area.width as f64 * 0.6).min(50.0).max(36.0) as u16;
    let popup_h = 8;
    let popup_x = (area.width - popup_w) / 2;
    let popup_y = (area.height - popup_h) / 2;

    let popup_area = Rect {
        x: popup_x,
        y: popup_y,
        width: popup_w,
        height: popup_h,
    };

    frame.render_widget(Clear, popup_area);

    let border_color = hex_color(&colors.detail_border);
    let bg_color = hex_color(&colors.detail_bg);
    let overlay = Block::default()
        .title(" ⚠ Commit Deleted ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .style(Style::default().bg(bg_color));
    let inner = overlay.inner(popup_area);
    frame.render_widget(overlay, popup_area);

    let value_style = Style::default().fg(hex_color(&colors.detail_value));
    let dim_style = Style::default().fg(Color::DarkGray);

    let msg = match kind {
        AlertKind::CommitDeleted { oid } => {
            let short = oid.chars().take(7).collect::<String>();
            format!("Commit {} no longer exists in the repository.", short)
        }
    };

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(inner);

    let msg_para = Paragraph::new(Line::from(Span::styled(msg, value_style)))
        .alignment(Alignment::Center);
    frame.render_widget(msg_para, layout[0]);

    let hint = Paragraph::new(Line::from(Span::styled(
        " [Enter / Esc] back to list ",
        dim_style,
    )))
    .alignment(Alignment::Center);
    frame.render_widget(hint, layout[1]);
}

/// Render a transient notification in the footer area.
pub fn render_notification(frame: &mut Frame, app: &App) {
    let colors = &app.colors;
    let area = frame.area();
    let notif = match &app.notification {
        Some(n) => n,
        None => return,
    };

    // Notification bar at the very bottom of the screen
    let bar = Rect {
        x: 0,
        y: area.height.saturating_sub(1),
        width: area.width,
        height: 1,
    };

    let bg_color = hex_color(&colors.detail_border);
    let notice = Paragraph::new(Line::from(Span::styled(
        format!(" {} ", notif.message),
        Style::default().fg(Color::Black).bg(bg_color),
    )));
    frame.render_widget(notice, bar);
}

/// Render the error screen.
pub fn render_error(frame: &mut Frame, msg: &str) {
    let area = frame.area();

    let block = Block::default().title(" tuit ").borders(Borders::ALL);

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Layout for error message
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // icon + spacing
            Constraint::Min(1),    // message
            Constraint::Length(3), // exit hint
        ])
        .split(inner);

    // Error icon
    let icon = Paragraph::new(Line::from(Span::styled(
        "  ⚠  Error  ",
        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
    )))
    .alignment(Alignment::Center);
    frame.render_widget(icon, layout[0]);

    // Error message – no wrap, just centred line.
    let msg_para = Paragraph::new(Line::from(Span::styled(
        msg,
        Style::default().fg(Color::White),
    )))
    .alignment(Alignment::Center);
    frame.render_widget(msg_para, layout[1]);

    // Exit hint
    let hint = Paragraph::new(Line::from(Span::styled(
        "  [Enter] exit  ",
        Style::default().fg(Color::DarkGray),
    )))
    .alignment(Alignment::Center);
    frame.render_widget(hint, layout[2]);
}
