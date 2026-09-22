# [AOH][Design] Session Archive, Artifact Retention, and Revival Schema

## Objective

Turn ADR-9 into an implementation-ready contract.

The current architecture says sessions are revived by injecting `session-archive.toml` plus archived context, but there is no dedicated definition of what the archive contains, which artifacts are retained, where they live, how long they persist, or what exactly a revival is allowed to reuse.

## Why This Needs Its Own Ticket

This affects multiple subsystems simultaneously:
- `agent-session` needs a stable archive schema
- `sandbox-manager` needs to know what workspace/container state is reusable
- `pr-manager` and review flow need to preserve evidence and change requests
- notifier/TUI need stable links to archived artifacts
- cost/accounting needs historical run data

Without a dedicated contract, each subsystem will make different assumptions.

## Resolved Decisions (2026-04-09)

The following questions were answered during the design interview. These decisions are now locked as ADRs in `34bc4938`.

| Question | Decision |
|---|---|
| **Q5: Archive storage layout** | **Option A** — `.aoh/archive/{ticket-id}/{session-id}/` inside the repository working tree. `.aoh/` is excluded from git via `.gitignore`. Locked as **ADR-14**. |
| **Q6: Revival source of truth** | **Agent branch is canonical after rebase.** On revival, the orchestrator rebases the agent branch onto the latest `main`. The agent resolves any conflicts in-container. After the rebase, branch state is authoritative; the archive TOML provides context only. Locked as **ADR-15**. |
| **Q7: Archive retention** | **Keep indefinitely.** Archives are retained until the operator explicitly prunes them (e.g. via `aoh archive prune`). No automatic pruning by terminal state or age. Locked as **ADR-14**. |
| **Q8: Multiple revivals versioning** | **Option C** — Each `session-archive.toml` contains an optional `revival_of` field referencing the prior session ID for the same ticket. Revivals form a linked chain; each revival gets a fresh, independent session ID. Locked as **ADR-14**. |

### Remaining Questions — Resolved During Refinement

> **Locked — do not reopen without new evidence.**

#### session-archive.toml Schema

```toml
# Required fields
[session]
id = "a1b2c3d4-..."                           # UUIDv4 session identifier
ticket_id = "f345b954-..."                     # Ticket this session implements
agent_id = "agent-petal"                       # Persona ID assigned to this session
persona_display_name = "Petal"                 # Human-readable persona name
branch = "aoh/agent-petal/implement-auth"      # Git branch name
worktree_path = ".aoh/worktrees/a1b2c3d4"     # Worktree path (relative to repo root)
created_at = "2026-04-09T14:30:00Z"            # ISO 8601
completed_at = "2026-04-09T15:45:00Z"          # ISO 8601 (set on completion/termination)
state = "completed"                            # completed | failed | terminated | cancelled

# Optional revival chain
revival_of = "prev-session-uuid"               # Prior session ID (empty for first session)
revival_depth = 1                              # 0 for original, increments per revival

# Agent report (structured result from agent)
[session.result]
summary = "Implemented auth provider with JWT validation and refresh token support."
modified_files = [
    "crates/auth/src/provider.rs",
    "crates/auth/src/jwt.rs",
    "crates/auth/tests/auth_tests.rs",
]
tests_added = 4
tests_passed = 4
tests_failed = 0
open_questions = [
    "Should refresh tokens expire after 30 days or be indefinite?",
]

# Cost metrics
[session.cost]
tokens_input = 125000
tokens_output = 42000
api_calls = 47
estimated_cost_usd = 1.23
wall_time_seconds = 4500
budget_limit = 200000                          # Token budget allocated
budget_used_pct = 83.5

# Review state (updated by orchestrator during review cycle)
[session.review]
pr_state = "open"                              # open | changes-requested | approved | merged | rejected
reviewer = ""                                  # Operator identity who reviewed
reviewed_at = ""                               # ISO 8601
change_requests = []                           # Array of change request strings from reviewer
approval_comment = ""

# Container state (for sandbox reuse decisions)
[session.container]
image = "aoh-sandbox:latest"
container_id = "abc123..."                     # Docker container ID (if preserved)
volume_name = "aoh-vol-a1b2c3d4"              # Named volume (if used)
container_preserved = false                    # true if container was stopped but not removed
volume_preserved = true                        # true if named volume still exists

# Notifier correlation
[session.notifications]
telegram_message_ids = [12345, 12346]          # For thread continuity on revival
discord_thread_id = "..."
slack_thread_ts = "..."
```

#### Artifact Directory Layout

```
.aoh/archive/{ticket-id}/{session-id}/
├── session-archive.toml          # Main archive record (schema above)
├── logs/
│   ├── agent-stdout.log          # Full agent stdout
│   ├── agent-stderr.log          # Full agent stderr
│   └── test-output.log           # Test runner output
├── diffs/
│   └── final.patch               # git format-patch of all agent commits
├── evidence/
│   ├── screenshots/              # Optional screenshots (e.g., from browser tests)
│   │   └── {timestamp}.png
│   └── structured-result.json    # Machine-readable result (same data as [session.result])
└── context/
    └── kickoff-prompt.md          # The exact prompt used to start the session
```

**Naming convention**: All files use lowercase with hyphens. Timestamps in filenames use `YYYYMMDD-HHmmss` format.

#### Evidence Path Format

Artifact references in tickets, PR records, and notifications use **relative paths** from the repository root:

```
.aoh/archive/{ticket-id}/{session-id}/logs/agent-stdout.log
.aoh/archive/{ticket-id}/{session-id}/diffs/final.patch
.aoh/archive/{ticket-id}/{session-id}/evidence/screenshots/20260409-143000.png
```

TUI and notifier use these paths for deep-linking:
- **TUI**: Opens file in `$EDITOR` or inline pager
- **Messenger**: Truncated path shown as clickable context (operator must access locally)
- **Ticket fields**: `evidence_refs` array stores these relative paths

#### Retention by Terminal State

All archives are retained indefinitely (ADR-14), but cleanup behavior varies:

| Terminal state | Archive | Worktree | Container | Volume | Branch |
|---|---|---|---|---|---|
| **completed → merged** | Retained | Removed | Removed | Removed | Deleted |
| **completed → rejected** | Retained | Retained (for revival) | Stopped (not removed) | Retained | Retained |
| **failed** | Retained | Removed | Removed | Retained (for debugging) | Retained |
| **terminated** | Retained | Removed | Force-removed | Removed | Retained |
| **cancelled** | Retained | Removed | Removed | Removed | Deleted |

**Explicit prune**: `aoh archive prune` removes archive directories. Flags:
- `--ticket {id}` — prune all archives for a specific ticket
- `--before {date}` — prune archives older than date
- `--state {merged|cancelled}` — prune only terminal-state archives
- `--dry-run` — show what would be pruned without deleting

#### Revival Reuse Contract

On revival, the orchestrator:

1. **Reuses**: persona (same agent_id via ticket field), branch (rebased onto latest main), archive context (summary injected into kickoff prompt)
2. **Creates fresh**: session ID, worktree (new path), container (new instance from same image), volume (new unless debugging failed session)
3. **Injects from archive**: `session.result.summary`, `session.result.open_questions`, `session.review.change_requests` — concatenated into the revival kickoff prompt
4. **Does NOT reuse**: container state, environment variables, secret nonces (all regenerated)

**Revival kickoff prompt addition:**
```markdown
## Context from Prior Session
This is a revival of session `{revival_of}` (revival depth: {revival_depth}).

### Prior session summary
{session.result.summary}

### Change requests from reviewer
{session.review.change_requests joined with newlines}

### Open questions from prior session
{session.result.open_questions joined with newlines}

Your branch `{branch}` has been rebased onto the latest `main`.
Continue from where the prior session left off.
```

## Questions to Resolve

### Archive content
1. What fields must `session-archive.toml` contain?
   - session ID
   - ticket ID
   - agent/persona ID
   - branch/worktree path
   - summary
   - modified files
   - validation results
   - open questions
   - cost metrics
   - notifier correlation IDs
   - review/change-request history
2. Which artifacts are stored separately from the TOML?
   - stdout/stderr logs
   - test logs
   - screenshots
   - patch/diff snapshot
   - structured result JSON

### Retention and lifecycle
3. How long are archives kept locally?
4. What is pruned automatically vs only by explicit user action?
5. What is the cleanup behavior after merge, cancellation, or hard termination?

### Revival semantics
6. What constitutes a revival?
   - same branch/worktree reused
   - same container image reused
   - same named volume reused
   - same persona reused
7. Which state is authoritative on revival: archive summary, current branch diff, local PR record, or ticket fields?
8. Can multiple revivals exist for the same ticket, and how are they versioned?

### Evidence and auditability
9. How do archives reference evidence needed for review or bug triage?
10. How are artifact paths represented so TUI/notifier can deep-link safely?
11. What must remain readable after worktree cleanup?

## Alternatives to Consider

### Storage layout
- **Option A**: `.aoh/archive/{ticket-id}/{session-id}/...` inside repo metadata branch/worktree
- **Option B**: external orchestrator data dir with ticket store references
- **Option C**: hybrid: metadata in repo, large artifacts outside repo

### Revival source of truth
- **Option A**: archive TOML is canonical; branch/worktree is a mutable implementation detail
- **Option B**: branch diff is canonical; archive is summary-only
- **Option C**: local PR record is canonical once reporting completes

## Deliverables

- Archive schema for `session-archive.toml`
- Artifact directory layout and retention policy
- Revival state machine and versioning rules
- Clear contract for what survives cleanup and what is disposable
- Recommendation for archive storage location and source-of-truth hierarchy

## Acceptance Criteria

- [x] `session-archive.toml` schema is documented with required and optional fields
- [x] Artifact directory layout is documented with naming conventions
- [x] Retention policy defined for merged, rejected, failed, and cancelled sessions
- [x] Revival semantics defined for worktree, branch, container, volume, and persona reuse
- [x] Versioning rules defined for multiple revivals of the same ticket
- [x] Evidence/artifact references are specified so TUI and notifier can link safely
- [x] Architecture ticket updated to reference the approved archive/revival contract