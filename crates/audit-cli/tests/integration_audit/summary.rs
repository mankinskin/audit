use assert_cmd::Command;
use audit_api::{
    audit::audit,
    models::AuditConfig,
    summary::{
        AuditSummaryBy,
        summarize_report,
    },
};
use audit_cli::cli::{
    CliOutput,
    parse_cli_from,
    run,
};
use tempfile::tempdir;

use super::fixtures::{
    assert_unix_formatted_output_text,
    write_workspace_repo,
};

#[test]
fn summary_groups_findings_by_crate_and_supports_cli_output() {
    let repo = tempdir().expect("temp repo");
    write_workspace_repo(repo.path());

    let report = audit(
        repo.path(),
        AuditConfig {
            max_file_lines: 20,
            max_cyclomatic_complexity: 3,
            coverage_warn_below: 80.0,
        },
    )
    .expect("audit succeeds");

    let summary = summarize_report(&report, AuditSummaryBy::Crate)
        .expect("summarize report");
    assert_eq!(summary.by, AuditSummaryBy::Crate);
    assert_eq!(summary.total_findings, report.findings.len());
    assert!(summary.repo_wide_issues >= 1);
    assert!(
        summary
            .groups
            .iter()
            .any(|group| group.key == "workspace-root")
    );
    assert!(
        summary
            .groups
            .iter()
            .any(|group| group.key == "nested-member")
    );
    assert!(
        summary
            .unmapped_paths
            .iter()
            .any(|group| group.key == "scripts/helper.py")
    );

    let cli = parse_cli_from([
        "audit",
        "--json",
        "summary",
        "--by",
        "crate",
        repo.path().to_string_lossy().as_ref(),
        "--max-file-lines",
        "20",
        "--max-cyclomatic-complexity",
        "3",
    ])
    .expect("parse summary cli");

    match run(cli).expect("run summary cli") {
        CliOutput::Machine(
            value,
            audit_cli::cli::MachineOutputFormat::Json,
        ) => {
            assert_eq!(value["by"], "crate");
            assert_eq!(value["total_findings"], report.findings.len());
            assert!(value["groups"].as_array().is_some_and(|groups| {
                groups.iter().any(|group| group["key"] == "workspace-root")
                    && groups
                        .iter()
                        .any(|group| group["key"] == "nested-member")
            }));
            assert!(value["unmapped_paths"].as_array().is_some_and(|groups| {
                groups
                    .iter()
                    .any(|group| group["key"] == "scripts/helper.py")
            }));
        },
        CliOutput::Machine(_, format) => {
            panic!("expected json machine output, got {format:?}");
        },
        CliOutput::Text(_) => panic!("expected json output"),
    }

    let text_cli = parse_cli_from([
        "audit",
        "summary",
        "--by",
        "package",
        repo.path().to_string_lossy().as_ref(),
        "--max-file-lines",
        "20",
        "--max-cyclomatic-complexity",
        "3",
    ])
    .expect("parse summary text cli");

    match run(text_cli).expect("run summary text cli") {
        CliOutput::Text(output) => {
            assert_unix_formatted_output_text(&output);
            assert!(output.contains("Repository Audit Summary"));
            assert!(output.contains("Grouped by: crate"));
            assert!(output.contains("workspace-root"));
            assert!(output.contains("nested-member"));
        },
        CliOutput::Machine(_, _) => panic!("expected text output"),
    }

    let mut command = Command::cargo_bin("audit").expect("audit binary");
    command
        .arg("summary")
        .arg("--by")
        .arg("crate")
        .arg(repo.path())
        .arg("--max-file-lines")
        .arg("20")
        .arg("--max-cyclomatic-complexity")
        .arg("3");
    command
        .assert()
        .success()
        .stdout(predicates::str::contains("Repository Audit Summary"));
}
