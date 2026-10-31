use super::*;
use crate::trials::markdown_links::targets::resolve_target;

#[test]
fn escaped_target_is_decoded_in_evidence() {
    let root = Path::new("/workspace/repo");
    let escaped = resolve_target(root, "README.md", "../outside%20dir/a.md").unwrap_err();
    assert_eq!(escaped, Path::new("/workspace/outside dir/a.md"));
}

#[test]
fn encoded_parent_segments_are_unsafe() {
    let root = Path::new("/workspace/repo");
    let escaped = resolve_target(root, "README.md", "%2e%2e/outside.md").unwrap_err();
    assert_eq!(escaped, Path::new("/workspace/outside.md"));
}

#[test]
fn encoded_absolute_path_is_unsafe() {
    let root = Path::new("/workspace/repo");
    let escaped = resolve_target(root, "README.md", "%2Fetc%2Fpasswd").unwrap_err();
    assert_eq!(escaped, Path::new("/etc/passwd"));
}

#[test]
fn intermediate_escape_back_into_repo_is_checked_normally() {
    let root = Path::new("/workspace/repo");
    let ok = resolve_target(root, "README.md", "../repo/README.md").unwrap();
    assert_eq!(ok, Path::new("/workspace/repo/README.md"));
    assert_eq!(
        resolve_target(root, "docs/a.md", "..").unwrap(),
        Path::new("/workspace/repo")
    );
}

#[test]
fn evaluate_checks_reentrant_link_and_flags_encoded_absolute() {
    let repo = tempdir().unwrap();
    let name = repo.path().file_name().unwrap().to_str().unwrap().to_string();
    std::fs::create_dir_all(repo.path().join(".agents")).unwrap();
    std::fs::write(repo.path().join("README.md"), "# Repo\n").unwrap();
    write(
        repo.path(),
        "AGENTS.md",
        &format!("[ok](../{name}/README.md)\n[abs](%2Fetc%2Fpasswd)\n"),
    );

    let result = evaluate(repo.path(), &[]);

    assert_eq!(result.metric.unsafe_path_links, 1);
    assert_eq!(result.metric.blocking_findings, 1);
    assert_eq!(result.findings[0].category, LinkClass::UnsafePath.category());
}
