//! Source discovery: which Markdown files are scanned.

use std::path::Path;

use ignore::WalkBuilder;

use super::options::EvaluateOptions;
use super::repos::{Attr, RepoSet};

/// A Markdown file to scan, attributed to its innermost repository.
pub(super) struct SourceFile {
    pub(super) rel: String,
    pub(super) node: usize,
}

/// Per-directory source exclusion file (gitignore syntax, hierarchical, never bypassed).
pub const AUDIT_IGNORE_FILENAME: &str = ".auditignore";

/// Walks the scanned tree (gitignore and `.auditignore` respected, hidden
/// included, symlinks not followed) and attributes every `*.md` to its
/// innermost repository.
/// Files inside unregistered nested repositories are dropped (TD14).
pub(super) fn collect_sources(
    repo_root: &Path,
    options: &EvaluateOptions,
    repos: &mut RepoSet,
) -> Vec<SourceFile> {
    let mut walker = WalkBuilder::new(repo_root);
    walker
        .standard_filters(true)
        .hidden(false)
        .follow_links(false)
        .add_custom_ignore_filename(AUDIT_IGNORE_FILENAME);
    let filter_root = repo_root.to_path_buf();
    let exclude_paths = options.exclude_paths.clone();
    let include_historical = options.include_historical;
    walker.filter_entry(move |entry| {
        let Ok(relative_path) = entry.path().strip_prefix(&filter_root) else {
            return true;
        };
        !is_excluded_path(relative_path, &exclude_paths, include_historical)
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
        let dir = path.parent().unwrap_or(repo_root);
        if let Attr::Node(node) = repos.attribute(dir) {
            files.push(SourceFile {
                rel: relative.to_string_lossy().replace('\\', "/"),
                node,
            });
        }
    }
    files.sort_by(|a, b| a.rel.cmp(&b.rel));
    files
}

pub(super) fn is_excluded_path(
    path: &Path,
    exclude_paths: &[String],
    include_historical: bool,
) -> bool {
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
        if !include_historical && matches!(file_name, "transcripts" | ".workflow-tools") {
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
