use terminal_emulator::ansi::{AnsiCapabilities, Parser};
use terminal_emulator::terminal::screen_buffer::{Color, Grid};

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
    parser.feed(b"ab\ncd\nef", &mut grid);

    assert_eq!(grid.get(1, 0).expect("cell").ch, 'e');
    assert_eq!(grid.get(1, 1).expect("cell").ch, 'f');
    assert_eq!(grid.get(0, 0).expect("cell").ch, ' ');
}

#[test]
fn vt_cursor_movement_fixture() {
    let fixture = include_str!("vt_sequences/cursor_movement.ans").replace("\\x1b", "\x1b");
    let mut parser = Parser::new(AnsiCapabilities::default());
    let mut grid = Grid::new(2, 6);
    parser.feed(fixture.as_bytes(), &mut grid);

    assert_eq!(grid.get(0, 3).expect("cell").ch, '?');
    assert_eq!(grid.get(1, 0).expect("cell").ch, '!');
}

#[test]
fn vt_erase_screen_fixture() {
    let fixture = include_str!("vt_sequences/erase_screen.ans").replace("\\x1b", "\x1b");
    let mut parser = Parser::new(AnsiCapabilities::default());
    let mut grid = Grid::new(2, 6);
    parser.feed(fixture.as_bytes(), &mut grid);

    for r in 0..2 {
        for c in 0..6 {
            assert_eq!(grid.get(r, c).expect("cell").ch, ' ');
        }
    }
}

#[test]
fn vt_erase_line_fixture() {
    let fixture = include_str!("vt_sequences/erase_line.ans").replace("\\x1b", "\x1b");
    let mut parser = Parser::new(AnsiCapabilities::default());
    let mut grid = Grid::new(1, 6);
    parser.feed(fixture.as_bytes(), &mut grid);

    assert_eq!(grid.get(0, 0).expect("cell").ch, 'a');
    assert_eq!(grid.get(0, 1).expect("cell").ch, 'b');
    assert_eq!(grid.get(0, 2).expect("cell").ch, ' ');
}

#[test]
fn vt_sgr_colors_fixture() {
    let fixture = include_str!("vt_sequences/sgr_colors.ans").replace("\\x1b", "\x1b");
    let mut parser = Parser::new(AnsiCapabilities::default());
    let mut grid = Grid::new(1, 6);
    parser.feed(fixture.as_bytes(), &mut grid);

    assert_eq!(grid.get(0, 0).expect("cell").attrs.fg, Color::Red);
    assert_eq!(grid.get(0, 1).expect("cell").attrs.fg, Color::Green);
    assert_eq!(grid.get(0, 2).expect("cell").attrs.fg, Color::Default);
}

#[test]
fn vt_save_restore_cursor_fixture() {
    let fixture = include_str!("vt_sequences/save_restore_cursor.ans").replace("\\x1b", "\x1b");
    let mut parser = Parser::new(AnsiCapabilities::default());
    let mut grid = Grid::new(1, 5);
    parser.feed(fixture.as_bytes(), &mut grid);

    assert_eq!(grid.get(0, 0).expect("cell").ch, 'A');
    assert_eq!(grid.get(0, 1).expect("cell").ch, 'Z');
    assert_eq!(grid.get(0, 2).expect("cell").ch, 'C');
}
