use std::process::Command;

use rmcp::handler::server::wrapper::Parameters;
use serde_json::Value;
use tempfile::TempDir;

use super::{
    AuditMoveInput,
    AuditMoveJournalInput,
    AuditServer,
};

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
