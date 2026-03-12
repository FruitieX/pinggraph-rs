use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget},
};

use crate::color::ColorScale;
use crate::ping::PingStats;

/// Sparkline characters for mini history (8 levels)
const SPARK_CHARS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
const SPARK_TIMEOUT: char = '×';

/// Footer widget showing statistics and recent ping history
pub struct Footer<'a> {
    stats: &'a PingStats,
    recent_rtts: &'a [Option<f64>], // Recent RTT values in ms as f64 (None = timeout)
    color_scale: &'a ColorScale,
    terminal_width: u16, // Terminal width for scaling sparkline
    no_sparkline: bool,
}

impl<'a> Footer<'a> {
    pub fn new(
        stats: &'a PingStats,
        recent_rtts: &'a [Option<f64>],
        color_scale: &'a ColorScale,
        terminal_width: u16,
        no_sparkline: bool,
    ) -> Self {
        Self {
            stats,
            recent_rtts,
            color_scale,
            terminal_width,
            no_sparkline,
        }
    }

    /// Generate sparkline from recent RTTs with given width
    fn sparkline(&self, sparkline_width: usize) -> Vec<Span<'a>> {
        // If we have fewer RTTs than width, pad with empty spaces from the left
        let mut spans = Vec::new();

        let rtt_count = self.recent_rtts.len();
        if rtt_count < sparkline_width {
            // Add empty padding on the left
            let padding = sparkline_width - rtt_count;
            for _ in 0..padding {
                spans.push(Span::styled(" ", Style::default()));
            }
        }

        // Take the last `sparkline_width` RTTs (or all if less)
        let start_idx = rtt_count.saturating_sub(sparkline_width);

        for rtt in &self.recent_rtts[start_idx..] {
            match rtt {
                None => spans.push(Span::styled(
                    SPARK_TIMEOUT.to_string(),
                    Style::default().fg(Color::Indexed(240)),
                )),
                Some(ms) => {
                    // Map RTT to sparkline character (0-7)
                    let ratio = (*ms / self.color_scale.max_rtt as f64).min(1.0);
                    let idx = ((ratio * 7.0).round() as usize).min(7);
                    let color = self.color_scale.color_for_rtt_f64(Some(*ms));
                    spans.push(Span::styled(
                        SPARK_CHARS[idx].to_string(),
                        Style::default().fg(color),
                    ));
                }
            }
        }

        spans
    }
}

impl Widget for Footer<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let width = self.terminal_width as usize;

        let min = self
            .stats
            .min_rtt
            .map(|d| format!("{:.2}", d.as_secs_f64() * 1000.0))
            .unwrap_or_else(|| "-".to_string());
        let avg = self
            .stats
            .avg_rtt()
            .map(|d| format!("{:.2}", d.as_secs_f64() * 1000.0))
            .unwrap_or_else(|| "-".to_string());
        let max = self
            .stats
            .max_rtt
            .map(|d| format!("{:.2}", d.as_secs_f64() * 1000.0))
            .unwrap_or_else(|| "-".to_string());

        let loss_color = if self.stats.loss_percent() > 5.0 {
            Color::Red
        } else if self.stats.loss_percent() > 1.0 {
            Color::Yellow
        } else {
            Color::Green
        };

        // Calculate lengths for different sections
        let sent_rcvd_section = format!(
            "Sent: {} │ Rcvd: {} │ ",
            self.stats.total_sent, self.stats.total_received
        );
        let loss_section = format!(
            "Lost: {} ({:.1}%) │ ",
            self.stats.total_lost,
            self.stats.loss_percent()
        );
        let rtt_section = format!("RTT min/avg/max: {}/{}/{} ms", min, avg, max);
        let recent_label = " │ Recent: ";
        let last_rtt_text = if let Some(last_rtt) = self.recent_rtts.last() {
            match last_rtt {
                Some(ms) => format!(" {:.2}ms", ms),
                None => " timeout".to_string(),
            }
        } else {
            " ---.--ms".to_string()
        };
        let quit_button = "[r: reset] [q: quit]";

        // Calculate total lengths for different display modes
        let full_static_len = sent_rcvd_section.len()
            + loss_section.len()
            + rtt_section.len()
            + recent_label.len()
            + last_rtt_text.len()
            + quit_button.len()
            + 2; // 2 spaces before button
        let no_sent_rcvd_len = loss_section.len()
            + rtt_section.len()
            + recent_label.len()
            + last_rtt_text.len()
            + quit_button.len()
            + 2;
        let no_recent_len = sent_rcvd_section.len()
            + loss_section.len()
            + rtt_section.len()
            + quit_button.len()
            + 2;
        let minimal_len = loss_section.len() + rtt_section.len() + quit_button.len() + 2;

        // Determine what to show based on terminal width
        // Thresholds (with some buffer for sparkline):
        // - Very wide (> full + 20): show everything with expanded sparkline
        // - Wide (> full + 5): show everything with sparkline
        // - Medium (> no_recent): show sent/rcvd, stats, recent text only (no sparkline)
        // - Narrow (> minimal): show loss, RTT stats, quit
        // - Very narrow: show only RTT stats and quit

        let show_sent_rcvd = width > no_sent_rcvd_len + 10;
        let show_recent_section = width > no_recent_len + 10;
        let show_sparkline = !self.no_sparkline && width > full_static_len + 10;

        // Build left-side spans (without sparkline first to calculate remaining space)
        let mut base_spans = Vec::new();

        // Sent/Rcvd section (hide on narrow terminals)
        if show_sent_rcvd {
            base_spans.extend(vec![
                Span::styled("Sent: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("{}", self.stats.total_sent),
                    Style::default().fg(Color::Cyan),
                ),
                Span::raw(" │ "),
                Span::styled("Rcvd: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("{}", self.stats.total_received),
                    Style::default().fg(Color::Green),
                ),
                Span::raw(" │ "),
            ]);
        }

        // Loss section (always show if there's room)
        if width > minimal_len {
            base_spans.extend(vec![
                Span::styled("Lost: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!(
                        "{} ({:.1}%)",
                        self.stats.total_lost,
                        self.stats.loss_percent()
                    ),
                    Style::default().fg(loss_color),
                ),
                Span::raw(" │ "),
            ]);
        }

        // RTT section (always show)
        base_spans.extend(vec![
            Span::styled("RTT min/avg/max: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{}/{}/{} ms", min, avg, max),
                Style::default().fg(Color::White),
            ),
        ]);

        // Build the "Recent: " label and last RTT text spans
        let mut recent_spans = Vec::new();
        let mut last_rtt_spans = Vec::new();

        if show_recent_section {
            recent_spans.push(Span::raw(" │ "));
            recent_spans.push(Span::styled(
                "Recent: ",
                Style::default().fg(Color::DarkGray),
            ));

            // Show the numeric value of the last ping
            if let Some(last_rtt) = self.recent_rtts.last() {
                last_rtt_spans.push(Span::raw(" "));
                match last_rtt {
                    Some(ms) => {
                        last_rtt_spans.push(Span::styled(
                            format!("{:.2}ms", ms),
                            Style::default().fg(Color::White),
                        ));
                    }
                    None => {
                        last_rtt_spans.push(Span::styled(
                            "timeout",
                            Style::default().fg(Color::Indexed(240)),
                        ));
                    }
                }
            } else {
                // No pings yet - show placeholder
                last_rtt_spans.push(Span::raw(" "));
                last_rtt_spans.push(Span::styled(
                    "---.--ms",
                    Style::default().fg(Color::DarkGray),
                ));
            }
        }

        // Calculate widths using Line::width() for proper Unicode handling
        let base_width = Line::from(base_spans.clone()).width();
        let recent_width = Line::from(recent_spans.clone()).width();
        let last_rtt_width = Line::from(last_rtt_spans.clone()).width();
        let right_section_len = quit_button.len() + 1; // button + 1 space before

        // Calculate sparkline width (fill ALL remaining space)
        let content_width = area.width as usize;
        let fixed_width = base_width + recent_width + last_rtt_width + right_section_len;
        let sparkline_width = if show_sparkline && show_recent_section {
            content_width.saturating_sub(fixed_width)
        } else {
            0
        };

        // Build final spans
        let mut spans = base_spans;
        spans.extend(recent_spans);

        // Add sparkline if there's room
        if sparkline_width > 0 {
            spans.extend(self.sparkline(sparkline_width));
        }

        spans.extend(last_rtt_spans);

        spans.push(Span::raw(" "));
        spans.push(Span::styled(
            quit_button,
            Style::default().fg(Color::DarkGray),
        ));

        let line = Line::from(spans);

        let block = Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(Color::DarkGray));

        let paragraph = Paragraph::new(line).block(block);
        paragraph.render(area, buf);
    }
}
