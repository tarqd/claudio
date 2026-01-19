//! Terminal rendering using termwiz
//!
//! Simple inline rendering that works with piped stdout because termwiz
//! uses /dev/tty on Unix and CONIN$/CONOUT$ on Windows.

use termwiz::cell::{Cell, CellAttributes};
use termwiz::color::ColorAttribute;

use crate::inline_term::InlineSurface;

const LISTENING_FRAMES: [&str; 4] = ["◐", "◓", "◑", "◒"];
const WAITING_FRAMES: [&str; 12] = ["⠋", "⠙", "⠹", "⠸", "⢰", "⣰", "⣠", "⣄", "⣆", "⡆", "⠇", "⠏"];
const CHAR_DELAY_MS: f32 = 20.0;

/// UI state passed to render functions
pub struct UiState<'a> {
    pub transcription: &'a str,
    pub elapsed_ms: f32,
    pub animation_start_index: usize,
    pub animation_frame: usize,
    pub is_ready: bool,
    pub is_listening: bool,
}

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

/// Render the UI to an InlineSurface
pub fn render_to_surface(surface: &mut InlineSurface, ui: &UiState) {
    // Clear the surface first
    surface.clear();

    let lines = build_lines(ui);
    let (width, _) = surface.dimensions();

    for (row, segments) in lines.iter().enumerate() {
        let mut col = 0;
        for seg in segments {
            let attrs = CellAttributes::default()
                .set_foreground(seg.color)
                .clone();

            for ch in seg.text.chars() {
                if col >= width {
                    break;
                }
                surface.set_cell(col, row, Cell::new(ch, attrs.clone()));
                col += 1;
            }
        }
    }
}

/// Returns the number of lines that should be rendered
#[allow(dead_code)]
pub fn line_count(ui: &UiState) -> usize {
    if ui.is_ready {
        2 // spinner line + status bar
    } else {
        1 // just spinner line
    }
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
