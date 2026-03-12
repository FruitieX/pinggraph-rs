mod color;
mod config;
mod ping;
mod ui;

use std::io;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Result;
use clap::Parser;
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, MouseButton,
        MouseEventKind,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use config::{Config, Mode};
use ping::icmp::IcmpPinger;
use ping::resolve_host;
use ping::udp::{UdpClientPinger, UdpServer};
use ping::{PingResult, Pinger};
use ui::app::{App, HeaderEditField, PingPopup};
use ui::footer::Footer;
use ui::graph::Graph;
use ui::header::{Header, HeaderField};
use ui::legend::{LEGEND_WIDTH, Legend, MIN_WIDTH_FOR_LEGEND};
use ui::settings::SettingsMenu;

const UI_RENDER_INTERVAL: Duration = Duration::from_millis(33);
const FOOTER_UPDATE_INTERVAL: Duration = Duration::from_millis(100);
const UI_IDLE_SLEEP: Duration = Duration::from_millis(1);

struct PingerThread {
    stop: Arc<AtomicBool>,
    join_handle: thread::JoinHandle<()>,
}

impl PingerThread {
    fn stop(self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Err(err) = self.join_handle.join() {
            eprintln!("Pinger thread panicked: {:?}", err);
        }
    }
}

fn build_pinger(
    mode: Mode,
    resolved_ip: IpAddr,
    interval: u64,
    timeout: u64,
    port: u16,
) -> Box<dyn Pinger> {
    match mode {
        Mode::Icmp => Box::new(IcmpPinger::new(resolved_ip, interval, timeout)),
        Mode::UdpClient => {
            let target = SocketAddr::new(resolved_ip, port);
            Box::new(UdpClientPinger::new(target, interval, timeout))
        }
        Mode::UdpServer => unreachable!(),
    }
}

/// Start a pinger on a dedicated thread.
fn start_pinger(
    mode: Mode,
    resolved_ip: IpAddr,
    interval: u64,
    timeout: u64,
    port: u16,
    tx: mpsc::Sender<PingResult>,
) -> PingerThread {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_clone = stop.clone();
    let join_handle = thread::spawn(move || {
        let pinger = build_pinger(mode, resolved_ip, interval, timeout, port);
        pinger.run(tx, stop_clone);
    });

    PingerThread { stop, join_handle }
}

fn spawn_event_reader(tx: mpsc::Sender<Event>) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        while let Ok(event) = event::read() {
            if tx.send(event).is_err() {
                break;
            }
        }
    })
}

fn main() -> Result<()> {
    let config = Config::parse();

    // Handle server mode separately (no TUI)
    if config.mode == Mode::UdpServer {
        config.validate()?;
        let server = UdpServer::new(config.bind.clone(), config.port);
        server.run()?;
        return Ok(());
    }

    // Check if we have a host - if not, we'll start with settings dialog open
    let has_host = config.host.is_some();
    let (mut resolved_ip, mut resolved_ip_str) = if has_host {
        let host = config.host.as_ref().unwrap();
        let ip = resolve_host(host)?;
        (Some(ip), ip.to_string())
    } else {
        (None, "not set".to_string())
    };

    // Set up terminal with mouse support
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app state
    let mut app = App::new(config.clone());

    // If no host provided, open settings dialog immediately
    if !has_host {
        app.open_settings();
    }

    let (event_tx, event_rx) = mpsc::channel::<Event>();
    let _event_reader = spawn_event_reader(event_tx);

    // Create ping channel
    let (mut tx, mut rx) = mpsc::channel::<PingResult>();

    // Start pinger only if we have a host
    let mut pinger_handle: Option<PingerThread> = if let Some(ip) = resolved_ip {
        Some(start_pinger(
            config.mode,
            ip,
            config.interval,
            config.timeout,
            config.port,
            tx.clone(),
        ))
    } else {
        None
    };

    // Main event loop with restart support
    loop {
        let result = run_app(&mut terminal, &mut app, &rx, &event_rx, &resolved_ip_str);

        // Check if we need to restart pinger
        if app.needs_pinger_restart {
            app.needs_pinger_restart = false;

            // Abort current pinger if running
            if let Some(handle) = pinger_handle.take() {
                handle.stop();
            }

            // Clear old results if target changed
            if app.new_target.is_some() {
                app.clear_all_data();

                // Resolve new target
                let new_host = app.new_target.take().unwrap();
                match resolve_host(&new_host) {
                    Ok(ip) => {
                        resolved_ip = Some(ip);
                        resolved_ip_str = ip.to_string();
                    }
                    Err(e) => {
                        // Failed to resolve - keep old target, show error
                        eprintln!("Failed to resolve {}: {}", new_host, e);
                        // Restore old host in config if we had one
                        if let Some(old_host) = &config.host {
                            app.config.host = Some(old_host.clone());
                        }
                    }
                }
            }

            // Get new interval
            let new_interval = app.new_interval.take().unwrap_or(app.config.interval);

            // Create new channel
            let (new_tx, new_rx) = mpsc::channel::<PingResult>();
            tx = new_tx;
            rx = new_rx;

            // Start new pinger only if we have a resolved IP
            if let Some(ip) = resolved_ip {
                pinger_handle = Some(start_pinger(
                    app.config.mode,
                    ip,
                    new_interval,
                    app.config.timeout,
                    app.config.port,
                    tx.clone(),
                ));
            }

            continue;
        }

        // Check result
        if let Err(e) = result {
            if let Some(handle) = pinger_handle.take() {
                handle.stop();
            }

            // Restore terminal before showing error
            disable_raw_mode()?;
            execute!(
                terminal.backend_mut(),
                LeaveAlternateScreen,
                DisableMouseCapture
            )?;
            terminal.show_cursor()?;
            eprintln!("Error: {}", e);
            return Err(e);
        }

        break;
    }

    if let Some(handle) = pinger_handle.take() {
        handle.stop();
    }

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    // Print final stats
    println!("\n{}", app.stats.format_stats());

    Ok(())
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    rx: &mpsc::Receiver<PingResult>,
    event_rx: &mpsc::Receiver<Event>,
    resolved_ip: &str,
) -> Result<()> {
    let mut last_draw = Instant::now()
        .checked_sub(UI_RENDER_INTERVAL)
        .unwrap_or_else(Instant::now);
    let mut last_footer_update = Instant::now()
        .checked_sub(FOOTER_UPDATE_INTERVAL)
        .unwrap_or_else(Instant::now);
    let mut cached_footer_stats = app.stats.clone();
    let mut cached_footer_rtts = app.recent_rtts_slice();
    let mut needs_redraw = true;

    loop {
        if needs_redraw && last_draw.elapsed() >= UI_RENDER_INTERVAL {
            terminal.draw(|frame| {
                let size = frame.area();

                // Determine if we have room for legend
                let show_legend = size.width >= MIN_WIDTH_FOR_LEGEND;

                // Main layout: header, graph (+ optional legend), footer
                let main_chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(3), // Header
                        Constraint::Min(5),    // Graph area
                        Constraint::Length(2), // Footer
                    ])
                    .split(size);

                // Header
                let header = Header::new(
                    &app.config,
                    Some(resolved_ip),
                    size.width,
                    app.header_selected,
                );
                frame.render_widget(header, main_chunks[0]);
                app.header_area = Some((
                    main_chunks[0].x,
                    main_chunks[0].y,
                    main_chunks[0].width,
                    main_chunks[0].height,
                ));

                // Graph area (with optional legend on right)
                let graph_width = if show_legend {
                    main_chunks[1].width.saturating_sub(LEGEND_WIDTH) as usize
                } else {
                    main_chunks[1].width as usize
                };
                let total_rows = app.total_rows(graph_width);

                let graph_area = if show_legend {
                    let graph_chunks = Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints([
                            Constraint::Min(20),              // Graph
                            Constraint::Length(LEGEND_WIDTH), // Legend
                        ])
                        .split(main_chunks[1]);

                    let graph = Graph::new(
                        &app.results,
                        &app.color_scale,
                        app.view_end_row,
                        total_rows,
                        app.result_base_seq,
                        app.paused,
                        app.config.hide_cursor,
                        app.defer_row,
                        app.highlight_rtt_range,
                    );
                    frame.render_widget(graph, graph_chunks[0]);

                    let legend = Legend::new(&app.color_scale);
                    frame.render_widget(legend, graph_chunks[1]);

                    // Store legend area for mouse calculations
                    app.legend_area = Some((
                        graph_chunks[1].x,
                        graph_chunks[1].y,
                        graph_chunks[1].width,
                        graph_chunks[1].height,
                    ));

                    graph_chunks[0]
                } else {
                    let graph = Graph::new(
                        &app.results,
                        &app.color_scale,
                        app.view_end_row,
                        total_rows,
                        app.result_base_seq,
                        app.paused,
                        app.config.hide_cursor,
                        app.defer_row,
                        app.highlight_rtt_range,
                    );
                    frame.render_widget(graph, main_chunks[1]);
                    app.legend_area = None; // No legend when terminal is narrow
                    main_chunks[1]
                };

                // Store graph area for mouse calculations
                app.graph_area = Some((
                    graph_area.x,
                    graph_area.y,
                    graph_area.width,
                    graph_area.height,
                ));

                // Footer (throttled to FOOTER_UPDATE_INTERVAL)
                if last_footer_update.elapsed() >= FOOTER_UPDATE_INTERVAL {
                    cached_footer_stats = app.stats.clone();
                    cached_footer_rtts = app.recent_rtts_slice();
                    last_footer_update = Instant::now();
                }
                let footer = Footer::new(
                    &cached_footer_stats,
                    &cached_footer_rtts,
                    &app.color_scale,
                    size.width,
                    app.config.no_sparkline,
                );
                frame.render_widget(footer, main_chunks[2]);
                app.footer_area = Some((
                    main_chunks[2].x,
                    main_chunks[2].y,
                    main_chunks[2].width,
                    main_chunks[2].height,
                ));

                // Render popup if present
                if let Some(popup) = &app.popup {
                    // Convert stable sequence number to VecDeque index
                    // If the sample has been evicted from the buffer, skip rendering
                    let result_idx = popup.result_seq.checked_sub(app.result_base_seq);
                    if let Some(idx) = result_idx
                        && let Some(result) = app.results.get(idx)
                    {
                        let rtt_str = result
                            .rtt_ms_f64()
                            .map(|ms| format!("{:.2}ms", ms))
                            .unwrap_or_else(|| "TIMEOUT".to_string());
                        let jitter_str = result
                            .jitter_ms_f64()
                            .map(|ms| format!("±{:.2}ms", ms))
                            .unwrap_or_else(|| "-".to_string());
                        let time_str = result.timestamp_str();

                        let popup_width = 28u16;
                        let popup_height = 6u16;

                        // Position popup near click but within bounds
                        let popup_x = popup
                            .screen_x
                            .saturating_sub(popup_width / 2)
                            .min(size.width.saturating_sub(popup_width));
                        let popup_y = if popup.screen_y > popup_height + 1 {
                            popup.screen_y - popup_height - 1
                        } else {
                            popup.screen_y + 1
                        }
                        .min(size.height.saturating_sub(popup_height));

                        let popup_area = Rect::new(popup_x, popup_y, popup_width, popup_height);

                        frame.render_widget(Clear, popup_area);

                        let popup_block = Block::default()
                            .title(" Ping Info ")
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(Color::Cyan))
                            .style(Style::default().bg(Color::Rgb(30, 30, 40)));

                        let popup_text = vec![
                            Line::from(vec![
                                Span::styled("Time:   ", Style::default().fg(Color::Gray)),
                                Span::styled(&time_str, Style::default().fg(Color::White)),
                            ]),
                            Line::from(vec![
                                Span::styled("RTT:    ", Style::default().fg(Color::Gray)),
                                Span::styled(
                                    &rtt_str,
                                    Style::default()
                                        .fg(Color::White)
                                        .add_modifier(Modifier::BOLD),
                                ),
                            ]),
                            Line::from(vec![
                                Span::styled("Jitter: ", Style::default().fg(Color::Gray)),
                                Span::styled(&jitter_str, Style::default().fg(Color::Yellow)),
                            ]),
                            Line::from(vec![
                                Span::styled("Seq:    ", Style::default().fg(Color::Gray)),
                                Span::styled(
                                    format!("{}", result.seq),
                                    Style::default().fg(Color::Cyan),
                                ),
                            ]),
                        ];

                        let popup_para = Paragraph::new(popup_text).block(popup_block);
                        frame.render_widget(popup_para, popup_area);
                    }
                }

                // Render settings menu if open
                if app.settings_open {
                    let settings_menu = SettingsMenu::new(
                        app.settings_field,
                        app.settings_target.clone(),
                        app.settings_interval,
                        app.settings_scale,
                        app.settings_colors,
                        app.settings_hide_cursor,
                        app.settings_buffer_mb,
                        app.settings_input_active,
                        app.settings_input_buffer.clone(),
                        app.settings_input_cursor,
                        app.settings_input_selected,
                    );
                    frame.render_widget(settings_menu, size);
                }

                // Render inline edit popup if active
                if let Some(field) = app.inline_edit {
                    let (popup_x, popup_y) = app.inline_edit_pos;

                    let title = match field {
                        HeaderEditField::Target => " Target ",
                        HeaderEditField::Interval => " Interval (ms) ",
                        HeaderEditField::Scale => " Scale (ms) ",
                        HeaderEditField::Colors => " Color Scheme ",
                    };

                    let popup_width = 30u16.max(app.inline_edit_buffer.len() as u16 + 6);
                    let popup_height = 4u16; // Increased for confirm button

                    // Position below the clicked item
                    let px = popup_x
                        .saturating_sub(1)
                        .min(size.width.saturating_sub(popup_width));
                    let py = (popup_y + 1).min(size.height.saturating_sub(popup_height));

                    let popup_area = Rect::new(px, py, popup_width, popup_height);
                    frame.render_widget(Clear, popup_area);

                    let popup_block = Block::default()
                        .title(title)
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Cyan))
                        .style(Style::default().bg(Color::Rgb(30, 30, 40)));

                    // Styles matching settings menu
                    let input_style = Style::default().fg(Color::White).bg(Color::Rgb(60, 60, 80));
                    let selected_text_style = Style::default()
                        .fg(Color::Black)
                        .bg(Color::Rgb(150, 180, 255));
                    let value_style = Style::default().fg(Color::Cyan);
                    let selected_style = Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD);
                    let button_style = Style::default().fg(Color::White).bg(Color::Rgb(60, 60, 80));
                    let button_selected_style = Style::default()
                        .fg(Color::Black)
                        .bg(Color::Rgb(100, 200, 100));

                    let input_focused = !app.inline_edit_confirm_focused;

                    // Render content based on field type
                    let input_line = if field == HeaderEditField::Colors {
                        // Colors is an enum selector - show with arrows
                        Line::from(vec![
                            Span::styled(
                                "◄ ",
                                if input_focused {
                                    Style::default().fg(Color::White)
                                } else {
                                    Style::default().fg(Color::DarkGray)
                                },
                            ),
                            Span::styled(
                                app.inline_edit_buffer.clone(),
                                if input_focused {
                                    selected_style
                                } else {
                                    value_style
                                },
                            ),
                            Span::styled(
                                " ►",
                                if input_focused {
                                    Style::default().fg(Color::White)
                                } else {
                                    Style::default().fg(Color::DarkGray)
                                },
                            ),
                        ])
                    } else if app.inline_edit_selected && input_focused {
                        // Selected text (select-all state)
                        Line::from(vec![Span::styled(
                            app.inline_edit_buffer.clone(),
                            selected_text_style,
                        )])
                    } else if app.inline_edit_input_active && input_focused {
                        // Text input mode - show cursor
                        let before: String = app
                            .inline_edit_buffer
                            .chars()
                            .take(app.inline_edit_cursor)
                            .collect();
                        let after: String = app
                            .inline_edit_buffer
                            .chars()
                            .skip(app.inline_edit_cursor)
                            .collect();
                        Line::from(vec![
                            Span::styled(before, input_style),
                            Span::styled("▏", Style::default().fg(Color::White)),
                            Span::styled(after, input_style),
                        ])
                    } else {
                        // Navigation mode or unfocused - show value with appropriate style
                        Line::from(vec![Span::styled(
                            app.inline_edit_buffer.clone(),
                            if input_focused {
                                selected_style
                            } else {
                                value_style
                            },
                        )])
                    };

                    // Confirm button line
                    let button_line = Line::from(vec![Span::styled(
                        " Confirm ",
                        if app.inline_edit_confirm_focused {
                            button_selected_style
                        } else {
                            button_style
                        },
                    )]);

                    let inner = popup_block.inner(popup_area);
                    frame.render_widget(popup_block, popup_area);
                    let para = Paragraph::new(vec![input_line, button_line]);
                    frame.render_widget(para, inner);

                    // Store confirm button area for click detection
                    app.inline_edit_confirm_area = Some((
                        px + 1, // after border
                        py + 2, // second line inside popup
                        9,      // " Confirm " width
                    ));
                } else {
                    app.inline_edit_confirm_area = None;
                }

                // Render quit confirmation dialog if active
                if app.quit_confirm {
                    let popup_width = 32u16;
                    let popup_height = 5u16;
                    let popup_x = size.width.saturating_sub(popup_width) / 2;
                    let popup_y = size.height.saturating_sub(popup_height) / 2;

                    let popup_area = Rect::new(popup_x, popup_y, popup_width, popup_height);
                    frame.render_widget(Clear, popup_area);

                    let popup_block = Block::default()
                        .title(" Quit? ")
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Yellow))
                        .style(Style::default().bg(Color::Rgb(40, 40, 50)));

                    // Styles matching settings menu
                    let button_style = Style::default().fg(Color::White).bg(Color::Rgb(60, 60, 80));
                    let yes_selected_style = Style::default()
                        .fg(Color::Black)
                        .bg(Color::Rgb(200, 100, 100));
                    let no_selected_style = Style::default()
                        .fg(Color::Black)
                        .bg(Color::Rgb(100, 200, 100));

                    let popup_text = vec![
                        Line::from(Span::styled(
                            "Are you sure you want to quit?",
                            Style::default().fg(Color::White),
                        )),
                        Line::from(""),
                        Line::from(vec![
                            Span::styled(
                                " Yes ",
                                if !app.quit_confirm_no_focused {
                                    yes_selected_style
                                } else {
                                    button_style
                                },
                            ),
                            Span::raw("  "),
                            Span::styled(
                                " No ",
                                if app.quit_confirm_no_focused {
                                    no_selected_style
                                } else {
                                    button_style
                                },
                            ),
                        ]),
                    ];

                    let para = Paragraph::new(popup_text)
                        .block(popup_block)
                        .alignment(ratatui::layout::Alignment::Center);
                    frame.render_widget(para, popup_area);

                    // Store button areas for click detection
                    // Buttons are centered: " Yes " (5) + "  " (2) + " No " (4) = 11
                    // Center offset: (popup_width - 2 - 11) / 2 = (30 - 11) / 2 = 9 (approx)
                    let buttons_start = popup_x + (popup_width - 11) / 2;
                    app.quit_confirm_yes_area = Some((buttons_start, popup_y + 3, 5));
                    app.quit_confirm_no_area = Some((buttons_start + 7, popup_y + 3, 4));
                } else {
                    app.quit_confirm_yes_area = None;
                    app.quit_confirm_no_area = None;
                }
            })?;
            last_draw = Instant::now();
            needs_redraw = false;
        }

        while let Ok(event) = event_rx.try_recv() {
            needs_redraw = true;
            match event {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    // Handle quit confirmation dialog first
                    if app.quit_confirm {
                        match key.code {
                            KeyCode::Char('y') | KeyCode::Char('Y') => {
                                app.confirm_quit();
                            }
                            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                                app.cancel_quit_confirm();
                            }
                            KeyCode::Enter => {
                                // Enter confirms the focused button
                                if app.quit_confirm_no_focused {
                                    app.cancel_quit_confirm();
                                } else {
                                    app.confirm_quit();
                                }
                            }
                            KeyCode::Left | KeyCode::Right | KeyCode::Tab => {
                                // Toggle between Yes and No
                                app.quit_confirm_no_focused = !app.quit_confirm_no_focused;
                            }
                            _ => {}
                        }
                    }
                    // Handle inline edit input
                    else if let Some(edit_field) = app.inline_edit {
                        if app.inline_edit_confirm_focused {
                            // Confirm button is focused
                            match key.code {
                                KeyCode::Esc => {
                                    app.cancel_inline_edit();
                                }
                                KeyCode::Enter => {
                                    app.apply_inline_edit();
                                }
                                KeyCode::Up | KeyCode::Tab => {
                                    // Move focus to input field
                                    app.inline_edit_confirm_focused = false;
                                }
                                _ => {}
                            }
                        } else if app.inline_edit_input_active {
                            // Text input mode - arrow keys move cursor
                            match key.code {
                                KeyCode::Esc => {
                                    // Exit text input mode back to navigation mode
                                    app.inline_edit_input_active = false;
                                }
                                KeyCode::Enter => {
                                    app.apply_inline_edit();
                                }
                                KeyCode::Backspace => {
                                    app.inline_edit_backspace();
                                }
                                KeyCode::Left => {
                                    app.inline_edit_left();
                                }
                                KeyCode::Right => {
                                    app.inline_edit_right();
                                }
                                KeyCode::Down | KeyCode::Tab => {
                                    // Move focus to confirm button
                                    app.inline_edit_confirm_focused = true;
                                    app.inline_edit_input_active = false;
                                }
                                KeyCode::Char(c) => {
                                    app.inline_edit_char(c);
                                }
                                _ => {}
                            }
                        } else {
                            // Navigation mode - arrow keys adjust values
                            match key.code {
                                KeyCode::Esc => {
                                    app.cancel_inline_edit();
                                }
                                KeyCode::Enter => {
                                    // Enter activates text input mode for text fields,
                                    // or applies for Colors, or applies if confirm focused
                                    if edit_field == HeaderEditField::Colors {
                                        app.apply_inline_edit();
                                    } else {
                                        app.inline_edit_activate_input();
                                    }
                                }
                                KeyCode::Left => {
                                    app.inline_edit_decrease();
                                }
                                KeyCode::Right => {
                                    app.inline_edit_increase();
                                }
                                KeyCode::Down | KeyCode::Tab => {
                                    // Move focus to confirm button
                                    app.inline_edit_confirm_focused = true;
                                }
                                KeyCode::Char(c) => {
                                    // Typing immediately replaces value (for text fields)
                                    if edit_field != HeaderEditField::Colors {
                                        // Clear and start fresh with typed char
                                        app.inline_edit_buffer.clear();
                                        app.inline_edit_cursor = 0;
                                        app.inline_edit_selected = false;
                                        app.inline_edit_char(c);
                                        // Activate input mode so subsequent chars are added
                                        app.inline_edit_input_active = true;
                                    }
                                }
                                _ => {}
                            }
                        }
                    } else if app.settings_open {
                        // Handle settings menu input
                        if app.settings_input_active {
                            // Text input mode
                            match key.code {
                                KeyCode::Esc => {
                                    // Cancel text input, restore previous value
                                    app.settings_input_active = false;
                                    app.settings_input_buffer.clear();
                                }
                                KeyCode::Enter => {
                                    // Confirm the text input
                                    app.settings_confirm_input();
                                }
                                KeyCode::Backspace => {
                                    app.settings_input_backspace();
                                }
                                KeyCode::Left => {
                                    app.settings_input_left();
                                }
                                KeyCode::Right => {
                                    app.settings_input_right();
                                }
                                KeyCode::Char(c) => {
                                    app.settings_input_char(c);
                                }
                                _ => {}
                            }
                        } else {
                            // Navigation mode
                            match key.code {
                                KeyCode::Esc => {
                                    app.cancel_settings();
                                }
                                KeyCode::Enter => {
                                    // Handle based on current field
                                    if app.settings_field.is_text_input() {
                                        app.settings_start_input();
                                    } else if app.settings_field == ui::app::SettingsField::Confirm
                                    {
                                        app.apply_settings();
                                        app.settings_open = false;
                                    } else if app.settings_field == ui::app::SettingsField::Cancel {
                                        app.cancel_settings();
                                    } else {
                                        // ColorScheme or HideCursor - just cycle with enter
                                        app.settings_increase();
                                    }
                                }
                                KeyCode::Up => {
                                    app.settings_prev_field();
                                }
                                KeyCode::Down => {
                                    app.settings_next_field();
                                }
                                KeyCode::Left => {
                                    // On buttons, left goes to Confirm
                                    if app.settings_field.is_button() {
                                        app.settings_field = ui::app::SettingsField::Confirm;
                                    } else {
                                        app.settings_decrease();
                                    }
                                }
                                KeyCode::Right => {
                                    // On buttons, right goes to Cancel
                                    if app.settings_field.is_button() {
                                        app.settings_field = ui::app::SettingsField::Cancel;
                                    } else {
                                        app.settings_increase();
                                    }
                                }
                                KeyCode::Char(c) => {
                                    // Start typing immediately on text fields
                                    if app.settings_field.is_text_input() {
                                        app.settings_start_input();
                                        app.settings_input_char(c);
                                    }
                                }
                                _ => {}
                            }
                        }
                    } else {
                        // Close popup on any key (except for header navigation)
                        if key.code != KeyCode::Tab
                            && key.code != KeyCode::BackTab
                            && key.code != KeyCode::Enter
                        {
                            app.popup = None;
                        }

                        match key.code {
                            KeyCode::Char('q') | KeyCode::Char('Q') => {
                                // Show quit confirmation if not scrolled
                                if app.view_end_row.is_some() {
                                    app.jump_to_live();
                                } else if app.header_selected.is_some() {
                                    app.header_deselect();
                                } else {
                                    app.show_quit_confirm();
                                }
                            }
                            KeyCode::Esc => {
                                // Esc deselects header, or shows quit confirm
                                if app.header_selected.is_some() {
                                    app.header_deselect();
                                } else if app.view_end_row.is_some() {
                                    app.jump_to_live();
                                } else {
                                    app.show_quit_confirm();
                                }
                            }
                            KeyCode::Tab => {
                                app.popup = None;
                                app.header_next_field();
                            }
                            KeyCode::BackTab => {
                                app.popup = None;
                                app.header_prev_field();
                            }
                            KeyCode::Enter => {
                                // Open inline edit for selected header field
                                if app.header_selected.is_some() {
                                    app.header_open_selected();
                                }
                            }
                            KeyCode::Char('s') | KeyCode::Char('S') => {
                                app.header_deselect();
                                app.toggle_settings();
                            }
                            KeyCode::Char(' ') => {
                                app.toggle_pause();
                            }
                            KeyCode::Up | KeyCode::PageUp => {
                                let rows = if key.code == KeyCode::PageUp { 10 } else { 1 };
                                app.scroll_up(rows);
                            }
                            KeyCode::Down | KeyCode::PageDown => {
                                let rows = if key.code == KeyCode::PageDown { 10 } else { 1 };
                                app.scroll_down(rows);
                            }
                            KeyCode::Home => {
                                app.jump_to_live();
                            }
                            KeyCode::Char('r') | KeyCode::Char('R') => {
                                app.clear_all_data();
                            }
                            _ => {}
                        }
                    }
                }
                Event::Mouse(mouse) => {
                    // Handle quit confirmation dialog mouse events
                    if app.quit_confirm {
                        if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
                            // Check if Yes button was clicked
                            if let Some((x, y, w)) = app.quit_confirm_yes_area
                                && mouse.row == y
                                && mouse.column >= x
                                && mouse.column < x + w
                            {
                                // If already focused, activate; otherwise just focus
                                if !app.quit_confirm_no_focused {
                                    app.confirm_quit();
                                } else {
                                    app.quit_confirm_no_focused = false;
                                }
                                continue;
                            }
                            // Check if No button was clicked
                            if let Some((x, y, w)) = app.quit_confirm_no_area
                                && mouse.row == y
                                && mouse.column >= x
                                && mouse.column < x + w
                            {
                                // If already focused, activate; otherwise just focus
                                if app.quit_confirm_no_focused {
                                    app.cancel_quit_confirm();
                                } else {
                                    app.quit_confirm_no_focused = true;
                                }
                                continue;
                            }
                        }
                    }
                    // Handle inline edit popup mouse events
                    else if app.inline_edit.is_some() {
                        match mouse.kind {
                            MouseEventKind::Down(MouseButton::Left) => {
                                // Check if confirm button was clicked
                                if let Some((cx, cy, cw)) = app.inline_edit_confirm_area
                                    && mouse.row == cy
                                    && mouse.column >= cx
                                    && mouse.column < cx + cw
                                {
                                    // If already focused, activate; otherwise just focus
                                    if app.inline_edit_confirm_focused {
                                        app.apply_inline_edit();
                                    } else {
                                        app.inline_edit_confirm_focused = true;
                                        app.inline_edit_input_active = false;
                                    }
                                    continue;
                                }

                                // Calculate popup bounds
                                let (px, py) = app.inline_edit_pos;
                                let size = terminal.size()?;
                                let popup_width =
                                    30u16.max(app.inline_edit_buffer.len() as u16 + 6);
                                let popup_height = 4u16;
                                let popup_x = px
                                    .saturating_sub(1)
                                    .min(size.width.saturating_sub(popup_width));
                                let popup_y =
                                    (py + 1).min(size.height.saturating_sub(popup_height));

                                // Check if input area was clicked (first line inside popup)
                                let input_row = popup_y + 1;
                                if mouse.row == input_row
                                    && mouse.column > popup_x
                                    && mouse.column < popup_x + popup_width - 1
                                {
                                    // Focus input and activate text input mode
                                    app.inline_edit_confirm_focused = false;
                                    if app.inline_edit != Some(HeaderEditField::Colors) {
                                        app.inline_edit_activate_input();
                                    }
                                    continue;
                                }

                                // Click outside popup closes it and applies
                                if mouse.column < popup_x
                                    || mouse.column >= popup_x + popup_width
                                    || mouse.row < popup_y
                                    || mouse.row >= popup_y + popup_height
                                {
                                    app.apply_inline_edit();
                                }
                            }
                            MouseEventKind::ScrollUp => {
                                app.inline_edit_increase();
                            }
                            MouseEventKind::ScrollDown => {
                                app.inline_edit_decrease();
                            }
                            _ => {}
                        }
                    } else if app.settings_open {
                        // Handle mouse in settings menu
                        match mouse.kind {
                            MouseEventKind::Down(MouseButton::Left) => {
                                let size = terminal.size()?;
                                app.settings_handle_click(
                                    mouse.column,
                                    mouse.row,
                                    size.width,
                                    size.height,
                                );
                            }
                            MouseEventKind::ScrollUp => {
                                app.settings_increase();
                            }
                            MouseEventKind::ScrollDown => {
                                app.settings_decrease();
                            }
                            _ => {}
                        }
                        // Don't use continue here - allow ping processing to continue
                    } else {
                        // Normal mouse handling
                        match mouse.kind {
                            MouseEventKind::Down(MouseButton::Left) => {
                                // Check header click regions first
                                let mut handled = false;
                                if let Some((hx, hy, hw, hh)) = app.header_area {
                                    let mx = mouse.column;
                                    let my = mouse.row;

                                    // Content row is at hy + 1 (after top border)
                                    if my == hy + 1 && my < hy + hh {
                                        // Calculate click regions for current config
                                        let header =
                                            Header::new(&app.config, Some(resolved_ip), hw, None);
                                        let regions = header.calculate_click_regions();

                                        // Check which region was clicked (mx relative to content start)
                                        let rel_x = mx.saturating_sub(hx); // relative to header area

                                        for region in regions {
                                            if rel_x >= region.start_x && rel_x < region.end_x {
                                                match region.field {
                                                    HeaderField::Target => {
                                                        app.start_inline_edit(
                                                            HeaderEditField::Target,
                                                            mx,
                                                            my,
                                                        );
                                                    }
                                                    HeaderField::Interval => {
                                                        app.start_inline_edit(
                                                            HeaderEditField::Interval,
                                                            mx,
                                                            my,
                                                        );
                                                    }
                                                    HeaderField::Scale => {
                                                        app.start_inline_edit(
                                                            HeaderEditField::Scale,
                                                            mx,
                                                            my,
                                                        );
                                                    }
                                                    HeaderField::Colors => {
                                                        app.start_inline_edit(
                                                            HeaderEditField::Colors,
                                                            mx,
                                                            my,
                                                        );
                                                    }
                                                    HeaderField::Settings => {
                                                        app.toggle_settings();
                                                    }
                                                }
                                                handled = true;
                                                break;
                                            }
                                        }
                                    }
                                }

                                // Check footer buttons (right-aligned: [r: reset] [q: quit])
                                if !handled && let Some((fx, fy, fw, fh)) = app.footer_area {
                                    let mx = mouse.column;
                                    let my = mouse.row;
                                    // Footer content is at fy + 1 (after top border)
                                    if my >= fy && my < fy + fh {
                                        // "[r: reset] [q: quit]" is 21 chars
                                        let buttons_start = fx + fw.saturating_sub(22);
                                        let quit_start = fx + fw.saturating_sub(10);
                                        if mx >= quit_start {
                                            app.quit();
                                            handled = true;
                                        } else if mx >= buttons_start && mx < buttons_start + 11 {
                                            app.clear_all_data();
                                            handled = true;
                                        }
                                    }
                                }

                                // Handle legend click to highlight matching samples
                                if !handled && let Some((lx, ly, lw, lh)) = app.legend_area {
                                    let mx = mouse.column;
                                    let my = mouse.row;

                                    if mx >= lx && mx < lx + lw && my >= ly && my < ly + lh {
                                        // Calculate which legend entry was clicked
                                        // Legend has a border (1 row) and title, content starts at ly + 1
                                        let content_y = ly + 1;
                                        if my >= content_y && my < ly + lh - 1 {
                                            let entry_idx = (my - content_y) as usize;
                                            // Get the RTT range for this legend entry
                                            if let Some(range) =
                                                app.color_scale.legend_entry_range(entry_idx)
                                            {
                                                app.highlight_rtt_range = Some(range);
                                            }
                                        }
                                        handled = true;
                                    }
                                }

                                // Show tooltip on graph click if not handled
                                if !handled && let Some((gx, gy, gw, gh)) = app.graph_area {
                                    let mx = mouse.column;
                                    let my = mouse.row;

                                    if mx >= gx && mx < gx + gw && my >= gy && my < gy + gh {
                                        let screen_col = (mx - gx) as usize;
                                        let screen_row = (my - gy) as usize;

                                        let width = gw as usize;
                                        let total_rows = app.total_rows(width);
                                        let view_end = app.view_end_row.unwrap_or(total_rows);

                                        if let Some(idx) = Graph::result_at_position(
                                            app.results.len(),
                                            app.result_base_seq,
                                            width,
                                            gh as usize,
                                            view_end,
                                            screen_row,
                                            screen_col,
                                        ) {
                                            // Freeze the view if we're in live mode
                                            if app.view_end_row.is_none() {
                                                app.popup_was_live = true;
                                                app.view_end_row = Some(total_rows);
                                            } else {
                                                app.popup_was_live = false;
                                            }
                                            // Convert VecDeque index to stable sequence number
                                            let result_seq = app.result_base_seq + idx;
                                            app.popup = Some(PingPopup {
                                                result_seq,
                                                screen_x: mx,
                                                screen_y: my,
                                            });
                                        } else {
                                            app.popup = None;
                                        }
                                    } else {
                                        app.popup = None;
                                    }
                                }
                            }
                            MouseEventKind::Drag(MouseButton::Left) => {
                                // Handle legend drag to update highlight
                                if let Some((lx, ly, lw, lh)) = app.legend_area {
                                    let mx = mouse.column;
                                    let my = mouse.row;

                                    if mx >= lx && mx < lx + lw && my >= ly && my < ly + lh {
                                        let content_y = ly + 1;
                                        if my >= content_y && my < ly + lh - 1 {
                                            let entry_idx = (my - content_y) as usize;
                                            if let Some(range) =
                                                app.color_scale.legend_entry_range(entry_idx)
                                            {
                                                app.highlight_rtt_range = Some(range);
                                            }
                                        }
                                    } else {
                                        // Clear highlight when dragging outside legend
                                        app.highlight_rtt_range = None;
                                    }
                                }

                                // Show tooltip while mouse button is held (Down or Drag)
                                if let Some((gx, gy, gw, gh)) = app.graph_area {
                                    let mx = mouse.column;
                                    let my = mouse.row;

                                    if mx >= gx && mx < gx + gw && my >= gy && my < gy + gh {
                                        let screen_col = (mx - gx) as usize;
                                        let screen_row = (my - gy) as usize;

                                        // Calculate which result was clicked/dragged over
                                        let width = gw as usize;
                                        let total_rows = app.total_rows(width);
                                        let view_end = app.view_end_row.unwrap_or(total_rows);

                                        if let Some(idx) = Graph::result_at_position(
                                            app.results.len(),
                                            app.result_base_seq,
                                            width,
                                            gh as usize,
                                            view_end,
                                            screen_row,
                                            screen_col,
                                        ) {
                                            // Convert VecDeque index to stable sequence number
                                            let result_seq = app.result_base_seq + idx;
                                            app.popup = Some(PingPopup {
                                                result_seq,
                                                screen_x: mx,
                                                screen_y: my,
                                            });
                                        } else {
                                            app.popup = None;
                                        }
                                    } else {
                                        app.popup = None;
                                    }
                                }
                            }
                            MouseEventKind::Up(MouseButton::Left) => {
                                // Hide tooltip and clear highlight when mouse button released
                                app.popup = None;
                                app.highlight_rtt_range = None;
                                // Restore live mode if we were in live mode before the popup
                                if app.popup_was_live {
                                    app.view_end_row = None;
                                    app.popup_was_live = false;
                                }
                            }
                            MouseEventKind::ScrollUp => {
                                app.scroll_up(3);
                            }
                            MouseEventKind::ScrollDown => {
                                app.scroll_down(3);
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }

        // Process any pending ping results (discard if paused)
        while let Ok(result) = rx.try_recv() {
            if !app.paused {
                app.record_result(result);
                needs_redraw = true;
            }
            // When paused, results are discarded - pings continue but aren't recorded
        }

        if app.should_quit {
            return Ok(());
        }

        // Check if pinger needs restart
        if app.needs_pinger_restart {
            return Ok(());
        }

        std::thread::sleep(UI_IDLE_SLEEP);
    }
}
