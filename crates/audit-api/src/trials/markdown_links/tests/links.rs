use super::*;

#[test]
fn reports_broken_guidance_links_and_skips_non_local_links() {
    let repo = tempdir().unwrap();
    std::fs::create_dir_all(repo.path().join(".agents/instructions")).unwrap();
    std::fs::write(repo.path().join("README.md"), "# Repo\n").unwrap();
    std::fs::write(
            repo.path().join(".agents/instructions/links.md"),
            "[good](../../README.md)\n[bad](missing.md)\n[web](https://example.com)\n[anchor](#repo)\n\n# Repo\n",
        )
        .unwrap();

    let result = evaluate(repo.path(), &[]);

    assert_eq!(result.metric.markdown_files, 2);
    assert_eq!(result.metric.links_checked, 3);
    assert_eq!(result.metric.broken_links, 1);
    assert_eq!(result.metric.skipped_links, 1);
    assert_eq!(result.metric.blocking_findings, 1);
    assert_eq!(result.findings[0].line, Some(2));
    assert_eq!(
        result.findings[0].path.as_deref(),
        Some(".agents/instructions/links.md")
    );
    assert_eq!(
        result.findings[0].category,
        LinkClass::MissingTarget.category()
    );
}

#[test]
fn blocks_links_to_non_guidance_local_targets() {
    let repo = tempdir().unwrap();
    std::fs::create_dir_all(repo.path().join(".agents")).unwrap();
    std::fs::create_dir_all(repo.path().join("src")).unwrap();
    std::fs::write(repo.path().join("src/lib.rs"), "fn main() {}\n").unwrap();
    std::fs::write(
        repo.path().join(".agents/links.md"),
        "[code](../src/lib.rs)\n",
    )
    .unwrap();

    let result = evaluate(repo.path(), &[]);

    assert_eq!(result.metric.non_guidance_links, 1);
    assert_eq!(result.metric.blocking_findings, 1);
    assert_eq!(
        result.findings[0].category,
        LinkClass::NonGuidanceTarget.category()
    );
    assert_eq!(result.findings[0].evidence["detected_class"], "source_code");
}

#[test]
fn blocks_unsupported_and_unsafe_destinations() {
    let repo = tempdir().unwrap();
    std::fs::create_dir_all(repo.path().join(".agents")).unwrap();
    std::fs::write(
        repo.path().join(".agents/links.md"),
        "[ftp](ftp://example.com/file)\n[escape](../../../../outside.md)\n",
    )
    .unwrap();

    let result = evaluate(repo.path(), &[]);

    assert_eq!(result.metric.unsupported_dependency_links, 1);
    assert_eq!(result.metric.unsafe_path_links, 1);
    assert_eq!(result.metric.blocking_findings, 2);
    let categories: Vec<&str> = result
        .findings
        .iter()
        .map(|finding| finding.category.as_str())
        .collect();
    assert!(categories.contains(&LinkClass::UnsupportedDependency.category()));
    assert!(categories.contains(&LinkClass::UnsafePath.category()));
}

#[test]
fn permits_non_markdown_files_under_agents_directory() {
    let repo = tempdir().unwrap();
    std::fs::create_dir_all(repo.path().join(".agents/skills")).unwrap();
    std::fs::write(repo.path().join(".agents/skills/icon.png"), b"\x89PNG").unwrap();
    std::fs::write(
        repo.path().join(".agents/links.md"),
        "[icon](skills/icon.png)\n",
    )
    .unwrap();

    let result = evaluate(repo.path(), &[]);

    assert_eq!(result.metric.links_checked, 1);
    assert_eq!(result.metric.blocking_findings, 0);
    assert!(result.findings.is_empty());
}

// 4
#[test]
fn guidance_sources_may_only_target_guidance_or_readme() {
    let repo = tempdir().unwrap();
    write(repo.path(), "src/lib.rs", "fn main() {}\n");
    write(repo.path(), "README.md", "# r\n");
    write(repo.path(), ".github/copilot-instructions.md", "# c\n");
    write(
        repo.path(),
        ".agents/a.md",
        "[code](../src/lib.rs)\n[readme](../README.md)\n[cp](../.github/copilot-instructions.md)\n",
    );
    let result = evaluate(repo.path(), &[]);
    let hits: Vec<_> = result
        .findings
        .iter()
        .filter(|f| f.path.as_deref() == Some(".agents/a.md"))
        .collect();
    assert_eq!(hits.len(), 1, "{:?}", categories(&result));
    assert_eq!(hits[0].category, LinkClass::NonGuidanceTarget.category());
    assert_eq!(hits[0].line, Some(1));
}

// 5
#[test]
fn non_guidance_sources_may_link_anywhere_in_repository() {
    let repo = tempdir().unwrap();
    write(repo.path(), "src/lib.rs", "fn main() {}\n");
    write(repo.path(), "docs/a.md", "[code](../src/lib.rs)\n");
    let result = evaluate(repo.path(), &[]);
    assert!(result.findings.is_empty(), "{:?}", categories(&result));
    assert_eq!(result.metric.links_checked, 1);
}

// 6
#[test]
fn transcripts_are_excluded_unless_historical_included() {
    let repo = tempdir().unwrap();
    write(repo.path(), "transcripts/x.md", "[bad](missing.md)\n");
    write(repo.path(), ".workflow-tools/y.md", "[bad](missing.md)\n");
    let default = evaluate(repo.path(), &[]);
    assert!(default.findings.is_empty());
    assert_eq!(default.metric.markdown_files, 0);
    let historical = evaluate_with_options(
        repo.path(),
        &EvaluateOptions {
            include_historical: true,
            ..EvaluateOptions::default()
        },
    );
    assert_eq!(historical.findings.len(), 2);
    assert_eq!(historical.metric.markdown_files, 2);
}

// 7
#[test]
fn missing_image_target_is_reported() {
    let repo = tempdir().unwrap();
    write(repo.path(), "README.md", "![i](missing.png)\n");
    let result = evaluate(repo.path(), &[]);
    assert_eq!(
        categories(&result),
        vec![LinkClass::MissingTarget.category()]
    );
}

// 8
#[cfg(unix)]
#[test]
fn symlinked_directories_are_not_scanned_twice() {
    let repo = tempdir().unwrap();
    write(repo.path(), ".agents/prompts/p.md", "[bad](missing.md)\n");
    std::fs::create_dir_all(repo.path().join(".claude")).unwrap();
    std::os::unix::fs::symlink("../.agents/prompts", repo.path().join(".claude/commands")).unwrap();
    let result = evaluate(repo.path(), &[]);
    assert_eq!(result.metric.markdown_files, 1);
    assert_eq!(result.findings.len(), 1);
}

// 10
#[test]
fn findings_carry_blocking_guidance_and_repository_evidence() {
    let (root, _sub) = root_with_sub(
        "[nope](nope.md)\n",
        &[
            (".agents/s.md", "[m](missing.md)\n"),
            ("up.md", "[u](../README.md)\n"),
        ],
    );
    let result = evaluate(root.path(), &[]);
    assert_eq!(result.findings.len(), 3, "{:?}", categories(&result));
    for finding in &result.findings {
        let ev = &finding.evidence;
        let class_blocking = [LinkClass::MissingTarget, LinkClass::CrossRepository]
            .iter()
            .find(|c| c.category() == finding.category)
            .map(|c| c.is_blocking())
            .unwrap();
        assert_eq!(ev["blocking"], class_blocking);
        match finding.path.as_deref().unwrap() {
            "README.md" => {
                assert_eq!(ev["source_guidance"], false);
                assert_eq!(ev["source_repository"], ".");
            }
            "sub/.agents/s.md" => {
                assert_eq!(ev["source_guidance"], true);
                assert_eq!(ev["source_repository"], "sub");
            }
            "sub/up.md" => {
                assert_eq!(ev["source_guidance"], false);
                assert_eq!(ev["source_repository"], "sub");
            }
            other => panic!("unexpected source {other}"),
        }
    }
}

// Part 02 test 8
#[test]
fn finding_order_is_deterministic() {
    let mut files: Vec<(String, String)> = (0..40)
        .map(|i| {
            (
                format!("docs/d{i}.md"),
                "[a](#nope) [b](missing.md) [c](#also)\n".to_string(),
            )
        })
        .collect();
    files.push(("README.md".to_string(), "# r\n".to_string()));
    let refs: Vec<(&str, &str)> = files
        .iter()
        .map(|(p, c)| (p.as_str(), c.as_str()))
        .collect();
    let repo = init_repo(&refs);
    let ids = |r: &MarkdownLinkResult| r.findings.iter().map(|f| f.id.clone()).collect::<Vec<_>>();
    let first = ids(&evaluate(repo.path(), &[]));
    assert_eq!(first.len(), 120);
    assert_eq!(first, ids(&evaluate(repo.path(), &[])));
    let mut sorted = first.clone();
    sorted.sort_by_key(|id| id.split(':').nth(1).map(str::to_string));
    assert_eq!(
        first
            .iter()
            .map(|id| id.split(':').nth(1))
            .collect::<Vec<_>>(),
        sorted
            .iter()
            .map(|id| id.split(':').nth(1))
            .collect::<Vec<_>>()
    );
}
