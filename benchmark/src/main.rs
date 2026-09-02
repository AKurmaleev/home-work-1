mod ba02_large;

use std::env;
use std::error::Error;
use std::hint::black_box;
use std::io::{self, BufReader, Cursor};
use std::time::{Duration, Instant};

use ba01::{count_bytes, count_bytes_initial};
use ba02::{Counts, count_lines_words_bytes, count_lines_words_bytes_initial};
use ba03::{sort_arguments, sort_arguments_initial};
use ba04::{add_u8_wrapping, add_u8_wrapping_initial};

const BYTE_DATASET_SIZE: usize = 64 * 1024 * 1024;
const READER_CAPACITY: usize = 64 * 1024;
const SORT_ITEMS: usize = 20_000;
const U8_REPETITIONS: usize = 256;
const SAMPLES: usize = 5;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn main() -> Result<()> {
    let arguments: Vec<String> = env::args().skip(1).collect();
    if arguments.first().is_some_and(|argument| argument == "ba02") {
        return ba02_large::run(&arguments[1..]);
    }
    if !arguments.is_empty() {
        return Err("usage: benchmark [ba02 <runtime|once> ...]".into());
    }

    let bytes = build_byte_dataset(BYTE_DATASET_SIZE);
    let sort_input = build_sort_dataset(SORT_ITEMS);

    verify_equivalence(&bytes, &sort_input)?;

    let ba01_initial = measure_reader(&bytes, count_bytes_initial)?;
    let ba01_optimized = measure_reader(&bytes, count_bytes)?;

    let ba02_initial = measure_reader(&bytes, |reader| {
        count_lines_words_bytes_initial(reader).map(counts_checksum)
    })?;
    let ba02_optimized = measure_reader(&bytes, |reader| {
        count_lines_words_bytes(reader).map(counts_checksum)
    })?;

    let ba03_initial = measure_sort(&sort_input, sort_arguments_initial);
    let ba03_optimized = measure_sort(&sort_input, sort_arguments);

    let ba04_initial = measure_u8(add_u8_wrapping_initial);
    let ba04_optimized = measure_u8(add_u8_wrapping);

    println!("Release benchmark: 1 warm-up + median of {SAMPLES} samples");
    println!("All implementations produced matching outputs before timing.\n");
    println!(
        "{:<6} | {:<24} | {:>14} | {:>14} | {:>17}",
        "case", "dataset", "initial median", "optimized", "initial/optimized"
    );
    println!("{}", "-".repeat(88));
    print_result("ba01", "64 MiB byte stream", ba01_initial, ba01_optimized);
    print_result("ba02", "64 MiB text stream", ba02_initial, ba02_optimized);
    print_result("ba03", "20,000 strings", ba03_initial, ba03_optimized);
    print_result("ba04", "65,536 pairs x 256", ba04_initial, ba04_optimized);

    Ok(())
}

fn build_byte_dataset(size: usize) -> Vec<u8> {
    const PATTERN: &[u8] = b"alpha beta gamma\ndelta epsilon zeta\n";

    let mut data = Vec::with_capacity(size);
    while data.len() + PATTERN.len() <= size {
        data.extend_from_slice(PATTERN);
    }
    data.extend_from_slice(&PATTERN[..size - data.len()]);
    data
}

fn build_sort_dataset(size: usize) -> Vec<String> {
    let mut state = 0x4d59_5df4_d0f3_3173_u64;
    let mut values = Vec::with_capacity(size);

    for index in 0..size {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        values.push(format!("{state:016x}-{index:05}"));
    }

    values
}

fn reader(data: &[u8]) -> BufReader<Cursor<&[u8]>> {
    BufReader::with_capacity(READER_CAPACITY, Cursor::new(data))
}

fn verify_equivalence(bytes: &[u8], sort_input: &[String]) -> Result<()> {
    let initial_bytes = count_bytes_initial(reader(bytes))?;
    let optimized_bytes = count_bytes(reader(bytes))?;
    assert_eq!(initial_bytes, optimized_bytes);

    let initial_counts = count_lines_words_bytes_initial(reader(bytes))?;
    let optimized_counts = count_lines_words_bytes(reader(bytes))?;
    assert_eq!(initial_counts, optimized_counts);

    let mut initial_sort = sort_input.to_vec();
    let mut optimized_sort = sort_input.to_vec();
    sort_arguments_initial(&mut initial_sort);
    sort_arguments(&mut optimized_sort);
    assert_eq!(initial_sort, optimized_sort);

    for a in u8::MIN..=u8::MAX {
        for b in u8::MIN..=u8::MAX {
            assert_eq!(add_u8_wrapping_initial(a, b), add_u8_wrapping(a, b));
        }
    }

    Ok(())
}

fn counts_checksum(counts: Counts) -> u64 {
    counts.lines ^ counts.words.rotate_left(21) ^ counts.bytes.rotate_left(42)
}

fn measure_reader<'a>(
    data: &'a [u8],
    mut operation: impl FnMut(BufReader<Cursor<&'a [u8]>>) -> io::Result<u64>,
) -> io::Result<Duration> {
    black_box(operation(reader(data))?);

    let mut samples = Vec::with_capacity(SAMPLES);
    for _ in 0..SAMPLES {
        let input = reader(data);
        let started = Instant::now();
        let result = operation(input)?;
        samples.push(started.elapsed());
        black_box(result);
    }

    Ok(median(samples))
}

fn measure_sort(input: &[String], operation: fn(&mut [String])) -> Duration {
    let mut warm_up = input.to_vec();
    operation(&mut warm_up);
    black_box(&warm_up);

    let mut samples = Vec::with_capacity(SAMPLES);
    for _ in 0..SAMPLES {
        let mut values = input.to_vec();
        let started = Instant::now();
        operation(&mut values);
        samples.push(started.elapsed());
        black_box(&values);
    }

    median(samples)
}

fn measure_u8(operation: fn(u8, u8) -> u8) -> Duration {
    black_box(run_u8_workload(operation));

    let mut samples = Vec::with_capacity(SAMPLES);
    for _ in 0..SAMPLES {
        let started = Instant::now();
        let checksum = run_u8_workload(operation);
        samples.push(started.elapsed());
        black_box(checksum);
    }

    median(samples)
}

fn run_u8_workload(operation: fn(u8, u8) -> u8) -> u64 {
    let mut checksum = 0_u64;

    for _ in 0..U8_REPETITIONS {
        for a in u8::MIN..=u8::MAX {
            for b in u8::MIN..=u8::MAX {
                let result = operation(black_box(a), black_box(b));
                checksum = checksum.wrapping_add(u64::from(black_box(result)));
            }
        }
    }

    checksum
}

fn median(mut samples: Vec<Duration>) -> Duration {
    samples.sort_unstable();
    samples[samples.len() / 2]
}

fn print_result(name: &str, dataset: &str, initial: Duration, optimized: Duration) {
    let ratio = initial.as_secs_f64() / optimized.as_secs_f64();

    println!(
        "{name:<6} | {dataset:<24} | {:>11.3} ms | {:>11.3} ms | {:>16.2}x",
        initial.as_secs_f64() * 1_000.0,
        optimized.as_secs_f64() * 1_000.0,
        ratio,
    );
}
