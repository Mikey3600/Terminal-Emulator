// File: src/grid.rs
// Milestone 2: The Screen Grid — a 2D array of Cells
//
// A terminal screen is fundamentally a grid of characters.
// Each cell holds one character plus its visual attributes.
// This is the data structure that represents what's on screen.

use std::fmt;

/// ANSI color — either one of 8 basic colors, a 256-color index,
/// or a full 24-bit RGB color.
/// Rust enums with data are perfect for this — no null, no magic numbers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Color {
    Default,
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    White,
    Indexed(u8),           // 256-color palette: \x1b[38;5;Nm
    Rgb(u8, u8, u8),       // True color: \x1b[38;2;R;G;Bm
}

/// Visual attributes for a cell.
/// Using a struct of bools is simple and cache-friendly for a grid.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Attributes {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub blink: bool,
    pub reverse: bool,   // swap fg/bg colors
    pub fg: Color,
    pub bg: Color,
}

impl Default for Attributes {
    fn default() -> Self {
        Attributes {
            bold: false,
            italic: false,
            underline: false,
            blink: false,
            reverse: false,
            fg: Color::Default,
            bg: Color::Default,
        }
    }
}

/// A single cell in the terminal grid.
/// Each cell = one character position on screen.
#[derive(Debug, Clone, PartialEq)]
pub struct Cell {
    /// The character displayed in this cell.
    /// Using char (Unicode scalar value) handles UTF-8 correctly.
    pub ch: char,

    /// Visual attributes (color, bold, etc.)
    pub attrs: Attributes,

    /// Whether this cell has changed since last render.
    /// Only redraw cells where dirty = true — huge performance win.
    pub dirty: bool,
}

impl Default for Cell {
    fn default() -> Self {
        Cell {
            ch: ' ',
            attrs: Attributes::default(),
            dirty: false,
        }
    }
}

/// The full terminal screen grid.
/// Internally a flat Vec<Cell> indexed as [row * cols + col].
/// Flat Vec is more cache-friendly than Vec<Vec<Cell>>.
pub struct Grid {
    pub rows: usize,
    pub cols: usize,
    cells: Vec<Cell>,

    /// Cursor position — where the next character will be written
    pub cursor_row: usize,
    pub cursor_col: usize,

    /// Current attributes for newly written characters
    pub current_attrs: Attributes,
}

impl Grid {
    /// Create a new grid filled with empty (space) cells.
    pub fn new(rows: usize, cols: usize) -> Self {
        Grid {
            rows,
            cols,
            cells: vec![Cell::default(); rows * cols],
            cursor_row: 0,
            cursor_col: 0,
            current_attrs: Attributes::default(),
        }
    }

    /// Get an immutable reference to a cell.
    /// Returns None if coordinates are out of bounds.
    pub fn get(&self, row: usize, col: usize) -> Option<&Cell> {
        if row < self.rows && col < self.cols {
            Some(&self.cells[row * self.cols + col])
        } else {
            None
        }
    }

    /// Get a mutable reference to a cell.
    pub fn get_mut(&mut self, row: usize, col: usize) -> Option<&mut Cell> {
        if row < self.rows && col < self.cols {
            Some(&mut self.cells[row * self.cols + col])
        } else {
            None
        }
    }

    /// Write a character at the current cursor position.
    /// Advances cursor. Handles line wrapping.
    pub fn write_char(&mut self, ch: char) {
        let row = self.cursor_row;
        let col = self.cursor_col;
        // Copy attrs before the mutable borrow of self via get_mut,
        // otherwise the borrow checker sees a simultaneous &mut self and &self.
        let attrs = self.current_attrs;

        if let Some(cell) = self.get_mut(row, col) {
            cell.ch = ch;
            cell.attrs = attrs;
            cell.dirty = true;
        }

        // Advance cursor — wrap to next line if at end
        self.cursor_col += 1;
        if self.cursor_col >= self.cols {
            self.cursor_col = 0;
            self.cursor_row += 1;

            // If we've gone past the last row, scroll up
            if self.cursor_row >= self.rows {
                self.scroll_up();
                self.cursor_row = self.rows - 1;
            }
        }
    }

    /// Scroll all rows up by one. The top row is lost.
    /// The bottom row becomes empty spaces.
    pub fn scroll_up(&mut self) {
        for row in 1..self.rows {
            for col in 0..self.cols {
                let src = row * self.cols + col;
                let dst = (row - 1) * self.cols + col;
                self.cells[dst] = self.cells[src].clone();
                self.cells[dst].dirty = true;
            }
        }
        let last_row = self.rows - 1;
        for col in 0..self.cols {
            let idx = last_row * self.cols + col;
            self.cells[idx] = Cell::default();
            self.cells[idx].dirty = true;
        }
    }

    /// Clear the entire screen — reset all cells to default.
    pub fn clear(&mut self) {
        for cell in self.cells.iter_mut() {
            *cell = Cell::default();
            cell.dirty = true;
        }
        self.cursor_row = 0;
        self.cursor_col = 0;
    }

    /// Clear from cursor to end of line.
    /// Cleared cells carry the current background color (ECMA-48 correct).
    pub fn clear_line_from_cursor(&mut self) {
        let row = self.cursor_row;
        let col = self.cursor_col;
        let attrs = self.current_attrs;
        for c in col..self.cols {
            if let Some(cell) = self.get_mut(row, c) {
                cell.ch = ' ';
                // Preserve the current background so colored prompts erase cleanly
                cell.attrs = Attributes {
                    fg: Color::Default,
                    bg: attrs.bg,
                    ..Attributes::default()
                };
                cell.dirty = true;
            }
        }
    }

    /// Move cursor to absolute position. Clamps to grid bounds.
    pub fn move_cursor(&mut self, row: usize, col: usize) {
        self.cursor_row = row.min(self.rows - 1);
        self.cursor_col = col.min(self.cols - 1);
    }

    /// Returns iterator over all dirty cells with their positions.
    /// Use this for efficient rendering — only draw what changed.
    pub fn dirty_cells(&self) -> impl Iterator<Item = (usize, usize, &Cell)> {
        self.cells.iter().enumerate().filter_map(|(idx, cell)| {
            if cell.dirty {
                let row = idx / self.cols;
                let col = idx % self.cols;
                Some((row, col, cell))
            } else {
                None
            }
        })
    }

    /// Mark all cells as clean after rendering.
    pub fn clear_dirty(&mut self) {
        for cell in self.cells.iter_mut() {
            cell.dirty = false;
        }
    }
}

impl fmt::Display for Grid {
    /// Simple text rendering for debugging — ignores colors.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for row in 0..self.rows {
            for col in 0..self.cols {
                let cell = &self.cells[row * self.cols + col];
                write!(f, "{}", cell.ch)?;
            }
            writeln!(f)?;
        }
        Ok(())
    }
}