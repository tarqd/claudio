//! Terminal rendering using termwiz
//!
//! Simple inline rendering that works with piped stdout because termwiz
//! uses /dev/tty on Unix and CONIN$/CONOUT$ on Windows.

use anyhow::Result;
use termwiz::cell::AttributeChange;
use termwiz::color::ColorAttribute;
use termwiz::surface::{Change, CursorVisibility, Position};
use termwiz::terminal::Terminal;

const LISTENING_FRAMES: [&str; 4] = ["◐", "◓", "◑", "◒"];
const WAITING_FRAMES: [&str; 12] = ["⠋", "⠙", "⠹", "⠸", "⢰", "⣰", "⣠", "⣄", "⣆", "⡆", "⠇", "⠏"];
const CHAR_DELAY_MS: f32 = 20.0;
const MAX_LINES: usize = 10;

/// Styled text segment
struct Segment {
    text: String,
    color: ColorAttribute,
}

impl Segment {
    fn new(text: impl Into<String>, color: ColorAttribute) -> Self {
        Self { text: text.into(), color }
    }
}

/// Render state for the UI
pub struct RenderState {
    pub rendered_lines: usize,
}

impl Default for RenderState {
    fn default() -> Self {
        Self { rendered_lines: 0 }
    }
}

/// UI state passed to render functions
pub struct UiState<'a> {
    pub transcription: &'a str,
    pub elapsed_ms: f32,
    pub animation_start_index: usize,
    pub animation_frame: usize,
    pub is_ready: bool,
    pub is_listening: bool,
}

/// Hide cursor for rendering
pub fn hide_cursor(term: &mut dyn Terminal) -> Result<()> {
    term.render(&[Change::CursorVisibility(CursorVisibility::Hidden)])
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(())
}

/// Show cursor (for cleanup)
pub fn show_cursor(term: &mut dyn Terminal) -> Result<()> {
    term.render(&[Change::CursorVisibility(CursorVisibility::Visible)])
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(())
}

/// Render the UI inline at current cursor position
pub fn render(
    term: &mut dyn Terminal,
    state: &mut RenderState,
    ui: &UiState,
) -> Result<()> {
    let mut changes = Vec::new();

    // Move cursor up to start of our rendering area
    if state.rendered_lines > 0 {
        changes.push(Change::CursorPosition {
            x: Position::Absolute(0),
            y: Position::Relative(-(state.rendered_lines as isize)),
        });
    }

    let lines = build_lines(ui);

    // Render each line
    let mut total_lines = 0;
    for line in &lines {
        changes.push(Change::ClearToEndOfLine(Default::default()));

        for seg in line {
            changes.push(Change::Attribute(AttributeChange::Foreground(seg.color)));
            changes.push(Change::Text(seg.text.clone()));
        }

        changes.push(Change::Attribute(AttributeChange::Foreground(ColorAttribute::Default)));
        changes.push(Change::Text("\r\n".to_string()));
        total_lines += 1;

        if total_lines >= MAX_LINES {
            break;
        }
    }

    // Clear any leftover lines from previous render
    while total_lines < state.rendered_lines {
        changes.push(Change::ClearToEndOfLine(Default::default()));
        changes.push(Change::Text("\r\n".to_string()));
        total_lines += 1;
    }

    state.rendered_lines = lines.len().min(MAX_LINES);

    // Move cursor back to start for next frame
    if state.rendered_lines > 0 {
        changes.push(Change::CursorPosition {
            x: Position::Absolute(0),
            y: Position::Relative(-(state.rendered_lines as isize)),
        });
    }

    term.render(&changes).map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(())
}

/// Clear rendered lines on exit
pub fn cleanup(term: &mut dyn Terminal, lines: usize) -> Result<()> {
    let mut changes = Vec::new();

    for _ in 0..lines {
        changes.push(Change::ClearToEndOfLine(Default::default()));
        changes.push(Change::CursorPosition {
            x: Position::Absolute(0),
            y: Position::Relative(1),
        });
    }

    // Return to start
    if lines > 0 {
        changes.push(Change::CursorPosition {
            x: Position::Absolute(0),
            y: Position::Relative(-(lines as isize)),
        });
    }

    term.render(&changes).map_err(|e| anyhow::anyhow!("{}", e))?;
    show_cursor(term)?;
    Ok(())
}

fn build_lines(ui: &UiState) -> Vec<Vec<Segment>> {
    let mut lines = Vec::new();

    // Line 1: Spinner + transcription
    let mut line1 = Vec::new();

    // Spinner
    let (spinner, spinner_color) = get_spinner(ui);
    line1.push(Segment::new(spinner, spinner_color));
    line1.push(Segment::new(" ", ColorAttribute::Default));

    // Transcription with animation
    if ui.transcription.is_empty() {
        if ui.is_ready && ui.is_listening {
            line1.push(Segment::new("Speak now...", ColorAttribute::PaletteIndex(8)));
        }
    } else {
        for (i, ch) in ui.transcription.chars().enumerate() {
            if let Some(color) = get_char_color(i, ui.animation_start_index, ui.elapsed_ms) {
                line1.push(Segment::new(ch.to_string(), color));
            }
        }
    }
    lines.push(line1);

    // Line 2: Status bar (only when ready)
    if ui.is_ready {
        let line2 = vec![
            Segment::new("Enter", ColorAttribute::PaletteIndex(3)),
            Segment::new(" finish • ", ColorAttribute::PaletteIndex(8)),
            Segment::new("Ctrl+R", ColorAttribute::PaletteIndex(4)),
            Segment::new(" restart • ", ColorAttribute::PaletteIndex(8)),
            Segment::new("Ctrl+C", ColorAttribute::PaletteIndex(1)),
            Segment::new(" cancel", ColorAttribute::PaletteIndex(8)),
        ];
        lines.push(line2);
    }

    lines
}

fn get_spinner(ui: &UiState) -> (&'static str, ColorAttribute) {
    if !ui.is_ready {
        let frame = ui.animation_frame % WAITING_FRAMES.len();
        (WAITING_FRAMES[frame], ColorAttribute::PaletteIndex(8))
    } else if ui.is_listening {
        // Pulsing red dot
        let pulse = (ui.animation_frame as f32 / LISTENING_FRAMES.len() as f32 * std::f32::consts::PI).sin();
        let brightness = 200 + ((pulse + 1.0) / 2.0 * 55.0) as u8;
        let color = ColorAttribute::TrueColorWithDefaultFallback(
            termwiz::color::SrgbaTuple(brightness as f32 / 255.0, 0.0, 0.0, 1.0).into()
        );
        ("●", color)
    } else {
        ("○", ColorAttribute::PaletteIndex(8))
    }
}

fn get_char_color(index: usize, animation_start: usize, elapsed: f32) -> Option<ColorAttribute> {
    if index < animation_start {
        // Already settled - white
        Some(ColorAttribute::TrueColorWithDefaultFallback(
            termwiz::color::SrgbaTuple(1.0, 1.0, 1.0, 1.0).into()
        ))
    } else {
        let relative = index - animation_start;
        let appear_time = relative as f32 * CHAR_DELAY_MS;

        if elapsed < appear_time {
            None // Not visible yet
        } else {
            let age = elapsed - appear_time;
            let progress = (age / 1500.0).min(1.0);
            let eased = 1.0 - (1.0 - progress).powi(3);

            // Cyan (120, 160, 180) -> White (255, 255, 255)
            let r = (120.0 + 135.0 * eased) / 255.0;
            let g = (160.0 + 95.0 * eased) / 255.0;
            let b = (180.0 + 75.0 * eased) / 255.0;

            Some(ColorAttribute::TrueColorWithDefaultFallback(
                termwiz::color::SrgbaTuple(r, g, b, 1.0).into()
            ))
        }
    }
}
