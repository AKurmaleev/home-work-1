pub fn add_u8_checked(a: u8, b: u8) -> Option<u8> {
    if b > u8::MAX - a { None } else { Some(a + b) }
}

pub fn add_u8_wrapping(a: u8, b: u8) -> u8 {
    let sum = u16::from(a) + u16::from(b);

    if sum > u16::from(u8::MAX) {
        (sum - 256) as u8
    } else {
        sum as u8
    }
}

pub fn add_u8_saturating(a: u8, b: u8) -> u8 {
    if b > u8::MAX - a { u8::MAX } else { a + b }
}

#[cfg(test)]
mod tests {
    use super::{add_u8_checked, add_u8_saturating, add_u8_wrapping};

    #[test]
    fn unsigned_overflow_modes() {
        assert_eq!(add_u8_checked(255, 1), None);
        assert_eq!(add_u8_wrapping(255, 1), 0);
        assert_eq!(add_u8_saturating(255, 1), 255);

        assert_eq!(add_u8_checked(10, 20), Some(30));
        assert_eq!(add_u8_wrapping(10, 20), 30);
        assert_eq!(add_u8_saturating(10, 20), 30);
    }

    #[test]
    fn handles_boundaries() {
        assert_eq!(add_u8_checked(254, 1), Some(255));
        assert_eq!(add_u8_wrapping(255, 255), 254);
        assert_eq!(add_u8_saturating(0, 255), 255);
    }
}
