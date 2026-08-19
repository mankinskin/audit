use std::{
    io::Cursor,
    path::Path,
    process::{
        Command,
        Output,
    },
};

use cargo_metadata::{
    Message,
    MetadataCommand,
    diagnostic::DiagnosticLevel,
};
use serde_json::json;

use crate::{
    config::{
        is_repo_relative_path_excluded,
        normalize_output_text,
        normalize_repo_relative_path,
    },
    error::AuditError,
    models::{
        AuditFinding,
        CountMetric,
        CoverageSummary,
        Severity,
        TestSummary,
        TrialStatus,
    },
};

mod coverage;
mod test_success;

pub struct CountTrialResult {
    pub metric: CountMetric,
    pub findings: Vec<AuditFinding>,
}

pub struct TestTrialResult {
    pub metric: TestSummary,
    pub findings: Vec<AuditFinding>,
}

pub struct CoverageTrialResult {
    pub metric: CoverageSummary,
    pub findings: Vec<AuditFinding>,
}

pub fn collect_compiler_warnings(
    repo_root: &Path,
    exclude_paths: &[String],
) -> Result<CountTrialResult, AuditError> {
    let cargo_scope = cargo_scope(repo_root, exclude_paths)?;
    if !cargo_scope.has_manifest {
        return Ok(CountTrialResult {
            metric: CountMetric {
                status: TrialStatus::NotApplicable,
                count: None,
                details: Some(
                    "No Cargo.toml found at the repository root.".to_string(),
                ),
            },
            findings: Vec::new(),
        });
    }

    if cargo_scope.package_names.is_empty() {
        return Ok(CountTrialResult {
            metric: CountMetric {
                status: TrialStatus::NotApplicable,
                count: None,
                details: Some(
                    "All workspace Cargo packages are excluded by config."
                        .to_string(),
                ),
            },
            findings: Vec::new(),
        });
    }

    let mut args = vec![
        "check".to_string(),
        "--all-targets".to_string(),
        "--message-format=json-diagnostic-rendered-ansi".to_string(),
    ];
    append_package_args(&mut args, &cargo_scope.package_names);

    let output = run_command(repo_root, "cargo", args)?;

    let mut warnings = Vec::new();
    for message in Message::parse_stream(Cursor::new(&output.stdout)) {
        let Ok(message) = message else {
            continue;
        };
        if let Message::CompilerMessage(compiler_message) = message {
            if compiler_message.message.level == DiagnosticLevel::Warning {
                let primary_span = compiler_message
                    .message
                    .spans
                    .iter()
                    .find(|span| span.is_primary);
                if primary_span.is_some_and(|span| {
                    is_file_name_excluded(
                        repo_root,
                        &span.file_name,
                        exclude_paths,
                    )
                }) {
                    continue;
                }
                warnings.push(json!({
                    "message": compiler_message.message.message,
                    "code": compiler_message.message.code.as_ref().map(|code| code.code.clone()),
                    "path": primary_span.map(|span| normalize_output_text(&span.file_name)),
                    "line": primary_span.map(|span| span.line_start),
                    "rendered": compiler_message
                        .message
                        .rendered
                        .as_deref()
                        .map(normalize_output_text),
                }));
            }
        }
    }

    let mut findings = Vec::new();
    if !warnings.is_empty() {
        findings.push(AuditFinding {
            id: "compiler_warnings".to_string(),
            category: "compiler_warning".to_string(),
            severity: if warnings.len() > 20 {
                Severity::High
            } else {
                Severity::Medium
            },
            summary: format!("cargo check reported {} compiler warnings.", warnings.len()),
            path: None,
            line: None,
            metric_name: "compiler_warning_count".to_string(),
            metric_value: json!(warnings.len()),
            threshold: Some(json!(0)),
            instructions: vec![
                "Fix compiler warnings before adding more changes so dead code, unused variables, and deprecations do not accumulate.".to_string(),
                "Re-run `cargo check --workspace --all-targets` after each warning batch to keep the workspace clean.".to_string(),
            ],
            evidence: json!({
                "warning_count": warnings.len(),
                "sample": warnings.iter().take(20).cloned().collect::<Vec<_>>(),
            }),
        });
    }

    if !output.status.success() {
        findings.push(AuditFinding {
            id: "compiler_check_failed".to_string(),
            category: "compiler_check".to_string(),
            severity: Severity::High,
            summary: "cargo check failed, so warning counts may be incomplete.".to_string(),
            path: None,
            line: None,
            metric_name: "cargo_check_exit_code".to_string(),
            metric_value: json!(output.status.code()),
            threshold: None,
            instructions: vec![
                "Fix build errors first, then rerun the audit so compiler warnings can be reported accurately.".to_string(),
            ],
            evidence: json!({
                "stderr": trim_output(&output.stderr),
            }),
        });
    }

    Ok(CountTrialResult {
        metric: CountMetric {
            status: if output.status.success() {
                TrialStatus::Collected
            } else {
                TrialStatus::Failed
            },
            count: Some(warnings.len()),
            details: if output.status.success() {
                None
            } else {
                Some(trim_output(&output.stderr))
            },
        },
        findings,
    })
}

pub fn collect_test_success(
    repo_root: &Path,
    exclude_paths: &[String],
) -> Result<TestTrialResult, AuditError> {
    test_success::collect_test_success(repo_root, exclude_paths)
}

pub fn collect_coverage(
    repo_root: &Path,
    exclude_paths: &[String],
    warn_below: f64,
) -> Result<CoverageTrialResult, AuditError> {
    coverage::collect_coverage(repo_root, exclude_paths, warn_below)
}

fn run_command(
    repo_root: &Path,
    program: &str,
    args: Vec<String>,
) -> Result<Output, AuditError> {
    let output = Command::new(program)
        .args(args)
        .current_dir(repo_root)
        .output()?;
    Ok(output)
}

fn has_cargo_manifest(repo_root: &Path) -> bool {
    repo_root.join("Cargo.toml").exists()
}

fn cargo_scope(
    repo_root: &Path,
    exclude_paths: &[String],
) -> Result<CargoScope, AuditError> {
    if !has_cargo_manifest(repo_root) {
        return Ok(CargoScope {
            has_manifest: false,
            package_names: Vec::new(),
        });
    }

    let metadata = MetadataCommand::new()
        .current_dir(repo_root)
        .no_deps()
        .exec()
        .map_err(|err| AuditError::CommandFailed {
            command: "cargo metadata --no-deps".to_string(),
            details: normalize_output_text(err.to_string()),
        })?;

    let package_names = metadata
        .workspace_packages()
        .iter()
        .filter_map(|package| {
            let manifest_path =
                package.manifest_path.as_std_path().canonicalize().ok()?;
            let relative_manifest =
                manifest_path.strip_prefix(repo_root).ok()?;
            if is_repo_relative_path_excluded(relative_manifest, exclude_paths)
            {
                return None;
            }
            Some(package.name.to_string())
        })
        .collect();

    Ok(CargoScope {
        has_manifest: true,
        package_names,
    })
}

fn append_package_args(
    args: &mut Vec<String>,
    package_names: &[String],
) {
    for package_name in package_names {
        args.push("-p".to_string());
        args.push(package_name.clone());
    }
}

fn is_file_name_excluded(
    repo_root: &Path,
    file_name: &str,
    exclude_paths: &[String],
) -> bool {
    let file_path = Path::new(file_name);
    let relative = file_path
        .strip_prefix(repo_root)
        .ok()
        .map(normalize_repo_relative_path)
        .or_else(|| {
            if file_path.is_relative() {
                Some(normalize_repo_relative_path(file_path))
            } else {
                None
            }
        });

    relative.as_deref().is_some_and(|relative| {
        is_repo_relative_path_excluded(Path::new(relative), exclude_paths)
    })
}

struct CargoScope {
    has_manifest: bool,
    package_names: Vec<String>,
}

fn trim_output(output: &[u8]) -> String {
    let text = String::from_utf8_lossy(output);
    let lines = text.lines().take(40).collect::<Vec<_>>();
    normalize_output_text(lines.join("\n"))
}
