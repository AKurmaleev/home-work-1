use std::io::{self, BufWriter, Write};

use ba03::sort_arguments;

fn main() -> io::Result<()> {
    let mut arguments: Vec<String> = std::env::args().skip(1).collect();
    sort_arguments(&mut arguments);

    let stdout = io::stdout();
    let mut output = BufWriter::new(stdout.lock());
    for argument in arguments {
        writeln!(output, "{argument}")?;
    }

    output.flush()
}
