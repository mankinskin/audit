use std::{
    fs,
    path::Path,
};

use crate::{
    config::is_repo_relative_path_excluded,
    error::AuditError,
};

const GITIGNORE_HEADER: &str =
    "# Excluded local audit index artifacts created by audit-api.";

pub(crate) fn ensure_index_gitignore(
    index_dir: &Path,
    db_filename: &str,
) -> Result<(), AuditError> {
    let gitignore_path = index_dir.join(".gitignore");
    let existing = match fs::read_to_string(&gitignore_path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound =>
            String::new(),
        Err(error) => return Err(error.into()),
    };

    let entries = [db_filename, "audit.sqlite3-shm", "audit.sqlite3-wal"];
    let missing: Vec<&str> = entries
        .iter()
        .copied()
        .filter(|entry| !existing.lines().any(|line| line.trim() == *entry))
        .collect();

    if missing.is_empty() {
        return Ok(());
    }

    let mut updated = existing;
    if updated.is_empty() {
        updated.push_str(GITIGNORE_HEADER);
        updated.push('\n');
    } else {
        if !updated.ends_with('\n') {
            updated.push('\n');
        }
        if !updated.contains(GITIGNORE_HEADER) {
            updated.push('\n');
            updated.push_str(GITIGNORE_HEADER);
            updated.push('\n');
        }
    }

    for entry in missing {
        updated.push_str(entry);
        updated.push('\n');
    }

    fs::write(gitignore_path, updated)?;
    Ok(())
}

pub(crate) fn is_excluded_path(
    relative_path: &Path,
    exclude_paths: &[String],
) -> bool {
    if relative_path.components().any(|component| {
        let value = component.as_os_str().to_string_lossy();
        matches!(
            value.as_ref(),
            ".git" | "target" | "node_modules" | ".audit" | ".idea" | ".vscode"
        )
    }) {
        return true;
    }

    is_repo_relative_path_excluded(relative_path, exclude_paths)
}

pub(crate) fn detect_language(path: &Path) -> Option<&'static str> {
    match path
        .extension()?
        .to_string_lossy()
        .to_ascii_lowercase()
        .as_str()
    {
        "rs" => Some("rust"),
        "ts" | "tsx" => Some("typescript"),
        "js" | "jsx" => Some("javascript"),
        "py" => Some("python"),
        "go" => Some("go"),
        "java" => Some("java"),
        "kt" => Some("kotlin"),
        "c" | "h" => Some("c"),
        "cc" | "cpp" | "cxx" | "hpp" => Some("cpp"),
        _ => None,
    }
}

pub(crate) fn count_lines(content: &[u8]) -> usize {
    if content.is_empty() {
        0
    } else {
        String::from_utf8_lossy(content).lines().count()
    }
}
