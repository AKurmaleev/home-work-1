#[derive(Debug, PartialEq, Eq)]
pub struct CodegenContract {
    pub target_cpu: Option<String>,
    pub target_feature_overrides: Vec<String>,
}

pub fn parse_codegen_contract(encoded: &str) -> Result<CodegenContract, String> {
    let arguments = encoded
        .split('\u{1f}')
        .filter(|argument| !argument.is_empty())
        .collect::<Vec<_>>();
    let mut target_cpus = Vec::new();
    let mut target_feature_overrides = Vec::new();
    let mut index = 0;

    while index < arguments.len() {
        let argument = arguments[index];
        let codegen_option = if argument == "-C" {
            index += 1;
            arguments
                .get(index)
                .copied()
                .ok_or_else(|| "-C requires a codegen option".to_owned())?
        } else if let Some(option) = argument.strip_prefix("-C") {
            option
        } else {
            argument
        };

        if let Some(cpu) = codegen_option.strip_prefix("target-cpu=") {
            if cpu.is_empty() {
                return Err("target-cpu must not be empty".to_owned());
            }
            target_cpus.push(cpu.to_owned());
        } else if let Some(features) = codegen_option.strip_prefix("target-feature=") {
            target_feature_overrides.push(features.to_owned());
        }

        index += 1;
    }

    if target_cpus.len() > 1 {
        return Err(format!(
            "benchmark build requires exactly one target-cpu flag; found {}",
            target_cpus.len()
        ));
    }

    Ok(CodegenContract {
        target_cpu: target_cpus.pop(),
        target_feature_overrides,
    })
}
