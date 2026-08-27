use bonus::{invert_bitmap_8x8, parse_bitmap_8x8, render_bitmap_8x8};

fn main() {
    let image = [
        "..####..", ".#....#.", "#.#..#.#", "#..##..#", "#......#", "#.#..#.#", ".#....#.",
        "..####..",
    ];

    let bytes = parse_bitmap_8x8(image);

    println!("Bytes:");
    for byte in bytes {
        println!("{byte:08b}  0x{byte:02X}");
    }

    println!();
    println!("Rendered:");
    for line in render_bitmap_8x8(bytes) {
        println!("{line}");
    }

    println!();
    println!("Inverted:");
    for line in render_bitmap_8x8(invert_bitmap_8x8(bytes)) {
        println!("{line}");
    }
}
