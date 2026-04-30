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

pub struct Parser {
    state: ParserState,
    params: Vec<u16>,
    current: u16,
    seen_digit: bool,
    utf8_buf: Vec<u8>,
    caps: AnsiCapabilities,
    saved_cursor: (usize, usize),
}

impl Parser {
    pub fn new(caps: AnsiCapabilities) -> Self {
        Self {
            state: ParserState::Ground,
            params: Vec::with_capacity(8),
            current: 0,
            seen_digit: false,
            utf8_buf: Vec::with_capacity(4),
            caps,
            saved_cursor: (0, 0),
        }
    }
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
                self.state = ParserState::Ground;
                let _ = b;
            }
        }
    }
    fn ground(&mut self, b: u8, grid: &mut Grid) {
        match b {
            0x1b => self.state = ParserState::Escape,
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
                grid.write_char('�');
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
                ParserState::Csi
            }
            b']' if self.caps.osc => ParserState::Osc,
            _ => ParserState::Ground,
        };
    }
    fn csi(&mut self, b: u8, grid: &mut Grid) {
        match b {
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
            }
            _ => self.state = ParserState::Ground,
        }
    }
    fn osc(&mut self, b: u8) {
        match b {
            0x07 => self.state = ParserState::Ground,
            0x1b => self.state = ParserState::OscEscape,
            _ => {}
        }
    }
    fn p(&self, i: usize, d: u16) -> u16 {
        self.params.get(i).copied().unwrap_or(d)
    }
    fn dispatch(&mut self, f: u8, grid: &mut Grid) {
        log::trace!("parser_dispatch_final={} params={:?}", f as char, self.params);
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
        for &code in &self.params {
            match code {
                0 => grid.current_attrs = Attributes::default(),
                1 => grid.current_attrs.bold = true,
                3 => grid.current_attrs.italic = true,
                4 => grid.current_attrs.underline = true,
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
                _ => {}
            }
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
}
