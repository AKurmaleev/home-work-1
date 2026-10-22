# Additional benchmark for the `ba02` exercise

This benchmark supplements the homework without changing its main exercises. The goal is to compare two implementations of line, word, and byte counting on large inputs.

## Implementations compared

- `count_lines_words_bytes_initial` reads the entire input into a `Vec<u8>`, then processes the stored data separately.
- `count_lines_words_bytes` processes `BufRead` sequentially in 64 KiB blocks and carries word state across blocks.

Both implementations return the same result. Before measurements, the result is checked against an independent reference count.

## Datasets

Each variant uses a 256 MiB input:

- `prose` — ordinary repeated ASCII text;
- `boundary` — words crossing 64 KiB buffer boundaries;
- `mixed` — a mixture of whitespace and non-whitespace bytes with less predictable transitions.

## Methodology

- The build uses the `release` profile.
- Input generation is excluded from the measured time.
- Each series is preceded by 3 warm-up pairs of runs.
- Then 101 `initial`/`optimized` pairs are measured, alternating the order within pairs.
- 15% of pairs are excluded from each tail of the final sample, leaving 71 pairs.
- The mean and median are calculated for each implementation; the comparative result is calculated from the same retained pairs.
- Peak RSS is measured separately: one warm-up and 31 fresh processes per implementation. After excluding 15% of values from each tail, 23 observations remain.

Measurements were taken under WSL2 on a machine with an AMD Ryzen 9 9950X3D.

## Runtime

All times are in milliseconds. A speedup greater than `1×` indicates an advantage for the streaming implementation.

### Portable build

| Data | Initial mean / median | Optimized mean / median | Median speedup |
|---|---:|---:|---:|
| `prose` | 161.740 / 161.650 | 84.155 / 84.038 | 1.93× |
| `boundary` | 166.957 / 166.338 | 82.881 / 82.681 | 2.02× |
| `mixed` | 797.412 / 795.862 | 838.978 / 836.667 | 0.95× |

### Build with `target-cpu=znver5`

| Data | Initial mean / median | Optimized mean / median | Median speedup |
|---|---:|---:|---:|
| `prose` | 106.826 / 106.679 | 76.489 / 76.386 | 1.39× |
| `boundary` | 117.665 / 117.253 | 80.819 / 80.621 | 1.45× |
| `mixed` | 753.220 / 751.893 | 822.726 / 819.739 | 0.91× |

The streaming version is faster on ordinary text and on words crossing buffer boundaries. It is slower on the synthetic `mixed` dataset, so the optimization is not beneficial for every input distribution.

## Peak memory

Peak RSS was measured on a 256 MiB `prose` input. Values are in KiB.

| Implementation | Mean | Median |
|---|---:|---:|
| Initial | 526,669.9 | 526,720 |
| Optimized | 2,637.9 | 2,688 |

The reduction in mean peak RSS was approximately **99.50%**. This measures memory for the entire process, not just the function's heap allocations.

## Complexity

| Implementation | Time | Auxiliary memory |
|---|---|---|
| Initial | `Θ(n)` | `Θ(n)` to store the entire input |
| Optimized | `Θ(n)` | `Θ(1)` relative to input size: a fixed 64 KiB buffer |

The time complexity class did not change. The main gain came from streaming, avoiding storage of the entire input, and making fewer passes over ordinary text.

## Limitations

Measurements were taken under WSL2. This environment does not allow full control over CPU power state, CCD/V-Cache placement, or load on the neighboring SMT thread.

The portable and `znver5` builds were measured sequentially, not in a single shared paired experiment. Therefore, differences between these two tables cannot be treated as evidence of the effect of `target-cpu=znver5` or the Zen 5 microarchitecture. Only differences between `initial` and `optimized` within each individual series are confirmed.

## Conclusion

The streaming implementation substantially reduces peak RSS and speeds up ordinary text processing, but loses on the `mixed` distribution. A Zen 5-specific gain under WSL2 has not been confirmed.

## Bonus: native Windows

An additional paired comparison of portable and `znver5` builds was performed on a Ryzen 9 9950X3D under native Windows build 26100. The practical significance threshold of 5% was exceeded in five of six workloads; streaming / `mixed` achieved about 3.4%. Streaming also reduced Peak Working Set by approximately 99.1%, but retained the regression on `mixed`.

Details and raw data: [native Windows benchmark](benchmark/WINDOWS_NATIVE.md).

WSL2 raw data: [portable runtime](benchmark/results/ba02-portable-wsl2-rule63.jsonl), [`znver5` runtime](benchmark/results/ba02-znver5-wsl2-rule63.jsonl), [initial RSS](benchmark/results/ba02-initial-peak-rss-kib-rule63.txt), [optimized RSS](benchmark/results/ba02-optimized-peak-rss-kib-rule63.txt).
