//! Move-health Criterion benchmark for
//! [`audit_api::move_domain::AuditMoveDomain`].
//!
//! Audit has no folder-per-entity store today: `AuditMoveDomain` is
//! intentionally fail-closed (`source_entity_path` always returns `None`,
//! `related_entities` always returns an empty set, `entity_indexed_in`
//! always returns `false`), so [`AuditMoveDomain::source_entity_path`]
//! guarantees every preflight plan carries a
//! [`MoveBlocker::MissingSourceEntity`] blocker for any synthetic id, moved
//! or not. See `IMPLEMENTATION-REVIEW.md`'s review disposition: "audit is
//! correctly fail-closed and has no meaningful apply/rollback performance
//! path until audit entities exist."
//!
//! In scope: preflight/fail-closed-path timing for a synthetic entity id
//! against an empty source workspace (no persisted audit entity folders),
//! at varying total-store background counts (varying only which git
//! worktree/board/lease context exists, since audit itself has no
//! move-relevant background rows).
//!
//! Explicitly out of scope (fabricating any of these would time a codepath
//! that never runs against a real audit entity, since none can currently be
//! moved):
//! - apply / rollback / resume: `execute_move_with_journal` requires a
//!   supported plan (`plan.supported()`), and a fail-closed
//!   `MissingSourceEntity` blocker means `plan.supported()` is always
//!   `false` for audit. There is no way to reach apply, rollback, or resume
//!   without first fabricating a persisted audit entity folder that the
//!   production adapter does not create.
//! - link density: `AuditMoveDomain::related_entities` always returns
//!   `MoveReferences::default()` (no inbound/outbound edges), so there is no
//!   link-density axis to vary.
//! - store-size mode (touched-set vs. total-store): `AuditMoveDomain` has no
//!   entity-backed store to grow; `scan_store` only re-opens the repository
//!   index, so there is no touched-set-vs-total-store distinction to
//!   measure.
//!
//! No fabricated percentiles: only Criterion's own sample statistics are
//! reported.

use std::{
    fs,
    path::{
        Path,
        PathBuf,
    },
    process::Command,
};

use audit_api::index::RepositoryIndex;
use criterion::{
    Criterion,
    criterion_group,
    criterion_main,
};
use memory_kernel::storage::move_kernel::MoveBlocker;
use tempfile::TempDir;
use uuid::Uuid;

const AUDIT_INDEX_DIR: &str = ".audit";

fn git_init(repo_root: &Path) {
    let status = Command::new("git")
        .current_dir(repo_root)
        .arg("init")
        .status()
        .expect("run git init");
    assert!(status.success(), "git init failed");
}

/// One isolated source+target workspace pair with an initialized (but
/// necessarily entity-empty) audit index, and `background_count` unrelated
/// files written into the source workspace to vary total workspace size
/// independent of the (always absent) audit entity set.
fn build_audit_fixture(
    background_count: usize,
) -> (TempDir, RepositoryIndex, PathBuf) {
    let workspace_dir = tempfile::tempdir().expect("tempdir");
    let repo = workspace_dir.path().join("repo");
    fs::create_dir_all(&repo).expect("create repo dir");
    git_init(&repo);

    let source_workspace = repo.join("source");
    let target_workspace = repo.join("target");
    fs::create_dir_all(&source_workspace).expect("create source workspace");
    fs::create_dir_all(target_workspace.join(AUDIT_INDEX_DIR))
        .expect("create target audit index dir");

    let index =
        RepositoryIndex::init(&source_workspace).expect("init audit index");

    for offset in 0..background_count {
        fs::write(
            source_workspace.join(format!("bench-background-{offset}.rs")),
            format!("// background file {offset}\n"),
        )
        .expect("write background file");
    }

    (workspace_dir, index, target_workspace)
}

/// Assert that a preflight plan for a synthetic (never-persisted) audit
/// entity id is fail-closed, per the domain's documented contract.
fn assert_fail_closed(
    entity_id: &Uuid,
    plan: &memory_kernel::storage::move_kernel::MovePlan,
) {
    assert!(
        !plan.supported(),
        "audit preflight unexpectedly supported for synthetic id {entity_id}: {:?}",
        plan.blockers
    );
    assert!(
        plan.blockers.iter().any(|blocker| matches!(
            blocker,
            MoveBlocker::MissingSourceEntity { entity_id: blocked_id }
                if blocked_id == entity_id
        )),
        "expected MissingSourceEntity blocker for synthetic id {entity_id}: {:?}",
        plan.blockers
    );
}

// --- Fail-closed preflight, fixed workspace size ---

fn bench_audit_move_preflight_fail_closed(c: &mut Criterion) {
    let (_workspace_dir, index, target_workspace) = build_audit_fixture(0);
    let entity_id = Uuid::new_v4();
    c.bench_function("audit_move_preflight_fail_closed", |b| {
        b.iter(|| {
            let plan = index
                .plan_move_preflight(&entity_id, &target_workspace)
                .expect("plan preflight");
            assert_fail_closed(&entity_id, &plan);
            criterion::black_box(plan);
        });
    });
}

// --- Fail-closed preflight across varying background workspace size ---
//
// Audit has no entity-backed store to grow, so this only varies unrelated
// background files in the source workspace; it does not exercise a
// touched-set-vs-total-store distinction (see module doc).

fn bench_audit_move_preflight_by_background_size(c: &mut Criterion) {
    for &background_count in &[0usize, 100, 500] {
        let (_workspace_dir, index, target_workspace) =
            build_audit_fixture(background_count);
        let entity_id = Uuid::new_v4();
        c.bench_function(
            &format!(
                "audit_move_preflight_fail_closed_background_{background_count}files"
            ),
            |b| {
                b.iter(|| {
                    let plan = index
                        .plan_move_preflight(&entity_id, &target_workspace)
                        .expect("plan preflight");
                    assert_fail_closed(&entity_id, &plan);
                    criterion::black_box(plan);
                });
            },
        );
    }
}

criterion_group!(
    move_health,
    bench_audit_move_preflight_fail_closed,
    bench_audit_move_preflight_by_background_size,
);
criterion_main!(move_health);
