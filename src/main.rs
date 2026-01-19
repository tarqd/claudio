//! Claudio - Voice-to-text CLI using macOS Speech framework
//!
//! A TUI application that listens via microphone and transcribes speech in real-time.

use std::{
    io::{self, stderr},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame, Terminal,
};
use tachyonfx::Interpolation;

mod speech;
use speech::SpeechRecognizer;

const MIC_ICON: &str = "🎤";
const LISTENING_FRAMES: [&str; 4] = ["◐", "◓", "◑", "◒"];
const FADE_DURATION_MS: f32 = 800.0;

struct App {
    transcription: Arc<Mutex<String>>,
    previous_transcription_len: usize,
    is_listening: Arc<AtomicBool>,
    should_quit: bool,
    exit_code: i32,
    animation_frame: usize,
    last_frame_time: Instant,
    word_effects: Vec<WordEffect>,
    recognizer: Option<SpeechRecognizer>,
}

struct WordEffect {
    word: String,
    start_idx: usize,
    start_time: Instant,
}

impl App {
    fn new() -> Self {
        Self {
            transcription: Arc::new(Mutex::new(String::new())),
            previous_transcription_len: 0,
            is_listening: Arc::new(AtomicBool::new(false)),
            should_quit: false,
            exit_code: 0,
            animation_frame: 0,
            last_frame_time: Instant::now(),
            word_effects: Vec::new(),
            recognizer: None,
        }
    }

    fn start_listening(&mut self) -> Result<()> {
        let transcription = Arc::clone(&self.transcription);
        let is_listening = Arc::clone(&self.is_listening);

        self.recognizer = Some(SpeechRecognizer::new(transcription, is_listening)?);
        self.recognizer.as_mut().unwrap().start()?;

        Ok(())
    }

    fn stop_listening(&mut self) {
        if let Some(ref mut recognizer) = self.recognizer {
            recognizer.stop();
        }
        self.is_listening.store(false, Ordering::SeqCst);
    }

    fn restart_recording(&mut self) -> Result<()> {
        self.stop_listening();
        {
            let mut transcription = self.transcription.lock().unwrap();
            transcription.clear();
        }
        self.previous_transcription_len = 0;
        self.word_effects.clear();
        self.recognizer = None;
        self.start_listening()
    }

    fn get_final_transcription(&self) -> String {
        self.transcription.lock().unwrap().clone()
    }

    fn update_word_effects(&mut self) {
        let transcription = self.transcription.lock().unwrap().clone();
        let current_len = transcription.len();

        if current_len > self.previous_transcription_len {
            let new_text = &transcription[self.previous_transcription_len..];
            let words: Vec<&str> = new_text.split_whitespace().collect();

            for word in words {
                if let Some(start_idx) = transcription[self.previous_transcription_len..].find(word)
                {
                    let absolute_idx = self.previous_transcription_len + start_idx;
                    self.word_effects.push(WordEffect {
                        word: word.to_string(),
                        start_idx: absolute_idx,
                        start_time: Instant::now(),
                    });
                }
            }
            self.previous_transcription_len = current_len;
        }

        // Remove completed effects (older than fade duration + buffer)
        self.word_effects
            .retain(|w| w.start_time.elapsed() < Duration::from_millis(1000));
    }

    fn update_animation(&mut self) {
        if self.last_frame_time.elapsed() >= Duration::from_millis(150) {
            self.animation_frame = (self.animation_frame + 1) % LISTENING_FRAMES.len();
            self.last_frame_time = Instant::now();
        }
    }
}

fn main() -> Result<()> {
    let mut app = App::new();

    // Set up terminal with stderr
    enable_raw_mode().context("Failed to enable raw mode")?;
    let mut stderr_handle = stderr();
    execute!(stderr_handle, EnterAlternateScreen).context("Failed to enter alternate screen")?;
    let backend = CrosstermBackend::new(stderr_handle);
    let mut terminal = Terminal::new(backend).context("Failed to create terminal")?;

    // Start speech recognition
    if let Err(e) = app.start_listening() {
        cleanup_terminal(&mut terminal)?;
        eprintln!("Failed to start speech recognition: {}", e);
        eprintln!("Make sure you have granted microphone and speech recognition permissions.");
        std::process::exit(1);
    }

    let result = run_app(&mut terminal, &mut app);

    // Restore terminal
    cleanup_terminal(&mut terminal)?;

    match result {
        Ok(()) => {
            if app.exit_code == 0 {
                // Echo transcription to stdout
                let transcription = app.get_final_transcription();
                if !transcription.is_empty() {
                    println!("{}", transcription);
                }
            }
            std::process::exit(app.exit_code);
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}

fn cleanup_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stderr>>) -> Result<()> {
    disable_raw_mode().context("Failed to disable raw mode")?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)
        .context("Failed to leave alternate screen")?;
    terminal.show_cursor().context("Failed to show cursor")?;
    Ok(())
}

fn run_app(terminal: &mut Terminal<CrosstermBackend<io::Stderr>>, app: &mut App) -> Result<()> {
    let tick_rate = Duration::from_millis(33); // ~30 FPS

    loop {
        // Update state
        app.update_animation();
        app.update_word_effects();

        // Draw
        terminal.draw(|f| ui(f, app))?;

        if app.should_quit {
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
                        code: KeyCode::Char('d'),
                        modifiers: KeyModifiers::CONTROL,
                        ..
                    } => {
                        app.restart_recording()?;
                    }
                    _ => {}
                }
            }
        }
    }
}

fn ui(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(2)
        .constraints([
            Constraint::Length(3), // Header with mic icon
            Constraint::Min(5),    // Transcription area
            Constraint::Length(3), // Help text
        ])
        .split(f.area());

    // Header with microphone icon and listening indicator
    let listening_indicator = if app.is_listening.load(Ordering::SeqCst) {
        LISTENING_FRAMES[app.animation_frame]
    } else {
        "○"
    };

    let header_text = format!("{} {} Listening...", MIC_ICON, listening_indicator);
    let header_style = if app.is_listening.load(Ordering::SeqCst) {
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let header = Paragraph::new(header_text)
        .style(header_style)
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
                .title(" Claudio ")
                .title_style(
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
        );
    f.render_widget(header, chunks[0]);

    // Transcription area with fading effect
    let transcription = app.transcription.lock().unwrap().clone();
    let transcription_spans = build_transcription_spans(&transcription, &app.word_effects);

    let transcription_widget = Paragraph::new(Line::from(transcription_spans))
        .wrap(Wrap { trim: false })
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(" Transcription ")
                .title_style(Style::default().fg(Color::White)),
        );
    f.render_widget(transcription_widget, chunks[1]);

    // Help text
    let help_text = Line::from(vec![
        Span::styled(
            "Enter",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" confirm  "),
        Span::styled(
            "Ctrl+C",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" cancel  "),
        Span::styled(
            "Ctrl+D",
            Style::default()
                .fg(Color::Blue)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" restart"),
    ]);

    let help = Paragraph::new(help_text).alignment(Alignment::Center).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray)),
    );
    f.render_widget(help, chunks[2]);
}

fn build_transcription_spans<'a>(
    transcription: &'a str,
    word_effects: &[WordEffect],
) -> Vec<Span<'a>> {
    if transcription.is_empty() {
        return vec![Span::styled(
            "Speak now...",
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::ITALIC),
        )];
    }

    let mut spans = Vec::new();
    let mut last_end = 0;

    // Sort effects by start index for proper rendering
    let mut sorted_effects: Vec<_> = word_effects.iter().collect();
    sorted_effects.sort_by_key(|e| e.start_idx);

    for effect in sorted_effects {
        // Add text before this effect
        if effect.start_idx > last_end && effect.start_idx <= transcription.len() {
            spans.push(Span::styled(
                &transcription[last_end..effect.start_idx],
                Style::default().fg(Color::White),
            ));
        }

        // Calculate fade progress using tachyonfx's easing (CubicOut for smooth fade-in)
        let elapsed = effect.start_time.elapsed().as_millis() as f32;
        let linear_progress = (elapsed / FADE_DURATION_MS).min(1.0);

        // Apply CubicOut easing for a smooth fade-in effect
        let eased_progress = Interpolation::CubicOut.alpha(linear_progress);

        // Interpolate color from dim (50) to bright (255)
        let brightness = (50.0 + eased_progress * 205.0) as u8;
        let color = Color::Rgb(brightness, brightness, brightness);

        let end_idx = (effect.start_idx + effect.word.len()).min(transcription.len());
        if effect.start_idx < transcription.len() {
            spans.push(Span::styled(
                &transcription[effect.start_idx..end_idx],
                Style::default().fg(color),
            ));
            last_end = end_idx;
        }
    }

    // Add remaining text
    if last_end < transcription.len() {
        spans.push(Span::styled(
            &transcription[last_end..],
            Style::default().fg(Color::White),
        ));
    }

    spans
}
