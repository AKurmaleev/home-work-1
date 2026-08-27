use std::io::{self, Read};

fn count_bytes(input: &[u8]) -> usize {
    input.len()
}

fn main() {
    let mut input = Vec::new();
    io::stdin()
        .read_to_end(&mut input)
        .expect("failed to read standard input");

    println!("{}", count_bytes(&input));
}

#[cfg(test)]
mod tests {
    use super::count_bytes;

    #[test]
    fn counts_empty_input() {
        assert_eq!(count_bytes(b""), 0);
    }

    #[test]
    fn counts_bytes_instead_of_characters() {
        assert_eq!(count_bytes("🦀\n".as_bytes()), 5);
    }

    #[test]
    fn counts_binary_input() {
        assert_eq!(count_bytes(&[0, 1, 2, 255]), 4);
    }
}
