use std::{path::PathBuf, process::Command};

use rmcp::handler::server::wrapper::Parameters;
use rusqlite::Connection;
use serde_json::Value;
use tempfile::TempDir;

use super::{
    AuditRepositoryInput,
    AuditSummaryByInput,
    AuditSummaryInput,
    AuditMoveInput,
    AuditMoveJournalInput,
    AuditServer,
};

#[tokio::test]
async fn audit_exposes_repository_guidance_findings() {
    let tmp = TempDir::new().expect("tempdir");
    std::fs::write(tmp.path().join("README.md"), "# Repo\n")
        .expect("write readme");
    std::fs::write(tmp.path().join("INSTALL.md"), " \n")
        .expect("write install");

    let server = AuditServer::new(tmp.path().to_path_buf());
    let result = server
        .audit(Parameters(AuditRepositoryInput {
            repo_root: Some(tmp.path().to_path_buf()),
            max_file_lines: None,
            max_cyclomatic_complexity: None,
            coverage_warn_below: None,
        }))
        .await
        .expect("audit");
    let json = extract_json(result);

    assert!(json["findings"].as_array().unwrap().iter().any(|finding| {
        finding["id"] == "repository_guidance:empty:INSTALL.md"
    }));
    assert!(json["findings"].as_array().unwrap().iter().any(|finding| {
        finding["id"] == "repository_guidance:missing:CONTRIBUTING.md"
    }));
}

fn run_git(
    repo_root: &std::path::Path,
    args: &[&str],
) {
    let status = Command::new("git")
        .current_dir(repo_root)
        .args(args)
        .status()
        .expect("git command");
    assert!(status.success(), "git {args:?} failed: {status}");
}

fn extract_json(result: rmcp::model::CallToolResult) -> Value {
    let text = result
        .content
        .iter()
        .find_map(|content| {
            if let rmcp::model::RawContent::Text(text) = &content.raw {
                Some(text.text.clone())
            } else {
                None
            }
        })
        .expect("text content");
    serde_json::from_str(&text).expect("parse json")
}

#[tokio::test]
async fn audit_write_tools_reject_omitted_and_ambient_workspace_selectors() {
    let tmp = TempDir::new().expect("tempdir");
    let server = AuditServer::new(tmp.path().to_path_buf());

    for selector in [None, Some(PathBuf::from("")), Some(PathBuf::from("  ")), Some(PathBuf::from("default")), Some(PathBuf::from(".."))] {
        let audit = server
            .audit(Parameters(AuditRepositoryInput {
                repo_root: selector.clone(),
                max_file_lines: None,
                max_cyclomatic_complexity: None,
                coverage_warn_below: None,
            }))
            .await;
        assert!(audit.is_err());

        let summary = server
            .audit_summary(Parameters(AuditSummaryInput {
                repo_root: selector,
                by: AuditSummaryByInput::Category,
                max_file_lines: None,
                max_cyclomatic_complexity: None,
                coverage_warn_below: None,
            }))
            .await;
        assert!(summary.is_err());
        assert!(!tmp.path().join(".workflow-tools/audit").exists());
    }
}

#[tokio::test]
async fn explicit_dot_workspace_persists_audit_and_summary_in_canonical_store() {
    const CHILD_ENV: &str = "AUDIT_MCP_DOT_READBACK_CHILD";
    if std::env::var_os(CHILD_ENV).is_none() {
        let tmp = TempDir::new().expect("tempdir");
        let selected = tmp.path().join("selected");
        std::fs::create_dir_all(&selected).expect("create selected workspace");
        std::fs::write(selected.join("README.md"), "audit selector fixture\n")
            .expect("write fixture file");

        let output = Command::new(std::env::current_exe().expect("test binary"))
            .args([
                "--exact",
                "server_tests::explicit_dot_workspace_persists_audit_and_summary_in_canonical_store",
                "--nocapture",
            ])
            .env(CHILD_ENV, "1")
            .current_dir(&selected)
            .output()
            .expect("run isolated child test");
        assert!(
            output.status.success(),
            "child test failed: {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!tmp.path().join(".workflow-tools/audit").exists());
        return;
    }

    let selected = std::env::current_dir().expect("selected workspace cwd");
    let ambient = selected.parent().expect("fixture parent").join("ambient");
    std::fs::create_dir_all(&ambient).expect("create ambient workspace");
    let server = AuditServer::new(ambient.clone());

    let audit_report = extract_json(
        server
            .audit(Parameters(AuditRepositoryInput {
                repo_root: Some(PathBuf::from(".")),
                max_file_lines: None,
                max_cyclomatic_complexity: None,
                coverage_warn_below: None,
            }))
            .await
            .expect("run Audit MCP tool"),
    );
    let summary_report = extract_json(
        server
            .audit_summary(Parameters(AuditSummaryInput {
                repo_root: Some(PathBuf::from(".")),
                by: AuditSummaryByInput::Category,
                max_file_lines: None,
                max_cyclomatic_complexity: None,
                coverage_warn_below: None,
            }))
            .await
            .expect("run Audit summary MCP tool"),
    );

    let canonical_store = selected.join(".workflow-tools").join("audit");
    let connection = Connection::open(canonical_store.join("audit.sqlite3"))
        .expect("open selected canonical audit database");
    let audit_run_id = audit_report["run"]["run_id"]
        .as_i64()
        .expect("audit report contains persisted run id");
    let (persisted_root, status): (String, String) = connection
        .query_row(
            "SELECT repo_root, status FROM audit_runs WHERE run_id = ?1",
            [audit_run_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("read Audit run from selected store");
    let run_count: i64 = connection
        .query_row("SELECT COUNT(*) FROM audit_runs", [], |row| row.get(0))
        .expect("count persisted Audit and summary runs");

    assert_eq!(persisted_root, audit_report["repo_root"]);
    assert_eq!(persisted_root, summary_report["repo_root"]);
    assert_eq!(status, "completed");
    assert_eq!(run_count, 2);
    assert!(!selected.join(".audit").exists());
    assert!(!ambient.join(".workflow-tools").join("audit").exists());
}

#[tokio::test]
async fn move_preflight_is_blocked_for_repository_level_audit_storage() {
    let tmp = TempDir::new().expect("tempdir");
    let repo_root = tmp.path().join("repo");
    std::fs::create_dir_all(&repo_root).expect("repo root");
    run_git(&repo_root, &["init"]);

    let source_workspace = repo_root.join("source-workspace");
    let target_workspace = repo_root.join("target-workspace");
    std::fs::create_dir_all(source_workspace.join(".audit"))
        .expect("source audit dir");
    std::fs::create_dir_all(target_workspace.join(".audit"))
        .expect("target audit dir");
    audit_api::index::RepositoryIndex::init(&source_workspace)
        .expect("init source audit index");

    let server = AuditServer::new(source_workspace.clone());
    let result = server
        .audit_move_preflight(Parameters(AuditMoveInput {
            repo_root: Some(source_workspace.clone()),
            id: "7b3a7c62-1f3f-45d6-b8a1-f2b83e3d9f71".to_string(),
            to_workspace_root: target_workspace.to_string_lossy().to_string(),
        }))
        .await
        .expect("audit move preflight");
    let json = extract_json(result);

    assert_eq!(json["status"], "blocked");
    assert_eq!(json["mode"], "preflight");
    assert!(json["plan"]["blockers"].as_array().unwrap().len() > 0);
}

#[tokio::test]
async fn move_targets_reject_ambient_workspace_aliases_before_store_access() {
    let tmp = TempDir::new().expect("tempdir");
    let missing_source = tmp.path().join("not-created-source");
    let server = AuditServer::new(tmp.path().to_path_buf());

    for selector in ["", "  ", "default", ".."] {
        let preflight = server
            .audit_move_preflight(Parameters(AuditMoveInput {
                repo_root: Some(missing_source.clone()),
                id: "7b3a7c62-1f3f-45d6-b8a1-f2b83e3d9f71".to_string(),
                to_workspace_root: selector.to_string(),
            }))
            .await;
        assert!(preflight.is_err());

        let apply = server
            .audit_move_apply(Parameters(AuditMoveInput {
                repo_root: Some(missing_source.clone()),
                id: "7b3a7c62-1f3f-45d6-b8a1-f2b83e3d9f71".to_string(),
                to_workspace_root: selector.to_string(),
            }))
            .await;
        assert!(apply.is_err());
        assert!(!missing_source.exists());
    }
}

fn persist_sample_finding(
    index: &audit_api::index::RepositoryIndex,
    id: uuid::Uuid,
) {
    use audit_api::{
        finding_entity::PersistedFinding,
        models::Severity,
    };
    let finding = PersistedFinding {
        id,
        category: "file-length".to_string(),
        severity: Severity::Medium,
        summary: "file too long".to_string(),
        path: Some("src/lib.rs".to_string()),
        line: None,
        metric_name: "line_count".to_string(),
        metric_value: serde_json::json!(500),
        threshold: Some(serde_json::json!(400)),
        instructions: vec!["split the file".to_string()],
        evidence: serde_json::json!({"lines": 500}),
    };
    index.persist_finding_entity(&finding).expect("persist finding");
}

#[tokio::test]
async fn move_apply_and_rollback_round_trip_entity_folder_finding() {
    let tmp = TempDir::new().expect("tempdir");
    let repo_root = tmp.path().join("repo");
    std::fs::create_dir_all(&repo_root).expect("repo root");
    run_git(&repo_root, &["init"]);

    let source_workspace = repo_root.join("source-workspace");
    let target_workspace = repo_root.join("target-workspace");
    std::fs::create_dir_all(&source_workspace).expect("source workspace");
    std::fs::create_dir_all(&target_workspace).expect("target workspace");
    audit_api::index::RepositoryIndex::init(&target_workspace)
        .expect("init target audit index");
    let source_index = audit_api::index::RepositoryIndex::init(&source_workspace)
        .expect("init source audit index");

    let finding_id = uuid::Uuid::new_v4();
    persist_sample_finding(&source_index, finding_id);

    let server = AuditServer::new(source_workspace.clone());

    let preflight = server
        .audit_move_preflight(Parameters(AuditMoveInput {
            repo_root: Some(source_workspace.clone()),
            id: finding_id.to_string(),
            to_workspace_root: target_workspace.to_string_lossy().to_string(),
        }))
        .await
        .expect("audit move preflight");
    let preflight_json = extract_json(preflight);
    assert_eq!(preflight_json["status"], "ok");

    let apply = server
        .audit_move_apply(Parameters(AuditMoveInput {
            repo_root: Some(source_workspace.clone()),
            id: finding_id.to_string(),
            to_workspace_root: target_workspace.to_string_lossy().to_string(),
        }))
        .await
        .expect("audit move apply");
    let apply_json = extract_json(apply);
    assert_eq!(apply_json["status"], "ok");
    let journal_id = apply_json["outcome"]["journal"]["id"]
        .as_str()
        .expect("journal id")
        .to_string();

    let rollback = server
        .audit_move_rollback(Parameters(AuditMoveJournalInput {
            repo_root: Some(source_workspace.clone()),
            id: journal_id,
        }))
        .await
        .expect("audit move rollback");
    let rollback_json = extract_json(rollback);
    assert_eq!(rollback_json["status"], "ok");
    assert_eq!(rollback_json["mode"], "rollback");

    assert!(
        source_index.finding_entity_path(&finding_id).is_some(),
        "rollback must restore the finding entity to its source folder"
    );
}
