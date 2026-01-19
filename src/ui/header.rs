use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget},
};

use crate::config::Config;
use crate::ui::app::HeaderEditField;

/// Clickable regions in header (start_x, end_x, field_type)
#[derive(Clone, Debug)]
pub struct HeaderClickRegion {
    pub start_x: u16,
    pub end_x: u16,
    pub field: HeaderField,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeaderField {
    Target,
    Interval,
    Scale,
    Colors,
    Settings,
}

/// Header widget showing target and configuration
pub struct Header<'a> {
    config: &'a Config,
    resolved_ip: Option<&'a str>,
    terminal_width: u16,
    selected_field: Option<HeaderEditField>,
}

impl<'a> Header<'a> {
    pub fn new(
        config: &'a Config,
        resolved_ip: Option<&'a str>,
        terminal_width: u16,
        selected_field: Option<HeaderEditField>,
    ) -> Self {
        Self {
            config,
            resolved_ip,
            terminal_width,
            selected_field,
        }
    }

    /// Calculate click regions for header fields
    /// Returns regions relative to content area (inside borders)
    pub fn calculate_click_regions(&self) -> Vec<HeaderClickRegion> {
        let target = match &self.config.host {
            Some(host) => {
                if let Some(ip) = self.resolved_ip {
                    if host != ip {
                        format!("{} ({})", host, ip)
                    } else {
                        host.clone()
                    }
                } else {
                    host.clone()
                }
            }
            None => "not set".to_string(),
        };

        let mode_str = format!("{}", self.config.mode);
        let interval_str = format!("{}ms", self.config.interval);
        let scale_str = format!("{}ms", self.config.scale);
        let colors_str = format!("{}", self.config.colors);

        let mut regions = Vec::new();
        let mut pos: u16 = 1; // Start after border

        // Target: "Target: " + value
        let target_label = "Target: ";
        pos += target_label.len() as u16;
        let target_start = pos;
        pos += target.len() as u16;
        regions.push(HeaderClickRegion {
            start_x: target_start,
            end_x: pos,
            field: HeaderField::Target,
        });
        pos += 3; // " │ "

        // Mode: "Mode: " + value (not clickable)
        pos += "Mode: ".len() as u16;
        pos += mode_str.len() as u16;
        pos += 3; // " │ "

        // Interval: "Interval: " + value
        pos += "Interval: ".len() as u16;
        let interval_start = pos;
        pos += interval_str.len() as u16;
        regions.push(HeaderClickRegion {
            start_x: interval_start,
            end_x: pos,
            field: HeaderField::Interval,
        });
        pos += 3; // " │ "

        // Scale: "Scale: " + value
        pos += "Scale: ".len() as u16;
        let scale_start = pos;
        pos += scale_str.len() as u16;
        regions.push(HeaderClickRegion {
            start_x: scale_start,
            end_x: pos,
            field: HeaderField::Scale,
        });
        pos += 3; // " │ "

        // Colors: "Colors: " + value
        pos += "Colors: ".len() as u16;
        let colors_start = pos;
        pos += colors_str.len() as u16;
        regions.push(HeaderClickRegion {
            start_x: colors_start,
            end_x: pos,
            field: HeaderField::Colors,
        });

        // Settings button is right-aligned
        // Calculate its position from the right edge
        let settings_text = "[s: Settings]";
        let settings_end = self.terminal_width.saturating_sub(2); // 1 for border
        let settings_start = settings_end.saturating_sub(settings_text.len() as u16);
        regions.push(HeaderClickRegion {
            start_x: settings_start,
            end_x: settings_end,
            field: HeaderField::Settings,
        });

        regions
    }
}

impl Widget for Header<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let target = match &self.config.host {
            Some(host) => {
                if let Some(ip) = self.resolved_ip {
                    if host != ip {
                        format!("{} ({})", host, ip)
                    } else {
                        host.clone()
                    }
                } else {
                    host.clone()
                }
            }
            None => "not set".to_string(),
        };

        let mode_str = format!("{}", self.config.mode);
        let interval_str = format!("{}ms", self.config.interval);
        let scale_str = format!("{}ms", self.config.scale);
        let colors_str = format!("{}", self.config.colors);

        // Helper to apply selection highlight
        let highlight = |base_style: Style, field: HeaderEditField| -> Style {
            if self.selected_field == Some(field) {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Rgb(150, 180, 255))
                    .add_modifier(Modifier::BOLD)
            } else {
                base_style
            }
        };

        // Calculate left side content with selection highlighting
        let left_spans = vec![
            Span::styled("Target: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                &target,
                highlight(
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                    HeaderEditField::Target,
                ),
            ),
            Span::raw(" │ "),
            Span::styled("Mode: ", Style::default().fg(Color::DarkGray)),
            Span::styled(&mode_str, Style::default().fg(Color::Yellow)),
            Span::raw(" │ "),
            Span::styled("Interval: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                &interval_str,
                highlight(Style::default().fg(Color::Green), HeaderEditField::Interval),
            ),
            Span::raw(" │ "),
            Span::styled("Scale: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                &scale_str,
                highlight(Style::default().fg(Color::Blue), HeaderEditField::Scale),
            ),
            Span::raw(" │ "),
            Span::styled("Colors: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                &colors_str,
                highlight(Style::default().fg(Color::Magenta), HeaderEditField::Colors),
            ),
        ];

        // Calculate left content width using Line::width() for proper Unicode handling
        let left_line = Line::from(left_spans.clone());
        let left_width = left_line.width();

        // Calculate padding needed for right-alignment
        // Content area width is area.width - 2 (borders)
        let content_width = area.width.saturating_sub(2) as usize;
        let settings_text = "[s: Settings]";
        // Ensure at least 1 space padding before settings button
        let padding_needed = content_width
            .saturating_sub(left_width)
            .saturating_sub(settings_text.len())
            .max(1);

        // Build final line with padding
        let mut spans = left_spans;
        spans.push(Span::raw(" ".repeat(padding_needed)));
        spans.push(Span::styled(
            settings_text,
            Style::default().fg(Color::DarkGray),
        ));

        let line = Line::from(spans);

        let block = Block::default()
            .title(" rttui ")
            .title_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray));

        let paragraph = Paragraph::new(line).block(block);
        paragraph.render(area, buf);
    }
}
