use std::{
    path::Path,
    process::Output,
};

use serde::Deserialize;
use serde_json::json;

use crate::{
    error::AuditError,
    models::{
        AuditFinding,
        Severity,
        TestSummary,
        TrialStatus,
    },
};

use super::{
    CargoScope,
    TestTrialResult,
    append_package_args,
    cargo_scope,
    run_command,
    trim_output,
};

pub(super) fn collect_test_success(
    repo_root: &Path,
    exclude_paths: &[String],
) -> Result<TestTrialResult, AuditError> {
    let cargo_scope = cargo_scope(repo_root, exclude_paths)?;
    if let Some(result) = not_applicable_result(&cargo_scope) {
        return Ok(result);
    }

    let output = run_test_command(repo_root, &cargo_scope.package_names)?;
    let summary = summarize_test_output(&output.stdout);
    let findings = build_findings(&output, &summary);

    Ok(TestTrialResult {
        metric: build_metric(&output, &summary),
        findings,
    })
}

fn not_applicable_result(cargo_scope: &CargoScope) -> Option<TestTrialResult> {
    if !cargo_scope.has_manifest {
        return Some(TestTrialResult {
            metric: TestSummary {
                status: TrialStatus::NotApplicable,
                total: None,
                passed: None,
                failed: None,
                ignored: None,
                success_rate: None,
                details: Some(
                    "No Cargo.toml found at the repository root.".to_string(),
                ),
            },
            findings: Vec::new(),
        });
    }

    if cargo_scope.package_names.is_empty() {
        return Some(TestTrialResult {
            metric: TestSummary {
                status: TrialStatus::NotApplicable,
                total: None,
                passed: None,
                failed: None,
                ignored: None,
                success_rate: None,
                details: Some(
                    "All workspace Cargo packages are excluded by config."
                        .to_string(),
                ),
            },
            findings: Vec::new(),
        });
    }

    None
}

fn run_test_command(
    repo_root: &Path,
    package_names: &[String],
) -> Result<Output, AuditError> {
    let mut args = vec![
        "test".to_string(),
        "--lib".to_string(),
        "--tests".to_string(),
        "--no-fail-fast".to_string(),
    ];
    append_package_args(&mut args, package_names);
    args.extend([
        "--".to_string(),
        "--format=json".to_string(),
        "-Z".to_string(),
        "unstable-options".to_string(),
    ]);

    run_command(repo_root, "cargo", args)
}

fn summarize_test_output(stdout: &[u8]) -> TestRunSummary {
    let mut summary = TestRunSummary::default();

    for line in String::from_utf8_lossy(stdout).lines() {
        let Ok(event) = serde_json::from_str::<LibtestEvent>(line) else {
            continue;
        };
        summary.record(event);
    }

    summary
}

fn build_findings(
    output: &Output,
    summary: &TestRunSummary,
) -> Vec<AuditFinding> {
    let mut findings = Vec::new();

    if summary.failed > 0 {
        findings.push(test_failure_finding(summary));
    }

    if !output.status.success() && summary.failed == 0 {
        findings.push(test_command_failed_finding(output));
    }

    findings
}

fn build_metric(
    output: &Output,
    summary: &TestRunSummary,
) -> TestSummary {
    let total = summary.total();

    TestSummary {
        status: if total > 0 || output.status.success() {
            TrialStatus::Collected
        } else {
            TrialStatus::Failed
        },
        total: Some(total),
        passed: Some(summary.passed),
        failed: Some(summary.failed),
        ignored: Some(summary.ignored),
        success_rate: summary.success_rate(),
        details: if output.status.success() {
            None
        } else {
            Some(trim_output(&output.stderr))
        },
    }
}

fn test_failure_finding(summary: &TestRunSummary) -> AuditFinding {
    AuditFinding {
        id: "test_failures".to_string(),
        category: "test_failure".to_string(),
        severity: Severity::High,
        summary: format!(
            "cargo test reported {} failing tests out of {} executed tests.",
            summary.failed,
            summary.passed + summary.failed
        ),
        path: None,
        line: None,
        metric_name: "test_success_rate".to_string(),
        metric_value: json!(summary.success_rate()),
        threshold: Some(json!(100.0)),
        instructions: vec![
            "Fix failing tests before trusting the rest of the quality metrics.".to_string(),
            "Re-run the failing test names directly so you can stabilize the smallest broken slice first.".to_string(),
        ],
        evidence: json!({
            "passed": summary.passed,
            "failed": summary.failed,
            "ignored": summary.ignored,
            "failing_tests": summary.failing_tests.iter().take(20).cloned().collect::<Vec<_>>(),
        }),
    }
}

fn test_command_failed_finding(output: &Output) -> AuditFinding {
    AuditFinding {
        id: "test_command_failed".to_string(),
        category: "test_execution".to_string(),
        severity: Severity::High,
        summary: "cargo test failed before structured test results could be collected.".to_string(),
        path: None,
        line: None,
        metric_name: "cargo_test_exit_code".to_string(),
        metric_value: json!(output.status.code()),
        threshold: None,
        instructions: vec![
            "Fix the cargo test invocation or build failure and rerun the audit to restore test success metrics.".to_string(),
        ],
        evidence: json!({
            "stderr": trim_output(&output.stderr),
        }),
    }
}

#[derive(Default)]
struct TestRunSummary {
    passed: usize,
    failed: usize,
    ignored: usize,
    failing_tests: Vec<String>,
}

impl TestRunSummary {
    fn total(&self) -> usize {
        self.passed + self.failed + self.ignored
    }

    fn success_rate(&self) -> Option<f64> {
        if self.passed + self.failed == 0 {
            None
        } else {
            Some(
                (self.passed as f64 / (self.passed + self.failed) as f64)
                    * 100.0,
            )
        }
    }

    fn record(
        &mut self,
        event: LibtestEvent,
    ) {
        if event.kind != "test" {
            return;
        }

        match event.event.as_deref() {
            Some("ok") => self.passed += 1,
            Some("failed") => {
                self.failed += 1;
                if let Some(name) = event.name {
                    self.failing_tests.push(name);
                }
            },
            Some("ignored") => self.ignored += 1,
            _ => {},
        }
    }
}

#[derive(Debug, Deserialize)]
struct LibtestEvent {
    #[serde(rename = "type")]
    kind: String,
    event: Option<String>,
    name: Option<String>,
}
