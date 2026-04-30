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
///
/// ## Why a struct of bools and not a bitfield?
/// A bitfield would save ~6 bytes per cell. But this struct is 4 bytes total
/// after alignment (2 Colors = 1 byte each as repr, 5 bools = 1 byte each),
/// and the straightforward bool access is branch-predictor friendly.
/// Premature micro-optimization would hurt readability here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Attributes {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub blink: bool,
    /// If true, the renderer should swap fg and bg when drawing.
    /// Used for selection highlights and cursor display.
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
///
/// ## Why `Copy`?
/// `Cell` contains only primitive types (char = u32, bools, u8s).
/// There is no heap allocation, no `String`, no `Vec`. Making it `Copy`
/// lets the compiler pass it by value in registers instead of by pointer,
/// and — critically — lets `scroll_up` use `copy_within` instead of
/// cloning element-by-element through a loop.
///
/// Rule of thumb: if a type is smaller than ~32 bytes and owns no heap
/// memory, `Copy` is almost always correct.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    /// The Unicode scalar value displayed in this cell.
    /// `char` in Rust is always a valid Unicode scalar — no surrogates,
    /// no invalid sequences. Space (`' '`) is the "empty" cell.
    pub ch: char,
    /// Visual style (color, bold, etc.)
    pub attrs: Attributes,
    /// True when this cell is the trailing half of a wide character.
    pub wide_continuation: bool,
    /// True if this cell changed since the last render pass.
    /// The renderer should only repaint dirty cells — skipping clean ones
    /// is the single biggest rendering performance win available.
    pub dirty: bool,
}

impl Default for Cell {
    /// A plain space with default styling. Represents an empty terminal cell.
    fn default() -> Self {
        Cell { ch: ' ', attrs: Attributes::default(), wide_continuation: false, dirty: false }
    }
}

// ─── EraseMode ───────────────────────────────────────────────────────────────

/// Which portion of the screen (or line) to erase.
/// Matches the ECMA-48 / VT100 ED (Erase Display) and EL (Erase Line) params.
///
///```text
/// ED 0 (ToEnd):   cursor position → end of screen
/// ED 1 (ToStart): beginning of screen → cursor position
/// ED 2 (All):     entire screen (cursor does NOT move per ECMA-48)
///```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EraseMode {
    /// From cursor to end (inclusive). ED 0 / EL 0.
    ToEnd,
    /// From start to cursor (inclusive). ED 1 / EL 1.
    ToStart,
    /// Entire screen or line. ED 2 / EL 2.
    All,
}

// ─── Grid ────────────────────────────────────────────────────────────────────

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

/// The full terminal screen — a 2-D grid of [`Cell`]s.
///
/// ## Coordinate system
/// `(row, col)` where `(0, 0)` is the **top-left** corner.
/// Row increases downward, col increases rightward — same as most terminals.
///
/// ## Resize
/// Call [`Grid::resize`] when the terminal window size changes.
/// Content that fits in the new size is preserved; new cells are blank.
pub struct Grid {
    pub rows: usize,
    pub cols: usize,
    /// Flat storage. Index with `row * cols + col`.
    cells: Vec<Cell>,

    /// Row of the text cursor (0-based). Next `write_char` writes here.
    pub cursor_row: usize,
    /// Column of the text cursor (0-based).
    pub cursor_col: usize,

    /// SGR attributes applied to newly written characters.
    /// The VT parser updates this whenever it sees `\x1b[...m` sequences.
    pub current_attrs: Attributes,

    pub scrollback: Scrollback,
}

impl Grid {
    /// Allocate a new grid filled with blank (space) cells.
    /// Cursor starts at (0, 0). All cells start clean (dirty = false).
    ///
    /// # Panics
    /// Panics if `rows == 0` or `cols == 0`. A zero-dimension grid is
    /// nonsensical and would cause underflow in `move_cursor` and others.
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

    // ── Cell access ──────────────────────────────────────────────────────

    /// Immutable reference to the cell at `(row, col)`.
    /// Returns `None` if the coordinates are out of bounds — never panics.
    #[inline]
    pub fn get(&self, row: usize, col: usize) -> Option<&Cell> {
        if row < self.rows && col < self.cols {
            Some(&self.cells[row * self.cols + col])
        } else {
            None
        }
    }

    /// Mutable reference to the cell at `(row, col)`.
    /// Returns `None` if out of bounds.
    #[inline]
    pub fn get_mut(&mut self, row: usize, col: usize) -> Option<&mut Cell> {
        if row < self.rows && col < self.cols {
            Some(&mut self.cells[row * self.cols + col])
        } else {
            None
        }
    }

    // ── Writing ──────────────────────────────────────────────────────────

    /// Write `ch` at the current cursor position with `current_attrs`,
    /// then advance the cursor one column to the right.
    ///
    /// ## Wrapping and scrolling
    /// - When the cursor reaches the last column, it wraps to column 0
    ///   of the next row.
    /// - When the cursor moves past the last row, the grid scrolls up
    ///   one line and the cursor stays on the last row.
    ///
    /// ## Wide characters
    /// This writes `ch` into exactly ONE column. Wide Unicode characters
    /// (CJK, emoji) that visually occupy two columns are NOT handled here.
    /// If you write a wide char, the neighboring cell will show garbage.
    /// For wide-char support, consult the `unicode-width` crate and add
    /// a `Cell::wide` / `Cell::placeholder` distinction.
    ///
    /// ## Control characters
    /// This function does NOT interpret `\r`, `\n`, `\x08` (backspace), etc.
    /// Control characters must be handled by the VT parser *before* calling
    /// this function. Passing a raw `\n` here will write a replacement
    /// character glyph into the cell, not move the cursor down.
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

    // ── Scrolling ─────────────────────────────────────────────────────────

    /// Scroll the grid up by `n` rows.
    ///
    /// - Rows `[n, rows)` move to `[0, rows-n)`.
    /// - The bottom `n` rows are cleared to blank cells.
    /// - All affected cells are marked dirty.
    ///
    /// ## Why `copy_within` instead of a clone loop?
    /// `copy_within` calls `memmove` under the hood — a single optimised
    /// SIMD memory copy. The old code did:
    ///```text
    /// for row in 1..rows { cell[dst] = cell[src].clone(); }  // N clone calls
    ///```
    /// `copy_within` does the same thing in one call and is ~10x faster on
    /// large grids. It works because `Cell: Copy`.
    ///
    /// # Panics
    /// Panics if `n >= self.rows`. Scrolling by the full height would leave
    /// nothing to copy and is almost certainly a caller bug.
    pub fn scroll_up(&mut self, n: usize) {
        assert!(n < self.rows, "scroll_up: n ({n}) must be < rows ({})", self.rows);

        // Store lines leaving the visible region in scrollback.
        for row in 0..n {
            let start = row * self.cols;
            let end = start + self.cols;
            self.scrollback.push_line(self.cells[start..end].to_vec());
        }

        // Shift rows [n..] to [0..rows-n] in one memmove.
        self.cells.copy_within(n * self.cols.., 0);

        // Clear the now-vacated bottom n rows.
        let clear_start = (self.rows - n) * self.cols;
        for cell in self.cells[clear_start..].iter_mut() {
            *cell = Cell::default();
            cell.dirty = true;
        }

        // Mark all moved rows dirty so the renderer repaints them.
        // (Their content changed position, even if the characters didn't change.)
        for cell in self.cells[..clear_start].iter_mut() {
            cell.dirty = true;
        }
    }

    // ── Erasing ───────────────────────────────────────────────────────────

    /// Erase part of the current line.
    ///
    /// Erased cells become spaces and adopt the *current background color*
    /// (not `Color::Default`). This is ECMA-48 correct: if the user has set
    /// a background color with `\x1b[41m`, clearing a line should fill it
    /// with that red background, not the terminal default.
    ///
    /// Cursor does NOT move (matches VT100 EL behaviour).
    pub fn erase_line(&mut self, mode: EraseMode) {
        let row = self.cursor_row;
        let col = self.cursor_col;
        let bg = self.current_attrs.bg;

        // Build a blank cell with the current background color.
        // All other attributes reset — only bg carries through on erase.
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

    /// Erase part of the display (screen).
    ///
    /// Same background-color semantics as [`erase_line`].
    /// Cursor does NOT move per ECMA-48.
    ///
    /// `EraseMode::All` is what most people call "clear screen".
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

        // Compute the flat-index range to blank.
        let (start, end) = match mode {
            EraseMode::ToEnd => (row * self.cols + col, self.rows * self.cols),
            EraseMode::ToStart => (0, row * self.cols + col + 1),
            EraseMode::All => (0, self.rows * self.cols),
        };

        for cell in self.cells[start..end].iter_mut() {
            *cell = blank;
        }
    }

    /// Convenience: erase the entire screen and home the cursor to (0, 0).
    /// This is the common "clear screen" operation — most callers want this
    /// rather than `erase_display(EraseMode::All)` which doesn't move cursor.
    pub fn clear(&mut self) {
        self.erase_display(EraseMode::All);
        self.cursor_row = 0;
        self.cursor_col = 0;
    }

    // ── Cursor movement ───────────────────────────────────────────────────

    /// Move the cursor to an absolute position.
    ///
    /// Coordinates are clamped to `[0, rows-1]` × `[0, cols-1]`.
    /// Clamping is friendlier than panicking — a rogue escape sequence
    /// shouldn't crash the emulator.
    pub fn move_cursor(&mut self, row: usize, col: usize) {
        self.cursor_row = row.min(self.rows - 1);
        self.cursor_col = col.min(self.cols - 1);
    }

    /// Move cursor by a relative delta. Positive = right/down, negative = left/up.
    /// Clamps to grid bounds — does not wrap.
    pub fn move_cursor_relative(&mut self, delta_row: isize, delta_col: isize) {
        let new_row =
            (self.cursor_row as isize + delta_row).clamp(0, self.rows as isize - 1) as usize;
        let new_col =
            (self.cursor_col as isize + delta_col).clamp(0, self.cols as isize - 1) as usize;
        self.cursor_row = new_row;
        self.cursor_col = new_col;
    }

    // ── Dirty tracking ────────────────────────────────────────────────────

    /// Iterator over every cell that changed since the last `clear_dirty()`.
    /// Yields `(row, col, &Cell)` tuples.
    ///
    /// ## Rendering loop pattern
    ///```rust,ignore
    /// for (row, col, cell) in grid.dirty_cells() {
    ///     renderer.draw(row, col, cell);
    /// }
    /// grid.clear_dirty(); // Must call this — otherwise everything is "dirty" forever
    ///```
    ///
    /// `dirty_cells` and `clear_dirty` are always used as a pair.
    /// Not calling `clear_dirty` after rendering means every cell will be
    /// "dirty" on the next frame and you'll repaint the entire screen every
    /// tick — same cost as if you didn't have dirty tracking at all.
    pub fn dirty_cells(&self) -> impl Iterator<Item = (usize, usize, &Cell)> {
        self.cells.iter().enumerate().filter_map(|(idx, cell)| {
            if cell.dirty {
                Some((idx / self.cols, idx % self.cols, cell))
            } else {
                None
            }
        })
    }

    /// Mark all cells as clean. Call this once per frame, after rendering.
    /// See [`dirty_cells`] for the full usage pattern.
    pub fn clear_dirty(&mut self) {
        for cell in self.cells.iter_mut() {
            cell.dirty = false;
        }
    }

    // ── Resize ────────────────────────────────────────────────────────────

    /// Resize the grid to `new_rows × new_cols`.
    ///
    /// Content that fits in both dimensions is preserved in place.
    /// New cells (if growing) are blank. Excess cells (if shrinking) are dropped.
    /// Cursor is clamped to the new bounds.
    ///
    /// ## Why this exists
    /// The terminal window can resize at any time (SIGWINCH). The grid must
    /// match the new dimensions or the VT parser will write out of bounds.
    ///
    /// # Panics
    /// Panics if `new_rows == 0` or `new_cols == 0`.
    pub fn resize(&mut self, new_rows: usize, new_cols: usize) {
        assert!(new_rows > 0, "resize: rows must be > 0");
        assert!(new_cols > 0, "resize: cols must be > 0");

        let mut new_cells = vec![Cell::default(); new_rows * new_cols];

        // Copy the overlap region from old grid to new grid.
        let copy_rows = self.rows.min(new_rows);
        let copy_cols = self.cols.min(new_cols);

        for r in 0..copy_rows {
            for c in 0..copy_cols {
                new_cells[r * new_cols + c] = self.cells[r * self.cols + c];
                // Mark copied cells dirty so the renderer repaints them —
                // they may be at different pixel positions after resize.
                new_cells[r * new_cols + c].dirty = true;
            }
        }

        self.rows = new_rows;
        self.cols = new_cols;
        self.cells = new_cells;

        // Clamp cursor — it may now be out of bounds.
        self.cursor_row = self.cursor_row.min(new_rows - 1);
        self.cursor_col = self.cursor_col.min(new_cols - 1);
    }
}

// ─── Display ─────────────────────────────────────────────────────────────────

impl fmt::Display for Grid {
    /// Plain-text rendering for debugging. Strips all color/attribute info.
    /// Useful with `println!("{}", grid)` or `dbg!` in tests.
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

    // ── Construction ──────────────────────────────────────────────────────

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

    // ── Writing & dirty tracking ──────────────────────────────────────────

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
        let mut g = Grid::new(4, 4); // 4 cols
        for _ in 0..4 {
            g.write_char('x');
        }
        // After 4 writes cursor should wrap to (1, 0)
        assert_eq!(g.cursor_row, 1);
        assert_eq!(g.cursor_col, 0);
    }

    #[test]
    fn write_char_scrolls_when_past_last_row() {
        let mut g = Grid::new(2, 2); // 2 rows, 2 cols
                                     // Fill all 4 cells: (0,0), (0,1), (1,0), (1,1)
        for ch in ['a', 'b', 'c', 'd'] {
            g.write_char(ch);
        }
        // The 5th char should trigger a scroll; cursor should be (1, 1)
        g.write_char('e');
        assert_eq!(g.cursor_row, 1);
        // Row 0 should now have what was in row 1: 'c', 'd'
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

    // ── Scrolling ─────────────────────────────────────────────────────────

    #[test]
    fn scroll_up_moves_rows() {
        let mut g = Grid::new(3, 3);
        // Write a known character on row 1
        g.move_cursor(1, 0);
        g.write_char('X');
        g.scroll_up(1);
        // Row 1 → row 0
        assert_eq!(g.get(0, 0).unwrap().ch, 'X');
        // Bottom row should be blank
        assert_eq!(g.get(2, 0).unwrap().ch, ' ');
    }

    #[test]
    fn scroll_up_pushes_rows_into_scrollback() {
        let mut g = Grid::new(2, 3);
        g.write_char('a');
        g.write_char('b');
        g.write_char('c');
        g.write_char('d');
        g.write_char('e');
        g.write_char('f');

        g.scroll_up(1);

        assert_eq!(g.scrollback.len(), 1);
    }

    #[test]
    #[should_panic(expected = "must be < rows")]
    fn scroll_up_full_height_panics() {
        let mut g = Grid::new(3, 3);
        g.scroll_up(3);
    }

    // ── Erasing ───────────────────────────────────────────────────────────

    #[test]
    fn erase_line_to_end() {
        let mut g = Grid::new(3, 5);
        for ch in ['a', 'b', 'c', 'd', 'e'] {
            g.write_char(ch);
        }
        g.move_cursor(0, 2); // cursor at col 2
        g.erase_line(EraseMode::ToEnd);
        // Cols 0,1 untouched
        assert_eq!(g.get(0, 0).unwrap().ch, 'a');
        assert_eq!(g.get(0, 1).unwrap().ch, 'b');
        // Cols 2+ erased
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

    // ── Cursor movement ───────────────────────────────────────────────────

    #[test]
    fn move_cursor_clamps_to_bounds() {
        let mut g = Grid::new(4, 8);
        g.move_cursor(100, 100);
        assert_eq!(g.cursor_row, 3); // clamped to rows-1
        assert_eq!(g.cursor_col, 7); // clamped to cols-1
    }

    #[test]
    fn move_cursor_relative_clamps() {
        let mut g = Grid::new(4, 8);
        g.move_cursor_relative(-999, -999); // can't go below 0
        assert_eq!(g.cursor_row, 0);
        assert_eq!(g.cursor_col, 0);
    }

    // ── Resize ────────────────────────────────────────────────────────────

    #[test]
    fn resize_preserves_content_in_overlap() {
        let mut g = Grid::new(4, 8);
        g.write_char('Q');
        g.resize(10, 20); // grow
        assert_eq!(g.get(0, 0).unwrap().ch, 'Q', "content should survive resize");
    }

    #[test]
    fn resize_clamps_cursor() {
        let mut g = Grid::new(10, 10);
        g.move_cursor(9, 9);
        g.resize(4, 4); // shrink — cursor was at (9,9), must clamp to (3,3)
        assert_eq!(g.cursor_row, 3);
        assert_eq!(g.cursor_col, 3);
    }

    // ── Bounds ────────────────────────────────────────────────────────────

    #[test]
    fn get_out_of_bounds_returns_none() {
        let g = Grid::new(4, 8);
        assert!(g.get(4, 0).is_none());
        assert!(g.get(0, 8).is_none());
        assert!(g.get(99, 99).is_none());
    }
}
