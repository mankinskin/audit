use std::{
    fs,
    path::{Component, Path, PathBuf},
};

use ignore::WalkBuilder;
use pulldown_cmark::{Event, Parser, Tag};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::models::{AuditFinding, MarkdownLinkMetric, Severity, TrialStatus};

/// Stable, machine-readable classification for a guidance Markdown link.
///
/// `category()` is the finding `category`/id prefix; `is_blocking()` decides
/// whether the class must fail `audit links` and the pre-commit hook.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkClass {
    /// Local target exists and is part of the portable guidance corpus.
    GuidanceTarget,
    /// Local target does not exist.
    MissingTarget,
    /// Local target exists but is outside the portable guidance corpus
    /// (ticket/spec store, source code, generated artifact, unrelated file).
    NonGuidanceTarget,
    /// Local target exists but crosses a nested-repository boundary.
    /// Recorded rather than silently skipped or accepted; never validated
    /// for existence.
    CrossRepository,
    /// http(s), mailto:, or data: destination; not resolved for existence.
    External,
    /// Pure `#fragment` destination with no path component.
    FragmentOnly,
    /// Destination uses a scheme/shape this audit does not support (e.g. a
    /// non-http(s) scheme, an absolute filesystem path, or a malformed
    /// destination).
    UnsupportedDependency,
    /// Local destination resolves outside the repository root entirely.
    UnsafePath,
    /// The guidance source file could not be read as UTF-8 Markdown.
    UnreadableArtifact,
}

impl LinkClass {
    pub fn category(self) -> &'static str {
        match self {
            LinkClass::GuidanceTarget => "markdown_link_guidance_target",
            LinkClass::MissingTarget => "markdown_link_missing_target",
            LinkClass::NonGuidanceTarget => "markdown_link_non_guidance_target",
            LinkClass::CrossRepository => "markdown_link_cross_repository",
            LinkClass::External => "markdown_link_external",
            LinkClass::FragmentOnly => "markdown_link_fragment_only",
            LinkClass::UnsupportedDependency => "markdown_link_unsupported_dependency",
            LinkClass::UnsafePath => "markdown_link_unsafe_path",
            LinkClass::UnreadableArtifact => "markdown_link_unreadable_artifact",
        }
    }

    /// Blocking classes fail `audit links` and the pre-commit hook.
    pub fn is_blocking(self) -> bool {
        matches!(
            self,
            LinkClass::MissingTarget
                | LinkClass::NonGuidanceTarget
                | LinkClass::UnsupportedDependency
                | LinkClass::UnsafePath
                | LinkClass::UnreadableArtifact
        )
    }
}

pub struct MarkdownLinkResult {
    pub metric: MarkdownLinkMetric,
    pub findings: Vec<AuditFinding>,
}

enum DestinationShape<'a> {
    Local(&'a str),
    External,
    FragmentOnly,
    Unsupported,
}

struct LineIndex {
    newlines: Vec<usize>,
}

impl LineIndex {
    fn new(contents: &str) -> Self {
        let mut newlines = Vec::new();
        for (offset, &byte) in contents.as_bytes().iter().enumerate() {
            if byte == b'\n' {
                newlines.push(offset);
            }
        }
        Self { newlines }
    }

    #[inline]
    fn line_number(&self, byte_offset: usize) -> usize {
        self.newlines.partition_point(|&pos| pos < byte_offset) + 1
    }
}

struct GitRepoCache<'a> {
    repo_root: &'a Path,
    known_roots: std::collections::HashMap<PathBuf, PathBuf>,
}

impl<'a> GitRepoCache<'a> {
    fn new(repo_root: &'a Path) -> Self {
        Self {
            repo_root,
            known_roots: std::collections::HashMap::new(),
        }
    }

    fn repository_root(&mut self, path: &Path) -> PathBuf {
        let parent = path.parent().unwrap_or(self.repo_root);
        if let Some(cached) = self.known_roots.get(parent) {
            return cached.clone();
        }

        let mut current = Some(parent);
        let mut searched = Vec::new();
        let mut resolved = self.repo_root.to_path_buf();

        while let Some(candidate) = current {
            if candidate == self.repo_root {
                resolved = self.repo_root.to_path_buf();
                break;
            }
            if let Some(cached) = self.known_roots.get(candidate) {
                resolved = cached.clone();
                break;
            }
            searched.push(candidate.to_path_buf());
            if candidate.join(".git").exists() {
                resolved = candidate.to_path_buf();
                break;
            }
            current = candidate.parent();
        }

        for dir in searched {
            self.known_roots.insert(dir, resolved.clone());
        }
        self.known_roots.insert(parent.to_path_buf(), resolved.clone());

        resolved
    }
}

pub fn evaluate(repo_root: &Path, exclude_paths: &[String]) -> MarkdownLinkResult {
    let mut git_cache = GitRepoCache::new(repo_root);
    let guidance_files = guidance_files_with_cache(repo_root, exclude_paths, &mut git_cache);
    let mut links_checked = 0usize;
    let mut broken_links = 0usize;
    let mut skipped_links = 0usize;
    let mut non_guidance_links = 0usize;
    let mut cross_repository_links = 0usize;
    let mut unsupported_dependency_links = 0usize;
    let mut unsafe_path_links = 0usize;
    let mut unreadable_links = 0usize;
    let mut findings = Vec::new();
    let mut existence_cache = std::collections::HashMap::<PathBuf, bool>::new();

    for file in &guidance_files {
        let source_path = repo_root.join(file);
        let Ok(contents) = fs::read_to_string(&source_path) else {
            unreadable_links += 1;
            findings.push(unreadable_source_finding(file));
            continue;
        };
        let line_index = LineIndex::new(&contents);
        let source_repository = git_cache.repository_root(&source_path);

        for (event, range) in Parser::new(&contents).into_offset_iter() {
            let Event::Start(Tag::Link { dest_url, .. }) = event else {
                continue;
            };
            let destination = dest_url.as_ref();
            let line = line_index.line_number(range.start);

            let local_path = match classify_destination(destination) {
                DestinationShape::External => {
                    skipped_links += 1;
                    continue;
                }
                DestinationShape::FragmentOnly => {
                    skipped_links += 1;
                    continue;
                }
                DestinationShape::Unsupported => {
                    skipped_links += 1;
                    unsupported_dependency_links += 1;
                    findings.push(unsupported_finding(file, line, destination));
                    continue;
                }
                DestinationShape::Local(path) => path,
            };

            let target_path = lexical_normalize(
                &source_path
                    .parent()
                    .unwrap_or(repo_root)
                    .join(decode_percent_escapes(local_path)),
            );

            if !target_path.starts_with(repo_root) {
                skipped_links += 1;
                unsafe_path_links += 1;
                findings.push(unsafe_path_finding(file, line, destination, &target_path));
                continue;
            }

            let target_repository = git_cache.repository_root(&target_path);
            if target_repository != source_repository {
                skipped_links += 1;
                cross_repository_links += 1;
                findings.push(cross_repository_finding(
                    file,
                    line,
                    destination,
                    repo_root,
                    &source_repository,
                    &target_repository,
                ));
                continue;
            }

            links_checked += 1;
            let target_exists = match existence_cache.get(&target_path) {
                Some(&exists) => exists,
                None => {
                    let exists = target_path.exists();
                    existence_cache.insert(target_path.clone(), exists);
                    exists
                }
            };

            if !target_exists {
                broken_links += 1;
                findings.push(missing_target_finding(
                    file,
                    line,
                    destination,
                    repo_root,
                    &target_path,
                ));
                continue;
            }

            let target_relative = target_path
                .strip_prefix(repo_root)
                .unwrap_or(&target_path)
                .to_string_lossy()
                .replace('\\', "/");

            if !is_guidance_target(&target_relative) {
                non_guidance_links += 1;
                findings.push(non_guidance_finding(
                    file,
                    line,
                    destination,
                    &target_relative,
                ));
            }
        }
    }

    let blocking_findings = broken_links
        + non_guidance_links
        + unsupported_dependency_links
        + unsafe_path_links
        + unreadable_links;

    MarkdownLinkResult {
        metric: MarkdownLinkMetric {
            status: TrialStatus::Collected,
            markdown_files: guidance_files.len(),
            links_checked,
            broken_links,
            skipped_links,
            blocking_findings,
            non_guidance_links,
            cross_repository_links,
            unsupported_dependency_links,
            unsafe_path_links,
            unreadable_links,
            details: None,
        },
        findings,
    }
}

/// Classifies a raw link destination before any filesystem resolution.
fn classify_destination(destination: &str) -> DestinationShape<'_> {
    let path = destination.split('#').next().unwrap_or(destination);
    if path.is_empty() {
        return DestinationShape::FragmentOnly;
    }
    if path.starts_with("mailto:") || path.starts_with("data:") {
        return DestinationShape::External;
    }
    if path.starts_with("http://") || path.starts_with("https://") {
        return DestinationShape::External;
    }
    if path.starts_with("//") || path.contains("://") {
        return DestinationShape::Unsupported;
    }
    if path.contains('<') || path.contains('>') {
        return DestinationShape::Unsupported;
    }
    if path.starts_with('/') {
        // Absolute filesystem path: not portable across checkouts/machines.
        return DestinationShape::Unsupported;
    }
    DestinationShape::Local(path)
}

/// The portable guidance corpus: `AGENTS.md`/`README.md` at any level (used
/// as guidance indices) and anything under a `.agents/` directory,
/// regardless of extension (permits non-Markdown guidance metadata).
fn is_guidance_target(relative_path: &str) -> bool {
    let path = relative_path.replace('\\', "/");
    path == "AGENTS.md"
        || path.ends_with("/AGENTS.md")
        || path == "README.md"
        || path.ends_with("/README.md")
        || path.starts_with(".agents/")
        || path.contains("/.agents/")
}

/// Best-effort human-readable subclass for a non-guidance target, used in
/// finding summaries/evidence only; does not affect the blocking decision.
fn detect_non_guidance_subclass(target_relative: &str) -> &'static str {
    if target_relative.starts_with(".ticket/") || target_relative.contains("/.ticket/") {
        "ticket_store"
    } else if target_relative.starts_with(".spec/") || target_relative.contains("/.spec/") {
        "spec_store"
    } else if target_relative.starts_with("target/") || target_relative.contains("/target/") {
        "generated_artifact"
    } else {
        match Path::new(target_relative)
            .extension()
            .and_then(|ext| ext.to_str())
        {
            Some(
                "rs" | "ts" | "tsx" | "js" | "jsx" | "py" | "toml" | "json" | "sh" | "ps1" | "yml"
                | "yaml",
            ) => "source_code",
            _ => "unrelated_repository_file",
        }
    }
}

fn build_finding(
    class: LinkClass,
    severity: Severity,
    file: &str,
    line: usize,
    destination: &str,
    summary: String,
    instructions: Vec<String>,
    mut evidence: Map<String, Value>,
) -> AuditFinding {
    evidence.insert("source".to_string(), json!(file));
    evidence.insert("line".to_string(), json!(line));
    evidence.insert("target".to_string(), json!(destination));
    evidence.insert("classification".to_string(), json!(class.category()));
    AuditFinding {
        id: format!("{}:{file}:{line}:{destination}", class.category()),
        category: class.category().to_string(),
        severity,
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

fn missing_target_finding(
    file: &str,
    line: usize,
    destination: &str,
    repo_root: &Path,
    target_path: &Path,
) -> AuditFinding {
    let display_target = display_relative(repo_root, target_path);
    build_finding(
        LinkClass::MissingTarget,
        Severity::High,
        file,
        line,
        destination,
        format!("Markdown link in {file}:{line} points to missing target '{destination}'."),
        vec![format!(
            "Repair the Markdown link in {file}:{line} so it resolves to an existing target (currently '{display_target}')."
        )],
        Map::from_iter([("resolved_target".to_string(), json!(display_target))]),
    )
}

fn non_guidance_finding(
    file: &str,
    line: usize,
    destination: &str,
    target_relative: &str,
) -> AuditFinding {
    let detected_class = detect_non_guidance_subclass(target_relative);
    build_finding(
        LinkClass::NonGuidanceTarget,
        Severity::High,
        file,
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

fn cross_repository_finding(
    file: &str,
    line: usize,
    destination: &str,
    repo_root: &Path,
    source_repository: &Path,
    target_repository: &Path,
) -> AuditFinding {
    let source_repo_display = display_relative(repo_root, source_repository);
    let target_repo_display = display_relative(repo_root, target_repository);
    build_finding(
        LinkClass::CrossRepository,
        Severity::Low,
        file,
        line,
        destination,
        format!(
            "Markdown link in {file}:{line} crosses a repository boundary from '{source_repo_display}' to '{target_repo_display}'."
        ),
        vec![format!(
            "Confirm the cross-repository reference to '{destination}' is intentional; nested-repository links are not validated for existence."
        )],
        Map::from_iter([
            ("source_repository".to_string(), json!(source_repo_display)),
            ("target_repository".to_string(), json!(target_repo_display)),
        ]),
    )
}

fn unsupported_finding(file: &str, line: usize, destination: &str) -> AuditFinding {
    build_finding(
        LinkClass::UnsupportedDependency,
        Severity::High,
        file,
        line,
        destination,
        format!("Markdown link in {file}:{line} uses an unsupported destination '{destination}'."),
        vec![
            "Use a relative guidance-corpus path, an http(s) URL, or a mailto:/data: destination instead.".to_string(),
        ],
        Map::new(),
    )
}

fn unsafe_path_finding(
    file: &str,
    line: usize,
    destination: &str,
    target_path: &Path,
) -> AuditFinding {
    build_finding(
        LinkClass::UnsafePath,
        Severity::High,
        file,
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

fn unreadable_source_finding(file: &str) -> AuditFinding {
    build_finding(
        LinkClass::UnreadableArtifact,
        Severity::Medium,
        file,
        0,
        "",
        format!("Guidance source '{file}' could not be read as UTF-8 Markdown."),
        vec![format!(
            "Ensure '{file}' is readable UTF-8 Markdown or remove it from the guidance corpus."
        )],
        Map::new(),
    )
}

fn display_relative(repo_root: &Path, path: &Path) -> String {
    if path == repo_root {
        ".".to_string()
    } else {
        path.strip_prefix(repo_root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/")
    }
}

#[cfg(test)]
#[allow(dead_code)]
fn guidance_files(repo_root: &Path, exclude_paths: &[String]) -> Vec<String> {
    let mut git_cache = GitRepoCache::new(repo_root);
    guidance_files_with_cache(repo_root, exclude_paths, &mut git_cache)
}

fn guidance_files_with_cache(
    repo_root: &Path,
    exclude_paths: &[String],
    git_cache: &mut GitRepoCache<'_>,
) -> Vec<String> {
    let mut walker = WalkBuilder::new(repo_root);
    walker.standard_filters(true).hidden(false);
    let filter_root = repo_root.to_path_buf();
    let exclude_paths = exclude_paths.to_vec();
    walker.filter_entry(move |entry| {
        let Ok(relative_path) = entry.path().strip_prefix(&filter_root) else {
            return true;
        };
        !is_excluded_path(relative_path, &exclude_paths)
    });

    let mut files = Vec::new();
    for entry in walker.build().filter_map(Result::ok) {
        if !entry.file_type().is_some_and(|ft| ft.is_file()) {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("md") {
            continue;
        }
        let Ok(relative) = path.strip_prefix(repo_root) else {
            continue;
        };
        let relative_str = relative.to_string_lossy().replace('\\', "/");
        if is_guidance_markdown(&relative_str) && git_cache.repository_root(path) == repo_root {
            files.push(relative_str);
        }
    }
    files
}

fn is_excluded_path(path: &Path, exclude_paths: &[String]) -> bool {
    if let Some(file_name) = path.file_name().and_then(|name| name.to_str()) {
        if matches!(
            file_name,
            ".git"
                | "target"
                | "node_modules"
                | ".audit"
                | ".idea"
                | ".vscode"
                | ".ticket"
                | ".spec"
                | ".rule"
                | ".test"
                | ".session"
                | ".feedback"
                | ".worktrees"
                | ".cargo"
                | "dist"
                | "build"
        ) {
            return true;
        }
    }

    if !exclude_paths.is_empty() {
        let path_str = path.to_string_lossy();
        let normalized = path_str.replace('\\', "/");
        let trimmed = normalized.trim_matches('/');
        if exclude_paths.iter().any(|excluded| {
            let excluded = excluded.trim_matches('/');
            !excluded.is_empty()
                && (trimmed == excluded || trimmed.starts_with(&format!("{excluded}/")))
        }) {
            return true;
        }
    }

    false
}

fn is_guidance_markdown(path: &str) -> bool {
    let path = path.replace('\\', "/");
    path.ends_with(".md")
        && (path == "AGENTS.md"
            || path.ends_with("/AGENTS.md")
            || path == ".agents/README.md"
            || path.starts_with(".agents/")
            || path.contains("/.agents/"))
}

#[cfg(test)]
#[allow(dead_code)]
fn repository_root(repo_root: &Path, path: &Path) -> PathBuf {
    let mut current = path.parent();
    while let Some(candidate) = current {
        if candidate == repo_root {
            break;
        }
        if candidate.join(".git").exists() {
            return candidate.to_path_buf();
        }
        current = candidate.parent();
    }
    repo_root.to_path_buf()
}

fn lexical_normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            component => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

fn decode_percent_escapes(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = String::with_capacity(value.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let high = hex_digit(bytes[index + 1]);
            let low = hex_digit(bytes[index + 2]);
            if let (Some(high), Some(low)) = (high, low) {
                decoded.push(char::from((high << 4) | low));
                index += 3;
                continue;
            }
        }
        decoded.push(char::from(bytes[index]));
        index += 1;
    }
    decoded
}

fn hex_digit(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::{LinkClass, evaluate};

    #[test]
    fn reports_broken_guidance_links_and_skips_non_local_links() {
        let repo = tempdir().unwrap();
        std::fs::create_dir_all(repo.path().join(".agents/instructions")).unwrap();
        std::fs::write(repo.path().join("README.md"), "# Repo\n").unwrap();
        std::fs::write(
            repo.path().join(".agents/instructions/links.md"),
            "[good](../../README.md)\n[bad](missing.md)\n[web](https://example.com)\n[anchor](#section)\n",
        )
        .unwrap();

        let result = evaluate(repo.path(), &[]);

        assert_eq!(result.metric.markdown_files, 1);
        assert_eq!(result.metric.links_checked, 2);
        assert_eq!(result.metric.broken_links, 1);
        assert_eq!(result.metric.skipped_links, 2);
        assert_eq!(result.metric.blocking_findings, 1);
        assert_eq!(result.findings[0].line, Some(2));
        assert_eq!(
            result.findings[0].path.as_deref(),
            Some(".agents/instructions/links.md")
        );
        assert_eq!(
            result.findings[0].category,
            LinkClass::MissingTarget.category()
        );
    }

    #[test]
    fn classifies_nested_repository_targets_instead_of_silently_skipping() {
        let repo = tempdir().unwrap();
        std::fs::create_dir_all(repo.path().join(".agents")).unwrap();
        std::fs::create_dir_all(repo.path().join("nested/.git")).unwrap();
        std::fs::write(repo.path().join("nested/README.md"), "# Nested\n").unwrap();
        std::fs::write(
            repo.path().join(".agents/links.md"),
            "[nested](../nested/README.md)\n",
        )
        .unwrap();

        let result = evaluate(repo.path(), &[]);

        assert_eq!(result.metric.links_checked, 0);
        assert_eq!(result.metric.skipped_links, 1);
        assert_eq!(result.metric.cross_repository_links, 1);
        assert_eq!(result.metric.blocking_findings, 0);
        assert_eq!(result.findings.len(), 1);
        assert_eq!(
            result.findings[0].category,
            LinkClass::CrossRepository.category()
        );
        assert_eq!(result.findings[0].evidence["source_repository"], ".");
        assert_eq!(result.findings[0].evidence["target_repository"], "nested");
    }

    #[test]
    fn blocks_links_to_non_guidance_local_targets() {
        let repo = tempdir().unwrap();
        std::fs::create_dir_all(repo.path().join(".agents")).unwrap();
        std::fs::create_dir_all(repo.path().join("src")).unwrap();
        std::fs::write(repo.path().join("src/lib.rs"), "fn main() {}\n").unwrap();
        std::fs::write(
            repo.path().join(".agents/links.md"),
            "[code](../src/lib.rs)\n",
        )
        .unwrap();

        let result = evaluate(repo.path(), &[]);

        assert_eq!(result.metric.non_guidance_links, 1);
        assert_eq!(result.metric.blocking_findings, 1);
        assert_eq!(
            result.findings[0].category,
            LinkClass::NonGuidanceTarget.category()
        );
        assert_eq!(result.findings[0].evidence["detected_class"], "source_code");
    }

    #[test]
    fn blocks_unsupported_and_unsafe_destinations() {
        let repo = tempdir().unwrap();
        std::fs::create_dir_all(repo.path().join(".agents")).unwrap();
        std::fs::write(
            repo.path().join(".agents/links.md"),
            "[ftp](ftp://example.com/file)\n[escape](../../../../outside.md)\n",
        )
        .unwrap();

        let result = evaluate(repo.path(), &[]);

        assert_eq!(result.metric.unsupported_dependency_links, 1);
        assert_eq!(result.metric.unsafe_path_links, 1);
        assert_eq!(result.metric.blocking_findings, 2);
        let categories: Vec<&str> = result
            .findings
            .iter()
            .map(|finding| finding.category.as_str())
            .collect();
        assert!(categories.contains(&LinkClass::UnsupportedDependency.category()));
        assert!(categories.contains(&LinkClass::UnsafePath.category()));
    }

    #[test]
    fn permits_non_markdown_files_under_agents_directory() {
        let repo = tempdir().unwrap();
        std::fs::create_dir_all(repo.path().join(".agents/skills")).unwrap();
        std::fs::write(repo.path().join(".agents/skills/icon.png"), b"\x89PNG").unwrap();
        std::fs::write(
            repo.path().join(".agents/links.md"),
            "[icon](skills/icon.png)\n",
        )
        .unwrap();

        let result = evaluate(repo.path(), &[]);

        assert_eq!(result.metric.links_checked, 1);
        assert_eq!(result.metric.blocking_findings, 0);
        assert!(result.findings.is_empty());
    }
}
