pub fn sort_arguments(arguments: &mut [String]) {
    for index in 1..arguments.len() {
        let mut current = index;

        while current > 0 && arguments[current] < arguments[current - 1] {
            arguments.swap(current, current - 1);
            current -= 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::sort_arguments;

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn sorts_arguments_lexicographically() {
        let mut arguments = strings(&["e", "d", "c", "b", "a"]);
        sort_arguments(&mut arguments);
        assert_eq!(arguments, strings(&["a", "b", "c", "d", "e"]));
    }

    #[test]
    fn keeps_repeated_arguments() {
        let mut arguments = strings(&["A", "a", "A", "a", "A", "a"]);
        sort_arguments(&mut arguments);
        assert_eq!(arguments, strings(&["A", "A", "A", "a", "a", "a"]));
    }

    #[test]
    fn handles_empty_arguments() {
        let mut arguments = Vec::new();
        sort_arguments(&mut arguments);
        assert!(arguments.is_empty());
    }

    #[test]
    fn preserves_punctuation() {
        let mut arguments = strings(&["hello,", "world.", "hello"]);
        sort_arguments(&mut arguments);
        assert_eq!(arguments, strings(&["hello", "hello,", "world."]));
    }
}
