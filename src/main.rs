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
    edit_original: String, // Saved text when entering edit mode
}

/// Open text in external editor, returns edited text
fn open_editor(text: &str) -> Result<String> {
    use std::fs;
    use std::io::Read;

    // Create temporary file
    let tmp_dir = env::temp_dir();
    let tmp_path = tmp_dir.join(format!("claudio-{}.txt", std::process::id()));
    fs::write(&tmp_path, text)?;

    // Determine editor: $VISUAL > $EDITOR > platform defaults
    let editor = env::var("VISUAL")
        .or_else(|_| env::var("EDITOR"))
        .unwrap_or_else(|_| {
            if cfg!(target_os = "windows") {
                "notepad".to_string()
            } else {
                "vi".to_string()
            }
        });

    // Open editor
    let status = Command::new(&editor).arg(&tmp_path).status()?;

    if !status.success() {
        fs::remove_file(&tmp_path)?;
        return Err(anyhow::anyhow!("Editor exited with non-zero status"));
    }

    // Read edited content
    let mut file = fs::File::open(&tmp_path)?;
    let mut edited = String::new();
    file.read_to_string(&mut edited)?;

    // Clean up
    fs::remove_file(&tmp_path)?;

    Ok(edited.trim_end().to_string())
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
        self.stop_listening();
        self.transcription.lock().unwrap().clear();
        self.start_time = Instant::now();
        self.is_ready.store(false, Ordering::SeqCst);

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
    term.terminal()
        .set_raw_mode()
        .map_err(|e| anyhow::anyhow!("{}", e))?;

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

        ui.show_placeholder = is_ready && is_listening && ui.is_empty();
        ui.show_controls = is_ready;

        // Update speech text - diff with previous determines animation
        let speech_text = app.transcription.lock().unwrap().clone();
        ui.set_text(&speech_text, elapsed_ms);

        // Check for terminal width resize (debounced)
        term.check_for_resize()?;

        // Skip rendering while resize is settling
        if !term.is_resizing() {
            // Check if we need to resize the surface for wrapping
            let (width, current_height) = term.surface().dimensions();
            let needed_lines = ui.lines_needed(width).min(MAX_LINES);
            if needed_lines != current_height {
                term.resize_height(needed_lines)?;
            }

            // Render UI to surface
            ui.render(term.surface(), elapsed_ms);
            let cursor_pos = ui.cursor_screen_position(width);
            term.render_with_cursor(cursor_pos)?;
        }

        if app.should_quit {
            // Clean up the UI
            term.cleanup()?;
            term.terminal()
                .set_cooked_mode()
                .map_err(|e| anyhow::anyhow!("{}", e))?;

            // Return the final transcription for output
            return Ok(ui.full_text().to_string());
        }

        // Poll input
        if let Some(event) = term
            .terminal()
            .poll_input(Some(tick_rate))
            .map_err(|e| anyhow::anyhow!("{}", e))?
        {
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
        (KeyCode::Char('d'), Modifiers::CTRL) => {
            ui.reset(); // Clear frozen state
            if let Err(e) = app.restart() {
                eprintln!("Failed to restart: {}", e);
                app.should_quit = true;
                app.exit_code = 1;
            }
        }
        (KeyCode::Char('e'), Modifiers::CTRL) => {
            // Enter editing mode
            app.edit_original = ui.full_text().to_string();
            app.stop_listening(); // Pause speech recognition while editing
            ui.start_editing();
        }
        (KeyCode::Char('E'), Modifiers::CTRL | Modifiers::SHIFT) => {
            // Open $EDITOR directly (hidden shortcut)
            app.stop_listening();
            let text = ui.full_text().to_string();
            match open_editor(&text) {
                Ok(edited) => {
                    ui.reset();
                    ui.set_text(&edited, app.start_time.elapsed().as_millis() as f32);
                    app.should_quit = true;
                    app.exit_code = 0;
                }
                Err(e) => {
                    eprintln!("Editor error: {}", e);
                    app.should_quit = true;
                    app.exit_code = 1;
                }
            }
        }
        _ => {}
    }
    Ok(())
}

fn handle_editing_input(app: &mut App, ui: &mut Ui, key: termwiz::input::KeyEvent) -> Result<()> {
    match (key.key, key.modifiers) {
        // Confirm edit
        (KeyCode::Char('s'), Modifiers::CTRL) => {
            // Finish editing and freeze the text (UI manages the buffers)
            ui.finish_editing_with_freeze();
            // Ensure trailing space for separation from new speech
            ui.ensure_trailing_space();
            // Clear the live transcription buffer for new speech
            app.transcription.lock().unwrap().clear();
            // Resume listening
            app.start_listening()?;
        }
        // Escalate to $EDITOR
        (KeyCode::Char('e'), Modifiers::CTRL) => {
            let text = ui.full_text().to_string();
            match open_editor(&text) {
                Ok(edited) => {
                    ui.reset();
                    ui.set_text(&edited, app.start_time.elapsed().as_millis() as f32);
                    ui.finish_editing_with_freeze();
                    ui.ensure_trailing_space();
                    app.transcription.lock().unwrap().clear();
                    app.start_listening()?;
                }
                Err(e) => {
                    eprintln!("Editor error: {}", e);
                    // Stay in edit mode on error
                }
            }
        }
        // Discard edits
        (KeyCode::Char('d'), Modifiers::CTRL) | (KeyCode::Escape, Modifiers::NONE) => {
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
