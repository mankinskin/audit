use std::path::Path;

use memory_kernel::error::StorageError;
use serde_json::json;
use spec_api::{
    SpecManifest,
    SpecStore,
    error::SpecError,
};

use crate::{
    config::format_output_path,
    models::{
        AuditFinding,
        Severity,
        SpecFulfillmentSummary,
        TrialStatus,
    },
};

pub struct SpecFulfillmentResult {
    pub metric: SpecFulfillmentSummary,
    pub findings: Vec<AuditFinding>,
}

#[derive(Default)]
struct SpecFulfillmentCounts {
    structured_specs: usize,
    satisfied_specs: usize,
    blocked_specs: usize,
    missed_specs: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FulfillmentDisposition {
    Blocked,
    Missed,
}

pub fn evaluate(repo_root: &Path) -> SpecFulfillmentResult {
    let mut store = match prepare_spec_store(repo_root) {
        Ok(store) => store,
        Err(result) => return result,
    };

    let indexed_specs = match store.entity_store().list_indexed() {
        Ok(indexed) => indexed,
        Err(err) => {
            return SpecFulfillmentResult {
                metric: SpecFulfillmentSummary::failed(format!(
                    "failed to list structured specs: {err}"
                )),
                findings: Vec::new(),
            };
        },
    };

    let mut counts = SpecFulfillmentCounts::default();
    let mut findings = Vec::new();

    for indexed in indexed_specs {
        process_indexed_spec(
            &mut store,
            repo_root,
            &indexed.id.to_string(),
            &indexed.path,
            &mut counts,
            &mut findings,
        );
    }

    let metric = if counts.structured_specs == 0 {
        SpecFulfillmentSummary::not_applicable(
            "no structured expectation-oriented specs found",
        )
    } else {
        SpecFulfillmentSummary {
            status: TrialStatus::Collected,
            structured_specs: counts.structured_specs,
            satisfied_specs: counts.satisfied_specs,
            blocked_specs: counts.blocked_specs,
            missed_specs: counts.missed_specs,
            details: None,
        }
    };

    SpecFulfillmentResult { metric, findings }
}

fn prepare_spec_store(
    repo_root: &Path
) -> Result<spec_api::SpecStore, SpecFulfillmentResult> {
    let mut store = match SpecStore::open(repo_root) {
        Ok(store) => store,
        Err(SpecError::Storage(StorageError::WorkspaceNotFound { path })) => {
            return Err(SpecFulfillmentResult {
                metric: SpecFulfillmentSummary::unavailable(format!(
                    "spec store not initialized at {}; skipping spec fulfillment audit",
                    format_output_path(&path)
                )),
                findings: Vec::new(),
            });
        },
        Err(err) => {
            return Err(SpecFulfillmentResult {
                metric: SpecFulfillmentSummary::failed(format!(
                    "failed to inspect structured specs: {err}"
                )),
                findings: Vec::new(),
            });
        },
    };

    if let Err(err) = store.scan(false) {
        return Err(SpecFulfillmentResult {
            metric: SpecFulfillmentSummary::failed(format!(
                "failed to scan spec store: {err}"
            )),
            findings: Vec::new(),
        });
    }

    Ok(store)
}

fn process_indexed_spec(
    store: &mut SpecStore,
    repo_root: &Path,
    indexed_id: &str,
    indexed_path: &std::path::Path,
    counts: &mut SpecFulfillmentCounts,
    findings: &mut Vec<AuditFinding>,
) {
    let spec = match store.get(indexed_id) {
        Ok(spec) => spec,
        Err(_) => return,
    };
    if !spec.uses_structured_contract() {
        return;
    }

    counts.structured_specs += 1;
    let issues = spec.health_issues();
    if issues.is_empty() {
        counts.satisfied_specs += 1;
        return;
    }

    let disposition = classify_issues(&issues);
    match disposition {
        FulfillmentDisposition::Blocked => counts.blocked_specs += 1,
        FulfillmentDisposition::Missed => counts.missed_specs += 1,
    }

    let display_path = indexed_path
        .strip_prefix(repo_root)
        .map(format_output_path)
        .unwrap_or_else(|_| format_output_path(indexed_path));

    for (issue_index, issue) in issues.iter().enumerate() {
        findings.push(finding_for_issue(
            &spec,
            &display_path,
            issue_index,
            issue,
            disposition,
        ));
    }
}

fn classify_issues(issues: &[String]) -> FulfillmentDisposition {
    if issues.iter().any(|issue| is_blocked_issue(issue)) {
        FulfillmentDisposition::Blocked
    } else {
        FulfillmentDisposition::Missed
    }
}

fn is_blocked_issue(issue: &str) -> bool {
    issue.starts_with("unsatisfied evidence requirement '")
        || issue.contains("missing expected property")
        || issue.contains("missing evidence requirement")
        || issue.contains("missing required evidence")
        || issue.contains("missing expected property links")
}

fn finding_for_issue(
    spec: &SpecManifest,
    display_path: &str,
    issue_index: usize,
    issue: &str,
    disposition: FulfillmentDisposition,
) -> AuditFinding {
    let severity = match disposition {
        FulfillmentDisposition::Blocked => Severity::High,
        FulfillmentDisposition::Missed => Severity::Medium,
    };
    let summary = if let Some(evidence_requirement_id) = issue_suffix(
        issue,
        "missing fulfillment summary for evidence requirement '",
    ) {
        format!(
            "{} is missing authoritative evidence for requirement '{}'.",
            spec.title().unwrap_or("structured spec"),
            evidence_requirement_id
        )
    } else if let Some(evidence_requirement_id) =
        issue_suffix(issue, "unsatisfied evidence requirement '")
    {
        format!(
            "{} is blocked by unsatisfied evidence requirement '{}'.",
            spec.title().unwrap_or("structured spec"),
            evidence_requirement_id
        )
    } else {
        format!(
            "{} has a structured contract fulfillment issue: {}",
            spec.title().unwrap_or("structured spec"),
            issue
        )
    };

    AuditFinding {
        id: format!("spec_fulfillment:{}:{issue_index}", spec.id),
        category: "spec_fulfillment".to_string(),
        severity,
        summary,
        path: Some(display_path.to_string()),
        line: None,
        metric_name: "spec_fulfillment_issue_count".to_string(),
        metric_value: json!(1),
        threshold: Some(json!(0)),
        instructions: finding_instructions(issue),
        evidence: json!({
            "spec_id": spec.id,
            "spec_slug": spec.slug(),
            "spec_title": spec.title(),
            "issue": issue,
            "evidence_requirement_id": issue_suffix(issue, "missing fulfillment summary for evidence requirement '")
                .or_else(|| issue_suffix(issue, "unsatisfied evidence requirement '")),
        }),
    }
}

fn finding_instructions(issue: &str) -> Vec<String> {
    if let Some(evidence_requirement_id) = issue_suffix(
        issue,
        "missing fulfillment summary for evidence requirement '",
    ) {
        return vec![
            format!(
                "Attach a store-owned doc-api, test-api, or log-api record for evidence requirement '{}' and record the resulting fulfillment status.",
                evidence_requirement_id
            ),
            format!(
                "Update the structured spec contract so evidence requirement '{}' resolves to an explicit satisfied or blocked outcome.",
                evidence_requirement_id
            ),
        ];
    }

    if let Some(evidence_requirement_id) =
        issue_suffix(issue, "unsatisfied evidence requirement '")
    {
        return vec![
            format!(
                "Resolve the blocking evidence for '{}' or capture the blocker explicitly in the owning store metadata.",
                evidence_requirement_id
            ),
            format!(
                "Refresh the structured fulfillment summary for '{}' once the blocker changes.",
                evidence_requirement_id
            ),
        ];
    }

    vec![
        "Repair the structured spec contract fields so acceptance criteria and evidence links resolve cleanly.".to_string(),
        "Prefer store-owned evidence links over free-form rollout prose when explaining why the spec is blocked or incomplete.".to_string(),
    ]
}

fn issue_suffix(
    issue: &str,
    prefix: &str,
) -> Option<String> {
    issue
        .strip_prefix(prefix)
        .and_then(|suffix| suffix.strip_suffix('\''))
        .map(ToString::to_string)
}

#[cfg(test)]
mod tests {
    use spec_api::{
        AcceptanceCriterion,
        EvidenceRequirement,
        ExpectedProperty,
        FulfillmentStatus,
        FulfillmentSubjectKind,
        FulfillmentSummary,
        SpecContractMode,
        SpecManifest,
        SpecStore,
    };
    use tempfile::tempdir;

    use super::evaluate;

    #[test]
    fn evaluates_structured_spec_fulfillment_as_satisfied_blocked_and_missed() {
        let dir = tempdir().unwrap();
        let mut store = SpecStore::init(dir.path()).unwrap();

        store
            .create(
                &make_structured_spec(
                    "Satisfied spec",
                    "spec/satisfied",
                    Some(FulfillmentStatus::Satisfied),
                ),
                "",
                None,
            )
            .unwrap();
        store
            .create(
                &make_structured_spec(
                    "Blocked spec",
                    "spec/blocked",
                    Some(FulfillmentStatus::Blocked),
                ),
                "",
                None,
            )
            .unwrap();
        store
            .create(
                &make_structured_spec("Missing spec", "spec/missing", None),
                "",
                None,
            )
            .unwrap();

        let result = evaluate(dir.path());

        assert_eq!(result.metric.structured_specs, 3);
        assert_eq!(result.metric.satisfied_specs, 1);
        assert_eq!(result.metric.blocked_specs, 1);
        assert_eq!(result.metric.missed_specs, 1);
        assert_eq!(result.findings.len(), 2);
        assert!(result.findings.iter().any(|finding| {
            finding
                .summary
                .contains("blocked by unsatisfied evidence requirement")
        }));
        assert!(result.findings.iter().any(|finding| {
            finding.summary.contains("missing authoritative evidence")
        }));
    }

    fn make_structured_spec(
        title: &str,
        slug: &str,
        status: Option<FulfillmentStatus>,
    ) -> SpecManifest {
        let mut manifest = SpecManifest::new(slug, title, "audit-api");
        manifest.set_contract_mode(Some(SpecContractMode::ExpectationOriented));
        manifest.set_expected_properties(vec![ExpectedProperty {
            id: "prop-visible".to_string(),
            statement: "Visible audit status is explicit.".to_string(),
        }]);
        manifest.set_acceptance_criteria(vec![AcceptanceCriterion {
            id: "criterion-visible".to_string(),
            statement: "Audit status is derived from structured store data."
                .to_string(),
            expected_property_ids: vec!["prop-visible".to_string()],
            required_evidence_ids: vec!["evidence-doc".to_string()],
        }]);
        manifest.set_evidence_requirements(vec![EvidenceRequirement {
            id: "evidence-doc".to_string(),
            kind: "documentation".to_string(),
            description: "Documentation evidence exists.".to_string(),
            optional: false,
        }]);
        if let Some(status) = status {
            manifest.set_fulfillment_summaries(vec![FulfillmentSummary {
                id: format!("summary-{}", slug.replace('/', "-")),
                subject_kind: FulfillmentSubjectKind::EvidenceRequirement,
                subject_id: "evidence-doc".to_string(),
                status,
                detail: Some("Derived during audit rollout tests".to_string()),
            }]);
        }
        manifest
    }
}
