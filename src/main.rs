//! Claudio - Voice-to-text CLI using macOS Speech framework
//!
//! A CLI tool that listens via microphone and transcribes speech in real-time.

use std::{
    env,
    fs,
    io::{stderr, Write},
    process::Command,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

use anyhow::Result;
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{self, Clear, ClearType},
};
use ratatui::{
    layout::{Constraint, Direction, Layout},
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
const SHIMMER_SPEED: f32 = 1.0;  // Speed of the shimmer wave (slower = more subtle)

struct App {
    /// Text that has been edited/finalized (not being transcribed)
    frozen_text: String,
    /// Current recognition session output (shared with speech callback)
    live_transcription: Arc<Mutex<String>>,
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
            frozen_text: String::new(),
            live_transcription: Arc::new(Mutex::new(String::new())),
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
        let transcription = Arc::clone(&self.live_transcription);
        let is_listening = Arc::clone(&self.is_listening);
        let is_ready = Arc::clone(&self.is_ready);

        self.recognizer = Some(SpeechRecognizer::new(transcription, is_listening, is_ready)?);
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

        // Clear both transcription buffers
        self.frozen_text.clear();
        self.live_transcription.lock().unwrap().clear();

        // Reset animation state
        self.previous_transcription_len = 0;
        self.animation_start_index = 0;
        self.transcription_start_time = Instant::now();
        self.is_ready.store(false, Ordering::SeqCst);

        // Start fresh recognition session
        let transcription = Arc::clone(&self.live_transcription);
        let is_listening = Arc::clone(&self.is_listening);
        let is_ready = Arc::clone(&self.is_ready);

        self.recognizer = Some(SpeechRecognizer::new(transcription, is_listening, is_ready)?);
        self.recognizer.as_mut().unwrap().start()?;

        Ok(())
    }

    /// Returns the complete transcription (frozen + live)
    fn full_transcription(&self) -> String {
        let live = self.live_transcription.lock().unwrap();
        format!("{}{}", self.frozen_text, &*live)
    }

    fn get_final_transcription(&self) -> String {
        self.full_transcription()
    }

    fn update_transcription_state(&mut self) {
        // Animation state is relative to live_transcription only
        let live = self.live_transcription.lock().unwrap();
        let current_len = live.chars().count();
        drop(live);

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

    /// Opens the current transcription in $EDITOR for manual editing.
    /// After editing, the edited text becomes frozen and a new recognition session starts.
    fn enter_edit_mode(&mut self) -> Result<()> {
        // 1. Stop current recognition
        self.stop_listening();

        // 2. Combine frozen + live for editing
        let current_text = self.full_transcription();

        // 3. Write to temp file
        let temp_path = env::temp_dir().join("claudio_edit.txt");
        fs::write(&temp_path, &current_text)?;

        // 4. Suspend TUI - disable raw mode and clear the inline viewport area
        terminal::disable_raw_mode()?;
        // Move cursor up to clear our viewport, then clear down
        execute!(
            stderr(),
            cursor::MoveUp(self.viewport_height),
            Clear(ClearType::FromCursorDown)
        )?;

        // 5. Find editor: $VISUAL -> $EDITOR -> vi
        let editor = env::var("VISUAL")
            .or_else(|_| env::var("EDITOR"))
            .unwrap_or_else(|_| "vi".to_string());

        // 6. Spawn editor and wait
        let status = Command::new(&editor).arg(&temp_path).status();

        // 7. Restore TUI
        terminal::enable_raw_mode()?;

        // 8. Handle editor result
        match status {
            Ok(exit_status) if exit_status.success() => {
                // Apply edits - read the edited content
                let edited = fs::read_to_string(&temp_path).unwrap_or(current_text);
                self.frozen_text = edited;
                self.live_transcription.lock().unwrap().clear();
            }
            _ => {
                // Editor cancelled or failed - restore original text to frozen
                self.frozen_text = current_text;
                self.live_transcription.lock().unwrap().clear();
            }
        }

        // 9. Reset animation state for the new live buffer
        self.previous_transcription_len = 0;
        self.animation_start_index = 0;
        self.transcription_start_time = Instant::now();
        self.is_ready.store(false, Ordering::SeqCst);

        // 10. Clean up temp file (ignore errors)
        let _ = fs::remove_file(&temp_path);

        // 11. Restart recognition
        let transcription = Arc::clone(&self.live_transcription);
        let is_listening = Arc::clone(&self.is_listening);
        let is_ready = Arc::clone(&self.is_ready);

        self.recognizer = Some(SpeechRecognizer::new(transcription, is_listening, is_ready)?);
        self.recognizer.as_mut().unwrap().start()?;

        Ok(())
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

    // Use stderr for TUI output
    let backend = ratatui::backend::CrosstermBackend::new(stderr());
    terminal::enable_raw_mode()?;
    let terminal_instance = ratatui::Terminal::with_options(
        backend,
        TerminalOptions {
            viewport: Viewport::Inline(2),
        },
    )?;
    let mut terminal = Some(terminal_instance);
    let mut last_height = 2u16;

    loop {
        // Update state
        app.update_animation();
        app.update_transcription_state();

        // Calculate needed height based on full transcription length (frozen + live)
        let full_transcription = app.full_transcription();
        let terminal_width = terminal::size()?.0 as usize;

        // Calculate lines needed, accounting for both wrapping and explicit newlines
        let transcription_lines: u16 = full_transcription
            .split('\n')
            .map(|line| ((line.len() as f32 / terminal_width as f32).ceil() as u16).max(1))
            .sum();
        let needed_height = (transcription_lines + 1).min(10); // +1 for status line

        // Recreate terminal if height changed
        if needed_height != last_height {
            if terminal.is_some() {
                terminal::disable_raw_mode()?;
            }

            // Recreate terminal with stderr backend
            let backend = ratatui::backend::CrosstermBackend::new(stderr());
            terminal::enable_raw_mode()?;
            let terminal_instance = ratatui::Terminal::with_options(
                backend,
                TerminalOptions {
                    viewport: Viewport::Inline(needed_height),
                },
            )?;
            terminal = Some(terminal_instance);
            last_height = needed_height;
            app.viewport_height = needed_height;
        }

        // Draw inline
        if let Some(ref mut term) = terminal {
            term.draw(|f| {
                let frozen_text = app.frozen_text.clone();
                let live_transcription = app.live_transcription.lock().unwrap().clone();
                let elapsed_since_update = app.transcription_start_time.elapsed().as_millis() as f32;
                let is_ready = app.is_ready.load(Ordering::SeqCst);
                let is_listening = app.is_listening.load(Ordering::SeqCst);

                // Build spans for frozen text (always white/settled)
                let frozen_spans = build_frozen_spans(&frozen_text);

                // Build spans for live transcription (with animation)
                let live_spans = build_transcription_spans(
                    &live_transcription,
                    elapsed_since_update,
                    app.shimmer_offset,
                    app.animation_start_index,
                    is_ready,
                    is_listening,
                    !frozen_text.is_empty(), // has_frozen_text - suppress placeholder if we have frozen text
                );

                // Split area into transcription and status line
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Min(1),     // Transcription
                        Constraint::Length(1),  // Status line
                    ])
                    .split(f.area());

                // Render transcription with spinner at the start
                let is_ready = app.is_ready.load(Ordering::SeqCst);
                let is_listening = app.is_listening.load(Ordering::SeqCst);
                let (spinner, spinner_style) = if !is_ready {
                    // Warming up
                    (WAITING_FRAMES[app.animation_frame],
                     Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD))
                } else if is_listening {
                    // Ready and listening - subtle pulsing red dot
                    let pulse_progress = (app.animation_frame as f32 / LISTENING_FRAMES.len() as f32) * std::f32::consts::PI;
                    let pulse = (pulse_progress.sin() + 1.0) / 2.0; // 0.0 to 1.0

                    // Subtle pulse - stays mostly bright red with gentle dimming
                    let min_brightness = 200;
                    let max_brightness = 255;
                    let brightness = (min_brightness as f32 + pulse * (max_brightness - min_brightness) as f32) as u8;

                    ("●", Style::default().fg(Color::Rgb(brightness, 0, 0)).add_modifier(Modifier::BOLD))
                } else {
                    // Not listening
                    ("○", Style::default().fg(Color::DarkGray))
                };

                let mut line_spans = vec![
                    Span::styled(spinner, spinner_style),
                    Span::raw(" "),
                ];
                line_spans.extend(frozen_spans);
                line_spans.extend(live_spans);

                let transcription_para = Paragraph::new(Line::from(line_spans))
                    .wrap(Wrap { trim: false });
                f.render_widget(transcription_para, chunks[0]);

                // Render status line - only show controls when ready
                let status_spans = if is_ready {
                    vec![
                        Span::styled(
                            "Enter",
                            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(" finish • ", Style::default().fg(Color::DarkGray)),
                        Span::styled(
                            "Ctrl+E",
                            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(" edit • ", Style::default().fg(Color::DarkGray)),
                        Span::styled(
                            "Ctrl+R",
                            Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD),
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
                    KeyEvent {
                        code: KeyCode::Char('e'),
                        modifiers: KeyModifiers::CONTROL,
                        ..
                    } => {
                        // Edit transcription in $EDITOR
                        if let Err(e) = app.enter_edit_mode() {
                            eprintln!("Failed to open editor: {}", e);
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

/// Build spans for frozen (edited) text - always rendered as settled white
fn build_frozen_spans(frozen_text: &str) -> Vec<Span<'_>> {
    if frozen_text.is_empty() {
        return vec![];
    }
    vec![Span::styled(
        frozen_text,
        Style::default().fg(Color::Rgb(255, 255, 255)),
    )]
}

/// Build spans for live transcription with animation
fn build_transcription_spans<'a>(
    transcription: &'a str,
    elapsed_since_update: f32,
    _shimmer_offset: f32,
    animation_start_index: usize,
    is_ready: bool,
    is_listening: bool,
    has_frozen_text: bool,
) -> Vec<Span<'a>> {
    if transcription.is_empty() {
        if !is_ready {
            // Just show nothing during warmup, spinner is enough
            return vec![];
        } else if is_listening && !has_frozen_text {
            // Only show placeholder if there's no frozen text
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
