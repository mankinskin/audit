//! Repository tree of the scanned root: registered submodules, gitlinks and
//! revision-level target lookups.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use git2::{Oid, Repository, TreeWalkMode, TreeWalkResult};

/// Git file mode of a gitlink (submodule) tree/index entry.
pub(super) const GITLINK_MODE: u32 = 0o160000;

/// One repository in the scanned tree (the scanned root or a registered
/// submodule at any depth).
pub(super) struct RepoNode {
    /// Path relative to the scanned root; `.` for the root.
    pub(super) rel: String,
    pub(super) abs: PathBuf,
    pub(super) parent: Option<usize>,
    /// Commit recorded for this submodule by its parent (`None` for root).
    pub(super) commit: Option<Oid>,
    pub(super) repo: Option<Repository>,
    pub(super) open_attempted: bool,
    /// Cached root tree of `commit` (children only).
    pub(super) tree: Option<Result<Oid, String>>,
    /// Gitlinks as `(path inside this repo, commit)`, loaded once.
    pub(super) gitlinks: Option<Vec<(String, Oid)>>,
}

#[derive(Clone)]
pub(super) enum Attr {
    Node(usize),
    /// Directory of a nested repository lacking a gitlink (TD14).
    Unregistered(PathBuf),
}

pub(super) struct RepoSet {
    pub(super) root: PathBuf,
    pub(super) nodes: Vec<RepoNode>,
    pub(super) by_abs: HashMap<PathBuf, usize>,
    pub(super) attr_cache: HashMap<PathBuf, Attr>,
    pub(super) exists_cache: HashMap<(usize, String), bool>,
}

impl RepoSet {
    pub(super) fn new(root: &Path) -> Self {
        let node = RepoNode {
            rel: ".".to_string(),
            abs: root.to_path_buf(),
            parent: None,
            commit: None,
            repo: None,
            open_attempted: false,
            tree: None,
            gitlinks: None,
        };
        Self {
            root: root.to_path_buf(),
            nodes: vec![node],
            by_abs: HashMap::from([(root.to_path_buf(), 0)]),
            attr_cache: HashMap::new(),
            exists_cache: HashMap::new(),
        }
    }

    pub(super) fn ensure_open(&mut self, idx: usize) -> Result<(), String> {
        let node = &mut self.nodes[idx];
        if !node.open_attempted {
            node.open_attempted = true;
            node.repo = Repository::open(&node.abs).ok();
        }
        if node.repo.is_some() {
            Ok(())
        } else {
            Err(format!(
                "repository '{}' is not initialised or cannot be opened",
                node.rel
            ))
        }
    }

    /// Root tree of the commit recorded for a submodule node.
    pub(super) fn root_tree(&mut self, idx: usize) -> Result<Oid, String> {
        if let Some(cached) = &self.nodes[idx].tree {
            return cached.clone();
        }
        let computed = self.load_root_tree(idx);
        self.nodes[idx].tree = Some(computed.clone());
        computed
    }

    pub(super) fn load_root_tree(&mut self, idx: usize) -> Result<Oid, String> {
        self.ensure_open(idx)?;
        let node = &self.nodes[idx];
        let commit_id = node
            .commit
            .ok_or_else(|| format!("repository '{}' has no recorded revision", node.rel))?;
        let repo = node.repo.as_ref().expect("opened above");
        let commit = repo.find_commit(commit_id).map_err(|_| {
            format!(
                "commit {commit_id} recorded for submodule '{}' is not available locally",
                node.rel
            )
        })?;
        Ok(commit.tree_id())
    }

    /// Gitlinks of a repository: the index for the scanned root (TD1), the
    /// tree at the recorded commit for submodules.
    pub(super) fn gitlinks(&mut self, idx: usize) -> Vec<(String, Oid)> {
        if let Some(cached) = &self.nodes[idx].gitlinks {
            return cached.clone();
        }
        let computed = self.load_gitlinks(idx).unwrap_or_default();
        self.nodes[idx].gitlinks = Some(computed.clone());
        computed
    }

    pub(super) fn load_gitlinks(&mut self, idx: usize) -> Result<Vec<(String, Oid)>, String> {
        let mut links = Vec::new();
        if idx == 0 {
            self.ensure_open(0)?;
            let repo = self.nodes[0].repo.as_ref().expect("opened above");
            let index = repo.index().map_err(|e| e.to_string())?;
            for entry in index.iter() {
                if entry.mode == GITLINK_MODE {
                    links.push((String::from_utf8_lossy(&entry.path).into_owned(), entry.id));
                }
            }
        } else {
            let tree_id = self.root_tree(idx)?;
            let repo = self.nodes[idx].repo.as_ref().expect("opened by root_tree");
            let tree = repo.find_tree(tree_id).map_err(|e| e.to_string())?;
            let _ = tree.walk(TreeWalkMode::PreOrder, |dir, entry| {
                if entry.filemode() as u32 == GITLINK_MODE
                    && let Some(name) = entry.name()
                {
                    links.push((format!("{dir}{name}"), entry.id()));
                }
                TreeWalkResult::Ok
            });
        }
        Ok(links)
    }

    pub(super) fn child_node(&mut self, parent: usize, gitlink_path: &str, commit: Oid) -> usize {
        let abs = self.nodes[parent].abs.join(gitlink_path);
        if let Some(&idx) = self.by_abs.get(&abs) {
            return idx;
        }
        let rel = abs
            .strip_prefix(&self.root)
            .unwrap_or(&abs)
            .to_string_lossy()
            .replace('\\', "/");
        let idx = self.nodes.len();
        self.nodes.push(RepoNode {
            rel,
            abs: abs.clone(),
            parent: Some(parent),
            commit: Some(commit),
            repo: None,
            open_attempted: false,
            tree: None,
            gitlinks: None,
        });
        self.by_abs.insert(abs, idx);
        idx
    }

    /// Innermost repository owning `dir`, or the unregistered nested
    /// repository boundary it sits inside.
    pub(super) fn attribute(&mut self, dir: &Path) -> Attr {
        if let Some(cached) = self.attr_cache.get(dir) {
            return cached.clone();
        }
        let attr = if dir == self.root || !dir.starts_with(&self.root) {
            Attr::Node(0)
        } else {
            let parent = dir.parent().unwrap_or(&self.root).to_path_buf();
            match self.attribute(&parent) {
                Attr::Unregistered(p) => Attr::Unregistered(p),
                Attr::Node(parent_idx) => {
                    if dir.join(".git").exists() {
                        let rel = relative_slash(&self.nodes[parent_idx].abs, dir);
                        let found = self
                            .gitlinks(parent_idx)
                            .into_iter()
                            .find(|(path, _)| *path == rel);
                        match found {
                            Some((path, commit)) => {
                                Attr::Node(self.child_node(parent_idx, &path, commit))
                            }
                            None => Attr::Unregistered(dir.to_path_buf()),
                        }
                    } else {
                        Attr::Node(parent_idx)
                    }
                }
            }
        };
        self.attr_cache.insert(dir.to_path_buf(), attr.clone());
        attr
    }

    /// Resolves a target to its owning repository by following gitlinks from
    /// the root, returning the repository and the path inside it.
    pub(super) fn descend(&mut self, target: &Path) -> (usize, String) {
        let mut current = 0usize;
        let mut remainder = relative_slash(&self.root, target);
        loop {
            let hit = self
                .gitlinks(current)
                .into_iter()
                .find(|(path, _)| remainder == *path || remainder.starts_with(&format!("{path}/")));
            let Some((path, commit)) = hit else {
                return (current, remainder);
            };
            let child = self.child_node(current, &path, commit);
            remainder = remainder[path.len()..].trim_start_matches('/').to_string();
            current = child;
        }
    }

    pub(super) fn is_descendant(&self, node: usize, ancestor: usize) -> bool {
        let mut current = self.nodes[node].parent;
        while let Some(idx) = current {
            if idx == ancestor {
                return true;
            }
            current = self.nodes[idx].parent;
        }
        false
    }

    /// Existence of `path` (blob or tree) in the submodule's recorded tree.
    pub(super) fn exists_at_revision(&mut self, idx: usize, path: &str) -> Result<bool, String> {
        let tree_id = self.root_tree(idx)?;
        if path.is_empty() {
            return Ok(true);
        }
        let key = (idx, path.to_string());
        if let Some(&cached) = self.exists_cache.get(&key) {
            return Ok(cached);
        }
        let repo = self.nodes[idx].repo.as_ref().expect("opened by root_tree");
        let tree = repo.find_tree(tree_id).map_err(|e| e.to_string())?;
        let exists = tree.get_path(Path::new(path)).is_ok();
        self.exists_cache.insert(key, exists);
        Ok(exists)
    }

    /// Contents of the blob at `path` in the submodule's recorded tree
    /// (`None` when absent or not a blob).
    pub(super) fn blob_at_revision(
        &mut self,
        idx: usize,
        path: &str,
    ) -> Result<Option<Vec<u8>>, String> {
        let tree_id = self.root_tree(idx)?;
        let repo = self.nodes[idx].repo.as_ref().expect("opened by root_tree");
        let tree = repo.find_tree(tree_id).map_err(|e| e.to_string())?;
        let Ok(entry) = tree.get_path(Path::new(path)) else {
            return Ok(None);
        };
        match repo.find_blob(entry.id()) {
            Ok(blob) => Ok(Some(blob.content().to_vec())),
            Err(_) => Ok(None),
        }
    }
}

pub(super) fn relative_slash(base: &Path, path: &Path) -> String {
    path.strip_prefix(base)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

pub(super) fn nearest_existing_dir(path: &Path) -> PathBuf {
    let mut current = path.to_path_buf();
    loop {
        if current.is_dir() {
            return current;
        }
        match current.parent() {
            Some(parent) => current = parent.to_path_buf(),
            None => return current,
        }
    }
}

pub(super) fn display_relative(repo_root: &Path, path: &Path) -> String {
    if path == repo_root {
        ".".to_string()
    } else {
        relative_slash(repo_root, path)
    }
}
