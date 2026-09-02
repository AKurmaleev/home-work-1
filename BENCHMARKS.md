# Сравнение исходных и оптимизированных решений

Первый раздел ниже сохраняет исходный общий benchmark как исторический результат. Строгий opt-in benchmark для AMD Zen 5 с большими данными, raw samples, оценкой разброса и отдельным измерением peak RSS приведён в разделе [Zen 5: `ba02` на больших данных](#zen-5-ba02-на-больших-данных).

## Запуск

Из корня репозитория:

```sh
CARGO_INCREMENTAL=0 cargo run --release -p benchmark
```

Benchmark использует только стандартную библиотеку Rust и внутренние path-зависимости на задания. Сторонние пакеты не добавлены.

## Методика

- Профиль сборки: `release` с оптимизациями Cargo по умолчанию.
- Перед измерением проверяется равенство результатов исходной и оптимизированной реализаций.
- Для каждой реализации выполняется один прогрев.
- Результат — медиана пяти измерений через `std::time::Instant`.
- Результаты вычислений передаются в `std::hint::black_box`, чтобы компилятор не удалил полезную работу.
- Генерация наборов данных не входит во время измерения.
- Для сортировки клонирование исходного массива выполняется до запуска таймера.
- Для потоковых заданий обе реализации получают одинаковый `BufReader` ёмкостью 64 KiB поверх одного набора данных.
- Для сложения `u8` обе реализации вызываются через одинаковый тип указателя на функцию.

Наборы данных создаются детерминированно, без `rand`:

- `ba01`: поток размером 64 MiB;
- `ba02`: текстовый поток размером 64 MiB;
- `ba03`: 20 000 уникальных псевдослучайно упорядоченных строк;
- `ba04`: все 65 536 пар значений `u8`, повторённые 256 раз — 16 777 216 операций на одно измерение.

## Окружение

```text
rustc 1.95.0 (59807616e 2026-04-14)
cargo 1.95.0 (f2d3ce0bd 2026-03-21)
host: x86_64-pc-windows-gnu
LLVM 22.1.2
OS: Windows 11, build 26100, MSYS2 MINGW64 3.6.3
CPU identifier: AMD64 Family 26 Model 68 Stepping 0, AuthenticAMD
```

## Результаты

```text
Release benchmark: 1 warm-up + median of 5 samples
All implementations produced matching outputs before timing.

case   | dataset                  | initial median |      optimized | initial/optimized
----------------------------------------------------------------------------------------
ba01   | 64 MiB byte stream       |      10.514 ms |       2.980 ms |             3.53x
ba02   | 64 MiB text stream       |      38.585 ms |      26.569 ms |             1.45x
ba03   | 20,000 strings           |     395.595 ms |       1.165 ms |           339.45x
ba04   | 65,536 pairs x 256       |       8.807 ms |       8.725 ms |             1.01x
```

Столбец `initial/optimized` показывает отношение времени исходной реализации ко времени оптимизированной. Значение больше `1` означает, что оптимизированный вариант быстрее на этом запуске.

## Почему изменились результаты

### `ba01`

Исходная функция `count_bytes_initial` полностью копирует вход в `Vec`, хотя для ответа нужна только длина. Оптимизированная функция передаёт поток в `io::sink()` через `io::copy`, не сохраняя весь ввод.

- исходная дополнительная память: `O(n)`;
- оптимизированная дополнительная память: `O(1)`, ограниченный внутренний буфер;
- измеренное отношение времени: `3.53x` в пользу оптимизированной версии.

### `ba02`

Исходная функция `count_lines_words_bytes_initial` сначала сохраняет весь вход в `Vec`, затем отдельно считает строки и слова. Оптимизированная функция обрабатывает каждый блок `BufRead` один раз и переносит состояние текущего слова между границами блоков.

- исходная дополнительная память: `O(n)`;
- оптимизированная дополнительная память: `O(1)`;
- исходная версия выполняет два прохода после чтения, оптимизированная — один потоковый проход;
- измеренное отношение времени: `1.45x` в пользу оптимизированной версии.

### `ba03`

Исходная `sort_arguments_initial` реализует сортировку вставками. На неупорядоченных данных она требует `O(n²)` сравнений и перемещений. Оптимизированная версия использует `slice::sort_unstable` из стандартной библиотеки, рассчитанную на эффективную сортировку больших массивов без требования сохранять порядок равных элементов.

- исходная временная сложность на типичных неупорядоченных данных: `O(n²)`;
- оптимизированная временная сложность: `O(n log n)` в худшем случае;
- обе версии сортируют на месте; `sort_unstable` не выделяет вспомогательный массив в куче;
- измеренное отношение времени: `339.45x` в пользу оптимизированной версии.

Такое большое отношение относится к конкретному размеру и распределению данных. Оно не является универсальной константой: разница растёт вместе с размером входа из-за различной алгоритмической сложности.

### `ba04`

Исходная `add_u8_wrapping_initial` расширяет операнды до `u16`, складывает их и сужает результат. Оптимизированная версия остаётся в диапазоне `u8` и использует проверенную ветвлением арифметику без потенциально усекающего `as`.

- обе версии имеют время и память `O(1)`;
- измеренное отношение `1.01x` слишком близко к единице, чтобы считать его значимым ускорением;
- основная ценность изменения — более явное доказательство корректности диапазонов, а не производительность.

## Ограничения интерпретации

- Время зависит от процессора, версии компилятора, фоновой нагрузки и состояния системы.
- Это локальный microbenchmark, а не прогноз производительности всего приложения.
- В этом историческом запуске пиковая память не измерялась инструментально. Новый `ba02` benchmark ниже измеряет Linux peak RSS отдельными процессами.
- Для повторяемого сравнения следует запускать одну и ту же release-сборку несколько раз на ненагруженной машине.

## Zen 5: `ba02` на больших данных

### Цель и гипотеза

Сравниваются две уже существующие корректные реализации:

- `count_lines_words_bytes_initial`: читает весь поток в `Vec`, затем сканирует сохранённые байты;
- `count_lines_words_bytes`: обрабатывает `BufRead` блоками по 64 KiB и хранит только счётчики и состояние текущего слова.

Гипотеза: потоковая версия должна уменьшить удерживаемое представление входа с `O(n)` до `O(1)` и ускорить предсказуемые текстовые потоки за счёт одного прохода. Ускорение на данных с трудно предсказываемыми переходами whitespace/non-whitespace заранее не предполагается.

Zen 5 policy применена вручную и только к этому benchmark. Portable workspace baseline не изменён; `znver5` собирается в отдельный target directory.

### CLI

```sh
# Строгий runtime benchmark всех трёх распределений.
cargo run --release -p benchmark -- ba02 runtime 256 31

# Один вариант в отдельном процессе — для peak RSS.
cargo run --release -p benchmark -- ba02 once initial prose 256
cargo run --release -p benchmark -- ba02 once optimized prose 256
```

`runtime` печатает JSONL: конфигурацию, fingerprints, все пары сырых измерений и итоговую статистику. `once` генерирует поток без resident source `Vec`, поэтому peak RSS отражает различие самих алгоритмов, а не хранение benchmark-набора.

### Окружение измерения

Дата: 2026-09-02.

```text
CPU: AMD Ryzen 9 9950X3D 16-Core Processor
Vendor/family/model/stepping: AuthenticAMD / 26 / 68 / 0
Environment: Ubuntu 24.04 under WSL2
Kernel: 6.6.87.2-microsoft-standard-WSL2
Visible topology: 16 logical CPUs, 8 cores, 2 threads/core
Pinned logical CPU: 2; reported SMT sibling: 3
rustc: 1.98.0 (88d9e12ae 2026-08-18)
LLVM: 22.1.8
Target: x86_64-unknown-linux-gnu
Profile: Cargo release defaults; no explicit LTO
Zen 5 flags: RUSTFLAGS="-C target-cpu=znver5"
AMD source: Software Optimization Guide for the AMD Zen5
            Microarchitecture, Publication 58455, Revision 1.00
Hardware counters: unavailable (`perf` absent)
```

Важно: ОС определила процессор как **9950X3D**, а не 9950X. WSL показывает только 8 cores × 2 threads из 16 доступных vCPU и не даёт надёжно контролировать CCD/V-Cache placement, host power state или планирование SMT sibling. Worker закреплён через `taskset -c 2`, но простой CPU 3 не мог быть гарантирован. Load average вырос примерно с `3.95` до `5.98` во время последовательных прогонов. Поэтому результаты относятся к WSL2 и не являются bare-metal доказательством предельной микроархитектурной производительности.

### Сборка и протокол

```sh
CARGO_INCREMENTAL=0 cargo build --release -p benchmark \
  --target-dir /tmp/hw1-portable

CARGO_INCREMENTAL=0 RUSTFLAGS="-C target-cpu=znver5" \
  cargo build --release -p benchmark \
  --target-dir /tmp/hw1-znver5

# Основной target-specific прогон.
taskset -c 2 /tmp/hw1-znver5/release/benchmark \
  ba02 runtime 256 31
```

Для каждого распределения:

- вход: 256 MiB (`268435456` bytes), создан до таймера;
- независимый reference counter и обе реализации должны дать одинаковый `Counts`;
- 3 непротоколируемые warm-up пары;
- 31 измеряемая пара;
- порядок чередуется `initial → optimized`, затем `optimized → initial`;
- outliers не удаляются;
- экспериментальная единица — одна парная обработка одного и того же resident набора;
- point estimate — `exp(median(ln(initial / optimized)))`;
- dispersion — MAD каждой реализации;
- 95% CI — deterministic paired percentile bootstrap, 10 000 resamples;
- практический порог — нижняя граница CI не меньше `1.05`.

Распределения:

- `prose`: повторяемый обычный ASCII-текст;
- `boundary`: слова пересекают границы 64 KiB `BufReader`;
- `mixed`: fixed-seed LCG создаёт менее предсказуемые переходы whitespace/non-whitespace.

### Runtime: `znver5`

| Dataset | Initial median ± MAD | Optimized median ± MAD | Paired speedup | 95% CI | Вывод |
|---|---:|---:|---:|---:|---|
| `prose` | 111.542 ± 2.916 ms | 69.672 ± 0.876 ms | 1.592× | [1.581, 1.622] | ускорение подтверждено |
| `boundary` | 121.520 ± 4.942 ms | 70.713 ± 2.222 ms | 1.721× | [1.688, 1.736] | ускорение подтверждено |
| `mixed` | 747.219 ± 46.897 ms | 834.247 ± 34.173 ms | 0.903× | [0.894, 0.939] | регрессия подтверждена |

На `prose` потоковая версия уменьшила медианное время примерно на **37.2%**, на `boundary` — на **41.9%**. На branch-stress `mixed` она оказалась примерно на **11.6% медленнее** относительно initial time (`optimized / initial - 1`). Следовательно, универсальное утверждение «optimized быстрее» неверно: выигрыш зависит от распределения данных.

Portable artifact также измерен тем же протоколом как sanity baseline:

| Dataset | Portable paired speedup | 95% CI | Вывод |
|---|---:|---:|---|
| `prose` | 1.903× | [1.757, 1.948] | ускорение |
| `boundary` | 1.828× | [1.777, 1.917] | ускорение |
| `mixed` | 0.994× | [0.972, 1.012] | неубедительно |

Portable и `znver5` artifacts запускались последовательно при изменившейся фоновой нагрузке. Эти таблицы нельзя использовать как чистое доказательство эффекта `target-cpu=znver5`; основной вывод — paired initial/optimized внутри одного artifact.

### Память

Peak RSS измерялся отдельно от runtime: 10 новых процессов на вариант, 256 MiB `prose`, target `znver5`, `taskset -c 2`, GNU `/usr/bin/time -f "%M"`.

```sh
/usr/bin/time -f "%M" taskset -c 2 \
  /tmp/hw1-znver5/release/benchmark ba02 once initial prose 256

/usr/bin/time -f "%M" taskset -c 2 \
  /tmp/hw1-znver5/release/benchmark ba02 once optimized prose 256
```

| Variant | Peak RSS median | Range | Относительно initial |
|---|---:|---:|---:|
| `initial` | 526656 KiB (514.31 MiB) | 526592–526720 KiB | baseline |
| `optimized` | 2688 KiB (2.63 MiB) | 2560–2688 KiB | −99.49% |

Медианный peak RSS уменьшился на **523968 KiB (511.69 MiB)**, примерно в **195.9 раза**. Это process-level Linux peak RSS в WSL2, не Windows host memory всей WSL VM и не allocator-specific heap usage.

Metric-specific representation evidence:

- initial удерживает полный `Vec` с payload 256 MiB; фактическая capacity может быть выше из-за роста `read_to_end`;
- optimized не хранит полный вход и работает через 64 KiB reader buffer;
- дополнительное удерживаемое представление входа меняется с `O(n)` на `O(1)`.

Allocation count и суммарные allocated bytes не заявляются: allocator profiler не использовался. Уменьшение peak RSS и representation size не переименовывается в непроверенный вывод об allocation count.

### Дизассемблер и Zen 5 вывод

Проверен финальный `znver5` ELF через `nm -C` и `objdump -d -C`. LLVM развернул byte loop блоками по 8 и использовал VEX zeroing idiom (`vxorps`), но не создал AVX-512 vector loop для whitespace state machine. Горячий участок остаётся branch-heavy (`cmp`, `bt`, `jcc`), что согласуется с регрессией на `mixed`.

Поэтому отдельная handwritten AVX-512 реализация для `ba02` не добавлена: механизм не подтверждён для stateful parser, а существующая safe Rust версия уже даёт главный выигрыш через streaming representation. Retained optimization — алгоритмический one-pass/`O(1)` memory path плюс изолированная `znver5` сборка для измерения, без изменения portable baseline.

### Raw data

- [`benchmark/results/ba02-znver5-wsl2.jsonl`](benchmark/results/ba02-znver5-wsl2.jsonl) — target-specific config, fingerprints, 93 paired samples и summaries;
- [`benchmark/results/ba02-portable-wsl2.jsonl`](benchmark/results/ba02-portable-wsl2.jsonl) — portable sanity baseline;
- [`benchmark/results/ba02-initial-peak-rss-kib.txt`](benchmark/results/ba02-initial-peak-rss-kib.txt) — 10 initial peak RSS samples;
- [`benchmark/results/ba02-optimized-peak-rss-kib.txt`](benchmark/results/ba02-optimized-peak-rss-kib.txt) — 10 optimized peak RSS samples.

### Итог

Гипотеза подтверждена частично:

- память: strong improvement — 256 MiB full-input retention устранён, median peak RSS ниже на 99.49%;
- runtime на обычном и boundary-тексте: statistically and practically significant improvement 1.59–1.72×;
- runtime на branch-stress данных: significant regression 0.903×;
- специфический AVX-512 код для parser отклонён как неподтверждённый кандидат;
- результаты требуют bare-metal повторения для строгих утверждений о самом Zen 5/9950X3D.
