use std::process::Command;

use rmcp::handler::server::wrapper::Parameters;
use serde_json::Value;
use tempfile::TempDir;

use super::{
    AuditMoveInput,
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
