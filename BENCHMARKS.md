# Сравнение исходных и оптимизированных решений

Разделы с пометкой **Legacy** сохраняют исходный общий benchmark только как исторический результат. Он не соответствует строгому протоколу Rule 63 и не используется для актуальных выводов. Новый воспроизводимый benchmark `ba02` для portable- и `znver5`-артефактов приведён ниже в разделе Rule 63.

## Legacy: запуск общего benchmark

Из корня репозитория:

```sh
CARGO_INCREMENTAL=0 cargo run --release -p benchmark
```

Benchmark использует только стандартную библиотеку Rust и внутренние path-зависимости на задания. Сторонние пакеты не добавлены.

## Legacy: методика общего benchmark

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

## Legacy: окружение общего benchmark

```text
rustc 1.95.0 (59807616e 2026-04-14)
cargo 1.95.0 (f2d3ce0bd 2026-03-21)
host: x86_64-pc-windows-gnu
LLVM 22.1.2
OS: Windows 11, build 26100, MSYS2 MINGW64 3.6.3
CPU identifier: AMD64 Family 26 Model 68 Stepping 0, AuthenticAMD
```

## Legacy: результаты общего benchmark

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

## Legacy: почему изменились результаты

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

## Legacy: ограничения интерпретации

- Время зависит от процессора, версии компилятора, фоновой нагрузки и состояния системы.
- Это локальный microbenchmark, а не прогноз производительности всего приложения.
- В этом историческом запуске пиковая память не измерялась инструментально. Новый `ba02` benchmark ниже измеряет Linux peak RSS отдельными процессами.
- Для повторяемого сравнения следует запускать одну и ту же release-сборку несколько раз на ненагруженной машине.

## Rule 63: `ba02` на больших данных — portable и `znver5`

### Область измерения

Сравниваются две существующие реализации с одинаковым результатом:

- `count_lines_words_bytes_initial` читает вход целиком в `Vec`, затем анализирует сохранённые байты;
- `count_lines_words_bytes` обрабатывает `BufRead` блоками по 64 KiB и переносит состояние текущего слова между блоками.

Строгий benchmark измеряет только `ba02` на трёх детерминированных наборах по 256 MiB: `prose`, `boundary` и `mixed`. Portable baseline репозитория не изменён. `znver5` — отдельный opt-in артефакт.

### Сборка и идентификация артефактов

Команды выполняются из корня репозитория в WSL2:

```sh
CARGO_INCREMENTAL=0 cargo build --frozen --release -p benchmark --target x86_64-unknown-linux-gnu --target-dir /tmp/hw1-rule63-portable

CARGO_INCREMENTAL=0 RUSTFLAGS="-C target-cpu=znver5" cargo build --frozen --release -p benchmark --target x86_64-unknown-linux-gnu --target-dir /tmp/hw1-rule63-znver5
```

Команда portable-сборки не задаёт `RUSTFLAGS`; `znver5`-сборка явно задаёт только `-C target-cpu=znver5`. Обе используют `--release`, зафиксированный `Cargo.lock` через `--frozen` и явный target `x86_64-unknown-linux-gnu`. В корневом `Cargo.toml` нет переопределения `[profile.release]`; LTO отдельно не включался.

| Артефакт | Путь измеренного executable | SHA256 |
|---|---|---|
| portable | `/tmp/hw1-rule63-portable/x86_64-unknown-linux-gnu/release/benchmark` | `bf7a6b2bd099b4afa95e705fb1feac3741859f1e1bc4a5ebada46fedd8f6d49d` |
| `znver5` | `/tmp/hw1-rule63-znver5/x86_64-unknown-linux-gnu/release/benchmark` | `bd1a505823629988703b78502de4f3705ae7a4da4b43bac52ca53c0130e0b0ef` |

Проверка идентичности измеряемых файлов:

```sh
sha256sum \
  /tmp/hw1-rule63-portable/x86_64-unknown-linux-gnu/release/benchmark \
  /tmp/hw1-rule63-znver5/x86_64-unknown-linux-gnu/release/benchmark
```

### Окружение

Дата измерения: **2026-09-03**.

```text
CPU: AMD Ryzen 9 9950X3D 16-Core Processor
Vendor: AuthenticAMD
Family: 26 (0x1A)
Model: 68 (0x44)
Environment: WSL2, Ubuntu 24.04
Kernel: 6.6.87.2-microsoft-standard-WSL2
Visible topology: 16 logical CPUs, 8 physical cores, 2 SMT threads/core
Pinned logical CPU: 6
Reported SMT sibling: CPU 7
rustc: 1.98.0
cargo: 1.98.0
LLVM: 22.1.8
Target: x86_64-unknown-linux-gnu
Profile: Cargo release defaults; no [profile.release] override in root Cargo.toml
```

CPU 6 выбран вместо CPU 2, где кратковременно наблюдался процесс `k9s` примерно с 0.6% CPU. Моментальная нагрузка от других процессов в контрольных срезах была практически нулевой. Load average около 3–4 отражал исторический хвост активности WSL, сборок и benchmark-прогонов, а не гарантированную одновременную нагрузку на закреплённый CPU.

WSL2 не позволяет гарантировать host power state, размещение на CCD/V-Cache и простой SMT sibling CPU 7. Поэтому результаты относятся к указанному WSL2-окружению; bare-metal вывод не делается.

### Runtime: команды и наборы данных

```sh
taskset -c 6 \
  /tmp/hw1-rule63-portable/x86_64-unknown-linux-gnu/release/benchmark \
  ba02 runtime 256 101 \
  > benchmark/results/ba02-portable-wsl2-rule63.jsonl

taskset -c 6 \
  /tmp/hw1-rule63-znver5/x86_64-unknown-linux-gnu/release/benchmark \
  ba02 runtime 256 101 \
  > benchmark/results/ba02-znver5-wsl2-rule63.jsonl
```

Каждый вход имеет ровно `268435456` bytes и создаётся до измеряемого участка:

- `prose` — повторяемый обычный ASCII-текст;
- `boundary` — слова пересекают границы 64 KiB `BufReader`;
- `mixed` — fixed-seed LCG создаёт менее предсказуемые переходы whitespace/non-whitespace.

До timing независимый reference counter и обе реализации проверяются на одинаковые `Counts`; fingerprints и checksums записываются в JSONL.

### Runtime: заранее заданный анализ Rule 63

Для каждого dataset применяется один и тот же протокол:

1. Экспериментальная единица — полная пара `initial`/`optimized` на одном resident input.
2. Выполняются 3 нетаймируемые warm-up пары; они исключены из статистики.
3. Измеряются `N = 101` полных пар. Порядок строго чередуется AB/BA: `initial → optimized`, затем `optimized → initial`.
4. Пары сортируются по `ln(initial / optimized)`; deterministic tie-break — ID пары по возрастанию.
5. Тримируются целые пары: `t = floor(101 × 0.15) = 15` с каждого хвоста; сохраняется `101 - 2 × 15 = 71` пара. Это заранее заданный robust estimator, а не удаление ошибочных наблюдений.
6. Один и тот же набор из 71 retained pairs используется для обеих реализаций, всех эффектов и confidence interval. Marginal distributions отдельно не тримируются.
7. Для каждой реализации считаются trimmed arithmetic mean, retained median и retained MAD.
8. Сравнительные оценки: отношение trimmed means `initial_mean / optimized_mean` и paired median speedup `exp(median(ln(initial / optimized)))` на retained pairs.
9. 95% CI относится к paired median speedup: deterministic paired percentile bootstrap, 10 000 resamples только из retained pairs, без повторного trimming внутри bootstrap. Анализатор использует фиксированный seed `0x8b8b_8b8b_02a5_2024`.
10. Заранее заданный минимальный практически значимый выигрыш — 5%; verdict `improved` требует `CI_low >= 1.05`.
11. Критерий достаточности: относительная полная ширина CI `(CI_high - CI_low) / paired_median_speedup <= 0.10`. Все шесть анализов проходят критерий.

Raw analyzer выдаёт `regressed`, когда критерий достаточности выполнен и `CI_high < 1.0`. В обоих `mixed`-прогонах весь CI также ниже `1 / 1.05 ≈ 0.952381`, то есть наблюдаемая регрессия пересекает и симметричную границу эффекта 5%.

### Runtime: portable

Времена приведены в миллисекундах. Среднее — arithmetic mean после парного trimming; median и MAD рассчитаны на том же retained set.

| Dataset | Initial mean | Initial median ± MAD | Optimized mean | Optimized median ± MAD |
|---|---:|---:|---:|---:|
| `prose` | 161.740 | 161.650 ± 1.965 | 84.155 | 84.038 ± 0.718 |
| `boundary` | 166.957 | 166.338 ± 2.418 | 82.881 | 82.681 ± 0.503 |
| `mixed` | 797.412 | 795.862 ± 3.847 | 838.978 | 836.667 ± 2.234 |

| Dataset | Ratio of trimmed means | Paired median speedup | 95% CI | Relative total CI width | Verdict |
|---|---:|---:|---:|---:|---|
| `prose` | 1.921941× | 1.93183× | [1.92088, 1.93665] | 0.008163 | `improved` |
| `boundary` | 2.014428× | 2.01512× | [2.00466, 2.02633] | 0.010750 | `improved` |
| `mixed` | 0.950457× | 0.94983× | [0.94807, 0.95188] | 0.004016 | `regressed` |

### Runtime: `znver5`

| Dataset | Initial mean | Initial median ± MAD | Optimized mean | Optimized median ± MAD |
|---|---:|---:|---:|---:|
| `prose` | 106.826 | 106.679 ± 0.541 | 76.489 | 76.386 ± 0.211 |
| `boundary` | 117.665 | 117.253 ± 0.992 | 80.819 | 80.621 ± 0.435 |
| `mixed` | 753.220 | 751.893 ± 14.023 | 822.726 | 819.739 ± 2.147 |

| Dataset | Ratio of trimmed means | Paired median speedup | 95% CI | Relative total CI width | Verdict |
|---|---:|---:|---:|---:|---|
| `prose` | 1.396623× | 1.39466× | [1.39358, 1.39777] | 0.003004 | `improved` |
| `boundary` | 1.455910× | 1.45479× | [1.44830, 1.46199] | 0.009409 | `improved` |
| `mixed` | 0.915517× | 0.91156× | [0.90561, 0.91958] | 0.015328 | `regressed` |

Значение больше `1` означает преимущество optimized, меньше `1` — преимущество initial. На обоих артефактах optimized улучшает `prose` и `boundary`, но регрессирует на `mixed`.

Portable- и `znver5`-прогоны выполнялись последовательно, а не как единый перемежающийся эксперимент. Поэтому различия между таблицами нельзя причинно приписывать `target-cpu=znver5`.

### Peak RSS: post-predeclaration unpaired rerun

Peak RSS измерялся отдельно от runtime: dataset `prose`, 256 MiB, CPU 6, GNU `/usr/bin/time -f "%M"`. Свежая выборка получена неизменённым исходным `znver5` measurement artifact `/tmp/hw1-rule63-znver5/x86_64-unknown-linux-gnu/release/benchmark` с SHA256 `bd1a505823629988703b78502de4f3705ae7a4da4b43bac52ca53c0130e0b0ef`. Для каждого варианта выполнен один отдельный warm-up process, исключённый из raw data, затем 31 новый последовательный fresh process.

Перед свежим rerun snapshot показывал около 95% idle CPU. Исторический load average оставался около 4.9. Как и для runtime, WSL2 не гарантирует host power state, CCD/V-Cache placement или простой SMT sibling CPU 7; bare-metal вывод не делается.

Воспроизведение свежих raw-файлов:

```sh
for variant in initial optimized; do
  output="benchmark/results/ba02-${variant}-peak-rss-kib-rule63.txt"

  # Один исключённый warm-up process.
  /usr/bin/time -f "%M" -o /dev/null \
    taskset -c 6 \
    /tmp/hw1-rule63-znver5/x86_64-unknown-linux-gnu/release/benchmark \
    ba02 once "${variant}" prose 256 \
    > /dev/null

  : > "${output}"
  for id in $(seq 1 31); do
    # Номер строки в output — 1-based ID наблюдения.
    /usr/bin/time -f "%M" -a -o "${output}" \
      taskset -c 6 \
      /tmp/hw1-rule63-znver5/x86_64-unknown-linux-gnu/release/benchmark \
      ba02 once "${variant}" prose 256 \
      > /dev/null
  done
done
```

Анализ выполнен отдельно текущим analysis-only artifact `/tmp/hw1-rule63-analysis/x86_64-unknown-linux-gnu/release/benchmark` с SHA256 `b203fd2bf876bd2a0b454a82f040c19611909d94e42fcf5097eb9671ae935122`. Он анализирует сохранённые наблюдения и не использовался для их измерения:

```sh
/tmp/hw1-rule63-analysis/x86_64-unknown-linux-gnu/release/benchmark \
  ba02 rss-summary \
  benchmark/results/ba02-initial-peak-rss-kib-rule63.txt \
  benchmark/results/ba02-optimized-peak-rss-kib-rule63.txt \
  > benchmark/results/ba02-rss-analysis-rule63.jsonl
```

До fresh rerun в коде анализатора был зафиксирован следующий протокол:

1. Независимая экспериментальная единица — peak RSS одного fresh process одной реализации.
2. Каждая реализация тримируется независимо. Сортировка: RSS по возрастанию; ties разрешаются по 1-based sample ID, затем по acquisition index.
3. Для каждой реализации `N = 31`, `t = floor(31 × 0.15) = 4` с каждого хвоста, retained `23`.
4. Основной эффект — relative reduction `1 - optimized_trimmed_mean / initial_trimmed_mean`.
5. 95% CI — deterministic independent percentile bootstrap: 10 000 resamples только из двух retained sets, без повторного trimming; фиксированный seed `15111065706836454659`.
6. Практический порог снижения — 5%; equivalence margin — ±1%.
7. Критерий достаточности — абсолютная полная ширина CI `<= 0.005`, то есть не более 0.5 percentage point.

| Variant | Low IDs | High IDs | Retained |
|---|---|---|---:|
| `initial` | 2, 3, 4, 5 | 26, 27, 30, 31 | 23 |
| `optimized` | 6, 8, 10, 11 | 25, 27, 28, 29 | 23 |

Остальные ID образуют retained set соответствующего варианта. 1-based номер строки raw-файла является sample ID; acquisition index дополнительно обеспечивает детерминированность tie-break.

| Variant | Trimmed arithmetic mean, KiB | Retained median ± MAD, KiB |
|---|---:|---:|
| `initial` | 526669.913043 | 526720 ± 0 |
| `optimized` | 2637.913043 | 2688 ± 0 |

Отношение trimmed means `initial / optimized` равно `199.654008439×`. Основной эффект, relative reduction process peak RSS, равен `0.994991335`, или `99.4991335%`; deterministic bootstrap 95% CI — `[0.994948908, 0.995044065]`, или `[99.4948908%, 99.5044065%]`. Абсолютная полная ширина CI равна `0.000095157`, то есть `0.0095157` percentage point: критерий `<= 0.005` выполнен, verdict — `improved`.

Описательное отношение retained medians равно `195.952381×`; соответствующее снижение — `99.4896719%`. Median и MAD не являются основным bootstrap effect.

`%M` — process peak RSS Linux в KiB. Это не heap usage, не allocation count, не суммарные allocated bytes и не память всей WSL VM на Windows host. Allocation count и allocated bytes не измерялись, поэтому выводов по ним нет.

### Сложность и границы выводов

- Обе реализации имеют временную сложность `Θ(n)`.
- Initial удерживает вспомогательное представление входа `Θ(n)`.
- Optimized использует вспомогательный буфер `Θ(B)`, где фиксированный `B = 64 KiB`; относительно размера входа это `Θ(1)`.
- Runtime, peak RSS и асимптотика представления — разные виды evidence; результат одного не обобщается на остальные метрики.
- Нет причинного сравнения portable против `znver5`, bare-metal вывода или неподтверждённого общего утверждения об «уменьшении памяти».

### Raw data и trim-маркеры

- [`benchmark/results/ba02-portable-wsl2-rule63.jsonl`](benchmark/results/ba02-portable-wsl2-rule63.jsonl) — portable runtime;
- [`benchmark/results/ba02-znver5-wsl2-rule63.jsonl`](benchmark/results/ba02-znver5-wsl2-rule63.jsonl) — `znver5` runtime;
- [`benchmark/results/ba02-initial-peak-rss-kib-rule63.txt`](benchmark/results/ba02-initial-peak-rss-kib-rule63.txt) — 31 fresh initial peak RSS observations;
- [`benchmark/results/ba02-optimized-peak-rss-kib-rule63.txt`](benchmark/results/ba02-optimized-peak-rss-kib-rule63.txt) — 31 fresh optimized peak RSS observations;
- [`benchmark/results/ba02-rss-analysis-rule63.jsonl`](benchmark/results/ba02-rss-analysis-rule63.jsonl) — 2 строки: predeclared analysis config и fresh RSS summary.

Каждый runtime JSONL содержит 310 строк: 1 config, 3 dataset records, 303 measured sample records и 3 summaries. Во всех трёх datasets суммарно отмечены 45 `low`, 213 `retained` и 45 `high`; каждая sample сохраняет ID пары, AB/BA order, оба времени, отношение и trim marker `selection`. Warm-up count записан в config, но warm-up пары нетаймируемые и не входят в measured sample records.

RSS-файлы сохраняют все 31 измеренное значение на вариант; 1-based номер строки является sample ID. Независимые trim sets перечислены выше. Analysis JSONL сохраняет полный predeclared policy, CI parameters, adequacy result и summary. Устаревшие raw-файлы без суффикса `-rule63` не используются ни в одной актуальной таблице или строгом выводе.

### Итог

- Portable: `prose` и `boundary` имеют practically significant `improved`; `mixed` — подтверждённая регрессия.
- `znver5`: `prose` и `boundary` имеют practically significant `improved`; `mixed` — подтверждённая регрессия.
- Все шесть runtime CI проходят заранее заданный критерий относительной полной ширины `<= 0.10`.
- В отдельном post-predeclaration `znver5`/`prose` эксперименте process peak RSS имеет relative reduction `99.4991335%` с 95% CI `[99.4948908%, 99.5044065%]`; adequacy выполнен, verdict `improved`. Это строго metric-specific результат.
- Результаты не доказывают преимущество `znver5` над portable и не переносятся на bare metal без отдельного контролируемого измерения.
