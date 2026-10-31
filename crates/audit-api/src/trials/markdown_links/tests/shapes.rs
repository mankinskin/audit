use super::*;

/// D5: backslash and drive-letter destinations are unsupported dependencies,
/// not missing targets.
#[test]
fn backslash_and_drive_letter_destinations_are_unsupported_dependencies() {
    let repo = init_repo(&[("a.md", "[a](\\x.md)\n[b](C:/x.md)\n")]);

    let result = evaluate(repo.path(), &[]);

    assert_eq!(result.metric.unsupported_dependency_links, 2);
    assert_eq!(result.metric.blocking_findings, 2);
    let categories = categories(&result);
    assert_eq!(
        categories,
        vec![
            LinkClass::UnsupportedDependency.category(),
            LinkClass::UnsupportedDependency.category()
        ]
    );
    assert!(!categories.contains(&LinkClass::MissingTarget.category()));
}
