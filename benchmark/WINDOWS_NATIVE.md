# Бонусный benchmark `ba02` на Ryzen 9 9950X3D

Цель — отдельно проверить две вещи на нативной Windows build 26100 (24H2):

1. эффект потоковой реализации из `ba02`;
2. эффект компиляции с `target-cpu=znver5` относительно portable-сборки.

Основное решение домашней работы не изменялось. Benchmark использует дополнительный код.

## Стенд и методика

- CPU: AMD Ryzen 9 9950X3D, 16 ядер / 32 логических процессора.
- ОС: Windows build 26100 (24H2), нативный процесс.
- Компилятор: Rust 1.95.0, LLVM 22.1.2, GNU target `x86_64-pc-windows-gnu`.
- Сборки: portable `target-cpu=x86-64` и отдельная `target-cpu=znver5`.
- Данные: `prose`, `boundary`, `mixed`, по 256 MiB.
- Время: 3 прогревочные пары, затем 101 пара с чередованием A/B и B/A.
- Отброшено по 15 пар с каждого края; сохранена 71 пара.
- Приведены усечённые среднее, медиана, MAD и парный 95% CI по сохранённым наблюдениям.
- Все серии прошли заранее выбранный критерий достаточной ширины CI.
- Поток закреплён за logical CPU 0.
- Peak Working Set измерен отдельно: свежий процесс на наблюдение, 1 прогрев и 31 наблюдение на реализацию; каждая реализация отсекалась независимо, осталось по 23 наблюдения.

Создание тестовых данных не входит в измеряемое время. Результаты обеих реализаций сверялись до измерений.

## Initial против streaming

Время указано в миллисекундах. Speedup больше `1×` означает преимущество streaming.

### Portable-сборка

| Данные | Initial mean / median, ms | Streaming mean / median, ms | MAD initial / streaming, ms | Median speedup, 95% CI |
|---|---:|---:|---:|---:|
| `prose` | 136.378 / 136.325 | 89.888 / 90.226 | 1.006 / 0.952 | 1.517× [1.508; 1.525] |
| `boundary` | 144.222 / 143.223 | 90.191 / 90.270 | 1.669 / 1.146 | 1.596× [1.580; 1.610] |
| `mixed` | 796.496 / 796.471 | 883.269 / 883.224 | 2.508 / 2.197 | 0.902× [0.901; 0.903] |

### `znver5`-сборка

| Данные | Initial mean / median, ms | Streaming mean / median, ms | MAD initial / streaming, ms | Median speedup, 95% CI |
|---|---:|---:|---:|---:|
| `prose` | 90.771 / 90.544 | 82.383 / 82.129 | 1.095 / 1.432 | 1.104× [1.096; 1.112] |
| `boundary` | 97.763 / 97.809 | 79.408 / 79.479 | 0.832 / 1.255 | 1.229× [1.223; 1.235] |
| `mixed` | 759.352 / 756.549 | 856.127 / 853.355 | 3.783 / 3.061 | 0.888× [0.886; 0.889] |

Streaming быстрее на обычном тексте и при пересечении границ буфера, но медленнее на искусственном `mixed`. Выигрыш зависит от протестированного распределения байтов; причина регрессии на `mixed` отдельно не изолирована.

## Portable против `znver5`

Для каждой строки две сборки измерялись в отдельной парной серии. Speedup больше `1×` означает преимущество `znver5`.

| Реализация / данные | Portable mean / median, ms | `znver5` mean / median, ms | MAD portable / `znver5`, ms | Median speedup, 95% CI |
|---|---:|---:|---:|---:|
| Initial / `prose` | 137.642 / 135.301 | 91.959 / 90.084 | 2.165 / 2.340 | 1.499× [1.479; 1.526] |
| Initial / `boundary` | 146.294 / 145.614 | 100.145 / 99.322 | 2.777 / 2.494 | 1.470× [1.447; 1.478] |
| Initial / `mixed` | 816.685 / 812.043 | 770.322 / 765.411 | 11.553 / 11.055 | 1.060× [1.056; 1.066] |
| Streaming / `prose` | 90.373 / 90.064 | 81.903 / 81.019 | 1.822 / 1.609 | 1.100× [1.091; 1.114] |
| Streaming / `boundary` | 93.533 / 91.917 | 80.692 / 79.875 | 2.451 / 1.639 | 1.152× [1.142; 1.167] |
| Streaming / `mixed` | 878.832 / 870.241 | 850.611 / 841.196 | 13.023 / 10.940 | 1.034× [1.030; 1.036] |

Заранее выбранный практически значимый порог — 5%. Он пройден в пяти случаях из шести. Для streaming / `mixed` измерен положительный эффект около 3.4%, но практический порог не достигнут.

## Память

Peak Working Set измерен на `prose` размером 256 MiB. Это пиковый резидентный working set процесса; метрика не равна общему virtual/committed memory и не измеряет heap allocations функции.

| Сборка | Initial mean / median, KiB | Streaming mean / median, KiB | MAD initial / streaming, KiB | Снижение среднего, 95% CI |
|---|---:|---:|---:|---:|
| Portable | 530,163.3 / 530,160 | 4,886.6 / 4,888 | 8 / 4 | 99.078% [99.078%; 99.079%] |
| `znver5` | 530,137.2 / 530,136 | 4,852.9 / 4,856 | 20 / 4 | 99.085% [99.084%; 99.085%] |

Количество и объём отдельных heap allocations не инструментировались. Алгоритмически initial требует `Θ(n)` дополнительной памяти для полного входа, streaming — `Θ(1)` относительно размера входа благодаря фиксированному буферу 64 KiB. Обе версии имеют временную сложность `Θ(n)`.

## Что изменил компилятор

Проверка финального дизассемблированного бинарника показала:

- portable initial считает переводы строк 128-битными SSE-инструкциями;
- `znver5` initial использует AVX-512 сравнения, маски и векторное суммирование;
- stateful цикл streaming остаётся скалярным, но разворачивается примерно с 2 до 8 байт на итерацию;
- более сильное разворачивание увеличивает отдельные hot-path участки кода.

Это объясняет возможный механизм ускорения, но не доказывает вклад каждой инструкции отдельно: аппаратные performance counters не собирались.

## Ограничения и вывод

Измерения относятся только к одной кампании на Ryzen 9 9950X3D под нативной Windows build 26100. Закрепление за CPU 0 не доказывает конкретное размещение по CCD/V-Cache и отсутствие работы на соседнем SMT-потоке. Режим питания не фиксировался; намеренная фоновая нагрузка не запускалась.

Streaming радикально уменьшает Peak Working Set и ускоряет два текстовых распределения, но проигрывает на `mixed`. Сборка `znver5` дала измеримый выигрыш во всех шести парных сравнениях, однако только пять превысили практический порог 5%. Это локальный результат для данного процессора и workload, а не универсальная гарантия для Zen 5.

## Воспроизведение

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

Для полного сравнения сборок команда `windows-build-runtime` повторяется для обеих реализаций и трёх наборов данных.

Raw data: [environment](results/ba02-windows-environment.txt), [portable runtime](results/ba02-portable-native-windows.jsonl), [`znver5` runtime](results/ba02-znver5-native-windows.jsonl), [paired build comparison](results/ba02-build-comparison-native-windows.jsonl), [portable memory](results/ba02-portable-memory-native-windows.jsonl), [`znver5` memory](results/ba02-znver5-memory-native-windows.jsonl).
