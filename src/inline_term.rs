//! Inline terminal rendering with efficient diffing
//!
//! Unlike BufferedTerminal which owns the entire screen, InlineSurface
//! renders a fixed-height region at the current cursor position. It supports
//! efficient differential updates without clearing existing terminal content.

use anyhow::Result;
use termwiz::cell::{Cell, CellAttributes};
use termwiz::color::ColorAttribute;
use termwiz::surface::change::Change;
use termwiz::surface::line::Line;
use termwiz::surface::{CursorVisibility, Position};
use termwiz::terminal::Terminal;

/// A surface for inline terminal rendering.
///
/// This maintains an in-memory buffer of a fixed number of lines and tracks
/// changes for efficient differential updates. Unlike a full-screen surface,
/// it uses relative cursor positioning and never clears the screen.
pub struct InlineSurface {
    width: usize,
    height: usize,
    lines: Vec<Line>,
    prev_lines: Vec<Line>,
}

impl InlineSurface {
    /// Create a new inline surface with the given dimensions.
    pub fn new(width: usize, height: usize) -> Self {
        let lines = (0..height).map(|_| Line::with_width(width, 0)).collect();
        let prev_lines = (0..height).map(|_| Line::with_width(width, 0)).collect();
        Self {
            width,
            height,
            lines,
            prev_lines,
        }
    }

    /// Resize the surface. This clears the content.
    pub fn resize(&mut self, width: usize, height: usize) {
        self.width = width;
        self.height = height;
        self.lines = (0..height).map(|_| Line::with_width(width, 0)).collect();
        self.prev_lines = (0..height).map(|_| Line::with_width(width, 0)).collect();
    }

    /// Get dimensions
    pub fn dimensions(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    /// Clear all lines
    pub fn clear(&mut self) {
        for line in &mut self.lines {
            line.fill_range(0..self.width, &Cell::blank(), 0);
        }
    }

    /// Set a cell at a specific position
    pub fn set_cell(&mut self, x: usize, y: usize, cell: Cell) {
        if y < self.height && x < self.width {
            self.lines[y].set_cell(x, cell, 0);
        }
    }

    /// Set text at a position with given attributes
    #[allow(dead_code)]
    pub fn set_text(&mut self, x: usize, y: usize, text: &str, attrs: CellAttributes) {
        if y >= self.height {
            return;
        }
        let mut col = x;
        for ch in text.chars() {
            if col >= self.width {
                break;
            }
            self.lines[y].set_cell(col, Cell::new(ch, attrs.clone()), 0);
            col += 1;
        }
    }

    /// Fill a line from a position to the end with blanks
    #[allow(dead_code)]
    pub fn clear_to_eol(&mut self, x: usize, y: usize) {
        if y < self.height {
            self.lines[y].fill_range(x..self.width, &Cell::blank(), 0);
        }
    }

    /// Compute changes needed to update the terminal from the previous state.
    /// Returns changes that use relative cursor positioning.
    #[allow(dead_code)]
    pub fn get_changes(&self) -> Vec<Change> {
        let mut changes = Vec::new();

        for (row, (line, prev_line)) in self.lines.iter().zip(self.prev_lines.iter()).enumerate() {
            let line_changes = self.diff_line(row, line, prev_line);
            changes.extend(line_changes);
        }

        changes
    }

    /// Get changes for a single line (uses only absolute X positions, no Y)
    pub fn get_line_changes(&self, row: usize) -> Vec<Change> {
        if row >= self.height {
            return Vec::new();
        }
        self.diff_line_x_only(&self.lines[row], &self.prev_lines[row])
    }

    /// Diff a single line, only using X position (no Y positioning)
    fn diff_line_x_only(&self, line: &Line, prev_line: &Line) -> Vec<Change> {
        let mut changes = Vec::new();
        let mut col = 0;
        let mut cursor_col: Option<usize> = None;
        let mut current_attrs: Option<CellAttributes> = None;

        let cells: Vec<_> = line.visible_cells().collect();
        let prev_cells: Vec<_> = prev_line.visible_cells().collect();

        while col < self.width {
            let cell = cells.get(col);
            let prev_cell = prev_cells.get(col);

            // Check if cells differ
            let differs = match (cell, prev_cell) {
                (Some(c), Some(p)) => !c.same_contents(&p),
                (Some(_), None) | (None, Some(_)) => true,
                (None, None) => false,
            };

            if differs {
                if let Some(c) = cell {
                    // Position cursor if needed (only X)
                    if cursor_col != Some(col) {
                        changes.push(Change::CursorPosition {
                            x: Position::Absolute(col),
                            y: Position::Relative(0),
                        });
                    }

                    // Update attributes if needed
                    let cell_attrs = c.attrs();
                    let need_attrs = match &current_attrs {
                        Some(a) => a != cell_attrs,
                        None => *cell_attrs != CellAttributes::default(),
                    };
                    if need_attrs {
                        changes.push(Change::AllAttributes(cell_attrs.clone()));
                        current_attrs = Some(cell_attrs.clone());
                    }

                    // Add text
                    changes.push(Change::Text(c.str().to_string()));
                    cursor_col = Some(col + c.width().max(1));
                }
            }

            col += 1;
        }

        changes
    }

    /// Diff a single line against its previous state (legacy, includes Y)
    #[allow(dead_code)]
    fn diff_line(&self, row: usize, line: &Line, prev_line: &Line) -> Vec<Change> {
        let mut changes = Vec::new();
        let mut col = 0;
        let mut need_position = true;
        let mut current_attrs: Option<CellAttributes> = None;

        let cells: Vec<_> = line.visible_cells().collect();
        let prev_cells: Vec<_> = prev_line.visible_cells().collect();

        while col < self.width {
            let cell = cells.get(col);
            let prev_cell = prev_cells.get(col);

            // Check if cells differ
            let differs = match (cell, prev_cell) {
                (Some(c), Some(p)) => !c.same_contents(&p),
                (Some(_), None) | (None, Some(_)) => true,
                (None, None) => false,
            };

            if differs {
                if let Some(c) = cell {
                    // Need to position cursor
                    if need_position {
                        changes.push(Change::CursorPosition {
                            x: Position::Absolute(col),
                            y: Position::Absolute(row),
                        });
                        need_position = false;
                    }

                    // Update attributes if needed
                    let cell_attrs = c.attrs();
                    let need_attrs = match &current_attrs {
                        Some(a) => a != cell_attrs,
                        None => true,
                    };
                    if need_attrs {
                        changes.push(Change::AllAttributes(cell_attrs.clone()));
                        current_attrs = Some(cell_attrs.clone());
                    }

                    // Add text
                    changes.push(Change::Text(c.str().to_string()));
                }
            } else {
                need_position = true;
            }

            col += 1;
        }

        changes
    }

    /// Commit changes - copy current state to previous state
    pub fn commit(&mut self) {
        self.prev_lines.clone_from(&self.lines);
    }

    /// Force a full repaint on next render
    pub fn invalidate(&mut self) {
        for line in &mut self.prev_lines {
            line.fill_range(0..self.width, &Cell::new('\x00', CellAttributes::default()), 0);
        }
    }

    /// Get a full repaint (all content, no diffing)
    #[allow(dead_code)]
    pub fn get_full_repaint(&self) -> Vec<Change> {
        let mut changes = Vec::new();
        let mut current_attrs: Option<CellAttributes> = None;

        for (row, line) in self.lines.iter().enumerate() {
            changes.push(Change::CursorPosition {
                x: Position::Absolute(0),
                y: Position::Absolute(row),
            });

            for cell in line.visible_cells() {
                let cell_attrs = cell.attrs();
                let need_attrs = match &current_attrs {
                    Some(a) => a != cell_attrs,
                    None => *cell_attrs != CellAttributes::default(),
                };
                if need_attrs {
                    changes.push(Change::AllAttributes(cell_attrs.clone()));
                    current_attrs = Some(cell_attrs.clone());
                }
                changes.push(Change::Text(cell.str().to_string()));
            }
        }

        changes
    }
}

/// Wrapper that manages inline rendering to a terminal
pub struct InlineTerminal<T: Terminal> {
    terminal: T,
    surface: InlineSurface,
    rendered_lines: usize,
}

impl<T: Terminal> InlineTerminal<T> {
    /// Create a new inline terminal with a fixed height
    pub fn new(mut terminal: T, height: usize) -> Result<Self> {
        let size = terminal.get_screen_size().map_err(|e| anyhow::anyhow!("{}", e))?;
        let surface = InlineSurface::new(size.cols, height);
        Ok(Self {
            terminal,
            surface,
            rendered_lines: 0,
        })
    }

    /// Get mutable access to the terminal
    pub fn terminal(&mut self) -> &mut T {
        &mut self.terminal
    }

    /// Get mutable access to the surface
    pub fn surface(&mut self) -> &mut InlineSurface {
        &mut self.surface
    }

    /// Check for terminal resize and update surface width
    pub fn check_for_resize(&mut self) -> Result<bool> {
        let size = self.terminal.get_screen_size().map_err(|e| anyhow::anyhow!("{}", e))?;
        let (width, height) = self.surface.dimensions();
        if width != size.cols {
            self.surface.resize(size.cols, height);
            self.surface.invalidate();
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Resize the height of the inline terminal
    pub fn resize_height(&mut self, new_height: usize) -> Result<()> {
        let (width, _) = self.surface.dimensions();
        self.surface.resize(width, new_height);
        self.surface.invalidate();
        Ok(())
    }

    /// Render the surface to the terminal using line-by-line approach.
    /// This uses relative cursor positioning to work inline.
    pub fn render(&mut self) -> Result<()> {
        let mut changes = Vec::new();

        // Move cursor up to our rendering region if we've rendered before
        if self.rendered_lines > 0 {
            changes.push(Change::CursorPosition {
                x: Position::Absolute(0),
                y: Position::Relative(-(self.rendered_lines as isize)),
            });
        }

        // Hide cursor during render
        changes.push(Change::CursorVisibility(CursorVisibility::Hidden));

        let (_, height) = self.surface.dimensions();

        // Render each line
        for row in 0..height {
            // Get changes for this line only
            let line_changes = self.surface.get_line_changes(row);

            // Position at start of this line (relative from where we are)
            if row > 0 {
                changes.push(Change::CursorPosition {
                    x: Position::Absolute(0),
                    y: Position::Relative(1),
                });
            }

            // Clear the line first
            changes.push(Change::ClearToEndOfLine(ColorAttribute::Default));

            // Apply the line changes (these use absolute X positions)
            changes.extend(line_changes);
        }

        // Move back to start of our region
        if height > 0 {
            changes.push(Change::CursorPosition {
                x: Position::Absolute(0),
                y: Position::Relative(-((height - 1) as isize)),
            });
        }

        // Render to terminal
        self.terminal.render(&changes).map_err(|e| anyhow::anyhow!("{}", e))?;

        // Commit the surface state
        self.surface.commit();
        self.rendered_lines = height;

        Ok(())
    }

    /// Clean up - clear our rendering region and show cursor
    pub fn cleanup(&mut self) -> Result<()> {
        let mut changes = Vec::new();

        // Move to start of our region
        if self.rendered_lines > 0 {
            changes.push(Change::CursorPosition {
                x: Position::Absolute(0),
                y: Position::Relative(-(self.rendered_lines as isize)),
            });
        }

        // Clear each line
        for i in 0..self.rendered_lines {
            changes.push(Change::ClearToEndOfLine(ColorAttribute::Default));
            if i < self.rendered_lines - 1 {
                changes.push(Change::CursorPosition {
                    x: Position::Absolute(0),
                    y: Position::Relative(1),
                });
            }
        }

        // Move back to start
        if self.rendered_lines > 1 {
            changes.push(Change::CursorPosition {
                x: Position::Absolute(0),
                y: Position::Relative(-((self.rendered_lines - 1) as isize)),
            });
        }

        // Show cursor
        changes.push(Change::CursorVisibility(CursorVisibility::Visible));

        self.terminal.render(&changes).map_err(|e| anyhow::anyhow!("{}", e))?;
        self.rendered_lines = 0;

        Ok(())
    }
}
