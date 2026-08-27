use ba03::sort_arguments;

fn main() {
    let mut arguments: Vec<String> = std::env::args().skip(1).collect();
    sort_arguments(&mut arguments);

    for argument in arguments {
        println!("{argument}");
    }
}
