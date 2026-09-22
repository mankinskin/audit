# [Board] ticket-api: Cleanup, File Ops, Reconciliation, Claim Deprecation

## Purpose

Add the operational maintenance layer to the draftboard in `crates/ticket-api/`. This builds on the core board storage (types, tables, check-in/out/heartbeat/show/configure) established by `0db86ac1` and adds:

- Confirmation-token cleanup workflow (`board_clean_preview`, `board_clean_apply`)
- Mid-session file ownership mutation (`board_update_files`, `board_rename_file`)
- Lifecycle reconciliation hooks in existing ticket methods
- Public `claim()`/`unclaim()` visibility change to `pub(crate)`

## Component Boundaries

### In scope
- `BoardCleanPreview`, `BoardCleanResult`, `BoardReconcileResult` types (public in `ticket_api`)
- New `BoardError` variants: `StaleCleanToken`, `FileRenameConflict`
- `board_clean_preview()`: returns cleanup candidates and a confirmation token bound to the current board state
- `board_clean_apply(token, include_stale)`: removes only token-identified entries; rejects stale tokens if the board changed materially since preview
- `board_update_files(ticket_id, agent_id, add, remove)`: modify file ownership mid-session with conflict re-check on newly added files
- `board_rename_file(ticket_id, agent_id, old_path, new_path)`: atomic rename transition — releases old path, claims new path, re-checks conflicts, records as single audit event
- `board_reconcile(ticket_id)`: internal helper called by `update_ticket`, `close_ticket`, `cancel_ticket`, `revert_ticket`; returns `BoardReconcileResult`
- Public `claim()`/`unclaim()` methods made `pub(crate)` — external callers must use board commands

### Out of scope
- Core board types and CRUD (established by `0db86ac1`)
- CLI subcommands (owned by `bcc111c6`)
- MCP tools (owned by `ec52f7cb`)
- Integration with `next`/`status` (owned by `74160bb8`)

### Depends on
- `0db86ac1` — core board storage must exist before operations can be added

## Key Data Types (added by this ticket)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoardCleanPreview {
    pub token: String,
    pub completed_candidates: Vec<BoardEntry>,
    pub stale_candidates: Vec<BoardEntry>,
    pub generated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoardCleanResult {
    pub removed_completed: u32,
    pub removed_stale: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ReconcileAction {
    MarkedCompleted,
    IntentStaleWarning,
    NoAction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoardReconcileResult {
    pub action: ReconcileAction,
    pub warnings: Vec<String>,
}
```

### Additional `BoardError` variants (added by this ticket)

```rust
// Appended to the BoardError enum defined in 0db86ac1
#[error("Stale clean token: board state changed since preview at {generated_at}")]
StaleCleanToken { generated_at: DateTime<Utc> },

#[error("File rename conflict on {new_path} with agent {conflicting_agent} (ticket {conflicting_ticket})")]
FileRenameConflict {
    new_path: String,
    conflicting_agent: String,
    conflicting_ticket: Uuid,
},
```

## Implementation Notes

### Cleanup Semantics

- Completed entries are not auto-pruned. They become cleanup-eligible only after `completed_audit_window_secs` elapses.
- `board_clean_preview()` returns a `BoardCleanPreview` with candidates and a confirmation token.
- `board_clean_apply(token, include_stale)` removes only the entries identified in the preview. The token is rejected if the board has changed materially since the preview was generated (e.g. an entry was renewed or a new entry was added that changes the candidate set).
- `include_stale` means stale entries are also removed, after operator confirmation.

### Token Design

The confirmation token encodes enough state to detect board mutations between preview and apply:

1. `board_clean_preview()` collects cleanup-eligible entry IDs, sorts them lexicographically, and concatenates them with a `generated_at` ISO-8601 timestamp.
2. This concatenated string is hashed (SHA-256) to produce the token. The `generated_at` timestamp is also stored in `BoardCleanPreview`.
3. `board_clean_apply(token, include_stale)` re-collects the current cleanup-eligible set, re-computes the hash using the same algorithm and the original `generated_at` from the preview, and compares. If the hashes differ, the board has changed materially and the method returns `BoardError::StaleCleanToken`.
4. This approach is stateless — no server-side token storage is needed. The token is deterministic and tamper-evident (changing any candidate or timestamp changes the hash).

### Board Reconciliation Hooks

Per Q9 (all mutating lifecycle operations trigger reconciliation), the following existing `TicketStore` methods must call a new internal `board_reconcile(ticket_id)` helper:

- `update_ticket()` (state transitions)
- `close_ticket()`
- `cancel_ticket()`
- `revert_ticket()`

`board_reconcile(ticket_id)` checks whether the ticket has an active board entry and returns a `BoardReconcileResult`:
- If the ticket reached a terminal state (`done`, `cancelled`): mark the board entry as `Completed`, set `action = ReconcileAction::MarkedCompleted`, and add a cleanup recommendation to `warnings`.
- If the ticket was reverted to an earlier state: set `action = ReconcileAction::IntentStaleWarning` and add a warning that the board entry's intent may be stale.
- If neither condition applies: set `action = ReconcileAction::NoAction` with empty warnings.
- Reconciliation never silently deletes entries. It flags them for human review.

Callers (the lifecycle methods) log any warnings but do not fail if reconciliation produces them — reconciliation is advisory, not blocking.

### File Rename Transition

`board_rename_file(ticket_id, agent_id, old_path, new_path)` provides an atomic rename: it removes `old_path` from `owned_files`, adds `new_path`, re-checks for conflicts on `new_path`, and records the operation as a single rename audit event. If `new_path` conflicts with another agent's entry, the rename is rejected with `BoardError::FileRenameConflict`. `board_update_files(add, remove)` can also express a rename when `add` and `remove` target the same logical file, but `board_rename_file` is the preferred explicit path.

### Claim/Unclaim Visibility Change

Public `claim()`/`unclaim()` methods are made `pub(crate)`. Board commands (`board_check_in`, `board_check_out`) handle lease management internally. This preserves internal use by board operations while preventing external callers from bypassing the board workflow. Any external code calling `claim()`/`unclaim()` directly must migrate to the board API.

## Acceptance Criteria

- [ ] `BoardCleanPreview`, `BoardCleanResult`, and `BoardReconcileResult` types are public in `ticket_api`
- [ ] `BoardError` has `StaleCleanToken` and `FileRenameConflict` variants
- [ ] `board_clean_preview()` returns candidates and a SHA-256-based confirmation token
- [ ] `board_clean_apply()` removes only token-identified entries and rejects stale tokens via hash comparison
- [ ] `board_update_files()` modifies file ownership with conflict re-check on added files
- [ ] `board_rename_file()` performs atomic rename with conflict check and audit event
- [ ] `board_reconcile()` is called by `update_ticket`, `close_ticket`, `cancel_ticket`, `revert_ticket` and returns `BoardReconcileResult`
- [ ] Reconciliation marks entries `Completed` on terminal ticket states and warns on reverts
- [ ] Public `claim()`/`unclaim()` methods are made `pub(crate)` (not removed)
- [ ] Unit tests cover: confirmation-token cleanup, stale-token rejection, file update conflicts, rename transitions, reconciliation hooks on close/cancel/revert
