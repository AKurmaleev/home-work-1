use std::io::{self, Write};

use ba01::count_bytes;

fn main() -> io::Result<()> {
    let stdin = io::stdin();
    let bytes = count_bytes(stdin.lock())?;

    let stdout = io::stdout();
    let mut output = stdout.lock();
    writeln!(output, "{bytes}")
}
