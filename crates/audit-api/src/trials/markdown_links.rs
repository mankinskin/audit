use std::{
    fs,
    path::{Component, Path, PathBuf},
};

use ignore::WalkBuilder;
use pulldown_cmark::{Event, Parser, Tag};
use serde_json::json;

use crate::models::{AuditFinding, MarkdownLinkMetric, Severity, TrialStatus};

pub struct MarkdownLinkResult {
    pub metric: MarkdownLinkMetric,
    pub findings: Vec<AuditFinding>,
}

pub fn evaluate(repo_root: &Path, exclude_paths: &[String]) -> MarkdownLinkResult {
    let guidance_files = guidance_files(repo_root, exclude_paths);
    let mut links_checked = 0usize;
    let mut broken_links = 0usize;
    let mut skipped_links = 0usize;
    let mut findings = Vec::new();

    for file in &guidance_files {
        let source_path = repo_root.join(file);
        let Ok(contents) = fs::read_to_string(&source_path) else {
            continue;
        };
        let source_repository = repository_root(repo_root, &source_path);

        for (event, range) in Parser::new(&contents).into_offset_iter() {
            let Event::Start(Tag::Link { dest_url, .. }) = event else {
                continue;
            };
            let destination = dest_url.as_ref();
            let Some(destination) = local_destination(destination) else {
                skipped_links += 1;
                continue;
            };

            let line = contents[..range.start]
                .bytes()
                .filter(|byte| *byte == b'\n')
                .count()
                + 1;
            let target_path = lexical_normalize(
                &source_path
                    .parent()
                    .unwrap_or(repo_root)
                    .join(decode_percent_escapes(destination)),
            );

            if !target_path.starts_with(repo_root)
                || repository_root(repo_root, &target_path) != source_repository
            {
                skipped_links += 1;
                continue;
            }

            links_checked += 1;
            if target_path.exists() {
                continue;
            }

            broken_links += 1;
            let display_target = target_path
                .strip_prefix(repo_root)
                .unwrap_or(&target_path)
                .to_string_lossy()
                .replace('\\', "/");
            findings.push(AuditFinding {
                id: format!(
                    "markdown_link_coherence:{}:{}:{}",
                    file, line, destination
                ),
                category: "markdown_link_coherence".to_string(),
                severity: Severity::High,
                summary: format!(
                    "Markdown link in {}:{} points to missing target '{}'.",
                    file, line, destination
                ),
                path: Some(file.clone()),
                line: Some(line),
                metric_name: "markdown_link_coherence".to_string(),
                metric_value: json!(destination),
                threshold: None,
                instructions: vec![format!(
                    "Repair the Markdown link in {}:{} so it resolves to an existing target (currently '{}').",
                    file, line, display_target
                )],
                evidence: json!({
                    "source": file,
                    "line": line,
                    "target": destination,
                    "resolved_target": display_target,
                }),
            });
        }
    }

    MarkdownLinkResult {
        metric: MarkdownLinkMetric {
            status: TrialStatus::Collected,
            markdown_files: guidance_files.len(),
            links_checked,
            broken_links,
            skipped_links,
            details: None,
        },
        findings,
    }
}

fn guidance_files(repo_root: &Path, exclude_paths: &[String]) -> Vec<String> {
    let mut walker = WalkBuilder::new(repo_root);
    walker.standard_filters(true).hidden(false);
    let repo_root = repo_root.to_path_buf();
    let filter_root = repo_root.clone();
    let exclude_paths = exclude_paths.to_vec();
    walker.filter_entry(move |entry| {
        let Ok(relative_path) = entry.path().strip_prefix(&filter_root) else {
            return true;
        };
        !is_excluded_path(relative_path, &exclude_paths)
    });

    walker
        .build()
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_type()
                .is_some_and(|file_type| file_type.is_file())
        })
        .filter_map(|entry| {
            let relative = entry.path().strip_prefix(&repo_root).ok()?;
            let relative = relative.to_string_lossy().replace('\\', "/");
            (repository_root(&repo_root, entry.path()) == repo_root
                && is_guidance_markdown(&relative))
            .then_some(relative)
        })
        .collect()
}

fn is_excluded_path(path: &Path, exclude_paths: &[String]) -> bool {
    if path.components().any(|component| {
        matches!(
            component.as_os_str().to_string_lossy().as_ref(),
            ".git" | "target" | "node_modules" | ".audit" | ".idea" | ".vscode"
        )
    }) {
        return true;
    }

    let path = path.to_string_lossy().replace('\\', "/");
    exclude_paths.iter().any(|excluded| {
        let excluded = excluded.trim_matches('/');
        !excluded.is_empty() && (path == excluded || path.starts_with(&format!("{excluded}/")))
    })
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

fn local_destination(destination: &str) -> Option<&str> {
    let path = destination.split('#').next().unwrap_or(destination);
    if path.is_empty()
        || path.contains('<')
        || path.contains('>')
        || path.starts_with("//")
        || path.starts_with("/")
        || path.contains("://")
        || path.starts_with("mailto:")
        || path.starts_with("data:")
    {
        return None;
    }
    Some(path)
}

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

    use super::evaluate;

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
        assert_eq!(result.findings[0].line, Some(2));
        assert_eq!(
            result.findings[0].path.as_deref(),
            Some(".agents/instructions/links.md")
        );
    }

    #[test]
    fn skips_targets_inside_nested_repositories() {
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
        assert!(result.findings.is_empty());
    }
}
