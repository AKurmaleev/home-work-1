# Homework 1 — Rust

A workspace containing solutions for the first Rust homework assignment. The main exercises are in the `hello` and `ba01`–`ba04` packages; `bonus` and `benchmark` contain additional materials.

## Quick start

```sh
cargo run -p hello
printf 'hello rust\n' | cargo run -p ba01
printf 'hello rust\n' | cargo run -p ba02
cargo run -p ba03 -- zebra apple rust
cargo test -p ba04
cargo run -p bonus
```

Check the entire workspace:

```sh
cargo fmt --all --check
cargo check --locked --workspace --all-targets
cargo clippy --locked --workspace --all-targets -- -D warnings -D unused_must_use -D clippy::let_underscore_must_use -D clippy::unused_result_ok
cargo test --locked --workspace
```

The library packages declare Rust 1.85, the edition 2024 baseline. All homework
packages (`hello`, `ba01`–`ba04`, and `bonus`), including their tests and doctests,
were verified on `1.85.0-x86_64-pc-windows-gnu`:

```sh
cargo +1.85.0-x86_64-pc-windows-gnu check --locked --workspace --exclude benchmark --all-targets
cargo +1.85.0-x86_64-pc-windows-gnu test --locked --workspace --exclude benchmark
```

The additional benchmark is not compatible with Rust 1.85: pre-existing code uses
`usize::is_multiple_of`, `avxvnni` feature detection, and `__cpuid` calls whose
safe-call form is unavailable on that compiler. Its source was not rewritten to
force compatibility. Full-workspace checks and strict lint gates were verified
with Rust/Cargo 1.99.1 on `x86_64-pc-windows-gnu`; the benchmark's earliest
compatible compiler has not been established.
The workspace has no optional Cargo features. `Cargo.lock` is tracked for
reproducible contributor resolution, including the library packages.

## Exercise navigation

| Section | What was implemented | Code |
|---|---|---|
| `hello` | A minimal `Hello, World!` program | [`main.rs`](hello/src/main.rs) |
| `ba01` | Streaming byte counting from standard input, similar to `wc -c` | [`lib.rs`](ba01/src/lib.rs), [`main.rs`](ba01/src/main.rs) |
| `ba02` | Line, word, and byte counting; both initial and streaming implementations are retained | [`lib.rs`](ba02/src/lib.rs), [`main.rs`](ba02/src/main.rs) |
| `ba03` | Lexicographic sorting of command-line arguments | [`lib.rs`](ba03/src/lib.rs), [`main.rs`](ba03/src/main.rs) |
| `ba04` | Checked, wrapping, and saturating addition for `u8`, including exhaustive testing of input pairs | [`lib.rs`](ba04/src/lib.rs) |
| `bonus` | Encoding, displaying, and inverting a monochrome 8×8 bitmap | [`lib.rs`](bonus/src/lib.rs), [`main.rs`](bonus/src/main.rs) |

Workspace members: [`Cargo.toml`](Cargo.toml).

## Input and API contracts

- `ba01` and `ba02` read arbitrary bytes through EOF; no UTF-8 decoding is required.
- `ba02` counts LF bytes as lines, including no extra line for an unterminated
  suffix. Word separators follow `u8::is_ascii_whitespace`: tab, LF, form feed,
  CR, and space; vertical tab and non-ASCII whitespace bytes are not separators.
- `ba03` sorts UTF-8 arguments case-sensitively, preserving empty arguments,
  punctuation, and duplicates. The executable name is excluded.
- `ba04` deliberately implements arithmetic manually because the assignment
  forbids built-in addition-mode helpers.
- `bonus::parse_bitmap_8x8` retains the assignment's infallible signature and
  panics on malformed rows; this is not a fallible parser for external input.
- The `*_initial` byte counters are retained for comparison, not unbounded input:
  they materialize the entire reader. Use the streaming variants for stdin.

## Additional `ba02` benchmark

The benchmark does not replace the main solution. It compares the initial and streaming variants of `ba02` on 256 MiB inputs, as well as portable and `znver5` builds.

- [Brief comparative report](BENCHMARKS.md)
- [Detailed native benchmark on Ryzen 9 9950X3D](benchmark/WINDOWS_NATIVE.md)
- [Benchmark source code](benchmark/src/ba02_large.rs)
- [Windows runner and CPU checks](benchmark/src/windows_native.rs)
- [Machine-readable results](benchmark/results/)
- [Native integration safety and cleanup contract](benchmark/SAFETY.md)

The bare `benchmark` command retains an older exploratory five-sample median
comparison. It does not implement the Rule 63 paired sampling, raw-observation,
trimming, and inference protocol; do not use it as comparative performance
evidence. The reports above use the dedicated `ba02` commands instead.
Historical raw measurements remain unchanged; this review did not run a new
benchmark campaign or revalidate historical speed, memory, or disassembly claims.

The main measured effect of the streaming version is a reduction from `Θ(n)` to `Θ(1)` auxiliary memory relative to input size. On native Windows, Peak Working Set decreased by approximately 99.1%. Performance depends on the data distribution: speedups were observed on `prose` and `boundary`, but a regression occurred on the synthetic `mixed` dataset.

## Major version history

The versions below mark major stages in the development of the assignment; no separate Git tags were created for them.

| Version | Date | Changes |
|---|---|---|
| **v1** | 2026-08-27 | First completed solutions for all main exercises and the bonus — [`e78d1a7`](https://github.com/AKurmaleev/home-work-1/commit/e78d1a7) |
| **v2** | 2026-08-27 | Optimized safe implementations, logic extracted into libraries, and expanded tests — [`272a5e4`](https://github.com/AKurmaleev/home-work-1/commit/272a5e4) |
| **v3** | 2026-09-02 — 2026-09-03 | Large benchmark datasets for `ba02`, repeated runtime and memory measurements, and statistical comparative analysis — [`90604d2`](https://github.com/AKurmaleev/home-work-1/commit/90604d2), [`ec79cb3`](https://github.com/AKurmaleev/home-work-1/commit/ec79cb3) |
| **v4** | 2026-09-07 | Native Windows comparison of portable and `znver5` builds on Ryzen 9 9950X3D, with a separate report and raw data — [`f257144`](https://github.com/AKurmaleev/home-work-1/commit/f257144) |
