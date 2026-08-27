use std::io::{self, BufRead, Read};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Counts {
    pub lines: u64,
    pub words: u64,
    pub bytes: u64,
}

pub fn count_lines_words_bytes_initial(mut input: impl Read) -> io::Result<Counts> {
    let mut bytes = Vec::new();
    input.read_to_end(&mut bytes)?;

    let lines = bytes.iter().filter(|&&byte| byte == b'\n').count();
    let mut words = 0;
    let mut inside_word = false;

    for byte in &bytes {
        if byte.is_ascii_whitespace() {
            inside_word = false;
        } else if !inside_word {
            words += 1;
            inside_word = true;
        }
    }

    Ok(Counts {
        lines: u64::try_from(lines)
            .map_err(|_| io::Error::other("input line count exceeds u64"))?,
        words: u64::try_from(words)
            .map_err(|_| io::Error::other("input word count exceeds u64"))?,
        bytes: u64::try_from(bytes.len())
            .map_err(|_| io::Error::other("input length exceeds u64"))?,
    })
}

fn checked_add(counter: &mut u64, amount: u64) -> io::Result<()> {
    *counter = counter
        .checked_add(amount)
        .ok_or_else(|| io::Error::other("input counters exceed u64"))?;

    Ok(())
}

pub fn count_lines_words_bytes(mut input: impl BufRead) -> io::Result<Counts> {
    let mut counts = Counts {
        lines: 0,
        words: 0,
        bytes: 0,
    };
    let mut inside_word = false;

    loop {
        let chunk = input.fill_buf()?;
        if chunk.is_empty() {
            break;
        }

        let consumed = chunk.len();
        let mut chunk_lines = 0;
        let mut chunk_words = 0;

        for &byte in chunk {
            if byte == b'\n' {
                chunk_lines += 1;
            }

            if byte.is_ascii_whitespace() {
                inside_word = false;
            } else if !inside_word {
                chunk_words += 1;
                inside_word = true;
            }
        }

        let byte_count = u64::try_from(consumed)
            .map_err(|_| io::Error::other("input chunk length exceeds u64"))?;
        let line_count = u64::try_from(chunk_lines)
            .map_err(|_| io::Error::other("input chunk line count exceeds u64"))?;
        let word_count = u64::try_from(chunk_words)
            .map_err(|_| io::Error::other("input chunk word count exceeds u64"))?;

        checked_add(&mut counts.bytes, byte_count)?;
        checked_add(&mut counts.lines, line_count)?;
        checked_add(&mut counts.words, word_count)?;
        input.consume(consumed);
    }

    Ok(counts)
}

#[cfg(test)]
mod tests {
    use std::io::{BufReader, Cursor};

    use super::{Counts, count_lines_words_bytes, count_lines_words_bytes_initial};

    fn assert_count(input: &[u8], expected: Counts) {
        assert_eq!(count_lines_words_bytes_initial(input).unwrap(), expected);
        assert_eq!(count_lines_words_bytes(input).unwrap(), expected);
    }

    #[test]
    fn counts_empty_input() {
        assert_count(
            b"",
            Counts {
                lines: 0,
                words: 0,
                bytes: 0,
            },
        );
    }

    #[test]
    fn counts_input_without_trailing_newline() {
        assert_count(
            b"hello",
            Counts {
                lines: 0,
                words: 1,
                bytes: 5,
            },
        );
    }

    #[test]
    fn counts_lines_words_and_bytes() {
        assert_count(
            b"hello rust\n",
            Counts {
                lines: 1,
                words: 2,
                bytes: 11,
            },
        );
        assert_count(
            b" hello rust \n",
            Counts {
                lines: 1,
                words: 2,
                bytes: 13,
            },
        );
        assert_count(
            b"a\tb\nc",
            Counts {
                lines: 1,
                words: 3,
                bytes: 5,
            },
        );
    }

    #[test]
    fn counts_binary_input() {
        assert_count(
            &[0, 255, b' ', 1],
            Counts {
                lines: 0,
                words: 2,
                bytes: 4,
            },
        );
    }

    #[test]
    fn preserves_word_state_across_buffer_boundaries() {
        let reader = BufReader::with_capacity(2, Cursor::new(b"hello rust\n"));

        assert_eq!(
            count_lines_words_bytes(reader).unwrap(),
            Counts {
                lines: 1,
                words: 2,
                bytes: 11,
            }
        );
    }
}
