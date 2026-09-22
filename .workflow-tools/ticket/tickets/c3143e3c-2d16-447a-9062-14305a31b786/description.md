# [Board][Design] Stale-Entry Review, Cleanup Approval, and Conflict Resolution Workflow

## Objective

Define the human-in-the-loop workflow for stale entries, explicit cleanup, and file ownership conflicts.

The current draftboard design says cleanup should be explicit and agents should seek user permission before removing stale entries, but it does not yet specify how that permission is represented, how stale/conflict cases are reviewed, or which resolution actions exist.

## Why This Is Missing but Critical

This is where the system will fail first in practice. Stale entries are inevitable:
- agents crash
- sessions disconnect
- users pause work without checking out
- heartbeats stop during long-running or blocked work

If stale cleanup and conflict resolution are not carefully designed, agents will either:
- ignore stale entries and deadlock the board, or
- over-delete entries and break coordination history

## Questions to Resolve

### 1. Cleanup authorization
- How does an agent present stale/completed entries for user review before cleanup?
- Do we need a two-step flow such as preview → confirm?
- Should `board clean` support a dry-run mode or confirmation token?
- How is approval represented in MCP, where interactive prompts do not exist?

### 2. Stale-entry handling
- What exactly happens when an entry crosses the one-hour stale threshold?
- Is there a grace period between "warning" and "cleanup-eligible"?
- What output should `board show`, `next`, and `status` display for stale entries?
- When should the operator be urged to renew instead of remove?

### 3. Conflict resolution actions
- What explicit actions can the operator take when two entries conflict on files?
- Candidate actions:
  - renew existing entry
  - release specific files from an entry
  - transfer ownership to another agent
  - force override with audit log
  - mark one side abandoned and clean it
- Which of these are supported in v1?

### 4. Auditability
- What audit record is retained when stale entries are renewed, cleaned, overridden, or transferred?
- Do resolution actions belong in board state only, lease history, ticket history, or all three?
- How should human approval be traceable after the fact?

## Resolved Decisions (2026-04-12)

Resolved via structured interview (`agents/interviews/20260409_INTERVIEW_DRAFTBOARD_REFINEMENT.md`, Batches 5–6).

### Cleanup Approval (Batch 5)

- **Q13 → C (Confirmation token):** `board clean` uses a two-step flow: preview generates a confirmation token bound to the reviewed candidate set; apply requires that token. The token is the actual approval primitive, keeping the destructive step tied to a specific board snapshot rather than whatever happens to be stale when apply runs.
- **Q14 → C (MCP also uses confirmation token):** MCP cleanup follows the same token-based approval model as CLI. Whether exposed as two tools or one tool with modes, the apply step must carry the preview token. This keeps CLI and MCP aligned on a single safety contract.
- **Q15 → B (Review-required stale):** When an entry crosses the one-hour threshold it becomes prominently stale and requires explicit operator review, but it is not auto-freed and not removed from the conflict model. `board show`, `status`, and `next` surface a strong warning recommending renew or clean. Two-stage thresholds are deferred as unnecessary v1 complexity.

### Conflict Resolution and Auditability (Batch 6)

- **Q16 → A+B+E (Renew, release files, mark abandoned + clean):** v1 supports three conflict resolution actions: renew (happy path for stale-but-alive work), release specific files (narrow scope without destroying the entry), and mark abandoned + clean (decommission path through the confirmation-token flow). Transfer and force override are deferred — transfer is better modeled as check-out + new check-in; force override can be approximated by mark-abandoned + clean + new check-in.
- **Q17 → A (Conservative):** v1 blocks aggressively and requires human review for anything ambiguous. The cost of a false negative (blocked work) is one operator command; the cost of a false positive (conflicting ownership) is silent data corruption. The policy can be relaxed once real operational data shows where false blocks occur.
- **Q18 → D (Board + ticket history):** Audit records are written to both board state and ticket history. Board captures the operational coordination event; ticket history captures the lifecycle impact. Lease history is redundant now that the board is canonical (per Q7/Q8). Every destructive or state-changing board operation appends entries to both stores with operator identity, timestamp, confirmation token, and reason.

## Deliverables

- Cleanup approval workflow for CLI and MCP
- Stale-entry review states and operator guidance text
- Conflict resolution action set for v1
- Audit requirements for cleanup and override operations
- Recommendation for the simplest safe v1 cleanup UX

## Acceptance Criteria

- [x] Explicit cleanup approval flow is defined for CLI and MCP use cases
- [x] `board clean` semantics distinguish preview/review from destructive cleanup
- [x] One-hour stale-entry escalation behavior is fully specified
- [x] Operator guidance text is defined for stale entries in `board show`, `next`, and `status`
- [x] Conflict resolution actions are defined and scoped for v1
- [x] Audit requirements are documented for renew, clean, override, and transfer actions
- [x] `8aff39cb` is updated to reference the approved stale-entry and cleanup workflow
