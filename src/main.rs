//! Claudio - Voice-to-text CLI using native speech recognition
//!
//! A CLI tool that listens via microphone and transcribes speech in real-time.

use std::{
    env,
    io::Write,
    process::Command,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

use anyhow::Result;
use termwiz::caps::Capabilities;
use termwiz::input::{InputEvent, KeyCode, Modifiers};
use termwiz::terminal::{SystemTerminal, Terminal};

mod inline_term;
mod speech;
mod ui;

use inline_term::InlineTerminal;
use speech::SpeechRecognizer;
use ui::{Mode, SpinnerState, Ui};

struct App {
    transcription: Arc<Mutex<String>>,
    is_listening: Arc<AtomicBool>,
    is_ready: Arc<AtomicBool>,
    should_quit: bool,
    exit_code: i32,
    start_time: Instant,
    recognizer: Option<SpeechRecognizer>,
    edit_original: String,  // Saved text when entering edit mode
    frozen_text: String,    // Confirmed text that won't be overwritten or animated
}

impl App {
    fn new() -> Self {
        Self {
            transcription: Arc::new(Mutex::new(String::new())),
            is_listening: Arc::new(AtomicBool::new(false)),
            is_ready: Arc::new(AtomicBool::new(false)),
            should_quit: false,
            exit_code: 0,
            start_time: Instant::now(),
            recognizer: None,
            edit_original: String::new(),
            frozen_text: String::new(),
        }
    }

    fn start_listening(&mut self) -> Result<()> {
        let transcription = Arc::clone(&self.transcription);
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
        self.stop_listening();
        self.transcription.lock().unwrap().clear();
        self.frozen_text.clear();
        self.start_time = Instant::now();
        self.is_ready.store(false, Ordering::SeqCst);

        let transcription = Arc::clone(&self.transcription);
        let is_listening = Arc::clone(&self.is_listening);
        let is_ready = Arc::clone(&self.is_ready);

        self.recognizer = Some(SpeechRecognizer::new(transcription, is_listening, is_ready)?);
        self.recognizer.as_mut().unwrap().start()?;
        Ok(())
    }

    /// Get full transcription (frozen prefix + new speech)
    fn get_transcription(&self) -> String {
        let new_text = self.transcription.lock().unwrap().clone();
        if self.frozen_text.is_empty() {
            new_text
        } else if new_text.is_empty() {
            self.frozen_text.clone()
        } else {
            format!("{} {}", self.frozen_text, new_text)
        }
    }

    /// Get the length of frozen text in characters
    #[allow(dead_code)]
    fn frozen_len(&self) -> usize {
        if self.frozen_text.is_empty() {
            0
        } else {
            // +1 for the space we add between frozen and new text
            self.frozen_text.chars().count() + 1
        }
    }
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    let exec_command = args.iter().position(|a| a == "--").and_then(|pos| {
        if pos + 1 < args.len() {
            Some(args[pos + 1..].to_vec())
        } else {
            None
        }
    });

    let mut app = App::new();

    if let Err(e) = app.start_listening() {
        eprintln!("Failed to start speech recognition: {}", e);
        eprintln!("Make sure you have granted microphone and speech recognition permissions.");
        std::process::exit(1);
    }

    let final_text = run_app(&mut app)?;

    if app.exit_code == 0 && !final_text.is_empty() {
        if let Some(cmd_args) = exec_command {
            let mut child = Command::new(&cmd_args[0])
                .args(&cmd_args[1..])
                .stdin(std::process::Stdio::piped())
                .spawn()?;
            if let Some(mut stdin) = child.stdin.take() {
                stdin.write_all(final_text.as_bytes())?;
            }
            let status = child.wait()?;
            std::process::exit(status.code().unwrap_or(1));
        } else {
            // Print final transcription to stdout
            println!("{}", final_text);
        }
    }

    std::process::exit(app.exit_code);
}

const MIN_LINES: usize = 1;
const MAX_LINES: usize = 10;

fn run_app(app: &mut App) -> Result<String> {
    let tick_rate = Duration::from_millis(33);
    let mut last_tick = Instant::now();

    // termwiz uses /dev/tty on Unix, CONIN$/CONOUT$ on Windows - works with piped stdout
    let caps = Capabilities::new_from_env().map_err(|e| anyhow::anyhow!("{}", e))?;
    let terminal = SystemTerminal::new(caps).map_err(|e| anyhow::anyhow!("{}", e))?;

    // Create inline terminal - starts with minimum height
    let mut term = InlineTerminal::new(terminal, MIN_LINES)?;

    // Raw mode for immediate keys, no alternate screen for inline rendering
    term.terminal().set_raw_mode().map_err(|e| anyhow::anyhow!("{}", e))?;

    // Initialize UI
    let mut ui = Ui::new();

    loop {
        let elapsed_ms = app.start_time.elapsed().as_millis() as f32;

        // Update spinner frame
        if last_tick.elapsed() >= Duration::from_millis(100) {
            ui.tick();
            last_tick = Instant::now();
        }

        // Update UI state from app
        let is_ready = app.is_ready.load(Ordering::SeqCst);
        let is_listening = app.is_listening.load(Ordering::SeqCst);

        ui.spinner_state = if !is_ready {
            SpinnerState::Loading
        } else if is_listening {
            SpinnerState::Listening
        } else {
            SpinnerState::Idle
        };

        ui.show_placeholder = is_ready && is_listening && ui.text().is_empty();
        ui.show_controls = is_ready;

        // Update transcription
        let transcription = app.get_transcription();
        ui.set_text(transcription, elapsed_ms);

        // Check if we need to resize the surface for wrapping
        let (width, current_height) = term.surface().dimensions();
        let needed_lines = ui.lines_needed(width).min(MAX_LINES);
        if needed_lines != current_height {
            term.resize_height(needed_lines)?;
        }

        // Check for terminal width resize
        term.check_for_resize()?;

        // Render UI to surface
        ui.render(term.surface(), elapsed_ms);
        let cursor_pos = ui.cursor_screen_position(width);
        term.render_with_cursor(cursor_pos)?;

        if app.should_quit {
            // Clean up the UI
            term.cleanup()?;
            term.terminal().set_cooked_mode().map_err(|e| anyhow::anyhow!("{}", e))?;

            // Return the final transcription for output
            return Ok(ui.text().to_string());
        }

        // Poll input
        if let Some(event) = term.terminal().poll_input(Some(tick_rate)).map_err(|e| anyhow::anyhow!("{}", e))? {
            handle_input(app, &mut ui, event)?;
        }
    }
}

fn handle_input(app: &mut App, ui: &mut Ui, event: InputEvent) -> Result<()> {
    let InputEvent::Key(key) = event else {
        return Ok(());
    };

    match ui.mode {
        Mode::Listening => handle_listening_input(app, ui, key),
        Mode::Editing => handle_editing_input(app, ui, key),
    }
}

fn handle_listening_input(app: &mut App, ui: &mut Ui, key: termwiz::input::KeyEvent) -> Result<()> {
    match (key.key, key.modifiers) {
        (KeyCode::Enter, Modifiers::NONE) => {
            app.stop_listening();
            app.should_quit = true;
            app.exit_code = 0;
        }
        (KeyCode::Char('c'), Modifiers::CTRL) => {
            app.stop_listening();
            app.should_quit = true;
            app.exit_code = 130;
        }
        (KeyCode::Char('r'), Modifiers::CTRL) => {
            ui.reset(); // Clear frozen state
            if let Err(e) = app.restart() {
                eprintln!("Failed to restart: {}", e);
                app.should_quit = true;
                app.exit_code = 1;
            }
        }
        (KeyCode::Char('e'), Modifiers::CTRL) => {
            // Enter editing mode
            app.edit_original = ui.text().to_string();
            app.stop_listening(); // Pause speech recognition while editing
            ui.start_editing();
        }
        _ => {}
    }
    Ok(())
}

fn handle_editing_input(app: &mut App, ui: &mut Ui, key: termwiz::input::KeyEvent) -> Result<()> {
    match (key.key, key.modifiers) {
        // Confirm edit
        (KeyCode::Enter, Modifiers::NONE) => {
            // Freeze the edited text - it becomes the new prefix
            app.frozen_text = ui.text().to_string();
            // Clear the live transcription buffer for new speech
            app.transcription.lock().unwrap().clear();
            // Tell UI to freeze current text (no animation)
            // +1 for the space we'll add between frozen and new text
            let frozen_len = app.frozen_text.chars().count() + 1;
            ui.finish_editing_with_freeze(frozen_len);
            // Resume listening
            app.start_listening()?;
        }
        // Cancel edit
        (KeyCode::Escape, Modifiers::NONE) => {
            ui.cancel_editing(&app.edit_original);
            // Resume listening
            app.start_listening()?;
        }
        // Navigation
        (KeyCode::LeftArrow, Modifiers::NONE) => ui.cursor_left(),
        (KeyCode::RightArrow, Modifiers::NONE) => ui.cursor_right(),
        (KeyCode::Home, Modifiers::NONE) => ui.cursor_home(),
        (KeyCode::End, Modifiers::NONE) => ui.cursor_end(),
        // Editing
        (KeyCode::Backspace, Modifiers::NONE) => ui.delete_back(),
        (KeyCode::Delete, Modifiers::NONE) => ui.delete_forward(),
        (KeyCode::Char(ch), Modifiers::NONE | Modifiers::SHIFT) => ui.insert_char(ch),
        _ => {}
    }
    Ok(())
}
