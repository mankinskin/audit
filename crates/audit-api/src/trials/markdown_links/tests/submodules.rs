use super::*;

#[test]
fn nested_repository_without_gitlink_is_blocking_and_not_scanned() {
    let repo = tempdir().unwrap();
    std::fs::create_dir_all(repo.path().join(".agents")).unwrap();
    std::fs::create_dir_all(repo.path().join("nested/.git")).unwrap();
    std::fs::write(repo.path().join("nested/README.md"), "[b](gone.md)\n").unwrap();
    std::fs::write(
        repo.path().join(".agents/links.md"),
        "[nested](../nested/README.md)\n",
    )
    .unwrap();

    let result = evaluate(repo.path(), &[]);

    assert_eq!(result.metric.markdown_files, 1);
    assert_eq!(result.metric.links_checked, 0);
    assert_eq!(result.metric.blocking_findings, 1);
    assert_eq!(result.findings.len(), 1);
    assert_eq!(
        result.findings[0].category,
        LinkClass::SubmoduleGitlinkMissing.category()
    );
    assert_eq!(result.findings[0].evidence["source_repository"], ".");
    assert_eq!(result.findings[0].evidence["target_repository"], "nested");
}

// 1
#[test]
fn submodule_target_present_at_gitlink_passes() {
    let (root, _sub) = root_with_sub(
        "[x](sub/docs/x.md)\n",
        &[("docs/x.md", "# x\n"), ("README.md", "# sub\n")],
    );
    let result = evaluate(root.path(), &[]);
    assert!(result.findings.is_empty(), "{:?}", categories(&result));
    assert_eq!(result.metric.links_checked, 1);
    assert!(result.metric.markdown_files >= 3);
}

// 2
#[test]
fn submodule_target_only_in_working_tree_is_missing_at_revision() {
    let (root, _sub) = root_with_sub("[x](sub/docs/new.md)\n", &[("README.md", "# sub\n")]);
    write(&root.path().join("sub"), "docs/new.md", "# uncommitted\n");
    let result = evaluate(root.path(), &[]);
    assert_eq!(
        categories(&result),
        vec![LinkClass::SubmoduleRevisionMissingTarget.category()]
    );
    assert_eq!(result.metric.blocking_findings, 1);
    assert_eq!(result.findings[0].path.as_deref(), Some("README.md"));
}

// 3
#[test]
fn submodule_upward_link_is_blocking_cross_repository() {
    let (root, _sub) = root_with_sub("# root\n", &[("docs/y.md", "[up](../../README.md)\n")]);
    let result = evaluate(root.path(), &[]);
    assert_eq!(
        categories(&result),
        vec![LinkClass::CrossRepository.category()]
    );
    assert_eq!(result.findings[0].evidence["blocking"], true);
    assert_eq!(result.findings[0].evidence["source_repository"], "sub");
    assert_eq!(result.findings[0].path.as_deref(), Some("sub/docs/y.md"));
}

// 9
#[test]
fn nested_submodule_chain_resolves_via_intermediate_tree() {
    let inner = init_repo(&[("docs/z.md", "# z\n")]);
    let mid = init_repo(&[("README.md", "# mid\n")]);
    add_submodule(mid.path(), inner.path(), "inner");
    git(mid.path(), &["commit", "-q", "-m", "add inner"]);
    let root = init_repo(&[(
        "README.md",
        "[ok](mid/inner/docs/z.md)\n[no](mid/inner/docs/nope.md)\n",
    )]);
    add_submodule(root.path(), mid.path(), "mid");
    git(root.path(), &["commit", "-q", "-m", "add mid"]);
    git(
        root.path(),
        &["submodule", "update", "--init", "--recursive", "-q"],
    );
    let result = evaluate(root.path(), &[]);
    assert_eq!(
        categories(&result),
        vec![LinkClass::SubmoduleRevisionMissingTarget.category()]
    );
    assert_eq!(result.findings[0].line, Some(2));
    assert_eq!(result.metric.links_checked, 2);
}

// 11
#[test]
fn nested_repo_never_added_to_index_is_gitlink_missing() {
    let root = init_repo(&[("README.md", "[x](nested/x.md)\n")]);
    let nested = root.path().join("nested");
    std::fs::create_dir_all(&nested).unwrap();
    git(&nested, &["init", "-q", "-b", "main"]);
    write(&nested, "x.md", "[broken](missing.md)\n");
    let result = evaluate(root.path(), &[]);
    assert_eq!(
        categories(&result),
        vec![LinkClass::SubmoduleGitlinkMissing.category()]
    );
    assert_eq!(result.metric.markdown_files, 1);
}

// 12
#[test]
fn intermediate_tree_without_inner_gitlink_is_gitlink_missing() {
    let inner = init_repo(&[("z.md", "[broken](missing.md)\n")]);
    let mid = init_repo(&[("README.md", "# mid\n")]);
    let root = init_repo(&[("README.md", "[z](mid/inner/z.md)\n")]);
    add_submodule(root.path(), mid.path(), "mid");
    git(root.path(), &["commit", "-q", "-m", "add mid"]);
    // Inner repo exists on disk and is staged only in mid's index.
    add_submodule(&root.path().join("mid"), inner.path(), "inner");
    let result = evaluate(root.path(), &[]);
    assert_eq!(
        categories(&result),
        vec![LinkClass::SubmoduleGitlinkMissing.category()]
    );
    assert!(
        result
            .findings
            .iter()
            .all(|f| !f.path.as_deref().unwrap_or("").starts_with("mid/inner"))
    );
}
