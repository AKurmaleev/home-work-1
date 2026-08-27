use std::io::{self, Read};

pub fn count_bytes_initial(mut input: impl Read) -> io::Result<u64> {
    let mut bytes = Vec::new();
    input.read_to_end(&mut bytes)?;

    u64::try_from(bytes.len()).map_err(|_| io::Error::other("input length exceeds u64"))
}

pub fn count_bytes(mut input: impl Read) -> io::Result<u64> {
    io::copy(&mut input, &mut io::sink())
}

#[cfg(test)]
mod tests {
    use super::{count_bytes, count_bytes_initial};

    fn assert_count(input: &[u8], expected: u64) {
        assert_eq!(count_bytes_initial(input).unwrap(), expected);
        assert_eq!(count_bytes(input).unwrap(), expected);
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
