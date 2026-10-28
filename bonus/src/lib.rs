//! Fixed-size monochrome bitmaps: one byte per row, most significant bit left.
//!
//! ```
//! let image = ["#......."; 8];
//! let bytes = bonus::parse_bitmap_8x8(image);
//! assert_eq!(bytes, [0x80; 8]);
//! assert_eq!(bonus::render_bitmap_8x8(bytes), image.map(str::to_owned));
//! assert_eq!(bonus::invert_bitmap_8x8(bytes), [0x7f; 8]);
//! ```

#![forbid(unsafe_code)]

/// Encodes exactly eight rows of eight ASCII pixels (`#` = 1, `.` = 0).
///
/// The leftmost pixel becomes bit 7; the rightmost becomes bit 0.
///
/// # Panics
/// Panics if any row has a byte length other than eight or contains a symbol
/// other than `#` or `.`. The assignment specifies this infallible signature;
/// callers must validate external input before invoking it.
pub fn parse_bitmap_8x8(lines: [&str; 8]) -> [u8; 8] {
    let mut bytes = [0; 8];

    for (row_index, line) in lines.iter().enumerate() {
        assert_eq!(line.len(), 8, "each bitmap row must contain 8 pixels");

        for (pixel_index, pixel) in line.bytes().enumerate() {
            match pixel {
                b'#' => bytes[row_index] |= 1 << (7 - pixel_index),
                b'.' => {}
                _ => panic!("bitmap pixels must be '#' or '.'"),
            }
        }
    }

    bytes
}

/// Decodes every byte into eight ASCII pixels, most significant bit first.
pub fn render_bitmap_8x8(bytes: [u8; 8]) -> [String; 8] {
    std::array::from_fn(|row_index| {
        let mut line = String::with_capacity(8);

        for bit_index in (0..8).rev() {
            let pixel = if bytes[row_index] & (1 << bit_index) != 0 {
                '#'
            } else {
                '.'
            };
            line.push(pixel);
        }

        line
    })
}

/// Flips all 64 pixels; no reserved or padding bits exist in this format.
pub fn invert_bitmap_8x8(bytes: [u8; 8]) -> [u8; 8] {
    bytes.map(|byte| !byte)
}

#[cfg(test)]
mod tests {
    use super::{invert_bitmap_8x8, parse_bitmap_8x8, render_bitmap_8x8};

    const IMAGE: [&str; 8] = [
        "..####..", ".#....#.", "#.#..#.#", "#..##..#", "#......#", "#.#..#.#", ".#....#.",
        "..####..",
    ];

    const BYTES: [u8; 8] = [
        0b0011_1100,
        0b0100_0010,
        0b1010_0101,
        0b1001_1001,
        0b1000_0001,
        0b1010_0101,
        0b0100_0010,
        0b0011_1100,
    ];

    #[test]
    #[should_panic(expected = "each bitmap row must contain 8 pixels")]
    fn rejects_short_rows() {
        parse_bitmap_8x8(["......."; 8]);
    }

    #[test]
    #[should_panic(expected = "each bitmap row must contain 8 pixels")]
    fn rejects_long_rows() {
        parse_bitmap_8x8(["........."; 8]);
    }

    #[test]
    #[should_panic(expected = "bitmap pixels must be '#' or '.'")]
    fn rejects_invalid_symbols() {
        parse_bitmap_8x8([".......x"; 8]);
    }

    #[test]
    #[should_panic(expected = "bitmap pixels must be '#' or '.'")]
    fn rejects_multibyte_pixels_even_when_byte_length_is_eight() {
        parse_bitmap_8x8(["é......"; 8]);
    }

    #[test]
    fn every_byte_round_trips_in_every_row_and_inverts_twice() {
        for byte in u8::MIN..=u8::MAX {
            let bytes = [byte; 8];
            let rendered = render_bitmap_8x8(bytes);
            assert_eq!(
                parse_bitmap_8x8(rendered.each_ref().map(String::as_str)),
                bytes
            );
            assert_eq!(invert_bitmap_8x8(invert_bitmap_8x8(bytes)), bytes);
        }
        for bit in 0..8 {
            let mut pixels = [b'.'; 8];
            pixels[bit] = b'#';
            let row = std::str::from_utf8(&pixels).unwrap();
            assert_eq!(parse_bitmap_8x8([row; 8]), [1 << (7 - bit); 8]);
        }
    }

    #[test]
    fn parses_bitmap() {
        assert_eq!(parse_bitmap_8x8(IMAGE), BYTES);
    }

    #[test]
    fn renders_bitmap() {
        assert_eq!(render_bitmap_8x8(BYTES), IMAGE.map(str::to_owned));
    }

    #[test]
    fn inverts_bitmap() {
        assert_eq!(
            render_bitmap_8x8(invert_bitmap_8x8(BYTES)),
            [
                "##....##", "#.####.#", ".#.##.#.", ".##..##.", ".######.", ".#.##.#.", "#.####.#",
                "##....##",
            ]
            .map(str::to_owned)
        );
    }
}
