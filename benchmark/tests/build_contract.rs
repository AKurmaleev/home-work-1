#[path = "../build_contract.rs"]
mod build_contract;

use build_contract::{CodegenContract, parse_codegen_contract};

#[test]
fn parses_supported_target_cpu_forms() {
    assert_eq!(
        parse_codegen_contract("-C\u{1f}target-cpu=znver5").unwrap(),
        CodegenContract {
            target_cpu: Some("znver5".to_owned()),
            target_feature_overrides: Vec::new(),
        }
    );
    assert_eq!(
        parse_codegen_contract("-Ctarget-cpu=x86-64").unwrap(),
        CodegenContract {
            target_cpu: Some("x86-64".to_owned()),
            target_feature_overrides: Vec::new(),
        }
    );
}

#[test]
fn recognizes_long_codegen_flags() {
    for encoded in [
        "--codegen=target-cpu=znver5",
        "--codegen\u{1f}target-cpu=znver5",
    ] {
        assert_eq!(
            parse_codegen_contract(encoded)
                .unwrap()
                .target_cpu
                .as_deref(),
            Some("znver5")
        );
    }
    let contract =
        parse_codegen_contract("-Ctarget-cpu=x86-64\u{1f}--codegen=target-feature=+avx2").unwrap();
    assert_eq!(contract.target_feature_overrides, ["+avx2"]);
    assert!(
        parse_codegen_contract("-Ctarget-cpu=x86-64\u{1f}--codegen=target-cpu=znver5").is_err()
    );
    assert!(parse_codegen_contract("--codegen").is_err());
}

#[test]
fn rejects_duplicate_target_cpu_flags() {
    let error = parse_codegen_contract("-C\u{1f}target-cpu=x86-64\u{1f}-C\u{1f}target-cpu=znver5")
        .unwrap_err();

    assert!(error.contains("exactly one target-cpu"));
}

#[test]
fn reports_target_feature_overrides() {
    let contract =
        parse_codegen_contract("-C\u{1f}target-cpu=znver5\u{1f}-Ctarget-feature=-avx512f,+aes")
            .unwrap();

    assert_eq!(
        contract.target_feature_overrides,
        ["-avx512f,+aes".to_owned()]
    );
}
