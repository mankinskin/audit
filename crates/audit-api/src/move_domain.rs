//! Audit-domain adapter onto the domain-neutral move kernel.
//!
//! The audit store is primarily a repository-level SQLite index plus generated
//! catalog artifacts (`.audit/README.md`, `.audit/index.toon`), neither of
//! which is an entity-folder record and neither of which is ever moveable
//! through this adapter. Alongside that repository-level index, a finding may
//! additionally be persisted as an entity folder under
//! `.audit/findings/<uuid>/` (see [`crate::finding_entity`]); only such
//! persisted findings resolve a source path here. Every other id — including
//! any id addressing only the repository-level SQLite index or catalog
//! artifacts — remains fail-closed via `MissingSourceEntity`.

use std::{
    collections::BTreeMap,
    path::{
        Path,
        PathBuf,
    },
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
    finding_entity::FINDING_ENTITY_SUBDIR,
    index::RepositoryIndex,
};

const AUDIT_INDEX_DIR: &str = ".audit";
const AUDIT_ENTITY_DIR: &str = FINDING_ENTITY_SUBDIR;

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
        entity_id: &Uuid,
    ) -> MoveResult<Option<PathBuf>> {
        Ok(self.index.finding_entity_path(entity_id))
    }

    fn source_entity_paths_for_set(
        &self,
        entity_ids: &[Uuid],
    ) -> MoveResult<BTreeMap<Uuid, PathBuf>> {
        Ok(entity_ids
            .iter()
            .filter_map(|entity_id| {
                self.index
                    .finding_entity_path(entity_id)
                    .map(|path| (*entity_id, path))
            })
            .collect())
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
        store_root: &Path,
        entity_id: &Uuid,
    ) -> MoveResult<bool> {
        Ok(store_root
            .join(AUDIT_ENTITY_DIR)
            .join(entity_id.to_string())
            .is_dir())
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

    fn digest_file(path: &Path) -> String {
        use sha2::{Digest, Sha256};
        let bytes = std::fs::read(path).unwrap();
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        format!("{:x}", hasher.finalize())
    }

    fn persist_sample_finding(index: &RepositoryIndex, id: Uuid) -> PathBuf {
        use crate::{finding_entity::PersistedFinding, models::Severity};
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
        index.persist_finding_entity(&finding).unwrap()
    }

    #[test]
    fn audit_move_round_trips_entity_folder_finding_via_apply_and_rollback() {
        let temp = tempdir().unwrap();
        let repo = temp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        run_git(&repo, &["init"]);

        let source_workspace = repo.join("source");
        let target_workspace = repo.join("target");
        std::fs::create_dir_all(&source_workspace).unwrap();
        std::fs::create_dir_all(&target_workspace).unwrap();
        RepositoryIndex::init(&target_workspace).unwrap();

        let source_index = RepositoryIndex::init(&source_workspace).unwrap();
        let finding_id = Uuid::new_v4();
        let source_dir = persist_sample_finding(&source_index, finding_id);
        let original_digest = digest_file(&source_dir.join("finding.json"));

        // An unrelated finding must be preserved untouched by the move.
        let unrelated_id = Uuid::new_v4();
        let unrelated_dir =
            persist_sample_finding(&source_index, unrelated_id);
        let unrelated_digest = digest_file(&unrelated_dir.join("finding.json"));

        let plan = source_index
            .plan_move_preflight(&finding_id, &target_workspace)
            .unwrap();
        assert!(plan.supported(), "expected supported plan: {:?}", plan.blockers);

        let outcome = source_index.execute_move_with_journal(&plan).unwrap();
        assert!(!source_dir.exists(), "source folder should be moved away");
        assert!(
            plan.destination_entity_path.join("finding.json").is_file(),
            "destination folder should hold the moved finding"
        );
        assert_eq!(
            digest_file(&plan.destination_entity_path.join("finding.json")),
            original_digest,
            "moved content must be byte-identical"
        );

        // Unrelated entity untouched.
        assert!(unrelated_dir.is_dir());
        assert_eq!(digest_file(&unrelated_dir.join("finding.json")), unrelated_digest);

        let rollback = source_index
            .rollback_move_with_journal(outcome.journal.id)
            .unwrap();
        assert!(rollback.rolled_back);
        assert!(source_dir.join("finding.json").is_file());
        assert_eq!(
            digest_file(&source_dir.join("finding.json")),
            original_digest,
            "rollback must restore the original checksum"
        );
        assert!(!plan.destination_entity_path.exists());
    }

    #[test]
    fn audit_move_rejects_repository_level_only_layout() {
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
        // Simulate the repository-level catalog artifacts; neither is an
        // entity folder and neither must ever resolve a source path.
        std::fs::write(
            source_workspace.join(AUDIT_INDEX_DIR).join("README.md"),
            "generated",
        )
        .unwrap();
        std::fs::write(
            source_workspace.join(AUDIT_INDEX_DIR).join("index.toon"),
            "generated",
        )
        .unwrap();

        let repository_level_id = Uuid::new_v4();
        let plan = index
            .plan_move_preflight(&repository_level_id, &target_workspace)
            .unwrap();
        assert!(!plan.supported());
        assert!(plan.blockers.iter().any(|blocker| matches!(
            blocker,
            MoveBlocker::MissingSourceEntity { entity_id } if *entity_id == repository_level_id
        )));
    }
}
