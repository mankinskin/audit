# [Board][Design] Entry Identity, Resume Flow, and Synchronization Invariants

## Objective

Close the remaining correctness gaps around what a draftboard entry actually represents, how an agent resumes existing work, and how board state stays synchronized with leases and ticket state transitions.

The current design is strong on surface API shape, but it still leaves several foundational invariants underspecified. If these questions are not answered before implementation, the first failures will be subtle synchronization bugs rather than obvious command errors.

## Why This Needs Its Own Ticket

The current design uses a compound key of `"{ticket_id}:{agent_id}"` and retains completed entries through an audit window. That creates immediate ambiguity when the same agent later resumes or re-checks into the same ticket:

- does the old completed entry get overwritten?
- does the new check-in fail because an entry already exists?
- do we need a stable `entry_id` / `session_id` independent of agent and ticket?

There is also no fully defined source-of-truth hierarchy between:
- board entries
- leases (`claim` / `unclaim`)
- ticket state (`new`, `in-implementation`, `in-review`, ...)

Without explicit invariants, the system will drift under normal use.

## Concrete Problems to Resolve

### 1. Entry identity and reuse
- Is a board entry identified by `(ticket_id, agent_id)` or by a separate immutable `entry_id` / `session_id`?
- What happens when the same agent checks back into the same ticket before the old completed entry has been cleaned?
- Can multiple agents be checked into the same ticket at the same time if they claim disjoint files?
- If yes, how are they displayed and filtered in `next`, `status`, and `board show`?

### 2. Resume and handoff flow
- When an agent starts and already has an active or stale board entry, what is the expected resume workflow?
- Should `board_show(agent_id=self)` highlight "your existing work" separately from the global board?
- Should `next` / `next_tickets` return a resume recommendation before proposing new work?
- How does intentional handoff between agents work for the same ticket?

### 3. Synchronization invariants
- What must always be true between board entry state, lease state, and ticket state?
- If a ticket is closed, cancelled, reverted, or moved back from `in-review`, what board updates are required?
- What happens if someone uses `claim` / `unclaim` directly without the board commands?
- Is the board the primary source of truth for short-term ownership, with leases as a compatibility mirror, or are they peers?

### 4. File ownership canonicalization
- How are file paths normalized before conflict detection?
- How do we handle Windows vs WSL path separators and case sensitivity?
- How do we represent renamed files, deleted files, and generated outputs?
- Is directory-level ownership supported, or files only?

## Resolved Decisions (2026-04-12)

Resolved via structured interview (`agents/interviews/20260409_INTERVIEW_DRAFTBOARD_REFINEMENT.md`, Batches 1–4).

### Entry Identity (Batch 1)

- **Q1 → C (Hybrid identity):** Each board entry gets an immutable `entry_id` (UUID) for audit and history. Active entry uniqueness is enforced on `(ticket_id, agent_id)`. The `entry_id` survives completion and is used in confirmation tokens, audit logs, and attempt linkage.
- **Q2 → C (New linked attempt):** When the same agent re-checks into a ticket after a completed entry, a new entry is created with a fresh `entry_id` and an optional `previous_attempt: Option<Uuid>` linking to the prior completed entry. The old entry is never overwritten or resumed in place.
- **Q3 → A (One active entry per ticket in v1):** Only one agent may hold an active board entry for a given ticket at a time. Multi-agent same-ticket check-in is deferred to a future release.

### Resume and Handoff (Batch 2)

- **Q4 → A (Strongly recommend resume):** When an agent calls `board_show(agent_id=self)` and already has an active or stale entry, the response should prominently recommend resuming that work before suggesting new tickets.
- **Q5 → A (Separate caller-owned section):** `board_show(agent_id=self)` returns a dedicated "your work" section listing the caller's active/stale entries before the global board snapshot.
- **Q6 → A (Handoff = check-out + new check-in):** Intentional handoff in v1 is modeled as the original agent checking out, followed by the new agent checking in. An optional `handoff_reason` metadata field on the check-out captures why the transfer happened.

### Synchronization Invariants (Batch 3)

- **Q7 → A (Board is primary):** The board is the canonical short-term ownership system. Leases are internal compatibility mirrors maintained by board operations. Ticket state is lifecycle state, not ownership truth.
- **Q8 → D (Remove public claim/unclaim):** Public raw `claim` / `unclaim` workflows should be removed in favor of board commands. Any lease primitive that remains is internal/admin-only.
- **Q9 → D (All mutating lifecycle operations reconcile):** Every mutating ticket lifecycle operation (close, cancel, revert, state transitions) must trigger board reconciliation — checking whether active ownership is still valid and surfacing conflicts or required cleanup.

### File Ownership Rules (Batch 4)

- **Q10 → B (Workspace-relative lexical normalization):** File paths are normalized to workspace-relative spelling with collapsed `.` / `..` and consistent separators before conflict detection. No filesystem I/O is required, so planned-but-unwritten files are supported.
- **Q11 → C (Workspace/platform-aware case sensitivity):** Path comparison respects the workspace's actual filesystem semantics rather than forcing a global case-sensitive or case-insensitive policy.
- **Q12 → B (Files + explicit renamed-file transitions):** v1 supports file-level ownership plus an explicit rename transition that atomically releases the old path and claims the new one. Directory-level and generated-output-group ownership are deferred.

## Deliverables

- Canonical identity model for board entries
- Resume / handoff workflow for agents with existing entries
- Synchronization invariants between board, leases, and ticket state
- File ownership canonicalization rules for cross-platform use
- Recommendation for whether same-ticket multi-agent work is supported in v1

## Acceptance Criteria

- [x] Board entry identity is defined unambiguously (`entry_id` / `session_id` vs composite key)
- [x] Re-check-in semantics are defined for the same agent and ticket across audit windows
- [x] Resume workflow is specified for agents that already own active or stale entries
- [x] Same-ticket multi-agent semantics are either explicitly supported or explicitly forbidden in v1
- [x] Synchronization invariants between board entries, leases, and ticket states are documented
- [x] Required hooks are identified for ticket close/cancel/revert/update paths
- [x] File ownership canonicalization rules are specified for Windows/WSL/Linux
- [x] `8aff39cb` is updated to reference the approved identity and synchronization contract
