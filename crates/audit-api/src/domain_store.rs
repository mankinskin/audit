//! Audit-domain adapter for the shared workspace store contract.
//!
//! Audit retains its repository-specific SQLite index and persisted-finding
//! representation.  This module only supplies the shared workspace boundary
//! and the two typed finding operations that Audit currently supports.

use std::path::Path;

use memory_kernel::{
    domain_store::{
        CreateEntity, DomainStore, DomainStoreError, DomainStoreResolution, ReadEntity,
        StoreAccessMode,
    },
    model::domain::DomainId,
};
use uuid::Uuid;

use crate::{
    error::AuditError,
    finding_entity::PersistedFinding,
    index::{INDEX_DIR, RepositoryIndex},
};

impl DomainStore for RepositoryIndex {
    fn domain_id() -> DomainId {
        DomainId::new("audit").expect("audit is a valid domain id")
    }

    fn store_dir_name() -> &'static str {
        INDEX_DIR
    }
}

impl RepositoryIndex {
    /// Resolve an explicitly selected Audit workspace through the shared
    /// contract. Read-only access never creates a missing store; writes always
    /// select the canonical `.workflow-tools/audit` store.
    pub fn resolve_workspace_store(
        local_workspace: &Path,
        access_mode: StoreAccessMode,
    ) -> Result<DomainStoreResolution, AuditError> {
        if access_mode == StoreAccessMode::CreateOrOpen {
            memory_kernel::workspace::validate_explicit_store_root_for_write(
                local_workspace,
                INDEX_DIR,
            )
            .map_err(|error| AuditError::Move(error.to_string()))?;
        }

        <Self as DomainStore>::resolve_store(local_workspace, access_mode).map_err(|error| {
            match error {
                DomainStoreError::Initialize { source, .. } => AuditError::Io(source),
            }
        })
    }
}

impl CreateEntity for RepositoryIndex {
    type Entity = PersistedFinding;
    type CreateInput = PersistedFinding;
    type CreateResult = std::path::PathBuf;
    type Error = AuditError;

    fn create_entity(&self, input: Self::CreateInput) -> Result<Self::CreateResult, Self::Error> {
        self.persist_finding_entity(&input)
    }
}

impl ReadEntity for RepositoryIndex {
    type EntityId = Uuid;
    type Entity = PersistedFinding;
    type ReadResult = Option<PersistedFinding>;
    type Error = AuditError;

    fn read_entity(&self, id: Self::EntityId) -> Result<Self::ReadResult, Self::Error> {
        self.load_finding_entity(&id)
    }
}

#[cfg(test)]
mod tests {
    use memory_kernel::{
        domain_store::{CreateEntity, ReadEntity, StoreAccessMode},
        workspace::canonical_store_root,
    };
    use tempfile::tempdir;
    use uuid::Uuid;

    use super::*;
    use crate::models::Severity;

    fn finding(id: Uuid) -> PersistedFinding {
        PersistedFinding {
            id,
            category: "file-length".to_string(),
            severity: Severity::Medium,
            summary: "selected-store finding".to_string(),
            path: Some("src/lib.rs".to_string()),
            line: Some(7),
            metric_name: "line_count".to_string(),
            metric_value: serde_json::json!(500),
            threshold: Some(serde_json::json!(400)),
            instructions: vec!["split the file".to_string()],
            evidence: serde_json::json!({"lines": 500}),
        }
    }

    #[test]
    fn selected_workspace_capabilities_write_and_read_back_from_its_canonical_store() {
        let parent = tempdir().unwrap();
        let selected = parent.path().join("selected");
        let sibling = parent.path().join("sibling");
        std::fs::create_dir_all(&selected).unwrap();
        std::fs::create_dir_all(&sibling).unwrap();

        let resolution =
            RepositoryIndex::resolve_workspace_store(&selected, StoreAccessMode::CreateOrOpen)
                .unwrap();
        assert_eq!(
            resolution.store_root,
            canonical_store_root(&selected, INDEX_DIR)
        );

        let index = RepositoryIndex::init(&selected).unwrap();
        let persisted = finding(Uuid::new_v4());
        index.create_entity(persisted.clone()).unwrap();

        assert_eq!(
            index.read_entity(persisted.id).unwrap(),
            Some(persisted.clone())
        );
        assert!(
            canonical_store_root(&selected, INDEX_DIR)
                .join("findings")
                .join(persisted.id.to_string())
                .join("finding.json")
                .is_file()
        );
        assert!(!canonical_store_root(parent.path(), INDEX_DIR).exists());
        assert!(!canonical_store_root(&sibling, INDEX_DIR).exists());
    }

    #[test]
    fn read_only_resolution_preserves_legacy_store_without_initializing_canonical_store() {
        let workspace = tempdir().unwrap();
        let legacy = workspace.path().join(INDEX_DIR);
        std::fs::create_dir_all(&legacy).unwrap();

        let resolution =
            RepositoryIndex::resolve_workspace_store(workspace.path(), StoreAccessMode::ReadOnly)
                .unwrap();

        assert_eq!(resolution.store_root, legacy);
        assert!(!canonical_store_root(workspace.path(), INDEX_DIR).exists());
    }
}
