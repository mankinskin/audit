# [Board] ticket-cli: Board Subcommand Family

## Purpose

Expose all draftboard operations as `ticket board <subcommand>` in the CLI. This is the primary human and agent interface for draftboard coordination. All subcommands follow the existing CLI conventions: `--json` for machine-readable output, consistent error envelope format, and prefix-based ticket ID resolution.

## Component Boundaries

### In scope
- New `board` subcommand group in `tools/cli/ticket-cli/src/cli.rs`
- New `commands/board.rs` module with handler functions
- Sub-subcommands: `show`, `check-in`, `check-out`, `heartbeat`, `configure`, `clean`, `update-files`
- Human-readable table output for `board show` (active entries, stale warnings, WIP meter)
- JSON envelope output for all subcommands via `--json`
- Argument validation and error messages

### Out of scope
- Board API logic (owned by `0db86ac1` — this ticket calls store methods only)
- MCP tool registration (owned by `ec52f7cb`)
- Integration with `next`/`status` (owned by `74160bb8`)

## CLI Argument Definitions

### `ticket board show`
```
ticket board show [--agent <AGENT_ID>] [--json]
```
Returns the full board snapshot. Human-readable output includes:
- WIP meter: `[3/5 active] [1 stale] [0 conflicts]`
- Table of entries: ticket ID (short), title, agent, intent, last heartbeat, status
- Warnings section (stale entries, conflicts)

When `--agent` is supplied, the command first performs a read-only snapshot and then issues a follow-up heartbeat for that agent. The snapshot itself is never mutated in-place.

### `ticket board check-in`
```
ticket board check-in <ID> --agent <AGENT_ID> [--intent "..."] [--files f1 f2 ...] [--ttl-secs N] [--json]
```
- `ID`: ticket UUID or 8+ char prefix
- `--agent`: required, agent session identifier
- `--intent`: optional, free-text description of planned work
- `--files`: optional, space-separated workspace-relative file paths
- `--ttl-secs`: optional, default 3600

### `ticket board check-out`
```
ticket board check-out <ID> [--agent <AGENT_ID>] [--reason "..."] [--json]
```
- `ID`: ticket UUID or prefix
- `--agent`: optional if only one agent is checked in for this ticket
- `--reason`: optional exit/handoff reason for audit

### `ticket board heartbeat`
```
ticket board heartbeat <ID> --agent <AGENT_ID> [--json]
```

### `ticket board configure`
```
ticket board configure [--max-wip N] [--stale-after-secs N] [--completed-audit-window-secs N] [--json]
```
- No args: display current config
- With args: update config

### `ticket board clean`
```
ticket board clean preview [--json]
ticket board clean apply <TOKEN> [--include-stale] [--json]
```

`board clean preview` shows cleanup candidates (completed entries past the audit window, stale entries) and returns a confirmation token. `board clean apply` executes cleanup using that token. The token is rejected if the board changed materially since the preview. With `--include-stale`, stale entries are also removed after explicit operator review.

### `ticket board update-files`
```
ticket board update-files <ID> --agent <AGENT_ID> [--add f1 f2 ...] [--remove f3 f4 ...] [--json]
```

### `ticket board rename-file`
```
ticket board rename-file <ID> --agent <AGENT_ID> --from <OLD_PATH> --to <NEW_PATH> [--json]
```
Atomic file rename: releases old path and claims new path as a single audited transition.

### `ticket update` convenience path
```
ticket update <ID> ... --board-check-in --agent <AGENT_ID> [--board-intent "..."] [--board-files f1 f2 ...] [--board-ttl-secs N] [--json]
```

This is an explicit convenience path only. The caller must opt in with `--board-check-in` and provide the required board arguments. `ticket update` does not automatically check the agent into the draftboard on every state transition.

## Output Examples

### `ticket board show` (human-readable)
```
Board Status: [3/5 active] [1 stale ⚠] [0 conflicts]

  TICKET     TITLE                          AGENT              INTENT                    HEARTBEAT   STATUS
  854f0e8f   Draftboard epic                copilot-session-1  Writing descriptions       2m ago      active
  34bc4938   Architecture design            copilot-session-2  Updating ADRs              8m ago      active
  8c185de3   Copilot auth client            copilot-session-3  Implementing CopilotClient 65m ago     stale ⚠

⚠ Stale entries (1):
  8c185de3 by copilot-session-3 — last heartbeat 65m ago (TTL: 60m). High-priority human review required: renew the lease or explicitly clean the entry.

Files:
  src/copilot.rs          → copilot-session-3
  src/architecture.md     → copilot-session-2
```

### `ticket board check-in` (JSON)
```json
{
  "command": "board_check_in",
  "status": "ok",
  "entry": {
    "ticket_id": "854f0e8f-...",
    "agent_id": "copilot-session-1",
    "status": "active",
    "wip": "3/5"
  }
}
```

## Batch Integration

`board check-in`, `board check-out`, `board heartbeat`, and `board clean` should be usable inside `ticket batch` for transactional multi-command workflows.

## Acceptance Criteria

- [ ] `ticket board show` displays human-readable board snapshot with WIP meter, entry table, warnings, and file ownership
- [ ] `ticket board show --json` returns `BoardShowResult` as JSON envelope
- [ ] `ticket board show --agent <AGENT_ID>` performs a follow-up heartbeat without changing the read-only snapshot semantics
- [ ] `ticket board check-in` validates all arguments and calls `store.board_check_in()`
- [ ] `ticket board check-out` calls `store.board_check_out()` with agent resolution
- [ ] `ticket board check-out` accepts optional `--reason` for handoff audit
- [ ] `ticket board heartbeat` calls `store.board_heartbeat()` and returns updated entry
- [ ] `ticket board configure` reads/writes board config through the store
- [ ] `ticket board clean preview` displays candidates and returns a confirmation token
- [ ] `ticket board clean apply` only removes token-identified entries and rejects stale tokens; stale entries removed only with `--include-stale`
- [ ] `ticket board update-files` modifies file ownership
- [ ] `ticket board rename-file` performs atomic rename with audit event
- [ ] All subcommands support `--json` output
- [ ] Error messages are clear: WIP limit, file conflict, not-checked-in
- [ ] `ticket update --board-check-in` explicitly composes ticket update and board check-in when the required board arguments are supplied
- [ ] Board subcommands work inside `ticket batch`
- [ ] Integration test: full check-in → heartbeat → update-files → check-out cycle via CLI
