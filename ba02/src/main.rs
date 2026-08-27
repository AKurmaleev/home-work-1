use std::io::{self, Write};

use ba02::count_lines_words_bytes;

fn main() -> io::Result<()> {
    let stdin = io::stdin();
    let counts = count_lines_words_bytes(stdin.lock())?;

    let stdout = io::stdout();
    let mut output = stdout.lock();
    writeln!(output, "{} {} {}", counts.lines, counts.words, counts.bytes)
}
