use terminal_emulator::ansi::{AnsiCapabilities, Parser};
use terminal_emulator::terminal::screen_buffer::Grid;

#[test]
fn replay_stream_without_real_tty() {
    let frames: &[&[u8]] = &[b"$ ", b"echo hi\r\n", b"hi\r\n", b"\x1b[32mok\x1b[0m\r\n"];

    let mut parser = Parser::new(AnsiCapabilities::default());
    let mut grid = Grid::new(8, 40);
    for frame in frames {
        parser.feed(frame, &mut grid);
    }

    assert_eq!(grid.get(0, 0).expect("cell").ch, '$');
    assert_eq!(grid.get(2, 0).expect("cell").ch, 'h');
    assert_eq!(grid.get(3, 0).expect("cell").ch, 'o');
}
