//! UI components for Claudio's inline terminal display
//!
//! Provides a compositor that renders:
//! - Animated spinner (loading/listening/idle)
//! - Transcribed text with character-by-character fade animation
//! - Editable text mode for corrections
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

/// UI interaction mode
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Listening,
    Editing,
}

/// A keyboard shortcut for the controls bar
struct Control {
    key: &'static str,
    label: &'static str,
    short: &'static str,
    color: u8, // Palette index
}

const CONTROLS_LISTENING: &[Control] = &[
    Control { key: "Enter", label: "finish", short: "fin", color: 3 },
    Control { key: "^E", label: "edit", short: "edt", color: 5 },
    Control { key: "^R", label: "restart", short: "rst", color: 4 },
    Control { key: "^C", label: "cancel", short: "esc", color: 1 },
];

const CONTROLS_EDITING: &[Control] = &[
    Control { key: "Enter", label: "done", short: "done", color: 3 },
    Control { key: "Esc", label: "cancel", short: "esc", color: 1 },
    Control { key: "←→", label: "move", short: "mv", color: 8 },
];

/// Main UI state and renderer
pub struct Ui {
    // Spinner state
    pub spinner_state: SpinnerState,
    spinner_frame: usize,

    // Text state:
    // - frozen_text: from confirmed edits, always white
    // - text: current speech transcription
    // - stable_len: chars that are stable (white, no animation)
    frozen_text: String,
    text: String,
    stable_len: usize,
    animation_start_ms: f32,

    // Editing state
    pub mode: Mode,
    cursor_pos: usize, // Character index (not byte)

    // Visibility flags
    pub show_placeholder: bool,
    pub show_controls: bool,
}

impl Ui {
    pub fn new() -> Self {
        Self {
            spinner_state: SpinnerState::Loading,
            spinner_frame: 0,
            frozen_text: String::new(),
            text: String::new(),
            stable_len: 0,
            animation_start_ms: 0.0,
            mode: Mode::Listening,
            cursor_pos: 0,
            show_placeholder: false,
            show_controls: false,
        }
    }

    /// Advance spinner animation frame
    pub fn tick(&mut self) {
        self.spinner_frame = self.spinner_frame.wrapping_add(1);
    }

    /// Update speech text - compares with current to find stable prefix.
    /// Characters that match current text stay white; changed/new chars animate.
    pub fn set_text(&mut self, text: &str, elapsed_ms: f32) {
        // Only update if not in editing mode
        if self.mode == Mode::Editing {
            return;
        }

        // If text hasn't changed, keep current animation state
        if text == self.text {
            return;
        }

        // Find first differing character between current text and new text
        let common_prefix_len = self.text
            .chars()
            .zip(text.chars())
            .take_while(|(a, b)| a == b)
            .count();

        let new_text_len = text.chars().count();

        // Stable portion = common prefix (text that didn't change)
        // But never decrease stable_len - once stable, stays stable
        let new_stable_len = common_prefix_len.max(self.stable_len.min(new_text_len));

        // Handle animation timing for unstable text
        if new_text_len > new_stable_len {
            if self.text.is_empty() || new_stable_len != self.stable_len {
                // First text or stable boundary changed - start animation now
                self.animation_start_ms = elapsed_ms;
            } else {
                // Compare unstable portions to detect content changes vs extensions
                let old_unstable: String = self.text.chars().skip(self.stable_len).collect();
                let new_unstable: String = text.chars().skip(new_stable_len).collect();

                if new_unstable.starts_with(&old_unstable) {
                    // New text extends old unstable text - adjust timing for new chars
                    let new_chars = new_unstable.chars().count() - old_unstable.chars().count();
                    if new_chars > 0 {
                        self.animation_start_ms -= new_chars as f32 * CHAR_FADE_DELAY_MS;
                    }
                } else {
                    // Unstable portion content changed (correction) - reset animation
                    self.animation_start_ms = elapsed_ms;
                }
            }
        }

        self.stable_len = new_stable_len;
        self.text = text.to_string();
    }

    /// Get the full transcription text (frozen + speech text)
    pub fn full_text(&self) -> String {
        format!("{}{}", self.frozen_text, self.text)
    }

    /// Check if there's any text content
    pub fn is_empty(&self) -> bool {
        self.frozen_text.is_empty() && self.text.is_empty()
    }

    /// Clear transcription and reset animation
    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.frozen_text.clear();
        self.text.clear();
        self.stable_len = 0;
        self.animation_start_ms = 0.0;
        self.cursor_pos = 0;
    }

    /// Full reset (for restart)
    pub fn reset(&mut self) {
        self.frozen_text.clear();
        self.text.clear();
        self.stable_len = 0;
        self.animation_start_ms = 0.0;
        self.cursor_pos = 0;
        self.mode = Mode::Listening;
    }

    // --- Editing mode ---

    /// Enter editing mode - combines all text into frozen for editing
    pub fn start_editing(&mut self) {
        self.mode = Mode::Editing;
        // Combine all text into frozen for editing
        let full = self.full_text();
        self.frozen_text = full;
        self.text.clear();
        self.stable_len = 0;
        self.cursor_pos = self.frozen_text.chars().count(); // Cursor at end
    }

    /// Exit editing mode, keeping changes
    #[allow(dead_code)]
    pub fn finish_editing(&mut self) {
        self.mode = Mode::Listening;
    }

    /// Exit editing mode and freeze the current text (no animation)
    pub fn finish_editing_with_freeze(&mut self) {
        // frozen_text already contains the edited text from start_editing
        self.mode = Mode::Listening;
    }

    /// Ensure frozen text ends with a space (for separation from new speech)
    pub fn ensure_trailing_space(&mut self) {
        if !self.frozen_text.is_empty() && !self.frozen_text.ends_with(' ') {
            self.frozen_text.push(' ');
        }
    }

    /// Exit editing mode, discarding changes
    pub fn cancel_editing(&mut self, original: &str) {
        self.frozen_text = original.to_string();
        self.text.clear();
        self.stable_len = 0;
        self.mode = Mode::Listening;
    }

    /// Move cursor left
    pub fn cursor_left(&mut self) {
        if self.cursor_pos > 0 {
            self.cursor_pos -= 1;
        }
    }

    /// Move cursor right
    pub fn cursor_right(&mut self) {
        let len = self.frozen_text.chars().count();
        if self.cursor_pos < len {
            self.cursor_pos += 1;
        }
    }

    /// Move cursor to start
    pub fn cursor_home(&mut self) {
        self.cursor_pos = 0;
    }

    /// Move cursor to end
    pub fn cursor_end(&mut self) {
        self.cursor_pos = self.frozen_text.chars().count();
    }

    /// Insert character at cursor (editing mode only, modifies frozen_text)
    pub fn insert_char(&mut self, ch: char) {
        let byte_pos = self.char_to_byte_index(self.cursor_pos);
        self.frozen_text.insert(byte_pos, ch);
        self.cursor_pos += 1;
    }

    /// Delete character before cursor (backspace)
    pub fn delete_back(&mut self) {
        if self.cursor_pos > 0 {
            self.cursor_pos -= 1;
            let byte_pos = self.char_to_byte_index(self.cursor_pos);
            let next_byte = self.char_to_byte_index(self.cursor_pos + 1);
            self.frozen_text.drain(byte_pos..next_byte);
        }
    }

    /// Delete character at cursor (delete key)
    pub fn delete_forward(&mut self) {
        let len = self.frozen_text.chars().count();
        if self.cursor_pos < len {
            let byte_pos = self.char_to_byte_index(self.cursor_pos);
            let next_byte = self.char_to_byte_index(self.cursor_pos + 1);
            self.frozen_text.drain(byte_pos..next_byte);
        }
    }

    fn char_to_byte_index(&self, char_idx: usize) -> usize {
        self.frozen_text
            .char_indices()
            .nth(char_idx)
            .map(|(i, _)| i)
            .unwrap_or(self.frozen_text.len())
    }

    // --- Layout ---

    /// Calculate lines needed to display current content
    pub fn lines_needed(&self, width: usize) -> usize {
        if width == 0 {
            return 1;
        }

        // First line has spinner (2 chars), rest are full width
        let first_line_width = width.saturating_sub(2);
        let char_count = self.total_char_count();

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

    /// Total character count (frozen + speech text)
    fn total_char_count(&self) -> usize {
        self.frozen_text.chars().count() + self.text.chars().count()
    }

    // --- Rendering ---

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

        // Render content based on mode
        if self.is_empty() {
            if self.show_placeholder {
                self.render_text(surface, "Speak now...", self.attrs(self.dim_color()), &mut row, &mut col, width, content_rows);
            }
        } else if self.mode == Mode::Editing {
            self.render_editable(surface, &mut row, &mut col, width, content_rows);
        } else {
            self.render_transcription(surface, elapsed_ms, &mut row, &mut col, width, content_rows);
        }

        // Render controls on last row
        if self.show_controls && height > 0 {
            self.render_controls(surface, height - 1, width);
        }
    }

    /// Get cursor position for terminal (if in editing mode)
    pub fn cursor_screen_position(&self, width: usize) -> Option<(usize, usize)> {
        if self.mode != Mode::Editing || width == 0 {
            return None;
        }

        let first_line_width = width.saturating_sub(2);

        if self.cursor_pos < first_line_width {
            // Cursor on first line (after spinner)
            Some((self.cursor_pos + 2, 0))
        } else {
            // Cursor on wrapped line
            let pos_after_first = self.cursor_pos - first_line_width;
            let row = 1 + pos_after_first / width;
            let col = pos_after_first % width;
            Some((col, row))
        }
    }

    fn render_transcription(&self, surface: &mut InlineSurface, elapsed_ms: f32, row: &mut usize, col: &mut usize, width: usize, max_rows: usize) {
        let relative_time = elapsed_ms - self.animation_start_ms;
        let white_attrs = self.attrs(self.white_color());

        // Render frozen text (always white)
        for ch in self.frozen_text.chars() {
            if !self.render_char(surface, ch, white_attrs.clone(), row, col, width, max_rows) {
                return;
            }
        }

        // Render speech text:
        // - chars < stable_len: white (stable, already animated)
        // - chars >= stable_len: animate cyan→white
        for (i, ch) in self.text.chars().enumerate() {
            if i < self.stable_len {
                // Stable character - render white
                if !self.render_char(surface, ch, white_attrs.clone(), row, col, width, max_rows) {
                    return;
                }
            } else {
                // Unstable character - animate
                let anim_index = i - self.stable_len;
                let color = self.char_animation_color(anim_index, relative_time);
                let Some(color) = color else { continue }; // Hidden chars (not visible yet)
                if !self.render_char(surface, ch, self.attrs(color), row, col, width, max_rows) {
                    return;
                }
            }
        }
    }

    /// Render a single character, handling wrapping. Returns false if we've exceeded max_rows.
    fn render_char(&self, surface: &mut InlineSurface, ch: char, attrs: CellAttributes, row: &mut usize, col: &mut usize, width: usize, max_rows: usize) -> bool {
        if *row >= max_rows {
            return false;
        }

        if *col >= width {
            *row += 1;
            *col = 0;
            if *row >= max_rows {
                return false;
            }
        }

        surface.set_cell(*col, *row, Cell::new(ch, attrs));
        *col += 1;
        true
    }

    fn render_editable(&self, surface: &mut InlineSurface, row: &mut usize, col: &mut usize, width: usize, max_rows: usize) {
        // In edit mode, render frozen_text in white (that's where edits happen)
        let attrs = self.attrs(self.white_color());

        for ch in self.frozen_text.chars() {
            if !self.render_char(surface, ch, attrs.clone(), row, col, width, max_rows) {
                return;
            }
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
        let controls = match self.mode {
            Mode::Listening => CONTROLS_LISTENING,
            Mode::Editing => CONTROLS_EDITING,
        };

        // Calculate total width needed for full labels
        let full_width: usize = controls.iter()
            .map(|c| c.key.len() + 1 + c.label.len() + 3) // "Key label • "
            .sum::<usize>().saturating_sub(3); // No separator after last

        // Calculate width for short labels
        let short_width: usize = controls.iter()
            .map(|c| c.key.len() + 1 + c.short.len() + 3)
            .sum::<usize>().saturating_sub(3);

        let use_short = full_width > width && short_width <= width;
        let use_minimal = short_width > width;

        let mut col = 0;

        for (i, ctrl) in controls.iter().enumerate() {
            // Separator
            if i > 0 && col < width {
                let sep = if use_minimal { " " } else { " • " };
                for ch in sep.chars() {
                    if col >= width { break; }
                    surface.set_cell(col, row, Cell::new(ch, self.attrs(self.dim_color())));
                    col += 1;
                }
            }

            // Key
            for ch in ctrl.key.chars() {
                if col >= width { break; }
                surface.set_cell(col, row, Cell::new(ch, self.attrs(ColorAttribute::PaletteIndex(ctrl.color))));
                col += 1;
            }

            // Space + label (unless minimal)
            if !use_minimal {
                if col < width {
                    surface.set_cell(col, row, Cell::new(' ', CellAttributes::default()));
                    col += 1;
                }

                let label = if use_short { ctrl.short } else { ctrl.label };
                for ch in label.chars() {
                    if col >= width { break; }
                    surface.set_cell(col, row, Cell::new(ch, self.attrs(self.dim_color())));
                    col += 1;
                }
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

    /// Calculate color for unsettled text character (animates cyan→white)
    fn char_animation_color(&self, index: usize, relative_time: f32) -> Option<ColorAttribute> {
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

    fn white_color(&self) -> ColorAttribute {
        self.rgb(1.0, 1.0, 1.0)
    }

    fn dim_color(&self) -> ColorAttribute {
        ColorAttribute::PaletteIndex(8)
    }
}

impl Default for Ui {
    fn default() -> Self {
        Self::new()
    }
}
