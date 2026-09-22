# [Board][Docs] Add board workflow guidance to .github agent files

## Context

The Draftboard feature (epic 854f0e8f) is now fully implemented across all
layers: storage API, CLI subcommands, MCP tools, and validation tests.
Before the epic can be closed, agents need consolidated guidance on how to
use the board in their daily workflow.

Currently the `.github` instruction and prompt files have no mention of
board commands. Agents discovering the board for the first time have no
authoritative reference for:
- when and how to call `board check-in` / `check-out` / `heartbeat`
- what the WIP limit and stale-entry warnings mean
- how `next` and `status` surface board state
- which MCP tools correspond to which CLI verbs

## Problem Statement

Without agent guidance, the board will be underused or misused:
- Agents may skip check-in and cause invisible conflicts on shared files.
- Stale entries will accumulate with no operator response.
- The WIP limit will not be respected as a coordination signal.
- Cross-interface ambiguity (CLI vs MCP) will slow onboarding.

## Acceptance Criteria

- [ ] `ticket-system.instructions.md` gains a **Board Coordination** section
      covering: orientation check, check-in/check-out/heartbeat workflow,
      stale-entry response, WIP limit interpretation, and cleanup approval.
- [ ] `mcp-tools.instructions.md` gains a **Board Tools** section with a
      tool-by-tool reference table, JSON input examples for each MCP tool,
      and guidance on the board-aware `next_tickets` response fields.
- [ ] `AGENTS.md` gains a brief **Board** orientation step in the Discovery
      Protocol section so agents run `board show` as part of their session
      start-up check.
- [ ] The `ticket-system.instructions.md` orientation block references board
      commands alongside the existing `next`, `list`, and `health` commands.
- [ ] All added content is accurate against the current implementation
      (verified by reading the actual CLI help and MCP input structs).

## Implementation Plan

### 1 — `ticket-system.instructions.md`: Board Coordination section

Insert after the existing "Picking Next Work" section, before "Dependency Maintenance".

**Board Coordination** section to add:

```
## Board Coordination

The draftboard tracks which agent is working on each ticket and which files
are owned. Check in when starting implementation; check out when done.

### Session Orientation

Include board state in the session start-up check:

    # Check current board snapshot (all agents)
    ./target/debug/ticket.exe board show --json

    # Check only your own entries (also refreshes their heartbeat)
    ./target/debug/ticket.exe board show --agent-id <your-agent-id> --json

### Check-In / Check-Out / Heartbeat

    # Register yourself as actively working a ticket
    ./target/debug/ticket.exe board check-in <ticket-id> \
      --agent-id <agent-id> \
      --intent "brief description of planned work" \
      --files "src/foo.rs,src/bar.rs" \
      --ttl 3600 \
      --json

    # Refresh your heartbeat before TTL elapses
    ./target/debug/ticket.exe board heartbeat <entry-id> --json

    # Check out when done (records handoff reason in audit trail)
    ./target/debug/ticket.exe board check-out <ticket-id> \
      --agent-id <agent-id> \
      --reason "implemented and tested" \
      --json

### WIP Limit

`board show` reports `wip_limit_reached` and `next` surfaces a warning
when the limit is hit. Do not start new implementation work when the WIP
limit is reached — finish or hand off an existing entry first.

Default limit: 5 simultaneous active entries. Configure:

    ./target/debug/ticket.exe board configure --max-wip 3 --json

### Stale-Entry Response

An entry becomes **stale** when its heartbeat TTL elapses. `board show`
lists stale entries under `warnings[]` and `stale_count`.

Required responses:
1. Agent still active: run `board heartbeat <entry-id>` to renew.
2. Work abandoned: run `board check-out <ticket-id>` then clean.
3. Remove stale entries: `board clean preview --include-stale`, then
   `board clean apply --token <token> --include-stale`.

### File Ownership

Owned files block other agents from checking in with overlapping paths.
Keep owned file lists narrow and release them (via check-out or
update-files) when no longer needed.

    # Add / remove files from an active entry
    ./target/debug/ticket.exe board update-files <ticket-id> \
      --agent-id <agent-id> --add "new.rs" --remove "old.rs" --json

    # Rename a file in an active entry (atomic)
    ./target/debug/ticket.exe board rename-file <ticket-id> \
      --agent-id <agent-id> --old-path "old.rs" --new-path "new.rs" --json
```

Also update the **Orientation** bash block at the top to add:

    # Check the draftboard (active agents, WIP limit, stale warnings)
    ./target/debug/ticket.exe board show --json

via MCP: `mcp_ticket-mcp_board_show` with `{"workspace": "default"}`.

---

### 2 — `mcp-tools.instructions.md`: Board Tools section

Append after the "Available ticket-mcp tools" table.

**Board Tools (ticket-mcp)** section to add:

```
## Board Tools (ticket-mcp)

Nine MCP tools cover the full board lifecycle. All require `workspace`.

### Tool reference

| Tool               | Required                             | Optional                                        |
|--------------------|--------------------------------------|-------------------------------------------------|
| board_show         | workspace                            | agent_id                                        |
| board_check_in     | workspace, ticket_id, agent_id       | intent, files, ttl_secs                         |
| board_check_out    | workspace, ticket_id                 | agent_id, reason                                |
| board_heartbeat    | workspace, entry_id                  | —                                               |
| board_configure    | workspace                            | max_wip, stale_after_secs, completed_audit_window_secs |
| board_clean_preview| workspace                            | include_stale                                   |
| board_clean_apply  | workspace, token                     | include_stale                                   |
| board_update_files | workspace, ticket_id, agent_id       | add, remove                                     |
| board_rename_file  | workspace, ticket_id, agent_id, old_path, new_path | —                              |

### JSON examples

    // board_show — read snapshot (no heartbeat)
    {"workspace": "default"}

    // board_show — read snapshot + refresh caller heartbeat
    {"workspace": "default", "agent_id": "copilot-agent-1"}

    // board_check_in
    {
      "workspace": "default",
      "ticket_id": "abcd1234",
      "agent_id": "copilot-agent-1",
      "intent": "implementing the storage layer",
      "files": ["crates/ticket-api/src/storage/board.rs"],
      "ttl_secs": 3600
    }

    // board_check_out
    {"workspace":"default","ticket_id":"abcd1234","agent_id":"copilot-agent-1","reason":"done"}

    // board_heartbeat
    {"workspace": "default", "entry_id": "<full-UUID-from-check-in>"}

    // board_configure — set WIP limit and stale timeout
    {"workspace": "default", "max_wip": 3, "stale_after_secs": 1800}

    // board_clean_preview — include stale entries
    {"workspace": "default", "include_stale": true}

    // board_clean_apply — consume a preview token
    {"workspace": "default", "token": "<token-from-preview>", "include_stale": true}

### next_tickets board fields

`next_tickets` integrates board state into its response:

- `board.active_count` / `board.stale_count` — current load
- `board.wip_limit_reached` — true when new check-in would be blocked
- `board.warnings[]` — stale-entry alert strings
- `excluded_by_board[]` — tickets excluded because an active/stale entry
  covers them; each object has fields: ticket_id, agent_id, status, intent

When `wip_limit_reached` is true, resolve existing entries before checking in.
```

---

### 3 — `AGENTS.md`: Board step in Discovery Protocol

In the "Use live sources first" numbered list, insert after point 2 (Known issues/plans):

```
2.5. **Board state**: `./target/debug/ticket.exe board show --json` — check
     active WIP, stale entries, and file ownership before touching
     implementation files.
     Via MCP: `mcp_ticket-mcp_board_show` with `{"workspace": "default"}`.
```

---

## Files to Change

| File | Change |
|------|--------|
| `.agents/instructions/ticket-system.instructions.md` | Add Board Coordination section + board show in Orientation bash block |
| `.agents/instructions/mcp-tools.instructions.md` | Add Board Tools section after existing tool table |
| `AGENTS.md` | Insert board show step in Discovery Protocol |

## Verification After Changes

1. Read each added bash snippet and cross-check flags against
   `./target/debug/ticket.exe board <subcommand> --help`.
2. Verify MCP tool names match `name = "board_*"` in
   `tools/mcp/ticket-mcp/src/server.rs`.
3. Verify JSON field names match the `BoardXxxInput` structs in `server.rs`.
4. Confirm markdown is well-formed: all code fences closed, tables balanced,
   no orphan heading levels.
