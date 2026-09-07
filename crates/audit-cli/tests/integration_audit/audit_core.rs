use std::fs;
use std::path::Path;

use audit_api::{
    audit::audit,
    models::{
        AuditConfig,
        TrialStatus,
    },
};
use rusqlite::{
    Connection,
    params,
};
use tempfile::tempdir;

use super::fixtures::write_sample_repo;

fn copy_fixture(
    fixture_name: &str,
) -> tempfile::TempDir {
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/repository-guidance")
        .join(fixture_name);
    let destination = tempdir().expect("fixture temp repo");

    for entry in fs::read_dir(source).expect("read fixture") {
        let entry = entry.expect("fixture entry");
        if !entry.file_type().expect("fixture entry type").is_file() {
            continue;
        }
        fs::copy(entry.path(), destination.path().join(entry.file_name()))
            .expect("copy fixture file");
    }

    destination
}

#[test]
fn audit_reports_repository_guidance() {
    let missing = copy_fixture("missing-files");
    let missing_report = audit(missing.path(), AuditConfig::default())
        .expect("missing guidance audit succeeds");

    assert!(missing_report.findings.iter().any(|finding| {
        finding.id == "repository_guidance:empty:INSTALL.md"
    }));
    assert!(missing_report.findings.iter().any(|finding| {
        finding.id == "repository_guidance:missing:CONTRIBUTING.md"
    }));
    assert_eq!(
        missing_report.metrics.repository_guidance.missing_files,
        1
    );
    assert_eq!(missing_report.metrics.repository_guidance.empty_files, 1);

    let complete = copy_fixture("complete-files");
    let complete_report = audit(complete.path(), AuditConfig::default())
        .expect("complete guidance audit succeeds");

    assert!(complete_report
        .findings
        .iter()
        .all(|finding| finding.category != "repository_guidance"));
    assert_eq!(complete_report.metrics.repository_guidance.present_files, 3);
}

#[test]
fn audit_collects_findings_and_prunes_stale_index_entries() {
    let repo = tempdir().expect("temp repo");
    write_sample_repo(repo.path());

    let report = audit(
        repo.path(),
        AuditConfig {
            max_file_lines: 20,
            max_cyclomatic_complexity: 3,
            coverage_warn_below: 80.0,
        },
    )
    .expect("first audit succeeds");

    assert!(report.findings.iter().any(|finding| {
        finding.category == "file_length"
            && finding.path.as_deref() == Some("src/lib.rs")
    }));
    assert!(report.findings.iter().any(|finding| {
        finding.category == "static_complexity"
            && finding.path.as_deref() == Some("src/lib.rs")
    }));
    assert!(report.metrics.compiler_warnings.count.unwrap_or_default() >= 1);
    assert_eq!(report.metrics.test_results.failed, Some(0));
    assert!(report.metrics.test_results.passed.unwrap_or_default() >= 1);
    assert_eq!(report.metrics.test_results.success_rate, Some(100.0));

    match report.metrics.coverage.status {
        TrialStatus::Collected => {
            assert!(report.metrics.coverage.line_percent.is_some());
        },
        TrialStatus::Unavailable => {
            assert!(report.findings.iter().any(|finding| {
                finding.id == "coverage_tool_missing"
                    || finding.id == "coverage_nested_invocation_skipped"
                    || finding.id == "coverage_profraw_missing"
            }));
        },
        TrialStatus::Failed => {
            panic!(
                "coverage collection should either succeed or report the tool as unavailable"
            );
        },
        TrialStatus::NotApplicable => {
            panic!("coverage should be applicable for a Cargo repository");
        },
    }

    fs::remove_file(repo.path().join("src/extra.rs"))
        .expect("remove stale file");

    let second_report = audit(
        repo.path(),
        AuditConfig {
            max_file_lines: 20,
            max_cyclomatic_complexity: 3,
            coverage_warn_below: 80.0,
        },
    )
    .expect("second audit succeeds");

    assert_eq!(second_report.sync.pruned_files, 1);

    let connection =
        Connection::open(&second_report.index_database).expect("open audit db");
    let indexed_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM files WHERE path = ?1",
            params!["src/extra.rs"],
            |row| row.get(0),
        )
        .expect("query stale row count");
    assert_eq!(indexed_count, 0);
}
