//! Temporary-root parity test for the audit transfer contract
//! (see `transcripts/03-09-2026_repository-entity-distribution/04-audit-transfer-contract.md`).
//!
//! Migrates a fixture entity from `audit/test-fixtures/transfer-fixture` into
//! a canonical `.workflow-tools/audit` container, rolls it back, and confirms
//! discovery finds the same physical store (same `entity_id`,
//! `canonical_path`, `digest`) whether scanned from a synthesized
//! `meta-workspace` root or directly from `workflow-tools/audit`.

use std::{
    fs,
    path::{
        Path,
        PathBuf,
    },
};

use audit_api::index::RepositoryIndex;
use memory_kernel::ContentKind;
use sha2::{
    Digest,
    Sha256,
};
use uuid::Uuid;

const FIXTURE_ENTITY_ID: &str = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";

fn run_git(
    repo_root: &Path,
    args: &[&str],
) {
    let status = std::process::Command::new("git")
        .current_dir(repo_root)
        .args(args)
        .status()
        .expect("git command");
    assert!(status.success(), "git {args:?} failed: {status}");
}

fn copy_dir_recursive(
    from: &Path,
    to: &Path,
) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let dest = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir_recursive(&entry.path(), &dest);
        } else {
            fs::copy(entry.path(), &dest).unwrap();
        }
    }
}

fn digest_file(path: &Path) -> String {
    let bytes = fs::read(path).unwrap();
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    format!("{:x}", hasher.finalize())
}

/// Read every entity-folder finding directly under `.audit`-domain
/// `store_root` (whichever legacy or canonical directory that is), returning
/// `(entity_id, canonical_path, digest)` tuples sorted by id.
fn discovery_tuples(store_root: &Path) -> Vec<(Uuid, PathBuf, String)> {
    let findings_root = store_root.join("findings");
    let mut tuples = Vec::new();
    if findings_root.is_dir() {
        for entry in fs::read_dir(&findings_root).unwrap() {
            let entry = entry.unwrap();
            if !entry.file_type().unwrap().is_dir() {
                continue;
            }
            let Some(name) = entry.file_name().to_str().map(str::to_string) else {
                continue;
            };
            let Ok(id) = name.parse::<Uuid>() else {
                continue;
            };
            let finding_file = entry.path().join("finding.json");
            let canonical_path = PathBuf::from("findings").join(&name).join("finding.json");
            tuples.push((id, canonical_path, digest_file(&finding_file)));
        }
    }
    tuples.sort_by_key(|(id, _, _)| *id);
    tuples
}

#[test]
fn audit_transfer_contract_temporary_root_parity() {
    let temp = tempfile::tempdir().unwrap();
    let meta_workspace = temp.path().join("meta-workspace");
    let workflow_tools_audit = meta_workspace.join("workflow-tools").join("audit");
    fs::create_dir_all(&workflow_tools_audit).unwrap();
    run_git(&meta_workspace, &["init"]);

    // Seed the source fixture into the temporary root.
    let fixture_source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("audit crate root")
        .join("test-fixtures")
        .join("transfer-fixture");
    let fixture_target = workflow_tools_audit.join("test-fixtures").join("transfer-fixture");
    copy_dir_recursive(&fixture_source, &fixture_target);

    let source_index = RepositoryIndex::open_or_init(&fixture_target).unwrap();
    let entity_id: Uuid = FIXTURE_ENTITY_ID.parse().unwrap();
    let source_finding_file = source_index
        .finding_entity_path(&entity_id)
        .expect("fixture finding entity present")
        .join("finding.json");
    let source_digest = digest_file(&source_finding_file);

    // Pre-create the canonical container so destination resolution prefers
    // it over the legacy `.audit` layout (mirrors the contract's
    // `workflow-tools/audit/.workflow-tools/audit` destination).
    fs::create_dir_all(workflow_tools_audit.join(".workflow-tools").join("audit")).unwrap();
    RepositoryIndex::init(&workflow_tools_audit).unwrap();

    let plan = source_index
        .plan_move_preflight(&entity_id, &workflow_tools_audit)
        .unwrap();
    assert!(plan.supported(), "expected a supported move plan: {:?}", plan.blockers);
    assert!(
        plan.destination_entity_path.ends_with(
            Path::new(".workflow-tools").join("audit").join("findings").join(entity_id.to_string())
        ),
        "destination must resolve under the canonical .workflow-tools/audit container: {:?}",
        plan.destination_entity_path
    );

    let outcome = source_index.execute_move_with_journal(&plan).unwrap();
    assert!(!source_finding_file.exists(), "source finding folder must be moved away");
    let destination_finding_file = plan.destination_entity_path.join("finding.json");
    assert!(destination_finding_file.is_file());
    assert_eq!(
        digest_file(&destination_finding_file),
        source_digest,
        "digest must survive the move"
    );

    // Discovery parity: scanning from the synthesized "meta-workspace" root
    // and scanning directly from "workflow-tools/audit" must agree on the
    // same physical store.
    let from_root = memory_kernel::discover_stores(&meta_workspace);
    let from_submodule = memory_kernel::discover_stores(&workflow_tools_audit);
    let canonical_workspace = fs::canonicalize(&workflow_tools_audit).unwrap();

    let audit_store_from_root = from_root
        .iter()
        .find(|store| {
            store.kind == ContentKind::AuditFinding
                && fs::canonicalize(&store.workspace_root).ok().as_ref()
                    == Some(&canonical_workspace)
        })
        .expect("root discovery finds the canonical audit store");
    let audit_store_from_submodule = from_submodule
        .iter()
        .find(|store| {
            store.kind == ContentKind::AuditFinding
                && fs::canonicalize(&store.workspace_root).ok().as_ref()
                    == Some(&canonical_workspace)
        })
        .expect("submodule discovery finds the canonical audit store");

    let canonical_root_a = fs::canonicalize(&audit_store_from_root.store_root).unwrap();
    let canonical_root_b = fs::canonicalize(&audit_store_from_submodule.store_root).unwrap();
    assert_eq!(
        canonical_root_a, canonical_root_b,
        "root and submodule discovery must resolve to the same physical store_root"
    );

    let tuples_from_root = discovery_tuples(&canonical_root_a);
    let tuples_from_submodule = discovery_tuples(&canonical_root_b);
    assert_eq!(
        tuples_from_root, tuples_from_submodule,
        "(entity_id, canonical_path, digest) tuples must match between discovery vantage points"
    );
    assert_eq!(
        tuples_from_root,
        vec![(entity_id, PathBuf::from("findings").join(entity_id.to_string()).join("finding.json"), source_digest.clone())]
    );

    // Roll back and compare checksums before and after.
    let journal_id = outcome.journal.id;
    let rollback_outcome = source_index.rollback_move_with_journal(journal_id).unwrap();
    assert!(rollback_outcome.rolled_back);
    assert!(source_finding_file.is_file(), "rollback must restore the source finding folder");
    assert_eq!(
        digest_file(&source_finding_file),
        source_digest,
        "rollback must restore the exact digest"
    );
    assert!(
        !plan.destination_entity_path.exists(),
        "destination entity removed after rollback"
    );
}
