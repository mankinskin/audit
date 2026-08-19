use std::{
    fs,
    path::{
        Path,
        PathBuf,
    },
};

use ignore::gitignore::{
    Gitignore,
    GitignoreBuilder,
};
use serde::Deserialize;
use serde_json::json;
use ticket_api::{
    error::StorageError,
    health,
    model::edge::EdgeRecord,
    storage::{
        TicketStore,
        indexed::IndexedTicket,
    },
    workflow::WorkflowModel,
};

use crate::{
    config::format_output_path,
    models::{
        AuditFinding,
        CountMetric,
        Severity,
        TrialStatus,
    },
};

pub struct TicketGraphResult {
    pub metric: CountMetric,
    pub findings: Vec<AuditFinding>,
}

#[derive(Debug, Deserialize, Default)]
struct WorkspacePolicyFile {
    #[serde(default)]
    ignore_workspaces: Vec<String>,
    #[serde(default)]
    include_overrides: Vec<String>,
    #[serde(default = "default_ignore_markers")]
    ignore_markers: Vec<String>,
    deny_external_paths: Option<bool>,
}

#[derive(Debug)]
struct WorkspacePolicyMatchers {
    repo_root: PathBuf,
    ignore: Gitignore,
    include: Gitignore,
    ignore_markers: Vec<String>,
    deny_external_paths: bool,
}

fn default_ignore_markers() -> Vec<String> {
    vec![
        ".ticket-ignore".to_string(),
        ".workspace-ignore".to_string(),
    ]
}

pub fn evaluate(repo_root: &Path) -> TicketGraphResult {
    let store = match open_ticket_store(repo_root) {
        Ok(store) => store,
        Err(result) => return result,
    };

    let (tickets, edges, workflow) = match load_ticket_graph_inputs(&store) {
        Ok(inputs) => inputs,
        Err(result) => return result,
    };

    let canonical_health =
        health::collect_findings(&store, &tickets, &edges, &workflow);
    let orphan_findings: Vec<AuditFinding> = canonical_health
        .findings
        .into_iter()
        .filter_map(|finding| {
            if finding.check != "graph_participation" {
                return None;
            }

            let display_path = relative_ticket_path(repo_root, &finding.path);
            let title = if finding.title.is_empty() {
                finding.ticket_id.to_string()
            } else {
                finding.title.clone()
            };
            Some(AuditFinding {
                id: format!("ticket_graph:{}", finding.ticket_id),
                category: "ticket_graph".to_string(),
                severity: orphan_severity(finding.state.as_deref()),
                summary: format!(
                    "{} is not linked into the depends_on graph.",
                    title
                ),
                path: Some(display_path.clone()),
                line: None,
                metric_name: "orphan_ticket_count".to_string(),
                metric_value: json!(1),
                threshold: Some(json!(0)),
                instructions: if finding.instructions.is_empty() {
                    vec![
                        format!(
                            "Link {} to its real prerequisites with depends_on edges, or attach it under an existing parent ticket.",
                            display_path
                        ),
                        "If this is otherwise standalone work, create a project-tracker parent ticket that depends_on this ticket so the task is still connected to the broader plan.".to_string(),
                    ]
                } else {
                    finding.instructions.clone()
                },
                evidence: json!({
                    "ticket_id": finding.ticket_id,
                    "path": display_path,
                    "state": finding.state,
                    "type": finding.r#type,
                    "check": finding.check,
                }),
            })
        })
        .collect();
    let convergence_findings =
        collect_convergence_findings(repo_root, &workflow);

    let policy_matchers = load_workspace_policy_matchers(repo_root);
    let policy_excluded_reference_findings =
        collect_policy_excluded_reference_findings(
            repo_root,
            &tickets,
            &edges,
            &policy_matchers,
            Some(&store),
        );

    let orphan_count = orphan_findings.len();
    let convergence_count = convergence_findings.len();
    let policy_excluded_reference_count =
        policy_excluded_reference_findings.len();
    let findings = orphan_findings
        .into_iter()
        .chain(convergence_findings)
        .chain(policy_excluded_reference_findings)
        .collect();
    TicketGraphResult {
        metric: CountMetric {
            status: TrialStatus::Collected,
            count: Some(orphan_count),
            details: Some(ticket_graph_details(
                orphan_count,
                convergence_count,
                policy_excluded_reference_count,
            )),
        },
        findings,
    }
}

fn open_ticket_store(
    repo_root: &Path
) -> Result<TicketStore, TicketGraphResult> {
    match TicketStore::open(repo_root) {
        Ok(store) => Ok(store),
        Err(StorageError::WorkspaceNotFound { path }) =>
            Err(TicketGraphResult {
                metric: CountMetric::unavailable(format!(
                    "ticket store not initialized at {}; skipping ticket dependency topology audit",
                    format_output_path(&path)
                )),
                findings: Vec::new(),
            }),
        Err(err) => Err(TicketGraphResult {
            metric: CountMetric {
                status: TrialStatus::Failed,
                count: None,
                details: Some(format!(
                    "failed to inspect ticket dependency topology: {err}"
                )),
            },
            findings: Vec::new(),
        }),
    }
}

fn load_ticket_graph_inputs(
    store: &TicketStore
) -> Result<
    (Vec<IndexedTicket>, Vec<EdgeRecord>, WorkflowModel),
    TicketGraphResult,
> {
    let tickets = store.list(None, None, None).map_err(failed_result)?;
    let edges = store.list_all_edges().map_err(failed_result)?;
    let workflow = WorkflowModel::build(store, tickets.clone(), edges.clone())
        .map_err(failed_result)?;
    Ok((tickets, edges, workflow))
}

fn collect_convergence_findings(
    repo_root: &Path,
    workflow: &WorkflowModel,
) -> Vec<AuditFinding> {
    let mut findings = Vec::new();
    let mut sorted_ticket_ids = workflow
        .actionable_candidate_ids(None)
        .into_iter()
        .chain(workflow.eligible_candidate_ids(None))
        .collect::<Vec<_>>();
    sorted_ticket_ids.sort();
    sorted_ticket_ids.dedup();

    for ticket_id in sorted_ticket_ids {
        let Some(ticket) = workflow.ticket(&ticket_id) else {
            continue;
        };
        let Some(issues) = workflow.dependency_state_inversions(&ticket_id)
        else {
            continue;
        };
        let dependent_path = relative_ticket_path(repo_root, &ticket.path);
        let dependent_title = ticket
            .title
            .clone()
            .unwrap_or_else(|| ticket.id.to_string());

        for issue in issues {
            let prerequisite_path =
                workflow.ticket(&issue.prerequisite_id).map(|prerequisite| {
                    relative_ticket_path(repo_root, &prerequisite.path)
                });
            findings.push(AuditFinding {
                id: format!(
                    "ticket_graph:convergence:{}:{}",
                    issue.dependent_id, issue.prerequisite_id
                ),
                category: "ticket_graph".to_string(),
                severity: convergence_severity(issue.dependent_state.as_deref()),
                summary: format!(
                    "{} depends on {} while the prerequisite is in an earlier workflow state.",
                    dependent_title,
                    issue
                        .prerequisite_title
                        .clone()
                        .unwrap_or_else(|| issue.prerequisite_id.to_string())
                ),
                path: Some(dependent_path.clone()),
                line: None,
                metric_name: "dependency_convergence_count".to_string(),
                metric_value: json!(1),
                threshold: Some(json!(0)),
                instructions: vec![
                    format!(
                        "Advance {} before continuing work on {} when the dependency is still real.",
                        prerequisite_path
                            .clone()
                            .unwrap_or_else(|| issue.prerequisite_id.to_string()),
                        dependent_path
                    ),
                    "If the dependent moved ahead intentionally, document the exception or correct the ticket states so the dependency order is explicit.".to_string(),
                ],
                evidence: json!({
                    "dependent_id": issue.dependent_id,
                    "dependent_path": dependent_path,
                    "dependent_title": issue.dependent_title,
                    "dependent_state": issue.dependent_state,
                    "prerequisite_id": issue.prerequisite_id,
                    "prerequisite_path": prerequisite_path,
                    "prerequisite_title": issue.prerequisite_title,
                    "prerequisite_state": issue.prerequisite_state,
                    "dependency_state_gap": issue.dependency_state_gap,
                    "affected_reverse_dependent_reach": issue.affected_reverse_dependent_reach,
                    "transitive_reverse_dependents": issue.transitive_reverse_dependents,
                }),
            });
        }
    }

    findings
}

fn ticket_graph_details(
    orphan_count: usize,
    convergence_count: usize,
    policy_excluded_reference_count: usize,
) -> String {
    if orphan_count == 0 {
        if convergence_count == 0 {
            "all tickets participate in at least one depends_on relationship"
                .to_string()
        } else {
            format!(
                "all tickets participate in at least one depends_on relationship; {convergence_count} dependency convergence finding(s) detected; {policy_excluded_reference_count} policy-excluded workspace reference finding(s) detected"
            )
        }
    } else {
        format!(
            "{orphan_count} ticket(s) have neither outgoing dependencies nor incoming dependees; {convergence_count} dependency convergence finding(s) detected; {policy_excluded_reference_count} policy-excluded workspace reference finding(s) detected"
        )
    }
}

fn load_workspace_policy_matchers(repo_root: &Path) -> WorkspacePolicyMatchers {
    let policy_path = repo_root.join(".ticket").join("workspace-policy.toml");
    let mut parsed = WorkspacePolicyFile {
        ignore_markers: default_ignore_markers(),
        ..WorkspacePolicyFile::default()
    };

    if let Ok(raw) = fs::read_to_string(&policy_path)
        && let Ok(file) = toml::from_str::<WorkspacePolicyFile>(&raw)
    {
        parsed = file;
        if parsed.ignore_markers.is_empty() {
            parsed.ignore_markers = default_ignore_markers();
        }
    }

    let mut ignore_builder = GitignoreBuilder::new(repo_root);
    for rule in &parsed.ignore_workspaces {
        let _ = ignore_builder.add_line(None, rule);
    }
    let ignore = ignore_builder
        .build()
        .unwrap_or_else(|_| Gitignore::empty());

    let mut include_builder = GitignoreBuilder::new(repo_root);
    for rule in &parsed.include_overrides {
        let _ = include_builder.add_line(None, rule);
    }
    let include = include_builder
        .build()
        .unwrap_or_else(|_| Gitignore::empty());

    WorkspacePolicyMatchers {
        repo_root: repo_root.to_path_buf(),
        ignore,
        include,
        ignore_markers: parsed.ignore_markers,
        deny_external_paths: parsed.deny_external_paths.unwrap_or(true),
    }
}

fn collect_policy_excluded_reference_findings(
    repo_root: &Path,
    tickets: &[IndexedTicket],
    edges: &[EdgeRecord],
    policy_matchers: &WorkspacePolicyMatchers,
    store: Option<&TicketStore>,
) -> Vec<AuditFinding> {
    let ticket_map = tickets
        .iter()
        .map(|ticket| (ticket.id, ticket))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut findings = Vec::new();

    for edge in edges {
        let source_ticket;
        let source = if let Some(ticket) = ticket_map.get(&edge.from) {
            *ticket
        } else if let Some(store) = store {
            let Ok(found) = store.get_indexed(&edge.from) else {
                continue;
            };
            let Some(found) = found else {
                continue;
            };
            source_ticket = found;
            &source_ticket
        } else {
            continue;
        };

        let target_ticket;
        let target = if let Some(ticket) = ticket_map.get(&edge.to) {
            *ticket
        } else if let Some(store) = store {
            let Ok(found) = store.get_indexed(&edge.to) else {
                continue;
            };
            let Some(found) = found else {
                continue;
            };
            target_ticket = found;
            &target_ticket
        } else {
            continue;
        };

        let Some(source_workspace_root) = ticket_workspace_root(&source.path)
        else {
            continue;
        };
        let Some(target_workspace_root) = ticket_workspace_root(&target.path)
        else {
            continue;
        };

        let source_exclusion_reason =
            exclusion_reason(&source_workspace_root, policy_matchers);
        let target_exclusion_reason =
            exclusion_reason(&target_workspace_root, policy_matchers);

        let Some(target_reason) = target_exclusion_reason else {
            continue;
        };
        if source_exclusion_reason.is_some() {
            continue;
        }
        if source_workspace_root == target_workspace_root {
            continue;
        }

        let source_path = relative_ticket_path(repo_root, &source.path);
        let target_path = relative_ticket_path(repo_root, &target.path);
        let source_workspace_path = format_output_path(&source_workspace_root);
        let target_workspace_path = format_output_path(&target_workspace_root);
        findings.push(AuditFinding {
            id: format!(
                "ticket_graph:policy_excluded_reference:{}:{}:{}",
                edge.kind, source.id, target.id
            ),
            category: "ticket_graph".to_string(),
            severity: orphan_severity(source.state.as_deref()),
            summary: format!(
                "{} references {} in policy-excluded workspace {}.",
                source
                    .title
                    .as_deref()
                    .unwrap_or("source ticket"),
                target
                    .title
                    .as_deref()
                    .unwrap_or("target ticket"),
                target_workspace_path
            ),
            path: Some(source_path.clone()),
            line: None,
            metric_name: "policy_excluded_reference_count".to_string(),
            metric_value: json!(1),
            threshold: Some(json!(0)),
            instructions: vec![
                format!(
                    "Remove or retarget the '{}' edge from {} to {} so non-excluded workspaces do not reference policy-excluded workspace tickets.",
                    edge.kind, source_path, target_path
                ),
                "If this relationship is intentional, move both tickets into the same allowed workspace or update policy so the target workspace is no longer excluded.".to_string(),
            ],
            evidence: json!({
                "edge_kind": edge.kind,
                "source_ticket_id": source.id,
                "source_path": source_path,
                "source_workspace_root": source_workspace_path,
                "target_ticket_id": target.id,
                "target_path": target_path,
                "target_workspace_root": target_workspace_path,
                "policy_exclusion_reason": target_reason,
            }),
        });
    }

    findings
}

fn ticket_workspace_root(ticket_path: &Path) -> Option<PathBuf> {
    ticket_path
        .ancestors()
        .find(|ancestor| {
            ancestor.file_name().is_some_and(|name| name == ".ticket")
        })
        .and_then(Path::parent)
        .map(Path::to_path_buf)
}

fn exclusion_reason(
    workspace_root: &Path,
    policy_matchers: &WorkspacePolicyMatchers,
) -> Option<String> {
    let workspace_path_text = format_output_path(workspace_root);
    if workspace_path_text.contains("/test-fixtures/") {
        return Some("default:test-fixtures".to_string());
    }

    if policy_matchers.deny_external_paths
        && !workspace_root.starts_with(&policy_matchers.repo_root)
    {
        return Some("policy:deny_external_paths".to_string());
    }

    if policy_matchers
        .include
        .matched_path_or_any_parents(workspace_root, true)
        .is_ignore()
    {
        return None;
    }

    if policy_matchers
        .ignore
        .matched_path_or_any_parents(workspace_root, true)
        .is_ignore()
    {
        return Some("policy:ignore_workspaces".to_string());
    }

    for marker in &policy_matchers.ignore_markers {
        if workspace_root.join(marker).exists() {
            return Some(format!("policy:ignore_marker:{marker}"));
        }
    }

    None
}

fn failed_result(err: StorageError) -> TicketGraphResult {
    TicketGraphResult {
        metric: CountMetric {
            status: TrialStatus::Failed,
            count: None,
            details: Some(format!(
                "failed to inspect ticket dependency topology: {err}"
            )),
        },
        findings: Vec::new(),
    }
}

fn orphan_severity(state: Option<&str>) -> Severity {
    match state {
        Some("in-implementation") | Some("in-review") => Severity::High,
        _ => Severity::Medium,
    }
}

fn convergence_severity(state: Option<&str>) -> Severity {
    match state {
        Some("in-implementation") | Some("in-review") => Severity::High,
        _ => Severity::Medium,
    }
}

fn relative_ticket_path(
    repo_root: &Path,
    ticket_path: &Path,
) -> String {
    ticket_path
        .strip_prefix(repo_root)
        .map(format_output_path)
        .unwrap_or_else(|_| format_output_path(ticket_path))
}

#[cfg(test)]
#[path = "ticket_graph/tests.rs"]
mod tests;
