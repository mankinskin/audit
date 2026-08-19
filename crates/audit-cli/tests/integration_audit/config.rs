use std::fs;

use audit_api::{
    audit::audit,
    models::AuditConfig,
};
use rusqlite::{
    Connection,
    params,
};
use tempfile::tempdir;

#[test]
fn config_excludes_paths_from_index_and_findings() {
    let repo = tempdir().expect("temp repo");
    fs::create_dir_all(repo.path().join("src")).expect("create src dir");
    fs::create_dir_all(repo.path().join("crates/deps/third_party/src"))
        .expect("create excluded dir");
    fs::write(
        repo.path().join("Cargo.toml"),
        r#"[package]
name = "sample-exclude-repo"
version = "0.1.0"
edition = "2021"
"#,
    )
    .expect("write Cargo.toml");
    fs::write(
        repo.path().join(".audit.toml"),
        "exclude_paths = [\"crates/deps/\"]\n",
    )
    .expect("write config");
    fs::write(
        repo.path().join("src/lib.rs"),
        "pub fn short() -> usize { 1 }\n",
    )
    .expect("write src/lib.rs");
    fs::write(
        repo.path().join("crates/deps/third_party/src/lib.rs"),
        "pub fn very_long() -> usize {\n    1\n}\n\npub fn a() {}\npub fn b() {}\npub fn c() {}\npub fn d() {}\n",
    )
    .expect("write excluded file");

    let report = audit(
        repo.path(),
        AuditConfig {
            max_file_lines: 2,
            max_cyclomatic_complexity: 1,
            coverage_warn_below: 80.0,
        },
    )
    .expect("audit succeeds");

    assert!(!report.findings.iter().any(|finding| {
        finding.path.as_deref() == Some("crates/deps/third_party/src/lib.rs")
    }));

    let connection =
        Connection::open(&report.index_database).expect("open audit db");
    let indexed_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM files WHERE path = ?1",
            params!["crates/deps/third_party/src/lib.rs"],
            |row| row.get(0),
        )
        .expect("query excluded row count");
    assert_eq!(indexed_count, 0);
}
