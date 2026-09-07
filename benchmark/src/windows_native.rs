//! Native Windows x86-64 GNU support for the optional BA02 benchmark.
//!
//! Win32 interaction stays inside this module. `extern "system"` matches the
//! Windows x64 ABI. Process and thread pseudo-handles are used only during the
//! call that receives them. Affinity changes are scoped by a guard and restored
//! on the same thread. Peak working set is process-wide, not heap allocation.

use std::arch::x86_64::__cpuid;
use std::env;
use std::error::Error;
use std::ffi::c_void;
use std::io::{self, BufRead, BufReader, BufWriter, Read, Write};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, ExitStatus, Stdio};
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::ba02_large::{
    DEFAULT_MEMORY_SAMPLES, DEFAULT_SAMPLES, DEFAULT_SIZE_MIB, MIN_MEASURED_PAIRS, WARM_UP_PAIRS,
    WindowsMemorySample,
};

const PROTOCOL_VERSION: u32 = 1;
const BUILD_FLAVOR: &str = env!("HW1_BENCH_BUILD_FLAVOR");
const BUILD_TARGET: &str = env!("HW1_BENCH_TARGET");
const BUILD_TARGET_CPU: &str = env!("HW1_BENCH_TARGET_CPU");
const WORKER_TIMEOUT: Duration = Duration::from_secs(120);
const PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(10);
const MEMORY_WARM_UPS_PER_VARIANT: usize = 1;
const TARGET_PROCESSOR_NAME: &str = "AMD Ryzen 9 9950X3D 16-Core Processor";
const TARGET_PROCESSOR_VENDOR: &str = "AuthenticAMD";
const TARGET_PROCESSOR_FAMILY: u32 = 0x1a;
const TARGET_PROCESSOR_MODEL: u32 = 0x44;
const ZNVER5_REQUIRED_FEATURES: &[&str] = &[
    "adx",
    "aes",
    "avx",
    "avx2",
    "avx512bf16",
    "avx512bitalg",
    "avx512bw",
    "avx512cd",
    "avx512dq",
    "avx512f",
    "avx512ifma",
    "avx512vbmi",
    "avx512vbmi2",
    "avx512vl",
    "avx512vnni",
    "avx512vp2intersect",
    "avx512vpopcntdq",
    "avxvnni",
    "bmi1",
    "bmi2",
    "cmpxchg16b",
    "f16c",
    "fma",
    "fxsr",
    "gfni",
    "lzcnt",
    "movbe",
    "pclmulqdq",
    "popcnt",
    "rdrand",
    "rdseed",
    "sha",
    "sse",
    "sse2",
    "sse3",
    "sse4.1",
    "sse4.2",
    "sse4a",
    "ssse3",
    "vaes",
    "vpclmulqdq",
    "xsave",
    "xsavec",
    "xsaveopt",
    "xsaves",
];
const MIB: usize = 1024 * 1024;

type DynResult<T> = Result<T, Box<dyn Error>>;
type Handle = *mut c_void;

#[repr(C)]
#[allow(non_snake_case)]
#[derive(Default)]
struct ProcessMemoryCounters {
    cb: u32,
    PageFaultCount: u32,
    PeakWorkingSetSize: usize,
    WorkingSetSize: usize,
    QuotaPeakPagedPoolUsage: usize,
    QuotaPagedPoolUsage: usize,
    QuotaPeakNonPagedPoolUsage: usize,
    QuotaNonPagedPoolUsage: usize,
    PagefileUsage: usize,
    PeakPagefileUsage: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ProcessorIdentity {
    name: String,
    vendor: String,
    family: u32,
    model: u32,
    stepping: u32,
}

const _: () = {
    assert!(std::mem::size_of::<ProcessMemoryCounters>() == 72);
    assert!(std::mem::align_of::<ProcessMemoryCounters>() == 8);
    assert!(std::mem::offset_of!(ProcessMemoryCounters, cb) == 0);
    assert!(std::mem::offset_of!(ProcessMemoryCounters, PageFaultCount) == 4);
    assert!(std::mem::offset_of!(ProcessMemoryCounters, PeakWorkingSetSize) == 8);
    assert!(std::mem::offset_of!(ProcessMemoryCounters, WorkingSetSize) == 16);
    assert!(std::mem::offset_of!(ProcessMemoryCounters, QuotaPeakPagedPoolUsage) == 24);
    assert!(std::mem::offset_of!(ProcessMemoryCounters, QuotaPagedPoolUsage) == 32);
    assert!(std::mem::offset_of!(ProcessMemoryCounters, QuotaPeakNonPagedPoolUsage) == 40);
    assert!(std::mem::offset_of!(ProcessMemoryCounters, QuotaNonPagedPoolUsage) == 48);
    assert!(std::mem::offset_of!(ProcessMemoryCounters, PagefileUsage) == 56);
    assert!(std::mem::offset_of!(ProcessMemoryCounters, PeakPagefileUsage) == 64);
};

#[link(name = "kernel32")]
#[allow(non_snake_case)]
unsafe extern "system" {
    fn GetCurrentProcess() -> Handle;
    fn GetCurrentThread() -> Handle;
    fn GetProcessAffinityMask(
        process: Handle,
        process_affinity_mask: *mut usize,
        system_affinity_mask: *mut usize,
    ) -> i32;
    fn SetThreadAffinityMask(thread: Handle, thread_affinity_mask: usize) -> usize;
}

#[link(name = "psapi")]
#[allow(non_snake_case)]
unsafe extern "system" {
    fn GetProcessMemoryInfo(
        process: Handle,
        counters: *mut ProcessMemoryCounters,
        size: u32,
    ) -> i32;
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct WorkerReady {
    protocol_version: u32,
    build_flavor: String,
    build_target: String,
    build_target_cpu: String,
    variant: String,
    dataset: String,
    size_bytes: usize,
    fingerprint: u64,
    checksum: u64,
}

impl WorkerReady {
    fn parse(line: &str) -> io::Result<Self> {
        let fields = line.trim_end().split('\t').collect::<Vec<_>>();
        if fields.len() != 10 || fields[0] != "READY" {
            return Err(io::Error::other(format!(
                "invalid worker readiness record {line:?}"
            )));
        }

        Ok(Self {
            protocol_version: parse_protocol_field(fields[1], "protocol version")?,
            build_flavor: fields[2].to_owned(),
            build_target: fields[3].to_owned(),
            build_target_cpu: fields[4].to_owned(),
            variant: fields[5].to_owned(),
            dataset: fields[6].to_owned(),
            size_bytes: parse_protocol_field(fields[7], "size_bytes")?,
            fingerprint: parse_hex_protocol_field(fields[8], "fingerprint")?,
            checksum: parse_hex_protocol_field(fields[9], "checksum")?,
        })
    }

    fn same_workload(&self, other: &Self) -> bool {
        self.protocol_version == other.protocol_version
            && self.build_target == other.build_target
            && self.variant == other.variant
            && self.dataset == other.dataset
            && self.size_bytes == other.size_bytes
            && self.fingerprint == other.fingerprint
            && self.checksum == other.checksum
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct MemoryObservation {
    protocol_version: u32,
    build_flavor: String,
    build_target: String,
    build_target_cpu: String,
    process_id: u32,
    logical_cpu: usize,
    variant: String,
    dataset: String,
    size_mib: usize,
    size_bytes: usize,
    fingerprint: u64,
    checksum: u64,
    peak_working_set_kib: u64,
}

impl MemoryObservation {
    fn parse(line: &str) -> io::Result<Self> {
        let fields = line.trim_end().split('\t').collect::<Vec<_>>();
        if fields.len() != 14 || fields[0] != "MEMORY" {
            return Err(io::Error::other(format!(
                "invalid memory worker record {line:?}"
            )));
        }

        Ok(Self {
            protocol_version: parse_protocol_field(fields[1], "protocol version")?,
            build_flavor: fields[2].to_owned(),
            build_target: fields[3].to_owned(),
            build_target_cpu: fields[4].to_owned(),
            process_id: parse_protocol_field(fields[5], "process_id")?,
            logical_cpu: parse_protocol_field(fields[6], "logical_cpu")?,
            variant: fields[7].to_owned(),
            dataset: fields[8].to_owned(),
            size_mib: parse_protocol_field(fields[9], "size_mib")?,
            size_bytes: parse_protocol_field(fields[10], "size_bytes")?,
            fingerprint: parse_hex_protocol_field(fields[11], "fingerprint")?,
            checksum: parse_hex_protocol_field(fields[12], "checksum")?,
            peak_working_set_kib: parse_protocol_field(fields[13], "peak_working_set_kib")?,
        })
    }

    fn workload_matches(&self, other: &Self) -> bool {
        self.protocol_version == other.protocol_version
            && self.build_flavor == other.build_flavor
            && self.build_target == other.build_target
            && self.build_target_cpu == other.build_target_cpu
            && self.logical_cpu == other.logical_cpu
            && self.dataset == other.dataset
            && self.size_mib == other.size_mib
            && self.size_bytes == other.size_bytes
            && self.fingerprint == other.fingerprint
            && self.checksum == other.checksum
    }
}

pub fn is_command(command: Option<&str>) -> bool {
    matches!(
        command,
        Some(
            "windows-info"
                | "windows-runtime"
                | "windows-once"
                | "windows-memory-run"
                | "windows-memory-summary"
                | "windows-memory-worker"
                | "windows-worker"
                | "windows-build-runtime"
        )
    )
}

pub fn run(args: &[String]) -> DynResult<()> {
    if BUILD_FLAVOR == "znver5" {
        verify_znver5_host()?;
    }

    match args.first().map(String::as_str) {
        Some("windows-info") => run_info(args),
        Some("windows-runtime") => run_runtime(args),
        Some("windows-once") => run_once(args),
        Some("windows-memory-run") => run_memory(args),
        Some("windows-memory-summary") => crate::ba02_large::run_peak_working_set_summary(args),
        Some("windows-memory-worker") => run_memory_worker(args),
        Some("windows-worker") => run_worker(args),
        Some("windows-build-runtime") => run_build_runtime(args),
        _ => Err(invalid_input(
            "usage: windows-info | windows-runtime <logical_cpu> [size_mib] [samples] | \
             windows-once <logical_cpu> <initial|optimized> <dataset> [size_mib] | \
             windows-memory-run <logical_cpu> <dataset> [size_mib] [samples] | \
             windows-memory-summary <initial_values_file> <optimized_values_file> | \
             windows-build-runtime <logical_cpu> <znver5_exe> <initial|optimized> \
             <dataset> [size_mib] [samples]",
        )),
    }
}

fn run_info(args: &[String]) -> DynResult<()> {
    if args.len() != 1 {
        return Err(invalid_input("windows-info accepts no arguments"));
    }

    let processor = processor_identity()?;
    let (process_mask, system_mask) = affinity_masks()?;

    println!("execution_environment=native_windows_process");
    println!("os={}", env::consts::OS);
    println!("arch={}", env::consts::ARCH);
    println!("build_flavor={BUILD_FLAVOR}");
    println!("build_target={BUILD_TARGET}");
    println!("build_target_cpu={BUILD_TARGET_CPU}");
    println!("processor_name={}", processor.name);
    println!(
        "processor_identifier=Family {} Model {} Stepping {}, {}",
        processor.family, processor.model, processor.stepping, processor.vendor
    );
    println!("logical_cpus_in_process_mask={}", process_mask.count_ones());
    println!("process_affinity_mask=0x{process_mask:016x}");
    println!("system_affinity_mask=0x{system_mask:016x}");
    println!("processor_group_scope=current_group_only");
    println!("detected_x86_features={}", detected_x86_features());

    Ok(())
}

fn run_runtime(args: &[String]) -> DynResult<()> {
    if !(2..=4).contains(&args.len()) {
        return Err(invalid_input(
            "windows-runtime requires <logical_cpu> [size_mib] [samples]",
        ));
    }
    require_known_build_identity()?;

    let logical_cpu = parse_logical_cpu(&args[1])?;
    let _affinity = pin_current_thread(logical_cpu)?;
    write_execution_record(logical_cpu)?;

    let mut delegated = vec!["runtime".to_owned()];
    delegated.extend(args[2..].iter().cloned());
    crate::ba02_large::run(&delegated)
}

fn run_once(args: &[String]) -> DynResult<()> {
    if !(4..=5).contains(&args.len()) {
        return Err(invalid_input(
            "windows-once requires <logical_cpu> <initial|optimized> \
             <prose|boundary|mixed> [size_mib]",
        ));
    }
    require_known_build_identity()?;

    let logical_cpu = parse_logical_cpu(&args[1])?;
    let _affinity = pin_current_thread(logical_cpu)?;
    let measurement = crate::ba02_large::measure_once(&args[2..])?;
    let peak_kib = peak_working_set_kib()?;

    let stdout = io::stdout();
    let mut output = BufWriter::new(stdout.lock());
    write_execution_record_to(&mut output, logical_cpu)?;
    crate::ba02_large::write_once_record(&mut output, measurement)?;
    writeln!(
        output,
        "{{\"record\":\"windows_memory\",\"build_flavor\":\"{BUILD_FLAVOR}\",\
         \"build_target_cpu\":\"{BUILD_TARGET_CPU}\",\"logical_cpu\":{logical_cpu},\
         \"variant\":\"{}\",\"dataset\":\"{}\",\
         \"metric\":\"peak_working_set_kib\",\"peak_working_set_kib\":{peak_kib}}}",
        measurement.variant, measurement.dataset,
    )?;
    output.flush()?;

    Ok(())
}

fn run_memory(args: &[String]) -> DynResult<()> {
    if !(3..=5).contains(&args.len()) {
        return Err(invalid_input(
            "windows-memory-run requires <logical_cpu> <prose|boundary|mixed> \
             [size_mib] [samples]",
        ));
    }
    require_known_build_identity()?;

    let logical_cpu = parse_logical_cpu(&args[1])?;
    let dataset = validate_dataset(&args[2])?;
    let size_mib = parse_positive_usize(args.get(3), DEFAULT_SIZE_MIB, "size_mib")?;
    let samples = parse_positive_usize(args.get(4), DEFAULT_MEMORY_SAMPLES, "samples")?;
    if samples < DEFAULT_MEMORY_SAMPLES {
        return Err(invalid_input(format!(
            "samples must be at least {DEFAULT_MEMORY_SAMPLES} fresh processes per implementation"
        )));
    }

    let executable = env::current_exe()?.canonicalize()?;
    let initial_warm_up = run_memory_child(&executable, logical_cpu, "initial", dataset, size_mib)?;
    validate_memory_observation(&initial_warm_up, "initial", dataset, size_mib, logical_cpu)?;
    let optimized_warm_up =
        run_memory_child(&executable, logical_cpu, "optimized", dataset, size_mib)?;
    validate_memory_observation(
        &optimized_warm_up,
        "optimized",
        dataset,
        size_mib,
        logical_cpu,
    )?;
    if !initial_warm_up.workload_matches(&optimized_warm_up) {
        return Err(io::Error::other(
            "initial and optimized memory warm-ups prepared different inputs",
        )
        .into());
    }

    let mut initial = Vec::with_capacity(samples);
    let mut optimized = Vec::with_capacity(samples);
    let mut acquisition_index = 0;
    for sample_index in 0..samples {
        let variants = if sample_index.is_multiple_of(2) {
            ["initial", "optimized"]
        } else {
            ["optimized", "initial"]
        };

        for variant in variants {
            let observation =
                run_memory_child(&executable, logical_cpu, variant, dataset, size_mib)?;
            validate_memory_observation(&observation, variant, dataset, size_mib, logical_cpu)?;
            if !initial_warm_up.workload_matches(&observation) {
                return Err(io::Error::other(format!(
                    "memory sample {} for {variant} prepared a different workload",
                    sample_index + 1
                ))
                .into());
            }

            let sample = WindowsMemorySample {
                sample_id: sample_index + 1,
                acquisition_index,
                process_id: observation.process_id,
                value_kib: observation.peak_working_set_kib,
            };
            if variant == "initial" {
                initial.push(sample);
            } else {
                optimized.push(sample);
            }
            acquisition_index += 1;
        }

        if (sample_index + 1).is_multiple_of(5) || sample_index + 1 == samples {
            eprintln!(
                "measured memory samples: {}/{} per variant",
                sample_index + 1,
                samples
            );
        }
    }

    let stdout = io::stdout();
    let mut output = BufWriter::new(stdout.lock());
    crate::ba02_large::write_windows_memory_records(
        &mut output,
        BUILD_FLAVOR,
        BUILD_TARGET,
        BUILD_TARGET_CPU,
        dataset,
        size_mib,
        logical_cpu,
        MEMORY_WARM_UPS_PER_VARIANT,
        &initial,
        &optimized,
    )?;
    output.flush()?;

    Ok(())
}

fn run_memory_worker(args: &[String]) -> DynResult<()> {
    if !(4..=5).contains(&args.len()) {
        return Err(invalid_input(
            "windows-memory-worker requires <logical_cpu> <initial|optimized> \
             <prose|boundary|mixed> [size_mib]",
        ));
    }

    let logical_cpu = parse_logical_cpu(&args[1])?;
    let _affinity = pin_current_thread(logical_cpu)?;
    let measurement = crate::ba02_large::measure_once(&args[2..])?;
    let peak_kib = peak_working_set_kib()?;

    println!(
        "MEMORY\t{PROTOCOL_VERSION}\t{BUILD_FLAVOR}\t{BUILD_TARGET}\t{BUILD_TARGET_CPU}\t{}\t{logical_cpu}\t{}\t{}\t{}\t{}\t{:016x}\t{:016x}\t{peak_kib}",
        std::process::id(),
        measurement.variant,
        measurement.dataset,
        measurement.size_mib,
        measurement.size_bytes,
        measurement.fingerprint,
        measurement.checksum,
    );

    Ok(())
}

fn run_worker(args: &[String]) -> DynResult<()> {
    if !(4..=5).contains(&args.len()) {
        return Err(invalid_input(
            "windows-worker requires <logical_cpu> <initial|optimized> \
             <prose|boundary|mixed> [size_mib]",
        ));
    }

    let logical_cpu = parse_logical_cpu(&args[1])?;
    let _affinity = pin_current_thread(logical_cpu)?;
    crate::ba02_large::run_windows_worker(
        &args[2..],
        PROTOCOL_VERSION,
        BUILD_FLAVOR,
        BUILD_TARGET,
        BUILD_TARGET_CPU,
    )
}

fn run_build_runtime(args: &[String]) -> DynResult<()> {
    if !(5..=7).contains(&args.len()) {
        return Err(invalid_input(
            "windows-build-runtime requires <logical_cpu> <znver5_exe> \
             <initial|optimized> <prose|boundary|mixed> [size_mib] [samples]",
        ));
    }
    validate_build_identity(BUILD_FLAVOR, BUILD_TARGET_CPU, "portable", "x86-64")?;
    verify_znver5_host()?;

    let logical_cpu = parse_logical_cpu(&args[1])?;
    let portable_exe = env::current_exe()?.canonicalize()?;
    let candidate_exe = PathBuf::from(&args[2]).canonicalize().map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("failed to resolve znver5 executable {:?}: {error}", args[2]),
        )
    })?;
    if !candidate_exe.is_file() {
        return Err(invalid_input(format!(
            "znver5 executable {candidate_exe:?} is not a file"
        )));
    }
    if portable_exe == candidate_exe {
        return Err(invalid_input(
            "portable and znver5 executables must be different files",
        ));
    }

    let variant = validate_variant(&args[3])?;
    let dataset = validate_dataset(&args[4])?;
    let size_mib = parse_positive_usize(args.get(5), DEFAULT_SIZE_MIB, "size_mib")?;
    let samples = parse_positive_usize(args.get(6), DEFAULT_SAMPLES, "samples")?;
    if samples < MIN_MEASURED_PAIRS {
        return Err(invalid_input(format!(
            "samples must be at least {MIN_MEASURED_PAIRS} measured pairs"
        )));
    }

    let mut portable = Worker::start(
        &portable_exe,
        logical_cpu,
        variant,
        dataset,
        size_mib,
        "portable",
        "x86-64",
    )?;
    let mut znver5 = Worker::start(
        &candidate_exe,
        logical_cpu,
        variant,
        dataset,
        size_mib,
        "znver5",
        "znver5",
    )?;

    if !portable.ready.same_workload(&znver5.ready) {
        return Err(io::Error::other(format!(
            "portable and znver5 workers prepared different inputs:\nportable: {:?}\nznver5: {:?}",
            portable.ready, znver5.ready
        ))
        .into());
    }

    eprintln!(
        "workers ready: variant={variant}, dataset={dataset}, size_mib={size_mib}, cpu={logical_cpu}"
    );
    for pair in 0..WARM_UP_PAIRS {
        let _ = measure_build_pair(pair, &mut portable, &mut znver5)?;
    }

    let mut pairs = Vec::with_capacity(samples);
    for pair in 0..samples {
        pairs.push(measure_build_pair(pair, &mut portable, &mut znver5)?);
        if (pair + 1).is_multiple_of(10) || pair + 1 == samples {
            eprintln!("measured build pairs: {}/{}", pair + 1, samples);
        }
    }

    portable.finish()?;
    znver5.finish()?;

    let stdout = io::stdout();
    let mut output = BufWriter::new(stdout.lock());
    crate::ba02_large::write_build_comparison_records(
        &mut output,
        variant,
        dataset,
        size_mib,
        logical_cpu,
        BUILD_TARGET,
        &pairs,
    )?;

    Ok(())
}

fn measure_build_pair(
    pair: usize,
    portable: &mut Worker,
    znver5: &mut Worker,
) -> DynResult<(Duration, Duration)> {
    if pair.is_multiple_of(2) {
        let portable_elapsed = portable.measure()?;
        let znver5_elapsed = znver5.measure()?;
        Ok((portable_elapsed, znver5_elapsed))
    } else {
        let znver5_elapsed = znver5.measure()?;
        let portable_elapsed = portable.measure()?;
        Ok((portable_elapsed, znver5_elapsed))
    }
}

struct Worker {
    child: Child,
    input: Option<BufWriter<ChildStdin>>,
    responses: Receiver<io::Result<String>>,
    reader_thread: Option<JoinHandle<()>>,
    ready: WorkerReady,
    finished: bool,
}

impl Worker {
    #[allow(clippy::too_many_arguments)]
    fn start(
        executable: &Path,
        logical_cpu: usize,
        variant: &str,
        dataset: &str,
        size_mib: usize,
        expected_flavor: &str,
        expected_target_cpu: &str,
    ) -> DynResult<Self> {
        let mut child = Command::new(executable)
            .arg("ba02")
            .arg("windows-worker")
            .arg(logical_cpu.to_string())
            .arg(variant)
            .arg(dataset)
            .arg(size_mib.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|error| {
                io::Error::new(
                    error.kind(),
                    format!("failed to start worker {executable:?}: {error}"),
                )
            })?;
        let input = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("worker stdin was not piped"))?;
        let output = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("worker stdout was not piped"))?;
        let (responses, reader_thread) = start_line_reader(output);
        let mut worker = Self {
            child,
            input: Some(BufWriter::new(input)),
            responses,
            reader_thread: Some(reader_thread),
            ready: WorkerReady {
                protocol_version: 0,
                build_flavor: String::new(),
                build_target: String::new(),
                build_target_cpu: String::new(),
                variant: String::new(),
                dataset: String::new(),
                size_bytes: 0,
                fingerprint: 0,
                checksum: 0,
            },
            finished: false,
        };
        let ready = WorkerReady::parse(&worker.receive_line("readiness")?)?;
        validate_worker_ready(
            &ready,
            expected_flavor,
            expected_target_cpu,
            variant,
            dataset,
            size_mib,
        )?;
        worker.ready = ready;

        Ok(worker)
    }

    fn measure(&mut self) -> DynResult<Duration> {
        self.send_command("run")?;
        let response = self.receive_line("sample")?;
        let nanoseconds = response
            .trim()
            .parse::<u64>()
            .map_err(|_| io::Error::other(format!("invalid worker response {response:?}")))?;
        if nanoseconds == 0 {
            return Err(io::Error::other("worker returned a zero-duration sample").into());
        }

        Ok(Duration::from_nanos(nanoseconds))
    }

    fn finish(&mut self) -> DynResult<()> {
        if self.finished {
            return Ok(());
        }

        self.send_command("quit")?;
        self.input.take();
        let status = wait_for_exit(&mut self.child, WORKER_TIMEOUT)?;
        self.finished = true;
        self.join_reader();
        if !status.success() {
            return Err(
                io::Error::other(format!("worker exited unsuccessfully with {status}")).into(),
            );
        }

        Ok(())
    }

    fn send_command(&mut self, command: &str) -> io::Result<()> {
        let input = self
            .input
            .as_mut()
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "worker stdin is closed"))?;
        writeln!(input, "{command}")?;
        input.flush()
    }

    fn receive_line(&self, phase: &str) -> io::Result<String> {
        match self.responses.recv_timeout(WORKER_TIMEOUT) {
            Ok(result) => result,
            Err(RecvTimeoutError::Timeout) => Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("worker {phase} timed out after {WORKER_TIMEOUT:?}"),
            )),
            Err(RecvTimeoutError::Disconnected) => Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                format!("worker exited before sending {phase}"),
            )),
        }
    }

    fn join_reader(&mut self) {
        if let Some(thread) = self.reader_thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        if self.finished {
            self.join_reader();
            return;
        }

        if let Some(input) = self.input.as_mut() {
            let _ = writeln!(input, "quit");
            let _ = input.flush();
        }
        self.input.take();
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
        }
        if wait_for_exit(&mut self.child, Duration::from_secs(5)).is_ok() {
            self.finished = true;
            self.join_reader();
        } else {
            self.reader_thread.take();
        }
    }
}

fn start_line_reader(output: ChildStdout) -> (Receiver<io::Result<String>>, JoinHandle<()>) {
    let (sender, receiver) = mpsc::channel();
    let reader_thread = thread::spawn(move || {
        let mut output = BufReader::new(output);
        loop {
            let mut line = String::new();
            match output.read_line(&mut line) {
                Ok(0) => return,
                Ok(_) => {
                    if sender.send(Ok(line)).is_err() {
                        return;
                    }
                }
                Err(error) => {
                    let _ = sender.send(Err(error));
                    return;
                }
            }
        }
    });
    (receiver, reader_thread)
}

fn wait_for_exit(child: &mut Child, timeout: Duration) -> io::Result<ExitStatus> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            let timeout_error = io::Error::new(
                io::ErrorKind::TimedOut,
                format!("child process timed out after {timeout:?}"),
            );
            child.kill()?;

            let reap_deadline = Instant::now() + Duration::from_secs(5);
            loop {
                if child.try_wait()?.is_some() {
                    return Err(timeout_error);
                }
                if Instant::now() >= reap_deadline {
                    return Err(io::Error::other(format!(
                        "{timeout_error}; terminated child could not be reaped within 5s"
                    )));
                }
                thread::sleep(PROCESS_POLL_INTERVAL);
            }
        }
        thread::sleep(PROCESS_POLL_INTERVAL);
    }
}

fn run_memory_child(
    executable: &Path,
    logical_cpu: usize,
    variant: &str,
    dataset: &str,
    size_mib: usize,
) -> DynResult<MemoryObservation> {
    let mut child = Command::new(executable)
        .arg("ba02")
        .arg("windows-memory-worker")
        .arg(logical_cpu.to_string())
        .arg(variant)
        .arg(dataset)
        .arg(size_mib.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("failed to start memory worker {executable:?}: {error}"),
            )
        })?;

    let stdout_reader = start_child_pipe_reader(child.stdout.take(), "stdout")?;
    let stderr_reader = start_child_pipe_reader(child.stderr.take(), "stderr")?;
    let status = match wait_for_exit(&mut child, WORKER_TIMEOUT) {
        Ok(status) => status,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            let _ = join_child_pipe_reader(stdout_reader, "stdout");
            let _ = join_child_pipe_reader(stderr_reader, "stderr");
            return Err(error.into());
        }
    };
    let stdout = join_child_pipe_reader(stdout_reader, "stdout")?;
    let stderr = join_child_pipe_reader(stderr_reader, "stderr")?;
    if !status.success() {
        return Err(io::Error::other(format!(
            "memory worker exited with {status}: {}",
            String::from_utf8_lossy(&stderr).trim()
        ))
        .into());
    }

    let stdout = String::from_utf8(stdout)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let lines = stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>();
    if lines.len() != 1 {
        return Err(io::Error::other(format!(
            "memory worker returned {} non-empty records instead of one",
            lines.len()
        ))
        .into());
    }

    Ok(MemoryObservation::parse(lines[0])?)
}

fn start_child_pipe_reader<R>(
    pipe: Option<R>,
    name: &'static str,
) -> io::Result<JoinHandle<io::Result<Vec<u8>>>>
where
    R: Read + Send + 'static,
{
    let mut pipe = pipe.ok_or_else(|| io::Error::other(format!("child {name} was not piped")))?;
    thread::Builder::new()
        .name(format!("benchmark-{name}-reader"))
        .spawn(move || {
            let mut bytes = Vec::new();
            pipe.read_to_end(&mut bytes)?;
            Ok(bytes)
        })
}

fn join_child_pipe_reader(
    reader: JoinHandle<io::Result<Vec<u8>>>,
    name: &str,
) -> io::Result<Vec<u8>> {
    reader
        .join()
        .map_err(|_| io::Error::other(format!("child {name} reader thread panicked")))?
}

fn validate_worker_ready(
    ready: &WorkerReady,
    expected_flavor: &str,
    expected_target_cpu: &str,
    expected_variant: &str,
    expected_dataset: &str,
    expected_size_mib: usize,
) -> io::Result<()> {
    validate_build_identity(
        &ready.build_flavor,
        &ready.build_target_cpu,
        expected_flavor,
        expected_target_cpu,
    )?;
    let expected_size = size_in_bytes(expected_size_mib)?;
    if ready.protocol_version != PROTOCOL_VERSION
        || ready.build_target != BUILD_TARGET
        || ready.variant != expected_variant
        || ready.dataset != expected_dataset
        || ready.size_bytes != expected_size
        || ready.fingerprint == 0
        || ready.checksum == 0
    {
        return Err(io::Error::other(format!(
            "worker readiness does not match requested workload: {ready:?}"
        )));
    }
    Ok(())
}

fn validate_memory_observation(
    observation: &MemoryObservation,
    expected_variant: &str,
    expected_dataset: &str,
    expected_size_mib: usize,
    expected_logical_cpu: usize,
) -> io::Result<()> {
    require_known_build_identity()?;
    let expected_size = size_in_bytes(expected_size_mib)?;
    if observation.protocol_version != PROTOCOL_VERSION
        || observation.build_flavor != BUILD_FLAVOR
        || observation.build_target != BUILD_TARGET
        || observation.build_target_cpu != BUILD_TARGET_CPU
        || observation.process_id == 0
        || observation.logical_cpu != expected_logical_cpu
        || observation.variant != expected_variant
        || observation.dataset != expected_dataset
        || observation.size_mib != expected_size_mib
        || observation.size_bytes != expected_size
        || observation.fingerprint == 0
        || observation.checksum == 0
        || observation.peak_working_set_kib == 0
    {
        return Err(io::Error::other(format!(
            "memory observation does not match requested workload: {observation:?}"
        )));
    }
    Ok(())
}

fn parse_protocol_field<T>(value: &str, name: &str) -> io::Result<T>
where
    T: std::str::FromStr,
{
    value.parse::<T>().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid {name} in worker protocol: {value:?}"),
        )
    })
}

fn parse_hex_protocol_field(value: &str, name: &str) -> io::Result<u64> {
    u64::from_str_radix(value, 16).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid hexadecimal {name} in worker protocol: {value:?}"),
        )
    })
}

fn parse_logical_cpu(value: &str) -> DynResult<usize> {
    let logical_cpu = value
        .parse::<usize>()
        .map_err(|error| invalid_input(format!("invalid logical_cpu {value:?}: {error}")))?;
    logical_cpu_mask(logical_cpu, affinity_masks()?.0)?;
    Ok(logical_cpu)
}

fn logical_cpu_mask(logical_cpu: usize, process_mask: usize) -> io::Result<usize> {
    if process_mask == 0 {
        return Err(io::Error::other("process affinity mask is empty"));
    }
    let shift = u32::try_from(logical_cpu).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("logical_cpu {logical_cpu} cannot be represented by an affinity mask"),
        )
    })?;
    let mask = 1_usize.checked_shl(shift).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "logical_cpu {logical_cpu} cannot be represented by a {}-bit affinity mask",
                usize::BITS
            ),
        )
    })?;
    if process_mask & mask == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "logical_cpu {logical_cpu} is not available in process affinity mask 0x{process_mask:016x}"
            ),
        ));
    }
    Ok(mask)
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

fn size_in_bytes(size_mib: usize) -> io::Result<usize> {
    size_mib.checked_mul(MIB).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "size_mib exceeds addressable memory",
        )
    })
}

fn validate_variant(value: &str) -> DynResult<&str> {
    match value {
        "initial" | "optimized" => Ok(value),
        _ => Err(invalid_input(format!(
            "unknown variant {value:?}; expected initial or optimized"
        ))),
    }
}

fn validate_dataset(value: &str) -> DynResult<&str> {
    match value {
        "prose" | "boundary" | "mixed" => Ok(value),
        _ => Err(invalid_input(format!(
            "unknown dataset {value:?}; expected prose, boundary, or mixed"
        ))),
    }
}

fn affinity_masks() -> io::Result<(usize, usize)> {
    let mut process_mask = 0_usize;
    let mut system_mask = 0_usize;

    // SAFETY: `GetCurrentProcess` returns a pseudo-handle valid in this process.
    // Both output pointers refer to initialized writable `usize` values for the
    // duration of the call. On x64 Windows, DWORD_PTR is `usize`.
    let succeeded =
        unsafe { GetProcessAffinityMask(GetCurrentProcess(), &mut process_mask, &mut system_mask) };
    if succeeded == 0 {
        return Err(io::Error::last_os_error());
    }
    if process_mask == 0 {
        return Err(io::Error::other(
            "GetProcessAffinityMask returned an empty process mask",
        ));
    }

    Ok((process_mask, system_mask))
}

struct ThreadAffinityGuard {
    previous_mask: usize,
    _not_send: PhantomData<Rc<()>>,
}

impl Drop for ThreadAffinityGuard {
    fn drop(&mut self) {
        // SAFETY: the guard is created and dropped on the benchmark thread.
        // `previous_mask` is the non-zero mask returned by the successful
        // `SetThreadAffinityMask` call that created this guard.
        unsafe {
            SetThreadAffinityMask(GetCurrentThread(), self.previous_mask);
        }
    }
}

fn pin_current_thread(logical_cpu: usize) -> io::Result<ThreadAffinityGuard> {
    let mask = logical_cpu_mask(logical_cpu, affinity_masks()?.0)?;

    // SAFETY: `GetCurrentThread` returns a valid pseudo-handle for the calling
    // thread. `mask` is non-zero, representable, and present in the process
    // affinity mask for the current processor group. No pointer is dereferenced.
    let previous_mask = unsafe { SetThreadAffinityMask(GetCurrentThread(), mask) };
    if previous_mask == 0 {
        return Err(io::Error::last_os_error());
    }

    Ok(ThreadAffinityGuard {
        previous_mask,
        _not_send: PhantomData,
    })
}

fn peak_working_set_kib() -> io::Result<u64> {
    let mut counters = ProcessMemoryCounters {
        cb: u32::try_from(std::mem::size_of::<ProcessMemoryCounters>())
            .map_err(|_| io::Error::other("process-memory counter structure is too large"))?,
        ..ProcessMemoryCounters::default()
    };

    // SAFETY: `GetCurrentProcess` returns a pseudo-handle valid in this process.
    // `counters` points to writable storage of exactly the supplied size and
    // remains alive for the call. Compile-time assertions above verify the x64
    // PROCESS_MEMORY_COUNTERS layout used by the GNU target.
    let succeeded =
        unsafe { GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, counters.cb) };
    if succeeded == 0 {
        return Err(io::Error::last_os_error());
    }

    Ok(bytes_to_kib(counters.PeakWorkingSetSize))
}

fn bytes_to_kib(bytes: usize) -> u64 {
    bytes.div_ceil(1024) as u64
}

fn require_known_build_identity() -> io::Result<()> {
    match BUILD_FLAVOR {
        "portable" => validate_build_identity(BUILD_FLAVOR, BUILD_TARGET_CPU, "portable", "x86-64"),
        "znver5" => validate_build_identity(BUILD_FLAVOR, BUILD_TARGET_CPU, "znver5", "znver5"),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "benchmark binary is unlabeled; rebuild with HW1_BENCH_BUILD_FLAVOR and matching target-cpu",
        )),
    }
}

fn validate_build_identity(
    actual_flavor: &str,
    actual_target_cpu: &str,
    expected_flavor: &str,
    expected_target_cpu: &str,
) -> io::Result<()> {
    if actual_flavor != expected_flavor || actual_target_cpu != expected_target_cpu {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "expected {expected_flavor}/{expected_target_cpu} build, found {actual_flavor}/{actual_target_cpu}"
            ),
        ));
    }
    Ok(())
}

fn verify_znver5_host() -> io::Result<()> {
    let processor = processor_identity()?;
    if processor.name != TARGET_PROCESSOR_NAME
        || processor.vendor != TARGET_PROCESSOR_VENDOR
        || processor.family != TARGET_PROCESSOR_FAMILY
        || processor.model != TARGET_PROCESSOR_MODEL
    {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            format!(
                "znver5 comparison requires {TARGET_PROCESSOR_NAME}, {TARGET_PROCESSOR_VENDOR} \
                 family {TARGET_PROCESSOR_FAMILY} model {TARGET_PROCESSOR_MODEL}; detected \
                 {:?}, {} family {} model {} stepping {}",
                processor.name,
                processor.vendor,
                processor.family,
                processor.model,
                processor.stepping,
            ),
        ));
    }

    let missing = missing_znver5_features();
    if !missing.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            format!(
                "host or OS state does not expose required znver5 features: {}",
                missing.join(",")
            ),
        ));
    }
    Ok(())
}

fn write_execution_record(logical_cpu: usize) -> io::Result<()> {
    let stdout = io::stdout();
    let mut output = BufWriter::new(stdout.lock());
    write_execution_record_to(&mut output, logical_cpu)?;
    output.flush()
}

fn write_execution_record_to(output: &mut impl Write, logical_cpu: usize) -> io::Result<()> {
    writeln!(
        output,
        "{{\"record\":\"windows_execution\",\"build_flavor\":\"{BUILD_FLAVOR}\",\
         \"build_target\":\"{BUILD_TARGET}\",\"build_target_cpu\":\"{BUILD_TARGET_CPU}\",\
         \"logical_cpu\":{logical_cpu},\"processor_group_scope\":\"current_group_only\"}}"
    )
}

fn processor_identity() -> io::Result<ProcessorIdentity> {
    // CPUID is available in x86-64 mode. These leaves only read CPU
    // identification registers and do not access memory.
    let basic = __cpuid(0);
    let mut vendor_bytes = Vec::with_capacity(12);
    vendor_bytes.extend_from_slice(&basic.ebx.to_le_bytes());
    vendor_bytes.extend_from_slice(&basic.edx.to_le_bytes());
    vendor_bytes.extend_from_slice(&basic.ecx.to_le_bytes());
    let vendor = String::from_utf8(vendor_bytes)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;

    // Leaf 1 is supported when leaf 0 reports a maximum basic leaf of at least
    // 1, which every x86-64 CPU must provide.
    let version = __cpuid(1).eax;
    let base_family = (version >> 8) & 0x0f;
    let extended_family = (version >> 20) & 0xff;
    let family = if base_family == 0x0f {
        base_family + extended_family
    } else {
        base_family
    };
    let base_model = (version >> 4) & 0x0f;
    let extended_model = (version >> 16) & 0x0f;
    let model = if matches!(base_family, 0x06 | 0x0f) {
        (extended_model << 4) | base_model
    } else {
        base_model
    };
    let stepping = version & 0x0f;

    // Extended leaf 0 reports the supported extended range.
    let maximum_extended_leaf = __cpuid(0x8000_0000).eax;
    if maximum_extended_leaf < 0x8000_0004 {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "CPU does not expose the extended processor brand leaves",
        ));
    }

    let mut name_bytes = Vec::with_capacity(48);
    for leaf in 0x8000_0002..=0x8000_0004 {
        // The maximum extended leaf check above covers this range.
        let brand = __cpuid(leaf);
        name_bytes.extend_from_slice(&brand.eax.to_le_bytes());
        name_bytes.extend_from_slice(&brand.ebx.to_le_bytes());
        name_bytes.extend_from_slice(&brand.ecx.to_le_bytes());
        name_bytes.extend_from_slice(&brand.edx.to_le_bytes());
    }
    let name_end = name_bytes
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(name_bytes.len());
    let name = String::from_utf8(name_bytes[..name_end].to_vec())
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?
        .trim()
        .to_owned();
    if name.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "CPUID processor brand string is empty",
        ));
    }

    Ok(ProcessorIdentity {
        name,
        vendor,
        family,
        model,
        stepping,
    })
}

fn missing_znver5_features() -> Vec<&'static str> {
    let mut missing = Vec::new();
    if !std::arch::is_x86_feature_detected!("adx") {
        missing.push("adx");
    }
    if !std::arch::is_x86_feature_detected!("aes") {
        missing.push("aes");
    }
    if !std::arch::is_x86_feature_detected!("avx") {
        missing.push("avx");
    }
    if !std::arch::is_x86_feature_detected!("avx2") {
        missing.push("avx2");
    }
    if !std::arch::is_x86_feature_detected!("avx512bf16") {
        missing.push("avx512bf16");
    }
    if !std::arch::is_x86_feature_detected!("avx512bitalg") {
        missing.push("avx512bitalg");
    }
    if !std::arch::is_x86_feature_detected!("avx512bw") {
        missing.push("avx512bw");
    }
    if !std::arch::is_x86_feature_detected!("avx512cd") {
        missing.push("avx512cd");
    }
    if !std::arch::is_x86_feature_detected!("avx512dq") {
        missing.push("avx512dq");
    }
    if !std::arch::is_x86_feature_detected!("avx512f") {
        missing.push("avx512f");
    }
    if !std::arch::is_x86_feature_detected!("avx512ifma") {
        missing.push("avx512ifma");
    }
    if !std::arch::is_x86_feature_detected!("avx512vbmi") {
        missing.push("avx512vbmi");
    }
    if !std::arch::is_x86_feature_detected!("avx512vbmi2") {
        missing.push("avx512vbmi2");
    }
    if !std::arch::is_x86_feature_detected!("avx512vl") {
        missing.push("avx512vl");
    }
    if !std::arch::is_x86_feature_detected!("avx512vnni") {
        missing.push("avx512vnni");
    }
    if !std::arch::is_x86_feature_detected!("avx512vp2intersect") {
        missing.push("avx512vp2intersect");
    }
    if !std::arch::is_x86_feature_detected!("avx512vpopcntdq") {
        missing.push("avx512vpopcntdq");
    }
    if !std::arch::is_x86_feature_detected!("avxvnni") {
        missing.push("avxvnni");
    }
    if !std::arch::is_x86_feature_detected!("bmi1") {
        missing.push("bmi1");
    }
    if !std::arch::is_x86_feature_detected!("bmi2") {
        missing.push("bmi2");
    }
    if !std::arch::is_x86_feature_detected!("cmpxchg16b") {
        missing.push("cmpxchg16b");
    }
    if !std::arch::is_x86_feature_detected!("f16c") {
        missing.push("f16c");
    }
    if !std::arch::is_x86_feature_detected!("fma") {
        missing.push("fma");
    }
    if !std::arch::is_x86_feature_detected!("fxsr") {
        missing.push("fxsr");
    }
    if !std::arch::is_x86_feature_detected!("gfni") {
        missing.push("gfni");
    }
    if !std::arch::is_x86_feature_detected!("lzcnt") {
        missing.push("lzcnt");
    }
    if !std::arch::is_x86_feature_detected!("movbe") {
        missing.push("movbe");
    }
    if !std::arch::is_x86_feature_detected!("pclmulqdq") {
        missing.push("pclmulqdq");
    }
    if !std::arch::is_x86_feature_detected!("popcnt") {
        missing.push("popcnt");
    }
    if !std::arch::is_x86_feature_detected!("rdrand") {
        missing.push("rdrand");
    }
    if !std::arch::is_x86_feature_detected!("rdseed") {
        missing.push("rdseed");
    }
    if !std::arch::is_x86_feature_detected!("sha") {
        missing.push("sha");
    }
    if !std::arch::is_x86_feature_detected!("sse") {
        missing.push("sse");
    }
    if !std::arch::is_x86_feature_detected!("sse2") {
        missing.push("sse2");
    }
    if !std::arch::is_x86_feature_detected!("sse3") {
        missing.push("sse3");
    }
    if !std::arch::is_x86_feature_detected!("sse4.1") {
        missing.push("sse4.1");
    }
    if !std::arch::is_x86_feature_detected!("sse4.2") {
        missing.push("sse4.2");
    }
    if !std::arch::is_x86_feature_detected!("sse4a") {
        missing.push("sse4a");
    }
    if !std::arch::is_x86_feature_detected!("ssse3") {
        missing.push("ssse3");
    }
    if !std::arch::is_x86_feature_detected!("vaes") {
        missing.push("vaes");
    }
    if !std::arch::is_x86_feature_detected!("vpclmulqdq") {
        missing.push("vpclmulqdq");
    }
    if !std::arch::is_x86_feature_detected!("xsave") {
        missing.push("xsave");
    }
    if !std::arch::is_x86_feature_detected!("xsavec") {
        missing.push("xsavec");
    }
    if !std::arch::is_x86_feature_detected!("xsaveopt") {
        missing.push("xsaveopt");
    }
    if !std::arch::is_x86_feature_detected!("xsaves") {
        missing.push("xsaves");
    }
    missing
}

fn detected_x86_features() -> String {
    let missing = missing_znver5_features();
    ZNVER5_REQUIRED_FEATURES
        .iter()
        .copied()
        .filter(|feature| !missing.contains(feature))
        .collect::<Vec<_>>()
        .join(",")
}

fn invalid_input(message: impl Into<String>) -> Box<dyn Error> {
    Box::new(io::Error::new(io::ErrorKind::InvalidInput, message.into()))
}

#[cfg(test)]
mod tests {
    use super::{
        BUILD_TARGET, MemoryObservation, PROTOCOL_VERSION, WorkerReady, affinity_masks,
        bytes_to_kib, is_command, logical_cpu_mask, peak_working_set_kib, pin_current_thread,
        validate_build_identity, validate_worker_ready,
    };

    #[test]
    fn recognizes_windows_commands_only() {
        for command in [
            "windows-info",
            "windows-runtime",
            "windows-once",
            "windows-memory-run",
            "windows-memory-summary",
            "windows-memory-worker",
            "windows-worker",
            "windows-build-runtime",
        ] {
            assert!(
                is_command(Some(command)),
                "command {command} was not recognized"
            );
        }
        assert!(!is_command(None));
        assert!(!is_command(Some("runtime")));
    }

    #[test]
    fn rounds_bytes_up_to_kib() {
        assert_eq!(bytes_to_kib(0), 0);
        assert_eq!(bytes_to_kib(1), 1);
        assert_eq!(bytes_to_kib(1023), 1);
        assert_eq!(bytes_to_kib(1024), 1);
        assert_eq!(bytes_to_kib(1025), 2);
    }

    #[test]
    fn validates_sparse_process_affinity_masks() {
        assert_eq!(logical_cpu_mask(1, 0b1010).unwrap(), 0b0010);
        assert_eq!(logical_cpu_mask(3, 0b1010).unwrap(), 0b1000);
        assert!(logical_cpu_mask(0, 0b1010).is_err());
        assert!(logical_cpu_mask(2, 0b1010).is_err());
        assert!(logical_cpu_mask(4, 0b1010).is_err());
        assert!(logical_cpu_mask(usize::BITS as usize, usize::MAX).is_err());
        assert!(logical_cpu_mask(0, 0).is_err());
    }

    #[test]
    fn rejects_wrong_or_missing_build_flavor() {
        validate_build_identity("portable", "x86-64", "portable", "x86-64").unwrap();
        assert!(validate_build_identity("znver5", "znver5", "portable", "x86-64").is_err());
        assert!(validate_build_identity("unlabeled", "unspecified", "portable", "x86-64").is_err());
    }

    #[test]
    fn parses_and_validates_worker_readiness() {
        let line = format!(
            "READY\t{PROTOCOL_VERSION}\tportable\t{BUILD_TARGET}\tx86-64\tinitial\tprose\t1048576\t0000000000000001\t0000000000000002"
        );
        let ready = WorkerReady::parse(&line).unwrap();
        validate_worker_ready(&ready, "portable", "x86-64", "initial", "prose", 1).unwrap();

        let mut mismatched = ready.clone();
        mismatched.dataset = "mixed".to_owned();
        assert!(
            validate_worker_ready(&mismatched, "portable", "x86-64", "initial", "prose", 1)
                .is_err()
        );
        assert!(validate_worker_ready(&ready, "znver5", "znver5", "initial", "prose", 1).is_err());
    }

    #[test]
    fn parses_memory_worker_record() {
        let line = format!(
            "MEMORY\t{PROTOCOL_VERSION}\tportable\t{BUILD_TARGET}\tx86-64\t42\t3\toptimized\tmixed\t1\t1048576\t0000000000000001\t0000000000000002\t4096"
        );
        let observation = MemoryObservation::parse(&line).unwrap();
        assert_eq!(observation.process_id, 42);
        assert_eq!(observation.logical_cpu, 3);
        assert_eq!(observation.variant, "optimized");
        assert_eq!(observation.peak_working_set_kib, 4096);
    }

    #[test]
    fn one_mib_initial_and_optimized_smoke_match() {
        let initial_args = ["initial", "prose", "1"].map(String::from);
        let optimized_args = ["optimized", "prose", "1"].map(String::from);
        let initial = crate::ba02_large::measure_once(&initial_args).unwrap();
        let optimized = crate::ba02_large::measure_once(&optimized_args).unwrap();

        assert_eq!(initial.counts, optimized.counts);
        assert_eq!(initial.fingerprint, optimized.fingerprint);
        assert_eq!(initial.checksum, optimized.checksum);
    }

    #[test]
    fn win32_memory_and_affinity_smoke() {
        assert!(peak_working_set_kib().unwrap() > 0);
        let (process_mask, _) = affinity_masks().unwrap();
        let logical_cpu = process_mask.trailing_zeros() as usize;
        let guard = pin_current_thread(logical_cpu).unwrap();
        drop(guard);
    }
}
