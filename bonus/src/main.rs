use std::io::{self, BufWriter, Write};

use bonus::{invert_bitmap_8x8, parse_bitmap_8x8, render_bitmap_8x8};

fn main() -> io::Result<()> {
    let image = [
        "..####..", ".#....#.", "#.#..#.#", "#..##..#", "#......#", "#.#..#.#", ".#....#.",
        "..####..",
    ];

    let bytes = parse_bitmap_8x8(image);

    let stdout = io::stdout();
    let mut output = BufWriter::new(stdout.lock());

    writeln!(output, "Bytes:")?;
    for byte in bytes {
        writeln!(output, "{byte:08b}  0x{byte:02X}")?;
    }

    writeln!(output)?;
    writeln!(output, "Rendered:")?;
    for line in render_bitmap_8x8(bytes) {
        writeln!(output, "{line}")?;
    }

    writeln!(output)?;
    writeln!(output, "Inverted:")?;
    for line in render_bitmap_8x8(invert_bitmap_8x8(bytes)) {
        writeln!(output, "{line}")?;
    }

    output.flush()
}
