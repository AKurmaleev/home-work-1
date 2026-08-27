pub fn add_u8_checked(a: u8, b: u8) -> Option<u8> {
    if b > u8::MAX - a { None } else { Some(a + b) }
}

pub fn add_u8_wrapping_initial(a: u8, b: u8) -> u8 {
    let sum = u16::from(a) + u16::from(b);

    if sum > u16::from(u8::MAX) {
        (sum - 256) as u8
    } else {
        sum as u8
    }
}

pub fn add_u8_wrapping(a: u8, b: u8) -> u8 {
    let remaining = u8::MAX - a;

    if b > remaining {
        // The branch proves `b - remaining >= 1`, so both subtractions fit in `u8`.
        b - remaining - 1
    } else {
        a + b
    }
}

pub fn add_u8_saturating(a: u8, b: u8) -> u8 {
    if b > u8::MAX - a { u8::MAX } else { a + b }
}

#[cfg(test)]
mod tests {
    use super::{add_u8_checked, add_u8_saturating, add_u8_wrapping, add_u8_wrapping_initial};

    #[test]
    fn unsigned_overflow_modes() {
        assert_eq!(add_u8_checked(255, 1), None);
        assert_eq!(add_u8_wrapping_initial(255, 1), 0);
        assert_eq!(add_u8_wrapping(255, 1), 0);
        assert_eq!(add_u8_saturating(255, 1), 255);

        assert_eq!(add_u8_checked(10, 20), Some(30));
        assert_eq!(add_u8_wrapping_initial(10, 20), 30);
        assert_eq!(add_u8_wrapping(10, 20), 30);
        assert_eq!(add_u8_saturating(10, 20), 30);
    }

    #[test]
    fn handles_boundaries() {
        assert_eq!(add_u8_checked(254, 1), Some(255));
        assert_eq!(add_u8_wrapping_initial(255, 255), 254);
        assert_eq!(add_u8_wrapping(255, 255), 254);
        assert_eq!(add_u8_saturating(0, 255), 255);
    }

    #[test]
    fn matches_defined_semantics_for_all_u8_pairs() {
        let modulus = u16::from(u8::MAX) + 1;

        for a in u8::MIN..=u8::MAX {
            for b in u8::MIN..=u8::MAX {
                let sum = u16::from(a) + u16::from(b);
                let checked = u8::try_from(sum).ok();
                let wrapping =
                    u8::try_from(sum % modulus).expect("a value modulo 256 must fit into u8");
                let saturating = u8::try_from(sum).unwrap_or(u8::MAX);

                assert_eq!(add_u8_checked(a, b), checked);
                assert_eq!(add_u8_wrapping_initial(a, b), wrapping);
                assert_eq!(add_u8_wrapping(a, b), wrapping);
                assert_eq!(add_u8_saturating(a, b), saturating);
            }
        }
    }
}
