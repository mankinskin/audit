use std::{
    fs,
    path::Path,
    time::{
        SystemTime,
        UNIX_EPOCH,
    },
};

use super::store_root_within;

fn temp_dir(prefix: &str) -> std::path::PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time went backwards")
        .as_nanos();
    let path = std::env::temp_dir()
        .join(format!("{prefix}-{}-{unique}", std::process::id()));
    fs::create_dir_all(&path).expect("create temp dir");
    path
}

fn assert_same_dir(
    resolved: &Path,
    expected: &Path,
) {
    let resolved = fs::canonicalize(resolved).expect("canonicalize resolved");
    let expected = fs::canonicalize(expected).expect("canonicalize expected");
    assert_eq!(resolved, expected);
}

#[test]
fn finds_a_store_inside_the_selection() {
    let workspace = temp_dir("audit-store-scope-local");
    let store = workspace.join(".ticket");
    fs::create_dir_all(&store).expect("create store");

    let resolved =
        store_root_within(&workspace, ".ticket").expect("store in selection");

    assert_same_dir(&resolved, &store);
    let _ = fs::remove_dir_all(&workspace);
}

#[test]
fn ignores_a_store_owned_by_a_parent_workspace() {
    let workspace = temp_dir("audit-store-scope-parent");
    fs::create_dir_all(workspace.join(".ticket")).expect("create store");
    let nested = workspace.join("nested-repo");
    fs::create_dir_all(&nested).expect("create nested repo");

    assert_eq!(store_root_within(&nested, ".ticket"), None);
    let _ = fs::remove_dir_all(&workspace);
}

#[test]
fn finds_a_canonical_store_inside_the_selection() {
    let workspace = temp_dir("audit-store-scope-canonical");
    let store = workspace.join(".workflow-tools").join("ticket");
    fs::create_dir_all(&store).expect("create canonical store");

    let resolved =
        store_root_within(&workspace, ".ticket").expect("store in selection");

    assert_same_dir(&resolved, &store);
    let _ = fs::remove_dir_all(&workspace);
}
