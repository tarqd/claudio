//! Widget system for inline terminal UI
//!
//! A simple widget system designed for inline rendering that supports:
//! - Dynamic height based on content wrapping
//! - Styled text segments with animations
//! - Composition of multiple widgets vertically

use termwiz::cell::{Cell, CellAttributes};
use termwiz::color::ColorAttribute;

use crate::inline_term::InlineSurface;

/// A styled span of text
#[derive(Clone)]
pub struct Span {
    pub text: String,
    pub style: CellAttributes,
}

impl Span {
    #[allow(dead_code)]
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            style: CellAttributes::default(),
        }
    }

    pub fn styled(text: impl Into<String>, fg: ColorAttribute) -> Self {
        Self {
            text: text.into(),
            style: CellAttributes::default().set_foreground(fg).clone(),
        }
    }

    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.text.chars().count()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }
}

/// A line of styled spans
#[derive(Clone, Default)]
pub struct Line {
    pub spans: Vec<Span>,
}

impl Line {
    pub fn new() -> Self {
        Self { spans: Vec::new() }
    }

    pub fn push(&mut self, span: Span) {
        self.spans.push(span);
    }

    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.spans.iter().map(|s| s.len()).sum()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.spans.is_empty() || self.spans.iter().all(|s| s.is_empty())
    }

    /// Render this line to a surface at the given row
    pub fn render_to(&self, surface: &mut InlineSurface, row: usize) {
        let (width, _) = surface.dimensions();
        let mut col = 0;

        for span in &self.spans {
            for ch in span.text.chars() {
                if col >= width {
                    break;
                }
                surface.set_cell(col, row, Cell::new(ch, span.style.clone()));
                col += 1;
            }
        }
    }
}

/// Spinner widget states
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SpinnerState {
    Loading,
    Listening,
    Idle,
}

const LOADING_FRAMES: [&str; 12] = ["⠋", "⠙", "⠹", "⠸", "⢰", "⣰", "⣠", "⣄", "⣆", "⡆", "⠇", "⠏"];

/// Spinner widget - shows loading, listening, or idle state
pub struct Spinner {
    pub state: SpinnerState,
    pub frame: usize,
}

impl Spinner {
    pub fn new() -> Self {
        Self {
            state: SpinnerState::Loading,
            frame: 0,
        }
    }

    pub fn tick(&mut self) {
        self.frame = self.frame.wrapping_add(1);
    }

    pub fn to_span(&self) -> Span {
        match self.state {
            SpinnerState::Loading => {
                let idx = self.frame % LOADING_FRAMES.len();
                Span::styled(LOADING_FRAMES[idx], ColorAttribute::PaletteIndex(8))
            }
            SpinnerState::Listening => {
                // Pulsing red dot
                let pulse = (self.frame as f32 / 4.0 * std::f32::consts::PI).sin();
                let brightness = 200 + ((pulse + 1.0) / 2.0 * 55.0) as u8;
                let color = ColorAttribute::TrueColorWithDefaultFallback(
                    termwiz::color::SrgbaTuple(brightness as f32 / 255.0, 0.0, 0.0, 1.0).into(),
                );
                Span::styled("●", color)
            }
            SpinnerState::Idle => Span::styled("○", ColorAttribute::PaletteIndex(8)),
        }
    }
}

impl Default for Spinner {
    fn default() -> Self {
        Self::new()
    }
}

/// Character state in transcription
#[derive(Clone)]
pub enum CharState {
    /// Fully settled (white)
    Settled,
    /// Animating from cyan to white
    Animating { age_ms: f32 },
    /// Not yet visible
    Hidden,
}

const CHAR_DELAY_MS: f32 = 20.0;
const SETTLE_DURATION_MS: f32 = 1500.0;

/// Transcription widget - shows text with character-by-character animation
pub struct Transcription {
    pub text: String,
    pub settled_count: usize,
    pub animation_start_ms: f32,
}

impl Transcription {
    pub fn new() -> Self {
        Self {
            text: String::new(),
            settled_count: 0,
            animation_start_ms: 0.0,
        }
    }

    /// Set new text, preserving animation state for existing chars
    pub fn set_text(&mut self, text: String, elapsed_ms: f32) {
        if text.len() > self.text.len() && self.text.is_empty() {
            // First text arriving - start animation
            self.animation_start_ms = elapsed_ms;
        }
        self.text = text;
    }

    /// Mark all current text as settled (for when user confirms)
    #[allow(dead_code)]
    pub fn settle_all(&mut self) {
        self.settled_count = self.text.chars().count();
    }

    /// Get the spans for rendering with current animation state
    pub fn to_spans(&self, elapsed_ms: f32) -> Vec<Span> {
        let mut spans = Vec::new();
        let relative_time = elapsed_ms - self.animation_start_ms;

        for (i, ch) in self.text.chars().enumerate() {
            let state = self.char_state(i, relative_time);
            if let Some(color) = self.state_to_color(&state) {
                spans.push(Span::styled(ch.to_string(), color));
            }
        }

        spans
    }

    fn char_state(&self, index: usize, relative_time: f32) -> CharState {
        if index < self.settled_count {
            CharState::Settled
        } else {
            let char_index = index - self.settled_count;
            let appear_time = char_index as f32 * CHAR_DELAY_MS;

            if relative_time < appear_time {
                CharState::Hidden
            } else {
                CharState::Animating {
                    age_ms: relative_time - appear_time,
                }
            }
        }
    }

    fn state_to_color(&self, state: &CharState) -> Option<ColorAttribute> {
        match state {
            CharState::Settled => Some(ColorAttribute::TrueColorWithDefaultFallback(
                termwiz::color::SrgbaTuple(1.0, 1.0, 1.0, 1.0).into(),
            )),
            CharState::Animating { age_ms } => {
                let progress = (age_ms / SETTLE_DURATION_MS).min(1.0);
                let eased = 1.0 - (1.0 - progress).powi(3);

                // Cyan (120, 160, 180) -> White (255, 255, 255)
                let r = (120.0 + 135.0 * eased) / 255.0;
                let g = (160.0 + 95.0 * eased) / 255.0;
                let b = (180.0 + 75.0 * eased) / 255.0;

                Some(ColorAttribute::TrueColorWithDefaultFallback(
                    termwiz::color::SrgbaTuple(r, g, b, 1.0).into(),
                ))
            }
            CharState::Hidden => None,
        }
    }

    /// Calculate number of lines needed for given width
    pub fn lines_needed(&self, width: usize) -> usize {
        if self.text.is_empty() || width == 0 {
            return 1;
        }
        let char_count = self.text.chars().count();
        // Account for spinner (2 chars: "● ")
        let available = width.saturating_sub(2);
        if available == 0 {
            return char_count;
        }
        (char_count + available - 1) / available
    }
}

impl Default for Transcription {
    fn default() -> Self {
        Self::new()
    }
}

/// Placeholder widget - shows "Speak now..." when idle
pub struct Placeholder {
    pub visible: bool,
}

impl Placeholder {
    pub fn new() -> Self {
        Self { visible: false }
    }

    pub fn to_span(&self) -> Option<Span> {
        if self.visible {
            Some(Span::styled("Speak now...", ColorAttribute::PaletteIndex(8)))
        } else {
            None
        }
    }
}

impl Default for Placeholder {
    fn default() -> Self {
        Self::new()
    }
}

/// Controls widget - shows keyboard shortcuts
pub struct Controls {
    pub visible: bool,
}

impl Controls {
    pub fn new() -> Self {
        Self { visible: false }
    }

    pub fn to_line(&self) -> Option<Line> {
        if !self.visible {
            return None;
        }

        let mut line = Line::new();
        line.push(Span::styled("Enter", ColorAttribute::PaletteIndex(3)));
        line.push(Span::styled(" finish • ", ColorAttribute::PaletteIndex(8)));
        line.push(Span::styled("Ctrl+R", ColorAttribute::PaletteIndex(4)));
        line.push(Span::styled(" restart • ", ColorAttribute::PaletteIndex(8)));
        line.push(Span::styled("Ctrl+C", ColorAttribute::PaletteIndex(1)));
        line.push(Span::styled(" cancel", ColorAttribute::PaletteIndex(8)));
        Some(line)
    }
}

impl Default for Controls {
    fn default() -> Self {
        Self::new()
    }
}

/// Main UI compositor that combines all widgets
pub struct ClaudioUi {
    pub spinner: Spinner,
    pub transcription: Transcription,
    pub placeholder: Placeholder,
    pub controls: Controls,
}

impl ClaudioUi {
    pub fn new() -> Self {
        Self {
            spinner: Spinner::new(),
            transcription: Transcription::new(),
            placeholder: Placeholder::new(),
            controls: Controls::new(),
        }
    }

    /// Calculate the number of lines needed for current content
    pub fn lines_needed(&self, width: usize) -> usize {
        let mut lines = self.transcription.lines_needed(width);
        if self.controls.visible {
            lines += 1;
        }
        lines.max(1)
    }

    /// Render the UI to the surface
    pub fn render(&self, surface: &mut InlineSurface, elapsed_ms: f32) {
        surface.clear();
        let (width, height) = surface.dimensions();

        // Build the first line: spinner + transcription/placeholder
        let mut row = 0;

        // Spinner
        let spinner_span = self.spinner.to_span();
        surface.set_cell(0, row, Cell::new_grapheme(&spinner_span.text, spinner_span.style.clone(), None));
        surface.set_cell(1, row, Cell::new(' ', CellAttributes::default()));

        let mut col = 2; // After spinner and space

        // Content (transcription or placeholder)
        if self.transcription.text.is_empty() {
            // Show placeholder if visible
            if let Some(placeholder_span) = self.placeholder.to_span() {
                for ch in placeholder_span.text.chars() {
                    if col >= width {
                        break;
                    }
                    surface.set_cell(col, row, Cell::new(ch, placeholder_span.style.clone()));
                    col += 1;
                }
            }
        } else {
            // Show transcription with wrapping
            let spans = self.transcription.to_spans(elapsed_ms);
            for span in spans {
                for ch in span.text.chars() {
                    if col >= width {
                        // Wrap to next line
                        row += 1;
                        col = 0;
                        if row >= height {
                            break;
                        }
                    }
                    surface.set_cell(col, row, Cell::new(ch, span.style.clone()));
                    col += 1;
                }
            }
        }

        // Controls on last line
        if self.controls.visible {
            if let Some(controls_line) = self.controls.to_line() {
                let controls_row = height.saturating_sub(1);
                controls_line.render_to(surface, controls_row);
            }
        }
    }

    /// Get the final transcription text (for output to stdout)
    pub fn final_text(&self) -> &str {
        &self.transcription.text
    }
}

impl Default for ClaudioUi {
    fn default() -> Self {
        Self::new()
    }
}
