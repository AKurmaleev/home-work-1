//! Counts LF bytes, ASCII-whitespace-delimited words, and arbitrary input bytes.
//!
//! Whitespace follows `u8::is_ascii_whitespace`: tab, LF, form feed, CR, and
//! space. Vertical tab and non-ASCII bytes are not separators.
//!
//! ```
//! let counts = ba02::count_lines_words_bytes(&b"hello rust\n"[..])?;
//! assert_eq!((counts.lines, counts.words, counts.bytes), (1, 2, 11));
//! # Ok::<(), std::io::Error>(())
//! ```

#![forbid(unsafe_code)]
#![deny(clippy::default_numeric_fallback)]

use std::io::{self, BufRead, Read};

/// Independent counts; an unterminated final line does not add an LF.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Counts {
    /// Number of LF (`\n`) bytes.
    pub lines: u64,
    /// Number of transitions from whitespace (or start of input) into a word.
    pub words: u64,
    /// Total byte length, including separators and invalid UTF-8.
    pub bytes: u64,
}

/// Buffers the entire reader, then counts lines, words, and bytes.
///
/// This educational baseline requires bounded input that fits in memory.
///
/// # Errors
/// Returns read errors or errors converting counts to `u64`.
pub fn count_lines_words_bytes_initial(mut input: impl Read) -> io::Result<Counts> {
    let mut bytes = Vec::new();
    input.read_to_end(&mut bytes)?;

    let lines = bytes.iter().filter(|&&byte| byte == b'\n').count();
    // Each word start belongs to this slice, so the count fits its length type.
    let mut words = 0_usize;
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

/// Counts through EOF while preserving word state across borrowed buffers.
///
/// Retains constant-size state in addition to the caller's input buffer.
///
/// # Errors
/// Retries `Interrupted`; propagates other read errors and counter overflow.
pub fn count_lines_words_bytes(mut input: impl BufRead) -> io::Result<Counts> {
    let mut counts = Counts {
        lines: 0,
        words: 0,
        bytes: 0,
    };
    let mut inside_word = false;

    loop {
        let chunk = match input.fill_buf() {
            Ok(chunk) => chunk,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        if chunk.is_empty() {
            break;
        }

        let consumed = chunk.len();
        // Neither local count can exceed the borrowed slice's `usize` length.
        let mut chunk_lines = 0_usize;
        let mut chunk_words = 0_usize;

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
    use std::io::{self, BufRead, BufReader, Cursor, Read};

    use super::{Counts, count_lines_words_bytes, count_lines_words_bytes_initial};

    fn assert_count(input: &[u8], expected: Counts) {
        assert_eq!(count_lines_words_bytes_initial(input).unwrap(), expected);
        assert_eq!(count_lines_words_bytes(input).unwrap(), expected);
    }

    struct InterruptedChunks {
        remaining: &'static [u8],
        interrupt: bool,
    }

    impl Read for InterruptedChunks {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            let chunk = self.fill_buf()?;
            let length = buffer.len().min(chunk.len());
            buffer[..length].copy_from_slice(&chunk[..length]);
            self.consume(length);
            Ok(length)
        }
    }

    impl BufRead for InterruptedChunks {
        fn fill_buf(&mut self) -> io::Result<&[u8]> {
            if self.interrupt {
                self.interrupt = false;
                return Err(io::ErrorKind::Interrupted.into());
            }
            Ok(&self.remaining[..self.remaining.len().min(2)])
        }

        fn consume(&mut self, amount: usize) {
            self.remaining = &self.remaining[amount..];
            self.interrupt = true;
        }
    }

    #[test]
    fn retries_interrupted_reads_without_losing_word_state() {
        let input = InterruptedChunks {
            remaining: b"hello rust\n",
            interrupt: true,
        };
        assert_eq!(
            count_lines_words_bytes(input).unwrap(),
            Counts {
                lines: 1,
                words: 2,
                bytes: 11
            }
        );
    }

    #[test]
    fn propagates_non_interrupted_read_errors() {
        struct FailedInput;
        impl Read for FailedInput {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::other("injected read error"))
            }
        }
        for error in [
            count_lines_words_bytes_initial(FailedInput).unwrap_err(),
            count_lines_words_bytes(BufReader::new(FailedInput)).unwrap_err(),
        ] {
            assert_eq!(error.kind(), io::ErrorKind::Other);
            assert_eq!(error.to_string(), "injected read error");
        }
    }

    #[test]
    fn rejects_total_counter_overflow_without_changing_it() {
        let mut count = u64::MAX;
        let error = super::checked_add(&mut count, 1).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert_eq!(count, u64::MAX);
    }

    #[test]
    fn handles_all_byte_values_and_every_small_buffer_boundary() {
        let bytes: Vec<u8> = (u8::MIN..=u8::MAX).collect();
        let expected = Counts {
            lines: 1,
            words: 4,
            bytes: 256,
        };
        assert_count(&bytes, expected);
        for capacity in 1..=17 {
            assert_eq!(
                count_lines_words_bytes(BufReader::with_capacity(capacity, bytes.as_slice()))
                    .unwrap(),
                expected
            );
        }
        assert_count(
            "🦀\n".as_bytes(),
            Counts {
                lines: 1,
                words: 1,
                bytes: 5,
            },
        );
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
