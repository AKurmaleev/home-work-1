//! Byte counters for arbitrary binary input.
//!
//! ```
//! assert_eq!(ba01::count_bytes(&b"hello\n"[..])?, 6);
//! # Ok::<(), std::io::Error>(())
//! ```

#![forbid(unsafe_code)]

use std::io::{self, Read};

/// Counts bytes after buffering the entire reader in memory.
///
/// Retained as an educational benchmark baseline; use only for bounded input
/// known to fit in memory. The reader is consumed through EOF.
///
/// # Errors
/// Returns read errors or an error if the input length cannot fit in `u64`.
pub fn count_bytes_initial(mut input: impl Read) -> io::Result<u64> {
    let mut bytes = Vec::new();
    input.read_to_end(&mut bytes)?;

    u64::try_from(bytes.len()).map_err(|_| io::Error::other("input length exceeds u64"))
}

/// Counts arbitrary bytes through EOF without retaining the entire input.
///
/// # Errors
/// Returns any unrecoverable error from the reader. Interrupted reads are retried.
pub fn count_bytes(mut input: impl Read) -> io::Result<u64> {
    io::copy(&mut input, &mut io::sink())
}

#[cfg(test)]
mod tests {
    use std::io::{self, Read};

    use super::{count_bytes, count_bytes_initial};

    fn assert_count(input: &[u8], expected: u64) {
        assert_eq!(count_bytes_initial(input).unwrap(), expected);
        assert_eq!(count_bytes(input).unwrap(), expected);
    }

    struct ShortReads {
        remaining: &'static [u8],
        interrupt: bool,
        fail_at_eof: bool,
    }

    impl Read for ShortReads {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            if buffer.is_empty() {
                return Ok(0);
            }
            if self.interrupt {
                self.interrupt = false;
                return Err(io::ErrorKind::Interrupted.into());
            }
            if self.remaining.is_empty() {
                return if self.fail_at_eof {
                    Err(io::Error::other("injected read error"))
                } else {
                    Ok(0)
                };
            }
            buffer[0] = self.remaining[0];
            self.remaining = &self.remaining[1..];
            self.interrupt = true;
            Ok(1)
        }
    }

    #[test]
    fn handles_short_reads_interruptions_and_failure_after_progress() {
        for fail_at_eof in [false, true] {
            let input = || ShortReads {
                remaining: b"a\0\xff\n",
                interrupt: true,
                fail_at_eof,
            };
            for result in [count_bytes_initial(input()), count_bytes(input())] {
                if fail_at_eof {
                    assert_eq!(result.unwrap_err().to_string(), "injected read error");
                } else {
                    assert_eq!(result.unwrap(), 4);
                }
            }
        }
    }

    #[test]
    fn counts_empty_input() {
        assert_count(b"", 0);
    }

    #[test]
    fn counts_bytes_instead_of_characters() {
        assert_count("🦀\n".as_bytes(), 5);
    }

    #[test]
    fn counts_binary_input() {
        assert_count(&[0, 1, 2, 255], 4);
    }
}
