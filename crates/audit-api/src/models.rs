use serde::{
    Deserialize,
    Serialize,
};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditConfig {
    pub max_file_lines: usize,
    pub max_cyclomatic_complexity: usize,
    pub coverage_warn_below: f64,
}

impl Default for AuditConfig {
    fn default() -> Self {
        Self {
            max_file_lines: 400,
            max_cyclomatic_complexity: 12,
            coverage_warn_below: 80.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexedFile {
    pub path: String,
    pub language: String,
    pub size_bytes: u64,
    pub modified_unix_ms: i64,
    pub sha256: String,
    pub line_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncStats {
    pub scan_token: String,
    pub scanned_files: usize,
    pub updated_files: usize,
    pub reused_files: usize,
    pub pruned_files: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrialStatus {
    Collected,
    Unavailable,
    Failed,
    NotApplicable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileLengthMetric {
    pub threshold: usize,
    pub long_files: usize,
    pub average_lines: f64,
    pub max_lines: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CountMetric {
    pub status: TrialStatus,
    pub count: Option<usize>,
    pub details: Option<String>,
}

impl CountMetric {
    pub fn unavailable(details: impl Into<String>) -> Self {
        Self {
            status: TrialStatus::Unavailable,
            count: None,
            details: Some(details.into()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestSummary {
    pub status: TrialStatus,
    pub total: Option<usize>,
    pub passed: Option<usize>,
    pub failed: Option<usize>,
    pub ignored: Option<usize>,
    pub success_rate: Option<f64>,
    pub details: Option<String>,
}

impl TestSummary {
    pub fn unavailable(details: impl Into<String>) -> Self {
        Self {
            status: TrialStatus::Unavailable,
            total: None,
            passed: None,
            failed: None,
            ignored: None,
            success_rate: None,
            details: Some(details.into()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageSummary {
    pub status: TrialStatus,
    pub line_percent: Option<f64>,
    pub covered_lines: Option<usize>,
    pub total_lines: Option<usize>,
    pub details: Option<String>,
}

impl CoverageSummary {
    pub fn unavailable(details: impl Into<String>) -> Self {
        Self {
            status: TrialStatus::Unavailable,
            line_percent: None,
            covered_lines: None,
            total_lines: None,
            details: Some(details.into()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StaticMetricsSummary {
    pub status: TrialStatus,
    pub threshold: usize,
    pub functions_analyzed: usize,
    pub parse_failures: usize,
    pub high_complexity_functions: usize,
    pub average_cyclomatic_complexity: Option<f64>,
    pub max_cyclomatic_complexity: Option<usize>,
    pub details: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecFulfillmentSummary {
    pub status: TrialStatus,
    pub structured_specs: usize,
    pub satisfied_specs: usize,
    pub blocked_specs: usize,
    pub missed_specs: usize,
    pub details: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleOverlapSummary {
    pub status: TrialStatus,
    pub rules_considered: usize,
    pub compared_pairs: usize,
    pub high_overlap_pairs: usize,
    pub max_similarity: Option<f64>,
    pub details: Option<String>,
}

impl RuleOverlapSummary {
    pub fn unavailable(details: impl Into<String>) -> Self {
        Self {
            status: TrialStatus::Unavailable,
            rules_considered: 0,
            compared_pairs: 0,
            high_overlap_pairs: 0,
            max_similarity: None,
            details: Some(details.into()),
        }
    }

    pub fn not_applicable(details: impl Into<String>) -> Self {
        Self {
            status: TrialStatus::NotApplicable,
            rules_considered: 0,
            compared_pairs: 0,
            high_overlap_pairs: 0,
            max_similarity: None,
            details: Some(details.into()),
        }
    }
}

impl SpecFulfillmentSummary {
    pub fn unavailable(details: impl Into<String>) -> Self {
        Self {
            status: TrialStatus::Unavailable,
            structured_specs: 0,
            satisfied_specs: 0,
            blocked_specs: 0,
            missed_specs: 0,
            details: Some(details.into()),
        }
    }

    pub fn not_applicable(details: impl Into<String>) -> Self {
        Self {
            status: TrialStatus::NotApplicable,
            structured_specs: 0,
            satisfied_specs: 0,
            blocked_specs: 0,
            missed_specs: 0,
            details: Some(details.into()),
        }
    }

    pub fn failed(details: impl Into<String>) -> Self {
        Self {
            status: TrialStatus::Failed,
            structured_specs: 0,
            satisfied_specs: 0,
            blocked_specs: 0,
            missed_specs: 0,
            details: Some(details.into()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditMetrics {
    pub source_files: usize,
    pub total_lines: usize,
    pub file_length: FileLengthMetric,
    pub compiler_warnings: CountMetric,
    pub test_results: TestSummary,
    pub coverage: CoverageSummary,
    pub static_metrics: StaticMetricsSummary,
    pub spec_fulfillment: SpecFulfillmentSummary,
    pub ticket_graph: CountMetric,
    pub session_workflow_graph: CountMetric,
    pub rule_overlap: RuleOverlapSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditFinding {
    pub id: String,
    pub category: String,
    pub severity: Severity,
    pub summary: String,
    pub path: Option<String>,
    pub line: Option<usize>,
    pub metric_name: String,
    pub metric_value: Value,
    pub threshold: Option<Value>,
    pub instructions: Vec<String>,
    pub evidence: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditRunInfo {
    pub run_id: i64,
    pub started_at: String,
    pub finished_at: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditReport {
    pub service: String,
    pub repo_root: String,
    pub index_database: String,
    pub sync: SyncStats,
    pub run: AuditRunInfo,
    pub metrics: AuditMetrics,
    pub findings: Vec<AuditFinding>,
    pub instructions: Vec<String>,
}
