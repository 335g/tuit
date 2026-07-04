use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use crate::app::{App, Screen};

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
    }
}

/// Loading placeholder screen.
fn render_loading(frame: &mut Frame) {
    let area = frame.area();
    let text = Paragraph::new("Loading commits…")
        .alignment(Alignment::Center)
        .block(Block::default().title(" ratty ").borders(Borders::ALL));
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
        Span::styled(" ratty ", Style::default().bold()),
        Span::styled("  q:quit", Style::default().fg(Color::DarkGray)),
    ]);
    let header = Paragraph::new(header_line);
    frame.render_widget(header, layout[0]);

    // --- Commit list ---
    let list_area = layout[1];
    let list_bg = hex_color(&colors.list_bg);
    let muted_fg = Style::default().fg(Color::DarkGray);

    // Build list rows
    let rows: Vec<Line> = app
        .commits
        .iter()
        .enumerate()
        .map(|(i, commit)| {
            let is_selected = i == app.selected_index;

            let cursor = if is_selected { "❯" } else { " " };

            // Truncate message to fit available width
            let max_msg_width = list_area.width.saturating_sub(35).max(5) as usize;
            let msg = if commit.message.len() > max_msg_width {
                let mut s: String = commit
                    .message
                    .chars()
                    .take(max_msg_width.saturating_sub(1))
                    .collect();
                s.push('…');
                s
            } else {
                commit.message.clone()
            };

            // Author padding
            let author_pad = 12usize.saturating_sub(commit.author.len());
            let author_padded = format!("{}{}", " ".repeat(author_pad), commit.author);

            // Date field (up to 10 chars)
            let date_str = if commit.date.len() > 10 {
                let mut s: String = commit.date.chars().take(9).collect();
                s.push('…');
                s
            } else {
                commit.date.clone()
            };

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
                Line::from(vec![
                    Span::styled(format!(" {}", cursor), base),
                    Span::styled(format!(" {} ", commit.hash), base),
                    Span::styled(format!(" {} ", msg), base),
                    Span::styled(format!(" {} {}", author_padded, date_str), base),
                ])
            } else {
                Line::from(vec![
                    Span::styled(format!(" {}", cursor), muted_fg),
                    Span::styled(
                        format!(" {} ", commit.hash),
                        if grayed_out {
                            muted_fg
                        } else {
                            fg(&colors.hash)
                        },
                    ),
                    Span::styled(
                        format!(" {} ", msg),
                        if grayed_out {
                            muted_fg
                        } else {
                            fg(&colors.message)
                        },
                    ),
                    Span::styled(
                        format!(" {}", author_padded),
                        if grayed_out {
                            muted_fg
                        } else {
                            fg(&colors.author)
                        },
                    ),
                    Span::styled(
                        format!(" {}", date_str),
                        if grayed_out {
                            muted_fg
                        } else {
                            fg(&colors.date)
                        },
                    ),
                ])
            }
        })
        .collect();

    let list_block = Block::default().style(Style::default().bg(list_bg));
    let list = Paragraph::new(Text::from(rows))
        .block(list_block)
        .wrap(Wrap { trim: false });
    frame.render_widget(list, layout[1]);

    // --- Footer ---
    let footer_text = " ↑↓ navigate · Enter detail · q quit ";
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

    // Overlay border
    let border_color = hex_color(&colors.detail_border);
    let bg_color = hex_color(&colors.detail_bg);
    let overlay = Block::default()
        .title(format!(" Commit {} ", commit.hash))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .style(Style::default().bg(bg_color));
    let inner = overlay.inner(overlay_area);
    frame.render_widget(overlay, overlay_area);

    // Layout inside the overlay (metadata, separator, scrollable content, close hint)
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // metadata (author, date)
            Constraint::Length(1), // separator ───────
            Constraint::Min(1),    // scrollable content (subject + body + diff)
            Constraint::Length(1), // close hint
        ])
        .split(inner);

    // --- Metadata line ---
    let heading_style = Style::default()
        .fg(hex_color(&colors.detail_heading))
        .add_modifier(Modifier::BOLD);
    let value_style = Style::default().fg(hex_color(&colors.detail_value));

    let meta_line = Line::from(vec![
        Span::styled(" Author:  ", heading_style),
        Span::styled(&commit.author, value_style),
        Span::raw("    "),
        Span::styled("Date:    ", heading_style),
        Span::styled(&commit.date, value_style),
    ]);
    let meta = Paragraph::new(meta_line).style(Style::default().bg(bg_color));
    frame.render_widget(meta, layout[0]);

    // --- Separator under metadata ---
    let sep_style = Style::default().fg(Color::DarkGray);
    let sep = Paragraph::new(Line::from(Span::styled(
        "─".repeat(layout[1].width.max(1) as usize),
        sep_style,
    )))
    .style(Style::default().bg(bg_color));
    frame.render_widget(sep, layout[1]);

    // --- Scrollable content (subject + body + diff) ---
    let content_area = layout[2];
    let content_width = content_area.width.saturating_sub(2) as usize;

    // Build combined content lines with proper styling
    let mut content_lines: Vec<Line> = Vec::new();
    let dim_style = Style::default().fg(Color::DarkGray);

    // Subject line (bold, always first in scrollable area)
    content_lines.push(Line::from(Span::styled(
        commit.message.clone(),
        value_style.add_modifier(Modifier::BOLD),
    )));

    // Body lines (if any)
    if !commit.body.is_empty() {
        content_lines.push(Line::from(Span::styled(String::new(), value_style)));
        for line in commit.body.lines() {
            content_lines.push(Line::from(Span::styled(
                line.to_string(),
                value_style,
            )));
        }
    }

    // Diff section — drawn separator + colour-coded lines
    if !commit.diff.is_empty() {
        content_lines.push(Line::from(Span::styled(String::new(), dim_style)));
        content_lines.push(Line::from(Span::styled(
            "───── diff ────────────────────────────",
            dim_style,
        )));
        content_lines.push(Line::from(Span::styled(String::new(), dim_style)));

        for line in commit.diff.lines() {
            let style = if line.starts_with('+') {
                fg(&colors.diff_add)
            } else if line.starts_with('-') {
                fg(&colors.diff_del)
            } else {
                fg(&colors.diff_normal)
            };
            let display = if line.len() > content_width {
                let mut s: String = line
                    .chars()
                    .take(content_width.saturating_sub(3))
                    .collect();
                s.push_str(" …");
                s
            } else {
                line.to_string()
            };
            content_lines.push(Line::from(Span::styled(display, style)));
        }
    }

    // Calculate scroll offset
    let content_height = content_area.height as usize;
    app.detail_content_height.set(content_height);
    let total_lines = content_lines.len();
    let max_scroll = total_lines.saturating_sub(content_height);
    let scroll = app.detail_scroll.min(max_scroll);

    let content = Paragraph::new(Text::from(content_lines))
        .style(Style::default().bg(bg_color))
        .scroll((scroll as u16, 0));
    frame.render_widget(content, content_area);

    // --- Close hint with optional scroll indicator ---
    let hint_text = if total_lines > content_height {
        let pct = if max_scroll > 0 {
            (scroll as f64 / max_scroll as f64 * 100.0) as u8
        } else {
            0
        };
        format!(" [Esc] close  ·  ↑↓ scroll  ({pct}%) ")
    } else {
        " [Esc] close ".to_string()
    };

    let hint = Paragraph::new(Line::from(Span::styled(
        hint_text,
        Style::default().fg(Color::DarkGray),
    )))
    .alignment(Alignment::Center)
    .style(Style::default().bg(bg_color));
    frame.render_widget(hint, layout[3]);
}

/// Render the error screen.
pub fn render_error(frame: &mut Frame, msg: &str) {
    let area = frame.area();

    let block = Block::default().title(" ratty ").borders(Borders::ALL);

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
