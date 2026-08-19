use serde_json::json;

use crate::models::{
    AuditFinding,
    FileLengthMetric,
    IndexedFile,
    Severity,
};

pub struct FileLengthResult {
    pub metric: FileLengthMetric,
    pub findings: Vec<AuditFinding>,
}

pub fn evaluate(
    files: &[IndexedFile],
    threshold: usize,
) -> FileLengthResult {
    let mut findings = Vec::new();
    let total_lines = files.iter().map(|file| file.line_count).sum::<usize>();
    let max_lines = files
        .iter()
        .map(|file| file.line_count)
        .max()
        .unwrap_or_default();

    for file in files.iter().filter(|file| file.line_count > threshold) {
        findings.push(AuditFinding {
            id: format!("file_length:{}", file.path),
            category: "file_length".to_string(),
            severity: if file.line_count >= threshold * 2 {
                Severity::High
            } else {
                Severity::Medium
            },
            summary: format!(
                "{} has {} lines, exceeding the {} line limit.",
                file.path, file.line_count, threshold
            ),
            path: Some(file.path.clone()),
            line: None,
            metric_name: "line_count".to_string(),
            metric_value: json!(file.line_count),
            threshold: Some(json!(threshold)),
            instructions: vec![
                format!(
                    "Split {} into smaller feature-focused modules so each file stays under {} lines.",
                    file.path, threshold
                ),
                "Extract unrelated types, helper functions, or tests into sibling modules and keep the public API thin.".to_string(),
            ],
            evidence: json!({
                "path": file.path,
                "line_count": file.line_count,
                "threshold": threshold,
                "language": file.language,
            }),
        });
    }

    FileLengthResult {
        metric: FileLengthMetric {
            threshold,
            long_files: findings.len(),
            average_lines: average(total_lines, files.len()),
            max_lines,
        },
        findings,
    }
}

fn average(
    total: usize,
    count: usize,
) -> f64 {
    if count == 0 {
        0.0
    } else {
        total as f64 / count as f64
    }
}
