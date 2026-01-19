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
use termwiz::terminal::{buffered::BufferedTerminal, SystemTerminal, Terminal};

mod render;
mod speech;

use render::{RenderState, UiState};
use speech::SpeechRecognizer;

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
        self.previous_transcription_len = 0;
        self.animation_start_index = 0;
        self.transcription_start_time = Instant::now();
        self.is_ready.store(false, Ordering::SeqCst);

        let transcription = Arc::clone(&self.transcription);
        let is_listening = Arc::clone(&self.is_listening);
        let is_ready = Arc::clone(&self.is_ready);

        self.recognizer = Some(SpeechRecognizer::new(transcription, is_listening, is_ready)?);
        self.recognizer.as_mut().unwrap().start()?;
        Ok(())
    }

    fn get_transcription(&self) -> String {
        self.transcription.lock().unwrap().clone()
    }

    fn update(&mut self) {
        // Update animation frame
        if self.last_frame_time.elapsed() >= Duration::from_millis(150) {
            self.animation_frame = self.animation_frame.wrapping_add(1);
            self.last_frame_time = Instant::now();
        }

        // Track new text for animation
        let current_len = self.transcription.lock().unwrap().chars().count();
        if current_len != self.previous_transcription_len {
            self.transcription_start_time = Instant::now();
            self.animation_start_index = self.previous_transcription_len;
            self.previous_transcription_len = current_len;
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

    let exit_code = run_app(&mut app)?;

    if exit_code == 0 {
        let transcription = app.get_transcription();
        if !transcription.is_empty() {
            if let Some(cmd_args) = exec_command {
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
                println!("{}", transcription);
            }
        }
    }

    std::process::exit(exit_code);
}

fn run_app(app: &mut App) -> Result<i32> {
    let tick_rate = Duration::from_millis(33);

    // termwiz uses /dev/tty on Unix, CONIN$/CONOUT$ on Windows - works with piped stdout
    let caps = Capabilities::new_from_env().map_err(|e| anyhow::anyhow!("{}", e))?;
    let terminal = SystemTerminal::new(caps).map_err(|e| anyhow::anyhow!("{}", e))?;
    let mut term = BufferedTerminal::new(terminal).map_err(|e| anyhow::anyhow!("{}", e))?;

    // Raw mode for immediate keys, no alternate screen for inline rendering
    term.terminal().set_raw_mode().map_err(|e| anyhow::anyhow!("{}", e))?;
    render::hide_cursor(&mut term)?;

    let mut render_state = RenderState::default();

    loop {
        app.update();

        // Build UI state (need owned transcription for lifetime)
        let transcription = app.get_transcription();
        let ui = UiState {
            transcription: &transcription,
            elapsed_ms: app.transcription_start_time.elapsed().as_millis() as f32,
            animation_start_index: app.animation_start_index,
            animation_frame: app.animation_frame,
            is_ready: app.is_ready.load(Ordering::SeqCst),
            is_listening: app.is_listening.load(Ordering::SeqCst),
        };

        render::render(&mut term, &mut render_state, &ui)?;

        if app.should_quit {
            render::cleanup(&mut term, render_state.rendered_lines)?;
            term.terminal().set_cooked_mode().map_err(|e| anyhow::anyhow!("{}", e))?;
            return Ok(app.exit_code);
        }

        // Poll input
        if let Some(event) = term.terminal().poll_input(Some(tick_rate)).map_err(|e| anyhow::anyhow!("{}", e))? {
            handle_input(app, event)?;
        }
    }
}

fn handle_input(app: &mut App, event: InputEvent) -> Result<()> {
    if let InputEvent::Key(key) = event {
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
                if let Err(e) = app.restart() {
                    eprintln!("Failed to restart: {}", e);
                    app.should_quit = true;
                    app.exit_code = 1;
                }
            }
            _ => {}
        }
    }
    Ok(())
}
