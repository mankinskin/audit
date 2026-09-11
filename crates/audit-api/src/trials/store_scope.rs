use std::path::{
    Path,
    PathBuf,
};

/// Resolve an entity store that physically lives inside the audited selection.
///
/// Store openers resolve their root by walking ancestor directories, which lets
/// an audit of a nested path pick up the surrounding workspace's store. Audits
/// must stay inside the selected path, so resolution is pinned to `repo_root`
/// and returns `None` when no store exists there.
pub fn store_root_within(
    repo_root: &Path,
    dir_name: &str,
) -> Option<PathBuf> {
    let root = memory_kernel::workspace::resolve_store_root_at_fixed_workspace(
        repo_root, dir_name,
    );
    root.is_dir().then_some(root)
}

#[cfg(test)]
#[path = "store_scope/tests.rs"]
mod tests;
