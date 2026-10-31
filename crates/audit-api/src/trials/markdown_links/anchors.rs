//! Anchor lookup for link targets (TD7), backed by `markdown-links`.

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;

use git2::Oid;

use super::repos::RepoSet;
use crate::markdown::{Anchors, anchors};

/// Per-target anchor cache keyed by `(repo node, revision?, path)`.
pub(super) type AnchorCache = HashMap<(usize, Option<Oid>, String), Option<Arc<Anchors>>>;

/// Anchors of a Markdown target, computed once per `(repo, revision?, path)`.
/// Submodule targets are read from the blob at the recorded gitlink commit;
/// same-repository targets from the working tree. Unreadable or non-UTF-8
/// targets are not anchor-checked.
pub(super) fn lookup_anchors(
    repos: &mut RepoSet,
    cache: &mut AnchorCache,
    node: usize,
    at_revision: bool,
    path_in_repo: &str,
    target_path: &Path,
) -> Option<Arc<Anchors>> {
    let revision = if at_revision {
        repos.nodes[node].commit
    } else {
        None
    };
    let key_path = if at_revision {
        path_in_repo.to_string()
    } else {
        target_path.to_string_lossy().into_owned()
    };
    let key = (node, revision, key_path);
    if let Some(cached) = cache.get(&key) {
        return cached.clone();
    }
    let contents = if at_revision {
        repos
            .blob_at_revision(node, path_in_repo)
            .ok()
            .flatten()
            .and_then(|bytes| String::from_utf8(bytes).ok())
    } else if target_path.is_file() {
        fs::read_to_string(target_path).ok()
    } else {
        None
    };
    let anchors = contents.map(|text| Arc::new(anchors(&text)));
    cache.insert(key, anchors.clone());
    anchors
}
