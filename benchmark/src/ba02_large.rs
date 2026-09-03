use std::error::Error;
use std::fs::File;
use std::hint::black_box;
use std::io::{self, BufRead, BufReader, Cursor, Read, Write};
use std::time::{Duration, Instant};

use ba02::{Counts, count_lines_words_bytes, count_lines_words_bytes_initial};

const MIB: usize = 1024 * 1024;
const DEFAULT_SIZE_MIB: usize = 256;
const DEFAULT_SAMPLES: usize = 101;
const MIN_MEASURED_PAIRS: usize = 101;
const READER_CAPACITY: usize = 64 * 1024;
const WARM_UP_PAIRS: usize = 3;
const TRIM_NUMERATOR: usize = 15;
const TRIM_DENOMINATOR: usize = 100;
const MIN_PRACTICAL_EFFECT: f64 = 0.05;
const PRACTICAL_SPEEDUP: f64 = 1.0 + MIN_PRACTICAL_EFFECT;
const CONFIDENCE_LEVEL: f64 = 0.95;
const MAX_RELATIVE_CI_TOTAL_WIDTH: f64 = 0.10;
const BOOTSTRAP_RESAMPLES: usize = 10_000;
const BOOTSTRAP_SEED: u64 = 0x8b8b_8b8b_02a5_2024;
const RSS_CONFIDENCE_LEVEL: f64 = 0.95;
const RSS_PRACTICAL_REDUCTION: f64 = 0.05;
const RSS_EQUIVALENCE_MARGIN: f64 = 0.01;
const RSS_MAX_ABSOLUTE_CI_TOTAL_WIDTH: f64 = 0.005;
const RSS_MIN_RAW_SAMPLES: usize = 31;
const RSS_BOOTSTRAP_RESAMPLES: usize = 10_000;
const RSS_BOOTSTRAP_SEED: u64 = 0xd1b5_4a32_d192_ed03;
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PairDisposition {
    Low,
    Retained,
    High,
}

impl PairDisposition {
    const fn name(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Retained => "retained",
            Self::High => "high",
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct PairedSelection {
    trim_each_tail: usize,
    dispositions: Vec<PairDisposition>,
    retained_indices: Vec<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct RuntimeSummary {
    raw_pairs: usize,
    trim_each_tail: usize,
    retained_pairs: usize,
    initial_trimmed_mean_ns: f64,
    initial_trimmed_median_ns: u128,
    initial_trimmed_mad_ns: u128,
    optimized_trimmed_mean_ns: f64,
    optimized_trimmed_median_ns: u128,
    optimized_trimmed_mad_ns: u128,
    ratio_of_trimmed_means: f64,
    paired_median_speedup: f64,
    ci95_low: f64,
    ci95_high: f64,
    ci_relative_total_width: f64,
    adequacy_criterion_met: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RssDisposition {
    Low,
    Retained,
    High,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RssSample {
    sample_id: usize,
    acquisition_index: usize,
    value_kib: u64,
}

#[derive(Debug, PartialEq, Eq)]
struct RssSelection {
    trim_each_tail: usize,
    dispositions: Vec<RssDisposition>,
    retained_indices: Vec<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct RssSummary {
    initial_raw_samples: usize,
    initial_trim_each_tail: usize,
    initial_retained_samples: usize,
    optimized_raw_samples: usize,
    optimized_trim_each_tail: usize,
    optimized_retained_samples: usize,
    initial_trimmed_mean_kib: f64,
    initial_trimmed_median_kib: f64,
    initial_trimmed_mad_kib: f64,
    optimized_trimmed_mean_kib: f64,
    optimized_trimmed_median_kib: f64,
    optimized_trimmed_mad_kib: f64,
    baseline_candidate_mean_ratio: f64,
    relative_reduction: f64,
    relative_reduction_ci95_low: f64,
    relative_reduction_ci95_high: f64,
    ci_absolute_total_width: f64,
    adequacy_criterion_met: bool,
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
        Some("rss-summary") => run_rss_summary(args),
        _ => Err(invalid_input(
            "usage: runtime [size_mib] [samples] | once <initial|optimized> \
             <prose|boundary|mixed> [size_mib] | rss-summary \
             <initial_values_file> <optimized_values_file>",
        )),
    }
}

fn run_runtime(args: &[String]) -> DynResult<()> {
    if args.len() > 3 {
        return Err(invalid_input("runtime accepts [size_mib] [samples]"));
    }

    let size_mib = parse_positive_usize(args.get(1), DEFAULT_SIZE_MIB, "size_mib")?;
    let samples = parse_positive_usize(args.get(2), DEFAULT_SAMPLES, "samples")?;
    if samples < MIN_MEASURED_PAIRS {
        return Err(invalid_input(format!(
            "samples must be at least {MIN_MEASURED_PAIRS} measured pairs"
        )));
    }
    let size = size_in_bytes(size_mib)?;
    let stdout = io::stdout();
    let mut output = io::BufWriter::new(stdout.lock());

    writeln!(
        output,
        "{{\"record\":\"config\",\"mode\":\"runtime\",\"size_mib\":{size_mib},\
         \"size_bytes\":{size},\"samples\":{samples},\
         \"default_measured_pairs\":{DEFAULT_SAMPLES},\
         \"minimum_measured_pairs\":{MIN_MEASURED_PAIRS},\
         \"warm_up_pairs\":{WARM_UP_PAIRS},\"reader_capacity\":{READER_CAPACITY},\
         \"trim_numerator\":{TRIM_NUMERATOR},\"trim_denominator\":{TRIM_DENOMINATOR},\
         \"selection_unit\":\"whole_pair\",\
         \"selection_sort_key\":\"ln(initial_ns/optimized_ns)\",\
         \"selection_tie_break\":\"pair_id_ascending\",\
         \"primary_effect\":\"exp(median(retained_ln_initial_over_optimized))\",\
         \"bootstrap_resamples\":{BOOTSTRAP_RESAMPLES},\
         \"bootstrap_input\":\"retained_pairs_only\",\"bootstrap_retrim\":false,\
         \"ci_method\":\"deterministic_paired_percentile_bootstrap\",\
         \"confidence_level\":{CONFIDENCE_LEVEL:.2},\
         \"minimum_practical_effect\":{MIN_PRACTICAL_EFFECT:.2},\
         \"practical_speedup_threshold\":{PRACTICAL_SPEEDUP:.2},\
         \"adequacy_criterion\":\"(ci_high-ci_low)/primary_effect<=0.10\"}}"
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
        let selection = select_paired_samples(&pairs)?;

        for (pair, disposition) in pairs.iter().zip(&selection.dispositions) {
            write_sample_record(&mut output, dataset, pair, *disposition)?;
        }

        let summary = summarize_runtime(&pairs, &selection)?;
        let verdict = performance_verdict(summary);
        writeln!(
            output,
            "{{\"record\":\"summary\",\"dataset\":\"{}\",\
             \"raw_pairs\":{},\"trim_each_tail\":{},\"retained_pairs\":{},\
             \"initial_trimmed_mean_ns\":{:.3},\"initial_trimmed_median_ns\":{},\
             \"initial_trimmed_mad_ns\":{},\"optimized_trimmed_mean_ns\":{:.3},\
             \"optimized_trimmed_median_ns\":{},\"optimized_trimmed_mad_ns\":{},\
             \"ratio_of_trimmed_means\":{:.9},\
             \"paired_median_speedup\":{:.9},\"ci95_low\":{:.9},\
             \"ci95_high\":{:.9},\"ci_relative_total_width\":{:.9},\
             \"adequacy_criterion_met\":{},\
             \"minimum_practical_effect\":{MIN_PRACTICAL_EFFECT:.2},\
             \"practical_speedup_threshold\":{PRACTICAL_SPEEDUP:.2},\
             \"verdict\":\"{}\"}}",
            dataset.name(),
            summary.raw_pairs,
            summary.trim_each_tail,
            summary.retained_pairs,
            summary.initial_trimmed_mean_ns,
            summary.initial_trimmed_median_ns,
            summary.initial_trimmed_mad_ns,
            summary.optimized_trimmed_mean_ns,
            summary.optimized_trimmed_median_ns,
            summary.optimized_trimmed_mad_ns,
            summary.ratio_of_trimmed_means,
            summary.paired_median_speedup,
            summary.ci95_low,
            summary.ci95_high,
            summary.ci_relative_total_width,
            summary.adequacy_criterion_met,
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

fn run_rss_summary(args: &[String]) -> DynResult<()> {
    if args.len() != 3 {
        return Err(invalid_input(
            "rss-summary requires <initial_values_file> <optimized_values_file>",
        ));
    }

    let initial = read_rss_samples(&args[1], "initial")?;
    let optimized = read_rss_samples(&args[2], "optimized")?;
    let summary = summarize_rss(&initial, &optimized)?;
    let stdout = io::stdout();
    let mut output = io::BufWriter::new(stdout.lock());
    write_rss_summary_records(&mut output, summary)?;
    output.flush()?;

    Ok(())
}

fn read_rss_samples(path: &str, implementation: &str) -> DynResult<Vec<RssSample>> {
    let file = File::open(path).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("failed to open {implementation} values file {path:?}: {error}"),
        )
    })?;
    Ok(parse_rss_samples(BufReader::new(file), implementation)?)
}

fn parse_rss_samples(input: impl BufRead, implementation: &str) -> io::Result<Vec<RssSample>> {
    let mut samples = Vec::new();

    for (line_index, line) in input.lines().enumerate() {
        let sample_id = line_index + 1;
        let line = line.map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("{implementation} values file line {sample_id} could not be read: {error}"),
            )
        })?;
        let value = line.trim();
        if value.is_empty() {
            continue;
        }

        let value_kib = value
            .parse::<u64>()
            .map_err(|_| invalid_rss_value_error(implementation, sample_id))?;
        if value_kib == 0 {
            return Err(invalid_rss_value_error(implementation, sample_id));
        }

        samples.push(RssSample {
            sample_id,
            acquisition_index: samples.len(),
            value_kib,
        });
    }

    if samples.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{implementation} values file contains no samples"),
        ));
    }
    if samples.len() < RSS_MIN_RAW_SAMPLES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "{implementation} values file requires at least {RSS_MIN_RAW_SAMPLES} samples; found {}",
                samples.len()
            ),
        ));
    }

    Ok(samples)
}

fn invalid_rss_value_error(implementation: &str, sample_id: usize) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!(
            "{implementation} values file line {sample_id} must be a positive integer KiB value"
        ),
    )
}

fn write_rss_summary_records(output: &mut impl Write, summary: RssSummary) -> io::Result<()> {
    writeln!(
        output,
        "{{\"record\":\"config\",\"mode\":\"rss-summary\",\
         \"metric\":\"peak_rss_kib\",\"experimental_unit\":\"one_implementation_measurement\",\
         \"minimum_raw_samples_per_implementation\":{RSS_MIN_RAW_SAMPLES},\
         \"trim_numerator\":{TRIM_NUMERATOR},\"trim_denominator\":{TRIM_DENOMINATOR},\
         \"selection\":\"independent_per_implementation\",\
         \"selection_sort_key\":\"value_kib_ascending\",\
         \"selection_tie_break\":\"sample_id_then_acquisition_index\",\
         \"bootstrap_resamples\":{RSS_BOOTSTRAP_RESAMPLES},\
         \"bootstrap_seed\":{RSS_BOOTSTRAP_SEED},\
         \"bootstrap_input\":\"retained_sets_only\",\"bootstrap_retrim\":false,\
         \"ci_method\":\"deterministic_independent_percentile_bootstrap\",\
         \"confidence_level\":{RSS_CONFIDENCE_LEVEL:.2},\
         \"practical_reduction_threshold\":{RSS_PRACTICAL_REDUCTION:.2},\
         \"equivalence_margin\":{RSS_EQUIVALENCE_MARGIN:.2},\
         \"max_absolute_ci_total_width\":{RSS_MAX_ABSOLUTE_CI_TOTAL_WIDTH:.3}}}"
    )?;

    writeln!(
        output,
        "{{\"record\":\"summary\",\
         \"initial_raw_samples\":{},\"initial_trim_each_tail\":{},\
         \"initial_retained_samples\":{},\"optimized_raw_samples\":{},\
         \"optimized_trim_each_tail\":{},\"optimized_retained_samples\":{},\
         \"initial_trimmed_mean_kib\":{:.6},\"initial_trimmed_median_kib\":{:.6},\
         \"initial_trimmed_mad_kib\":{:.6},\"optimized_trimmed_mean_kib\":{:.6},\
         \"optimized_trimmed_median_kib\":{:.6},\"optimized_trimmed_mad_kib\":{:.6},\
         \"baseline_candidate_mean_ratio\":{:.9},\"relative_reduction\":{:.9},\
         \"relative_reduction_ci95_low\":{:.9},\"relative_reduction_ci95_high\":{:.9},\
         \"ci_absolute_total_width\":{:.9},\"adequacy_criterion_met\":{},\
         \"practical_reduction_threshold\":{RSS_PRACTICAL_REDUCTION:.2},\
         \"equivalence_margin\":{RSS_EQUIVALENCE_MARGIN:.2},\
         \"max_absolute_ci_total_width\":{RSS_MAX_ABSOLUTE_CI_TOTAL_WIDTH:.3},\
         \"verdict\":\"{}\"}}",
        summary.initial_raw_samples,
        summary.initial_trim_each_tail,
        summary.initial_retained_samples,
        summary.optimized_raw_samples,
        summary.optimized_trim_each_tail,
        summary.optimized_retained_samples,
        summary.initial_trimmed_mean_kib,
        summary.initial_trimmed_median_kib,
        summary.initial_trimmed_mad_kib,
        summary.optimized_trimmed_mean_kib,
        summary.optimized_trimmed_median_kib,
        summary.optimized_trimmed_mad_kib,
        summary.baseline_candidate_mean_ratio,
        summary.relative_reduction,
        summary.relative_reduction_ci95_low,
        summary.relative_reduction_ci95_high,
        summary.ci_absolute_total_width,
        summary.adequacy_criterion_met,
        rss_verdict(summary),
    )
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

fn write_sample_record(
    output: &mut impl Write,
    dataset: DatasetKind,
    pair: &SamplePair,
    disposition: PairDisposition,
) -> io::Result<()> {
    let initial_ns = pair.initial.as_nanos();
    let optimized_ns = pair.optimized.as_nanos();
    let speedup = initial_ns as f64 / optimized_ns as f64;
    writeln!(
        output,
        "{{\"record\":\"sample\",\"dataset\":\"{}\",\"pair\":{},\
         \"order\":\"{}\",\"initial_ns\":{},\"optimized_ns\":{},\
         \"speedup\":{speedup:.9},\"selection\":\"{}\"}}",
        dataset.name(),
        pair.pair,
        pair.order,
        initial_ns,
        optimized_ns,
        disposition.name(),
    )
}

const fn trim_count(raw_count: usize) -> usize {
    raw_count / TRIM_DENOMINATOR * TRIM_NUMERATOR
        + (raw_count % TRIM_DENOMINATOR) * TRIM_NUMERATOR / TRIM_DENOMINATOR
}

fn select_rss_samples(samples: &[RssSample]) -> io::Result<RssSelection> {
    if samples.is_empty() {
        return Err(io::Error::other("cannot select an empty RSS sample set"));
    }

    let trim_each_tail = trim_count(samples.len());
    let retained_count = samples.len() - 2 * trim_each_tail;
    if retained_count == 0 {
        return Err(io::Error::other("RSS trimming retained no samples"));
    }

    let mut ranked: Vec<_> = samples
        .iter()
        .enumerate()
        .map(|(index, sample)| {
            (
                index,
                sample.value_kib,
                sample.sample_id,
                sample.acquisition_index,
            )
        })
        .collect();
    ranked.sort_unstable_by(|left, right| {
        left.1
            .cmp(&right.1)
            .then_with(|| left.2.cmp(&right.2))
            .then_with(|| left.3.cmp(&right.3))
            .then_with(|| left.0.cmp(&right.0))
    });

    let mut dispositions = vec![RssDisposition::Retained; samples.len()];
    for &(index, _, _, _) in &ranked[..trim_each_tail] {
        dispositions[index] = RssDisposition::Low;
    }
    for &(index, _, _, _) in &ranked[samples.len() - trim_each_tail..] {
        dispositions[index] = RssDisposition::High;
    }

    let retained_indices = dispositions
        .iter()
        .enumerate()
        .filter_map(|(index, disposition)| {
            (*disposition == RssDisposition::Retained).then_some(index)
        })
        .collect();

    Ok(RssSelection {
        trim_each_tail,
        dispositions,
        retained_indices,
    })
}

fn summarize_rss(initial: &[RssSample], optimized: &[RssSample]) -> io::Result<RssSummary> {
    validate_rss_summary_input(initial, "initial")?;
    validate_rss_summary_input(optimized, "optimized")?;

    let initial_selection = select_rss_samples(initial)?;
    let optimized_selection = select_rss_samples(optimized)?;
    let initial_retained = retained_rss_values(initial, &initial_selection)?;
    let optimized_retained = retained_rss_values(optimized, &optimized_selection)?;
    let (initial_trimmed_mean_kib, initial_trimmed_median_kib, initial_trimmed_mad_kib) =
        rss_descriptive_statistics(&initial_retained);
    let (optimized_trimmed_mean_kib, optimized_trimmed_median_kib, optimized_trimmed_mad_kib) =
        rss_descriptive_statistics(&optimized_retained);
    let baseline_candidate_mean_ratio = initial_trimmed_mean_kib / optimized_trimmed_mean_kib;
    let relative_reduction = 1.0 - optimized_trimmed_mean_kib / initial_trimmed_mean_kib;
    let (relative_reduction_ci95_low, relative_reduction_ci95_high) =
        independent_bootstrap_relative_reduction(
            &initial_retained,
            &optimized_retained,
            RSS_BOOTSTRAP_RESAMPLES,
        );
    let ci_absolute_total_width = relative_reduction_ci95_high - relative_reduction_ci95_low;

    Ok(RssSummary {
        initial_raw_samples: initial.len(),
        initial_trim_each_tail: initial_selection.trim_each_tail,
        initial_retained_samples: initial_retained.len(),
        optimized_raw_samples: optimized.len(),
        optimized_trim_each_tail: optimized_selection.trim_each_tail,
        optimized_retained_samples: optimized_retained.len(),
        initial_trimmed_mean_kib,
        initial_trimmed_median_kib,
        initial_trimmed_mad_kib,
        optimized_trimmed_mean_kib,
        optimized_trimmed_median_kib,
        optimized_trimmed_mad_kib,
        baseline_candidate_mean_ratio,
        relative_reduction,
        relative_reduction_ci95_low,
        relative_reduction_ci95_high,
        ci_absolute_total_width,
        adequacy_criterion_met: ci_absolute_total_width <= RSS_MAX_ABSOLUTE_CI_TOTAL_WIDTH,
    })
}

fn validate_rss_summary_input(samples: &[RssSample], implementation: &str) -> io::Result<()> {
    if samples.len() < RSS_MIN_RAW_SAMPLES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "{implementation} RSS summary requires at least {RSS_MIN_RAW_SAMPLES} samples; found {}",
                samples.len()
            ),
        ));
    }
    if samples.iter().any(|sample| sample.value_kib == 0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{implementation} RSS summary requires positive KiB values"),
        ));
    }
    Ok(())
}

fn retained_rss_values(samples: &[RssSample], selection: &RssSelection) -> io::Result<Vec<u64>> {
    if samples.len() != selection.dispositions.len()
        || selection.retained_indices.is_empty()
        || selection
            .retained_indices
            .iter()
            .any(|&index| index >= samples.len())
    {
        return Err(io::Error::other("RSS selection does not match samples"));
    }

    Ok(selection
        .retained_indices
        .iter()
        .map(|&index| samples[index].value_kib)
        .collect())
}

fn rss_descriptive_statistics(values: &[u64]) -> (f64, f64, f64) {
    let mean = arithmetic_mean_u64(values);
    let median = median_u64(values);
    let deviations: Vec<_> = values
        .iter()
        .map(|&value| (value as f64 - median).abs())
        .collect();
    (mean, median, median_f64(&deviations))
}

fn arithmetic_mean_u64(values: &[u64]) -> f64 {
    values.iter().map(|&value| value as f64).sum::<f64>() / values.len() as f64
}

fn median_u64(values: &[u64]) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let middle = sorted.len() / 2;
    if sorted.len().is_multiple_of(2) {
        (sorted[middle - 1] as f64 + sorted[middle] as f64) / 2.0
    } else {
        sorted[middle] as f64
    }
}

fn independent_bootstrap_relative_reduction(
    initial_retained: &[u64],
    optimized_retained: &[u64],
    resamples: usize,
) -> (f64, f64) {
    assert!(
        !initial_retained.is_empty() && !optimized_retained.is_empty(),
        "bootstrap requires both retained RSS sets"
    );
    assert!(resamples > 0, "bootstrap requires resamples");

    let mut rng = DeterministicRng::new(RSS_BOOTSTRAP_SEED);
    let mut estimates = Vec::with_capacity(resamples);
    for _ in 0..resamples {
        let initial_mean = (0..initial_retained.len())
            .map(|_| initial_retained[rng.index(initial_retained.len())] as f64)
            .sum::<f64>()
            / initial_retained.len() as f64;
        let optimized_mean = (0..optimized_retained.len())
            .map(|_| optimized_retained[rng.index(optimized_retained.len())] as f64)
            .sum::<f64>()
            / optimized_retained.len() as f64;
        estimates.push(1.0 - optimized_mean / initial_mean);
    }

    estimates.sort_by(f64::total_cmp);
    let tail_probability = (1.0 - RSS_CONFIDENCE_LEVEL) / 2.0;
    (
        percentile(&estimates, tail_probability),
        percentile(&estimates, 1.0 - tail_probability),
    )
}

fn rss_verdict(summary: RssSummary) -> &'static str {
    if !summary.adequacy_criterion_met {
        "inconclusive"
    } else if summary.relative_reduction_ci95_low >= RSS_PRACTICAL_REDUCTION {
        "improved"
    } else if summary.relative_reduction_ci95_high <= -RSS_PRACTICAL_REDUCTION {
        "regressed"
    } else if summary.relative_reduction_ci95_low >= -RSS_EQUIVALENCE_MARGIN
        && summary.relative_reduction_ci95_high <= RSS_EQUIVALENCE_MARGIN
    {
        "practically_equivalent"
    } else {
        "inconclusive"
    }
}

fn pair_log_effect(pair: &SamplePair) -> io::Result<f64> {
    if pair.initial.is_zero() || pair.optimized.is_zero() {
        return Err(io::Error::other(
            "paired selection requires non-zero durations",
        ));
    }

    Ok((pair.initial.as_nanos() as f64 / pair.optimized.as_nanos() as f64).ln())
}

fn select_paired_samples(pairs: &[SamplePair]) -> io::Result<PairedSelection> {
    if pairs.is_empty() {
        return Err(io::Error::other("cannot select an empty sample set"));
    }

    let trim_each_tail = trim_count(pairs.len());
    let retained_count = pairs.len() - 2 * trim_each_tail;
    if retained_count == 0 {
        return Err(io::Error::other("paired trimming retained no samples"));
    }

    let mut ranked = pairs
        .iter()
        .enumerate()
        .map(|(index, pair)| Ok((index, pair.pair, pair_log_effect(pair)?)))
        .collect::<io::Result<Vec<_>>>()?;
    ranked.sort_by(|left, right| {
        left.2
            .total_cmp(&right.2)
            .then_with(|| left.1.cmp(&right.1))
            .then_with(|| left.0.cmp(&right.0))
    });

    let mut dispositions = vec![PairDisposition::Retained; pairs.len()];
    for &(index, _, _) in &ranked[..trim_each_tail] {
        dispositions[index] = PairDisposition::Low;
    }
    for &(index, _, _) in &ranked[pairs.len() - trim_each_tail..] {
        dispositions[index] = PairDisposition::High;
    }

    let retained_indices = dispositions
        .iter()
        .enumerate()
        .filter_map(|(index, disposition)| {
            (*disposition == PairDisposition::Retained).then_some(index)
        })
        .collect();

    Ok(PairedSelection {
        trim_each_tail,
        dispositions,
        retained_indices,
    })
}

fn arithmetic_mean_u128(values: &[u128]) -> f64 {
    values.iter().map(|&value| value as f64).sum::<f64>() / values.len() as f64
}

fn summarize_runtime(
    pairs: &[SamplePair],
    selection: &PairedSelection,
) -> io::Result<RuntimeSummary> {
    if pairs.len() != selection.dispositions.len()
        || selection.retained_indices.is_empty()
        || selection
            .retained_indices
            .iter()
            .any(|&index| index >= pairs.len())
    {
        return Err(io::Error::other("paired selection does not match samples"));
    }

    let retained = selection
        .retained_indices
        .iter()
        .map(|&index| &pairs[index]);
    let initial: Vec<u128> = retained
        .clone()
        .map(|pair| pair.initial.as_nanos())
        .collect();
    let optimized: Vec<u128> = retained
        .clone()
        .map(|pair| pair.optimized.as_nanos())
        .collect();
    let log_effects = retained
        .map(pair_log_effect)
        .collect::<io::Result<Vec<_>>>()?;

    let initial_trimmed_mean_ns = arithmetic_mean_u128(&initial);
    let optimized_trimmed_mean_ns = arithmetic_mean_u128(&optimized);
    let paired_median_speedup = paired_median_speedup(&log_effects);
    let (ci95_low, ci95_high) = paired_bootstrap_ci(&log_effects, BOOTSTRAP_RESAMPLES);
    let ci_relative_total_width = (ci95_high - ci95_low) / paired_median_speedup;

    Ok(RuntimeSummary {
        raw_pairs: pairs.len(),
        trim_each_tail: selection.trim_each_tail,
        retained_pairs: selection.retained_indices.len(),
        initial_trimmed_mean_ns,
        initial_trimmed_median_ns: median_u128(&initial),
        initial_trimmed_mad_ns: median_absolute_deviation(&initial),
        optimized_trimmed_mean_ns,
        optimized_trimmed_median_ns: median_u128(&optimized),
        optimized_trimmed_mad_ns: median_absolute_deviation(&optimized),
        ratio_of_trimmed_means: initial_trimmed_mean_ns / optimized_trimmed_mean_ns,
        paired_median_speedup,
        ci95_low,
        ci95_high,
        ci_relative_total_width,
        adequacy_criterion_met: ci_relative_total_width <= MAX_RELATIVE_CI_TOTAL_WIDTH,
    })
}

fn performance_verdict(summary: RuntimeSummary) -> &'static str {
    if !summary.adequacy_criterion_met {
        "inconclusive_ci_width_exceeds_adequacy_criterion"
    } else if summary.ci95_low >= PRACTICAL_SPEEDUP {
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

fn paired_median_speedup(log_effects: &[f64]) -> f64 {
    median_f64(log_effects).exp()
}

fn paired_bootstrap_ci(log_effects: &[f64], resamples: usize) -> (f64, f64) {
    assert!(!log_effects.is_empty(), "bootstrap requires paired effects");
    assert!(resamples > 0, "bootstrap requires resamples");

    let mut rng = DeterministicRng::new(BOOTSTRAP_SEED);
    let mut sample = Vec::with_capacity(log_effects.len());
    let mut estimates = Vec::with_capacity(resamples);

    for _ in 0..resamples {
        sample.clear();
        for _ in 0..log_effects.len() {
            sample.push(log_effects[rng.index(log_effects.len())]);
        }
        estimates.push(paired_median_speedup(&sample));
    }

    estimates.sort_by(f64::total_cmp);
    let tail_probability = (1.0 - CONFIDENCE_LEVEL) / 2.0;
    (
        percentile(&estimates, tail_probability),
        percentile(&estimates, 1.0 - tail_probability),
    )
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
    use std::io::{Cursor, Read};
    use std::time::Duration;

    use super::{
        BOOTSTRAP_RESAMPLES, BOUNDARY_PERIOD, Counts, DATASETS, DEFAULT_SAMPLES, DEFAULT_SIZE_MIB,
        DatasetKind, DeterministicInput, MIN_MEASURED_PAIRS, PairDisposition, PairedSelection,
        READER_CAPACITY, RSS_BOOTSTRAP_RESAMPLES, RSS_EQUIVALENCE_MARGIN,
        RSS_MAX_ABSOLUTE_CI_TOTAL_WIDTH, RSS_PRACTICAL_REDUCTION, RssDisposition, RssSample,
        SamplePair, Variant, WARM_UP_PAIRS, analyze_bytes, build_dataset,
        independent_bootstrap_relative_reduction, median_absolute_deviation, median_u128,
        paired_bootstrap_ci, parse_rss_samples, rss_verdict, run_runtime, run_slice_untimed,
        sample_order, select_paired_samples, select_rss_samples, size_in_bytes, summarize_rss,
        summarize_runtime, trim_count, write_rss_summary_records, write_sample_record,
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

    fn sample_pair(pair: usize, initial_ns: u64, optimized_ns: u64) -> SamplePair {
        SamplePair {
            pair,
            order: if pair.is_multiple_of(2) {
                "initial-optimized"
            } else {
                "optimized-initial"
            },
            initial: Duration::from_nanos(initial_ns),
            optimized: Duration::from_nanos(optimized_ns),
        }
    }

    fn monotonic_pairs(count: usize) -> Vec<SamplePair> {
        (0..count)
            .map(|pair| sample_pair(pair, (pair as u64 + 1) * 100, 100))
            .collect()
    }

    fn pair_ids_with_disposition(
        pairs: &[SamplePair],
        selection: &PairedSelection,
        disposition: PairDisposition,
    ) -> Vec<usize> {
        pairs
            .iter()
            .zip(&selection.dispositions)
            .filter_map(|(pair, &actual)| (actual == disposition).then_some(pair.pair))
            .collect()
    }

    fn rss_samples(values: &[u64]) -> Vec<RssSample> {
        values
            .iter()
            .enumerate()
            .map(|(acquisition_index, &value_kib)| RssSample {
                sample_id: acquisition_index + 1,
                acquisition_index,
                value_kib,
            })
            .collect()
    }

    fn rss_samples_with_center(center: u64) -> Vec<RssSample> {
        let mut values = vec![1; 4];
        values.extend(std::iter::repeat_n(center, 23));
        values.extend(std::iter::repeat_n(center * 100, 4));
        rss_samples(&values)
    }

    fn rss_samples_with_retained(retained: &[u64]) -> Vec<RssSample> {
        assert_eq!(retained.len(), 23);
        let mut values = vec![1; 4];
        values.extend_from_slice(retained);
        values.extend(std::iter::repeat_n(10_000, 4));
        rss_samples(&values)
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
    fn trim_count_uses_exact_floor_without_overflow() {
        assert_eq!(trim_count(31), 4);
        assert_eq!(trim_count(101), 15);
        assert!(trim_count(usize::MAX) <= usize::MAX / 2);
    }

    #[test]
    fn defaults_enforce_large_dataset_contract_without_allocating_it() {
        assert_eq!(DEFAULT_SAMPLES, 101);
        assert_eq!(MIN_MEASURED_PAIRS, 101);
        assert!(std::hint::black_box(WARM_UP_PAIRS) >= 3);
        assert_eq!(DEFAULT_SIZE_MIB, 256);
        assert_eq!(size_in_bytes(DEFAULT_SIZE_MIB).unwrap(), 268_435_456);

        let args = ["runtime", "1", "100"].map(String::from);
        let error = run_runtime(&args).unwrap_err();
        assert_eq!(
            error.to_string(),
            "samples must be at least 101 measured pairs"
        );
    }

    #[test]
    fn rss_independent_selection_trims_n31_as_4_23_4_with_id_ties() {
        let samples: Vec<_> = (1..=31)
            .rev()
            .enumerate()
            .map(|(acquisition_index, sample_id)| RssSample {
                sample_id,
                acquisition_index,
                value_kib: 100,
            })
            .collect();
        let selection = select_rss_samples(&samples).unwrap();
        let ids = |disposition| {
            samples
                .iter()
                .zip(&selection.dispositions)
                .filter_map(|(sample, &actual)| (actual == disposition).then_some(sample.sample_id))
                .collect::<Vec<_>>()
        };

        assert_eq!(selection.trim_each_tail, 4);
        assert_eq!(selection.retained_indices.len(), 23);
        assert_eq!(ids(RssDisposition::Low), vec![4, 3, 2, 1]);
        assert_eq!(ids(RssDisposition::Retained).len(), 23);
        assert_eq!(ids(RssDisposition::High), vec![31, 30, 29, 28]);
    }

    #[test]
    fn rss_retained_only_bootstrap_classifies_known_constant_groups() {
        let equal = rss_samples(&[100; 31]);
        let equivalent = summarize_rss(&equal, &equal).unwrap();

        assert_eq!(equivalent.relative_reduction, 0.0);
        assert_eq!(
            (
                equivalent.relative_reduction_ci95_low,
                equivalent.relative_reduction_ci95_high,
            ),
            (0.0, 0.0)
        );
        assert_eq!(equivalent.ci_absolute_total_width, 0.0);
        assert!(equivalent.adequacy_criterion_met);
        assert_eq!(rss_verdict(equivalent), "practically_equivalent");

        let initial = rss_samples_with_center(200);
        let optimized = rss_samples_with_center(100);
        let improved = summarize_rss(&initial, &optimized).unwrap();

        assert_eq!(improved.initial_raw_samples, 31);
        assert_eq!(improved.initial_trim_each_tail, 4);
        assert_eq!(improved.initial_retained_samples, 23);
        assert_eq!(improved.optimized_raw_samples, 31);
        assert_eq!(improved.optimized_trim_each_tail, 4);
        assert_eq!(improved.optimized_retained_samples, 23);
        assert_eq!(improved.initial_trimmed_mean_kib, 200.0);
        assert_eq!(improved.optimized_trimmed_mean_kib, 100.0);
        assert_eq!(improved.baseline_candidate_mean_ratio, 2.0);
        assert_eq!(improved.relative_reduction, 0.5);
        assert_eq!(
            (
                improved.relative_reduction_ci95_low,
                improved.relative_reduction_ci95_high,
            ),
            (0.5, 0.5)
        );
        assert_eq!(improved.ci_absolute_total_width, 0.0);
        assert!(improved.adequacy_criterion_met);
        assert_eq!(rss_verdict(improved), "improved");

        let mut output = Vec::new();
        write_rss_summary_records(&mut output, improved).unwrap();
        let jsonl = String::from_utf8(output).unwrap();
        assert_eq!(jsonl.lines().count(), 2);
        assert!(
            jsonl
                .lines()
                .next()
                .unwrap()
                .contains("\"ci_method\":\"deterministic_independent_percentile_bootstrap\"")
        );
        assert!(jsonl.lines().last().unwrap().contains(
            "\"relative_reduction\":0.500000000,\"relative_reduction_ci95_low\":0.500000000"
        ));
    }

    #[test]
    fn rss_parser_rejects_empty_short_zero_and_non_integer_input() {
        let error = parse_rss_samples(Cursor::new("\n"), "initial").unwrap_err();
        assert_eq!(error.to_string(), "initial values file contains no samples");

        let error = parse_rss_samples(Cursor::new("1\n2\n"), "optimized").unwrap_err();
        assert_eq!(
            error.to_string(),
            "optimized values file requires at least 31 samples; found 2"
        );

        let error = parse_rss_samples(Cursor::new("1\n0\n"), "initial").unwrap_err();
        assert_eq!(
            error.to_string(),
            "initial values file line 2 must be a positive integer KiB value"
        );

        let error = parse_rss_samples(Cursor::new("1\nNaN\n"), "initial").unwrap_err();
        assert_eq!(
            error.to_string(),
            "initial values file line 2 must be a positive integer KiB value"
        );
    }

    #[test]
    fn rss_variable_retained_bootstrap_ci_is_nonzero_and_repeatable() {
        let initial: Vec<_> = (180..=202).collect();
        let optimized: Vec<_> = (140..=162).collect();

        let first =
            independent_bootstrap_relative_reduction(&initial, &optimized, RSS_BOOTSTRAP_RESAMPLES);
        let second =
            independent_bootstrap_relative_reduction(&initial, &optimized, RSS_BOOTSTRAP_RESAMPLES);

        assert_eq!(first, second);
        assert!(first.0.is_finite() && first.1.is_finite());
        assert_ne!(first.0, 0.0);
        assert_ne!(first.1, 0.0);
        assert!(first.0 < first.1);
    }

    #[test]
    fn rss_adequate_negative_effect_is_regressed() {
        let initial = rss_samples_with_center(100);
        let optimized = rss_samples_with_center(200);
        let summary = summarize_rss(&initial, &optimized).unwrap();

        assert!(summary.adequacy_criterion_met);
        assert!(summary.relative_reduction_ci95_high <= -RSS_PRACTICAL_REDUCTION);
        assert_eq!(rss_verdict(summary), "regressed");
    }

    #[test]
    fn rss_inadequate_width_is_inconclusive_before_effect_classification() {
        let initial = rss_samples_with_retained(&[100; 23]);
        let optimized_retained: Vec<_> = (150..=172).collect();
        let optimized = rss_samples_with_retained(&optimized_retained);
        let summary = summarize_rss(&initial, &optimized).unwrap();

        assert!(summary.relative_reduction_ci95_high <= -RSS_PRACTICAL_REDUCTION);
        assert!(summary.ci_absolute_total_width > RSS_MAX_ABSOLUTE_CI_TOTAL_WIDTH);
        assert!(!summary.adequacy_criterion_met);
        assert_eq!(rss_verdict(summary), "inconclusive");
    }

    #[test]
    fn rss_effect_between_equivalence_and_practical_thresholds_is_inconclusive() {
        let initial = rss_samples_with_center(1_000);
        let optimized = rss_samples_with_center(970);
        let summary = summarize_rss(&initial, &optimized).unwrap();

        assert!(summary.adequacy_criterion_met);
        assert!(summary.relative_reduction_ci95_low > RSS_EQUIVALENCE_MARGIN);
        assert!(summary.relative_reduction_ci95_high < RSS_PRACTICAL_REDUCTION);
        assert_eq!(rss_verdict(summary), "inconclusive");
    }

    #[test]
    fn paired_selection_trims_whole_pairs_and_marks_101_as_15_71_15() {
        let pairs = monotonic_pairs(101);
        let selection = select_paired_samples(&pairs).unwrap();

        assert_eq!(selection.trim_each_tail, 15);
        assert_eq!(selection.retained_indices, (15..=85).collect::<Vec<_>>());
        assert_eq!(
            pair_ids_with_disposition(&pairs, &selection, PairDisposition::Low),
            (0..15).collect::<Vec<_>>()
        );
        assert_eq!(
            pair_ids_with_disposition(&pairs, &selection, PairDisposition::Retained),
            (15..=85).collect::<Vec<_>>()
        );
        assert_eq!(
            pair_ids_with_disposition(&pairs, &selection, PairDisposition::High),
            (86..101).collect::<Vec<_>>()
        );
    }

    #[test]
    fn equal_effects_use_pair_id_as_deterministic_tie_break() {
        let pairs: Vec<_> = (0..31)
            .rev()
            .map(|pair| sample_pair(pair, 200, 100))
            .collect();
        let selection = select_paired_samples(&pairs).unwrap();
        let mut low = pair_ids_with_disposition(&pairs, &selection, PairDisposition::Low);
        let mut high = pair_ids_with_disposition(&pairs, &selection, PairDisposition::High);
        low.sort_unstable();
        high.sort_unstable();

        assert_eq!(low, vec![0, 1, 2, 3]);
        assert_eq!(high, vec![27, 28, 29, 30]);
    }

    #[test]
    fn retained_pairs_feed_all_statistics_and_bootstrap_without_retrim() {
        let pairs: Vec<_> = (0..31)
            .map(|pair| match pair {
                0..=3 => sample_pair(pair, pair as u64 + 1, 100),
                4..=26 => sample_pair(pair, 100 + (pair - 4) as u64, 100),
                _ => sample_pair(pair, 10_000 + pair as u64, 100),
            })
            .collect();
        let selection = select_paired_samples(&pairs).unwrap();
        let summary = summarize_runtime(&pairs, &selection).unwrap();
        let retained_log_effects: Vec<_> = (100..=122)
            .map(|initial| (f64::from(initial) / 100.0).ln())
            .collect();
        let expected_ci = paired_bootstrap_ci(&retained_log_effects, BOOTSTRAP_RESAMPLES);

        assert_eq!(selection.retained_indices, (4..=26).collect::<Vec<_>>());
        assert_eq!(summary.raw_pairs, 31);
        assert_eq!(summary.trim_each_tail, 4);
        assert_eq!(summary.retained_pairs, 23);
        assert_eq!(summary.initial_trimmed_mean_ns, 111.0);
        assert_eq!(summary.initial_trimmed_median_ns, 111);
        assert_eq!(summary.initial_trimmed_mad_ns, 6);
        assert_eq!(summary.optimized_trimmed_mean_ns, 100.0);
        assert_eq!(summary.optimized_trimmed_median_ns, 100);
        assert_eq!(summary.optimized_trimmed_mad_ns, 0);
        assert_eq!(summary.ratio_of_trimmed_means, 1.11);
        assert!((summary.paired_median_speedup - 1.11).abs() < 1e-12);
        assert_eq!((summary.ci95_low, summary.ci95_high), expected_ci);
    }

    #[test]
    fn changing_trimmed_extremes_does_not_change_retained_results() {
        let pairs: Vec<_> = (0..31)
            .map(|pair| match pair {
                0..=3 => sample_pair(pair, pair as u64 + 1, 100),
                4..=26 => sample_pair(pair, 100 + (pair - 4) as u64, 100),
                _ => sample_pair(pair, 10_000 + pair as u64, 100),
            })
            .collect();
        let selection = select_paired_samples(&pairs).unwrap();
        let expected = summarize_runtime(&pairs, &selection).unwrap();

        let mut changed = pairs.clone();
        for pair in &mut changed[..4] {
            pair.initial = Duration::from_nanos(90);
        }
        for pair in &mut changed[27..] {
            pair.initial = Duration::from_nanos(200);
        }
        let changed_selection = select_paired_samples(&changed).unwrap();
        let actual = summarize_runtime(&changed, &changed_selection).unwrap();

        assert_eq!(
            selection.retained_indices,
            changed_selection.retained_indices
        );
        assert_eq!(actual, expected);
    }

    #[test]
    fn raw_jsonl_preserves_pair_order_and_selection_markers() {
        let pairs = monotonic_pairs(101);
        let selection = select_paired_samples(&pairs).unwrap();
        let mut output = Vec::new();

        for (pair, disposition) in pairs.iter().zip(&selection.dispositions) {
            write_sample_record(&mut output, DatasetKind::Prose, pair, *disposition).unwrap();
        }

        let jsonl = String::from_utf8(output).unwrap();
        assert_eq!(jsonl.lines().count(), 101);
        assert_eq!(jsonl.matches("\"selection\":\"low\"").count(), 15);
        assert_eq!(jsonl.matches("\"selection\":\"retained\"").count(), 71);
        assert_eq!(jsonl.matches("\"selection\":\"high\"").count(), 15);
        assert!(
            jsonl
                .lines()
                .next()
                .unwrap()
                .contains("\"pair\":0,\"order\":\"initial-optimized\"")
        );
        assert!(
            jsonl
                .lines()
                .last()
                .unwrap()
                .contains("\"pair\":100,\"order\":\"initial-optimized\"")
        );
    }

    #[test]
    fn median_mad_and_retained_bootstrap_are_deterministic() {
        let values = [10, 20, 30, 40, 100];
        assert_eq!(median_u128(&values), 30);
        assert_eq!(median_absolute_deviation(&values), 10);

        let log_effects = [1.10_f64, 1.20, 1.30, 1.40, 1.50].map(f64::ln);
        let first = paired_bootstrap_ci(&log_effects, BOOTSTRAP_RESAMPLES);
        let second = paired_bootstrap_ci(&log_effects, BOOTSTRAP_RESAMPLES);
        assert_eq!(first, second);
        assert!(first.0 <= 1.30);
        assert!(first.1 >= 1.30);
    }
}
