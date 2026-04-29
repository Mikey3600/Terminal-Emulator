// File: src/parser.rs
// Milestone 3: ANSI Escape Code Parser — a Finite State Machine
//
// The shell sends a stream of bytes mixing two things:
//   - Printable characters (write them to the grid)
//   - Escape sequences (commands: move cursor, set color, clear screen, etc.)
//
// Escape sequences look like: ESC [ <params> <final_byte>
//   ESC = 0x1b
//   [   = the CSI introducer
//   <params> = digits and semicolons, e.g. "1;32"
//   <final_byte> = a letter that says what the command is
//
// Bytes arrive piecemeal — we cannot wait for a complete sequence.
// Solution: a state machine that consumes one byte at a time.

use crate::terminal::screen_buffer::{Attributes, Color, EraseMode, Grid};

/// The parser's current state. Each variant represents a position
/// in the recognition of an escape sequence.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ParserState {
    /// Normal text mode — printable bytes are written to the grid.
    Ground,
    /// Just saw ESC (0x1b) — waiting for the next byte to know what kind.
    Escape,
    /// Saw ESC [ — collecting CSI parameters.
    CsiEntry,
    /// Inside CSI params, accumulating digits and semicolons.
    CsiParam,
    /// Saw ESC ] — Operating System Command, terminated by BEL or ST (ESC \).
    OscString,
    /// Saw ESC inside an OSC string — checking for ST terminator.
    OscEscape,
}

/// The parser wraps state plus a small buffer for accumulating params.
pub struct Parser {
    state: ParserState,
    /// Accumulated parameter bytes for the current CSI sequence.
    params: Vec<u16>,
    /// The number currently being built up from digit bytes.
    current_param: u16,
    /// Whether the current_param has received at least one digit.
    has_current: bool,
}

impl Parser {
    pub fn new() -> Self {
        Parser {
            state: ParserState::Ground,
            params: Vec::with_capacity(8),
            current_param: 0,
            has_current: false,
        }
    }

    /// Feed a slice of bytes into the parser. Mutates the grid in place.
    pub fn feed(&mut self, bytes: &[u8], grid: &mut Grid) {
        for &b in bytes {
            self.step(b, grid);
        }
    }

    /// Process a single byte.
    fn step(&mut self, b: u8, grid: &mut Grid) {
        match self.state {
            ParserState::Ground => self.ground(b, grid),
            ParserState::Escape => self.escape(b, grid),
            ParserState::CsiEntry | ParserState::CsiParam => self.csi(b, grid),
            ParserState::OscString => self.osc(b),
            ParserState::OscEscape => self.osc_escape(b),
        }
    }

    fn ground(&mut self, b: u8, grid: &mut Grid) {
        match b {
            0x1b => self.state = ParserState::Escape, // ESC
            0x07 => {}                                // BEL — ignore (or beep)
            0x08 => {
                // BS — backspace
                if grid.cursor_col > 0 {
                    grid.cursor_col -= 1;
                }
            }
            0x09 => {
                // TAB — advance to next 8-column boundary
                let next = ((grid.cursor_col / 8) + 1) * 8;
                grid.cursor_col = next.min(grid.cols - 1);
            }
            0x0a => {
                // LF — newline
                grid.cursor_row += 1;
                if grid.cursor_row >= grid.rows {
                    grid.scroll_up(1);
                    grid.cursor_row = grid.rows - 1;
                }
            }
            0x0d => grid.cursor_col = 0, // CR — carriage return
            0x20..=0x7e => grid.write_char(b as char), // printable ASCII
            // For UTF-8 multi-byte chars, a real parser would decode here.
            // For Milestone 3 we only handle ASCII. UTF-8 is a future improvement.
            _ => {}
        }
    }

    fn escape(&mut self, b: u8, _grid: &mut Grid) {
        match b {
            b'[' => {
                // Beginning of CSI sequence
                self.state = ParserState::CsiEntry;
                self.params.clear();
                self.current_param = 0;
                self.has_current = false;
            }
            b']' => self.state = ParserState::OscString,
            b'c' => {
                // RIS — full reset (not implemented, return to ground)
                self.state = ParserState::Ground;
            }
            _ => self.state = ParserState::Ground, // unknown, abort
        }
    }

    fn csi(&mut self, b: u8, grid: &mut Grid) {
        match b {
            b'0'..=b'9' => {
                // Accumulate digit into current_param
                self.current_param = self.current_param.saturating_mul(10) + (b - b'0') as u16;
                self.has_current = true;
                self.state = ParserState::CsiParam;
            }
            b';' => {
                // Parameter separator — push current and reset
                self.params.push(if self.has_current {
                    self.current_param
                } else {
                    0
                });
                self.current_param = 0;
                self.has_current = false;
            }
            0x40..=0x7e => {
                // Final byte — execute the command
                if self.has_current {
                    self.params.push(self.current_param);
                }
                self.dispatch_csi(b, grid);
                self.state = ParserState::Ground;
                self.params.clear();
                self.current_param = 0;
                self.has_current = false;
            }
            _ => {
                // Unexpected byte — abort sequence
                self.state = ParserState::Ground;
            }
        }
    }

    fn osc(&mut self, b: u8) {
        // OSC strings end with BEL (0x07) or ST (ESC \).
        // We ignore the contents — typically used for window title.
        match b {
            0x07 => self.state = ParserState::Ground, // BEL terminator
            0x1b => self.state = ParserState::OscEscape, // possible ST
            _ => {}                                   // accumulate (ignored)
        }
    }

    /// Called after seeing ESC inside an OSC string.
    /// If the next byte is '\', this is the String Terminator (ST = ESC \).
    fn osc_escape(&mut self, b: u8) {
        // ESC \ = ST — end of OSC regardless of content
        // Any other byte after ESC inside OSC is malformed; return to ground.
        self.state = ParserState::Ground;
        let _ = b; // '\' or anything else both terminate
    }

    /// Dispatch a completed CSI sequence to the grid.
    /// `final_byte` determines what command this is.
    fn dispatch_csi(&mut self, final_byte: u8, grid: &mut Grid) {
        let p = |i: usize, default: u16| -> u16 { self.params.get(i).copied().unwrap_or(default) };
        match final_byte {
            b'A' => {
                // Cursor Up
                let n = p(0, 1) as usize;
                grid.cursor_row = grid.cursor_row.saturating_sub(n);
            }
            b'B' => {
                // Cursor Down
                let n = p(0, 1) as usize;
                grid.cursor_row = (grid.cursor_row + n).min(grid.rows - 1);
            }
            b'C' => {
                // Cursor Forward
                let n = p(0, 1) as usize;
                grid.cursor_col = (grid.cursor_col + n).min(grid.cols - 1);
            }
            b'D' => {
                // Cursor Back
                let n = p(0, 1) as usize;
                grid.cursor_col = grid.cursor_col.saturating_sub(n);
            }
            b'H' | b'f' => {
                // Cursor Position (1-indexed)
                let row = (p(0, 1) as usize).saturating_sub(1);
                let col = (p(1, 1) as usize).saturating_sub(1);
                grid.move_cursor(row, col);
            }
            b'J' => {
                // Erase in Display
                match p(0, 0) {
                    0 => grid.erase_display(EraseMode::ToEnd),
                    1 => grid.erase_display(EraseMode::ToStart),
                    2 => grid.erase_display(EraseMode::All),
                    _ => {}
                }
            }
            b'K' => {
                // Erase in Line
                if p(0, 0) == 0 {
                    grid.erase_line(EraseMode::ToEnd);
                }
            }
            b'm' => self.apply_sgr(grid),
            _ => {} // unimplemented command — ignore
        }
    }

    /// SGR — Select Graphic Rendition. Applies color and text style.
    fn apply_sgr(&mut self, grid: &mut Grid) {
        if self.params.is_empty() {
            grid.current_attrs = Attributes::default();
            return;
        }
        let mut i = 0;
        while i < self.params.len() {
            let code = self.params[i];
            match code {
                0 => grid.current_attrs = Attributes::default(),
                1 => grid.current_attrs.bold = true,
                3 => grid.current_attrs.italic = true,
                4 => grid.current_attrs.underline = true,
                5 => grid.current_attrs.blink = true,
                7 => grid.current_attrs.reverse = true,
                22 => grid.current_attrs.bold = false,
                23 => grid.current_attrs.italic = false,
                24 => grid.current_attrs.underline = false,
                25 => grid.current_attrs.blink = false,
                27 => grid.current_attrs.reverse = false,
                30..=37 => grid.current_attrs.fg = basic_color(code as u8 - 30),
                39 => grid.current_attrs.fg = Color::Default,
                40..=47 => grid.current_attrs.bg = basic_color(code as u8 - 40),
                49 => grid.current_attrs.bg = Color::Default,
                38 | 48 => {
                    // Extended color: 38;5;N (256-color) or 38;2;R;G;B (true color)
                    let target_fg = code == 38;
                    if let Some(&kind) = self.params.get(i + 1) {
                        if kind == 5 {
                            // 256-color: needs 1 more param
                            if let Some(&n) = self.params.get(i + 2) {
                                let c = Color::Indexed(n as u8);
                                if target_fg {
                                    grid.current_attrs.fg = c;
                                } else {
                                    grid.current_attrs.bg = c;
                                }
                            }
                            i += 2;
                        } else if kind == 2 {
                            // True color: needs 3 more params (R, G, B)
                            // Fixed: was `>= i + 5` which is correct only if
                            // params.len() is 1-indexed; use `> i + 4` instead.
                            if self.params.len() > i + 4 {
                                let r = self.params[i + 2] as u8;
                                let g = self.params[i + 3] as u8;
                                let b = self.params[i + 4] as u8;
                                let c = Color::Rgb(r, g, b);
                                if target_fg {
                                    grid.current_attrs.fg = c;
                                } else {
                                    grid.current_attrs.bg = c;
                                }
                            }
                            i += 4;
                        }
                    }
                }
                _ => {} // unimplemented SGR code
            }
            i += 1;
        }
    }
}

fn basic_color(n: u8) -> Color {
    match n {
        0 => Color::Black,
        1 => Color::Red,
        2 => Color::Green,
        3 => Color::Yellow,
        4 => Color::Blue,
        5 => Color::Magenta,
        6 => Color::Cyan,
        7 => Color::White,
        _ => Color::Default,
    }
}
