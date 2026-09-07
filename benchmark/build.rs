use std::env;

mod build_contract;

use build_contract::parse_codegen_contract;

fn main() {
    println!("cargo:rerun-if-env-changed=HW1_BENCH_BUILD_FLAVOR");

    let flavor = env::var("HW1_BENCH_BUILD_FLAVOR").unwrap_or_else(|_| "unlabeled".to_owned());
    let target = env::var("TARGET").expect("Cargo must provide TARGET to build scripts");
    let encoded_rustflags = env::var("CARGO_ENCODED_RUSTFLAGS").unwrap_or_default();
    let codegen = parse_codegen_contract(&encoded_rustflags)
        .unwrap_or_else(|error| panic!("invalid benchmark codegen flags: {error}"));
    let target_cpu = codegen.target_cpu.as_deref().unwrap_or("unspecified");

    match flavor.as_str() {
        "portable" if target_cpu != "x86-64" => {
            panic!("portable benchmark build requires -C target-cpu=x86-64")
        }
        "znver5" if target_cpu != "znver5" => {
            panic!("znver5 benchmark build requires -C target-cpu=znver5")
        }
        "portable" | "znver5" if !codegen.target_feature_overrides.is_empty() => {
            panic!("labeled benchmark builds forbid explicit -C target-feature overrides")
        }
        "portable" | "znver5" | "unlabeled" => {}
        _ => panic!("HW1_BENCH_BUILD_FLAVOR must be portable, znver5, or unset"),
    }

    println!("cargo:rustc-env=HW1_BENCH_BUILD_FLAVOR={flavor}");
    println!("cargo:rustc-env=HW1_BENCH_TARGET={target}");
    println!("cargo:rustc-env=HW1_BENCH_TARGET_CPU={target_cpu}");
}
