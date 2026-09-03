//! Audit-domain adapter onto the domain-neutral move kernel.
//!
//! The audit store is currently a repository-level SQLite index plus generated
//! catalog artifacts, not a folder-per-entity store. This adapter is therefore
//! intentionally fail-closed: callers can use the shared kernel preflight shape,
//! but no audit entity can be moved until audit introduces persisted entity
//! folders.

use std::path::{
    Path,
    PathBuf,
};

use memory_kernel::storage::move_kernel::{
    self,
    MoveDomain,
    MoveError,
    MoveOutcome,
    MovePlan,
    MoveReferences,
    MoveResult,
    MoveSetPlan,
};
use uuid::Uuid;

use crate::{
    error::AuditError,
    index::RepositoryIndex,
};

const AUDIT_INDEX_DIR: &str = ".audit";
const AUDIT_ENTITY_DIR: &str = "findings";

fn to_move_error(error: AuditError) -> MoveError {
    match error {
        AuditError::Io(io) => MoveError::Io(io),
        other => MoveError::Domain(other.to_string()),
    }
}

fn from_move_error(error: MoveError) -> AuditError {
    match error {
        MoveError::Io(io) => AuditError::Move(io.to_string()),
        MoveError::Domain(message) => AuditError::Move(message),
        MoveError::InteroperabilityContract {
            artifact_class,
            detail,
        } => AuditError::Move(format!(
            "interoperability contract violation for {artifact_class}: {detail}"
        )),
    }
}

/// Audit-domain implementation of the move kernel's [`MoveDomain`] trait.
pub struct AuditMoveDomain<'a> {
    index: &'a RepositoryIndex,
}

impl<'a> AuditMoveDomain<'a> {
    pub fn new(index: &'a RepositoryIndex) -> Self {
        Self { index }
    }
}

impl MoveDomain for AuditMoveDomain<'_> {
    fn entity_subdir(&self) -> &str {
        AUDIT_ENTITY_DIR
    }

    fn store_index_dir(&self) -> &str {
        AUDIT_INDEX_DIR
    }

    fn source_store_root(&self) -> PathBuf {
        self.index
            .db_path()
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from(AUDIT_INDEX_DIR))
    }

    fn source_entity_path(
        &self,
        _entity_id: &Uuid,
    ) -> MoveResult<Option<PathBuf>> {
        Ok(None)
    }

    fn related_entities(
        &self,
        _entity_id: &Uuid,
    ) -> MoveResult<MoveReferences> {
        Ok(MoveReferences::default())
    }

    fn target_store_present(
        &self,
        target_store_root: &Path,
    ) -> MoveResult<bool> {
        Ok(target_store_root.is_dir())
    }

    fn entity_indexed_in(
        &self,
        _store_root: &Path,
        _entity_id: &Uuid,
    ) -> MoveResult<bool> {
        Ok(false)
    }

    fn scan_store(
        &self,
        store_root: &Path,
    ) -> MoveResult<()> {
        let workspace_root =
            memory_kernel::workspace::resolve_workspace_root_from_store_root(
                store_root,
                AUDIT_INDEX_DIR,
            );
        RepositoryIndex::open(&workspace_root).map_err(to_move_error)?;
        Ok(())
    }
}

impl RepositoryIndex {
    /// Build one normalized preflight plan for a set of audit entities.
    pub fn plan_move_set(
        &self,
        audit_entity_ids: &[Uuid],
        target_workspace_root: &Path,
    ) -> Result<MoveSetPlan, AuditError> {
        let domain = AuditMoveDomain::new(self);
        move_kernel::plan_move_set(&domain, audit_entity_ids, target_workspace_root)
            .map_err(from_move_error)
    }

    /// Build a read-only preflight plan for an audit entity move.
    ///
    /// Audit has no folder-per-entity records today, so the returned plan is
    /// expected to include `MissingSourceEntity` for every id.
    pub fn plan_move_preflight(
        &self,
        audit_entity_id: &Uuid,
        target_workspace_root: &Path,
    ) -> Result<MovePlan, AuditError> {
        let domain = AuditMoveDomain::new(self);
        move_kernel::plan_move(&domain, audit_entity_id, target_workspace_root)
            .map_err(from_move_error)
    }

    /// Execute a supported audit move with a fresh journal.
    pub fn execute_move_with_journal(
        &self,
        plan: &MovePlan,
    ) -> Result<MoveOutcome, AuditError> {
        let domain = AuditMoveDomain::new(self);
        move_kernel::execute_move(&domain, plan).map_err(from_move_error)
    }

    /// Resume an interrupted audit move from its journal id.
    pub fn resume_move_with_journal(
        &self,
        journal_id: Uuid,
    ) -> Result<MoveOutcome, AuditError> {
        let domain = AuditMoveDomain::new(self);
        move_kernel::resume_move(&domain, journal_id).map_err(from_move_error)
    }

    /// Roll back an audit move from its journal id.
    pub fn rollback_move_with_journal(
        &self,
        journal_id: Uuid,
    ) -> Result<MoveOutcome, AuditError> {
        let domain = AuditMoveDomain::new(self);
        move_kernel::rollback_move(&domain, journal_id).map_err(from_move_error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use memory_kernel::storage::move_kernel::MoveBlocker;
    use std::process::Command;
    use tempfile::tempdir;

    fn run_git(
        repo_root: &Path,
        args: &[&str],
    ) {
        let status = Command::new("git")
            .current_dir(repo_root)
            .args(args)
            .status()
            .expect("git command");
        assert!(status.success(), "git {args:?} failed: {status}");
    }

    #[test]
    fn audit_move_preflight_is_fail_closed_until_audit_has_entity_folders() {
        let temp = tempdir().unwrap();
        let repo = temp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        run_git(&repo, &["init"]);

        let source_workspace = repo.join("source");
        let target_workspace = repo.join("target");
        std::fs::create_dir_all(&source_workspace).unwrap();
        std::fs::create_dir_all(target_workspace.join(AUDIT_INDEX_DIR))
            .unwrap();

        let index = RepositoryIndex::init(&source_workspace).unwrap();
        let audit_entity_id = Uuid::new_v4();
        let plan = index
            .plan_move_preflight(&audit_entity_id, &target_workspace)
            .unwrap();

        assert!(plan.blockers.iter().any(|blocker| matches!(
            blocker,
            MoveBlocker::MissingSourceEntity { entity_id } if *entity_id == audit_entity_id
        )), "expected missing source entity blocker: {:?}", plan.blockers);
    }
}
