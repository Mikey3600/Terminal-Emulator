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
