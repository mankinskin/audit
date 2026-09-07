use std::path::Path;

use serde_json::json;

use crate::models::{AuditFinding, Severity, TrialStatus};

const REQUIRED_FILES: [&str; 3] = ["README.md", "INSTALL.md", "CONTRIBUTING.md"];

pub struct RepositoryGuidanceResult {
    pub metric: RepositoryGuidanceMetric,
    pub findings: Vec<AuditFinding>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RepositoryGuidanceMetric {
    pub status: TrialStatus,
    pub expected_files: usize,
    pub present_files: usize,
    pub missing_files: usize,
    pub empty_files: usize,
    pub details: Option<String>,
}

pub fn evaluate(repo_root: &Path) -> RepositoryGuidanceResult {
    let mut findings = Vec::new();
    let mut present_files = 0;
    let mut missing_files = 0;
    let mut empty_files = 0;

    for file_name in REQUIRED_FILES {
        let path = repo_root.join(file_name);
        let relative_path = file_name.to_string();
        let contents = match std::fs::read(&path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                missing_files += 1;
                findings.push(finding(
                    "missing",
                    &relative_path,
                    "is missing from the repository root",
                    json!({"path": relative_path, "state": "missing"}),
                ));
                continue;
            }
            Err(error) => {
                missing_files += 1;
                findings.push(finding(
                    "unreadable",
                    &relative_path,
                    &format!("could not be read: {error}"),
                    json!({"path": relative_path, "state": "unreadable", "error": error.to_string()}),
                ));
                continue;
            }
        };

        if contents.iter().all(u8::is_ascii_whitespace) {
            empty_files += 1;
            findings.push(finding(
                "empty",
                &relative_path,
                "contains no non-whitespace content",
                json!({"path": relative_path, "state": "empty"}),
            ));
        } else {
            present_files += 1;
        }
    }

    RepositoryGuidanceResult {
        metric: RepositoryGuidanceMetric {
            status: TrialStatus::Collected,
            expected_files: REQUIRED_FILES.len(),
            present_files,
            missing_files,
            empty_files,
            details: None,
        },
        findings,
    }
}

fn finding(
    state: &str,
    path: &str,
    description: &str,
    evidence: serde_json::Value,
) -> AuditFinding {
    AuditFinding {
        id: format!("repository_guidance:{state}:{path}"),
        category: "repository_guidance".to_string(),
        severity: Severity::Medium,
        summary: format!("Repository guidance file {path} {description}."),
        path: Some(path.to_string()),
        line: None,
        metric_name: "repository_guidance".to_string(),
        metric_value: json!(state),
        threshold: None,
        instructions: vec![format!(
            "Add non-whitespace content to the repository-root {path} file."
        )],
        evidence,
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::evaluate;

    #[test]
    fn reports_missing_and_empty_guidance_files() {
        let repo = tempdir().unwrap();
        std::fs::write(repo.path().join("README.md"), "# Repo\n").unwrap();
        std::fs::write(repo.path().join("INSTALL.md"), "  \n").unwrap();

        let result = evaluate(repo.path());

        assert_eq!(result.metric.present_files, 1);
        assert_eq!(result.metric.missing_files, 1);
        assert_eq!(result.metric.empty_files, 1);
        assert!(result
            .findings
            .iter()
            .any(|finding| { finding.id == "repository_guidance:empty:INSTALL.md" }));
        assert!(result
            .findings
            .iter()
            .any(|finding| { finding.id == "repository_guidance:missing:CONTRIBUTING.md" }));
    }

    #[test]
    fn accepts_non_empty_guidance_files() {
        let repo = tempdir().unwrap();
        for file_name in ["README.md", "INSTALL.md", "CONTRIBUTING.md"] {
            std::fs::write(repo.path().join(file_name), "content").unwrap();
        }

        let result = evaluate(repo.path());

        assert!(result.findings.is_empty());
        assert_eq!(result.metric.present_files, 3);
        assert_eq!(result.metric.missing_files, 0);
        assert_eq!(result.metric.empty_files, 0);
    }
}
