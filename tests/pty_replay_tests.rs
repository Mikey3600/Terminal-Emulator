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
    assert_eq!(grid.get(1, 0).expect("cell").ch, 'h');
    assert_eq!(grid.get(2, 0).expect("cell").ch, 'o');
}

#[test]
fn replay_long_session_with_unicode_and_scroll() {
    let mut parser = Parser::new(AnsiCapabilities::default());
    let mut grid = Grid::new(4, 12);

    let frames: Vec<Vec<u8>> = vec![
        b"\x1b[2J\x1b[H".to_vec(),
        "日本語\r\n".as_bytes().to_vec(),
        "e\u{301}\r\n".as_bytes().to_vec(),
        b"line3\r\nline4\r\nline5\r\n".to_vec(),
    ];

    for frame in &frames {
        parser.feed(frame, &mut grid);
    }

    assert_eq!(grid.cursor_row, 3);
    assert!(grid.get(0, 0).expect("cell").ch != ' ');
    assert_eq!(grid.cursor_row, 3);
}
