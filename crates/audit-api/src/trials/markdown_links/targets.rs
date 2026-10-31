//! Target checks: Markdown detection, guidance-set membership and
//! resolution of local destinations to filesystem paths.

use std::path::{Component, Path, PathBuf};

use crate::markdown::decode_percent;

/// Resolves a local `destination` path of the link source `source_rel`
/// (root-relative, `/`-separated) to an absolute path under `repo_root`.
///
/// Compatibility adapter keeping the audit's historical semantics: percent
/// decode, filesystem join to the source directory, full lexical
/// normalization, then a containment check. `Err` carries the normalized path
/// when it leaves the root.
pub(super) fn resolve_target(
    repo_root: &Path,
    source_rel: &str,
    local_path: &str,
) -> Result<PathBuf, PathBuf> {
    let source_dir = source_rel.rsplit_once('/').map_or("", |(dir, _)| dir);
    let normalized = lexical_normalize(
        &repo_root
            .join(source_dir)
            .join(decode_percent(local_path)),
    );
    if normalized.starts_with(repo_root) {
        Ok(normalized)
    } else {
        Err(normalized)
    }
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

pub(super) fn is_markdown_path(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
}

/// The per-repository guidance set (A1): `.agents/**`, `AGENTS.md`,
/// `.github/copilot-instructions.md`, `.claude/**`. Paths are relative to the
/// owning repository root.
pub(super) fn is_guidance_set(relative_path: &str) -> bool {
    let path = relative_path.replace('\\', "/");
    path == "AGENTS.md"
        || path.ends_with("/AGENTS.md")
        || path == ".github/copilot-instructions.md"
        || path == ".agents"
        || path.starts_with(".agents/")
        || path.contains("/.agents/")
        || path == ".claude"
        || path.starts_with(".claude/")
}

pub(super) fn is_guidance_source(relative_path: &str) -> bool {
    is_guidance_set(relative_path)
}

/// Allowed targets of guidance sources: the guidance set, or any `README.md`
/// (D6).
pub(super) fn is_guidance_target(relative_path: &str) -> bool {
    let path = relative_path.replace('\\', "/");
    is_guidance_set(&path) || path == "README.md" || path.ends_with("/README.md")
}

/// Best-effort human-readable subclass for a non-guidance target, used in
/// finding summaries/evidence only; does not affect the blocking decision.
pub(super) fn detect_non_guidance_subclass(target_relative: &str) -> &'static str {
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
