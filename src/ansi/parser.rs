use crate::terminal::screen_buffer::{Attributes, Color, EraseMode, Grid};
use unicode_segmentation::UnicodeSegmentation;

#[derive(Debug, Clone, Copy)]
pub struct AnsiCapabilities {
    pub ansi_core: bool,
    pub xterm_extended: bool,
    pub osc: bool,
}
impl Default for AnsiCapabilities {
    fn default() -> Self {
        Self { ansi_core: true, xterm_extended: true, osc: true }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ParserState {
    Ground,
    Escape,
    Csi,
    Osc,
    OscEscape,
}

/// Minimal ANSI/VT escape sequence parser used by the terminal grid.
///
/// The parser is byte-oriented and stateful:
/// - [`ParserState::Ground`]: prints plain text and handles simple control bytes.
/// - [`ParserState::Escape`]: reads `ESC`-prefixed sequences.
/// - [`ParserState::Csi`]: collects CSI parameters and dispatches final bytes.
/// - [`ParserState::Osc`]/[`ParserState::OscEscape`]: consumes OSC until terminated.
///
/// Capabilities are feature-gated through [`AnsiCapabilities`], allowing
/// selected protocol families to be enabled/disabled at runtime.
pub struct Parser {
    state: ParserState,
    params: Vec<u16>,
    current: u16,
    seen_digit: bool,
    utf8_buf: Vec<u8>,
    caps: AnsiCapabilities,
    saved_cursor: (usize, usize),
    // FIX #5: track whether CSI has a private '?' prefix
    csi_private: bool,
    // FIX #3: store OSC window title
    osc_buf: Vec<u8>,
    pub osc_title: Option<String>,
}

impl Parser {
    /// Creates a parser configured with the provided ANSI capabilities.
    pub fn new(caps: AnsiCapabilities) -> Self {
        Self {
            state: ParserState::Ground,
            params: Vec::with_capacity(8),
            current: 0,
            seen_digit: false,
            utf8_buf: Vec::with_capacity(4),
            caps,
            saved_cursor: (0, 0),
            csi_private: false,
            osc_buf: Vec::new(),
            osc_title: None,
        }
    }
    /// Feeds a chunk of bytes into the parser and applies effects to `grid`.
    ///
    /// This method may be called repeatedly with partial escape sequences; parser
    /// state is preserved between calls.
    pub fn feed(&mut self, bytes: &[u8], grid: &mut Grid) {
        log::debug!("parser_feed_bytes={}", bytes.len());
        for &b in bytes {
            self.step(b, grid);
        }
    }
    fn step(&mut self, b: u8, grid: &mut Grid) {
        log::trace!("parser_state={:?} byte={}", self.state, b);
        match self.state {
            ParserState::Ground => self.ground(b, grid),
            ParserState::Escape => self.escape(b),
            ParserState::Csi => self.csi(b, grid),
            ParserState::Osc => self.osc(b),
            ParserState::OscEscape => {
                // FIX #3: finalize OSC title on ESC \ terminator
                self.finalize_osc();
                self.state = ParserState::Ground;
                let _ = b;
            }
        }
    }
    fn ground(&mut self, b: u8, grid: &mut Grid) {
        match b {
            0x1b => {
                // FIX #4: clear partial UTF-8 buffer when leaving Ground
                self.utf8_buf.clear();
                self.state = ParserState::Escape;
            }
            0x08 => grid.cursor_col = grid.cursor_col.saturating_sub(1),
            0x09 => grid.cursor_col = (((grid.cursor_col / 8) + 1) * 8).min(grid.cols - 1),
            0x0a => {
                if grid.cursor_row + 1 >= grid.rows {
                    grid.scroll_up(1);
                    grid.cursor_row = grid.rows - 1;
                } else {
                    grid.cursor_row += 1;
                }
            }
            0x0d => grid.cursor_col = 0,
            0x20..=0x7e => grid.write_char(b as char),
            _ => self.decode_utf8(b, grid),
        }
    }
    fn decode_utf8(&mut self, b: u8, grid: &mut Grid) {
        self.utf8_buf.push(b);
        match std::str::from_utf8(&self.utf8_buf) {
            Ok(s) => {
                for grapheme in UnicodeSegmentation::graphemes(s, true) {
                    if let Some(ch) = grapheme.chars().next() {
                        grid.write_char(ch);
                    }
                }
                self.utf8_buf.clear();
            }
            Err(e) if e.error_len().is_none() => {}
            Err(_) => {
                grid.write_char('?');
                self.utf8_buf.clear();
            }
        }
    }
    fn escape(&mut self, b: u8) {
        self.state = match b {
            b'[' if self.caps.ansi_core => {
                self.params.clear();
                self.current = 0;
                self.seen_digit = false;
                self.csi_private = false; // FIX #5: reset private flag
                ParserState::Csi
            }
            b']' if self.caps.osc => {
                self.osc_buf.clear(); // FIX #3: clear OSC buffer on entry
                ParserState::Osc
            }
            _ => ParserState::Ground,
        };
    }
    fn csi(&mut self, b: u8, grid: &mut Grid) {
        match b {
            // FIX #5: handle '?' private parameter prefix
            b'?' if !self.seen_digit && self.params.is_empty() => {
                self.csi_private = true;
            }
            b'0'..=b'9' => {
                self.current = self.current.saturating_mul(10) + (b - b'0') as u16;
                self.seen_digit = true;
            }
            b';' => {
                self.params.push(if self.seen_digit { self.current } else { 0 });
                self.current = 0;
                self.seen_digit = false;
            }
            0x40..=0x7e => {
                if self.seen_digit {
                    self.params.push(self.current);
                }
                self.dispatch(b, grid);
                self.state = ParserState::Ground;
                self.params.clear();
                self.current = 0;
                self.seen_digit = false;
                self.csi_private = false; // FIX #5: reset after dispatch
            }
            _ => self.state = ParserState::Ground,
        }
    }
    fn osc(&mut self, b: u8) {
        match b {
            0x07 => {
                // FIX #3: BEL terminator — finalize and store title
                self.finalize_osc();
                self.state = ParserState::Ground;
            }
            0x1b => self.state = ParserState::OscEscape,
            _ => {
                self.osc_buf.push(b); // FIX #3: accumulate OSC bytes
            }
        }
    }
    // FIX #3: parse and store OSC window title
    fn finalize_osc(&mut self) {
        if let Ok(s) = std::str::from_utf8(&self.osc_buf) {
            // OSC format: "Ps;data" — codes 0, 1, 2 all set the title
            if let Some(rest) = s
                .strip_prefix("0;")
                .or_else(|| s.strip_prefix("1;"))
                .or_else(|| s.strip_prefix("2;"))
            {
                self.osc_title = Some(rest.to_string());
            }
        }
        self.osc_buf.clear();
    }
    fn p(&self, i: usize, d: u16) -> u16 {
        self.params.get(i).copied().unwrap_or(d)
    }
    fn dispatch(&mut self, f: u8, grid: &mut Grid) {
        log::trace!("parser_dispatch_final={} params={:?}", f as char, self.params);

        // FIX #5: handle private CSI sequences (e.g. ESC[?25h / ESC[?25l)
        if self.csi_private {
            match f {
                b'h' => {
                    // e.g. ESC[?25h — show cursor (no-op for grid; extend if needed)
                }
                b'l' => {
                    // e.g. ESC[?25l — hide cursor (no-op for grid; extend if needed)
                }
                _ => {}
            }
            return;
        }

        match f {
            b'A' => grid.cursor_row = grid.cursor_row.saturating_sub(self.p(0, 1) as usize),
            b'B' => grid.cursor_row = (grid.cursor_row + self.p(0, 1) as usize).min(grid.rows - 1),
            b'C' => grid.cursor_col = (grid.cursor_col + self.p(0, 1) as usize).min(grid.cols - 1),
            b'D' => grid.cursor_col = grid.cursor_col.saturating_sub(self.p(0, 1) as usize),
            b'E' => {
                grid.cursor_col = 0;
                grid.cursor_row = (grid.cursor_row + self.p(0, 1) as usize).min(grid.rows - 1);
            }
            b'F' => {
                grid.cursor_col = 0;
                grid.cursor_row = grid.cursor_row.saturating_sub(self.p(0, 1) as usize);
            }
            b'G' => grid.move_cursor(grid.cursor_row, self.p(0, 1).saturating_sub(1) as usize),
            b'H' | b'f' => grid.move_cursor(
                self.p(0, 1).saturating_sub(1) as usize,
                self.p(1, 1).saturating_sub(1) as usize,
            ),
            b'J' => match self.p(0, 0) {
                0 => grid.erase_display(EraseMode::ToEnd),
                1 => grid.erase_display(EraseMode::ToStart),
                2 | 3 => grid.erase_display(EraseMode::All),
                _ => {}
            },
            b'K' => match self.p(0, 0) {
                0 => grid.erase_line(EraseMode::ToEnd),
                1 => grid.erase_line(EraseMode::ToStart),
                2 => grid.erase_line(EraseMode::All),
                _ => {}
            },
            b'm' => self.sgr(grid),
            b'd' => grid.move_cursor(self.p(0, 1).saturating_sub(1) as usize, grid.cursor_col),
            b's' => self.saved_cursor = (grid.cursor_row, grid.cursor_col),
            b'u' => grid.move_cursor(self.saved_cursor.0, self.saved_cursor.1),
            _ => {}
        }
    }
    fn sgr(&mut self, grid: &mut Grid) {
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
                // FIX #1: SGR off-codes for bold, italic, underline
                21 | 22 => grid.current_attrs.bold = false,
                3 => grid.current_attrs.italic = true,
                23 => grid.current_attrs.italic = false,
                4 => grid.current_attrs.underline = true,
                24 => grid.current_attrs.underline = false,
                30..=37 => grid.current_attrs.fg = basic_color((code - 30) as u8),
                39 => grid.current_attrs.fg = Color::Default,
                40..=47 => grid.current_attrs.bg = basic_color((code - 40) as u8),
                49 => grid.current_attrs.bg = Color::Default,
                90..=97 if self.caps.xterm_extended => {
                    grid.current_attrs.fg = basic_color((code - 90) as u8)
                }
                100..=107 if self.caps.xterm_extended => {
                    grid.current_attrs.bg = basic_color((code - 100) as u8)
                }
                // FIX #2: 256-color and true color (38/48 extended color)
                38 | 48 if self.caps.xterm_extended => {
                    let is_fg = code == 38;
                    if self.params.get(i + 1).copied() == Some(5) && i + 2 < self.params.len() {
                        // 38;5;n — 256-color
                        let n = self.params[i + 2];
                        let color = Color::Indexed(n as u8);
                        if is_fg {
                            grid.current_attrs.fg = color;
                        } else {
                            grid.current_attrs.bg = color;
                        }
                        i += 2;
                    } else if self.params.get(i + 1).copied() == Some(2)
                        && i + 4 < self.params.len()
                    {
                        // 38;2;r;g;b — true color
                        let r = self.params[i + 2] as u8;
                        let g = self.params[i + 3] as u8;
                        let b = self.params[i + 4] as u8;
                        let color = Color::Rgb(r, g, b);
                        if is_fg {
                            grid.current_attrs.fg = color;
                        } else {
                            grid.current_attrs.bg = color;
                        }
                        i += 4;
                    }
                }
                _ => {}
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_utf8_and_csi() {
        let mut p = Parser::new(AnsiCapabilities::default());
        let mut g = Grid::new(5, 20);
        p.feed("hé".as_bytes(), &mut g);
        p.feed(b"\x1b[2;3H!", &mut g);
        assert_eq!(g.get(0, 0).expect("cell").ch, 'h');
        assert_eq!(g.get(0, 1).expect("cell").ch, 'é');
        assert_eq!(g.get(1, 2).expect("cell").ch, '!');
    }

    #[test]
    fn sgr_bold_toggle() {
        let mut p = Parser::new(AnsiCapabilities::default());
        let mut g = Grid::new(5, 20);
        p.feed(b"\x1b[1mA\x1b[22mB", &mut g);
        // after ESC[22m bold should be off
        assert!(!g.current_attrs.bold);
    }

    #[test]
    fn sgr_256_color() {
        let mut p = Parser::new(AnsiCapabilities::default());
        let mut g = Grid::new(5, 20);
        p.feed(b"\x1b[38;5;200m", &mut g);
        assert_eq!(g.current_attrs.fg, Color::Indexed(200));
    }

    #[test]
    fn sgr_true_color() {
        let mut p = Parser::new(AnsiCapabilities::default());
        let mut g = Grid::new(5, 20);
        p.feed(b"\x1b[38;2;10;20;30m", &mut g);
        assert_eq!(g.current_attrs.fg, Color::Rgb(10, 20, 30));
    }

    #[test]
    fn osc_title_stored() {
        let mut p = Parser::new(AnsiCapabilities::default());
        let mut g = Grid::new(5, 20);
        p.feed(b"\x1b]0;My Terminal\x07", &mut g);
        assert_eq!(p.osc_title.as_deref(), Some("My Terminal"));
    }

    #[test]
    fn private_csi_does_not_corrupt_state() {
        let mut p = Parser::new(AnsiCapabilities::default());
        let mut g = Grid::new(5, 20);
        // ESC[?25l followed by plain text must still render
        p.feed(b"\x1b[?25lHello", &mut g);
        assert_eq!(g.get(0, 0).expect("cell").ch, 'H');
    }

    #[test]
    fn utf8_buf_cleared_on_esc() {
        let mut p = Parser::new(AnsiCapabilities::default());
        let mut g = Grid::new(5, 20);
        // 0xC3 is the first byte of a 2-byte UTF-8 sequence; then ESC + plain text
        p.feed(&[0xC3, 0x1b, b'[', b'A', b'X'], &mut g);
        // Should not panic or leave garbage; X must render
        assert_eq!(g.get(0, 0).expect("cell").ch, 'X');
    }
}
