# [AOH][Design] Local-First Git Management — Branch Lifecycle Without Remote Dependency

## Context

**User decision (Q3):** GitHub is the remote, but PR management should be **local**. Only push to the remote when explicitly merging/sharing. No automatic remote pushes during agent implementation.

This is an important architectural constraint: the full review cycle (diff, review, comment, approve/reject) happens against the **local repository** and only the final merge commit (or the feature branch) is pushed to GitHub.

## Design Goals

1. Agent sessions create and commit to local branches — no `git push` during implementation
2. User reviews diffs locally (or via local viewer) before anything is pushed remote
3. Local "PR" is a structured record (not a GitHub PR) with: source branch, target branch, diff, ticket reference, agent metadata
4. On approval: orchestrator pushes the branch and opens a real GitHub PR OR squash-merges locally and pushes the merge commit only
5. On rejection/change-request: agent revives session locally; PR record updated; no remote interaction

## Resolved Decisions

> **Locked — do not reopen without new evidence.**

| Decision | Choice | Rationale |
|---|---|---|
| Local PR storage | TOML sidecar on `aoh-meta` branch + mirror to ticket fields (Option B+C) | Portable, auditable, no external deps. Ticket fields keep key state accessible without checkout. |
| Remote push timing | Explicit user trigger only | User controls what reaches remote. Accidental pushes prevented. |
| Branch cleanup | After local merge confirmed | Preserve until safe to delete |
| Remote PR creation | On push trigger via octocrab | GitHub PR = optional remote mirror |
| Branch naming | `aoh/{agent-id}/{ticket-slug}` | ADR-11, consistent and grep-friendly |
| Metadata branch | `aoh-meta` (long-lived, never merged to main) | Clean separation of PR records from feature code |
| **Git library: `git2` for v1** | `git2` (libgit2 bindings) | Stable, battle-tested, full API coverage for all needed operations (worktree, branch, commit, diff, merge, config). `gitoxide`/`gix` is promising pure-Rust alternative but worktree management and merge operations are still incomplete as of 2026. Revisit `gix` for v2 when its worktree API stabilizes. |
| **Conflict detection** | Pre-merge `--name-only` overlap check | Before creating a local PR or attempting merge, diff the agent branch against current `main` AND all other open agent branches (`git diff --name-only main..aoh/{other-agent}/...`). If file overlap is detected with another open PR, flag in TUI for operator decision (serialize merges, manual conflict resolution, or reassign). |

### Conflict Detection Strategy

When an agent session completes and creates a local PR:

1. **Against main**: `git diff --name-only main..aoh/{agent}/{ticket}` — always expected; this is the changeset.
2. **Against other open PRs**: For each open local PR with `state=open`, compute `git diff --name-only aoh/{other-agent}/{other-ticket}..aoh/{agent}/{ticket}`. If the intersection of modified files is non-empty → **conflict warning**.
3. **Resolution options** (presented in TUI):
   - **Serialize**: merge the earlier PR first, then rebase the later branch
   - **Manual**: operator resolves conflicts and commits
   - **Reassign**: send the later ticket back to the agent with conflict context
4. **No automatic resolution**: conflicts always surface to operator. Agents do not resolve cross-branch conflicts autonomously in v1.

### gitoxide vs git2 — Detailed Evaluation

| Criterion | `git2` (libgit2) | `gitoxide` (`gix`) |
|---|---|---|
| Worktree create/remove | ✅ Full API | ⚠️ Partial (read-only listing in some versions) |
| Branch operations | ✅ Full | ✅ Full |
| Commit/tree building | ✅ Full | ✅ Full |
| Diff generation | ✅ Full | ✅ Full |
| Merge (in-memory) | ✅ `merge_commits`, index-level | ⚠️ Merge machinery still maturing |
| Config read/write | ✅ Full | ✅ Full |
| Notes API (`refs/notes/`) | ✅ Full | ⚠️ Limited |
| Windows support | ✅ Stable | ✅ Stable |
| Pure Rust | ❌ C dependency (libgit2) | ✅ Pure Rust |
| crates.io maturity | 82M+ downloads | 3.5M+ downloads |

**Decision**: Use `git2` for v1. The worktree and merge APIs are critical for AOH and are fully stable in `git2`. Plan migration to `gix` when worktree + merge APIs reach parity (tracked as future ticket).

## Local PR Record Format

A local PR is stored as a git note or a structured file in the repository:

### Option A: Git Notes
```bash
git notes --ref=refs/notes/aoh-prs add -m "$(cat pr-record.json)" <merge-commit-sha>
```
- Travels with the repo on `git fetch --notes`
- No extra files in worktree
- Low discoverability (requires git notes commands)

### Option B: Sidecar TOML/JSON (tracked file or gitignored)
```
.aoh/prs/{ticket-id}/{session-id}/pr.toml
```
- Human readable, easily parsed by Rust
- Can be committed to a dedicated `aoh-meta` branch (never merged to main)
- Clean separation from feature code

### Option C: Ticket-api integration
- Store PR record as ticket fields / evidence_refs
- PR state transitions drive ticket state machine
- Zero new storage format

**Chosen**: Option B (sidecar TOML on `aoh-meta` branch) + Option C (mirror key state to ticket fields)

## Branch Naming Convention

```
aoh/{agent-id}/{ticket-slug}          # feature branch (agent implementation)
aoh-meta                               # long-lived PR record branch (never merged to main)
```

Agent branch example: `aoh/agent-petal/implement-auth-provider`

## Git Operations Performed by Orchestrator

### Setup (before agent session)
```bash
git worktree add .aoh/worktrees/{session-id} -b aoh/{agent-id}/{ticket-slug}
git -C .aoh/worktrees/{session-id} config user.name "{agent-name}"
git -C .aoh/worktrees/{session-id} config user.email "{agent-email}"
```

### During Session (agent commits freely)
```bash
# Agent commits incrementally — orchestrator does not interfere
git -C .aoh/worktrees/{session-id} commit -am "wip: ..."
```

### On Session Completion (result received)
```bash
# Create local PR record
git checkout aoh-meta
cat > .aoh/prs/{ticket-id}/pr.toml << EOF
[pr]
ticket_id = "..."
agent_id = "..."
session_id = "..."
source_branch = "aoh/{agent-id}/{ticket-slug}"
target_branch = "main"
state = "open"
created_at = "..."
[agent_report]
# ... structured result from agent
EOF
git add .aoh/prs/{ticket-id}/pr.toml && git commit -m "pr: open {ticket-id}"
git checkout main  # back to main for orchestrator work
```

### Review Workflow (local diff viewer)
```bash
# Orchestrator exposes diff for review
git diff main..aoh/{agent-id}/{ticket-slug}
git log --oneline main..aoh/{agent-id}/{ticket-slug}
# User views in VS Code git panel, ticket-viewer, or TUI diff viewer
```

### On Approval — Local Merge Only
```bash
# Squash merge into main locally
git checkout main
git merge --squash aoh/{agent-id}/{ticket-slug}
git commit -m "{ticket-id}: {ticket-title} [Agent: {agent-id}]"
# Update PR record
# git checkout aoh-meta && update pr.toml state=merged
```

### On Approval — Push to Remote + GitHub PR (optional)
```bash
# Push feature branch for GitHub PR creation
git push origin aoh/{agent-id}/{ticket-slug}
# Then call GitHub API: POST /repos/.../pulls
# On GitHub merge, orchestrator syncs local main with remote
git pull --ff-only origin main
```

### On Rejection
```bash
# Update local PR record state=changes-requested
# Orchestrator revives agent session in same worktree
# Agent reads pr.toml for change requests and continues
```

### Cleanup After Merge
```bash
git worktree remove .aoh/worktrees/{session-id}
git branch -d aoh/{agent-id}/{ticket-slug}
# Archive PR record
git checkout aoh-meta && mv .aoh/prs/{ticket-id} .aoh/archive/prs/{ticket-id}
```

## Local PR Viewer (TUI)

The orchestrator TUI should show:
```
┌─ Local PRs ──────────────────────────────────────────────────────┐
│ open   #1  [aoh/agent-petal/implement-auth]  ← main    +142 -23 │
│ open   #2  [aoh/agent-cedar/fix-sandbox-boot] ← main   +38  -5  │
│ merged #0  [aoh/agent-fern/update-deps]       ← main   +12  -3  │
├──────────────────────────────────────────────────────────────────┤
│ [d] diff  [r] review  [a] approve  [x] reject  [p] push-remote  │
└──────────────────────────────────────────────────────────────────┘
```

## GitHub Remote Integration (Deferred to Push Time)

When user decides to push:
1. Orchestrator pushes branch: `git push origin aoh/{agent-id}/{ticket-slug}`
2. Creates GitHub PR via octocrab
3. GitHub PR body includes link to local ticket record
4. On GitHub PR merge: `git pull --ff-only` to sync local main
5. Delete remote branch after merge

Local PR record remains authoritative; GitHub PR is the remote mirror.

## Key Decisions

| Decision | Choice | Rationale |
|---|---|---|
| Local PR storage | TOML on `aoh-meta` branch | Portable, auditable, no external deps |
| Remote push timing | Explicit user trigger only | User controls what reaches remote |
| Branch cleanup | After local merge confirmed | Preserve until safe to delete |
| Remote PR creation | On push trigger | GitHub PR = optional remote record |
| gitoxide vs git2 | **git2 for v1** | Stable worktree + merge APIs; gix worktree still incomplete |

## Rust Crate Candidates

| Crate | Purpose | Status | Decision |
|---|---|---|---|
| `git2` | libgit2 bindings — worktree, branch, commit, diff | Stable, widely used | **v1 dependency** |
| `gitoxide` / `gix` | Pure Rust git implementation | Active; worktree/merge incomplete | **Monitor for v2** |
| `octocrab` | GitHub API (PR creation, push) | Active | **v1 dependency** |

## Acceptance Criteria

- [x] Branch naming convention documented and enforced in sandbox-manager
- [x] Local PR TOML schema defined (all required fields)
- [x] `aoh-meta` branch lifecycle documented (creation, update, archive)
- [x] Worktree create/remove script sequence documented
- [x] Remote push trigger flow documented (manual vs automatic)
- [x] gitoxide vs git2 choice recorded with rationale
- [x] TUI PR viewer wireframe defined
- [x] Conflict resolution when two agent branches modify same file — local detection documented