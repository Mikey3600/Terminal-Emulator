use terminal_emulator::ansi::{AnsiCapabilities, Parser};
use terminal_emulator::terminal::screen_buffer::Grid;

#[test]
fn parser_fixture_basic_prompt() {
    let fixture = include_str!("fixtures/parser/basic_prompt.ans").replace("\\x1b", "\x1b");
    let mut parser = Parser::new(AnsiCapabilities::default());
    let mut grid = Grid::new(6, 20);
    parser.feed(fixture.as_bytes(), &mut grid);

    assert_eq!(grid.get(0, 0).expect("cell").ch, 'h');
    assert_eq!(grid.get(1, 4).expect("cell").ch, '!');
}

#[test]
fn parser_csi_cursor_and_erase_modes() {
    let mut parser = Parser::new(AnsiCapabilities::default());
    let mut grid = Grid::new(3, 6);
    parser.feed(b"abcdef", &mut grid);
    parser.feed(b"\x1b[1;3H\x1b[K", &mut grid);

    assert_eq!(grid.get(0, 0).expect("cell").ch, 'a');
    assert_eq!(grid.get(0, 2).expect("cell").ch, ' ');
    assert_eq!(grid.get(0, 5).expect("cell").ch, ' ');
}

#[test]
fn parser_newline_scrolls_bottom_row() {
    let mut parser = Parser::new(AnsiCapabilities::default());
    let mut grid = Grid::new(2, 4);
    parser.feed(b"line1\nline2\nline3", &mut grid);

    assert_eq!(grid.get(0, 0).expect("cell").ch, 'l');
    assert_eq!(grid.get(0, 1).expect("cell").ch, 'i');
    assert_eq!(grid.get(1, 0).expect("cell").ch, 'l');
}
