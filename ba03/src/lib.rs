//! Sorts UTF-8 command-line argument values without removing duplicates.
//!
//! ```
//! let mut arguments = ["zebra".to_owned(), "apple".to_owned()];
//! ba03::sort_arguments(&mut arguments);
//! assert_eq!(arguments, ["apple", "zebra"]);
//! ```

#![forbid(unsafe_code)]

/// Sorts in place using insertion sort for the assignment's manual variant.
///
/// Ordering is case-sensitive and lexicographic; punctuation and duplicate
/// multiplicity are preserved. Worst-case time is quadratic in argument count.
pub fn sort_arguments_initial(arguments: &mut [String]) {
    for index in 1..arguments.len() {
        let mut current = index;

        while current > 0 && arguments[current] < arguments[current - 1] {
            arguments.swap(current, current - 1);
            current -= 1;
        }
    }
}

/// Sorts in place using case-sensitive lexicographic string ordering.
///
/// Preserves punctuation and duplicate multiplicity. Equal strings do not need
/// stable ordering, so the standard unstable sort is sufficient.
pub fn sort_arguments(arguments: &mut [String]) {
    arguments.sort_unstable();
}

#[cfg(test)]
mod tests {
    use super::{sort_arguments, sort_arguments_initial};

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    fn assert_sort(input: &[&str], expected: &[&str]) {
        let expected = strings(expected);
        let mut initial = strings(input);
        let mut optimized = initial.clone();

        sort_arguments_initial(&mut initial);
        sort_arguments(&mut optimized);

        assert_eq!(initial, expected);
        assert_eq!(optimized, expected);
    }

    #[test]
    fn sorts_arguments_lexicographically() {
        assert_sort(&["e", "d", "c", "b", "a"], &["a", "b", "c", "d", "e"]);
    }

    #[test]
    fn keeps_repeated_arguments() {
        assert_sort(
            &["A", "a", "A", "a", "A", "a"],
            &["A", "A", "A", "a", "a", "a"],
        );
    }

    #[test]
    fn handles_empty_arguments() {
        assert_sort(&[], &[]);
    }

    #[test]
    fn handles_single_sorted_empty_and_unicode_arguments() {
        assert_sort(&["single"], &["single"]);
        assert_sort(&["", "A", "a"], &["", "A", "a"]);
        assert_sort(&["🦀", "é", "z", ""], &["", "z", "é", "🦀"]);
    }

    #[test]
    fn preserves_punctuation() {
        assert_sort(
            &["hello,", "world.", "hello"],
            &["hello", "hello,", "world."],
        );
    }
}
