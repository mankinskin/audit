//! Construction of link findings.

use std::path::Path;

use serde_json::{Map, Value, json};

use super::model::LinkClass;
use super::repos::display_relative;
use super::targets::detect_non_guidance_subclass;
use crate::models::{AuditFinding, Severity};

/// Identity of the source of a link, used for finding evidence.
pub(super) struct Src<'a> {
    pub(super) path: &'a str,
    pub(super) guidance: bool,
    pub(super) repository: &'a str,
}

pub(super) fn missing_anchor_finding(
    src: &Src<'_>,
    line: usize,
    destination: &str,
    fragment: &str,
    target_relative: &str,
    revision: Option<String>,
) -> AuditFinding {
    let file = src.path;
    let mut evidence = Map::from_iter([
        ("fragment".to_string(), json!(fragment)),
        ("resolved_target".to_string(), json!(target_relative)),
    ]);
    if let Some(revision) = &revision {
        evidence.insert("revision".to_string(), json!(revision));
    }
    link_finding(
        LinkClass::MissingAnchor,
        src,
        line,
        destination,
        format!(
            "Markdown link in {file}:{line} points to anchor '#{fragment}' which does not exist in '{target_relative}'."
        ),
        vec![format!(
            "Update the fragment to match a current heading slug or HTML id/name in '{target_relative}'."
        )],
        evidence,
    )
}

pub(super) fn external_finding(
    class: LinkClass,
    src: &Src<'_>,
    line: usize,
    destination: &str,
    reason: &str,
    status: Option<u16>,
) -> AuditFinding {
    let file = src.path;
    let mut evidence = Map::from_iter([("reason".to_string(), json!(reason))]);
    if let Some(status) = status {
        evidence.insert("status".to_string(), json!(status));
    }
    link_finding(
        class,
        src,
        line,
        destination,
        format!(
            "External link in {file}:{line} '{destination}' failed the opt-in check: {reason}."
        ),
        vec![format!(
            "Verify '{destination}' is reachable, or replace/remove the link in {file}:{line}."
        )],
        evidence,
    )
}
pub(super) fn link_finding(
    class: LinkClass,
    src: &Src<'_>,
    line: usize,
    destination: &str,
    summary: String,
    instructions: Vec<String>,
    mut evidence: Map<String, Value>,
) -> AuditFinding {
    let file = src.path;
    evidence.insert("source".to_string(), json!(file));
    evidence.insert("line".to_string(), json!(line));
    evidence.insert("target".to_string(), json!(destination));
    evidence.insert("classification".to_string(), json!(class.category()));
    evidence.insert("blocking".to_string(), json!(class.is_blocking()));
    evidence.insert("source_guidance".to_string(), json!(src.guidance));
    evidence.insert("source_repository".to_string(), json!(src.repository));
    AuditFinding {
        id: format!("{}:{file}:{line}:{destination}", class.category()),
        category: class.category().to_string(),
        severity: if class.is_blocking() {
            Severity::High
        } else {
            Severity::Low
        },
        summary,
        path: Some(file.to_string()),
        line: Some(line),
        metric_name: "markdown_link_coherence".to_string(),
        metric_value: json!(destination),
        threshold: None,
        instructions,
        evidence: Value::Object(evidence),
    }
}

pub(super) fn missing_target_finding(
    src: &Src<'_>,
    line: usize,
    destination: &str,
    repo_root: &Path,
    target_path: &Path,
) -> AuditFinding {
    let file = src.path;
    let display_target = display_relative(repo_root, target_path);
    link_finding(
        LinkClass::MissingTarget,
        src,
        line,
        destination,
        format!("Markdown link in {file}:{line} points to missing target '{destination}'."),
        vec![format!(
            "Repair the Markdown link in {file}:{line} so it resolves to an existing target (currently '{display_target}')."
        )],
        Map::from_iter([("resolved_target".to_string(), json!(display_target))]),
    )
}

pub(super) fn non_guidance_finding(
    src: &Src<'_>,
    line: usize,
    destination: &str,
    target_relative: &str,
) -> AuditFinding {
    let file = src.path;
    let detected_class = detect_non_guidance_subclass(target_relative);
    link_finding(
        LinkClass::NonGuidanceTarget,
        src,
        line,
        destination,
        format!(
            "Markdown link in {file}:{line} points to a non-guidance target '{target_relative}' ({detected_class})."
        ),
        vec![format!(
            "Remove the reference to '{target_relative}' from the portable guidance corpus, or replace it with a guidance-corpus pointer; do not link directly to {detected_class}."
        )],
        Map::from_iter([
            ("resolved_target".to_string(), json!(target_relative)),
            ("detected_class".to_string(), json!(detected_class)),
        ]),
    )
}

pub(super) fn cross_repository_finding(
    src: &Src<'_>,
    line: usize,
    destination: &str,
    target_repository: &str,
) -> AuditFinding {
    let file = src.path;
    let source_repo_display = src.repository;
    link_finding(
        LinkClass::CrossRepository,
        src,
        line,
        destination,
        format!(
            "Markdown link in {file}:{line} crosses upward/sideways from repository '{source_repo_display}' to '{target_repository}'."
        ),
        vec![format!(
            "Only the same repository or its descendant submodules may be linked; replace '{destination}' with a link inside the repository or plain text."
        )],
        Map::from_iter([("target_repository".to_string(), json!(target_repository))]),
    )
}

pub(super) fn unsupported_finding(src: &Src<'_>, line: usize, destination: &str) -> AuditFinding {
    let file = src.path;
    link_finding(
        LinkClass::UnsupportedDependency,
        src,
        line,
        destination,
        format!("Markdown link in {file}:{line} uses an unsupported destination '{destination}'."),
        vec![
            "Use a relative repository path, an http(s) URL, or a mailto:/data: destination instead.".to_string(),
        ],
        Map::new(),
    )
}

pub(super) fn unsafe_path_finding(
    src: &Src<'_>,
    line: usize,
    destination: &str,
    target_path: &Path,
) -> AuditFinding {
    let file = src.path;
    link_finding(
        LinkClass::UnsafePath,
        src,
        line,
        destination,
        format!(
            "Markdown link in {file}:{line} resolves outside the repository root ('{destination}')."
        ),
        vec![format!(
            "Repair the Markdown link in {file}:{line} so it stays within the repository."
        )],
        Map::from_iter([(
            "resolved_target".to_string(),
            json!(target_path.to_string_lossy().replace('\\', "/")),
        )]),
    )
}

pub(super) fn unreadable_source_finding(src: &Src<'_>) -> AuditFinding {
    let file = src.path;
    link_finding(
        LinkClass::UnreadableArtifact,
        src,
        0,
        "",
        format!("Markdown source '{file}' could not be read as UTF-8 Markdown."),
        vec![format!(
            "Ensure '{file}' is readable UTF-8 Markdown or exclude it from the audit."
        )],
        Map::new(),
    )
}
