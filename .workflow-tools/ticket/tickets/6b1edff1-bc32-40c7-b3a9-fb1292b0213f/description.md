# Session bootstrap correctness remediation

Repairs correctness holes found during independent review of the durable
session-bootstrap implementation (actions 1–7) before action 8 or epic closure.

## Findings to repair (in order)

1. **Authoritative validation** — `resolve_validation_gates` lets a caller-provided
   gate outcome win when present in `by_id`. Required guard outcomes must always come
   from test-api; caller payloads may identify/display gates but never certify them.
   Fail closed for unknown specs, absent, failed, or blocked executions.
2. **Live ticket authority** — `node_is_effectively_done` returns true for any locally
   `Done` node before checking whether it is ticket-backed. Ticket-backed nodes must
   derive completion exclusively from authoritative live terminal state; local status is
   display/cache only.
3. **Missing/misrouted ticket state fails open** — default resolver returns `Ok(None)`
   for absent tickets and the URN workspace segment is ignored. Treat missing required
   ticket state as unavailable (block finish); route cross-workspace URNs or reject
   unsupported routing explicitly.
4. **Windows atomic replacement** — the Windows branch deletes the destination before
   renaming the temp file, losing durable state on crash. Preserve the old file until
   replacement succeeds; enforce concurrent-mutation safety.
5. **Stale finish** — `finish_workflow` returns an existing `finish.json` before
   reevaluating authorities, and mutation APIs remain available afterward. Choose and
   enforce one invariant (immutable-after-finish OR mutation invalidates finish).
6. **Evidence traceability** — spec-cited `exec-val-session-*` IDs have no persisted
   execution records. Record required validations through test-api; reconcile cited IDs.
7. **CLI contract** — expose nested `session workflow ...` hierarchy with flat forms as
   compatibility aliases; add parsing/integration tests.

## Acceptance criteria

- Required validation outcomes come only from authoritative test-api executions; caller
  `passed` cannot produce a successful finish (regression test).
- Ticket-backed nodes require live terminal ticket state; locally `Done` + non-terminal
  live state rejects finish (regression test).
- Missing/failed/misrouted ticket resolution blocks finish (production-path tests).
- Post-finish mutation behavior is explicit, implemented, and tested.
- Durable writes preserve the previous file if replacement fails; concurrent updates
  cannot silently overwrite each other.
- Canonical nested CLI commands and compatibility aliases are tested.
- Every spec-cited execution ID resolves to persisted evidence; unrelated execution
  deletions are explained or excluded.
- Focused and full session-api, CLI, and MCP validations pass.

## Audit follow-up (2026-07-14)

A second-pass `audit-cli` review surfaced deeper correctness issues on the same
finish/locking/durability scope. Repaired:

- **Finish-vs-mutation race** — the finished-check ran before the mutation lock, so a
  mutation that observed "not finished" could commit after `finish_workflow` wrote its
  record. All runtime mutations now go through `begin_runtime_mutation`, which acquires
  the lock first and evaluates the finished-check under the lock. Regression:
  `finished_check_runs_under_mutation_lock`.
- **Unlocked init/resume lineage** — `init_runtime_context`/`resume_workspace_context`
  appended run lineage without the mutation lock and without an immutability guard. They
  now acquire the lock and reject run creation on a finished workspace. Regression:
  `finished_workspace_rejects_resume_run_creation`.
- **Windows crash interval** — the backup-swap dance left the destination absent between
  two renames. Replaced with a single `fs::rename` (atomic `MoveFileExW` replace-existing
  on Windows, `rename(2)` on Unix), removing the crash window.
- **`finish_workflow` complexity** — extracted `existing_finish_result`,
  `evaluate_workflow_completion`, and `evaluate_required_validation` so the locked finish
  path is a linear sequence and the completion invariant is independently testable.
- **Three unresolved runtime-spec executions** — `exec-val-session-init-idempotency-20260714`,
  `exec-val-session-run-lineage-20260714`, and `exec-val-session-context-capture-isolation-20260714`
  (cited by spec 709f067a) were removed by an earlier `prune_execution_runs` side-effect
  and are re-recorded from passing runs; all now resolve.
- **Formatting** — `cargo fmt` applied to session-api/session-cli/session-mcp; check is clean.

Validation: `cargo test -p session-api -p session-cli -p session-mcp` — 121 passed, 0 failed.
Maintainability findings (file-size splits for store.rs/store_tests.rs/lib.rs/server.rs,
CLI dispatch complexity, coverage-evidence gap) are noted as follow-up quality debt and do
not block action 8.

## Independent review correction (2026-07-14)

The remediation remains in review. The earlier audit follow-up overstates completion in
four places:

1. **Live-lock reclamation is unsafe.** `.context.lock` is considered stale solely by
   age after 30 seconds. A legitimate finish can exceed that threshold while resolving
   ticket/test authorities, allowing another process to delete the live lock, enter the
   critical section, and later have its replacement lock removed by the original guard.
   Replace age-only reclamation with an ownership- and liveness-safe cross-platform
   locking protocol; a guard must remove only the lock instance it owns.
2. **Finished workspace init is not immutable.** Plain idempotent init does not call the
   finished-workspace guard unless forcing or linking a run. It still updates timestamps
   and rewrites `context.json` and `active_workspace_session.json`. Ordinary init after
   finish must be read-only or rejected.
3. **Race coverage is not yet discriminating.** `finished_check_runs_under_mutation_lock`
   proves check ordering/error precedence but does not create a real finish/mutation
   interleaving or hold a live lock beyond the stale threshold. Add deterministic
   concurrency tests, including ownership-safe release.
4. **Durability claims exceed evidence.** A single `fs::rename` removes the previous
   explicit delete-before-rename interval, but the Windows replace-existing and
   power-loss guarantee remains unproven, and parent-directory sync failures are ignored.
   Either use a proven primitive and platform-specific failure tests or narrow the spec
   and ticket claims to the behavior actually established.

Action 8 and epic closure remain blocked until these items are implemented, focused tests
pass, and spec/evidence claims are reconciled.

## Traceability

- Epic: effba966-f0a8-4d7d-b289-b7feba826cf8
- Blocks action 8: b4a8dc5e-9d80-4fea-bb42-0c30aba0ecd6
- Spec: 8c880efc-7083-4e1d-bf06-96b8254be913 (parent contract)
- Spec: c677182e-90da-4ac3-8b94-9e2e97c825cf (workflow/finish contract)
- Spec: 709f067a-21b6-41b6-8879-3cacef4bacaf (runtime-context contract)
- Validation: val-session-workflow-finish
