//! Claudio - Voice-to-text CLI using macOS Speech framework
//!
//! A CLI tool that listens via microphone and transcribes speech in real-time.

use std::{
    env,
    io::{stderr, IsTerminal, Read, Write},
    process::Command,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    terminal,
};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
    TerminalOptions, Viewport,
};

mod speech;
use speech::SpeechRecognizer;

const LISTENING_FRAMES: [&str; 4] = ["◐", "◓", "◑", "◒"];
const WAITING_FRAMES: [&str; 12] = ["⠋", "⠙", "⠹", "⠸", "⢰", "⣰", "⣠", "⣄", "⣆", "⡆", "⠇", "⠏"];
const CHAR_DELAY_MS: f32 = 20.0; // Delay between each character appearing
const SHIMMER_SPEED: f32 = 1.0; // Speed of the shimmer wave (slower = more subtle)

/// Query cursor position via /dev/tty directly, bypassing stdout.
/// This allows the TUI to work when stdout is piped (e.g., `claudio | less`).
/// Uses /dev/tty which is the controlling terminal regardless of redirections.
/// Returns (column, row) with 0-based indexing.
#[cfg(unix)]
fn cursor_position_via_tty() -> Result<(u16, u16)> {
    use std::fs::OpenOptions;
    use std::os::unix::fs::OpenOptionsExt;

    // Open /dev/tty for both reading and writing
    // This gives us direct access to the controlling terminal
    let mut tty = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NOCTTY) // Don't make it controlling terminal
        .open("/dev/tty")?;

    // Write cursor position query: ESC [ 6 n
    tty.write_all(b"\x1B[6n")?;
    tty.flush()?;

    // Read response: ESC [ row ; col R
    let mut buf = [0u8; 32];
    let mut i = 0;

    let start = Instant::now();
    let timeout = Duration::from_secs(2);

    loop {
        if start.elapsed() > timeout {
            anyhow::bail!("Timeout waiting for cursor position response");
        }

        if let Ok(1) = tty.read(&mut buf[i..i + 1]) {
            if buf[i] == b'R' {
                break;
            }
            i += 1;
            if i >= buf.len() {
                anyhow::bail!("Cursor position response too long");
            }
        }
    }

    // Parse response: ESC [ row ; col R
    let response = std::str::from_utf8(&buf[..i])?;
    let esc_pos = response.find('\x1B').unwrap_or(0);
    let coords = &response[esc_pos..];

    if !coords.starts_with("\x1B[") {
        anyhow::bail!("Invalid cursor position response: {}", response);
    }

    let coords = &coords[2..]; // Skip ESC [
    let parts: Vec<&str> = coords.split(';').collect();
    if parts.len() != 2 {
        anyhow::bail!("Invalid cursor position format: {}", coords);
    }

    let row: u16 = parts[0].parse()?;
    let col: u16 = parts[1].parse()?;

    // Terminal reports 1-based, convert to 0-based
    Ok((col.saturating_sub(1), row.saturating_sub(1)))
}

/// Fallback for non-Unix systems - just return (0, 0) and hope for the best
#[cfg(not(unix))]
fn cursor_position_via_tty() -> Result<(u16, u16)> {
    Ok((0, 0))
}

struct App {
    transcription: Arc<Mutex<String>>,
    previous_transcription_len: usize,
    animation_start_index: usize,
    is_listening: Arc<AtomicBool>,
    is_ready: Arc<AtomicBool>,
    should_quit: bool,
    exit_code: i32,
    animation_frame: usize,
    last_frame_time: Instant,
    transcription_start_time: Instant,
    recognizer: Option<SpeechRecognizer>,
    viewport_height: u16,
    shimmer_offset: f32,
}

impl App {
    fn new() -> Self {
        Self {
            transcription: Arc::new(Mutex::new(String::new())),
            previous_transcription_len: 0,
            animation_start_index: 0,
            is_listening: Arc::new(AtomicBool::new(false)),
            is_ready: Arc::new(AtomicBool::new(false)),
            should_quit: false,
            exit_code: 0,
            animation_frame: 0,
            last_frame_time: Instant::now(),
            transcription_start_time: Instant::now(),
            recognizer: None,
            viewport_height: 1,
            shimmer_offset: 0.0,
        }
    }

    fn start_listening(&mut self) -> Result<()> {
        let transcription = Arc::clone(&self.transcription);
        let is_listening = Arc::clone(&self.is_listening);
        let is_ready = Arc::clone(&self.is_ready);

        self.recognizer = Some(SpeechRecognizer::new(
            transcription,
            is_listening,
            is_ready,
        )?);
        self.recognizer.as_mut().unwrap().start()?;

        Ok(())
    }

    fn stop_listening(&mut self) {
        if let Some(ref mut recognizer) = self.recognizer {
            recognizer.stop();
        }
        self.is_listening.store(false, Ordering::SeqCst);
    }

    fn restart(&mut self) -> Result<()> {
        // Stop current recognition session
        self.stop_listening();

        // Clear transcription buffer
        self.transcription.lock().unwrap().clear();

        // Reset animation state
        self.previous_transcription_len = 0;
        self.animation_start_index = 0;
        self.transcription_start_time = Instant::now();
        self.is_ready.store(false, Ordering::SeqCst);

        // Start fresh recognition session
        let transcription = Arc::clone(&self.transcription);
        let is_listening = Arc::clone(&self.is_listening);
        let is_ready = Arc::clone(&self.is_ready);

        self.recognizer = Some(SpeechRecognizer::new(
            transcription,
            is_listening,
            is_ready,
        )?);
        self.recognizer.as_mut().unwrap().start()?;

        Ok(())
    }

    fn get_final_transcription(&self) -> String {
        self.transcription.lock().unwrap().clone()
    }

    fn update_transcription_state(&mut self) {
        let transcription = self.transcription.lock().unwrap();
        let current_len = transcription.chars().count();
        drop(transcription);

        if current_len != self.previous_transcription_len {
            self.transcription_start_time = Instant::now();
            self.animation_start_index = self.previous_transcription_len;
            self.previous_transcription_len = current_len;
        }
    }

    fn update_animation(&mut self) {
        if self.last_frame_time.elapsed() >= Duration::from_millis(150) {
            self.animation_frame = (self.animation_frame + 1) % LISTENING_FRAMES.len();
            self.last_frame_time = Instant::now();
        }

        // Update shimmer wave
        self.shimmer_offset += SHIMMER_SPEED;
        if self.shimmer_offset > 1000.0 {
            self.shimmer_offset = 0.0;
        }
    }
}

fn main() -> Result<()> {
    // Parse args: check if we have `--` followed by a command
    let args: Vec<String> = env::args().collect();
    let exec_command = if let Some(separator_pos) = args.iter().position(|arg| arg == "--") {
        if separator_pos + 1 < args.len() {
            Some(args[separator_pos + 1..].to_vec())
        } else {
            None
        }
    } else {
        None
    };

    let mut app = App::new();

    // Start speech recognition
    if let Err(e) = app.start_listening() {
        eprintln!("Failed to start speech recognition: {}", e);
        eprintln!("Make sure you have granted microphone and speech recognition permissions.");
        std::process::exit(1);
    }

    let result = run_app(&mut app);

    match result {
        Ok(()) => {
            if app.exit_code == 0 {
                let transcription = app.get_final_transcription();
                if !transcription.is_empty() {
                    if let Some(cmd_args) = exec_command {
                        // Execute the command with transcription as stdin
                        let mut child = Command::new(&cmd_args[0])
                            .args(&cmd_args[1..])
                            .stdin(std::process::Stdio::piped())
                            .spawn()?;

                        if let Some(mut stdin) = child.stdin.take() {
                            stdin.write_all(transcription.as_bytes())?;
                        }

                        let status = child.wait()?;
                        std::process::exit(status.code().unwrap_or(1));
                    } else {
                        // Just print to stdout
                        println!("{}", transcription);
                    }
                }
            }
            std::process::exit(app.exit_code);
        }
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    }
}

fn run_app(app: &mut App) -> Result<()> {
    let tick_rate = Duration::from_millis(33); // ~30 FPS

    // Check if stdout is piped - if so, we need to use stderr for cursor position queries
    // because crossterm's Viewport::Inline queries cursor position via stdout
    let stdout_is_tty = std::io::stdout().is_terminal();

    // Get initial cursor position and terminal width
    // We need raw mode enabled BEFORE querying cursor position so the response is readable
    terminal::enable_raw_mode()?;

    let (_initial_cursor_col, initial_cursor_row) = if stdout_is_tty {
        // stdout is a TTY, crossterm's normal cursor query will work
        // We don't need to query here, Viewport::Inline handles it
        (0, 0) // These won't be used
    } else {
        // stdout is piped, query cursor position via /dev/tty
        cursor_position_via_tty()?
    };

    let (terminal_width, terminal_height) = terminal::size()?;

    // Create initial viewport
    let initial_height = 2u16;
    let viewport = if stdout_is_tty {
        Viewport::Inline(initial_height)
    } else {
        // Use Fixed viewport with position from our stderr-based query
        // Ensure we don't overflow past terminal bottom
        let y = initial_cursor_row.min(terminal_height.saturating_sub(initial_height));
        Viewport::Fixed(Rect::new(0, y, terminal_width, initial_height))
    };

    let backend = ratatui::backend::CrosstermBackend::new(stderr());
    let terminal_instance = ratatui::Terminal::with_options(backend, TerminalOptions { viewport })?;
    let mut terminal = Some(terminal_instance);
    let mut last_height = initial_height;

    // Track the fixed viewport row for non-TTY mode
    let fixed_viewport_row = initial_cursor_row;

    loop {
        // Update state
        app.update_animation();
        app.update_transcription_state();

        // Calculate needed height based on transcription length
        let transcription = app.transcription.lock().unwrap().clone();
        let terminal_width = terminal::size()?.0 as usize;

        // Estimate lines needed: transcription + 1 line for status
        let content_length = transcription.len();
        let transcription_lines =
            ((content_length as f32 / terminal_width as f32).ceil() as u16).max(1);
        let needed_height = (transcription_lines + 1).min(10);

        // Recreate terminal if height changed
        if needed_height != last_height {
            if terminal.is_some() {
                terminal::disable_raw_mode()?;
            }

            // Recreate terminal with stderr backend
            let backend = ratatui::backend::CrosstermBackend::new(stderr());
            terminal::enable_raw_mode()?;

            let viewport = if stdout_is_tty {
                Viewport::Inline(needed_height)
            } else {
                // Recalculate Fixed viewport with new height
                let (term_width, term_height) = terminal::size()?;
                let y = fixed_viewport_row.min(term_height.saturating_sub(needed_height));
                Viewport::Fixed(Rect::new(0, y, term_width, needed_height))
            };

            let terminal_instance =
                ratatui::Terminal::with_options(backend, TerminalOptions { viewport })?;
            terminal = Some(terminal_instance);
            last_height = needed_height;
            app.viewport_height = needed_height;
        }

        // Draw inline
        if let Some(ref mut term) = terminal {
            term.draw(|f| {
                let transcription = app.transcription.lock().unwrap().clone();
                let elapsed_since_update =
                    app.transcription_start_time.elapsed().as_millis() as f32;
                let is_ready = app.is_ready.load(Ordering::SeqCst);
                let is_listening = app.is_listening.load(Ordering::SeqCst);
                let transcription_spans = build_transcription_spans(
                    &transcription,
                    elapsed_since_update,
                    app.shimmer_offset,
                    app.animation_start_index,
                    is_ready,
                    is_listening,
                );

                // Split area into transcription and status line
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Min(1),    // Transcription
                        Constraint::Length(1), // Status line
                    ])
                    .split(f.area());

                // Render transcription with spinner at the start
                let is_ready = app.is_ready.load(Ordering::SeqCst);
                let is_listening = app.is_listening.load(Ordering::SeqCst);
                let (spinner, spinner_style) = if !is_ready {
                    // Warming up
                    (
                        WAITING_FRAMES[app.animation_frame],
                        Style::default()
                            .fg(Color::DarkGray)
                            .add_modifier(Modifier::BOLD),
                    )
                } else if is_listening {
                    // Ready and listening - subtle pulsing red dot
                    let pulse_progress = (app.animation_frame as f32
                        / LISTENING_FRAMES.len() as f32)
                        * std::f32::consts::PI;
                    let pulse = (pulse_progress.sin() + 1.0) / 2.0; // 0.0 to 1.0

                    // Subtle pulse - stays mostly bright red with gentle dimming
                    let min_brightness = 200;
                    let max_brightness = 255;
                    let brightness = (min_brightness as f32
                        + pulse * (max_brightness - min_brightness) as f32)
                        as u8;

                    (
                        "●",
                        Style::default()
                            .fg(Color::Rgb(brightness, 0, 0))
                            .add_modifier(Modifier::BOLD),
                    )
                } else {
                    // Not listening
                    ("○", Style::default().fg(Color::DarkGray))
                };

                let mut line_spans = vec![Span::styled(spinner, spinner_style), Span::raw(" ")];
                line_spans.extend(transcription_spans);

                let transcription_para =
                    Paragraph::new(Line::from(line_spans)).wrap(Wrap { trim: false });
                f.render_widget(transcription_para, chunks[0]);

                // Render status line - only show controls when ready
                let status_spans = if is_ready {
                    vec![
                        Span::styled(
                            "Enter",
                            Style::default()
                                .fg(Color::Yellow)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(" finish • ", Style::default().fg(Color::DarkGray)),
                        Span::styled(
                            "Ctrl+R",
                            Style::default()
                                .fg(Color::Blue)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(" restart • ", Style::default().fg(Color::DarkGray)),
                        Span::styled(
                            "Ctrl+C",
                            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(" cancel", Style::default().fg(Color::DarkGray)),
                    ]
                } else {
                    vec![]
                };

                let status_para = Paragraph::new(Line::from(status_spans));
                f.render_widget(status_para, chunks[1]);
            })?;
        }

        if app.should_quit {
            // Clear the viewport before exiting
            if let Some(ref mut term) = terminal {
                term.draw(|_f| {
                    // Draw empty frame to clear viewport
                })?;
            }
            // Disable raw mode
            terminal::disable_raw_mode()?;
            return Ok(());
        }

        // Handle input with timeout
        if event::poll(tick_rate)? {
            if let Event::Key(key) = event::read()? {
                match key {
                    KeyEvent {
                        code: KeyCode::Enter,
                        modifiers: KeyModifiers::NONE,
                        ..
                    } => {
                        app.stop_listening();
                        app.should_quit = true;
                        app.exit_code = 0;
                    }
                    KeyEvent {
                        code: KeyCode::Char('c'),
                        modifiers: KeyModifiers::CONTROL,
                        ..
                    } => {
                        app.stop_listening();
                        app.should_quit = true;
                        app.exit_code = 130; // Standard Ctrl+C exit code
                    }
                    KeyEvent {
                        code: KeyCode::Char('r'),
                        modifiers: KeyModifiers::CONTROL,
                        ..
                    } => {
                        // Restart with fresh recognition session
                        if let Err(e) = app.restart() {
                            eprintln!("Failed to restart: {}", e);
                            app.should_quit = true;
                            app.exit_code = 1;
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

fn build_transcription_spans<'a>(
    transcription: &'a str,
    elapsed_since_update: f32,
    _shimmer_offset: f32,
    animation_start_index: usize,
    is_ready: bool,
    is_listening: bool,
) -> Vec<Span<'a>> {
    if transcription.is_empty() {
        if !is_ready {
            // Just show nothing during warmup, spinner is enough
            return vec![];
        } else if is_listening {
            return vec![Span::styled(
                "Speak now...",
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::ITALIC),
            )];
        } else {
            return vec![];
        }
    }

    let chars: Vec<char> = transcription.chars().collect();
    let mut spans = Vec::new();
    let mut current_word = String::new();
    let mut current_color = Color::White;

    for (i, &ch) in chars.iter().enumerate() {
        let color = if i < animation_start_index {
            // Character is from previous update - already settled (bright white)
            Color::Rgb(255, 255, 255)
        } else {
            // Character is part of the new update - apply animation
            let relative_index = i - animation_start_index;
            let char_appearance_time = relative_index as f32 * CHAR_DELAY_MS;

            if elapsed_since_update < char_appearance_time {
                // Character hasn't appeared yet
                Color::Reset
            } else {
                // Character has appeared, fade from cyan to white over time
                let char_age = elapsed_since_update - char_appearance_time;

                // Fade from cyan to white over 1.5 seconds
                if char_age < 1500.0 {
                    // Progress from 0.0 to 1.0 over fade duration
                    let fade_progress = (char_age / 1500.0).min(1.0);

                    // Smooth ease-out curve for more natural fade
                    let eased_progress = 1.0 - (1.0 - fade_progress).powi(3);

                    // Start color: dim cyan (120, 160, 180)
                    // End color: bright white (255, 255, 255)
                    let start_r = 120.0;
                    let start_g = 160.0;
                    let start_b = 180.0;
                    let end_r = 255.0;
                    let end_g = 255.0;
                    let end_b = 255.0;

                    let r = (start_r + (end_r - start_r) * eased_progress) as u8;
                    let g = (start_g + (end_g - start_g) * eased_progress) as u8;
                    let b = (start_b + (end_b - start_b) * eased_progress) as u8;

                    Color::Rgb(r, g, b)
                } else {
                    // After fade completes, settle to bright white
                    Color::Rgb(255, 255, 255)
                }
            }
        };

        // Skip characters that haven't appeared yet
        if color == Color::Reset {
            continue;
        }

        // Group consecutive characters with same color into spans
        if color != current_color {
            if !current_word.is_empty() {
                spans.push(Span::styled(
                    current_word.clone(),
                    Style::default().fg(current_color),
                ));
                current_word.clear();
            }
            current_color = color;
        }

        current_word.push(ch);
    }

    // Add final span
    if !current_word.is_empty() {
        spans.push(Span::styled(
            current_word,
            Style::default().fg(current_color),
        ));
    }

    spans
}
