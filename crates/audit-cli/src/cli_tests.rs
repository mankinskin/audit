use super::*;
use session_api::{
    CopilotHookMessage, CopilotHookPayload, SessionCaptureRequest, SessionRole, SessionStoreConfig,
};
use tempfile::tempdir;

#[test]
fn links_command_passes_on_clean_guidance_corpus() {
    let temp = tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join(".agents")).unwrap();
    std::fs::write(temp.path().join("README.md"), "# Repo\n").unwrap();
    std::fs::write(
        temp.path().join(".agents/links.md"),
        "[good](../README.md)\n",
    )
    .unwrap();

    let cli = parse_cli_from(["audit", "--json", "links", temp.path().to_str().unwrap()])
        .expect("parse links");

    match run(cli).expect("run links") {
        CliOutput::Machine(value, _) => {
            assert_eq!(value["metric"]["blocking_findings"], 0);
            assert_eq!(value["metric"]["links_checked"], 1);
        }
        other => panic!("unexpected output: {other:?}"),
    }
}

#[test]
fn links_command_fails_with_stable_category_and_evidence_on_blocking_finding() {
    let temp = tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join(".agents")).unwrap();
    std::fs::write(
        temp.path().join(".agents/links.md"),
        "[broken](missing.md)\n",
    )
    .unwrap();

    let cli = parse_cli_from(["audit", "--json", "links", temp.path().to_str().unwrap()])
        .expect("parse links");

    let error = run(cli).expect_err("blocking finding must fail");
    let message = error.to_string();
    assert!(message.contains("markdown_link_missing_target"));
    assert!(message.contains(".agents/links.md"));
    assert!(message.contains("missing.md"));
}

#[test]
fn run_command_fails_when_guidance_links_are_blocking() {
    let temp = tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join(".agents")).unwrap();
    std::fs::write(
        temp.path().join(".agents/links.md"),
        "[broken](missing.md)\n",
    )
    .unwrap();

    let cli = parse_cli_from(["audit", "run", temp.path().to_str().unwrap()])
        .expect("parse run");

    let error = run(cli).expect_err("full audit must fail on blocking guidance links");
    let message = error.to_string();
    assert!(message.contains("markdown_link_missing_target"));
    assert!(message.contains(".agents/links.md"));
    assert!(message.contains("missing.md"));
}

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
        }
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
        }
        other => panic!("unexpected output: {other:?}"),
    }
}

fn persist_sample_finding(index: &RepositoryIndex, id: Uuid) {
    use audit_api::{finding_entity::PersistedFinding, models::Severity};
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
    index.persist_finding_entity(&finding).unwrap();
}

#[test]
fn move_applies_resumes_and_rolls_back_entity_folder_finding() {
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
    std::fs::create_dir_all(&target_workspace).unwrap();
    RepositoryIndex::init(&target_workspace).unwrap();

    let index = RepositoryIndex::init(&repo_root).unwrap();
    let finding_id = Uuid::new_v4();
    persist_sample_finding(&index, finding_id);

    let cli = parse_cli_from([
        "audit",
        "--json",
        "move",
        &finding_id.to_string(),
        "--repo-root",
        repo_root.to_string_lossy().as_ref(),
        "--to-workspace-root",
        target_workspace.to_string_lossy().as_ref(),
    ])
    .expect("parse move apply");

    let journal_id = match run(cli).expect("run move apply") {
        CliOutput::Machine(value, _) => {
            assert_eq!(value["status"], "ok");
            assert_eq!(value["mode"], "execute");
            let journal_id = value["outcome"]["journal"]["id"]
                .as_str()
                .expect("journal id")
                .to_string();
            journal_id
        }
        other => panic!("unexpected output: {other:?}"),
    };

    let rollback_cli = parse_cli_from([
        "audit",
        "--json",
        "move",
        "--repo-root",
        repo_root.to_string_lossy().as_ref(),
        "--rollback",
        &journal_id,
    ])
    .expect("parse move rollback");

    match run(rollback_cli).expect("run move rollback") {
        CliOutput::Machine(value, _) => {
            assert_eq!(value["status"], "ok");
            assert_eq!(value["mode"], "rollback");
        }
        other => panic!("unexpected output: {other:?}"),
    }
}

#[test]
fn move_rejects_unsupported_repository_level_layout_with_dry_run() {
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

    let repository_level_id = Uuid::new_v4();
    let cli = parse_cli_from([
        "audit",
        "--json",
        "move",
        &repository_level_id.to_string(),
        "--repo-root",
        repo_root.to_string_lossy().as_ref(),
        "--to-workspace-root",
        target_workspace.to_string_lossy().as_ref(),
        "--dry-run",
    ])
    .expect("parse move dry-run");

    match run(cli).expect("run move dry-run") {
        CliOutput::Machine(value, _) => {
            assert_eq!(value["status"], "blocked");
            assert_eq!(value["dry_run"], true);
        }
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
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn run_latest_session_emits_session_audit_payload() {
    let temp = tempdir().unwrap();
    let repo_root = temp.path().join("repo");
    std::fs::create_dir_all(&repo_root).unwrap();
    let store_root = repo_root.join(".session");
    let store = SessionStoreConfig::new(&store_root);

    let payload = CopilotHookPayload {
        session_id: "session-cli".to_string(),
        workspace_path: "repo".to_string(),
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
    let expected_workspace_path = repo_root.to_string_lossy().replace('\\', "/");

    match run(cli).expect("run latest-session") {
        CliOutput::Machine(value, _) => {
            assert_eq!(value["session_id"], "session-cli");
            assert!(value["schema_version"].as_u64().unwrap_or(0) >= 1);
            assert_eq!(value["workspace_path"], expected_workspace_path);
        }
        other => panic!("unexpected output: {other:?}"),
    }
}
