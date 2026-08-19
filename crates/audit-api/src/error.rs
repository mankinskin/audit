use thiserror::Error;

#[derive(Debug, Error)]
pub enum AuditError {
    #[error("repository root does not exist: {0}")]
    MissingRepoRoot(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("config parse error: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("json error: {0}")]
    SerdeJson(#[from] serde_json::Error),
    #[error("command `{command}` failed: {details}")]
    CommandFailed { command: String, details: String },
    #[error("audit move failed: {0}")]
    Move(String),
    #[error(
        "workspace not initialized at {path}: run 'audit run' or 'RepositoryIndex::init' to create a new index"
    )]
    WorkspaceNotFound { path: String },
}

impl memory_kernel::storage::NotFoundError for AuditError {
    fn is_workspace_not_found(&self) -> bool {
        matches!(self, AuditError::WorkspaceNotFound { .. })
    }
}
