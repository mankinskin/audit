use std::{
    ffi::OsString,
    fs,
    path::{
        Path,
        PathBuf,
    },
};

use clap::{
    Args,
    Parser,
    Subcommand,
    ValueEnum,
};
use serde_json::{
    Value,
    json,
};

use audit_api::{
    audit::audit,
    error::AuditError,
    index::RepositoryIndex,
    models::{
        AuditConfig,
        AuditReport,
        TrialStatus,
    },
    store_index::{
        AUDIT_INDEX_AGENT_HOOK_PATH,
        AuditCatalogSource,
        generate_audit_catalog,
    },
    summary::{
        AuditSummaryBy,
        AuditSummaryReport,
        summarize_report,
    },
};
use memory_kernel::generated_markdown::prepare_generated_output;
use session_api::{
    SessionAuditReport,
    SessionAuditSelector,
    SessionStoreConfig,
};
use uuid::Uuid;

#[derive(Debug, Parser)]
#[command(
    name = "audit",
    about = "Repository quality audit CLI",
    version,
    arg_required_else_help = true
)]
pub struct AuditCli {
    #[arg(long, global = true, conflicts_with = "toon")]
    pub json: bool,

    #[arg(long, global = true, conflicts_with = "json")]
    pub toon: bool,

    #[command(subcommand)]
    pub command: AuditCommand,
}

#[derive(Debug, Subcommand)]
pub enum AuditCommand {
    /// Run an audit for a repository.
    Run(AuditArgs),

    /// Move an audit repository root to another workspace store.
    Move(MoveArgs),

    /// Generate (or check) the committed audit status catalog artifacts:
    /// `.audit/README.md`, `.audit/index.toon`, and `.agents/audit-catalog.md`.
    ///
    /// Runs a full audit then writes the catalog. Not included in the pre-commit
    /// hook (Q6.3) because full audits are too expensive for commit-time checks;
    /// use manually or in CI.
    StoreIndex(StoreIndexArgs),

    /// Summarize findings grouped by one key.
    Summary(AuditSummaryArgs),
}

#[derive(Debug, Args)]
pub struct StoreIndexArgs {
    /// Repository root to audit.
    #[arg(default_value = ".")]
    pub repo_root: PathBuf,

    /// Check whether the committed catalog is up to date instead of writing it.
    /// Exits non-zero if any artifact is out of date.
    #[arg(long)]
    pub check: bool,
}

#[derive(Debug, Args)]
pub struct AuditArgs {
    /// Repository root to audit.
    #[arg(default_value = ".")]
    pub repo_root: PathBuf,

    /// Audit a specific persisted session id instead of a repo-wide audit.
    #[arg(long, conflicts_with = "latest_session")]
    pub session_id: Option<String>,

    /// Audit the latest persisted session instead of a repo-wide audit.
    #[arg(long, default_value_t = false, conflicts_with = "session_id")]
    pub latest_session: bool,

    /// Session store root (defaults to <repo_root>/.session for session audit mode).
    #[arg(long)]
    pub session_store_root: Option<PathBuf>,

    /// Workspace slug for session store operations.
    #[arg(long)]
    pub session_workspace_slug: Option<String>,

    #[arg(long)]
    pub max_file_lines: Option<usize>,

    #[arg(long)]
    pub max_cyclomatic_complexity: Option<usize>,

    #[arg(long)]
    pub coverage_warn_below: Option<f64>,
}

#[derive(Debug, Args)]
pub struct AuditSummaryArgs {
    #[arg(long, value_enum)]
    pub by: SummaryByArg,

    #[command(flatten)]
    pub args: AuditArgs,
}

#[derive(Debug, Args)]
pub struct MoveArgs {
    /// Audit entity UUID to move (required unless --resume/--rollback is used).
    pub id: Option<String>,

    /// Repository root for the audit store.
    #[arg(long = "repo-root", default_value = ".")]
    pub repo_root: PathBuf,

    /// Destination workspace root.
    #[arg(long = "to-workspace-root")]
    pub to_workspace_root: Option<PathBuf>,

    /// Plan only; do not execute the move.
    #[arg(long, default_value_t = false)]
    pub dry_run: bool,

    /// Resume an interrupted move from a journal UUID.
    #[arg(long)]
    pub resume: Option<String>,

    /// Roll back a move from a journal UUID.
    #[arg(long)]
    pub rollback: Option<String>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum SummaryByArg {
    Crate,
    Package,
    Category,
    Severity,
    Metric,
    Path,
}

impl From<SummaryByArg> for AuditSummaryBy {
    fn from(value: SummaryByArg) -> Self {
        match value {
            SummaryByArg::Crate | SummaryByArg::Package =>
                AuditSummaryBy::Crate,
            SummaryByArg::Category => AuditSummaryBy::Category,
            SummaryByArg::Severity => AuditSummaryBy::Severity,
            SummaryByArg::Metric => AuditSummaryBy::Metric,
            SummaryByArg::Path => AuditSummaryBy::Path,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CliRunError {
    #[error("audit error: {0}")]
    Audit(#[from] AuditError),

    #[error("session error: {0}")]
    Session(#[from] session_api::SessionError),

    #[error("bad request: {0}")]
    BadRequest(String),

    #[error("store-index error: {0}")]
    StoreIndex(String),
}

#[derive(Debug)]
pub enum CliOutput {
    Machine(Value, MachineOutputFormat),
    Text(String),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MachineOutputFormat {
    Json,
    Toon,
}

pub fn parse_cli_from<I, T>(args: I) -> Result<AuditCli, clap::Error>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    AuditCli::try_parse_from(args)
}

pub fn run(cli: AuditCli) -> Result<CliOutput, CliRunError> {
    match cli.command {
        AuditCommand::Run(args) => {
            if args.session_id.is_some() || args.latest_session {
                let report = run_session_audit(&args)?;
                if let Some(format) = machine_output_format(cli.json, cli.toon)
                {
                    Ok(CliOutput::Machine(json!(report), format))
                } else {
                    Ok(CliOutput::Text(render_session_audit_human(&report)))
                }
            } else {
                let report = run_audit(&args)?;
                if let Some(format) = machine_output_format(cli.json, cli.toon)
                {
                    Ok(CliOutput::Machine(json!(report), format))
                } else {
                    Ok(CliOutput::Text(render_human(&report)))
                }
            }
        },
        AuditCommand::StoreIndex(args) => {
            let result = cmd_store_index(args)?;
            if let Some(format) = machine_output_format(cli.json, cli.toon) {
                Ok(CliOutput::Machine(result, format))
            } else {
                Ok(CliOutput::Text(render_store_index_result(&result)))
            }
        },
        AuditCommand::Summary(summary) => {
            let report = run_audit(&summary.args)?;
            let summary = summarize_report(&report, summary.by.into())?;
            if let Some(format) = machine_output_format(cli.json, cli.toon) {
                Ok(CliOutput::Machine(json!(summary), format))
            } else {
                Ok(CliOutput::Text(render_summary_human(&summary)))
            }
        },
        AuditCommand::Move(args) => {
            let result = cmd_move(args)?;
            if let Some(format) = machine_output_format(cli.json, cli.toon) {
                Ok(CliOutput::Machine(result, format))
            } else {
                Ok(CliOutput::Text(
                    serde_json::to_string_pretty(&result)
                        .unwrap_or_else(|_| format!("{result:?}")),
                ))
            }
        },
    }
}

fn cmd_move(args: MoveArgs) -> Result<Value, CliRunError> {
    if args.resume.is_some() && args.rollback.is_some() {
        return Err(CliRunError::BadRequest(
            "move accepts only one of --resume or --rollback".to_string(),
        ));
    }

    let index = RepositoryIndex::open(&args.repo_root)?;

    if let Some(journal_id) = args.resume.as_deref() {
        let journal_id = journal_id.parse::<Uuid>().map_err(|error| {
            CliRunError::BadRequest(format!(
                "invalid --resume journal UUID: {error}"
            ))
        })?;
        let outcome = index.resume_move_with_journal(journal_id)?;
        return Ok(json!({
            "command": "move",
            "status": "ok",
            "mode": "resume",
            "journal_id": outcome.journal.id,
            "phase": outcome.journal.phase,
            "recovery": recovery_hint(),
        }));
    }

    if let Some(journal_id) = args.rollback.as_deref() {
        let journal_id = journal_id.parse::<Uuid>().map_err(|error| {
            CliRunError::BadRequest(format!(
                "invalid --rollback journal UUID: {error}"
            ))
        })?;
        let outcome = index.rollback_move_with_journal(journal_id)?;
        return Ok(json!({
            "command": "move",
            "status": "ok",
            "mode": "rollback",
            "journal_id": outcome.journal.id,
            "phase": outcome.journal.phase,
            "recovery": recovery_hint(),
        }));
    }

    let id = args.id.as_deref().ok_or_else(|| {
        CliRunError::BadRequest(
            "move requires <id> unless --resume/--rollback is used".to_string(),
        )
    })?;
    let to_workspace_root =
        args.to_workspace_root.as_deref().ok_or_else(|| {
            CliRunError::BadRequest(
                "move requires --to-workspace-root in plan/execute mode"
                    .to_string(),
            )
        })?;

    let audit_id = id.parse::<Uuid>().map_err(|error| {
        CliRunError::BadRequest(format!("invalid audit UUID: {error}"))
    })?;
    let report = index.plan_move_preflight(&audit_id, to_workspace_root)?;

    if args.dry_run || !report.supported() {
        return Ok(json!({
            "command": "move",
            "status": if report.supported() { "ok" } else { "blocked" },
            "mode": "plan",
            "dry_run": true,
            "repo_root": path_display(&args.repo_root),
            "audit_id": audit_id,
            "plan": move_plan_json(&report),
            "recovery": recovery_hint(),
        }));
    }

    let outcome = index.execute_move_with_journal(&report)?;
    Ok(json!({
        "command": "move",
        "status": "ok",
        "mode": "execute",
        "repo_root": path_display(&args.repo_root),
        "audit_id": audit_id,
        "plan": move_plan_json(&report),
        "outcome": move_outcome_json(&outcome),
        "recovery": recovery_hint(),
    }))
}

fn move_plan_json(
    report: &memory_kernel::storage::move_kernel::MovePlan
) -> Value {
    json!({
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
        "path_reference_files": report.path_reference_files.iter().map(|p| path_display(p)).collect::<Vec<_>>(),
        "blockers": report.blockers,
        "captured_at": report.captured_at,
    })
}

fn move_outcome_json(
    outcome: &memory_kernel::storage::move_kernel::MoveOutcome
) -> Value {
    json!({
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

fn recovery_hint() -> Value {
    json!({
        "resume": "audit move --resume <journal-uuid>",
        "rollback": "audit move --rollback <journal-uuid>",
    })
}

fn path_display(path: &std::path::Path) -> String {
    memory_kernel::workspace::normalize_path_for_display(path)
}

fn run_audit(args: &AuditArgs) -> Result<AuditReport, CliRunError> {
    let mut config = AuditConfig::default();
    if let Some(max_file_lines) = args.max_file_lines {
        config.max_file_lines = max_file_lines;
    }
    if let Some(max_cyclomatic_complexity) = args.max_cyclomatic_complexity {
        config.max_cyclomatic_complexity = max_cyclomatic_complexity;
    }
    if let Some(coverage_warn_below) = args.coverage_warn_below {
        config.coverage_warn_below = coverage_warn_below;
    }

    Ok(audit(&args.repo_root, config)?)
}

fn run_session_audit(
    args: &AuditArgs
) -> Result<SessionAuditReport, CliRunError> {
    let repo_root = args
        .repo_root
        .canonicalize()
        .unwrap_or_else(|_| args.repo_root.clone());
    let store_root = args
        .session_store_root
        .clone()
        .unwrap_or_else(|| repo_root.join(".session"));
    let workspace_slug = args
        .session_workspace_slug
        .clone()
        .or_else(|| {
            repo_root
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "workspace".to_string());

    let selector = if args.latest_session {
        SessionAuditSelector::Latest
    } else if let Some(session_id) = args.session_id.clone() {
        SessionAuditSelector::SessionId(session_id)
    } else {
        return Err(CliRunError::BadRequest(
            "session audit mode requires --latest-session or --session-id"
                .to_string(),
        ));
    };

    let store = SessionStoreConfig::new(store_root, workspace_slug);
    Ok(store.session_audit(selector)?)
}

pub fn error_output(
    message: &str,
    format: Option<MachineOutputFormat>,
) -> String {
    let payload = json!({
        "code": "invalid_request",
        "message": message,
    });
    match format {
        Some(MachineOutputFormat::Json) =>
            serde_json::to_string_pretty(&payload).unwrap_or_else(|_| {
                format!(
                    "{{\"code\":\"invalid_request\",\"message\":{:?}}}",
                    message
                )
            }),
        Some(MachineOutputFormat::Toon) =>
            toon_format::encode_default(&payload).unwrap_or_else(|_| {
                format!("code: invalid_request\nmessage: {message}")
            }),
        None => message.to_string(),
    }
}

pub fn render_machine_output(
    payload: &Value,
    format: MachineOutputFormat,
) -> Result<String, String> {
    match format {
        MachineOutputFormat::Json =>
            serde_json::to_string_pretty(payload).map_err(|err| err.to_string()),
        MachineOutputFormat::Toon =>
            toon_format::encode_default(payload).map_err(|err| err.to_string()),
    }
}

pub fn machine_output_format(
    as_json: bool,
    as_toon: bool,
) -> Option<MachineOutputFormat> {
    if as_json {
        Some(MachineOutputFormat::Json)
    } else if as_toon {
        Some(MachineOutputFormat::Toon)
    } else {
        None
    }
}

pub fn requested_machine_output_format_from_args() -> Option<MachineOutputFormat>
{
    machine_output_format(
        std::env::args().any(|arg| arg == "--json"),
        std::env::args().any(|arg| arg == "--toon"),
    )
}

#[cfg(test)]
#[path = "cli_tests.rs"]
mod tests;

fn render_human(report: &AuditReport) -> String {
    let mut lines = Vec::new();
    lines.push("Repository Audit".to_string());
    lines.push(format!("Repo: {}", report.repo_root));
    lines.push(format!("Index: {}", report.index_database));
    lines.push(format!(
        "Sync: scanned {}, updated {}, reused {}, pruned {}",
        report.sync.scanned_files,
        report.sync.updated_files,
        report.sync.reused_files,
        report.sync.pruned_files
    ));
    lines.push(format!(
        "Files: {} source files, {} total lines",
        report.metrics.source_files, report.metrics.total_lines
    ));
    lines.push(format!(
        "File length: {} long files over {} lines (max {})",
        report.metrics.file_length.long_files,
        report.metrics.file_length.threshold,
        report.metrics.file_length.max_lines
    ));
    lines.push(format!(
        "Compiler warnings: {}",
        render_count_metric(&report.metrics.compiler_warnings)
    ));
    lines.push(format!(
        "Test success: {}",
        render_test_metric(&report.metrics.test_results)
    ));
    lines.push(format!(
        "Coverage: {}",
        render_coverage_metric(&report.metrics.coverage)
    ));
    lines.push(format!(
        "Static metrics: {} high-complexity functions over threshold {} (avg {})",
        report.metrics.static_metrics.high_complexity_functions,
        report.metrics.static_metrics.threshold,
        report
            .metrics
            .static_metrics
            .average_cyclomatic_complexity
            .map(|value| format!("{value:.1}"))
            .unwrap_or_else(|| "n/a".to_string())
    ));
    lines.push(format!(
        "Spec fulfillment: {}",
        render_spec_fulfillment_metric(&report.metrics.spec_fulfillment)
    ));
    lines.push(format!(
        "Ticket graph: {}",
        render_count_metric(&report.metrics.ticket_graph)
    ));
    lines.push(format!(
        "Rule overlap: {}",
        render_rule_overlap_metric(&report.metrics.rule_overlap)
    ));

    if report.findings.is_empty() {
        lines.push("Findings: none".to_string());
    } else {
        lines.push(format!("Findings: {}", report.findings.len()));
        for finding in &report.findings {
            let mut line =
                format!("- [{:?}] {}", finding.severity, finding.summary);
            if let Some(path) = &finding.path {
                line.push_str(&format!(" ({path})"));
            }
            lines.push(line);
            for instruction in &finding.instructions {
                lines.push(format!("  fix: {instruction}"));
            }
        }
    }

    lines.join("\n")
}

fn render_session_audit_human(report: &SessionAuditReport) -> String {
    let mut lines = Vec::new();
    lines.push("Session Audit".to_string());
    lines.push(format!("Session: {}", report.session_id));
    lines.push(format!("Schema version: {}", report.schema_version));
    lines.push(format!("Source: {}", report.source));
    lines.push(format!("Workspace: {}", report.workspace_slug));
    lines.push(format!("Captured at: {}", report.captured_at));
    lines.push(format!(
        "Turns: {} (assistant {}, empty assistant {})",
        report.metrics.turn_count,
        report.metrics.assistant_turn_count,
        report.metrics.empty_assistant_turn_count
    ));
    lines.push(format!(
        "Events: {} (tool.execution_result {}, assistant.tool_plan {}, ambiguous sync-terminal {})",
        report.metrics.event_count,
        report.metrics.tool_execution_result_count,
        report.metrics.assistant_tool_plan_count,
        report.metrics.ambiguous_sync_terminal_count
    ));

    if report.top_tools.is_empty() {
        lines.push("Top tools: none".to_string());
    } else {
        lines.push("Top tools:".to_string());
        for tool in &report.top_tools {
            lines.push(format!("- {}: {}", tool.tool_name, tool.count));
        }
    }

    if report.findings.is_empty() {
        lines.push("Findings: none".to_string());
    } else {
        lines.push(format!("Findings: {}", report.findings.len()));
        for finding in &report.findings {
            lines.push(format!(
                "- [{:?}] {} ({})",
                finding.severity, finding.summary, finding.code
            ));
        }
    }

    lines.join("\n")
}

fn render_summary_human(summary: &AuditSummaryReport) -> String {
    let mut lines = Vec::new();
    lines.push("Repository Audit Summary".to_string());
    lines.push(format!("Repo: {}", summary.repo_root));
    lines.push(format!("Grouped by: {}", summary.by.as_str()));
    lines.push(format!("Total findings: {}", summary.total_findings));
    lines.push(format!("Repo-wide issues: {}", summary.repo_wide_issues));

    if summary.groups.is_empty() {
        lines.push("Groups: none".to_string());
    } else {
        lines.push("Groups:".to_string());
        for group in &summary.groups {
            lines.push(format!("- {}: {}", group.key, group.issues));
        }
    }

    if !summary.unmapped_paths.is_empty() {
        lines.push("Unmapped paths:".to_string());
        for group in &summary.unmapped_paths {
            lines.push(format!("- {}: {}", group.key, group.issues));
        }
    }

    lines.join("\n")
}

fn render_count_metric(metric: &audit_api::models::CountMetric) -> String {
    match metric.status {
        TrialStatus::Collected | TrialStatus::Failed => metric
            .count
            .map(|count| count.to_string())
            .unwrap_or_else(|| "n/a".to_string()),
        TrialStatus::Unavailable | TrialStatus::NotApplicable => metric
            .details
            .clone()
            .unwrap_or_else(|| "unavailable".to_string()),
    }
}

fn render_test_metric(metric: &audit_api::models::TestSummary) -> String {
    match metric.status {
        TrialStatus::Collected | TrialStatus::Failed => format!(
            "{} passed, {} failed, {} ignored, success rate {}",
            metric.passed.unwrap_or_default(),
            metric.failed.unwrap_or_default(),
            metric.ignored.unwrap_or_default(),
            metric
                .success_rate
                .map(|value| format!("{value:.1}%"))
                .unwrap_or_else(|| "n/a".to_string())
        ),
        TrialStatus::Unavailable | TrialStatus::NotApplicable => metric
            .details
            .clone()
            .unwrap_or_else(|| "unavailable".to_string()),
    }
}

fn render_coverage_metric(
    metric: &audit_api::models::CoverageSummary
) -> String {
    match metric.status {
        TrialStatus::Collected => metric
            .line_percent
            .map(|value| format!("{value:.1}%"))
            .unwrap_or_else(|| "n/a".to_string()),
        TrialStatus::Unavailable
        | TrialStatus::NotApplicable
        | TrialStatus::Failed => metric
            .details
            .clone()
            .unwrap_or_else(|| "unavailable".to_string()),
    }
}

fn render_spec_fulfillment_metric(
    metric: &audit_api::models::SpecFulfillmentSummary
) -> String {
    match metric.status {
        TrialStatus::Collected => format!(
            "{} structured specs ({} satisfied, {} blocked, {} missed)",
            metric.structured_specs,
            metric.satisfied_specs,
            metric.blocked_specs,
            metric.missed_specs
        ),
        TrialStatus::Unavailable
        | TrialStatus::NotApplicable
        | TrialStatus::Failed => metric
            .details
            .clone()
            .unwrap_or_else(|| "unavailable".to_string()),
    }
}

fn render_rule_overlap_metric(
    metric: &audit_api::models::RuleOverlapSummary
) -> String {
    match metric.status {
        TrialStatus::Collected => format!(
            "{} high-overlap pairs across {} rules (max similarity {})",
            metric.high_overlap_pairs,
            metric.rules_considered,
            metric
                .max_similarity
                .map(|value| format!("{:.1}%", value * 100.0))
                .unwrap_or_else(|| "n/a".to_string())
        ),
        TrialStatus::Unavailable
        | TrialStatus::NotApplicable
        | TrialStatus::Failed => metric
            .details
            .clone()
            .unwrap_or_else(|| "unavailable".to_string()),
    }
}

// ---------------------------------------------------------------------------
// store-index command
// ---------------------------------------------------------------------------

const STORE_DIR: &str = ".audit";

/// Generate or check the committed audit catalog artifacts.
fn cmd_store_index(args: StoreIndexArgs) -> Result<Value, CliRunError> {
    let repo_root = args.repo_root.canonicalize().unwrap_or(args.repo_root);
    let report = run_audit_from_root(&repo_root)?;

    let source = AuditCatalogSource {
        report: Some(&report),
        store_dir: STORE_DIR,
    };
    let artifacts = generate_audit_catalog(&source);

    let readme_path = repo_root.join(STORE_DIR).join("README.md");
    let sidecar_path = repo_root.join(STORE_DIR).join("index.toon");
    let agent_hook_path = repo_root.join(AUDIT_INDEX_AGENT_HOOK_PATH);

    let sidecar_toon = artifacts
        .sidecar
        .encode_toon()
        .map_err(|e| CliRunError::StoreIndex(e.to_string()))?;

    let readme_out = prepare_generated_output(
        &artifacts.readme_markdown,
        read_existing(&readme_path).as_deref(),
    );
    let agent_hook_out = prepare_generated_output(
        &artifacts.agent_hook_markdown,
        read_existing(&agent_hook_path).as_deref(),
    );
    let sidecar_out = prepare_generated_output(
        &sidecar_toon,
        read_existing(&sidecar_path).as_deref(),
    );

    let planned = [
        (&readme_path, &readme_out),
        (&sidecar_path, &sidecar_out),
        (&agent_hook_path, &agent_hook_out),
    ];

    let total_findings = report.findings.len();
    let category_count = {
        let mut cats: std::collections::BTreeSet<&str> = Default::default();
        for f in &report.findings {
            cats.insert(f.category.as_str());
        }
        cats.len()
    };

    if args.check {
        let drifted: Vec<String> = planned
            .iter()
            .filter(|(path, content)| {
                read_existing(path).as_deref() != Some(content.as_str())
            })
            .map(|(path, _)| display_path(path))
            .collect();

        if !drifted.is_empty() {
            return Err(CliRunError::StoreIndex(format!(
                "audit store-index is out of date; regenerate and re-stage: {}",
                drifted.join(", ")
            )));
        }

        return Ok(json!({
            "command": "store-index",
            "status": "ok",
            "check": true,
            "drift": false,
            "findings": total_findings,
            "categories": category_count,
        }));
    }

    let mut written = Vec::new();
    for (path, content) in &planned {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| CliRunError::StoreIndex(e.to_string()))?;
        }
        fs::write(path, content)
            .map_err(|e| CliRunError::StoreIndex(e.to_string()))?;
        written.push(display_path(path));
    }

    Ok(json!({
        "command": "store-index",
        "status": "ok",
        "check": false,
        "findings": total_findings,
        "categories": category_count,
        "written": written,
    }))
}

fn run_audit_from_root(repo_root: &Path) -> Result<AuditReport, CliRunError> {
    let args = AuditArgs {
        repo_root: repo_root.to_path_buf(),
        session_id: None,
        latest_session: false,
        session_store_root: None,
        session_workspace_slug: None,
        max_file_lines: None,
        max_cyclomatic_complexity: None,
        coverage_warn_below: None,
    };
    run_audit(&args)
}

fn read_existing(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok()
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn render_store_index_result(result: &Value) -> String {
    let check = result["check"].as_bool().unwrap_or(false);
    let findings = result["findings"].as_u64().unwrap_or(0);
    let categories = result["categories"].as_u64().unwrap_or(0);
    if check {
        format!(
            "audit store-index: ok (up to date) — {findings} findings in \
             {categories} categories"
        )
    } else {
        let written = result["written"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        format!(
            "audit store-index: written — {findings} findings in {categories} \
             categories\nFiles: {written}"
        )
    }
}
