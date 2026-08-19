use std::path::Path;

use serde_json::{
    Value,
    json,
};
use session_api::{
    SessionWorkflowGraph,
    validate_workflow_graph,
};

use crate::models::{
    AuditFinding,
    CountMetric,
    Severity,
    TrialStatus,
};

pub struct SessionWorkflowGraphResult {
    pub metric: CountMetric,
    pub findings: Vec<AuditFinding>,
}

pub fn evaluate(repo_root: &Path) -> SessionWorkflowGraphResult {
    let sessions_root = repo_root.join(".session").join("sessions");
    if !sessions_root.exists() {
        return SessionWorkflowGraphResult {
            metric: CountMetric::unavailable(
                "no .session/sessions directory found; skipping session workflow graph audit",
            ),
            findings: Vec::new(),
        };
    }

    let mut findings = Vec::new();
    for (source_path, graph) in scan_session_workflow_graphs(&sessions_root) {
        let relative_path = relative_path(repo_root, &source_path);
        for issue in validate_workflow_graph(&graph) {
            findings.push(AuditFinding {
                id: format!(
                    "session_workflow_graph:{}:{}",
                    relative_path, issue.code
                ),
                category: "session_workflow_graph".to_string(),
                severity: Severity::Medium,
                summary: issue.message.clone(),
                path: Some(relative_path.clone()),
                line: None,
                metric_name: "session_workflow_graph_issue_count".to_string(),
                metric_value: json!(1),
                threshold: Some(json!(0)),
                instructions: vec![format!(
                    "Fix the session workflow graph in {relative_path}."
                )],
                evidence: json!({
                    "source_file": relative_path,
                    "node_id": issue.node_id,
                    "code": issue.code,
                    "message": issue.message,
                }),
            });
        }
    }

    let issue_count = findings.len();
    SessionWorkflowGraphResult {
        metric: CountMetric {
            status: TrialStatus::Collected,
            count: Some(issue_count),
            details: Some(if issue_count == 0 {
                "all scanned session workflow graphs are structurally valid"
                    .to_string()
            } else {
                format!(
                    "{issue_count} structural issue(s) found across scanned session workflow graphs"
                )
            }),
        },
        findings,
    }
}

/// Walk legacy `.session/sessions/*/context.json` and
/// `.session/sessions/*/handoffs/*/handoff.json`, extracting embedded
/// workflow graphs and skipping files that are missing, unparseable, or
/// have no nodes.
fn scan_session_workflow_graphs(
    sessions_root: &Path
) -> Vec<(std::path::PathBuf, SessionWorkflowGraph)> {
    let mut results = Vec::new();
    let Ok(session_dirs) = std::fs::read_dir(sessions_root) else {
        return results;
    };

    for session_dir in session_dirs.flatten() {
        let session_path = session_dir.path();
        if !session_path.is_dir() {
            continue;
        }

        let context_path = session_path.join("context.json");
        if let Some(graph) =
            extract_graph(&context_path, |value| value.get("workflow").cloned())
        {
            results.push((context_path, graph));
        }

        let handoffs_root = session_path.join("handoffs");
        let Ok(handoff_dirs) = std::fs::read_dir(&handoffs_root) else {
            continue;
        };
        for handoff_dir in handoff_dirs.flatten() {
            let handoff_path = handoff_dir.path().join("handoff.json");
            if let Some(graph) = extract_graph(&handoff_path, |value| {
                value.get("workflow")?.get("workflow").cloned()
            }) {
                results.push((handoff_path, graph));
            }
        }
    }

    results
}

fn extract_graph(
    path: &Path,
    extract: impl Fn(&Value) -> Option<Value>,
) -> Option<SessionWorkflowGraph> {
    let raw = std::fs::read_to_string(path).ok()?;
    let value: Value = serde_json::from_str(&raw).ok()?;
    let graph_value = extract(&value)?;
    let graph: SessionWorkflowGraph =
        serde_json::from_value(graph_value).ok()?;
    if graph.nodes.is_empty() {
        return None;
    }
    Some(graph)
}

fn relative_path(
    repo_root: &Path,
    path: &Path,
) -> String {
    path.strip_prefix(repo_root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn evaluate_reports_dangling_edge_in_context_json() {
        let temp_dir = TempDir::new().expect("tempdir");
        let repo_root = temp_dir.path();
        let session_dir = repo_root
            .join(".session")
            .join("sessions")
            .join("session-1");
        std::fs::create_dir_all(&session_dir).expect("create session dir");

        let context = json!({
            "workflow": {
                "nodes": [
                    {
                        "node_id": "n1",
                        "kind": "task",
                        "title": "Node 1",
                        "requirement": "required",
                        "status": "pending",
                        "created_at": "2026-01-01T00:00:00Z",
                        "updated_at": "2026-01-01T00:00:00Z",
                    }
                ],
                "edges": [
                    { "from": "n1", "to": "missing", "kind": "depends-on" }
                ]
            }
        });
        std::fs::write(
            session_dir.join("context.json"),
            serde_json::to_string(&context).unwrap(),
        )
        .expect("write context.json");

        let result = evaluate(repo_root);

        assert_eq!(result.findings.len(), 1);
        let finding = &result.findings[0];
        assert_eq!(finding.category, "session_workflow_graph");
        assert!(finding.id.contains("dangling-edge"));
    }
}
