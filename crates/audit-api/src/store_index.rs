//! Audit store status summary generator (ticket `855a1e5d`).
//!
//! Converts an [`AuditReport`] into committed catalog artifacts:
//!
//! - `.audit/README.md` — human-browsable status summary grouped by metric
//!   category.
//! - `.audit/index.toon` — machine-readable [`IndexSidecar`] with one root
//!   [`ContentKind::WorkspaceSummary`] entry plus per-category
//!   [`ContentKind::AuditFinding`] summary entries.
//! - `.agents/audit-catalog.md` — agent-hook pointer (D1).
//!
//! Per Q1.1, this normalization lives in the owning domain crate (`audit-api`),
//! not in `memory-api`.
//!
//! # Determinism
//!
//! All artifacts use a fixed epoch `generated_at` and derive digest-field values
//! only from finding counts and categories (never from run timestamps, scan
//! tokens, or mtime). This makes the sidecar byte-stable across consecutive
//! invocations over the same codebase, so `--check` reliably returns
//! `drift: false` when the catalog is current.
//!
//! # Pre-commit hook
//!
//! Per design spec Q6.3, `audit store-index` is intentionally **excluded** from
//! the pre-commit hook because a full audit run is too expensive for commit-time
//! checks. Use it manually or in CI after running `audit run .`.

use std::collections::BTreeMap;

use chrono::{
    DateTime,
    Utc,
};
use uuid::Uuid;

use memory_kernel::{
    ContentKind,
    IndexEntry,
    IndexRef,
    IndexRelations,
    IndexSidecar,
    RelationKind,
    index_generator::deterministic_uuid,
};

use crate::models::{
    AuditFinding,
    AuditReport,
    Severity,
};

/// Provenance comment written at the top of `.audit/README.md`.
///
/// Uses an `-index` suffixed prefix so index/catalog files are never confused
/// with audit *data* files (decision Q2.1 of the `rendering-pipeline-integration`
/// spec).
pub const AUDIT_INDEX_FILE_COMMENT: &str =
    "<!-- audit-index:file generated=true -->";

/// Per-entry provenance prefix (Q2.1). Each marker also carries a digest prefix
/// (Q4.1): `<!-- audit-index:entry id=<uuid> digest=<hex12> -->`.
pub const AUDIT_INDEX_ENTRY_PREFIX: &str = "audit-index:entry";

/// Provenance comment for the generated agent-hook file.
pub const AUDIT_INDEX_AGENT_HOOK_COMMENT: &str =
    "<!-- audit-index:agent-hook generated=true -->";

/// Repository-relative path of the generated agent-hook file (D1).
pub const AUDIT_INDEX_AGENT_HOOK_PATH: &str = ".agents/audit-catalog.md";

/// Namespace UUID for deterministic audit entry UUIDs.
const AUDIT_NS: Uuid = Uuid::from_bytes([
    0xab, 0xcd, 0x12, 0x34, 0x56, 0x78, 0x90, 0xab, 0xcd, 0xef, 0x00, 0x11,
    0x22, 0x33, 0x44, 0x55,
]);

/// The input to the generator: either a completed audit report or nothing.
///
/// `report = None` means no audit has been run yet; the generator emits a
/// "no audit data" placeholder so the committed artifacts always exist and
/// `--check` can verify they are up to date.
pub struct AuditCatalogSource<'a> {
    /// Most recent audit report, or `None` if no run is available.
    pub report: Option<&'a AuditReport>,
    /// Store folder relative to the workspace root (normally `.audit`).
    pub store_dir: &'a str,
}

/// Generated audit catalog artifacts, ready for the caller to write or diff.
pub struct AuditCatalogArtifacts {
    /// Sidecar for `.audit/index.toon`. Entries are sealed and sorted by id.
    pub sidecar: IndexSidecar,
    /// Rendered `.audit/README.md` catalog (LF newlines, single trailing
    /// newline).
    pub readme_markdown: String,
    /// Rendered `.agents/audit-catalog.md` agent-hook content.
    pub agent_hook_markdown: String,
}

/// Fixed, reproducible generation timestamp embedded in every artifact.
fn epoch() -> DateTime<Utc> {
    DateTime::from_timestamp(0, 0).expect("epoch is valid")
}

fn severity_rank(s: &Severity) -> u8 {
    match s {
        Severity::High => 0,
        Severity::Medium => 1,
        Severity::Low => 2,
    }
}

fn stable_finding_key(
    f: &AuditFinding
) -> (u8, &str, &str, Option<&str>, Option<usize>) {
    (
        severity_rank(&f.severity),
        f.id.as_str(),
        f.summary.as_str(),
        f.path.as_deref(),
        f.line,
    )
}

/// Generate the full audit status catalog from the most recent audit run.
pub fn generate_audit_catalog(
    source: &AuditCatalogSource<'_>
) -> AuditCatalogArtifacts {
    let generated_at = epoch();
    match source.report {
        None => generate_no_data_catalog(source.store_dir, generated_at),
        Some(report) =>
            generate_from_report(report, source.store_dir, generated_at),
    }
}

// ---------------------------------------------------------------------------
// No-data path
// ---------------------------------------------------------------------------

fn generate_no_data_catalog(
    store_dir: &str,
    generated_at: DateTime<Utc>,
) -> AuditCatalogArtifacts {
    let root_id =
        deterministic_uuid(AUDIT_NS, &format!("audit-root:{store_dir}"));

    let mut root = IndexEntry {
        id: root_id,
        kind: ContentKind::WorkspaceSummary,
        source_path: format!("{store_dir}/index.toon"),
        title: "Audit summary — no data".to_string(),
        summary: "No audit has been run yet. Run `audit run .` to populate."
            .to_string(),
        keywords: vec!["audit".to_string(), "summary".to_string()],
        scope: None,
        non_goals: None,
        relations: IndexRelations::default(),
        digest: String::new(),
        tags: vec!["audit".to_string(), "no-data".to_string()],
        generated_at,
        source_modified_at: None,
    };
    root.seal();

    let mut sidecar =
        IndexSidecar::new(ContentKind::WorkspaceSummary, store_dir, vec![root]);
    sidecar.generated_at = generated_at;
    sidecar.sort();

    let readme_markdown = render_readme_no_data(store_dir);
    let agent_hook_markdown = render_agent_hook_no_data(store_dir);

    AuditCatalogArtifacts {
        sidecar,
        readme_markdown,
        agent_hook_markdown,
    }
}

// ---------------------------------------------------------------------------
// Report path
// ---------------------------------------------------------------------------

fn generate_from_report(
    report: &AuditReport,
    store_dir: &str,
    generated_at: DateTime<Utc>,
) -> AuditCatalogArtifacts {
    let root_id =
        deterministic_uuid(AUDIT_NS, &format!("audit-root:{store_dir}"));

    // Group findings by category (BTreeMap keeps keys sorted).
    let mut by_category: BTreeMap<String, Vec<&AuditFinding>> = BTreeMap::new();
    for f in &report.findings {
        by_category.entry(f.category.clone()).or_default().push(f);
    }
    for findings in by_category.values_mut() {
        findings.sort_by_key(|f| stable_finding_key(f));
    }

    let total_findings = report.findings.len();
    let high_count = report
        .findings
        .iter()
        .filter(|f| matches!(f.severity, Severity::High))
        .count();
    let medium_count = report
        .findings
        .iter()
        .filter(|f| matches!(f.severity, Severity::Medium))
        .count();
    let low_count = total_findings - high_count - medium_count;

    // Digest-stable root summary — no timestamps.
    let overall_label = if high_count > 0 {
        format!("{high_count} high")
    } else if medium_count > 0 {
        format!("{medium_count} medium")
    } else if total_findings > 0 {
        format!("{total_findings} low")
    } else {
        "clean".to_string()
    };

    let run_summary = format!(
        "{total_findings} findings in {} categories: {high_count} high, \
         {medium_count} medium, {low_count} low",
        by_category.len(),
    );

    // Per-category entries.
    let mut category_entries: Vec<IndexEntry> = by_category
        .iter()
        .map(|(category, findings)| {
            make_category_entry(
                category,
                findings,
                root_id,
                store_dir,
                generated_at,
            )
        })
        .collect();
    for e in &mut category_entries {
        e.seal();
    }

    // Root entry with children per category.
    let mut root = IndexEntry {
        id: root_id,
        kind: ContentKind::WorkspaceSummary,
        source_path: format!("{store_dir}/index.toon"),
        title: format!("Audit summary — {overall_label}"),
        summary: run_summary.clone(),
        keywords: vec!["audit".to_string(), "summary".to_string()],
        scope: None,
        non_goals: None,
        relations: IndexRelations {
            parent: None,
            children: category_entries
                .iter()
                .map(|e| IndexRef {
                    canonical_path: e.source_path.clone(),
                    entry_id: e.id,
                    relation_kind: RelationKind::Child,
                    content_kind: ContentKind::AuditFinding,
                    digest: String::new(),
                    anchor: None,
                })
                .collect(),
            depends_on: vec![],
            related: vec![],
        },
        digest: String::new(),
        tags: vec!["audit".to_string(), "summary".to_string()],
        generated_at,
        source_modified_at: None,
    };
    root.seal();

    let mut entries = vec![root];
    entries.extend(category_entries);

    let mut sidecar =
        IndexSidecar::new(ContentKind::AuditFinding, store_dir, entries);
    sidecar.generated_at = generated_at;
    sidecar.sort();

    let readme_markdown =
        render_readme(report, store_dir, &sidecar, &by_category);
    let agent_hook_markdown = render_agent_hook(store_dir, &sidecar);

    AuditCatalogArtifacts {
        sidecar,
        readme_markdown,
        agent_hook_markdown,
    }
}

fn make_category_entry(
    category: &str,
    findings: &[&AuditFinding],
    parent_id: Uuid,
    store_dir: &str,
    generated_at: DateTime<Utc>,
) -> IndexEntry {
    let entry_id = deterministic_uuid(
        AUDIT_NS,
        &format!("audit-category:{store_dir}:{category}"),
    );

    let high = findings
        .iter()
        .filter(|f| matches!(f.severity, Severity::High))
        .count();
    let medium = findings
        .iter()
        .filter(|f| matches!(f.severity, Severity::Medium))
        .count();
    let low = findings.len() - high - medium;

    let worst = if high > 0 {
        "high"
    } else if medium > 0 {
        "medium"
    } else {
        "low"
    };
    let count = findings.len();
    let title = format!(
        "[{worst}] {category}: {count} finding{}",
        if count == 1 { "" } else { "s" }
    );
    let summary = format!("{high} high, {medium} medium, {low} low");
    let worst_rank = if high > 0 {
        severity_rank(&Severity::High)
    } else if medium > 0 {
        severity_rank(&Severity::Medium)
    } else {
        severity_rank(&Severity::Low)
    };

    let scope_text = findings
        .iter()
        .filter(|f| severity_rank(&f.severity) == worst_rank)
        .min_by_key(|f| stable_finding_key(f))
        .map(|f| f.summary.as_str())
        .unwrap_or(category);

    let mut tags = vec![
        "audit-finding".to_string(),
        category.to_string(),
        worst.to_string(),
    ];
    tags.sort_unstable();
    tags.dedup();

    IndexEntry {
        id: entry_id,
        kind: ContentKind::AuditFinding,
        source_path: format!("{store_dir}/index.toon"),
        title,
        summary,
        keywords: vec!["audit".to_string(), category.to_string()],
        scope: Some(scope_text.to_string()),
        non_goals: None,
        relations: IndexRelations {
            parent: Some(IndexRef {
                canonical_path: format!("{store_dir}/index.toon"),
                entry_id: parent_id,
                relation_kind: RelationKind::Parent,
                content_kind: ContentKind::WorkspaceSummary,
                digest: String::new(),
                anchor: None,
            }),
            children: vec![],
            depends_on: vec![],
            related: vec![],
        },
        digest: String::new(),
        tags,
        generated_at,
        source_modified_at: None,
    }
}

// ---------------------------------------------------------------------------
// Rendering helpers
// ---------------------------------------------------------------------------

fn first12(digest: &str) -> &str {
    &digest[..digest.len().min(12)]
}

fn render_readme_no_data(store_dir: &str) -> String {
    let mut out = String::new();
    out.push_str(AUDIT_INDEX_FILE_COMMENT);
    out.push_str("\n\n## Audit Status\n\n");
    out.push_str("No audit has been run yet.\n\n");
    out.push_str(&format!("Run `audit run .` to populate `{store_dir}/`.\n"));
    out
}

fn render_agent_hook_no_data(store_dir: &str) -> String {
    let mut out = String::new();
    out.push_str(AUDIT_INDEX_AGENT_HOOK_COMMENT);
    out.push_str("\n\n# Audit Catalog\n\n");
    out.push_str(&format!(
        "The audit status catalog will be generated at `{store_dir}/README.md`\n\
         once `audit run .` has been executed and `audit store-index` run.\n"
    ));
    out
}

fn render_readme(
    report: &AuditReport,
    store_dir: &str,
    sidecar: &IndexSidecar,
    by_category: &BTreeMap<String, Vec<&AuditFinding>>,
) -> String {
    let root = sidecar
        .entries
        .iter()
        .find(|e| e.kind == ContentKind::WorkspaceSummary);

    let mut out = String::new();
    out.push_str(AUDIT_INDEX_FILE_COMMENT);
    out.push('\n');

    out.push_str("\n## Audit Status\n");

    if let Some(root_entry) = root {
        out.push('\n');
        out.push_str(&format!(
            "<!-- {prefix} id={id} digest={digest} -->\n",
            prefix = AUDIT_INDEX_ENTRY_PREFIX,
            id = root_entry.id,
            digest = first12(&root_entry.digest),
        ));
        out.push_str("### ");
        out.push_str(&root_entry.title);
        out.push('\n');
        if !root_entry.summary.is_empty() {
            out.push('\n');
            out.push_str(&root_entry.summary);
            out.push('\n');
        }
        out.push('\n');
        out.push_str(&format!(
            "- source files: {}\n",
            report.metrics.source_files
        ));
        out.push_str(&format!(
            "- total lines: {}\n",
            report.metrics.total_lines
        ));
        out.push_str(&format!("- ref: `{store_dir}/index.toon`\n"));
    }

    if !by_category.is_empty() {
        out.push_str("\n## Findings by Category\n");

        for (category, findings) in by_category {
            let category_id = deterministic_uuid(
                AUDIT_NS,
                &format!("audit-category:{store_dir}:{category}"),
            );
            let category_entry =
                sidecar.entries.iter().find(|e| e.id == category_id);

            let high = findings
                .iter()
                .filter(|f| matches!(f.severity, Severity::High))
                .count();
            let medium = findings
                .iter()
                .filter(|f| matches!(f.severity, Severity::Medium))
                .count();
            let low = findings.len() - high - medium;

            out.push('\n');
            if let Some(cat_entry) = category_entry {
                out.push_str(&format!(
                    "<!-- {prefix} id={id} digest={digest} -->\n",
                    prefix = AUDIT_INDEX_ENTRY_PREFIX,
                    id = cat_entry.id,
                    digest = first12(&cat_entry.digest),
                ));
                out.push_str("### ");
                out.push_str(&cat_entry.title);
                out.push('\n');
            } else {
                // Fallback — look up by category name in scope or title.
                let fallback = sidecar.entries.iter().find(|e| {
                    e.kind == ContentKind::AuditFinding
                        && e.title.contains(category.as_str())
                });
                if let Some(fb) = fallback {
                    out.push_str(&format!(
                        "<!-- {prefix} id={id} digest={digest} -->\n",
                        prefix = AUDIT_INDEX_ENTRY_PREFIX,
                        id = fb.id,
                        digest = first12(&fb.digest),
                    ));
                    out.push_str("### ");
                    out.push_str(&fb.title);
                    out.push('\n');
                } else {
                    out.push_str(&format!("### {category}\n"));
                }
            }
            out.push('\n');
            out.push_str(&format!(
                "- findings: {} ({high} high, {medium} medium, {low} low)\n",
                findings.len()
            ));
            out.push_str(&format!("- category: `{category}`\n"));
            out.push_str(&format!("- ref: `{store_dir}/index.toon`\n"));
        }
    }

    out
}

fn render_agent_hook(
    store_dir: &str,
    sidecar: &IndexSidecar,
) -> String {
    let total = sidecar.entries.len().saturating_sub(1); // exclude root
    let finding_entries: Vec<_> = sidecar
        .entries
        .iter()
        .filter(|e| e.kind == ContentKind::AuditFinding)
        .collect();

    let category_list = finding_entries
        .iter()
        .filter_map(|e| {
            e.title
                .split("] ")
                .nth(1)
                .and_then(|s| s.split(':').next())
                .map(str::to_string)
        })
        .collect::<Vec<_>>()
        .join(", ");

    let mut out = String::new();
    out.push_str(AUDIT_INDEX_AGENT_HOOK_COMMENT);
    out.push_str("\n\n# Audit Catalog\n\n");
    out.push_str(&format!(
        "The full audit status catalog is generated at `{store_dir}/README.md`\n\
         (machine-readable sidecar: `{store_dir}/index.toon`).\n\n"
    ));
    out.push_str(&format!("- Finding categories: {total}\n"));
    if !category_list.is_empty() {
        out.push_str(&format!("- Categories: {category_list}\n"));
    }
    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::models::{
        AuditFinding,
        AuditMetrics,
        AuditReport,
        AuditRunInfo,
        CountMetric,
        CoverageSummary,
        FileLengthMetric,
        RuleOverlapSummary,
        Severity,
        SpecFulfillmentSummary,
        StaticMetricsSummary,
        SyncStats,
        TestSummary,
        TrialStatus,
    };

    fn minimal_report(findings: Vec<AuditFinding>) -> AuditReport {
        AuditReport {
            service: "test".to_string(),
            repo_root: "/workspace".to_string(),
            index_database: ".audit/audit.sqlite3".to_string(),
            sync: SyncStats {
                scan_token: "tok".to_string(),
                scanned_files: 1,
                updated_files: 1,
                reused_files: 0,
                pruned_files: 0,
            },
            run: AuditRunInfo {
                run_id: 1,
                started_at: "2026-01-01T00:00:00Z".to_string(),
                finished_at: "2026-01-01T00:01:00Z".to_string(),
                status: "completed".to_string(),
            },
            metrics: AuditMetrics {
                source_files: 10,
                total_lines: 500,
                file_length: FileLengthMetric {
                    threshold: 400,
                    long_files: 0,
                    average_lines: 50.0,
                    max_lines: 200,
                },
                compiler_warnings: CountMetric::unavailable("n/a"),
                test_results: TestSummary::unavailable("n/a"),
                coverage: CoverageSummary::unavailable("n/a"),
                static_metrics: StaticMetricsSummary {
                    status: TrialStatus::Unavailable,
                    threshold: 12,
                    functions_analyzed: 0,
                    parse_failures: 0,
                    high_complexity_functions: 0,
                    average_cyclomatic_complexity: None,
                    max_cyclomatic_complexity: None,
                    details: None,
                },
                spec_fulfillment: SpecFulfillmentSummary::not_applicable("n/a"),
                ticket_graph: CountMetric::unavailable("n/a"),
                session_workflow_graph: CountMetric::unavailable("n/a"),
                rule_overlap: RuleOverlapSummary::not_applicable("n/a"),
            },
            findings,
            instructions: vec![],
        }
    }

    fn finding(
        id: &str,
        category: &str,
        severity: Severity,
    ) -> AuditFinding {
        AuditFinding {
            id: id.to_string(),
            category: category.to_string(),
            severity,
            summary: format!("{id} summary"),
            path: None,
            line: None,
            metric_name: category.to_string(),
            metric_value: json!(1),
            threshold: None,
            instructions: vec![],
            evidence: json!({}),
        }
    }

    #[test]
    fn no_data_produces_placeholder() {
        let source = AuditCatalogSource {
            report: None,
            store_dir: ".audit",
        };
        let artifacts = generate_audit_catalog(&source);
        assert_eq!(artifacts.sidecar.entries.len(), 1);
        let root = &artifacts.sidecar.entries[0];
        assert_eq!(root.kind, ContentKind::WorkspaceSummary);
        assert!(root.is_digest_valid());
        assert!(root.title.contains("no data"));
        assert!(artifacts.readme_markdown.contains(AUDIT_INDEX_FILE_COMMENT));
    }

    #[test]
    fn empty_report_produces_root_only() {
        let report = minimal_report(vec![]);
        let source = AuditCatalogSource {
            report: Some(&report),
            store_dir: ".audit",
        };
        let artifacts = generate_audit_catalog(&source);
        assert_eq!(artifacts.sidecar.entries.len(), 1);
        let root = &artifacts.sidecar.entries[0];
        assert_eq!(root.kind, ContentKind::WorkspaceSummary);
        assert!(root.is_digest_valid());
        assert!(root.title.contains("clean"));
    }

    #[test]
    fn findings_grouped_by_category() {
        let report = minimal_report(vec![
            finding("f1", "file_length", Severity::Low),
            finding("f2", "file_length", Severity::Medium),
            finding("f3", "static_metrics", Severity::High),
        ]);
        let source = AuditCatalogSource {
            report: Some(&report),
            store_dir: ".audit",
        };
        let artifacts = generate_audit_catalog(&source);
        // root + 2 categories
        assert_eq!(artifacts.sidecar.entries.len(), 3);
        let finding_entries: Vec<_> = artifacts
            .sidecar
            .entries
            .iter()
            .filter(|e| e.kind == ContentKind::AuditFinding)
            .collect();
        assert_eq!(finding_entries.len(), 2);
        for e in &finding_entries {
            assert!(e.is_digest_valid());
            assert!(e.relations.parent.is_some());
        }
        // Root has 2 children
        let root = artifacts
            .sidecar
            .entries
            .iter()
            .find(|e| e.kind == ContentKind::WorkspaceSummary)
            .unwrap();
        assert_eq!(root.relations.children.len(), 2);
        assert!(root.is_digest_valid());
    }

    #[test]
    fn catalog_is_byte_stable() {
        let report =
            minimal_report(vec![finding("f1", "file_length", Severity::Low)]);
        let source = AuditCatalogSource {
            report: Some(&report),
            store_dir: ".audit",
        };
        let a = generate_audit_catalog(&source);
        let b = generate_audit_catalog(&source);
        assert_eq!(a.readme_markdown, b.readme_markdown);
        assert_eq!(a.agent_hook_markdown, b.agent_hook_markdown);
        assert_eq!(
            a.sidecar.encode_toon().unwrap(),
            b.sidecar.encode_toon().unwrap()
        );
    }

    #[test]
    fn catalog_is_stable_for_permuted_finding_order() {
        let report_a = minimal_report(vec![
            finding("f1", "file_length", Severity::High),
            finding("f2", "file_length", Severity::High),
            finding("f3", "static_metrics", Severity::Medium),
        ]);
        let report_b = minimal_report(vec![
            finding("f2", "file_length", Severity::High),
            finding("f1", "file_length", Severity::High),
            finding("f3", "static_metrics", Severity::Medium),
        ]);

        let source_a = AuditCatalogSource {
            report: Some(&report_a),
            store_dir: ".audit",
        };
        let source_b = AuditCatalogSource {
            report: Some(&report_b),
            store_dir: ".audit",
        };

        let a = generate_audit_catalog(&source_a);
        let b = generate_audit_catalog(&source_b);

        assert_eq!(a.readme_markdown, b.readme_markdown);
        assert_eq!(a.agent_hook_markdown, b.agent_hook_markdown);
        assert_eq!(
            a.sidecar.encode_toon().unwrap(),
            b.sidecar.encode_toon().unwrap()
        );
    }

    #[test]
    fn readme_has_provenance_and_category_sections() {
        let report =
            minimal_report(vec![finding("f1", "file_length", Severity::High)]);
        let source = AuditCatalogSource {
            report: Some(&report),
            store_dir: ".audit",
        };
        let artifacts = generate_audit_catalog(&source);
        let md = &artifacts.readme_markdown;
        assert!(md.starts_with(AUDIT_INDEX_FILE_COMMENT));
        assert!(md.contains("## Audit Status"));
        assert!(md.contains("## Findings by Category"));
        assert!(md.contains("<!-- audit-index:entry id="));
        assert!(md.contains("digest="));
        assert!(md.contains("file_length"));
    }
}
