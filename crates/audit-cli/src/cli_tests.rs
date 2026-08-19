use super::*;
use session_api::{
    CopilotHookMessage,
    CopilotHookPayload,
    SessionCaptureRequest,
    SessionRole,
    SessionStoreConfig,
};
use tempfile::tempdir;

#[test]
fn parses_move_command() {
    let cli = parse_cli_from([
        "audit",
        "move",
        "7b3a7c62-1f3f-45d6-b8a1-f2b83e3d9f71",
        "--repo-root",
        "/repo",
        "--to-workspace-root",
        "/target",
    ])
    .expect("parse move");

    match cli.command {
        AuditCommand::Move(args) => {
            assert_eq!(
                args.id.as_deref(),
                Some("7b3a7c62-1f3f-45d6-b8a1-f2b83e3d9f71")
            );
            assert_eq!(args.repo_root, PathBuf::from("/repo"));
            assert_eq!(args.to_workspace_root, Some(PathBuf::from("/target")));
        },
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn move_plans_blocked_when_audit_entity_has_no_folder() {
    let temp = tempdir().unwrap();
    let repo_root = temp.path().join("repo");
    std::fs::create_dir_all(&repo_root).unwrap();
    std::process::Command::new("git")
        .current_dir(&repo_root)
        .args(["init"])
        .status()
        .expect("git init")
        .success()
        .then_some(())
        .expect("git init failed");

    let target_workspace = repo_root.join("target-workspace");
    std::fs::create_dir_all(target_workspace.join(".audit")).unwrap();
    RepositoryIndex::init(&repo_root).unwrap();

    let cli = parse_cli_from([
        "audit",
        "--json",
        "move",
        "7b3a7c62-1f3f-45d6-b8a1-f2b83e3d9f71",
        "--repo-root",
        repo_root.to_string_lossy().as_ref(),
        "--to-workspace-root",
        target_workspace.to_string_lossy().as_ref(),
    ])
    .expect("parse move");

    match run(cli).expect("run move") {
        CliOutput::Machine(value, _) => {
            assert_eq!(value["status"], "blocked");
            assert_eq!(value["mode"], "plan");
            assert!(value["plan"]["blockers"].as_array().unwrap().len() > 0);
        },
        other => panic!("unexpected output: {other:?}"),
    }
}

#[test]
fn parses_run_session_selector_flags() {
    let cli = parse_cli_from([
        "audit",
        "run",
        "/repo",
        "--latest-session",
        "--session-store-root",
        "/repo/.session",
        "--session-workspace-slug",
        "context-engine",
    ])
    .expect("parse run latest-session");

    match cli.command {
        AuditCommand::Run(args) => {
            assert!(args.latest_session);
            assert_eq!(args.session_id, None);
            assert_eq!(
                args.session_store_root,
                Some(PathBuf::from("/repo/.session"))
            );
            assert_eq!(
                args.session_workspace_slug,
                Some("context-engine".to_string())
            );
        },
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn run_latest_session_emits_session_audit_payload() {
    let temp = tempdir().unwrap();
    let repo_root = temp.path().join("repo");
    std::fs::create_dir_all(&repo_root).unwrap();
    let store_root = repo_root.join(".session");
    let store = SessionStoreConfig::new(&store_root, "repo");

    let payload = CopilotHookPayload {
        session_id: "session-cli".to_string(),
        workspace_slug: "repo".to_string(),
        captured_at: chrono::Utc::now(),
        conversation_id: Some("conv-1".to_string()),
        agent_id: Some("copilot".to_string()),
        model: Some("GPT-5.3-Codex".to_string()),
        trigger: Some("test".to_string()),
        provisioning: None,
        messages: vec![CopilotHookMessage {
            role: SessionRole::Assistant,
            content: "audit me".to_string(),
            tool_name: None,
            captured_at: None,
            event_meta: None,
        }],
        events: vec![],
        runtime: None,
    };
    store
        .persist_capture(SessionCaptureRequest::copilot(payload))
        .unwrap();

    let cli = parse_cli_from([
        "audit",
        "--json",
        "run",
        repo_root.to_string_lossy().as_ref(),
        "--latest-session",
        "--session-store-root",
        store_root.to_string_lossy().as_ref(),
        "--session-workspace-slug",
        "repo",
    ])
    .expect("parse run latest-session");

    match run(cli).expect("run latest-session") {
        CliOutput::Machine(value, _) => {
            assert_eq!(value["session_id"], "session-cli");
            assert!(value["schema_version"].as_u64().unwrap_or(0) >= 1);
            assert_eq!(value["workspace_slug"], "repo");
        },
        other => panic!("unexpected output: {other:?}"),
    }
}
