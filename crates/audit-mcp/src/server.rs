use std::{path::PathBuf, sync::Arc};

use audit_api::{
    audit,
    index::RepositoryIndex,
    models::AuditConfig,
    summary::{AuditSummaryBy, summarize_report},
};
use rmcp::{
    ErrorData as McpError, ServerHandler, ServiceExt,
    handler::server::{tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, Content},
    schemars::{self, JsonSchema},
    tool, tool_handler, tool_router,
    transport::stdio,
};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use uuid::Uuid;

#[path = "server_move_json.rs"]
mod server_move_json;

use server_move_json::{move_outcome_json, move_plan_json, path_display};

#[cfg(test)]
mod contract_tests {
    use super::*;

    #[test]
    fn advertised_audit_contracts_expose_execution_and_store_requirements() {
        let tools = AuditServer::tool_router().list_all();
        assert_eq!(tools.len(), 6);
        for name in ["audit", "audit_summary"] {
            let tool = tools.iter().find(|tool| tool.name == name).unwrap();
            let value = serde_json::to_value(tool).unwrap();
            assert!(value["inputSchema"]["properties"]["repo_root"]["description"]
                .as_str().unwrap().contains("Required explicit"));
            assert!(tool.description.as_deref().unwrap().contains("persist"));
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AuditRepositoryInput {
    /// Required explicit repository path despite optional schema shape. Omitted/null/empty/default/.. fail; . means server cwd.
    #[serde(default)]
    pub repo_root: Option<PathBuf>,
    /// File length threshold in lines. Omit/null: 400; zero is literal.
    #[serde(default)]
    pub max_file_lines: Option<usize>,
    /// Complexity threshold. Omit/null: 12; zero is literal.
    #[serde(default)]
    pub max_cyclomatic_complexity: Option<usize>,
    /// Coverage warning threshold in percent. Omit/null: 80.0; zero is literal.
    #[serde(default)]
    pub coverage_warn_below: Option<f64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AuditSummaryByInput {
    Crate,
    Package,
    Category,
    Severity,
    Metric,
    Path,
}

impl From<AuditSummaryByInput> for AuditSummaryBy {
    fn from(value: AuditSummaryByInput) -> Self {
        match value {
            AuditSummaryByInput::Crate | AuditSummaryByInput::Package => AuditSummaryBy::Crate,
            AuditSummaryByInput::Category => AuditSummaryBy::Category,
            AuditSummaryByInput::Severity => AuditSummaryBy::Severity,
            AuditSummaryByInput::Metric => AuditSummaryBy::Metric,
            AuditSummaryByInput::Path => AuditSummaryBy::Path,
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AuditSummaryInput {
    /// Required explicit repository path despite optional schema shape. Omitted/null/empty/default/.. fail; . means server cwd.
    #[serde(default)]
    pub repo_root: Option<PathBuf>,
    /// Group by crate/package (aliases), category, severity, metric or path.
    pub by: AuditSummaryByInput,
    /// File length threshold. Omit/null: 400; zero is literal.
    #[serde(default)]
    pub max_file_lines: Option<usize>,
    /// Complexity threshold. Omit/null: 12; zero is literal.
    #[serde(default)]
    pub max_cyclomatic_complexity: Option<usize>,
    /// Coverage percentage warning threshold. Omit/null: 80.0; zero is literal.
    #[serde(default)]
    pub coverage_warn_below: Option<f64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AuditMoveInput {
    /// Source repo path. Preflight omission uses server startup cwd; apply requires explicit selection.
    #[serde(default)]
    pub repo_root: Option<PathBuf>,
    /// Finding entity UUID, not the repository audit index.
    pub id: String,
    /// Explicit destination workspace root; . means server cwd.
    pub to_workspace_root: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AuditMoveJournalInput {
    /// Source repo path; omission uses server startup cwd.
    #[serde(default)]
    pub repo_root: Option<PathBuf>,
    /// Move journal UUID from audit_move_apply, not finding entity UUID.
    pub id: String,
}

#[derive(Clone)]
pub struct AuditServer {
    base_dir: PathBuf,
    tool_router: ToolRouter<Self>,
    audit_lock: Arc<Mutex<()>>,
}

impl AuditServer {
    pub fn new(base_dir: PathBuf) -> Self {
        Self {
            base_dir,
            tool_router: Self::tool_router(),
            audit_lock: Arc::new(Mutex::new(())),
        }
    }

    fn repo_root(&self, repo_root: Option<PathBuf>) -> PathBuf {
        repo_root.unwrap_or_else(|| self.base_dir.clone())
    }

    fn explicit_repo_root(repo_root: Option<PathBuf>) -> Result<PathBuf, McpError> {
        let repo_root = repo_root.ok_or_else(|| {
            McpError::invalid_params(
                "repo_root is required for audit writes; provide an explicit workspace path",
                None,
            )
        })?;
        let selector = repo_root.to_string_lossy();
        memory_kernel::workspace::normalize_explicit_workspace_selector(Some(&selector))
            .map_err(|error| McpError::invalid_params(error.to_string(), None))
    }

    fn normalize_target_workspace(selector: &str) -> Result<PathBuf, McpError> {
        memory_kernel::workspace::normalize_explicit_workspace_selector(Some(selector))
            .map_err(|err| McpError::invalid_params(err.to_string(), None))
    }

    fn json_result<T: Serialize>(value: &T) -> Result<CallToolResult, McpError> {
        let text = serde_json::to_string(value)
            .map_err(|err| McpError::internal_error(format!("serialization: {err}"), None))?;
        Ok(CallToolResult::success(vec![Content::text(text)]))
    }

    fn build_config(
        max_file_lines: Option<usize>,
        max_cyclomatic_complexity: Option<usize>,
        coverage_warn_below: Option<f64>,
    ) -> AuditConfig {
        let mut config = AuditConfig::default();

        if let Some(max_file_lines) = max_file_lines {
            config.max_file_lines = max_file_lines;
        }
        if let Some(max_cyclomatic_complexity) = max_cyclomatic_complexity {
            config.max_cyclomatic_complexity = max_cyclomatic_complexity;
        }
        if let Some(coverage_warn_below) = coverage_warn_below {
            config.coverage_warn_below = coverage_warn_below;
        }

        config
    }
}

#[tool_router]
impl AuditServer {
    #[tool(
        name = "audit",
        description = "Run a full repository audit, not a read-only inspection: creates/updates the audit index, syncs sources and persists an audit run; runs cargo check --all-targets, cargo test --lib --tests --no-fail-fast and optional cargo llvm-cov without timeout. repo_root requires an explicit path (. means server cwd); omitted/empty/default/.. fail. Omitted/null thresholds: max_file_lines=400, max_cyclomatic_complexity=12, coverage_warn_below=80 percent; zero is literal. Reads .audit.toml, which may enable external checks. Returns structured metrics/findings/run evidence."
    )]
    async fn audit(
        &self,
        Parameters(input): Parameters<AuditRepositoryInput>,
    ) -> Result<CallToolResult, McpError> {
        let repo_root = Self::explicit_repo_root(input.repo_root)?;
        let _guard = self.audit_lock.lock().await;
        let config = Self::build_config(
            input.max_file_lines,
            input.max_cyclomatic_complexity,
            input.coverage_warn_below,
        );

        let report = audit::audit(&repo_root, config)
            .map_err(|err| McpError::internal_error(err.to_string(), None))?;

        Self::json_result(&report)
    }

    #[tool(
        name = "audit_summary",
        description = "Run the same full audit as audit, including command execution, index updates and persisted run; return grouped counts, not a cached summary. Explicit repo_root required. by supports crate/package (aliases), category, severity, metric, path; crate grouping uses cargo metadata."
    )]
    async fn audit_summary(
        &self,
        Parameters(input): Parameters<AuditSummaryInput>,
    ) -> Result<CallToolResult, McpError> {
        let repo_root = Self::explicit_repo_root(input.repo_root)?;
        let _guard = self.audit_lock.lock().await;
        let config = Self::build_config(
            input.max_file_lines,
            input.max_cyclomatic_complexity,
            input.coverage_warn_below,
        );

        let report = audit::audit(&repo_root, config)
            .map_err(|err| McpError::internal_error(err.to_string(), None))?;
        let summary = summarize_report(&report, input.by.into())
            .map_err(|err| McpError::internal_error(err.to_string(), None))?;

        Self::json_result(&summary)
    }

    #[tool(
        name = "audit_move_preflight",
        description = "Preflight a persisted finding entity UUID move; no move occurs. Omitted repo_root uses server startup cwd; destination must be explicit. Only finding entity folders are movable, not the repository audit index. Returns status ok/blocked and plan.blockers."
    )]
    async fn audit_move_preflight(
        &self,
        Parameters(input): Parameters<AuditMoveInput>,
    ) -> Result<CallToolResult, McpError> {
        let _guard = self.audit_lock.lock().await;
        let target_workspace_root = Self::normalize_target_workspace(&input.to_workspace_root)?;
        let repo_root = self.repo_root(input.repo_root);
        let audit_id = input.id.parse::<Uuid>().map_err(|error| {
            McpError::invalid_params(format!("invalid audit UUID: {error}"), None)
        })?;
        let report = RepositoryIndex::open(&repo_root)
            .map_err(|err| McpError::internal_error(err.to_string(), None))?
            .plan_move_preflight(&audit_id, &target_workspace_root)
            .map_err(|err| McpError::internal_error(err.to_string(), None))?;

        Self::json_result(&serde_json::json!({
            "command": "move",
            "status": if report.supported() { "ok" } else { "blocked" },
            "mode": "preflight",
            "repo_root": path_display(&repo_root),
            "id": audit_id,
            "plan": move_plan_json(&report),
            "recovery": {"resume": "audit move --resume <journal-uuid>", "rollback": "audit move --rollback <journal-uuid>"},
        }))
    }

    #[tool(
        name = "audit_move_apply",
        description = "Re-plan and move a supported finding between stores, with explicit repo_root/destination; blocked preflight fails. Retain journal identity/phase from the outcome for audit_move_resume/rollback; do not blindly replay apply."
    )]
    async fn audit_move_apply(
        &self,
        Parameters(input): Parameters<AuditMoveInput>,
    ) -> Result<CallToolResult, McpError> {
        let _guard = self.audit_lock.lock().await;
        let target_workspace_root = Self::normalize_target_workspace(&input.to_workspace_root)?;
        let repo_root = Self::explicit_repo_root(input.repo_root)?;
        let audit_id = input.id.parse::<Uuid>().map_err(|error| {
            McpError::invalid_params(format!("invalid audit UUID: {error}"), None)
        })?;
        let index = RepositoryIndex::open(&repo_root)
            .map_err(|err| McpError::internal_error(err.to_string(), None))?;
        let report = index
            .plan_move_preflight(&audit_id, &target_workspace_root)
            .map_err(|err| McpError::internal_error(err.to_string(), None))?;
        if !report.supported() {
            return Err(McpError::invalid_params(
                "move preflight blocked; run audit_move_preflight for details".to_string(),
                None,
            ));
        }
        let outcome = index
            .execute_move_with_journal(&report)
            .map_err(|err| McpError::internal_error(err.to_string(), None))?;

        Self::json_result(&serde_json::json!({
            "command": "move",
            "status": "ok",
            "mode": "apply",
            "repo_root": path_display(&repo_root),
            "id": audit_id,
            "plan": move_plan_json(&report),
            "outcome": move_outcome_json(&outcome),
            "recovery": {"resume": "audit move --resume <journal-uuid>", "rollback": "audit move --rollback <journal-uuid>"},
        }))
    }

    #[tool(
        name = "audit_move_resume",
        description = "Resume an interrupted finding move using id as journal UUID, not finding UUID. Omitted repo_root uses server startup cwd. Returns journal_id/phase."
    )]
    async fn audit_move_resume(
        &self,
        Parameters(input): Parameters<AuditMoveJournalInput>,
    ) -> Result<CallToolResult, McpError> {
        let _guard = self.audit_lock.lock().await;
        let repo_root = self.repo_root(input.repo_root);
        let journal = input.id.parse::<Uuid>().map_err(|error| {
            McpError::invalid_params(format!("invalid journal id: {error}"), None)
        })?;
        let outcome = RepositoryIndex::open(&repo_root)
            .map_err(|err| McpError::internal_error(err.to_string(), None))?
            .resume_move_with_journal(journal)
            .map_err(|err| McpError::internal_error(err.to_string(), None))?;

        Self::json_result(&serde_json::json!({
            "command": "move",
            "status": "ok",
            "mode": "resume",
            "repo_root": path_display(&repo_root),
            "journal_id": outcome.journal.id,
            "phase": outcome.journal.phase,
            "recovery": {"resume": "audit move --resume <journal-uuid>", "rollback": "audit move --rollback <journal-uuid>"},
        }))
    }

    #[tool(
        name = "audit_move_rollback",
        description = "Roll back a finding move using id as journal UUID, not finding UUID. Omitted repo_root uses server startup cwd. Returns journal_id/phase."
    )]
    async fn audit_move_rollback(
        &self,
        Parameters(input): Parameters<AuditMoveJournalInput>,
    ) -> Result<CallToolResult, McpError> {
        let _guard = self.audit_lock.lock().await;
        let repo_root = self.repo_root(input.repo_root);
        let journal = input.id.parse::<Uuid>().map_err(|error| {
            McpError::invalid_params(format!("invalid journal id: {error}"), None)
        })?;
        let outcome = RepositoryIndex::open(&repo_root)
            .map_err(|err| McpError::internal_error(err.to_string(), None))?
            .rollback_move_with_journal(journal)
            .map_err(|err| McpError::internal_error(err.to_string(), None))?;

        Self::json_result(&serde_json::json!({
            "command": "move",
            "status": "ok",
            "mode": "rollback",
            "repo_root": path_display(&repo_root),
            "journal_id": outcome.journal.id,
            "phase": outcome.journal.phase,
            "recovery": {"resume": "audit move --resume <journal-uuid>", "rollback": "audit move --rollback <journal-uuid>"},
        }))
    }
}

#[tool_handler]
impl ServerHandler for AuditServer {
    fn get_info(&self) -> rmcp::model::ServerInfo {
        rmcp::model::ServerInfo {
            server_info: rmcp::model::Implementation {
                name: env!("CARGO_PKG_NAME").to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                ..Default::default()
            },
            instructions: Some(
                "Use audit for the full report, audit_summary for grouped issue counts, or audit_move_* for move preflight/apply/resume/rollback.".
                    to_string(),
            ),
            capabilities: rmcp::model::ServerCapabilities::builder().enable_tools().build(),
            ..Default::default()
        }
    }
}

pub async fn run_mcp_server(
    base_dir: PathBuf,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let server = AuditServer::new(base_dir);

    tracing::info!("Starting audit-mcp server on stdio");

    let service = server.serve(stdio()).await.inspect_err(|err| {
        eprintln!("Server error: {err:?}");
    })?;

    service.waiting().await?;
    Ok(())
}

#[cfg(test)]
#[path = "server_tests.rs"]
mod server_tests;
