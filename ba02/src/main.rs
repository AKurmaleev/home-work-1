use std::io::{self, Read};

fn count_lines_words_bytes(input: &[u8]) -> (usize, usize, usize) {
    let lines = input.iter().filter(|&&byte| byte == b'\n').count();
    let mut words = 0;
    let mut inside_word = false;

    for byte in input {
        if byte.is_ascii_whitespace() {
            inside_word = false;
        } else if !inside_word {
            words += 1;
            inside_word = true;
        }
    }

    (lines, words, input.len())
}

fn main() {
    let mut input = Vec::new();
    io::stdin()
        .read_to_end(&mut input)
        .expect("failed to read standard input");

    let (lines, words, bytes) = count_lines_words_bytes(&input);
    println!("{lines} {words} {bytes}");
}

#[cfg(test)]
mod tests {
    use super::count_lines_words_bytes;

    #[test]
    fn counts_empty_input() {
        assert_eq!(count_lines_words_bytes(b""), (0, 0, 0));
    }

    #[test]
    fn counts_input_without_trailing_newline() {
        assert_eq!(count_lines_words_bytes(b"hello"), (0, 1, 5));
    }

    #[test]
    fn counts_lines_words_and_bytes() {
        assert_eq!(count_lines_words_bytes(b"hello rust\n"), (1, 2, 11));
        assert_eq!(count_lines_words_bytes(b" hello rust \n"), (1, 2, 13));
        assert_eq!(count_lines_words_bytes(b"a\tb\nc"), (1, 3, 5));
    }

    #[test]
    fn counts_binary_input() {
        assert_eq!(count_lines_words_bytes(&[0, 255, b' ', 1]), (0, 2, 4));
    }
}
