#![allow(dead_code)]

//! # grid.rs — The Terminal Screen Grid
//!
//! ## Mental model
//! A terminal screen is a rectangular grid of *cells*. Every character
//! position on screen is one cell. Think of it as a 2-D spreadsheet where
//! each cell holds one character and its visual style.
//!
//!```text
//!  col →   0    1    2    3  …  79
//! row 0: ['$'  ' '  'l'  's' …   ]
//! row 1: ['h'  'e'  'l'  'l' …   ]
//!  …
//! row 23: [' '  ' '  …            ]
//!```
//!
//! ## Internal layout
//! We store cells in a *flat* `Vec<Cell>` rather than `Vec<Vec<Cell>>`.
//! A flat vec is one contiguous block of memory — the CPU prefetcher loves
//! it. A vec-of-vecs is heap pointers pointing to scattered allocations —
//! every row access is a potential cache miss.
//!
//! Index formula: `cells[row * cols + col]`
//!
//! ## Dirty-cell rendering
//! Every cell has a `dirty` flag. When you write a character, only that
//! cell is marked dirty. The renderer calls `dirty_cells()` to find only
//! what changed, draws those, then calls `clear_dirty()`. On a typical
//! shell prompt only ~10 cells change per keypress — without dirty tracking
//! you'd redraw all 1920 cells (80×24) every frame.
//!
//! ## Wide character caveat
//! Unicode contains "wide" characters (most CJK glyphs) that occupy two
//! columns. This implementation treats every `char` as one column wide.
//! If you need wide-char support, replace `char` with a `Grapheme` type
//! and add a `Cell::wide` flag. See the `unicode-width` crate.

use std::collections::VecDeque;
use std::fmt;
use unicode_width::UnicodeWidthChar;

// ─── Color ───────────────────────────────────────────────────────────────────

/// ANSI/VT terminal color.
///
/// ## The three systems
///
/// 1. **Named colors** (`Black`..`White`, `Default`):
///    The original 8 ANSI colors. The terminal emulator maps these to
///    actual RGB values — the user can theme them. `Default` means
///    "whatever the terminal's default fg/bg is" — don't hardcode black/white.
///
/// 2. **Indexed(u8)** — 256-color palette:
///    Escape: `\x1b[38;5;Nm` (fg) or `\x1b[48;5;Nm` (bg).
///    - 0–7:   same as the 8 named colors above
///    - 8–15:  bright variants
///    - 16–231: 6×6×6 color cube
///    - 232–255: grayscale ramp
///
/// 3. **Rgb(r, g, b)** — 24-bit "true color":
///    Escape: `\x1b[38;2;R;G;Bm` (fg) or `\x1b[48;2;R;G;Bm` (bg).
///    ~16 million colors. Supported by most modern terminals.
///
/// Using a Rust enum here is idiomatic — no null, no sentinel values,
/// exhaustive matching forces callers to handle every case.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Color {
    /// Terminal's default color — lets the user's theme show through.
    Default,
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    White,
    /// 256-color palette index (0–255).
    Indexed(u8),
    /// 24-bit RGB true color.
    Rgb(u8, u8, u8),
}

// ─── Attributes ──────────────────────────────────────────────────────────────

/// Visual style applied to a single cell.
///
/// Each field corresponds to an SGR (Select Graphic Rendition) escape code:
///   - bold:      `\x1b[1m`
///   - italic:    `\x1b[3m`
///   - underline: `\x1b[4m`
///   - blink:     `\x1b[5m`
///   - reverse:   `\x1b[7m`  — swaps fg and bg colors
///   - fg/bg:     `\x1b[3Xm` / `\x1b[4Xm`
///
/// Reset all: `\x1b[0m` → set this struct back to `Attributes::default()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Attributes {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub blink: bool,
    /// If true, the renderer should swap fg and bg when drawing.
    pub reverse: bool,
    pub fg: Color,
    pub bg: Color,
}

impl Default for Attributes {
    /// Plain text, no decoration, terminal default colors.
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

// ─── Cell ────────────────────────────────────────────────────────────────────

/// One character position on the terminal screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    pub ch: char,
    pub attrs: Attributes,
    pub wide_continuation: bool,
    pub dirty: bool,
}

impl Default for Cell {
    fn default() -> Self {
        Cell { ch: ' ', attrs: Attributes::default(), wide_continuation: false, dirty: false }
    }
}

// ─── EraseMode ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EraseMode {
    ToEnd,
    ToStart,
    All,
}

// ─── Scrollback ──────────────────────────────────────────────────────────────

pub struct Scrollback {
    lines: VecDeque<Vec<Cell>>,
    max_lines: usize,
}

impl Scrollback {
    pub fn new(max_lines: usize) -> Self {
        Self { lines: VecDeque::with_capacity(max_lines), max_lines }
    }

    fn push_line(&mut self, line: Vec<Cell>) {
        self.lines.push_back(line);
        while self.lines.len() > self.max_lines {
            self.lines.pop_front();
        }
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

// ─── Grid ────────────────────────────────────────────────────────────────────

pub struct Grid {
    pub rows: usize,
    pub cols: usize,
    cells: Vec<Cell>,
    pub cursor_row: usize,
    pub cursor_col: usize,
    pub current_attrs: Attributes,
    pub scrollback: Scrollback,
}

impl Grid {
    pub fn new(rows: usize, cols: usize) -> Self {
        assert!(rows > 0, "Grid rows must be > 0");
        assert!(cols > 0, "Grid cols must be > 0");
        Grid {
            rows,
            cols,
            cells: vec![Cell::default(); rows * cols],
            cursor_row: 0,
            cursor_col: 0,
            current_attrs: Attributes::default(),
            scrollback: Scrollback::new(5000),
        }
    }

    #[inline]
    pub fn get(&self, row: usize, col: usize) -> Option<&Cell> {
        if row < self.rows && col < self.cols {
            Some(&self.cells[row * self.cols + col])
        } else {
            None
        }
    }

    #[inline]
    pub fn get_mut(&mut self, row: usize, col: usize) -> Option<&mut Cell> {
        if row < self.rows && col < self.cols {
            Some(&mut self.cells[row * self.cols + col])
        } else {
            None
        }
    }

    pub fn write_char(&mut self, ch: char) {
        let width = UnicodeWidthChar::width(ch).unwrap_or(1);
        if width == 0 {
            return;
        }

        if width == 2 && self.cursor_col + 1 >= self.cols {
            self.cursor_col = 0;
            self.cursor_row += 1;
            if self.cursor_row >= self.rows {
                self.scroll_up(1);
                self.cursor_row = self.rows - 1;
            }
        }

        let row = self.cursor_row;
        let col = self.cursor_col;
        let attrs = self.current_attrs;

        if let Some(cell) = self.get_mut(row, col) {
            cell.ch = ch;
            cell.attrs = attrs;
            cell.wide_continuation = false;
            cell.dirty = true;
        }

        if width == 2 {
            if let Some(next) = self.get_mut(row, col + 1) {
                next.ch = ' ';
                next.attrs = attrs;
                next.wide_continuation = true;
                next.dirty = true;
            }
        }

        self.cursor_col += width;
        if self.cursor_col >= self.cols {
            self.cursor_col = 0;
            self.cursor_row += 1;
            if self.cursor_row >= self.rows {
                self.scroll_up(1);
                self.cursor_row = self.rows - 1;
            }
        }
    }

    pub fn scroll_up(&mut self, n: usize) {
        assert!(n < self.rows, "scroll_up: n ({n}) must be < rows ({})", self.rows);

        // FIX: removed the duplicate second loop that called
        // `self.scrollback.push_back(row)` — `push_back` is a VecDeque
        // method that doesn't exist on `Scrollback`, causing a compile
        // error. The first loop using `push_line` is the correct path and
        // is sufficient; the second loop was pure duplication that slipped
        // in alongside the refactor to the Scrollback wrapper type.
        for row in self.cells.chunks_exact(self.cols).take(n) {
            self.scrollback.push_line(row.to_vec());
        }

        // Shift rows [n..] up to [0..rows-n]
        self.cells.copy_within(n * self.cols.., 0);

        // Clear bottom n rows
        let clear_start = (self.rows - n) * self.cols;
        for cell in self.cells[clear_start..].iter_mut() {
            *cell = Cell::default();
            cell.dirty = true;
        }

        // Mark shifted cells dirty
        for cell in self.cells[..clear_start].iter_mut() {
            cell.dirty = true;
        }
    }

    pub fn erase_line(&mut self, mode: EraseMode) {
        let row = self.cursor_row;
        let col = self.cursor_col;
        let bg = self.current_attrs.bg;

        let blank = Cell {
            ch: ' ',
            attrs: Attributes { bg, ..Attributes::default() },
            dirty: true,
            wide_continuation: false,
        };

        let range = match mode {
            EraseMode::ToEnd => col..self.cols,
            EraseMode::ToStart => 0..col + 1,
            EraseMode::All => 0..self.cols,
        };

        for c in range {
            if let Some(cell) = self.get_mut(row, c) {
                *cell = blank;
            }
        }
    }

    pub fn erase_display(&mut self, mode: EraseMode) {
        let row = self.cursor_row;
        let col = self.cursor_col;
        let bg = self.current_attrs.bg;

        let blank = Cell {
            ch: ' ',
            attrs: Attributes { bg, ..Attributes::default() },
            dirty: true,
            wide_continuation: false,
        };

        let (start, end) = match mode {
            EraseMode::ToEnd => (row * self.cols + col, self.rows * self.cols),
            EraseMode::ToStart => (0, row * self.cols + col + 1),
            EraseMode::All => (0, self.rows * self.cols),
        };

        for cell in self.cells[start..end].iter_mut() {
            *cell = blank;
        }
    }

    pub fn clear(&mut self) {
        self.erase_display(EraseMode::All);
        self.cursor_row = 0;
        self.cursor_col = 0;
    }

    pub fn move_cursor(&mut self, row: usize, col: usize) {
        self.cursor_row = row.min(self.rows - 1);
        self.cursor_col = col.min(self.cols - 1);
    }

    pub fn move_cursor_relative(&mut self, delta_row: isize, delta_col: isize) {
        let new_row =
            (self.cursor_row as isize + delta_row).clamp(0, self.rows as isize - 1) as usize;
        let new_col =
            (self.cursor_col as isize + delta_col).clamp(0, self.cols as isize - 1) as usize;
        self.cursor_row = new_row;
        self.cursor_col = new_col;
    }

    pub fn dirty_cells(&self) -> impl Iterator<Item = (usize, usize, &Cell)> {
        self.cells.iter().enumerate().filter_map(|(idx, cell)| {
            if cell.dirty {
                Some((idx / self.cols, idx % self.cols, cell))
            } else {
                None
            }
        })
    }

    pub fn clear_dirty(&mut self) {
        for cell in self.cells.iter_mut() {
            cell.dirty = false;
        }
    }

    pub fn resize(&mut self, new_rows: usize, new_cols: usize) {
        assert!(new_rows > 0, "resize: rows must be > 0");
        assert!(new_cols > 0, "resize: cols must be > 0");

        let mut new_cells = vec![Cell::default(); new_rows * new_cols];

        let copy_rows = self.rows.min(new_rows);
        let copy_cols = self.cols.min(new_cols);

        for r in 0..copy_rows {
            for c in 0..copy_cols {
                new_cells[r * new_cols + c] = self.cells[r * self.cols + c];
                new_cells[r * new_cols + c].dirty = true;
            }
        }

        self.rows = new_rows;
        self.cols = new_cols;
        self.cells = new_cells;

        self.cursor_row = self.cursor_row.min(new_rows - 1);
        self.cursor_col = self.cursor_col.min(new_cols - 1);
    }
}

// ─── Display ─────────────────────────────────────────────────────────────────

impl fmt::Display for Grid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for row in 0..self.rows {
            for col in 0..self.cols {
                write!(f, "{}", self.cells[row * self.cols + col].ch)?;
            }
            writeln!(f)?;
        }
        Ok(())
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_grid_is_blank() {
        let g = Grid::new(4, 8);
        for row in 0..4 {
            for col in 0..8 {
                let cell = g.get(row, col).unwrap();
                assert_eq!(cell.ch, ' ');
                assert!(!cell.dirty, "new cell should not be dirty");
            }
        }
    }

    #[test]
    #[should_panic(expected = "rows must be > 0")]
    fn new_zero_rows_panics() {
        Grid::new(0, 10);
    }

    #[test]
    #[should_panic(expected = "cols must be > 0")]
    fn new_zero_cols_panics() {
        Grid::new(10, 0);
    }

    #[test]
    fn write_char_sets_cell_and_advances_cursor() {
        let mut g = Grid::new(4, 8);
        g.write_char('A');
        assert_eq!(g.get(0, 0).unwrap().ch, 'A');
        assert!(g.get(0, 0).unwrap().dirty);
        assert_eq!(g.cursor_col, 1);
        assert_eq!(g.cursor_row, 0);
    }

    #[test]
    fn write_char_wraps_at_end_of_line() {
        let mut g = Grid::new(4, 4);
        for _ in 0..4 {
            g.write_char('x');
        }
        assert_eq!(g.cursor_row, 1);
        assert_eq!(g.cursor_col, 0);
    }

    #[test]
    fn write_char_scrolls_when_past_last_row() {
        let mut g = Grid::new(2, 2);
        for ch in ['a', 'b', 'c', 'd'] {
            g.write_char(ch);
        }
        g.write_char('e');
        assert_eq!(g.cursor_row, 1);
        assert_eq!(g.get(0, 0).unwrap().ch, 'c');
    }

    #[test]
    fn clear_dirty_resets_all_flags() {
        let mut g = Grid::new(4, 8);
        g.write_char('Z');
        assert!(g.dirty_cells().count() > 0);
        g.clear_dirty();
        assert_eq!(g.dirty_cells().count(), 0);
    }

    #[test]
    fn scroll_up_moves_rows() {
        let mut g = Grid::new(3, 3);
        g.move_cursor(1, 0);
        g.write_char('X');
        g.scroll_up(1);
        assert_eq!(g.get(0, 0).unwrap().ch, 'X');
        assert_eq!(g.get(2, 0).unwrap().ch, ' ');
    }

    #[test]
    fn scroll_up_pushes_rows_into_scrollback() {
        let mut g = Grid::new(2, 3);
        for ch in ['a', 'b', 'c', 'd', 'e', 'f'] {
            g.write_char(ch);
        }
        g.scroll_up(1);
        assert_eq!(g.scrollback.len(), 1);
    }

    #[test]
    #[should_panic(expected = "must be < rows")]
    fn scroll_up_full_height_panics() {
        let mut g = Grid::new(3, 3);
        g.scroll_up(3);
    }

    #[test]
    fn erase_line_to_end() {
        let mut g = Grid::new(3, 5);
        for ch in ['a', 'b', 'c', 'd', 'e'] {
            g.write_char(ch);
        }
        g.move_cursor(0, 2);
        g.erase_line(EraseMode::ToEnd);
        assert_eq!(g.get(0, 0).unwrap().ch, 'a');
        assert_eq!(g.get(0, 1).unwrap().ch, 'b');
        for c in 2..5 {
            assert_eq!(g.get(0, c).unwrap().ch, ' ', "col {c} should be blank");
        }
    }

    #[test]
    fn erase_line_preserves_current_bg() {
        let mut g = Grid::new(3, 5);
        g.current_attrs.bg = Color::Red;
        g.erase_line(EraseMode::All);
        for c in 0..5 {
            assert_eq!(g.get(0, c).unwrap().attrs.bg, Color::Red);
        }
    }

    #[test]
    fn clear_homes_cursor() {
        let mut g = Grid::new(3, 5);
        g.move_cursor(2, 4);
        g.clear();
        assert_eq!(g.cursor_row, 0);
        assert_eq!(g.cursor_col, 0);
    }

    #[test]
    fn move_cursor_clamps_to_bounds() {
        let mut g = Grid::new(4, 8);
        g.move_cursor(100, 100);
        assert_eq!(g.cursor_row, 3);
        assert_eq!(g.cursor_col, 7);
    }

    #[test]
    fn move_cursor_relative_clamps() {
        let mut g = Grid::new(4, 8);
        g.move_cursor_relative(-999, -999);
        assert_eq!(g.cursor_row, 0);
        assert_eq!(g.cursor_col, 0);
    }

    #[test]
    fn resize_preserves_content_in_overlap() {
        let mut g = Grid::new(4, 8);
        g.write_char('Q');
        g.resize(10, 20);
        assert_eq!(g.get(0, 0).unwrap().ch, 'Q', "content should survive resize");
    }

    #[test]
    fn resize_clamps_cursor() {
        let mut g = Grid::new(10, 10);
        g.move_cursor(9, 9);
        g.resize(4, 4);
        assert_eq!(g.cursor_row, 3);
        assert_eq!(g.cursor_col, 3);
    }

    #[test]
    fn get_out_of_bounds_returns_none() {
        let g = Grid::new(4, 8);
        assert!(g.get(4, 0).is_none());
        assert!(g.get(0, 8).is_none());
        assert!(g.get(99, 99).is_none());
    }
}
