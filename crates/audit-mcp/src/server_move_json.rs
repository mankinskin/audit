pub(super) fn path_display(path: &std::path::Path) -> String {
    memory_kernel::workspace::normalize_path_for_display(path)
}

pub(super) fn move_plan_json(
    report: &memory_kernel::storage::move_kernel::MovePlan
) -> serde_json::Value {
    serde_json::json!({
        "supported": report.supported(),
        "entity_id": report.entity_id,
        "source_workspace_root": path_display(&report.source_workspace_root),
        "target_workspace_root": path_display(&report.target_workspace_root),
        "source_store_root": path_display(&report.source_store_root),
        "target_store_root": path_display(&report.target_store_root),
        "source_git_worktree_root": path_display(&report.source_git_worktree_root),
        "target_git_worktree_root": path_display(&report.target_git_worktree_root),
        "git_worktree_topology": report.git_worktree_topology,
        "source_entity_path": path_display(&report.source_entity_path),
        "destination_entity_path": path_display(&report.destination_entity_path),
        "inbound_related_entity_ids": report.inbound_related_entity_ids,
        "outbound_related_entity_ids": report.outbound_related_entity_ids,
        "reference_visibility": report.reference_visibility,
        "active_board_entries": report.active_board_entries,
        "historical_board_entries": report.historical_board_entries,
        "active_leases": report.active_leases,
        "path_reference_files": report
            .path_reference_files
            .iter()
            .map(|path| path_display(path))
            .collect::<Vec<_>>(),
        "blockers": report.blockers,
        "captured_at": report.captured_at,
    })
}

pub(super) fn move_outcome_json(
    outcome: &memory_kernel::storage::move_kernel::MoveOutcome
) -> serde_json::Value {
    serde_json::json!({
        "resumed": outcome.resumed,
        "rolled_back": outcome.rolled_back,
        "journal": {
            "id": outcome.journal.id,
            "entity_id": outcome.journal.entity_id,
            "source_store_root": path_display(&outcome.journal.source_store_root),
            "target_store_root": path_display(&outcome.journal.target_store_root),
            "source_entity_path": path_display(&outcome.journal.source_entity_path),
            "destination_entity_path": path_display(&outcome.journal.destination_entity_path),
            "phase": outcome.journal.phase,
            "created_at": outcome.journal.created_at,
            "updated_at": outcome.journal.updated_at,
            "steps": outcome.journal.steps,
            "rollback_steps": outcome.journal.rollback_steps,
            "lock_paths": outcome.journal.lock_paths,
            "migrated_board_entries": outcome.journal.migrated_board_entries,
            "rewritten_path_files": outcome.journal.rewritten_path_files,
            "manual_followups": outcome.journal.manual_followups,
            "failure": outcome.journal.failure,
            "next_recovery_step": outcome.journal.next_recovery_step,
        },
    })
}
