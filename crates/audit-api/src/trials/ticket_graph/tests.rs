use std::{
    collections::BTreeMap,
    fs,
    path::{
        Path,
        PathBuf,
    },
    time::{
        SystemTime,
        UNIX_EPOCH,
    },
};

use chrono::Utc;
use ticket_api::{
    model::edge::EdgeRecord,
    storage::{
        TicketStore,
        indexed::IndexedTicket,
    },
};
use uuid::Uuid;

use super::{
    collect_policy_excluded_reference_findings,
    evaluate,
    load_workspace_policy_matchers,
};
use crate::models::{
    Severity,
    TrialStatus,
};

struct TestDir {
    path: PathBuf,
}

impl TestDir {
    fn new(prefix: &str) -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time went backwards")
            .as_nanos();
        let path = std::env::temp_dir()
            .join(format!("{prefix}-{}-{unique}", std::process::id()));
        fs::create_dir_all(&path).expect("create temp dir");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[test]
fn reports_only_orphan_tickets() {
    let repo = TestDir::new("audit-ticket-graph");
    let store = TicketStore::init(repo.path()).expect("init ticket store");
    let linked_parent = store
        .create(
            None,
            "tracker-improvement",
            Some("linked parent"),
            Some("open"),
            BTreeMap::new(),
            None,
            None,
        )
        .expect("create linked parent");
    let linked_child = store
        .create(
            None,
            "tracker-improvement",
            Some("linked child"),
            Some("open"),
            BTreeMap::new(),
            None,
            None,
        )
        .expect("create linked child");
    let orphan = store
        .create(
            None,
            "tracker-improvement",
            Some("orphan task"),
            Some("in-implementation"),
            BTreeMap::new(),
            None,
            None,
        )
        .expect("create orphan ticket");

    store
        .add_edge(EdgeRecord {
            from: linked_parent,
            to: linked_child,
            kind: "depends_on".to_string(),
            created_at: Utc::now(),
        })
        .expect("add edge");

    let result = evaluate(repo.path());

    assert!(matches!(result.metric.status, TrialStatus::Collected));
    assert_eq!(result.metric.count, Some(1));
    assert_eq!(result.findings.len(), 1);
    assert_eq!(result.findings[0].category, "ticket_graph");
    assert!(matches!(result.findings[0].severity, Severity::High));
    assert_eq!(
        result.findings[0].evidence["ticket_id"],
        serde_json::json!(orphan)
    );
}

#[test]
fn reports_unavailable_without_ticket_store() {
    let repo = TestDir::new("audit-ticket-graph-missing-store");

    let result = evaluate(repo.path());

    assert!(matches!(result.metric.status, TrialStatus::Unavailable));
    assert_eq!(result.metric.count, None);
    assert!(result.findings.is_empty());
}

#[test]
fn reports_dependency_convergence_findings() {
    let repo = TestDir::new("audit-ticket-graph-convergence");
    let store = TicketStore::init(repo.path()).expect("init ticket store");
    let prerequisite = store
        .create(
            None,
            "tracker-improvement",
            Some("lagging prerequisite"),
            Some("planned"),
            BTreeMap::new(),
            None,
            None,
        )
        .expect("create prerequisite");
    let dependent = store
        .create(
            None,
            "tracker-improvement",
            Some("advanced dependent"),
            Some("in-review"),
            BTreeMap::new(),
            None,
            None,
        )
        .expect("create dependent");

    store
        .add_edge(EdgeRecord {
            from: dependent,
            to: prerequisite,
            kind: "depends_on".to_string(),
            created_at: Utc::now(),
        })
        .expect("add edge");

    let result = evaluate(repo.path());

    assert!(matches!(result.metric.status, TrialStatus::Collected));
    assert_eq!(result.metric.count, Some(0));
    let convergence = result
        .findings
        .iter()
        .find(|finding| finding.metric_name == "dependency_convergence_count")
        .expect("convergence finding");
    assert!(matches!(convergence.severity, Severity::High));
    assert_eq!(
        convergence.evidence["dependent_id"],
        serde_json::json!(dependent)
    );
    assert_eq!(
        convergence.evidence["prerequisite_id"],
        serde_json::json!(prerequisite)
    );
    assert_eq!(
        convergence.evidence["dependency_state_gap"],
        serde_json::json!(2)
    );
}

#[test]
fn reports_policy_excluded_workspace_references() {
    let repo = TestDir::new("audit-ticket-graph-policy-excluded");
    let excluded_workspace = repo.path().join("excluded-workspace");
    fs::create_dir_all(&excluded_workspace)
        .expect("create excluded workspace root");
    fs::write(excluded_workspace.join(".ticket-ignore"), "")
        .expect("write marker");

    let source = Uuid::new_v4();
    let fixture_target = Uuid::new_v4();
    let fixture_internal = Uuid::new_v4();
    let now = Utc::now();
    let source_path = repo
        .path()
        .join(".ticket")
        .join("tickets")
        .join(source.to_string());
    let target_path = excluded_workspace
        .join(".ticket")
        .join("tickets")
        .join(fixture_target.to_string());
    let internal_path = excluded_workspace
        .join(".ticket")
        .join("tickets")
        .join(fixture_internal.to_string());
    let tickets = vec![
        IndexedTicket {
            id: source,
            path: source_path,
            type_id: "tracker-improvement".to_string(),
            title: Some("source ticket".to_string()),
            state: Some("in-review".to_string()),
            created_at: now,
            updated_at: now,
        },
        IndexedTicket {
            id: fixture_target,
            path: target_path,
            type_id: "tracker-improvement".to_string(),
            title: Some("fixture target".to_string()),
            state: Some("open".to_string()),
            created_at: now,
            updated_at: now,
        },
        IndexedTicket {
            id: fixture_internal,
            path: internal_path,
            type_id: "tracker-improvement".to_string(),
            title: Some("fixture internal".to_string()),
            state: Some("open".to_string()),
            created_at: now,
            updated_at: now,
        },
    ];
    let edges = vec![
        EdgeRecord {
            from: source,
            to: fixture_target,
            kind: "depends_on".to_string(),
            created_at: now,
        },
        EdgeRecord {
            from: fixture_target,
            to: fixture_internal,
            kind: "depends_on".to_string(),
            created_at: now,
        },
    ];

    let policy_matchers = load_workspace_policy_matchers(repo.path());
    let policy_findings = collect_policy_excluded_reference_findings(
        repo.path(),
        &tickets,
        &edges,
        &policy_matchers,
        None,
    );
    assert_eq!(policy_findings.len(), 1);
    let finding = &policy_findings[0];
    assert!(matches!(finding.severity, Severity::High));
    assert_eq!(
        finding.evidence["source_ticket_id"],
        serde_json::json!(source)
    );
    assert_eq!(
        finding.evidence["target_ticket_id"],
        serde_json::json!(fixture_target)
    );
    assert_eq!(
        finding.evidence["policy_exclusion_reason"],
        serde_json::json!("policy:ignore_marker:.ticket-ignore")
    );
}
