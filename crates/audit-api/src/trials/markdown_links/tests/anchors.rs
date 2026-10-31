use super::*;

fn anchor_result(target: &str, link: &str) -> MarkdownLinkResult {
    let repo = init_repo(&[("other.md", target), ("a.md", link), ("code.rs", "//\n")]);
    evaluate(repo.path(), &[])
}

// Part 02 test 1
#[test]
fn heading_slug_with_parentheses_matches() {
    let result = anchor_result(
        "## Ambiguity Resolution Gate (before prompt-ingestion Stage 2)\n",
        "[a](other.md#ambiguity-resolution-gate-before-prompt-ingestion-stage-2)\n",
    );
    assert!(result.findings.is_empty(), "{:?}", categories(&result));
}

// Part 02 test 2
#[test]
fn heading_slug_drops_dots_in_file_names() {
    let result = anchor_result(
        "## audio-transcript.instructions.md Stage 1\n",
        "[a](other.md#audio-transcriptinstructionsmd-stage-1)\n",
    );
    assert!(result.findings.is_empty(), "{:?}", categories(&result));
}

// Part 02 test 3
#[test]
fn duplicate_headings_get_numeric_suffixes() {
    let result = anchor_result(
        "# x\n\n# x\n",
        "[a](other.md#x) [b](other.md#x-1) [c](other.md#x-2)\n",
    );
    assert_eq!(
        categories(&result),
        vec![LinkClass::MissingAnchor.category()]
    );
    assert_eq!(result.findings[0].evidence["fragment"], "x-2");
    assert_eq!(result.metric.blocking_findings, 1);
}

// Part 02 test 4
#[test]
fn html_id_and_name_attributes_are_anchors() {
    let result = anchor_result(
        "<a id=\"custom\"></a>\n\ntext <a name='legacy'>x</a> and <span id=bare>y</span>\n",
        "[a](other.md#custom) [b](other.md#legacy) [c](other.md#bare) [d](other.md#nope)\n",
    );
    assert_eq!(
        categories(&result),
        vec![LinkClass::MissingAnchor.category()]
    );
    assert_eq!(result.findings[0].evidence["fragment"], "nope");
}

// Part 02 test 5
#[test]
fn fragment_only_missing_is_blocking_and_line_anchors_are_unchecked() {
    let repo = init_repo(&[
        ("code.rs", "//\n"),
        (
            "a.md",
            "# Title\n[ok](#title) [bad](#missing)\n[l](code.rs#L10) [r](code.rs#L10-L20) [m](a.md#L3)\n[x](code.rs#anything)\n",
        ),
    ]);
    let result = evaluate(repo.path(), &[]);
    assert_eq!(
        categories(&result),
        vec![LinkClass::MissingAnchor.category()]
    );
    assert_eq!(result.findings[0].evidence["fragment"], "missing");
    assert_eq!(result.findings[0].line, Some(2));
    assert_eq!(result.metric.blocking_findings, 1);
}

// Part 02 test 6
#[test]
fn submodule_anchor_is_read_from_gitlink_blob_not_working_tree() {
    let (root, _sub) = root_with_sub(
        "[old](sub/docs/x.md#old-heading)\n[new](sub/docs/x.md#new-heading)\n",
        &[("docs/x.md", "# Old Heading\n"), ("README.md", "# sub\n")],
    );
    write(&root.path().join("sub"), "docs/x.md", "# New Heading\n");
    let result = evaluate(root.path(), &[]);
    assert_eq!(
        categories(&result),
        vec![LinkClass::MissingAnchor.category()]
    );
    assert_eq!(result.findings[0].evidence["fragment"], "new-heading");
    assert_eq!(result.findings[0].line, Some(2));
    assert!(result.findings[0].evidence["revision"].is_string());
}
