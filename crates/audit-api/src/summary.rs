use std::{
    collections::{
        BTreeMap,
        BTreeSet,
    },
    path::Path,
};

use cargo_metadata::MetadataCommand;
use serde::{
    Deserialize,
    Serialize,
};

use crate::{
    error::AuditError,
    models::{
        AuditFinding,
        AuditReport,
        Severity,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditSummaryBy {
    Crate,
    Category,
    Severity,
    Metric,
    Path,
}

impl AuditSummaryBy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Crate => "crate",
            Self::Category => "category",
            Self::Severity => "severity",
            Self::Metric => "metric",
            Self::Path => "path",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditSummaryGroup {
    pub key: String,
    pub issues: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditSummaryReport {
    pub repo_root: String,
    pub by: AuditSummaryBy,
    pub total_findings: usize,
    pub repo_wide_issues: usize,
    pub groups: Vec<AuditSummaryGroup>,
    pub unmapped_paths: Vec<AuditSummaryGroup>,
}

pub fn summarize_report(
    report: &AuditReport,
    by: AuditSummaryBy,
) -> Result<AuditSummaryReport, AuditError> {
    let (groups, repo_wide_issues, unmapped_paths) = match by {
        AuditSummaryBy::Crate => summarize_by_crate(report)?,
        AuditSummaryBy::Category => {
            let (groups, repo_wide_issues) =
                summarize_by_key(&report.findings, |finding| {
                    Some(finding.category.clone())
                });
            (groups, repo_wide_issues, Vec::new())
        },
        AuditSummaryBy::Severity => {
            let (groups, repo_wide_issues) =
                summarize_by_key(&report.findings, |finding| {
                    Some(severity_key(&finding.severity).to_string())
                });
            (groups, repo_wide_issues, Vec::new())
        },
        AuditSummaryBy::Metric => {
            let (groups, repo_wide_issues) =
                summarize_by_key(&report.findings, |finding| {
                    Some(finding.metric_name.clone())
                });
            (groups, repo_wide_issues, Vec::new())
        },
        AuditSummaryBy::Path => {
            let (groups, repo_wide_issues) =
                summarize_by_key(&report.findings, |finding| {
                    finding.path.clone()
                });
            (groups, repo_wide_issues, Vec::new())
        },
    };

    Ok(AuditSummaryReport {
        repo_root: report.repo_root.clone(),
        by,
        total_findings: report.findings.len(),
        repo_wide_issues,
        groups,
        unmapped_paths,
    })
}

fn summarize_by_key<F>(
    findings: &[AuditFinding],
    key_fn: F,
) -> (Vec<AuditSummaryGroup>, usize)
where
    F: Fn(&AuditFinding) -> Option<String>,
{
    let mut counts = BTreeMap::<String, usize>::new();
    let mut repo_wide_issues = 0usize;

    for finding in findings {
        if let Some(key) = key_fn(finding) {
            *counts.entry(key).or_default() += 1;
        } else {
            repo_wide_issues += 1;
        }
    }

    (sorted_groups(counts), repo_wide_issues)
}

fn summarize_by_crate(
    report: &AuditReport
) -> Result<(Vec<AuditSummaryGroup>, usize, Vec<AuditSummaryGroup>), AuditError>
{
    let repo_root = Path::new(&report.repo_root).canonicalize()?;
    let package_roots = workspace_package_roots(&repo_root)?;
    let mut counts = BTreeMap::<String, usize>::new();
    let mut repo_wide_issues = 0usize;
    let mut unmapped_paths = BTreeMap::<String, usize>::new();

    for finding in &report.findings {
        match finding.path.as_deref() {
            None => repo_wide_issues += 1,
            Some(path) => {
                if let Some(package_name) =
                    package_for_path(path, &package_roots)
                {
                    *counts.entry(package_name.to_string()).or_default() += 1;
                } else {
                    *unmapped_paths.entry(path.to_string()).or_default() += 1;
                }
            },
        }
    }

    Ok((
        sorted_groups(counts),
        repo_wide_issues,
        sorted_groups(unmapped_paths),
    ))
}

fn sorted_groups(counts: BTreeMap<String, usize>) -> Vec<AuditSummaryGroup> {
    let mut groups = counts
        .into_iter()
        .map(|(key, issues)| AuditSummaryGroup { key, issues })
        .collect::<Vec<_>>();
    groups.sort_by(|left, right| {
        right
            .issues
            .cmp(&left.issues)
            .then_with(|| left.key.cmp(&right.key))
    });
    groups
}

fn severity_key(severity: &Severity) -> &'static str {
    match severity {
        Severity::Low => "low",
        Severity::Medium => "medium",
        Severity::High => "high",
    }
}

fn workspace_package_roots(
    repo_root: &Path
) -> Result<Vec<PackageRoot>, AuditError> {
    if !repo_root.join("Cargo.toml").exists() {
        return Ok(Vec::new());
    }

    let metadata = MetadataCommand::new()
        .current_dir(repo_root)
        .no_deps()
        .exec()
        .map_err(|err| AuditError::CommandFailed {
            command: "cargo metadata --no-deps".to_string(),
            details: normalize_summary_text(err.to_string()),
        })?;

    let mut package_roots = BTreeSet::<PackageRoot>::new();
    for package in metadata.workspace_packages() {
        for target in &package.targets {
            let source_path = target.src_path.as_std_path().canonicalize().ok();
            let Some(source_path) = source_path else {
                continue;
            };
            let Ok(relative_source_path) = source_path.strip_prefix(&repo_root)
            else {
                continue;
            };

            package_roots.insert(PackageRoot {
                name: package.name.to_string(),
                path: normalize_summary_path(relative_source_path),
                recursive: false,
            });

            let Some(source_dir) = relative_source_path.parent() else {
                continue;
            };
            let source_dir = normalize_summary_path(source_dir);
            if source_dir.is_empty() {
                continue;
            }

            package_roots.insert(PackageRoot {
                name: package.name.to_string(),
                path: source_dir,
                recursive: true,
            });
        }
    }

    let mut package_roots = package_roots.into_iter().collect::<Vec<_>>();

    package_roots.sort_by(|left, right| {
        right
            .path
            .len()
            .cmp(&left.path.len())
            .then_with(|| right.recursive.cmp(&left.recursive))
            .then_with(|| left.name.cmp(&right.name))
    });

    Ok(package_roots)
}

fn package_for_path<'a>(
    path: &str,
    package_roots: &'a [PackageRoot],
) -> Option<&'a str> {
    package_roots
        .iter()
        .find(|package_root| package_root.matches(path))
        .map(|package_root| package_root.name.as_str())
}

fn normalize_summary_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn normalize_summary_text(text: String) -> String {
    text.replace('\\', "/")
}

#[derive(Debug, Clone)]
struct PackageRoot {
    name: String,
    path: String,
    recursive: bool,
}

impl PackageRoot {
    fn matches(
        &self,
        path: &str,
    ) -> bool {
        path == self.path
            || (self.recursive
                && path
                    .strip_prefix(self.path.as_str())
                    .is_some_and(|suffix| suffix.starts_with('/')))
    }
}

impl PartialEq for PackageRoot {
    fn eq(
        &self,
        other: &Self,
    ) -> bool {
        self.name == other.name
            && self.path == other.path
            && self.recursive == other.recursive
    }
}

impl Eq for PackageRoot {}

impl PartialOrd for PackageRoot {
    fn partial_cmp(
        &self,
        other: &Self,
    ) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PackageRoot {
    fn cmp(
        &self,
        other: &Self,
    ) -> std::cmp::Ordering {
        self.path
            .cmp(&other.path)
            .then_with(|| self.name.cmp(&other.name))
            .then_with(|| self.recursive.cmp(&other.recursive))
    }
}
