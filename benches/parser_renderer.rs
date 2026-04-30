use criterion::{criterion_group, criterion_main, Criterion};
use terminal_emulator::ansi::{AnsiCapabilities, Parser};
use terminal_emulator::terminal::renderer::render;
use terminal_emulator::terminal::screen_buffer::Grid;

/// Benchmarks ANSI parsing throughput by repeatedly parsing a colored line
/// payload into a fresh terminal grid.
fn parser_throughput(c: &mut Criterion) {
    let data = b"\x1b[32mhello world\x1b[0m\r\n".repeat(200);
    c.bench_function("parser_throughput", |b| {
        b.iter(|| {
            let mut parser = Parser::new(AnsiCapabilities::default());
            let mut grid = Grid::new(80, 120);
            parser.feed(&data, &mut grid);
        })
    });
}

/// Benchmarks render performance by writing a short sample line to the grid
/// and rendering it into a sink output stream.
fn renderer_perf(c: &mut Criterion) {
    let mut grid = Grid::new(40, 120);
    for ch in "benchmark render line".chars() {
        grid.write_char(ch);
    }
    c.bench_function("renderer_performance", |b| {
        b.iter(|| {
            let mut sink = std::io::sink();
            let _ = render(&grid, &mut sink);
        })
    });
}

criterion_group!(benches, parser_throughput, renderer_perf);
criterion_main!(benches);
