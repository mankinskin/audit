use std::{
    env,
    fs,
    path::{
        Path,
        PathBuf,
    },
    process,
    process::Command,
    time::{
        SystemTime,
        UNIX_EPOCH,
    },
};

use serde_json::{
    Value,
    json,
};

use crate::{
    error::AuditError,
    models::{
        AuditFinding,
        CoverageSummary,
        Severity,
        TrialStatus,
    },
};

use super::{
    CoverageTrialResult,
    append_package_args,
    cargo_scope,
    trim_output,
};

pub(super) fn collect_coverage(
    repo_root: &Path,
    exclude_paths: &[String],
    warn_below: f64,
) -> Result<CoverageTrialResult, AuditError> {
    let cargo_scope = cargo_scope(repo_root, exclude_paths)?;
    if !cargo_scope.has_manifest {
        return Ok(CoverageTrialResult {
            metric: CoverageSummary {
                status: TrialStatus::NotApplicable,
                line_percent: None,
                covered_lines: None,
                total_lines: None,
                details: Some(
                    "No Cargo.toml found at the repository root.".to_string(),
                ),
            },
            findings: Vec::new(),
        });
    }

    if cargo_scope.package_names.is_empty() {
        return Ok(CoverageTrialResult {
            metric: CoverageSummary {
                status: TrialStatus::NotApplicable,
                line_percent: None,
                covered_lines: None,
                total_lines: None,
                details: Some(
                    "All workspace Cargo packages are excluded by config."
                        .to_string(),
                ),
            },
            findings: Vec::new(),
        });
    }

    if env::var_os("CARGO_LLVM_COV").is_some() {
        return Ok(nested_coverage_tool_result());
    }

    let version_probe = Command::new("cargo")
        .arg("llvm-cov")
        .arg("--version")
        .current_dir(repo_root)
        .output();

    let Ok(version_probe) = version_probe else {
        return Ok(missing_coverage_tool_result());
    };
    if !version_probe.status.success() {
        return Ok(missing_coverage_tool_result());
    }

    let mut args = vec![
        "llvm-cov".to_string(),
        "--json".to_string(),
        "--summary-only".to_string(),
        "--ignore-run-fail".to_string(),
        "--no-clean".to_string(),
    ];
    let target_dir = coverage_target_dir(repo_root);
    append_package_args(&mut args, &cargo_scope.package_names);
    let output = Command::new("cargo")
        .args(&args)
        .current_dir(repo_root)
        .env("CARGO_LLVM_COV_TARGET_DIR", &target_dir)
        .output();
    let _ = fs::remove_dir_all(&target_dir);
    let output = output?;

    if !output.status.success() {
        let stderr = trim_output(&output.stderr);
        if stderr.contains("not found *.profraw files") {
            return Ok(missing_profraw_coverage_result(stderr));
        }

        return Ok(CoverageTrialResult {
            metric: CoverageSummary {
                status: TrialStatus::Failed,
                line_percent: None,
                covered_lines: None,
                total_lines: None,
                details: Some(stderr.clone()),
            },
            findings: vec![AuditFinding {
                id: "coverage_collection_failed".to_string(),
                category: "coverage".to_string(),
                severity: Severity::High,
                summary: "cargo llvm-cov failed, so coverage metrics could not be collected.".to_string(),
                path: None,
                line: None,
                metric_name: "coverage_status".to_string(),
                metric_value: json!("failed"),
                threshold: Some(json!(warn_below)),
                instructions: vec![
                    "Fix the failing coverage command, then rerun the audit so line coverage can be measured.".to_string(),
                ],
                evidence: json!({
                    "stderr": stderr,
                }),
            }],
        });
    }

    let json: Value = serde_json::from_slice(&output.stdout)?;
    let lines = json
        .pointer("/totals/lines")
        .or_else(|| json.pointer("/data/0/totals/lines"));

    let Some(lines) = lines else {
        return Ok(CoverageTrialResult {
            metric: CoverageSummary {
                status: TrialStatus::Failed,
                line_percent: None,
                covered_lines: None,
                total_lines: None,
                details: Some("Could not parse the coverage summary emitted by cargo llvm-cov.".to_string()),
            },
            findings: vec![AuditFinding {
                id: "coverage_parse_failed".to_string(),
                category: "coverage".to_string(),
                severity: Severity::Medium,
                summary: "Coverage output was present but did not match the expected summary format.".to_string(),
                path: None,
                line: None,
                metric_name: "coverage_status".to_string(),
                metric_value: json!("unparsed"),
                threshold: Some(json!(warn_below)),
                instructions: vec![
                    "Check the installed cargo-llvm-cov version and update the parser if its JSON summary format changed.".to_string(),
                ],
                evidence: json!({
                    "stdout": trim_output(&output.stdout),
                }),
            }],
        });
    };

    let covered_lines = lines
        .get("covered")
        .and_then(Value::as_u64)
        .map(|value| value as usize);
    let total_lines = lines
        .get("count")
        .and_then(Value::as_u64)
        .map(|value| value as usize);
    let line_percent =
        lines.get("percent").and_then(Value::as_f64).or_else(|| {
            match (covered_lines, total_lines) {
                (Some(covered_lines), Some(total_lines)) if total_lines > 0 =>
                    Some((covered_lines as f64 / total_lines as f64) * 100.0),
                _ => None,
            }
        });

    let mut findings = Vec::new();
    if let Some(line_percent) = line_percent {
        if line_percent < warn_below {
            findings.push(AuditFinding {
                id: "coverage_below_threshold".to_string(),
                category: "coverage".to_string(),
                severity: Severity::Medium,
                summary: format!(
                    "Line coverage is {:.1}%, below the {:.1}% target.",
                    line_percent, warn_below
                ),
                path: None,
                line: None,
                metric_name: "line_coverage_percent".to_string(),
                metric_value: json!(line_percent),
                threshold: Some(json!(warn_below)),
                instructions: vec![
                    "Add focused unit tests around the highest-risk modules until coverage clears the configured target.".to_string(),
                    "Prefer small branch-specific tests over broad integration tests when closing coverage gaps.".to_string(),
                ],
                evidence: json!({
                    "line_percent": line_percent,
                    "covered_lines": covered_lines,
                    "total_lines": total_lines,
                }),
            });
        }
    }

    Ok(CoverageTrialResult {
        metric: CoverageSummary {
            status: TrialStatus::Collected,
            line_percent,
            covered_lines,
            total_lines,
            details: None,
        },
        findings,
    })
}

fn coverage_target_dir(repo_root: &Path) -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    repo_root
        .join("target")
        .join("audit-llvm-cov")
        .join(format!("{}-{}", process::id(), timestamp))
}

fn missing_coverage_tool_result() -> CoverageTrialResult {
    CoverageTrialResult {
        metric: CoverageSummary {
            status: TrialStatus::Unavailable,
            line_percent: None,
            covered_lines: None,
            total_lines: None,
            details: Some("cargo llvm-cov is not installed in this environment.".to_string()),
        },
        findings: vec![AuditFinding {
            id: "coverage_tool_missing".to_string(),
            category: "coverage".to_string(),
            severity: Severity::Medium,
            summary: "Coverage metrics are unavailable because cargo llvm-cov is not installed.".to_string(),
            path: None,
            line: None,
            metric_name: "coverage_status".to_string(),
            metric_value: json!("unavailable"),
            threshold: None,
            instructions: vec![
                "Install cargo-llvm-cov in the audit environment so line coverage can be collected automatically.".to_string(),
                "After installation, rerun the audit to populate coverage metrics and threshold findings.".to_string(),
            ],
            evidence: json!({
                "command": "cargo llvm-cov --version",
            }),
        }],
    }
}

fn nested_coverage_tool_result() -> CoverageTrialResult {
    CoverageTrialResult {
        metric: CoverageSummary {
            status: TrialStatus::Unavailable,
            line_percent: None,
            covered_lines: None,
            total_lines: None,
            details: Some(
                "Skipping nested cargo llvm-cov invocation because audit is already running under cargo llvm-cov."
                    .to_string(),
            ),
        },
        findings: vec![AuditFinding {
            id: "coverage_nested_invocation_skipped".to_string(),
            category: "coverage".to_string(),
            severity: Severity::Medium,
            summary:
                "Coverage metrics are unavailable during nested cargo llvm-cov runs."
                    .to_string(),
            path: None,
            line: None,
            metric_name: "coverage_status".to_string(),
            metric_value: json!("unavailable"),
            threshold: None,
            instructions: vec![
                "Run audit outside cargo llvm-cov when you need repository coverage metrics.".to_string(),
                "Keep nested audit invocations coverage-free so audit-cli integration tests can run under cargo llvm-cov.".to_string(),
            ],
            evidence: json!({
                "env_var": "CARGO_LLVM_COV",
            }),
        }],
    }
}

fn missing_profraw_coverage_result(stderr: String) -> CoverageTrialResult {
    CoverageTrialResult {
        metric: CoverageSummary {
            status: TrialStatus::Unavailable,
            line_percent: None,
            covered_lines: None,
            total_lines: None,
            details: Some(
                "Coverage metrics are unavailable because cargo llvm-cov did not produce any profraw data in this environment."
                    .to_string(),
            ),
        },
        findings: vec![AuditFinding {
            id: "coverage_profraw_missing".to_string(),
            category: "coverage".to_string(),
            severity: Severity::Medium,
            summary:
                "Coverage metrics are unavailable because cargo llvm-cov produced no profraw data."
                    .to_string(),
            path: None,
            line: None,
            metric_name: "coverage_status".to_string(),
            metric_value: json!("unavailable"),
            threshold: None,
            instructions: vec![
                "Re-run the audit in an environment where cargo llvm-cov can write profraw data, or treat coverage as unavailable for this run."
                    .to_string(),
            ],
            evidence: json!({
                "stderr": stderr,
            }),
        }],
    }
}
