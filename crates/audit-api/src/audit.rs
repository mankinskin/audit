use std::{
    collections::BTreeSet,
    path::Path,
};

use chrono::Utc;

use crate::{
    config::{
        AuditFileConfig,
        format_output_path,
    },
    error::AuditError,
    index::RepositoryIndex,
    models::{
        AuditConfig,
        AuditMetrics,
        AuditReport,
        AuditRunInfo,
    },
    trials::{
        cargo_quality,
        file_length,
        rule_overlap,
        session_workflow_graph,
        spec_fulfillment,
        static_metrics,
        ticket_graph,
    },
};

pub fn audit(
    repo_root: &Path,
    config: AuditConfig,
) -> Result<AuditReport, AuditError> {
    if !repo_root.exists() {
        return Err(AuditError::MissingRepoRoot(format_output_path(repo_root)));
    }

    let repo_root = repo_root.canonicalize()?;
    let file_config = AuditFileConfig::load(&repo_root)?;
    let started_at = Utc::now();
    let index = RepositoryIndex::open_or_init(&repo_root)?;
    let sync = index.sync_source_files(&file_config.exclude_paths)?;
    let indexed_files = index.indexed_files()?;

    let file_length_result =
        file_length::evaluate(&indexed_files, config.max_file_lines);
    let static_metrics_result = static_metrics::evaluate(
        &repo_root,
        &indexed_files,
        config.max_cyclomatic_complexity,
    )?;
    let compiler_warnings_result = cargo_quality::collect_compiler_warnings(
        &repo_root,
        &file_config.exclude_paths,
    )?;
    let test_results = cargo_quality::collect_test_success(
        &repo_root,
        &file_config.exclude_paths,
    )?;
    let coverage_result = cargo_quality::collect_coverage(
        &repo_root,
        &file_config.exclude_paths,
        config.coverage_warn_below,
    )?;
    let spec_fulfillment_result = spec_fulfillment::evaluate(&repo_root);
    let ticket_graph_result = ticket_graph::evaluate(&repo_root);
    let session_workflow_graph_result =
        session_workflow_graph::evaluate(&repo_root);
    let rule_overlap_result = rule_overlap::evaluate(&repo_root);

    let mut findings = file_length_result.findings;
    findings.extend(static_metrics_result.findings);
    findings.extend(compiler_warnings_result.findings);
    findings.extend(test_results.findings);
    findings.extend(coverage_result.findings);
    findings.extend(spec_fulfillment_result.findings);
    findings.extend(ticket_graph_result.findings);
    findings.extend(session_workflow_graph_result.findings);
    findings.extend(rule_overlap_result.findings);

    let total_lines = indexed_files.iter().map(|file| file.line_count).sum();
    let metrics = AuditMetrics {
        source_files: indexed_files.len(),
        total_lines,
        file_length: file_length_result.metric,
        compiler_warnings: compiler_warnings_result.metric,
        test_results: test_results.metric,
        coverage: coverage_result.metric,
        static_metrics: static_metrics_result.metric,
        spec_fulfillment: spec_fulfillment_result.metric,
        ticket_graph: ticket_graph_result.metric,
        session_workflow_graph: session_workflow_graph_result.metric,
        rule_overlap: rule_overlap_result.metric,
    };

    let finished_at = Utc::now();
    let instructions = collect_instructions(&findings);
    let started_at_string = started_at.to_rfc3339();
    let finished_at_string = finished_at.to_rfc3339();
    let run_id = index.record_audit_run(
        &started_at_string,
        &finished_at_string,
        "completed",
        &metrics,
        &sync,
        &findings,
    )?;

    Ok(AuditReport {
        service: "audit-mcp".to_string(),
        repo_root: format_output_path(&repo_root),
        index_database: format_output_path(index.db_path()),
        sync,
        run: AuditRunInfo {
            run_id,
            started_at: started_at_string,
            finished_at: finished_at_string,
            status: "completed".to_string(),
        },
        metrics,
        findings,
        instructions,
    })
}

fn collect_instructions(
    findings: &[crate::models::AuditFinding]
) -> Vec<String> {
    let mut unique = BTreeSet::new();
    for finding in findings {
        for instruction in &finding.instructions {
            unique.insert(instruction.clone());
        }
    }
    unique.into_iter().collect()
}
