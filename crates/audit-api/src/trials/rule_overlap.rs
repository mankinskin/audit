use std::{
    collections::HashSet,
    path::Path,
};

use serde_json::json;

use crate::models::{
    AuditFinding,
    RuleOverlapSummary,
    Severity,
    TrialStatus,
};

const HIGH_OVERLAP_THRESHOLD: f64 = 0.80;
const HIGH_SEVERITY_THRESHOLD: f64 = 0.92;
const MIN_TOKEN_COUNT: usize = 20;
const MAX_FINDINGS: usize = 25;

pub struct RuleOverlapResult {
    pub metric: RuleOverlapSummary,
    pub findings: Vec<AuditFinding>,
}

pub fn evaluate(repo_root: &Path) -> RuleOverlapResult {
    let mut store = match rule_api::RuleStore::open(repo_root) {
        Ok(store) => store,
        Err(rule_api::error::RuleError::Storage(
            memory_kernel::error::StorageError::WorkspaceNotFound { path },
        )) => {
            return RuleOverlapResult {
                metric: RuleOverlapSummary::unavailable(format!(
                    "rule store not initialized at {}; skipping rule-overlap audit",
                    memory_kernel::workspace::normalize_path_for_display(&path)
                )),
                findings: Vec::new(),
            };
        },
        Err(error) => {
            return RuleOverlapResult {
                metric: RuleOverlapSummary::unavailable(format!(
                    "failed to open rule store: {error}"
                )),
                findings: Vec::new(),
            };
        },
    };

    if let Err(error) = store.scan(false) {
        return RuleOverlapResult {
            metric: RuleOverlapSummary::unavailable(format!(
                "failed to scan rule store: {error}"
            )),
            findings: Vec::new(),
        };
    }

    let rules = match store.list(&rule_api::RuleFilter::default(), None) {
        Ok(items) => items,
        Err(error) => {
            return RuleOverlapResult {
                metric: RuleOverlapSummary::unavailable(format!(
                    "failed to list rules: {error}"
                )),
                findings: Vec::new(),
            };
        },
    };

    let fingerprints = rules
        .iter()
        .filter_map(rule_fingerprint)
        .collect::<Vec<_>>();

    if fingerprints.len() < 2 {
        return RuleOverlapResult {
            metric: RuleOverlapSummary::not_applicable(
                "not enough rule bodies with lexical content to compare",
            ),
            findings: Vec::new(),
        };
    }

    let mut compared_pairs = 0usize;
    let mut max_similarity = None;
    let mut overlaps = Vec::new();

    for i in 0..fingerprints.len() {
        for j in (i + 1)..fingerprints.len() {
            compared_pairs += 1;
            let left = &fingerprints[i];
            let right = &fingerprints[j];
            let similarity = jaccard_similarity(&left.tokens, &right.tokens);
            max_similarity =
                Some(max_similarity.map_or(similarity, |current: f64| {
                    current.max(similarity)
                }));

            if similarity >= HIGH_OVERLAP_THRESHOLD {
                overlaps.push((left, right, similarity));
            }
        }
    }

    overlaps.sort_by(|a, b| {
        b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal)
    });

    let findings = overlaps
        .iter()
        .take(MAX_FINDINGS)
        .map(|(left, right, similarity)| {
            let severity = if *similarity >= HIGH_SEVERITY_THRESHOLD {
                Severity::High
            } else {
                Severity::Medium
            };
            AuditFinding {
                id: format!("rule_overlap:{}:{}", left.id, right.id),
                category: "rule_overlap".to_string(),
                severity,
                summary: format!(
                    "Rules '{}' and '{}' have {:.1}% lexical overlap.",
                    left.slug,
                    right.slug,
                    similarity * 100.0
                ),
                path: None,
                line: None,
                metric_name: "lexical_overlap".to_string(),
                metric_value: json!(similarity),
                threshold: Some(json!(HIGH_OVERLAP_THRESHOLD)),
                instructions: vec![
                    format!(
                        "Consolidate shared guidance between '{}' and '{}' into one canonical rule entry, then keep thin references in callers.",
                        left.slug, right.slug
                    ),
                    "Re-run `rule sync-targets --config rule-targets.yaml --check` after deduplicating to verify generated targets remain deterministic.".to_string(),
                ],
                evidence: json!({
                    "left": { "id": left.id, "slug": left.slug, "token_count": left.tokens.len() },
                    "right": { "id": right.id, "slug": right.slug, "token_count": right.tokens.len() },
                    "similarity": similarity,
                }),
            }
        })
        .collect::<Vec<_>>();

    RuleOverlapResult {
        metric: RuleOverlapSummary {
            status: TrialStatus::Collected,
            rules_considered: fingerprints.len(),
            compared_pairs,
            high_overlap_pairs: overlaps.len(),
            max_similarity,
            details: None,
        },
        findings,
    }
}

struct RuleFingerprint {
    id: String,
    slug: String,
    tokens: HashSet<String>,
}

fn rule_fingerprint(rule: &rule_api::RuleManifest) -> Option<RuleFingerprint> {
    let body = rule.body().unwrap_or_default();
    let tokens = tokenize(body);
    if tokens.len() < MIN_TOKEN_COUNT {
        return None;
    }

    Some(RuleFingerprint {
        id: rule.id.to_string(),
        slug: rule.slug().unwrap_or_default().to_string(),
        tokens,
    })
}

fn tokenize(text: &str) -> HashSet<String> {
    text.to_ascii_lowercase()
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|token| token.len() >= 4)
        .map(ToString::to_string)
        .collect()
}

fn jaccard_similarity(
    left: &HashSet<String>,
    right: &HashSet<String>,
) -> f64 {
    if left.is_empty() && right.is_empty() {
        return 1.0;
    }

    let intersection = left.intersection(right).count();
    let union = left.union(right).count();
    if union == 0 {
        0.0
    } else {
        intersection as f64 / union as f64
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::evaluate;

    #[test]
    fn reports_high_overlap_between_near_duplicate_rules() {
        let dir = tempdir().unwrap();
        let mut store = rule_api::RuleStore::init(dir.path()).unwrap();

        let body = "Handoff status must include implementation validation documentation blockers and next steps. Preserve concrete file ownership and explicit command evidence for reviewers and follow-up agents.";
        let near_duplicate = "Handoff status should include implementation validation documentation blockers and next steps. Preserve concrete file ownership plus explicit command evidence for reviewers and follow-up agents.";

        let first = rule_api::RuleManifest::new(
            "shared/prompts/handoff-a",
            "Handoff A",
            ".prompt",
            "handoff",
            body,
        );
        let second = rule_api::RuleManifest::new(
            "shared/prompts/handoff-b",
            "Handoff B",
            ".prompt",
            "handoff",
            near_duplicate,
        );

        store.create(&first, None).unwrap();
        store.create(&second, None).unwrap();

        let result = evaluate(dir.path());
        assert!(matches!(
            result.metric.status,
            crate::models::TrialStatus::Collected
        ));
        assert!(result.metric.high_overlap_pairs >= 1);
        assert!(!result.findings.is_empty());
        assert!(
            result
                .findings
                .iter()
                .any(|finding| finding.category == "rule_overlap")
        );
    }
}
