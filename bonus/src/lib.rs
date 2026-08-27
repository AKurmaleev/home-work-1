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
