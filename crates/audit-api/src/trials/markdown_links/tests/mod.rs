use std::path::Path;
use std::process::Command;

use tempfile::{TempDir, tempdir};

use super::{EvaluateOptions, LinkClass, MarkdownLinkResult, evaluate, evaluate_with_options};

mod anchors;
mod auditignore;
mod external;
mod links;
mod resolution;
mod shapes;
mod submodules;

fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(dir)
        .args([
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "protocol.file.allow=always",
        ])
        .args(args)
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "git {args:?} failed in {}: {}",
        dir.display(),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn write(dir: &Path, rel: &str, contents: &str) {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

/// Initialises a repository with the given files and one commit.
fn init_repo(files: &[(&str, &str)]) -> TempDir {
    let dir = tempdir().unwrap();
    git(dir.path(), &["init", "-q", "-b", "main"]);
    for (rel, contents) in files {
        write(dir.path(), rel, contents);
    }
    git(dir.path(), &["add", "-A"]);
    git(dir.path(), &["commit", "-q", "-m", "init"]);
    dir
}

fn add_submodule(parent: &Path, source: &Path, name: &str) {
    git(
        parent,
        &["submodule", "add", "-q", source.to_str().unwrap(), name],
    );
}

fn categories(result: &MarkdownLinkResult) -> Vec<&str> {
    result
        .findings
        .iter()
        .map(|f| f.category.as_str())
        .collect()
}

/// Root repo with `README.md` and a committed submodule `sub`.
fn root_with_sub(root_readme: &str, sub_files: &[(&str, &str)]) -> (TempDir, TempDir) {
    let sub_src = init_repo(sub_files);
    let root = init_repo(&[("README.md", root_readme)]);
    add_submodule(root.path(), sub_src.path(), "sub");
    git(root.path(), &["commit", "-q", "-m", "add sub"]);
    (root, sub_src)
}
