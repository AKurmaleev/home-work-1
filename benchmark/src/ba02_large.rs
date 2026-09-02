use std::error::Error;
use std::hint::black_box;
use std::io::{self, BufReader, Cursor, Read, Write};
use std::time::{Duration, Instant};

use ba02::{Counts, count_lines_words_bytes, count_lines_words_bytes_initial};

const MIB: usize = 1024 * 1024;
const DEFAULT_SIZE_MIB: usize = 256;
const DEFAULT_SAMPLES: usize = 31;
const READER_CAPACITY: usize = 64 * 1024;
const WARM_UP_PAIRS: usize = 3;
const PRACTICAL_SPEEDUP: f64 = 1.05;
const BOOTSTRAP_RESAMPLES: usize = 10_000;
const BOOTSTRAP_SEED: u64 = 0x8b8b_8b8b_02a5_2024;
const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
const MIXED_SEED: u64 = 0x4d59_5df4_d0f3_3173;
const LCG_MULTIPLIER: u64 = 6_364_136_223_846_793_005;
const LCG_INCREMENT: u64 = 1_442_695_040_888_963_407;
const PROSE_PATTERN: &[u8] = b"alpha beta gamma\ndelta epsilon zeta\n";
const BOUNDARY_PERIOD: usize = READER_CAPACITY + 17;

const DATASETS: [DatasetKind; 3] = [
    DatasetKind::Prose,
    DatasetKind::Boundary,
    DatasetKind::Mixed,
];

type DynResult<T> = Result<T, Box<dyn Error>>;
type SliceReader<'a> = BufReader<Cursor<&'a [u8]>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DatasetKind {
    Prose,
    Boundary,
    Mixed,
}

impl DatasetKind {
    fn parse(value: &str) -> DynResult<Self> {
        match value {
            "prose" => Ok(Self::Prose),
            "boundary" => Ok(Self::Boundary),
            "mixed" => Ok(Self::Mixed),
            _ => Err(invalid_input(format!(
                "unknown dataset {value:?}; expected prose, boundary, or mixed"
            ))),
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Prose => "prose",
            Self::Boundary => "boundary",
            Self::Mixed => "mixed",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Variant {
    Initial,
    Optimized,
}

impl Variant {
    fn parse(value: &str) -> DynResult<Self> {
        match value {
            "initial" => Ok(Self::Initial),
            "optimized" => Ok(Self::Optimized),
            _ => Err(invalid_input(format!(
                "unknown variant {value:?}; expected initial or optimized"
            ))),
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Initial => "initial",
            Self::Optimized => "optimized",
        }
    }
}

#[derive(Debug)]
struct DeterministicInput {
    dataset: DatasetKind,
    remaining: usize,
    position: usize,
    mixed_state: u64,
}

impl DeterministicInput {
    const fn new(dataset: DatasetKind, size: usize) -> Self {
        Self {
            dataset,
            remaining: size,
            position: 0,
            mixed_state: MIXED_SEED,
        }
    }

    fn next_byte(&mut self) -> u8 {
        let byte = match self.dataset {
            DatasetKind::Prose => PROSE_PATTERN[self.position % PROSE_PATTERN.len()],
            DatasetKind::Boundary => match self.position % BOUNDARY_PERIOD {
                0 => b' ',
                offset if offset + 1 == BOUNDARY_PERIOD => b'\n',
                _ => b'a',
            },
            DatasetKind::Mixed => {
                self.mixed_state = self
                    .mixed_state
                    .wrapping_mul(LCG_MULTIPLIER)
                    .wrapping_add(LCG_INCREMENT);
                let selector = (self.mixed_state >> 60) as u8;
                match selector {
                    0 => b'\t',
                    1 => b'\n',
                    2 => 0x0b,
                    3 => 0x0c,
                    4 => b'\r',
                    5 => b' ',
                    _ => b'a' + ((self.mixed_state >> 32) % 26) as u8,
                }
            }
        };

        self.position += 1;
        self.remaining -= 1;
        byte
    }
}

impl Read for DeterministicInput {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let read_len = buffer.len().min(self.remaining);
        for byte in &mut buffer[..read_len] {
            *byte = self.next_byte();
        }
        Ok(read_len)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Analysis {
    counts: Counts,
    fingerprint: u64,
}

#[derive(Debug)]
struct ReferenceCounter {
    lines: usize,
    words: usize,
    bytes: usize,
    inside_word: bool,
    fingerprint: u64,
}

impl ReferenceCounter {
    const fn new() -> Self {
        Self {
            lines: 0,
            words: 0,
            bytes: 0,
            inside_word: false,
            fingerprint: FNV_OFFSET_BASIS,
        }
    }

    fn update(&mut self, bytes: &[u8]) {
        self.bytes += bytes.len();

        for &byte in bytes {
            if byte == b'\n' {
                self.lines += 1;
            }

            if is_reference_whitespace(byte) {
                self.inside_word = false;
            } else if !self.inside_word {
                self.words += 1;
                self.inside_word = true;
            }

            self.fingerprint ^= u64::from(byte);
            self.fingerprint = self.fingerprint.wrapping_mul(FNV_PRIME);
        }
    }

    fn finish(self) -> io::Result<Analysis> {
        Ok(Analysis {
            counts: Counts {
                lines: u64::try_from(self.lines)
                    .map_err(|_| io::Error::other("reference line count exceeds u64"))?,
                words: u64::try_from(self.words)
                    .map_err(|_| io::Error::other("reference word count exceeds u64"))?,
                bytes: u64::try_from(self.bytes)
                    .map_err(|_| io::Error::other("reference byte count exceeds u64"))?,
            },
            fingerprint: self.fingerprint,
        })
    }
}

#[derive(Clone, Copy, Debug)]
struct SamplePair {
    pair: usize,
    order: &'static str,
    initial: Duration,
    optimized: Duration,
}

#[derive(Clone, Copy, Debug)]
struct RuntimeSummary {
    initial_median_ns: u128,
    initial_mad_ns: u128,
    optimized_median_ns: u128,
    optimized_mad_ns: u128,
    paired_median_speedup: f64,
    ci95_low: f64,
    ci95_high: f64,
}

#[derive(Debug)]
struct DeterministicRng(u64);

impl DeterministicRng {
    const fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next_u64(&mut self) -> u64 {
        let mut value = self.0;
        value ^= value << 13;
        value ^= value >> 7;
        value ^= value << 17;
        self.0 = value;
        value
    }

    fn index(&mut self, len: usize) -> usize {
        (self.next_u64() as usize) % len
    }
}

pub fn run(args: &[String]) -> Result<(), Box<dyn Error>> {
    match args.first().map(String::as_str) {
        Some("runtime") => run_runtime(args),
        Some("once") => run_once(args),
        _ => Err(invalid_input(
            "usage: runtime [size_mib] [samples] | once <initial|optimized> \
             <prose|boundary|mixed> [size_mib]",
        )),
    }
}

fn run_runtime(args: &[String]) -> DynResult<()> {
    if args.len() > 3 {
        return Err(invalid_input("runtime accepts [size_mib] [samples]"));
    }

    let size_mib = parse_positive_usize(args.get(1), DEFAULT_SIZE_MIB, "size_mib")?;
    let samples = parse_positive_usize(args.get(2), DEFAULT_SAMPLES, "samples")?;
    let size = size_in_bytes(size_mib)?;
    let stdout = io::stdout();
    let mut output = io::BufWriter::new(stdout.lock());

    writeln!(
        output,
        "{{\"record\":\"config\",\"mode\":\"runtime\",\"size_mib\":{size_mib},\
         \"size_bytes\":{size},\"samples\":{samples},\"warm_up_pairs\":{WARM_UP_PAIRS},\
         \"reader_capacity\":{READER_CAPACITY},\"bootstrap_resamples\":{BOOTSTRAP_RESAMPLES},\
         \"paired_statistic\":\"exp_median_log_ratio\",\
         \"ci_method\":\"deterministic_paired_percentile_bootstrap\",\
         \"practical_threshold\":{PRACTICAL_SPEEDUP:.2}}}"
    )?;

    for dataset in DATASETS {
        let bytes = build_dataset(dataset, size)?;
        let analysis = verify_dataset(&bytes)?;

        writeln!(
            output,
            "{{\"record\":\"dataset\",\"dataset\":\"{}\",\"size_bytes\":{},\
             \"fingerprint\":\"{:016x}\",\"lines\":{},\"words\":{},\"bytes\":{},\
             \"checksum\":\"{:016x}\"}}",
            dataset.name(),
            size,
            analysis.fingerprint,
            analysis.counts.lines,
            analysis.counts.words,
            analysis.counts.bytes,
            counts_checksum(analysis.counts),
        )?;
        output.flush()?;

        warm_up(&bytes)?;
        let pairs = measure_pairs(&bytes, samples)?;

        for pair in &pairs {
            let initial_ns = pair.initial.as_nanos();
            let optimized_ns = pair.optimized.as_nanos();
            let speedup = initial_ns as f64 / optimized_ns as f64;
            writeln!(
                output,
                "{{\"record\":\"sample\",\"dataset\":\"{}\",\"pair\":{},\
                 \"order\":\"{}\",\"initial_ns\":{},\"optimized_ns\":{},\
                 \"speedup\":{speedup:.9}}}",
                dataset.name(),
                pair.pair,
                pair.order,
                initial_ns,
                optimized_ns,
            )?;
        }

        let summary = summarize_runtime(&pairs)?;
        let verdict = performance_verdict(summary);
        writeln!(
            output,
            "{{\"record\":\"summary\",\"dataset\":\"{}\",\"samples\":{},\
             \"initial_median_ns\":{},\"initial_mad_ns\":{},\
             \"optimized_median_ns\":{},\"optimized_mad_ns\":{},\
             \"paired_median_speedup\":{:.9},\"ci95_low\":{:.9},\
             \"ci95_high\":{:.9},\"practical_threshold\":{PRACTICAL_SPEEDUP:.2},\
             \"verdict\":\"{}\"}}",
            dataset.name(),
            pairs.len(),
            summary.initial_median_ns,
            summary.initial_mad_ns,
            summary.optimized_median_ns,
            summary.optimized_mad_ns,
            summary.paired_median_speedup,
            summary.ci95_low,
            summary.ci95_high,
            verdict,
        )?;
        output.flush()?;
    }

    Ok(())
}

fn run_once(args: &[String]) -> DynResult<()> {
    if !(3..=4).contains(&args.len()) {
        return Err(invalid_input(
            "once requires <initial|optimized> <prose|boundary|mixed> [size_mib]",
        ));
    }

    let variant = Variant::parse(&args[1])?;
    let dataset = DatasetKind::parse(&args[2])?;
    let size_mib = parse_positive_usize(args.get(3), DEFAULT_SIZE_MIB, "size_mib")?;
    let size = size_in_bytes(size_mib)?;
    let expected = analyze_reader(DeterministicInput::new(dataset, size))?;
    let input = black_box(DeterministicInput::new(dataset, size));
    let reader = BufReader::with_capacity(READER_CAPACITY, input);
    let counts = match variant {
        Variant::Initial => count_lines_words_bytes_initial(reader)?,
        Variant::Optimized => count_lines_words_bytes(reader)?,
    };
    black_box(counts);

    if counts != expected.counts {
        return Err(io::Error::other(format!(
            "{} result mismatch for {}: expected {:?}, got {:?}",
            variant.name(),
            dataset.name(),
            expected.counts,
            counts
        ))
        .into());
    }

    let stdout = io::stdout();
    let mut output = io::BufWriter::new(stdout.lock());
    writeln!(
        output,
        "{{\"record\":\"once\",\"variant\":\"{}\",\"dataset\":\"{}\",\
         \"size_mib\":{},\"size_bytes\":{},\"reader_capacity\":{},\
         \"lines\":{},\"words\":{},\"bytes\":{},\"fingerprint\":\"{:016x}\",\
         \"checksum\":\"{:016x}\"}}",
        variant.name(),
        dataset.name(),
        size_mib,
        size,
        READER_CAPACITY,
        counts.lines,
        counts.words,
        counts.bytes,
        expected.fingerprint,
        counts_checksum(counts),
    )?;
    output.flush()?;

    Ok(())
}

fn parse_positive_usize(value: Option<&String>, default: usize, name: &str) -> DynResult<usize> {
    let Some(value) = value else {
        return Ok(default);
    };
    let parsed = value
        .parse::<usize>()
        .map_err(|error| invalid_input(format!("invalid {name} {value:?}: {error}")))?;
    if parsed == 0 {
        return Err(invalid_input(format!("{name} must be greater than zero")));
    }
    Ok(parsed)
}

fn size_in_bytes(size_mib: usize) -> DynResult<usize> {
    size_mib
        .checked_mul(MIB)
        .ok_or_else(|| invalid_input("size_mib exceeds addressable memory"))
}

fn invalid_input(message: impl Into<String>) -> Box<dyn Error> {
    Box::new(io::Error::new(io::ErrorKind::InvalidInput, message.into()))
}

const fn is_reference_whitespace(byte: u8) -> bool {
    matches!(byte, b'\t' | b'\n' | 0x0c | b'\r' | b' ')
}

fn build_dataset(dataset: DatasetKind, size: usize) -> io::Result<Vec<u8>> {
    let mut bytes = vec![0; size];
    DeterministicInput::new(dataset, size).read_exact(&mut bytes)?;
    Ok(bytes)
}

fn analyze_bytes(bytes: &[u8]) -> io::Result<Analysis> {
    let mut reference = ReferenceCounter::new();
    reference.update(bytes);
    reference.finish()
}

fn analyze_reader(mut input: impl Read) -> io::Result<Analysis> {
    let mut reference = ReferenceCounter::new();
    let mut buffer = [0_u8; 8 * 1024];

    loop {
        let read = input.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        reference.update(&buffer[..read]);
    }

    reference.finish()
}

fn verify_dataset(bytes: &[u8]) -> io::Result<Analysis> {
    let expected = analyze_bytes(bytes)?;
    let initial = run_slice_untimed(Variant::Initial, bytes)?;
    let optimized = run_slice_untimed(Variant::Optimized, bytes)?;
    black_box((initial, optimized));

    if initial != expected.counts || optimized != expected.counts {
        return Err(io::Error::other(format!(
            "correctness mismatch: expected {:?}, initial {:?}, optimized {:?}",
            expected.counts, initial, optimized
        )));
    }

    Ok(expected)
}

fn slice_reader(bytes: &[u8]) -> SliceReader<'_> {
    let bytes = black_box(bytes);
    BufReader::with_capacity(READER_CAPACITY, Cursor::new(bytes))
}

fn run_slice_untimed(variant: Variant, bytes: &[u8]) -> io::Result<Counts> {
    let reader = slice_reader(bytes);
    match variant {
        Variant::Initial => count_lines_words_bytes_initial(reader),
        Variant::Optimized => count_lines_words_bytes(reader),
    }
}

fn sample_order(pair: usize) -> [Variant; 2] {
    if pair.is_multiple_of(2) {
        [Variant::Initial, Variant::Optimized]
    } else {
        [Variant::Optimized, Variant::Initial]
    }
}

fn warm_up(bytes: &[u8]) -> io::Result<()> {
    for pair in 0..WARM_UP_PAIRS {
        for variant in sample_order(pair) {
            black_box(run_slice_untimed(variant, bytes)?);
        }
    }
    Ok(())
}

fn measure_pairs(bytes: &[u8], samples: usize) -> io::Result<Vec<SamplePair>> {
    let mut pairs = Vec::with_capacity(samples);

    for pair in 0..samples {
        let order = sample_order(pair);
        let first = measure_variant(order[0], bytes)?;
        let second = measure_variant(order[1], bytes)?;
        let (initial, optimized, order_name) = match order {
            [Variant::Initial, Variant::Optimized] => (first, second, "initial-optimized"),
            [Variant::Optimized, Variant::Initial] => (second, first, "optimized-initial"),
            _ => unreachable!("sample order always contains both variants"),
        };

        if initial.is_zero() || optimized.is_zero() {
            return Err(io::Error::other(
                "timer resolution produced a zero-duration sample",
            ));
        }

        pairs.push(SamplePair {
            pair,
            order: order_name,
            initial,
            optimized,
        });
    }

    Ok(pairs)
}

fn measure_variant(variant: Variant, bytes: &[u8]) -> io::Result<Duration> {
    let reader = slice_reader(bytes);
    match variant {
        Variant::Initial => measure_operation(reader, count_lines_words_bytes_initial),
        Variant::Optimized => measure_operation(reader, count_lines_words_bytes),
    }
}

fn measure_operation<R>(
    reader: R,
    operation: impl FnOnce(R) -> io::Result<Counts>,
) -> io::Result<Duration> {
    let started = Instant::now();
    let counts = operation(reader)?;
    let elapsed = started.elapsed();
    black_box(counts);
    Ok(elapsed)
}

fn counts_checksum(counts: Counts) -> u64 {
    counts.lines ^ counts.words.rotate_left(21) ^ counts.bytes.rotate_left(42)
}

fn summarize_runtime(pairs: &[SamplePair]) -> io::Result<RuntimeSummary> {
    if pairs.is_empty() {
        return Err(io::Error::other("cannot summarize an empty sample set"));
    }

    let initial: Vec<u128> = pairs.iter().map(|pair| pair.initial.as_nanos()).collect();
    let optimized: Vec<u128> = pairs.iter().map(|pair| pair.optimized.as_nanos()).collect();
    let ratios: Vec<f64> = pairs
        .iter()
        .map(|pair| pair.initial.as_secs_f64() / pair.optimized.as_secs_f64())
        .collect();
    let paired_median_speedup = paired_median_speedup(&ratios);
    let (ci95_low, ci95_high) = paired_bootstrap_ci(&ratios, BOOTSTRAP_RESAMPLES);

    Ok(RuntimeSummary {
        initial_median_ns: median_u128(&initial),
        initial_mad_ns: median_absolute_deviation(&initial),
        optimized_median_ns: median_u128(&optimized),
        optimized_mad_ns: median_absolute_deviation(&optimized),
        paired_median_speedup,
        ci95_low,
        ci95_high,
    })
}

fn performance_verdict(summary: RuntimeSummary) -> &'static str {
    if summary.ci95_low >= PRACTICAL_SPEEDUP {
        "improved"
    } else if summary.ci95_high < 1.0 {
        "regressed"
    } else if summary.ci95_low > 1.0 {
        "positive_but_practical_gain_not_established"
    } else {
        "inconclusive"
    }
}

fn median_u128(values: &[u128]) -> u128 {
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let middle = sorted.len() / 2;
    if sorted.len().is_multiple_of(2) {
        sorted[middle - 1] + (sorted[middle] - sorted[middle - 1]) / 2
    } else {
        sorted[middle]
    }
}

fn median_absolute_deviation(values: &[u128]) -> u128 {
    let median = median_u128(values);
    let deviations: Vec<u128> = values.iter().map(|value| value.abs_diff(median)).collect();
    median_u128(&deviations)
}

fn median_f64(values: &[f64]) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let middle = sorted.len() / 2;
    if sorted.len().is_multiple_of(2) {
        (sorted[middle - 1] + sorted[middle]) / 2.0
    } else {
        sorted[middle]
    }
}

fn paired_median_speedup(ratios: &[f64]) -> f64 {
    let log_ratios: Vec<f64> = ratios.iter().map(|ratio| ratio.ln()).collect();
    median_f64(&log_ratios).exp()
}

fn paired_bootstrap_ci(ratios: &[f64], resamples: usize) -> (f64, f64) {
    assert!(!ratios.is_empty(), "bootstrap requires paired ratios");
    assert!(resamples > 0, "bootstrap requires resamples");

    let log_ratios: Vec<f64> = ratios.iter().map(|ratio| ratio.ln()).collect();
    let mut rng = DeterministicRng::new(BOOTSTRAP_SEED);
    let mut sample = Vec::with_capacity(log_ratios.len());
    let mut estimates = Vec::with_capacity(resamples);

    for _ in 0..resamples {
        sample.clear();
        for _ in 0..log_ratios.len() {
            sample.push(log_ratios[rng.index(log_ratios.len())]);
        }
        estimates.push(median_f64(&sample).exp());
    }

    estimates.sort_by(f64::total_cmp);
    (percentile(&estimates, 0.025), percentile(&estimates, 0.975))
}

fn percentile(sorted: &[f64], probability: f64) -> f64 {
    let position = (sorted.len() - 1) as f64 * probability;
    let lower = position.floor() as usize;
    let upper = position.ceil() as usize;
    if lower == upper {
        sorted[lower]
    } else {
        let fraction = position - lower as f64;
        sorted[lower] + (sorted[upper] - sorted[lower]) * fraction
    }
}

#[cfg(test)]
mod tests {
    use std::io::Read;
    use std::time::Duration;

    use super::{
        BOOTSTRAP_RESAMPLES, BOUNDARY_PERIOD, Counts, DATASETS, DatasetKind, DeterministicInput,
        READER_CAPACITY, SamplePair, Variant, analyze_bytes, build_dataset,
        median_absolute_deviation, median_u128, paired_bootstrap_ci, run_slice_untimed,
        sample_order, summarize_runtime,
    };

    fn read_in_chunks(dataset: DatasetKind, size: usize, chunk_size: usize) -> Vec<u8> {
        let mut input = DeterministicInput::new(dataset, size);
        let mut chunk = vec![0; chunk_size];
        let mut output = Vec::with_capacity(size);

        loop {
            let read = input.read(&mut chunk).unwrap();
            if read == 0 {
                break;
            }
            output.extend_from_slice(&chunk[..read]);
        }

        output
    }

    #[test]
    fn generation_is_independent_of_read_chunk_size() {
        for dataset in DATASETS {
            let expected = read_in_chunks(dataset, BOUNDARY_PERIOD * 2 + 29, 1);
            for chunk_size in [
                2,
                17,
                READER_CAPACITY - 1,
                READER_CAPACITY,
                READER_CAPACITY + 1,
            ] {
                assert_eq!(
                    read_in_chunks(dataset, expected.len(), chunk_size),
                    expected,
                    "dataset {} changed with chunk size {chunk_size}",
                    dataset.name()
                );
            }
        }
    }

    #[test]
    fn prose_generation_has_known_reference_counts() {
        let bytes = build_dataset(DatasetKind::Prose, super::PROSE_PATTERN.len()).unwrap();
        assert_eq!(bytes, super::PROSE_PATTERN);

        let analysis = analyze_bytes(&bytes).unwrap();
        assert_eq!(
            analysis.counts,
            Counts {
                lines: 2,
                words: 6,
                bytes: 36,
            }
        );
        assert_eq!(
            run_slice_untimed(Variant::Initial, &bytes).unwrap(),
            analysis.counts
        );
        assert_eq!(
            run_slice_untimed(Variant::Optimized, &bytes).unwrap(),
            analysis.counts
        );
    }

    #[test]
    fn reference_matches_rust_ascii_whitespace_definition() {
        let bytes = b"a\x0bb\x0cc";
        let expected = Counts {
            lines: 0,
            words: 2,
            bytes: bytes.len() as u64,
        };

        assert_eq!(analyze_bytes(bytes).unwrap().counts, expected);
        assert_eq!(
            run_slice_untimed(Variant::Initial, bytes).unwrap(),
            expected
        );
        assert_eq!(
            run_slice_untimed(Variant::Optimized, bytes).unwrap(),
            expected
        );
    }

    #[test]
    fn boundary_dataset_preserves_word_state_across_reader_buffer() {
        let size = READER_CAPACITY + 8;
        let bytes = build_dataset(DatasetKind::Boundary, size).unwrap();
        let expected = Counts {
            lines: 0,
            words: 1,
            bytes: size as u64,
        };

        assert_eq!(analyze_bytes(&bytes).unwrap().counts, expected);
        assert_eq!(
            run_slice_untimed(Variant::Initial, &bytes).unwrap(),
            expected
        );
        assert_eq!(
            run_slice_untimed(Variant::Optimized, &bytes).unwrap(),
            expected
        );
    }

    #[test]
    fn sample_order_alternates_ab_and_ba() {
        assert_eq!(sample_order(0), [Variant::Initial, Variant::Optimized]);
        assert_eq!(sample_order(1), [Variant::Optimized, Variant::Initial]);
        assert_eq!(sample_order(2), [Variant::Initial, Variant::Optimized]);
    }

    #[test]
    fn median_mad_and_bootstrap_are_deterministic() {
        let values = [10, 20, 30, 40, 100];
        assert_eq!(median_u128(&values), 30);
        assert_eq!(median_absolute_deviation(&values), 10);

        let ratios = [1.10, 1.20, 1.30, 1.40, 1.50];
        let first = paired_bootstrap_ci(&ratios, BOOTSTRAP_RESAMPLES);
        let second = paired_bootstrap_ci(&ratios, BOOTSTRAP_RESAMPLES);
        assert_eq!(first, second);
        assert!(first.0 <= 1.30);
        assert!(first.1 >= 1.30);

        let pairs = [
            SamplePair {
                pair: 0,
                order: "initial-optimized",
                initial: Duration::from_nanos(110),
                optimized: Duration::from_nanos(100),
            },
            SamplePair {
                pair: 1,
                order: "optimized-initial",
                initial: Duration::from_nanos(240),
                optimized: Duration::from_nanos(200),
            },
            SamplePair {
                pair: 2,
                order: "initial-optimized",
                initial: Duration::from_nanos(390),
                optimized: Duration::from_nanos(300),
            },
        ];
        let summary = summarize_runtime(&pairs).unwrap();
        assert_eq!(summary.initial_median_ns, 240);
        assert_eq!(summary.initial_mad_ns, 130);
        assert_eq!(summary.optimized_median_ns, 200);
        assert_eq!(summary.optimized_mad_ns, 100);
        assert!((summary.paired_median_speedup - 1.2).abs() < f64::EPSILON);
    }
}
