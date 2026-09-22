# [AOH][Impl] Orchestrator Core — Daemon, Scheduler, Secret Server, Conflict Detector

## Purpose

Central daemon process that hosts all AOH orchestration services. The orchestrator core is the main binary (`orchestrator-tui` crate) that wires together the session scheduler, secret server, conflict detector, cost watchdog, audit logger, and PR manager into a single long-running process. It owns the `tokio` runtime and the internal event routing (`tokio::mpsc`) that connects the assignment runner, review coordinator, notifier adapters, and TUI.

This ticket was identified as a gap during reconciliation (`02412b9a`): no existing Phase 2 implementation ticket covered the daemon process itself, the session scheduler, or the secret server.

## Component Boundaries

### In scope
- **Main daemon binary**: CLI entrypoint, config loading (`orchestrator.toml`), graceful shutdown
- **Session scheduler**: decides which ready tickets to assign next, respects WIP limits, selects personas via `agent-identity` crate
- **Secret server**: HTTP or Unix socket endpoint serving one-time nonce-gated secrets to containers (ADR-13)
  - UUIDv4 nonce per session, 60s TTL, single-use consumption
  - Docker Desktop: `host.docker.internal:{port}` with per-session port
  - Linux: bind-mounted Unix socket `/tmp/aoh-{session-id}.sock`
- **Conflict detector**: pre-assignment file-overlap check against running sessions and open PRs
- **Cost watchdog**: token/time budget tracking, soft/hard signal emission (ADR-10)
- **Audit logger**: append-only JSONL to `.aoh/audit/audit.jsonl`
- **Internal event bus**: `tokio::mpsc` channels routing `ProgressEvent` from assignment runner to TUI and notifier
  - Runner holds N `mpsc::Sender` handles (one per subscriber: TUI, notifier, audit logger)
  - This is intra-process plumbing per ADR-6 clarification, NOT cross-agent coordination
- **Config model**: `orchestrator.toml` parsing — operator allow-list, budget defaults, messenger credentials, persona pool path, WIP limit

### Out of scope
- Sandbox provisioning (owned by `51471c3e`)
- Assignment runner session lifecycle (owned by `a8632357`)
- Review coordinator (owned by `d0cc3c8b`)
- Notifier adapters (owned by `8db8ef2f`)
- TUI rendering (owned by `5af54f6c`)
- Agent identity / persona store (owned by `a92569e5`)

## Key Data Types

```rust
/// Top-level orchestrator configuration.
struct OrchestratorConfig {
    operators: Vec<OperatorEntry>,          // allow-list (ADR-12)
    budget_defaults: BudgetLimits,          // ADR-10
    wip_limit: u32,                         // max concurrent sessions
    persona_pool: PathBuf,                  // path to personas.toml
    messengers: MessengerConfig,            // Telegram/Discord/Slack credentials
    secret_server: SecretServerConfig,      // bind address, TTL
    audit_path: PathBuf,                    // .aoh/audit/audit.jsonl
    archive_root: PathBuf,                  // .aoh/archive/
}

/// Secret server state.
struct SecretServer {
    pending: HashMap<Nonce, PendingSecret>,
}

struct PendingSecret {
    session_id: SessionId,
    secrets: HashMap<String, String>,       // key → value
    expires_at: Instant,
    consumed: bool,
}

/// Session scheduler state.
struct Scheduler {
    active_sessions: Vec<SessionId>,
    wip_limit: u32,
    persona_store: PersonaStore,            // from agent-identity crate
}

/// Conflict detection result.
struct ConflictCheck {
    session_id: SessionId,
    ticket_id: TicketId,
    overlapping_files: Vec<PathBuf>,
    conflicting_sessions: Vec<SessionId>,
    recommendation: ConflictAction,
}

enum ConflictAction {
    Proceed,                                // no overlap
    Warn(String),                           // minor overlap, operator decides
    Block(String),                          // critical overlap, cannot proceed
}
```

## Design Decisions Mapped from ADRs

| ADR | Implication |
|---|---|
| ADR-4 (Rust daemon + TUI) | Orchestrator core IS the daemon; TUI is a view layer on top |
| ADR-6 (Coordination protocol) | `ticket-api` for durable state; `tokio::mpsc` for intra-process event routing |
| ADR-10 (Budget controls) | Cost watchdog lives here; configurable thresholds from `orchestrator.toml` |
| ADR-12 (Operator authorization) | Operator allow-list loaded from config; trust tier enforcement |
| ADR-13 (Secret delivery) | Secret server is a sub-service of the daemon |
| `db784443` (Auth design) | Nonce lifecycle, TTL, single-use, audit logging all implemented here |

## Dependencies

- Depends on: `51471c3e` (sandbox manager), `a92569e5` (agent identity)
- Blocks: `a8632357` (assignment runner), `d0cc3c8b` (review coordinator), `5af54f6c` (TUI), `0135d961` (E2E)

## Acceptance Criteria

- [ ] Daemon binary starts, loads `orchestrator.toml`, and sets up all sub-services
- [ ] Session scheduler picks highest-priority ready tickets up to WIP limit
- [ ] Secret server issues nonce-gated secrets with 60s TTL and single-use enforcement
- [ ] Conflict detector blocks sessions with critical file overlap against running sessions
- [ ] Cost watchdog emits soft/hard budget signals per session
- [ ] Audit logger appends structured JSONL records for all state transitions and operator actions
- [ ] Internal `tokio::mpsc` event routing delivers `ProgressEvent` to TUI and notifier subscribers
- [ ] Graceful shutdown drains active sessions and flushes audit log
- [ ] Config validation rejects invalid `orchestrator.toml` with actionable error messages
- [ ] Unit tests cover: scheduler WIP enforcement, secret server nonce lifecycle, conflict detection, budget signal emission