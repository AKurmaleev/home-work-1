# Домашняя работа 1 — Rust

Workspace с решениями первой домашней работы по Rust. Основные задания находятся в пакетах `hello` и `ba01`–`ba04`; `bonus` и `benchmark` содержат дополнительные материалы.

## Быстрый запуск

```sh
cargo run -p hello
printf 'hello rust\n' | cargo run -p ba01
printf 'hello rust\n' | cargo run -p ba02
cargo run -p ba03 -- zebra apple rust
cargo test -p ba04
cargo run -p bonus
```

Проверка всего workspace:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Навигация по заданиям

| Раздел | Что сделано | Код |
|---|---|---|
| `hello` | Минимальная программа `Hello, World!` | [`main.rs`](hello/src/main.rs) |
| `ba01` | Потоковый подсчёт байтов из standard input, аналог `wc -c` | [`lib.rs`](ba01/src/lib.rs), [`main.rs`](ba01/src/main.rs) |
| `ba02` | Подсчёт строк, слов и байтов; сохранены initial и streaming реализации | [`lib.rs`](ba02/src/lib.rs), [`main.rs`](ba02/src/main.rs) |
| `ba03` | Лексикографическая сортировка аргументов командной строки | [`lib.rs`](ba03/src/lib.rs), [`main.rs`](ba03/src/main.rs) |
| `ba04` | Checked, wrapping и saturating сложение для `u8`, включая полный перебор входных пар в тестах | [`lib.rs`](ba04/src/lib.rs) |
| `bonus` | Кодирование, отображение и инверсия монохромного bitmap 8×8 | [`lib.rs`](bonus/src/lib.rs), [`main.rs`](bonus/src/main.rs) |

Состав workspace: [`Cargo.toml`](Cargo.toml).

## Дополнительный benchmark `ba02`

Benchmark не заменяет основное решение. Он сравнивает initial и streaming варианты `ba02` на входах размером 256 MiB, а также portable- и `znver5`-сборки.

- [Краткий сравнительный отчёт](BENCHMARKS.md)
- [Подробный нативный benchmark на Ryzen 9 9950X3D](benchmark/WINDOWS_NATIVE.md)
- [Исходный код benchmark](benchmark/src/ba02_large.rs)
- [Windows-раннер и проверка CPU](benchmark/src/windows_native.rs)
- [Машиночитаемые результаты](benchmark/results/)

Главный измеренный эффект streaming-версии — переход с `Θ(n)` на `Θ(1)` дополнительной памяти относительно размера входа. На нативной Windows Peak Working Set уменьшился примерно на 99.1%. Производительность зависит от распределения данных: получено ускорение на `prose` и `boundary`, но регрессия на искусственном `mixed`.

## История мажорных версий

Версии ниже обозначают крупные этапы развития работы; отдельные Git tags для них не создавались.

| Версия | Дата | Изменения |
|---|---|---|
| **v1** | 2026-08-27 | Первые завершённые решения всех основных заданий и bonus — [`e78d1a7`](https://github.com/AKurmaleev/home-work-1/commit/e78d1a7) |
| **v2** | 2026-08-27 | Оптимизированные безопасные реализации, выделение логики в библиотеки и расширение тестов — [`272a5e4`](https://github.com/AKurmaleev/home-work-1/commit/272a5e4) |
| **v3** | 2026-09-02 — 2026-09-03 | Большие benchmark-наборы для `ba02`, повторные измерения времени и памяти, статистический сравнительный анализ — [`90604d2`](https://github.com/AKurmaleev/home-work-1/commit/90604d2), [`ec79cb3`](https://github.com/AKurmaleev/home-work-1/commit/ec79cb3) |
| **v4** | 2026-09-07 | Нативное Windows-сравнение portable и `znver5` на Ryzen 9 9950X3D с отдельным отчётом и raw data — [`f257144`](https://github.com/AKurmaleev/home-work-1/commit/f257144) |
