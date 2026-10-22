# Bonus `ba02` benchmark on Ryzen 9 9950X3D

The goal is to test two things separately on native Windows build 26100 (24H2):

1. the effect of the streaming implementation in `ba02`;
2. the effect of compiling with `target-cpu=znver5` compared with a portable build.

The main homework solution was not changed. The benchmark uses additional code.

## Test environment and methodology

- CPU: AMD Ryzen 9 9950X3D, 16 cores / 32 logical processors.
- OS: Windows build 26100 (24H2), native process.
- Compiler: Rust 1.95.0, LLVM 22.1.2, GNU target `x86_64-pc-windows-gnu`.
- Builds: portable `target-cpu=x86-64` and a separate `target-cpu=znver5` build.
- Data: `prose`, `boundary`, `mixed`, 256 MiB each.
- Timing: 3 warm-up pairs, then 101 pairs alternating A/B and B/A.
- 15 pairs were discarded from each tail; 71 pairs were retained.
- The trimmed mean, median, MAD, and paired 95% CI are reported for the retained observations.
- All series met the preselected CI-width adequacy criterion.
- The thread was pinned to logical CPU 0.
- Peak Working Set was measured separately: a fresh process per observation, 1 warm-up and 31 observations per implementation; each implementation was trimmed independently, leaving 23 observations each.

Test data generation is excluded from the measured time. The results of both implementations were checked against each other before measurements.

## What was applied

### Streaming instead of storing the entire input

The initial version first calls `read_to_end`, copies the entire input into a growing `Vec<u8>`, then makes a separate pass over it to count newlines and another pass to count words. This approach is simple, but stores the entire input and makes multiple passes over the data.

The streaming version uses `BufRead::fill_buf` with a 64 KiB buffer. In a single pass over each block, it counts bytes, lines, and word starts simultaneously, then marks the block as processed using `consume`. The `inside_word` state is carried across blocks, so a word crossing a buffer boundary is counted correctly.

Both implementations have `Θ(n)` time complexity, but differ in their constants and auxiliary memory:

- initial: the entire input in a `Vec`, `Θ(n)` auxiliary memory;
- streaming: a fixed buffer and a few counters, `Θ(1)` auxiliary memory relative to input size.

This is an algorithmic and I/O optimization that is useful beyond Zen 5.

### Separate Zen 5 specialization

The same safe Rust code was built in two ways: a portable variant with `target-cpu=x86-64` and a separate variant with `target-cpu=znver5`. No manual SIMD instructions, assembly, or `unsafe` were added: LLVM handled instruction selection and loop unrolling.

This neither replaces the main homework version nor moves `znver5` into the shared baseline. Separate release binaries are compared, and the specialized binary is allowed to run only after checking the CPU and the required OS-exposed ISA capabilities.

## Initial versus streaming

Times are in milliseconds. A speedup greater than `1×` indicates an advantage for streaming.

### Portable build

| Data | Initial mean / median, ms | Streaming mean / median, ms | MAD initial / streaming, ms | Median speedup, 95% CI |
|---|---:|---:|---:|---:|
| `prose` | 136.378 / 136.325 | 89.888 / 90.226 | 1.006 / 0.952 | 1.517× [1.508; 1.525] |
| `boundary` | 144.222 / 143.223 | 90.191 / 90.270 | 1.669 / 1.146 | 1.596× [1.580; 1.610] |
| `mixed` | 796.496 / 796.471 | 883.269 / 883.224 | 2.508 / 2.197 | 0.902× [0.901; 0.903] |

### `znver5` build

| Data | Initial mean / median, ms | Streaming mean / median, ms | MAD initial / streaming, ms | Median speedup, 95% CI |
|---|---:|---:|---:|---:|
| `prose` | 90.771 / 90.544 | 82.383 / 82.129 | 1.095 / 1.432 | 1.104× [1.096; 1.112] |
| `boundary` | 97.763 / 97.809 | 79.408 / 79.479 | 0.832 / 1.255 | 1.229× [1.223; 1.235] |
| `mixed` | 759.352 / 756.549 | 856.127 / 853.355 | 3.783 / 3.061 | 0.888× [0.886; 0.889] |

Converting the median speedup to a change in streaming time relative to initial gives:

- portable / `prose`: approximately 34.1% less time;
- portable / `boundary`: approximately 37.3% less time;
- portable / `mixed`: approximately 10.9% more time, a regression;
- `znver5` / `prose`: approximately 9.4% less time;
- `znver5` / `boundary`: approximately 18.6% less time;
- `znver5` / `mixed`: approximately 12.6% more time, a regression.

On `prose` and `boundary`, streaming wins by avoiding a full copy of the input, making fewer passes, and reusing a small buffer. `boundary` additionally confirms that carrying `inside_word` across 64-KiB block boundaries does not break the result.

On the synthetic `mixed` dataset, word state and branch outcomes change much more frequently. This plausibly hinders optimization of the stateful loop, whereas the initial implementation's separate newline-counting pass vectorizes well. However, this is only a possible mechanism: the cause of the regression was not isolated separately, and hardware branch and cache-miss counters were not collected.

## Portable versus `znver5`

For each row, the two builds were measured in a separate paired series. A speedup greater than `1×` indicates an advantage for `znver5`.

| Implementation / data | Portable mean / median, ms | `znver5` mean / median, ms | MAD portable / `znver5`, ms | Median speedup, 95% CI |
|---|---:|---:|---:|---:|
| Initial / `prose` | 137.642 / 135.301 | 91.959 / 90.084 | 2.165 / 2.340 | 1.499× [1.479; 1.526] |
| Initial / `boundary` | 146.294 / 145.614 | 100.145 / 99.322 | 2.777 / 2.494 | 1.470× [1.447; 1.478] |
| Initial / `mixed` | 816.685 / 812.043 | 770.322 / 765.411 | 11.553 / 11.055 | 1.060× [1.056; 1.066] |
| Streaming / `prose` | 90.373 / 90.064 | 81.903 / 81.019 | 1.822 / 1.609 | 1.100× [1.091; 1.114] |
| Streaming / `boundary` | 93.533 / 91.917 | 80.692 / 79.875 | 2.451 / 1.639 | 1.152× [1.142; 1.167] |
| Streaming / `mixed` | 878.832 / 870.241 | 850.611 / 841.196 | 13.023 / 10.940 | 1.034× [1.030; 1.036] |

Converting the median speedup to a reduction in `znver5` time relative to portable gives:

- initial / `prose`: approximately 33.3%;
- initial / `boundary`: approximately 32.0%;
- initial / `mixed`: approximately 5.7%;
- streaming / `prose`: approximately 9.1%;
- streaming / `boundary`: approximately 13.2%;
- streaming / `mixed`: approximately 3.3%.

The strongest effect was observed in the initial version, where LLVM replaced the narrower portable newline-counting code with AVX-512 code. The stateful streaming loop did not become a similar fully vectorized pass, so its gain is more modest and more dependent on the data.

In all six comparisons, the 95% CI is above `1×`, meaning that the speedup in this campaign is distinguishable from the observed variability. However, statistical distinguishability does not equal practical value: the preselected 5% threshold was exceeded in only five of six cases. Streaming / `mixed` sped up by approximately 3.3–3.4%, so the result is positive but below the practical threshold.

## Memory

Peak Working Set was measured on a 256 MiB `prose` input. This is the process's peak resident working set; the metric is not equivalent to total virtual/committed memory and does not measure the function's heap allocations.

| Build | Initial mean / median, KiB | Streaming mean / median, KiB | MAD initial / streaming, KiB | Reduction in mean, 95% CI |
|---|---:|---:|---:|---:|
| Portable | 530,163.3 / 530,160 | 4,886.6 / 4,888 | 8 / 4 | 99.078% [99.078%; 99.079%] |
| `znver5` | 530,137.2 / 530,136 | 4,852.9 / 4,856 | 20 / 4 | 99.085% [99.084%; 99.085%] |

In the memory series, the input was generated by a deterministic reader inside a fresh process. Initial materializes it in full in a `Vec`, while streaming retains only the current 64-KiB block and constant-size counting state. As a result, mean Peak Working Set decreased:

- portable: from 530,163.3 to 4,886.6 KiB — by 99.078%;
- `znver5`: from 530,137.2 to 4,852.9 KiB — by 99.085%.

Peak Working Set covers the process's entire resident working set, including runtime and allocator behavior, so these numbers do not equal the exact size of the `Vec` or the function's heap allocations. The count and size of individual heap allocations were not instrumented. The demonstrated conclusion applies specifically to Peak Working Set and algorithmic auxiliary memory: initial is `Θ(n)`, streaming is `Θ(1)` relative to input size. Both versions have `Θ(n)` time complexity.

## What the compiler changed

Inspection of the disassembled final binary showed:

- portable initial counts newlines using 128-bit SSE instructions;
- `znver5` initial uses AVX-512 comparisons, masks, and vector summation;
- the stateful streaming loop remains scalar, but is unrolled from approximately 2 to 8 bytes per iteration;
- more aggressive unrolling increases the size of individual hot-path code sections.

This explains a possible speedup mechanism, but does not establish the contribution of each instruction individually: hardware performance counters were not collected.

## Limitations and conclusion

The measurements apply only to a single campaign on a Ryzen 9 9950X3D under native Windows build 26100. Pinning to CPU 0 does not establish specific CCD/V-Cache placement or the absence of work on the neighboring SMT thread. The power mode was not fixed; no deliberate background load was started.

Streaming drastically reduces Peak Working Set and speeds up two text distributions, but loses on `mixed`. The `znver5` build produced a measurable gain in all six paired comparisons, but only five exceeded the practical 5% threshold. This is a local result for this processor and workload, not a universal guarantee for Zen 5.

## Reproduction

```sh
CARGO_INCREMENTAL=0 HW1_BENCH_BUILD_FLAVOR=portable RUSTFLAGS='-C target-cpu=x86-64' cargo build --frozen --release -p benchmark --target x86_64-pc-windows-gnu --target-dir target/windows-portable
CARGO_INCREMENTAL=0 HW1_BENCH_BUILD_FLAVOR=znver5 RUSTFLAGS='-C target-cpu=znver5' cargo build --frozen --release -p benchmark --target x86_64-pc-windows-gnu --target-dir target/windows-znver5

target/windows-portable/x86_64-pc-windows-gnu/release/benchmark.exe ba02 windows-runtime 0 256 101
target/windows-portable/x86_64-pc-windows-gnu/release/benchmark.exe ba02 windows-build-runtime 0 target/windows-znver5/x86_64-pc-windows-gnu/release/benchmark.exe initial prose 256 101
target/windows-portable/x86_64-pc-windows-gnu/release/benchmark.exe ba02 windows-memory-run 0 prose 256 31
target/windows-znver5/x86_64-pc-windows-gnu/release/benchmark.exe ba02 windows-memory-run 0 prose 256 31

nm -C target/windows-znver5/x86_64-pc-windows-gnu/release/benchmark.exe
objdump -Cd target/windows-znver5/x86_64-pc-windows-gnu/release/benchmark.exe
```

For a full build comparison, the `windows-build-runtime` command is repeated for both implementations and all three datasets.

Raw data: [environment](results/ba02-windows-environment.txt), [portable runtime](results/ba02-portable-native-windows.jsonl), [`znver5` runtime](results/ba02-znver5-native-windows.jsonl), [paired build comparison](results/ba02-build-comparison-native-windows.jsonl), [portable memory](results/ba02-portable-memory-native-windows.jsonl), [`znver5` memory](results/ba02-znver5-memory-native-windows.jsonl).
