use super::*;

fn sources_with_findings(result: &MarkdownLinkResult) -> Vec<String> {
    let mut paths: Vec<String> = result
        .findings
        .iter()
        .filter_map(|f| f.path.clone())
        .collect();
    paths.sort();
    paths.dedup();
    paths
}

#[test]
fn auditignore_excludes_globs_anchored_paths_and_honours_negation() {
    let repo = init_repo(&[
        ("README.md", "# Repo\n"),
        (
            ".auditignore",
            "*.draft.md\n/archive/\nreviews/**\n!reviews/keep.md\n",
        ),
        ("notes.draft.md", "[x](missing.md)\n"),
        ("archive/old.md", "[x](missing.md)\n"),
        ("docs/archive/live.md", "[x](missing.md)\n"),
        ("reviews/a/b.md", "[x](missing.md)\n"),
        ("reviews/keep.md", "[x](missing.md)\n"),
    ]);
    let result = evaluate(repo.path(), &[]);
    assert_eq!(
        sources_with_findings(&result),
        vec!["docs/archive/live.md", "reviews/keep.md"]
    );
}

#[test]
fn auditignore_in_nested_directory_applies_to_its_subtree_only() {
    let repo = init_repo(&[
        ("README.md", "# Repo\n"),
        ("docs/.auditignore", "generated/\n"),
        ("docs/generated/out.md", "[x](missing.md)\n"),
        ("generated/top.md", "[x](missing.md)\n"),
    ]);
    let result = evaluate(repo.path(), &[]);
    assert_eq!(sources_with_findings(&result), vec!["generated/top.md"]);
}

#[test]
fn auditignore_inside_submodule_applies_to_submodule_sources() {
    let (root, _sub) = root_with_sub(
        "# Root\n",
        &[
            ("README.md", "# Sub\n"),
            (".auditignore", "fixtures/\n"),
            ("fixtures/golden.md", "[x](missing.md)\n"),
            ("docs/live.md", "[x](missing.md)\n"),
        ],
    );
    let result = evaluate(root.path(), &[]);
    assert_eq!(sources_with_findings(&result), vec!["sub/docs/live.md"]);
}

#[test]
fn auditignored_files_remain_valid_link_targets() {
    let repo = init_repo(&[
        ("README.md", "[r](reviews/old.md)\n"),
        (".auditignore", "reviews/\n"),
        ("reviews/old.md", "[x](missing.md)\n"),
    ]);
    let result = evaluate(repo.path(), &[]);
    assert!(result.findings.is_empty(), "{:?}", categories(&result));
}

#[test]
fn auditignore_is_not_bypassed_by_include_historical() {
    let repo = init_repo(&[
        ("README.md", "# Repo\n"),
        (".auditignore", "reviews/\n"),
        ("reviews/old.md", "[x](missing.md)\n"),
    ]);
    let result = evaluate_with_options(
        repo.path(),
        &EvaluateOptions {
            include_historical: true,
            ..EvaluateOptions::default()
        },
    );
    assert!(result.findings.is_empty(), "{:?}", categories(&result));
}
