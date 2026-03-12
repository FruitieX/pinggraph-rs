use crate::color::{ColorScale, ColorScheme};
use crate::config::Config;
use crate::ping::{PingResult, PingStats};
use std::collections::VecDeque;

/// Maximum number of recent pings to track for footer sparkline (enough for wide terminals)
const MAX_RECENT_RTT_COUNT: usize = 500;

/// Popup info for clicked ping
#[derive(Clone)]
pub struct PingPopup {
    /// Stable sequence number of the ping result (not VecDeque index)
    pub result_seq: usize,
    pub screen_x: u16,
    pub screen_y: u16,
}

/// Settings menu field being edited
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SettingsField {
    Target,
    Interval,
    Scale,
    ColorScheme,
    HideCursor,
    BufferSize,
    Confirm,
    Cancel,
}

/// Header field that can be edited inline
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum HeaderEditField {
    Target,
    Interval,
    Scale,
    Colors,
}

impl SettingsField {
    pub fn next(self) -> Self {
        match self {
            SettingsField::Target => SettingsField::Interval,
            SettingsField::Interval => SettingsField::Scale,
            SettingsField::Scale => SettingsField::ColorScheme,
            SettingsField::ColorScheme => SettingsField::HideCursor,
            SettingsField::HideCursor => SettingsField::BufferSize,
            SettingsField::BufferSize => SettingsField::Confirm,
            SettingsField::Confirm => SettingsField::Cancel,
            SettingsField::Cancel => SettingsField::Target,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            SettingsField::Target => SettingsField::Cancel,
            SettingsField::Interval => SettingsField::Target,
            SettingsField::Scale => SettingsField::Interval,
            SettingsField::ColorScheme => SettingsField::Scale,
            SettingsField::HideCursor => SettingsField::ColorScheme,
            SettingsField::BufferSize => SettingsField::HideCursor,
            SettingsField::Confirm => SettingsField::BufferSize,
            SettingsField::Cancel => SettingsField::Confirm,
        }
    }

    /// Returns true if this field supports text input
    pub fn is_text_input(self) -> bool {
        matches!(
            self,
            SettingsField::Target
                | SettingsField::Interval
                | SettingsField::Scale
                | SettingsField::BufferSize
        )
    }

    /// Returns true if this is a button field
    pub fn is_button(self) -> bool {
        matches!(self, SettingsField::Confirm | SettingsField::Cancel)
    }
}

/// Application state
pub struct App {
    pub config: Config,
    pub color_scale: ColorScale,
    pub stats: PingStats,
    pub results: VecDeque<PingResult>,
    /// Maximum history size (calculated from buffer_mb)
    pub max_history: usize,
    /// Base sequence number - total results ever recorded minus current buffer size
    /// Used for stable row calculations when ring buffer wraps
    pub result_base_seq: usize,
    pub should_quit: bool,
    /// Recent RTT values for footer sparkline (ms as f64, None = timeout)
    pub recent_rtts: VecDeque<Option<f64>>,
    /// Whether to defer rendering of incomplete rows
    pub defer_row: bool,
    /// Whether the display is paused (not recording new pings)
    pub paused: bool,
    /// The row index we're viewing at the BOTTOM of the screen (None = live/follow mode)
    /// When Some(row), we're showing rows ending at `row`, and new data won't shift the view
    pub view_end_row: Option<usize>,
    /// Currently displayed popup (if any)
    pub popup: Option<PingPopup>,
    /// Graph area dimensions for mouse calculations
    pub graph_area: Option<(u16, u16, u16, u16)>, // x, y, width, height
    /// Header area dimensions for mouse calculations
    pub header_area: Option<(u16, u16, u16, u16)>, // x, y, width, height
    /// Footer area dimensions for mouse calculations
    pub footer_area: Option<(u16, u16, u16, u16)>, // x, y, width, height
    /// Whether settings menu is open
    pub settings_open: bool,
    /// Currently selected settings field
    pub settings_field: SettingsField,
    /// Temporary target host being edited
    pub settings_target: String,
    /// Temporary interval value being edited
    pub settings_interval: u64,
    /// Temporary scale value being edited
    pub settings_scale: u64,
    /// Temporary color scheme being edited
    pub settings_colors: ColorScheme,
    /// Text input buffer for typing values
    pub settings_input_buffer: String,
    /// Cursor position within input buffer
    pub settings_input_cursor: usize,
    /// Whether we're in text input mode
    pub settings_input_active: bool,
    /// Whether the entire input is selected (for select-all behavior)
    pub settings_input_selected: bool,
    /// Original values when settings was opened (for cancel)
    pub settings_original_scale: u64,
    /// Original color scheme when settings was opened (for cancel)
    pub settings_original_colors: ColorScheme,
    /// Original hide cursor when settings was opened (for cancel)
    pub settings_original_hide_cursor: bool,
    /// Temporary hide cursor value being edited
    pub settings_hide_cursor: bool,
    /// Temporary buffer size being edited (in MB)
    pub settings_buffer_mb: u64,
    /// Whether pinger needs to be restarted (target or interval changed)
    pub needs_pinger_restart: bool,
    /// New target for pinger restart (if changed)
    pub new_target: Option<String>,
    /// New interval for pinger restart (if changed)
    pub new_interval: Option<u64>,
    /// Inline edit popup for header fields
    pub inline_edit: Option<HeaderEditField>,
    /// Inline edit popup position (x, y)
    pub inline_edit_pos: (u16, u16),
    /// Inline edit input buffer
    pub inline_edit_buffer: String,
    /// Inline edit cursor position
    pub inline_edit_cursor: usize,
    /// Inline edit text selected
    pub inline_edit_selected: bool,
    /// Original value before inline edit (for cancel)
    pub inline_edit_original: String,
    /// Whether inline edit is in text input mode (vs navigation mode)
    pub inline_edit_input_active: bool,
    /// Inline edit confirm button area (x, y, width)
    pub inline_edit_confirm_area: Option<(u16, u16, u16)>,
    /// Whether confirm button is focused in inline edit (false = input focused)
    pub inline_edit_confirm_focused: bool,
    /// Currently selected header field for tab navigation (None = no selection)
    pub header_selected: Option<HeaderEditField>,
    /// Whether quit confirmation dialog is shown
    pub quit_confirm: bool,
    /// Which button is focused in quit dialog (false = Yes, true = No)
    pub quit_confirm_no_focused: bool,
    /// Quit dialog Yes button area (x, y, width)
    pub quit_confirm_yes_area: Option<(u16, u16, u16)>,
    /// Quit dialog No button area (x, y, width)
    pub quit_confirm_no_area: Option<(u16, u16, u16)>,
    /// Legend area dimensions for mouse calculations
    pub legend_area: Option<(u16, u16, u16, u16)>, // x, y, width, height
    /// Currently highlighted RTT range from legend hover (min_rtt, max_rtt, is_timeout)
    /// When Some, graph samples within this range will be highlighted
    pub highlight_rtt_range: Option<(f64, f64, bool)>,
    /// Whether we were in live mode before the popup was shown (to restore when popup closes)
    pub popup_was_live: bool,
}

impl App {
    pub fn new(config: Config) -> Self {
        let color_scale = ColorScale::new(config.scale, config.colors);
        let settings_interval = config.interval;
        let settings_scale = config.scale;
        let settings_colors = config.colors;
        let settings_target = config.host.clone().unwrap_or_default();
        let settings_hide_cursor = config.hide_cursor;
        let settings_buffer_mb = config.buffer_mb;
        let defer_row = config.defer_row;
        let max_history = config.max_history();
        Self {
            max_history,
            result_base_seq: 0,
            config,
            color_scale,
            stats: PingStats::new(),
            results: VecDeque::with_capacity(max_history.min(100000)),
            should_quit: false,
            recent_rtts: VecDeque::with_capacity(MAX_RECENT_RTT_COUNT),
            defer_row,
            paused: false,
            view_end_row: None, // None = live mode (follow newest)
            popup: None,
            graph_area: None,
            header_area: None,
            footer_area: None,
            settings_open: false,
            settings_field: SettingsField::Target,
            settings_target,
            settings_interval,
            settings_scale,
            settings_colors,
            settings_input_buffer: String::new(),
            settings_input_cursor: 0,
            settings_input_active: false,
            settings_input_selected: false,
            settings_original_scale: settings_scale,
            settings_original_colors: settings_colors,
            settings_original_hide_cursor: settings_hide_cursor,
            settings_hide_cursor,
            settings_buffer_mb,
            needs_pinger_restart: false,
            new_target: None,
            new_interval: None,
            inline_edit: None,
            inline_edit_pos: (0, 0),
            inline_edit_buffer: String::new(),
            inline_edit_cursor: 0,
            inline_edit_selected: false,
            inline_edit_original: String::new(),
            inline_edit_input_active: false,
            inline_edit_confirm_area: None,
            inline_edit_confirm_focused: false,
            header_selected: None,
            quit_confirm: false,
            quit_confirm_no_focused: false,
            quit_confirm_yes_area: None,
            quit_confirm_no_area: None,
            legend_area: None,
            highlight_rtt_range: None,
            popup_was_live: false,
        }
    }

    pub fn record_result(&mut self, result: PingResult) {
        self.stats.record(&result);

        // Track recent RTT for sparkline
        let rtt_ms = result.rtt_ms_f64();
        self.recent_rtts.push_back(rtt_ms);
        while self.recent_rtts.len() > MAX_RECENT_RTT_COUNT {
            self.recent_rtts.pop_front();
        }

        self.results.push_back(result);

        // Keep history bounded to max_history
        while self.results.len() > self.max_history {
            self.results.pop_front();
            self.result_base_seq += 1;
        }
    }

    /// Get recent RTTs as a slice for the footer
    pub fn recent_rtts_slice(&self) -> Vec<Option<f64>> {
        self.recent_rtts.iter().cloned().collect()
    }

    /// Clear all stats, results, and history (used when target changes)
    pub fn clear_all_data(&mut self) {
        self.stats = PingStats::new();
        self.results.clear();
        self.recent_rtts.clear();
        self.result_base_seq = 0;
        self.view_end_row = None;
        self.popup = None;
    }

    /// Calculate current total rows of data (using stable sequence numbers)
    pub fn total_rows(&self, width: usize) -> usize {
        if width == 0 || self.results.is_empty() {
            return 0;
        }
        // Use base_seq + results.len() for stable row calculation
        let total_results = self.result_base_seq + self.results.len();
        total_results.div_ceil(width)
    }

    /// Get the current view end row (for display purposes)
    #[allow(dead_code)]
    pub fn current_view_end_row(&self, width: usize) -> usize {
        match self.view_end_row {
            Some(row) => row,
            None => self.total_rows(width), // Live mode: show latest
        }
    }

    pub fn toggle_pause(&mut self) {
        self.paused = !self.paused;
        if !self.paused {
            // Resume: jump to live view
            self.view_end_row = None;
        }
    }

    pub fn scroll_up(&mut self, rows: usize) {
        if let Some((_, _, width, _)) = self.graph_area {
            let width = width as usize;
            let total_rows = self.total_rows(width);

            if total_rows == 0 {
                return;
            }

            // Get current view end
            let current_end = self.view_end_row.unwrap_or(total_rows);

            // Scroll up means showing older data (lower row numbers)
            // Allow scrolling all the way to row 1 (view_end_row = 1) so only first row is visible
            let new_end = current_end.saturating_sub(rows).max(1);

            self.view_end_row = Some(new_end);
            // Note: scrolling doesn't pause - pings keep collecting, just view is locked
        }
    }

    pub fn scroll_down(&mut self, rows: usize) {
        if let Some((_, _, width, _)) = self.graph_area {
            let width = width as usize;
            let total_rows = self.total_rows(width);

            if let Some(current_end) = self.view_end_row {
                let new_end = current_end + rows;

                if new_end >= total_rows {
                    // Reached the end, switch to live mode
                    self.view_end_row = None;
                } else {
                    self.view_end_row = Some(new_end);
                }
            }
            // If already in live mode, do nothing
        }
    }

    pub fn jump_to_live(&mut self) {
        self.view_end_row = None;
        self.paused = false;
    }

    /// Check if we're in live mode (following newest data)
    #[allow(dead_code)]
    pub fn is_live(&self) -> bool {
        self.view_end_row.is_none()
    }

    /// Get the PingResult at a given index if it exists
    #[allow(dead_code)]
    pub fn get_result(&self, idx: usize) -> Option<&PingResult> {
        self.results.get(idx)
    }

    pub fn quit(&mut self) {
        self.should_quit = true;
    }

    /// Open settings menu (unconditional)
    pub fn open_settings(&mut self) {
        self.settings_open = true;
        // Always start with the first field selected
        self.settings_field = SettingsField::Target;
        // Initialize with current active values (not startup values)
        self.settings_target = self.config.host.clone().unwrap_or_default();
        self.settings_interval = self.config.interval;
        self.settings_scale = self.color_scale.max_rtt;
        self.settings_colors = self.color_scale.scheme;
        self.settings_hide_cursor = self.config.hide_cursor;
        self.settings_input_buffer.clear();
        self.settings_input_cursor = 0;
        self.settings_input_active = false;
        self.settings_input_selected = false;
        // Store originals for cancel
        self.settings_original_scale = self.color_scale.max_rtt;
        self.settings_original_colors = self.color_scale.scheme;
        self.settings_original_hide_cursor = self.config.hide_cursor;
    }

    /// Toggle settings menu
    pub fn toggle_settings(&mut self) {
        if self.settings_open {
            self.settings_open = false;
        } else {
            self.open_settings();
        }
    }

    /// Cancel settings and restore original values
    pub fn cancel_settings(&mut self) {
        // Restore original values
        self.color_scale =
            ColorScale::new(self.settings_original_scale, self.settings_original_colors);
        self.config.hide_cursor = self.settings_original_hide_cursor;
        self.settings_open = false;
        self.settings_input_active = false;
    }

    /// Apply settings changes
    pub fn apply_settings(&mut self) {
        // Check if target changed
        let current_target = self.config.host.clone().unwrap_or_default();
        let target_changed =
            !self.settings_target.is_empty() && self.settings_target != current_target;

        // Check if interval changed
        let interval_changed = self.settings_interval != self.config.interval;

        // Apply target
        if !self.settings_target.is_empty() {
            self.config.host = Some(self.settings_target.clone());
        }
        // Apply interval
        self.config.interval = self.settings_interval;
        // Apply scale and colors
        self.config.scale = self.settings_scale;
        self.config.colors = self.settings_colors;
        self.color_scale = ColorScale::new(self.settings_scale, self.settings_colors);
        // Apply hide cursor
        self.config.hide_cursor = self.settings_hide_cursor;
        // Apply buffer size
        self.config.buffer_mb = self.settings_buffer_mb;
        self.max_history = self.config.max_history();

        // Signal pinger restart if target or interval changed
        if target_changed || interval_changed {
            self.needs_pinger_restart = true;
            if target_changed {
                self.new_target = Some(self.settings_target.clone());
            }
            if interval_changed {
                self.new_interval = Some(self.settings_interval);
            }
        }
    }

    /// Navigate to next settings field
    pub fn settings_next_field(&mut self) {
        self.settings_field = self.settings_field.next();
    }

    /// Navigate to previous settings field
    pub fn settings_prev_field(&mut self) {
        self.settings_field = self.settings_field.prev();
    }

    /// Increase current settings value
    pub fn settings_increase(&mut self) {
        match self.settings_field {
            SettingsField::Target | SettingsField::Confirm | SettingsField::Cancel => {}
            SettingsField::Interval => {
                self.settings_interval = self.settings_interval.saturating_add(1).min(100000);
            }
            SettingsField::Scale => {
                self.settings_scale = self.settings_scale.saturating_add(1).min(100000);
                // Apply immediately
                self.color_scale = ColorScale::new(self.settings_scale, self.settings_colors);
            }
            SettingsField::ColorScheme => {
                self.settings_colors = self.settings_colors.next();
                // Apply immediately
                self.color_scale = ColorScale::new(self.settings_scale, self.settings_colors);
            }
            SettingsField::HideCursor => {
                self.settings_hide_cursor = !self.settings_hide_cursor;
                // Apply immediately for live preview
                self.config.hide_cursor = self.settings_hide_cursor;
            }
            SettingsField::BufferSize => {
                self.settings_buffer_mb = self.settings_buffer_mb.saturating_add(1).min(1000);
            }
        }
    }

    /// Decrease current settings value
    pub fn settings_decrease(&mut self) {
        match self.settings_field {
            SettingsField::Target | SettingsField::Confirm | SettingsField::Cancel => {}
            SettingsField::Interval => {
                self.settings_interval = self.settings_interval.saturating_sub(1).max(1);
            }
            SettingsField::Scale => {
                self.settings_scale = self.settings_scale.saturating_sub(1).max(1);
                // Apply immediately
                self.color_scale = ColorScale::new(self.settings_scale, self.settings_colors);
            }
            SettingsField::ColorScheme => {
                self.settings_colors = self.settings_colors.prev();
                // Apply immediately
                self.color_scale = ColorScale::new(self.settings_scale, self.settings_colors);
            }
            SettingsField::HideCursor => {
                self.settings_hide_cursor = !self.settings_hide_cursor;
                // Apply immediately for live preview
                self.config.hide_cursor = self.settings_hide_cursor;
            }
            SettingsField::BufferSize => {
                self.settings_buffer_mb = self.settings_buffer_mb.saturating_sub(1).max(1);
            }
        }
    }

    /// Start text input mode for current field
    pub fn settings_start_input(&mut self) {
        if self.settings_field.is_text_input() {
            self.settings_input_active = true;
            self.settings_input_selected = true; // Select all on entry
            self.settings_input_buffer = match self.settings_field {
                SettingsField::Target => self.settings_target.clone(),
                SettingsField::Interval => self.settings_interval.to_string(),
                SettingsField::Scale => self.settings_scale.to_string(),
                SettingsField::BufferSize => self.settings_buffer_mb.to_string(),
                SettingsField::ColorScheme
                | SettingsField::HideCursor
                | SettingsField::Confirm
                | SettingsField::Cancel => String::new(),
            };
            self.settings_input_cursor = self.settings_input_buffer.len();
        }
    }

    /// Handle character input in text mode
    pub fn settings_input_char(&mut self, c: char) {
        if self.settings_input_active {
            // If text is selected, clear it and start fresh
            if self.settings_input_selected {
                self.settings_input_buffer.clear();
                self.settings_input_cursor = 0;
                self.settings_input_selected = false;
            }

            match self.settings_field {
                SettingsField::Target => {
                    self.settings_input_buffer
                        .insert(self.settings_input_cursor, c);
                    self.settings_input_cursor += 1;
                    self.settings_target = self.settings_input_buffer.clone();
                }
                SettingsField::Interval | SettingsField::Scale | SettingsField::BufferSize => {
                    if c.is_ascii_digit() {
                        self.settings_input_buffer
                            .insert(self.settings_input_cursor, c);
                        self.settings_input_cursor += 1;
                        if let Ok(val) = self.settings_input_buffer.parse::<u64>() {
                            let clamped = val.clamp(1, 100000);
                            match self.settings_field {
                                SettingsField::Interval => self.settings_interval = clamped,
                                SettingsField::Scale => {
                                    self.settings_scale = clamped;
                                    self.color_scale =
                                        ColorScale::new(self.settings_scale, self.settings_colors);
                                }
                                SettingsField::BufferSize => {
                                    self.settings_buffer_mb = clamped;
                                }
                                _ => {}
                            }
                        }
                    }
                }
                SettingsField::ColorScheme
                | SettingsField::HideCursor
                | SettingsField::Confirm
                | SettingsField::Cancel => {}
            }
        }
    }

    /// Handle backspace in text mode
    pub fn settings_input_backspace(&mut self) {
        if self.settings_input_active {
            // If text is selected, clear all
            if self.settings_input_selected {
                self.settings_input_buffer.clear();
                self.settings_input_cursor = 0;
                self.settings_input_selected = false;
            } else if self.settings_input_cursor > 0 {
                self.settings_input_cursor -= 1;
                self.settings_input_buffer
                    .remove(self.settings_input_cursor);
            }

            match self.settings_field {
                SettingsField::Target => {
                    self.settings_target = self.settings_input_buffer.clone();
                }
                SettingsField::Interval => {
                    self.settings_interval = self.settings_input_buffer.parse().unwrap_or(1).max(1);
                }
                SettingsField::Scale => {
                    self.settings_scale = self.settings_input_buffer.parse().unwrap_or(1).max(1);
                    self.color_scale = ColorScale::new(self.settings_scale, self.settings_colors);
                }
                SettingsField::BufferSize => {
                    self.settings_buffer_mb =
                        self.settings_input_buffer.parse().unwrap_or(1).max(1);
                }
                SettingsField::ColorScheme
                | SettingsField::HideCursor
                | SettingsField::Confirm
                | SettingsField::Cancel => {}
            }
        }
    }

    /// Move cursor left in text input mode
    pub fn settings_input_left(&mut self) {
        if self.settings_input_active {
            self.settings_input_selected = false;
            if self.settings_input_cursor > 0 {
                self.settings_input_cursor -= 1;
            }
        }
    }

    /// Move cursor right in text input mode
    pub fn settings_input_right(&mut self) {
        if self.settings_input_active {
            self.settings_input_selected = false;
            if self.settings_input_cursor < self.settings_input_buffer.len() {
                self.settings_input_cursor += 1;
            }
        }
    }

    /// Confirm text input
    pub fn settings_confirm_input(&mut self) {
        self.settings_input_active = false;
        self.settings_input_selected = false;
    }

    /// Handle mouse click in settings menu
    /// Returns true if click was handled (inside settings area)
    pub fn settings_handle_click(
        &mut self,
        screen_x: u16,
        screen_y: u16,
        area_width: u16,
        area_height: u16,
    ) -> bool {
        // Calculate settings menu position (same as in SettingsMenu render)
        let menu_width = 65u16.min(area_width.saturating_sub(4));
        let menu_height = 19u16.min(area_height.saturating_sub(4));
        let menu_x = (area_width.saturating_sub(menu_width)) / 2;
        let menu_y = (area_height.saturating_sub(menu_height)) / 2;

        // Check if click is inside menu
        if screen_x < menu_x
            || screen_x >= menu_x + menu_width
            || screen_y < menu_y
            || screen_y >= menu_y + menu_height
        {
            return false;
        }

        // Convert to relative coordinates within menu (accounting for border)
        let rel_x = screen_x.saturating_sub(menu_x + 1); // +1 for border
        let rel_y = screen_y.saturating_sub(menu_y + 1); // +1 for border

        // Map y coordinate to fields (based on line numbers in render)
        // Line 0: empty
        // Line 1: Target
        // Line 2: empty
        // Line 3: Interval
        // Line 4: empty
        // Line 5: Scale
        // Line 6: empty
        // Line 7: ColorScheme
        // Line 8: empty
        // Line 9: HideCursor
        // Menu lines (relative y):
        // Line 0: empty
        // Line 1: Target
        // Line 2: empty
        // Line 3: Interval
        // Line 4: empty
        // Line 5: Scale
        // Line 6: empty
        // Line 7: ColorScheme
        // Line 8: empty
        // Line 9: HideCursor
        // Line 10: empty
        // Line 11: BufferSize
        // Line 12: empty
        // Line 13: Buttons

        let clicked_field = match rel_y {
            1 => Some(SettingsField::Target),
            3 => Some(SettingsField::Interval),
            5 => Some(SettingsField::Scale),
            7 => Some(SettingsField::ColorScheme),
            9 => Some(SettingsField::HideCursor),
            11 => Some(SettingsField::BufferSize),
            13 => {
                // Buttons row - check x position
                // "                    " (20 spaces) + " Confirm " (9) + "    " (4) + " Cancel " (8)
                // Confirm: x 20-28, Cancel: x 33-40
                if (20..29).contains(&rel_x) {
                    Some(SettingsField::Confirm)
                } else if (33..41).contains(&rel_x) {
                    Some(SettingsField::Cancel)
                } else {
                    None
                }
            }
            _ => None,
        };

        if let Some(field) = clicked_field {
            // If clicking on currently selected field
            if field == self.settings_field {
                // Activate based on field type
                match field {
                    SettingsField::Target
                    | SettingsField::Interval
                    | SettingsField::Scale
                    | SettingsField::BufferSize => {
                        if !self.settings_input_active {
                            self.settings_start_input();
                        }
                    }
                    SettingsField::ColorScheme => {
                        self.settings_increase();
                    }
                    SettingsField::HideCursor => {
                        self.settings_increase();
                    }
                    SettingsField::Confirm => {
                        self.apply_settings();
                        self.settings_open = false;
                    }
                    SettingsField::Cancel => {
                        self.cancel_settings();
                    }
                }
            } else {
                // Select the field
                self.settings_input_active = false;
                self.settings_field = field;
            }
        }

        true
    }

    /// Start inline edit for a header field
    pub fn start_inline_edit(&mut self, field: HeaderEditField, x: u16, y: u16) {
        self.inline_edit = Some(field);
        self.inline_edit_pos = (x, y);
        // Start in navigation mode (not text input mode) - like settings menu
        self.inline_edit_input_active = false;
        // Don't select initially - just focus, no selection
        self.inline_edit_selected = false;
        self.inline_edit_buffer = match field {
            HeaderEditField::Target => self.config.host.clone().unwrap_or_default(),
            HeaderEditField::Interval => self.config.interval.to_string(),
            HeaderEditField::Scale => self.color_scale.max_rtt.to_string(),
            HeaderEditField::Colors => format!("{}", self.color_scale.scheme),
        };
        self.inline_edit_original = self.inline_edit_buffer.clone();
        // For Colors, cursor position is not used
        self.inline_edit_cursor = if field == HeaderEditField::Colors {
            0
        } else {
            self.inline_edit_buffer.len()
        };
        // Clear header selection when opening inline edit
        self.header_selected = None;
        // Start with input focused (not confirm button)
        self.inline_edit_confirm_focused = false;
    }

    /// Activate text input mode for inline edit
    pub fn inline_edit_activate_input(&mut self) {
        if let Some(field) = self.inline_edit {
            // Don't activate input mode for Colors (it's an enum selector)
            if field != HeaderEditField::Colors {
                self.inline_edit_input_active = true;
                self.inline_edit_selected = false;
                self.inline_edit_cursor = self.inline_edit_buffer.len();
            }
        }
    }

    /// Cancel inline edit
    pub fn cancel_inline_edit(&mut self) {
        if let Some(field) = self.inline_edit {
            // Restore original value for scale/colors (they have live preview)
            match field {
                HeaderEditField::Scale => {
                    if let Ok(val) = self.inline_edit_original.parse::<u64>() {
                        self.color_scale = ColorScale::new(val.max(1), self.color_scale.scheme);
                    }
                }
                HeaderEditField::Colors => {
                    // Find original color scheme by cycling through
                    let mut scheme = ColorScheme::default();
                    for _ in 0..10 {
                        if format!("{}", scheme) == self.inline_edit_original {
                            self.color_scale = ColorScale::new(self.color_scale.max_rtt, scheme);
                            break;
                        }
                        scheme = scheme.next();
                    }
                }
                _ => {}
            }
        }
        self.inline_edit = None;
        self.inline_edit_input_active = false;
    }

    /// Apply inline edit
    pub fn apply_inline_edit(&mut self) {
        if let Some(field) = self.inline_edit {
            match field {
                HeaderEditField::Target => {
                    if !self.inline_edit_buffer.is_empty()
                        && self.inline_edit_buffer != self.config.host.clone().unwrap_or_default()
                    {
                        self.config.host = Some(self.inline_edit_buffer.clone());
                        self.new_target = Some(self.inline_edit_buffer.clone());
                        self.needs_pinger_restart = true;
                    }
                }
                HeaderEditField::Interval => {
                    if let Ok(val) = self.inline_edit_buffer.parse::<u64>() {
                        let clamped = val.clamp(1, 100000);
                        if clamped != self.config.interval {
                            self.config.interval = clamped;
                            self.new_interval = Some(clamped);
                            self.needs_pinger_restart = true;
                        }
                    }
                }
                HeaderEditField::Scale => {
                    // Already applied via live preview
                    self.config.scale = self.color_scale.max_rtt;
                }
                HeaderEditField::Colors => {
                    // Already applied via live preview
                    self.config.colors = self.color_scale.scheme;
                }
            }
        }
        self.inline_edit = None;
    }

    /// Handle character input in inline edit
    pub fn inline_edit_char(&mut self, c: char) {
        if self.inline_edit.is_none() {
            return;
        }
        let field = self.inline_edit.unwrap();

        if self.inline_edit_selected {
            self.inline_edit_buffer.clear();
            self.inline_edit_cursor = 0;
            self.inline_edit_selected = false;
        }

        match field {
            HeaderEditField::Target => {
                self.inline_edit_buffer.insert(self.inline_edit_cursor, c);
                self.inline_edit_cursor += 1;
            }
            HeaderEditField::Interval | HeaderEditField::Scale => {
                if c.is_ascii_digit() {
                    self.inline_edit_buffer.insert(self.inline_edit_cursor, c);
                    self.inline_edit_cursor += 1;
                    // Live preview for scale
                    if field == HeaderEditField::Scale
                        && let Ok(val) = self.inline_edit_buffer.parse::<u64>()
                    {
                        self.color_scale =
                            ColorScale::new(val.clamp(1, 100000), self.color_scale.scheme);
                    }
                }
            }
            HeaderEditField::Colors => {
                // Colors don't accept text input - use scroll wheel
            }
        }
    }

    /// Handle backspace in inline edit
    pub fn inline_edit_backspace(&mut self) {
        if self.inline_edit.is_none() {
            return;
        }
        let field = self.inline_edit.unwrap();

        if self.inline_edit_selected {
            self.inline_edit_buffer.clear();
            self.inline_edit_cursor = 0;
            self.inline_edit_selected = false;
        } else if self.inline_edit_cursor > 0 {
            self.inline_edit_cursor -= 1;
            self.inline_edit_buffer.remove(self.inline_edit_cursor);
        }

        // Live preview for scale
        if field == HeaderEditField::Scale {
            let val = self.inline_edit_buffer.parse::<u64>().unwrap_or(1).max(1);
            self.color_scale = ColorScale::new(val, self.color_scale.scheme);
        }
    }

    /// Move cursor left in inline edit
    pub fn inline_edit_left(&mut self) {
        self.inline_edit_selected = false;
        if self.inline_edit_cursor > 0 {
            self.inline_edit_cursor -= 1;
        }
    }

    /// Move cursor right in inline edit
    pub fn inline_edit_right(&mut self) {
        self.inline_edit_selected = false;
        if self.inline_edit_cursor < self.inline_edit_buffer.len() {
            self.inline_edit_cursor += 1;
        }
    }

    /// Increase value in inline edit (for scroll wheel)
    pub fn inline_edit_increase(&mut self) {
        if let Some(field) = self.inline_edit {
            match field {
                HeaderEditField::Interval => {
                    if let Ok(val) = self.inline_edit_buffer.parse::<u64>() {
                        let new_val = val.saturating_add(1).min(100000);
                        self.inline_edit_buffer = new_val.to_string();
                        self.inline_edit_cursor = self.inline_edit_buffer.len();
                        self.inline_edit_selected = false;
                    }
                }
                HeaderEditField::Scale => {
                    if let Ok(val) = self.inline_edit_buffer.parse::<u64>() {
                        let new_val = val.saturating_add(1).min(100000);
                        self.inline_edit_buffer = new_val.to_string();
                        self.inline_edit_cursor = self.inline_edit_buffer.len();
                        self.inline_edit_selected = false;
                        self.color_scale = ColorScale::new(new_val, self.color_scale.scheme);
                    }
                }
                HeaderEditField::Colors => {
                    let new_scheme = self.color_scale.scheme.next();
                    self.color_scale = ColorScale::new(self.color_scale.max_rtt, new_scheme);
                    self.inline_edit_buffer = format!("{}", new_scheme);
                    self.inline_edit_selected = false;
                }
                HeaderEditField::Target => {}
            }
        }
    }

    /// Decrease value in inline edit (for scroll wheel)
    pub fn inline_edit_decrease(&mut self) {
        if let Some(field) = self.inline_edit {
            match field {
                HeaderEditField::Interval => {
                    if let Ok(val) = self.inline_edit_buffer.parse::<u64>() {
                        let new_val = val.saturating_sub(1).max(1);
                        self.inline_edit_buffer = new_val.to_string();
                        self.inline_edit_cursor = self.inline_edit_buffer.len();
                        self.inline_edit_selected = false;
                    }
                }
                HeaderEditField::Scale => {
                    if let Ok(val) = self.inline_edit_buffer.parse::<u64>() {
                        let new_val = val.saturating_sub(1).max(1);
                        self.inline_edit_buffer = new_val.to_string();
                        self.inline_edit_cursor = self.inline_edit_buffer.len();
                        self.inline_edit_selected = false;
                        self.color_scale = ColorScale::new(new_val, self.color_scale.scheme);
                    }
                }
                HeaderEditField::Colors => {
                    let new_scheme = self.color_scale.scheme.prev();
                    self.color_scale = ColorScale::new(self.color_scale.max_rtt, new_scheme);
                    self.inline_edit_buffer = format!("{}", new_scheme);
                    self.inline_edit_selected = false;
                }
                HeaderEditField::Target => {}
            }
        }
    }

    /// Cycle to next header field (Tab navigation)
    pub fn header_next_field(&mut self) {
        self.header_selected = Some(match self.header_selected {
            None => HeaderEditField::Target,
            Some(HeaderEditField::Target) => HeaderEditField::Interval,
            Some(HeaderEditField::Interval) => HeaderEditField::Scale,
            Some(HeaderEditField::Scale) => HeaderEditField::Colors,
            Some(HeaderEditField::Colors) => HeaderEditField::Target,
        });
    }

    /// Cycle to previous header field (Shift+Tab navigation)
    pub fn header_prev_field(&mut self) {
        self.header_selected = Some(match self.header_selected {
            None => HeaderEditField::Colors,
            Some(HeaderEditField::Target) => HeaderEditField::Colors,
            Some(HeaderEditField::Interval) => HeaderEditField::Target,
            Some(HeaderEditField::Scale) => HeaderEditField::Interval,
            Some(HeaderEditField::Colors) => HeaderEditField::Scale,
        });
    }

    /// Deselect header field
    pub fn header_deselect(&mut self) {
        self.header_selected = None;
    }

    /// Open inline edit for currently selected header field
    pub fn header_open_selected(&mut self) {
        if let Some(field) = self.header_selected {
            // Use position (0, 0) - will be calculated in render based on field
            self.start_inline_edit(field, 10, 1);
        }
    }

    /// Show quit confirmation dialog
    pub fn show_quit_confirm(&mut self) {
        self.quit_confirm = true;
        self.quit_confirm_no_focused = true; // Start with "No" focused (safer default)
    }

    /// Cancel quit confirmation
    pub fn cancel_quit_confirm(&mut self) {
        self.quit_confirm = false;
    }

    /// Confirm quit and exit
    pub fn confirm_quit(&mut self) {
        self.quit_confirm = false;
        self.should_quit = true;
    }
}
