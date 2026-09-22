# [Board] ticket-mcp: Board Tool Endpoints for Agent Coordination

## Purpose

Expose the draftboard as MCP tools so that agent sessions can coordinate through the MCP protocol without shelling out to the CLI. This is the primary machine interface for agent-to-board interaction during automated sessions.

## Component Boundaries

### In scope
- New MCP tools in `tools/mcp/ticket-mcp/` registered on the `ToolRouter`
- Tools: `board_show`, `board_check_in`, `board_check_out`, `board_heartbeat`, `board_configure`, `board_clean_preview`, `board_clean_apply`, `board_update_files`, `board_rename_file`
- JSON schema definitions for all tool parameters and return types
- Error mapping from `BoardError` → MCP error responses

### Out of scope
- Board storage logic (owned by `0db86ac1`)
- CLI output formatting (owned by `bcc111c6`)
- HTTP endpoint exposure (future work if ticket-http needs it)

## MCP Tool Definitions

### `board_show`
**Parameters:**
- `workspace` (string, required): Workspace name
- `agent_id` (string, optional): Caller session identifier. When supplied, the tool performs a follow-up heartbeat after the read-only snapshot.

**Returns:** `BoardShowResult` — `{ snapshot: BoardSnapshot, heartbeat?: BoardEntry }`.

**Usage:** Agent sessions call this at startup to orient: understand current WIP, find stale entries, discover file conflicts before starting work. The snapshot is always produced from a read-only store call; if `agent_id` is present, the tool then issues `board_heartbeat()` as a second store call and returns the refreshed entry alongside the snapshot.

### `board_check_in`
**Parameters:**
- `workspace` (string, required)
- `ticket_id` (string, required): UUID or prefix
- `agent_id` (string, required): Caller's session identifier
- `intent` (string, optional): Description of planned work
- `files` (string[], optional): Files the agent will own
- `ttl_secs` (number, optional): Heartbeat TTL in seconds (default: 3600)

**Returns:** `BoardEntry` on success. `BoardError` on WIP limit or file conflict.

**Usage:** Agent calls this immediately after selecting a ticket to work on.

### `board_check_out`
**Parameters:**
- `workspace` (string, required)
- `ticket_id` (string, required)
- `agent_id` (string, optional)
- `reason` (string, optional): Exit/handoff reason for audit

**Returns:** Success confirmation or `NotCheckedIn` error.

**Usage:** Agent calls this when finishing work on a ticket (session end, ticket state advanced, or abandoning).

### `board_heartbeat`
**Parameters:**
- `workspace` (string, required)
- `ticket_id` (string, required)
- `agent_id` (string, required)

**Returns:** Updated `BoardEntry` with refreshed TTL.

**Usage:** Long-running sessions call this periodically (recommended: every `ttl_secs / 2`).

### `board_configure`
**Parameters:**
- `workspace` (string, required)
- `max_wip` (number, optional)
- `stale_after_secs` (number, optional)

**Returns:** Current `BoardConfig` (after any updates).

### `board_clean_preview`
**Parameters:**
- `workspace` (string, required)

**Returns:** `BoardCleanPreview` — `{ token, completed_candidates, stale_candidates, generated_at }`.

**Usage:** Returns cleanup candidates and a confirmation token. The token must be passed to `board_clean_apply` to execute the cleanup.

### `board_clean_apply`
**Parameters:**
- `workspace` (string, required)
- `token` (string, required): Confirmation token from `board_clean_preview`
- `include_stale` (boolean, optional, default: false)

**Returns:** `BoardCleanResult` with counts of removed entries.

**Usage:** Executes cleanup using the preview token. Rejects the token if the board has changed materially since the preview was generated. `include_stale` is intended for cases where the operator has already reviewed the stale entries and decided to remove them.

### `board_update_files`
**Parameters:**
- `workspace` (string, required)
- `ticket_id` (string, required)
- `agent_id` (string, required)
- `add` (string[], optional): Files to add to ownership
- `remove` (string[], optional): Files to release from ownership

**Returns:** Updated `BoardEntry`.

**Usage:** Modify file ownership mid-session. Conflict detection runs on newly added files.

### `board_rename_file`
**Parameters:**
- `workspace` (string, required)
- `ticket_id` (string, required)
- `agent_id` (string, required)
- `old_path` (string, required)
- `new_path` (string, required)

**Returns:** Updated `BoardEntry`.

**Usage:** Atomic file rename transition: releases old path and claims new path as one audited operation.

## Agent Workflow Integration

Recommended agent session startup sequence:

```
1. board_show(agent_id=self) → read current state, refresh own heartbeat, check WIP budget
2. next_tickets → get unblocked work (filtered by board state)
3. board_check_in → register selected ticket + file ownership
4. ... do work, periodically board_heartbeat ...
5. board_check_out → release ticket on completion
```

## Acceptance Criteria

- [ ] `board_show` tool registered and returns `BoardShowResult` JSON
- [ ] `board_show(agent_id=...)` performs read-only snapshot plus follow-up heartbeat as two store calls
- [ ] `board_check_in` validates inputs and returns `BoardEntry` or structured error
- [ ] `board_check_out` accepts optional `reason` for handoff audit and releases board entry and lease
- [ ] `board_heartbeat` updates TTL and returns refreshed entry
- [ ] `board_configure` reads/writes board configuration
- [ ] `board_clean_preview` returns candidates and confirmation token
- [ ] `board_clean_apply` prunes only token-identified entries and rejects stale tokens; stale entries removed with `include_stale`
- [ ] `board_update_files` modifies file ownership with conflict re-check
- [ ] `board_rename_file` performs atomic rename with audit event
- [ ] All tools handle workspace resolution consistently with existing ticket MCP tools
- [ ] Error responses from `BoardError` are structured and actionable
- [ ] Tools appear in `mcp_ticket-mcp_help` output
- [ ] Integration test: full MCP tool cycle (show → check-in → heartbeat → check-out → show)
