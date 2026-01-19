//! UI components for Claudio's inline terminal display
//!
//! Provides a compositor that renders:
//! - Animated spinner (loading/listening/idle)
//! - Transcribed text with character-by-character fade animation
//! - Status bar with keyboard shortcuts

use termwiz::cell::{Cell, CellAttributes};
use termwiz::color::ColorAttribute;

use crate::inline_term::InlineSurface;

// Animation constants
const LOADING_FRAMES: [&str; 12] = ["⠋", "⠙", "⠹", "⠸", "⢰", "⣰", "⣠", "⣄", "⣆", "⡆", "⠇", "⠏"];
const CHAR_FADE_DELAY_MS: f32 = 20.0;
const CHAR_FADE_DURATION_MS: f32 = 1500.0;

/// Spinner display state
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum SpinnerState {
    #[default]
    Loading,
    Listening,
    Idle,
}

/// Main UI state and renderer
pub struct Ui {
    // Spinner state
    pub spinner_state: SpinnerState,
    spinner_frame: usize,

    // Transcription with animation
    text: String,
    animation_start_ms: f32,

    // Visibility flags
    pub show_placeholder: bool,
    pub show_controls: bool,
}

impl Ui {
    pub fn new() -> Self {
        Self {
            spinner_state: SpinnerState::Loading,
            spinner_frame: 0,
            text: String::new(),
            animation_start_ms: 0.0,
            show_placeholder: false,
            show_controls: false,
        }
    }

    /// Advance spinner animation frame
    pub fn tick(&mut self) {
        self.spinner_frame = self.spinner_frame.wrapping_add(1);
    }

    /// Update transcription text, tracking animation timing
    pub fn set_text(&mut self, text: String, elapsed_ms: f32) {
        // Start animation timer when first text arrives
        if !text.is_empty() && self.text.is_empty() {
            self.animation_start_ms = elapsed_ms;
        }
        self.text = text;
    }

    /// Get the transcription text
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Clear transcription and reset animation
    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.text.clear();
        self.animation_start_ms = 0.0;
    }

    /// Calculate lines needed to display current content
    pub fn lines_needed(&self, width: usize) -> usize {
        if width == 0 {
            return 1;
        }

        // First line has spinner (2 chars), rest are full width
        let first_line_width = width.saturating_sub(2);
        let char_count = self.text.chars().count();

        let content_lines = if char_count == 0 || first_line_width == 0 {
            1
        } else if char_count <= first_line_width {
            1
        } else {
            // First line fills, then full-width lines
            let remaining = char_count - first_line_width;
            1 + (remaining + width - 1) / width
        };

        // Add controls line if visible
        if self.show_controls {
            content_lines + 1
        } else {
            content_lines
        }
    }

    /// Render the UI to the surface
    pub fn render(&self, surface: &mut InlineSurface, elapsed_ms: f32) {
        surface.clear();
        let (width, height) = surface.dimensions();
        if width == 0 || height == 0 {
            return;
        }

        let mut row = 0;
        let mut col = 0;

        // Render spinner
        let (spinner_char, spinner_color) = self.spinner_glyph();
        surface.set_cell(col, row, Cell::new_grapheme(spinner_char, self.attrs(spinner_color), None));
        col += 1;
        surface.set_cell(col, row, Cell::new(' ', CellAttributes::default()));
        col += 1;

        // Reserve last row for controls if visible
        let content_rows = if self.show_controls { height.saturating_sub(1) } else { height };

        // Render content (placeholder or transcription)
        if self.text.is_empty() {
            if self.show_placeholder {
                self.render_text(surface, "Speak now...", self.attrs(self.dim_color()), &mut row, &mut col, width, content_rows);
            }
        } else {
            self.render_transcription(surface, elapsed_ms, &mut row, &mut col, width, content_rows);
        }

        // Render controls on last row
        if self.show_controls && height > 0 {
            self.render_controls(surface, height - 1, width);
        }
    }

    fn render_transcription(&self, surface: &mut InlineSurface, elapsed_ms: f32, row: &mut usize, col: &mut usize, width: usize, max_rows: usize) {
        let relative_time = elapsed_ms - self.animation_start_ms;

        for (i, ch) in self.text.chars().enumerate() {
            if *row >= max_rows {
                break;
            }

            // Calculate character color based on animation
            let color = self.char_color(i, relative_time);
            let Some(color) = color else { continue }; // Hidden chars

            // Wrap to next line if needed
            if *col >= width {
                *row += 1;
                *col = 0;
                if *row >= max_rows {
                    break;
                }
            }

            surface.set_cell(*col, *row, Cell::new(ch, self.attrs(color)));
            *col += 1;
        }
    }

    fn render_text(&self, surface: &mut InlineSurface, text: &str, attrs: CellAttributes, row: &mut usize, col: &mut usize, width: usize, max_rows: usize) {
        for ch in text.chars() {
            if *row >= max_rows || *col >= width {
                break;
            }
            surface.set_cell(*col, *row, Cell::new(ch, attrs.clone()));
            *col += 1;
        }
    }

    fn render_controls(&self, surface: &mut InlineSurface, row: usize, width: usize) {
        let controls = [
            ("Enter", self.yellow_color()),
            (" finish • ", self.dim_color()),
            ("Ctrl+R", self.blue_color()),
            (" restart • ", self.dim_color()),
            ("Ctrl+C", self.red_color()),
            (" cancel", self.dim_color()),
        ];

        let mut col = 0;
        for (text, color) in controls {
            for ch in text.chars() {
                if col >= width {
                    return;
                }
                surface.set_cell(col, row, Cell::new(ch, self.attrs(color)));
                col += 1;
            }
        }
    }

    // --- Spinner ---

    fn spinner_glyph(&self) -> (&'static str, ColorAttribute) {
        match self.spinner_state {
            SpinnerState::Loading => {
                let idx = self.spinner_frame % LOADING_FRAMES.len();
                (LOADING_FRAMES[idx], self.dim_color())
            }
            SpinnerState::Listening => {
                let pulse = (self.spinner_frame as f32 / 4.0 * std::f32::consts::PI).sin();
                let brightness = 200.0 + (pulse + 1.0) / 2.0 * 55.0;
                ("●", self.rgb(brightness / 255.0, 0.0, 0.0))
            }
            SpinnerState::Idle => ("○", self.dim_color()),
        }
    }

    // --- Character animation ---

    fn char_color(&self, index: usize, relative_time: f32) -> Option<ColorAttribute> {
        let appear_time = index as f32 * CHAR_FADE_DELAY_MS;

        if relative_time < appear_time {
            return None; // Not visible yet
        }

        let age = relative_time - appear_time;
        let progress = (age / CHAR_FADE_DURATION_MS).min(1.0);
        let eased = 1.0 - (1.0 - progress).powi(3); // ease-out cubic

        // Cyan (120, 160, 180) → White (255, 255, 255)
        let r = (120.0 + 135.0 * eased) / 255.0;
        let g = (160.0 + 95.0 * eased) / 255.0;
        let b = (180.0 + 75.0 * eased) / 255.0;

        Some(self.rgb(r, g, b))
    }

    // --- Color helpers ---

    fn attrs(&self, fg: ColorAttribute) -> CellAttributes {
        CellAttributes::default().set_foreground(fg).clone()
    }

    fn rgb(&self, r: f32, g: f32, b: f32) -> ColorAttribute {
        ColorAttribute::TrueColorWithDefaultFallback(
            termwiz::color::SrgbaTuple(r, g, b, 1.0).into(),
        )
    }

    fn dim_color(&self) -> ColorAttribute {
        ColorAttribute::PaletteIndex(8)
    }

    fn red_color(&self) -> ColorAttribute {
        ColorAttribute::PaletteIndex(1)
    }

    fn yellow_color(&self) -> ColorAttribute {
        ColorAttribute::PaletteIndex(3)
    }

    fn blue_color(&self) -> ColorAttribute {
        ColorAttribute::PaletteIndex(4)
    }
}

impl Default for Ui {
    fn default() -> Self {
        Self::new()
    }
}
