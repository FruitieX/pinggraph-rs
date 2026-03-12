use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    widgets::Widget,
};

use crate::color::ColorScale;
use crate::ping::PingResult;
use std::collections::VecDeque;

/// The filled square character for the graph
const FILLED_SQUARE: &str = "█";
const TIMEOUT_CHAR: &str = "X";
/// Cursor character showing current position
const CURSOR_CHAR: &str = "▌";

/// Graph widget that displays ping results as colored squares
///
/// Rendering behavior:
/// - Content is aligned to the BOTTOM of the screen
/// - New pings fill the current row from left to right
/// - When scrolled, view stays at fixed position (doesn't follow new data)
pub struct Graph<'a> {
    results: &'a VecDeque<PingResult>,
    color_scale: &'a ColorScale,
    /// The row number to show at the bottom of the screen (None = live, follow newest)
    view_end_row: Option<usize>,
    /// Total rows of data (using stable sequence numbers)
    total_rows: usize,
    /// Base sequence number for stable indexing
    result_base_seq: usize,
    /// Whether the graph is paused
    paused: bool,
    /// Whether to hide the cursor
    hide_cursor: bool,
    /// Whether to defer rendering of incomplete rows
    defer_row: bool,
    /// Optional RTT range to highlight (min_rtt, max_rtt, is_timeout)
    highlight_range: Option<(f64, f64, bool)>,
}

impl<'a> Graph<'a> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        results: &'a VecDeque<PingResult>,
        color_scale: &'a ColorScale,
        view_end_row: Option<usize>,
        total_rows: usize,
        result_base_seq: usize,
        paused: bool,
        hide_cursor: bool,
        defer_row: bool,
        highlight_range: Option<(f64, f64, bool)>,
    ) -> Self {
        Self {
            results,
            color_scale,
            view_end_row,
            total_rows,
            result_base_seq,
            paused,
            hide_cursor,
            defer_row,
            highlight_range,
        }
    }

    /// Calculate which result index corresponds to a screen position
    /// Returns None if the position is empty
    pub fn result_at_position(
        results_len: usize,
        result_base_seq: usize,
        width: usize,
        height: usize,
        view_end_row: usize,
        screen_row: usize,
        screen_col: usize,
    ) -> Option<usize> {
        if results_len == 0 || width == 0 || height == 0 {
            return None;
        }

        let total_results = result_base_seq + results_len;
        let total_rows = total_results.div_ceil(width);
        let actual_end = view_end_row.min(total_rows);
        let visible_rows = actual_end.min(height);
        let view_start_row = actual_end.saturating_sub(visible_rows);

        // Calculate empty rows at top
        let empty_rows_at_top = height.saturating_sub(visible_rows);

        if screen_row < empty_rows_at_top {
            return None;
        }

        let data_row = view_start_row + (screen_row - empty_rows_at_top);

        if data_row >= actual_end {
            return None;
        }

        // Calculate the stable sequence index
        let seq_idx = data_row * width + screen_col;

        // Convert to VecDeque index
        if seq_idx >= result_base_seq && seq_idx < total_results {
            Some(seq_idx - result_base_seq)
        } else {
            None
        }
    }
}

impl Widget for Graph<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }

        let width = area.width as usize;
        let height = area.height as usize;
        let result_count = self.results.len();
        let total_results = self.result_base_seq + result_count;

        if result_count == 0 {
            // Draw cursor at start position if not hidden
            if !self.hide_cursor {
                buf.set_string(
                    area.x,
                    area.y + area.height - 1,
                    CURSOR_CHAR,
                    Style::default().fg(Color::White),
                );
            }
            return;
        }

        // Determine which row to show at bottom
        // When defer_row is enabled in live mode, exclude the incomplete last row
        let display_total_rows = if self.defer_row && self.view_end_row.is_none() {
            // Only show complete rows
            total_results / width
        } else {
            self.total_rows
        };

        let view_end = match self.view_end_row {
            Some(row) => row.min(self.total_rows),
            None => display_total_rows, // Live mode (possibly deferred)
        };

        let visible_rows = view_end.min(height);
        let view_start_row = view_end.saturating_sub(visible_rows);

        // Calculate empty rows at top (for bottom alignment)
        let empty_rows_at_top = height.saturating_sub(visible_rows);

        let is_live = self.view_end_row.is_none();

        // Calculate the first row that has data in our buffer
        let first_buffered_row = self.result_base_seq / width;

        // Render results row by row (aligned to bottom)
        for data_row in view_start_row..view_end {
            let screen_row = empty_rows_at_top + (data_row - view_start_row);

            if screen_row >= height {
                break;
            }

            // Skip rows that are before our buffer
            if data_row < first_buffered_row {
                continue;
            }

            for col in 0..width {
                // Calculate stable sequence index
                let seq_idx = data_row * width + col;

                // Skip if before our buffer or after our data
                if seq_idx < self.result_base_seq || seq_idx >= total_results {
                    continue;
                }

                // Convert to VecDeque index
                let vec_idx = seq_idx - self.result_base_seq;
                let result = &self.results[vec_idx];
                let x = area.x + col as u16;
                let y = area.y + screen_row as u16;

                // Check if this sample should be highlighted
                let is_highlighted =
                    if let Some((min_rtt, max_rtt, is_timeout_highlight)) = self.highlight_range {
                        if is_timeout_highlight {
                            // Highlight timeouts
                            result.rtt_ms_f64().is_none()
                        } else if let Some(rtt) = result.rtt_ms_f64() {
                            // Highlight samples within the RTT range
                            rtt >= min_rtt && rtt < max_rtt
                        } else {
                            false
                        }
                    } else {
                        false
                    };

                // Highlight color: bright red for visibility
                let highlight_color = Color::Rgb(255, 50, 50);

                if let Some(rtt) = result.rtt_ms_f64() {
                    let color = if is_highlighted {
                        highlight_color
                    } else {
                        self.color_scale.color_for_rtt_f64(Some(rtt))
                    };
                    buf.set_string(x, y, FILLED_SQUARE, Style::default().fg(color));
                } else {
                    let color = if is_highlighted {
                        highlight_color
                    } else {
                        Color::Indexed(240)
                    };
                    buf.set_string(x, y, TIMEOUT_CHAR, Style::default().fg(color));
                }
            }
        }

        // Draw cursor at current position (unless hidden)
        if !self.hide_cursor && is_live {
            // Calculate cursor position using stable indices
            let cursor_seq = total_results;
            let cursor_row = cursor_seq / width;
            let cursor_col = cursor_seq % width;

            // Only draw if cursor row is visible
            if cursor_row >= view_start_row && cursor_row < view_end {
                let screen_row = empty_rows_at_top + (cursor_row - view_start_row);
                if screen_row < height {
                    let x = area.x + cursor_col as u16;
                    let y = area.y + screen_row as u16;
                    buf.set_string(x, y, CURSOR_CHAR, Style::default().fg(Color::White));
                }
            } else if cursor_row == view_end && cursor_col == 0 {
                // Cursor is at start of next row (just wrapped)
                let screen_row = empty_rows_at_top + visible_rows;
                if screen_row < height {
                    let x = area.x;
                    let y = area.y + screen_row as u16;
                    buf.set_string(x, y, CURSOR_CHAR, Style::default().fg(Color::White));
                }
            }
        }

        // Show indicator when paused or scrolled
        if self.paused || !is_live {
            let indicator = if !is_live {
                // Show "row X of Y" style
                format!(" {}/{} ", view_end, self.total_rows)
            } else {
                " PAUSED ".to_string()
            };
            let x = area.x + area.width.saturating_sub(indicator.len() as u16 + 1);
            let y = area.y;
            buf.set_string(
                x,
                y,
                &indicator,
                Style::default().fg(Color::Yellow).bg(Color::DarkGray),
            );
        }
    }
}
