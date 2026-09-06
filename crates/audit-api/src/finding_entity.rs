//! Entity-folder persistence for moveable audit findings.
//!
//! This is distinct from the ephemeral SQLite-backed findings the `audit`
//! command regenerates on every scan. A persisted finding is written once,
//! explicitly, via [`RepositoryIndex::persist_finding_entity`] and lives at
//! `<audit index dir>/findings/<uuid>/finding.json`, so the domain-neutral
//! move kernel ([`crate::move_domain::AuditMoveDomain`]) can address, move,
//! and journal it like any other entity-folder record. Repository-level
//! artifacts such as `README.md` and `index.toon` are never represented this
//! way and therefore remain unsupported for entity moves.

use std::{
    fs,
    path::PathBuf,
};

use serde::{
    Deserialize,
    Serialize,
};
use uuid::Uuid;

use crate::{
    error::AuditError,
    index::RepositoryIndex,
    models::{
        AuditFinding,
        Severity,
    },
};

pub const FINDING_ENTITY_SUBDIR: &str = "findings";
pub const FINDING_ENTITY_FILE: &str = "finding.json";

/// A durably persisted, entity-addressable audit finding.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PersistedFinding {
    pub id: Uuid,
    pub category: String,
    pub severity: Severity,
    pub summary: String,
    pub path: Option<String>,
    pub line: Option<usize>,
    pub metric_name: String,
    pub metric_value: serde_json::Value,
    pub threshold: Option<serde_json::Value>,
    pub instructions: Vec<String>,
    pub evidence: serde_json::Value,
}

impl PersistedFinding {
    /// Build a persisted finding from a scan-produced [`AuditFinding`],
    /// assigning it the given stable entity id.
    pub fn from_finding(
        id: Uuid,
        finding: &AuditFinding,
    ) -> Self {
        Self {
            id,
            category: finding.category.clone(),
            severity: finding.severity.clone(),
            summary: finding.summary.clone(),
            path: finding.path.clone(),
            line: finding.line,
            metric_name: finding.metric_name.clone(),
            metric_value: finding.metric_value.clone(),
            threshold: finding.threshold.clone(),
            instructions: finding.instructions.clone(),
            evidence: finding.evidence.clone(),
        }
    }
}

impl RepositoryIndex {
    /// Root directory holding entity-folder findings: `<index dir>/findings`.
    pub fn finding_entities_root(&self) -> PathBuf {
        self.db_path()
            .parent()
            .expect("db_path always has a parent")
            .join(FINDING_ENTITY_SUBDIR)
    }

    /// On-disk folder for `id`, if it exists.
    pub fn finding_entity_path(
        &self,
        id: &Uuid,
    ) -> Option<PathBuf> {
        let path = self.finding_entities_root().join(id.to_string());
        path.is_dir().then_some(path)
    }

    /// Persist `finding` as a new (or overwritten) entity folder, returning
    /// the folder path.
    pub fn persist_finding_entity(
        &self,
        finding: &PersistedFinding,
    ) -> Result<PathBuf, AuditError> {
        let dir = self.finding_entities_root().join(finding.id.to_string());
        fs::create_dir_all(&dir)?;
        let payload = serde_json::to_string_pretty(finding)?;
        fs::write(dir.join(FINDING_ENTITY_FILE), payload)?;
        Ok(dir)
    }

    /// Load a persisted finding entity, if present.
    pub fn load_finding_entity(
        &self,
        id: &Uuid,
    ) -> Result<Option<PersistedFinding>, AuditError> {
        let Some(dir) = self.finding_entity_path(id) else {
            return Ok(None);
        };
        let content = fs::read_to_string(dir.join(FINDING_ENTITY_FILE))?;
        Ok(Some(serde_json::from_str(&content)?))
    }

    /// List every persisted finding entity id under this store, sorted.
    pub fn list_finding_entity_ids(&self) -> Result<Vec<Uuid>, AuditError> {
        let root = self.finding_entities_root();
        if !root.is_dir() {
            return Ok(Vec::new());
        }
        let mut ids = Vec::new();
        for entry in fs::read_dir(&root)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            if let Some(name) = entry.file_name().to_str() {
                if let Ok(id) = name.parse::<Uuid>() {
                    ids.push(id);
                }
            }
        }
        ids.sort();
        Ok(ids)
    }
}
