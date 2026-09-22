# [AOH][Design] Operator Authorization, Secret Lifecycle, and Trust Boundaries

## Objective

Define the security and trust model for AOH before implementation starts.

Current tickets describe messenger approvals, container secret injection, remote GitHub actions, and operator control, but there is no dedicated ticket that answers who is allowed to do what, how secrets move through the system, or how the orchestrator proves that an approval/termination command came from an authorized human.

## Why This Is Missing but Critical

The current plan already assumes all of the following:
- users can approve/reject work from messenger adapters
- containers can fetch one-time secrets from the orchestrator
- local and remote git operations can happen under agent identities
- logs and reports are safe to surface in TUI and messenger notifications

Without a security contract, these assumptions are unsafe and ambiguous.

## Resolved Decisions (2026-04-09)

The following questions were answered during the design interview. These decisions are now locked as ADRs in `34bc4938`.

| Question | Decision |
|---|---|
| **Q2: Messenger approval model** | **Option C** — Full messenger control. Operators can approve, reject, retry, extend-budget, stop, and terminate sessions from Telegram/Discord/Slack. Locked as **ADR-12**. |
| **Q3: Operator identity model (v1)** | **Option B → C** — Flat allow-list of messenger user IDs in `orchestrator.toml`. Local OS user implicitly trusted for TUI actions. Designed for extensibility toward per-action grants (Option C). Locked as **ADR-12**. |
| **Q4: Secret delivery** | **Option A** — One-time HTTP/Unix-socket fetch with nonce + TTL (default 60s). Nonce is consumed on first fetch; further requests return 404. Env vars forbidden in CI/prod; local-dev convenience flag only. Locked as **ADR-13**. |

### Remaining Questions — Resolved During Refinement

> **Locked — do not reopen without new evidence.**

#### Secret Storage at Rest

| Secret | v1 Storage | Rationale |
|---|---|---|
| Copilot API key | `orchestrator.toml` under `[secrets]`, file permission 0600 | Single-user local machine; OS keyring adds complexity without matching threat model in v1. |
| GitHub PAT / App credentials | `orchestrator.toml` under `[secrets]`, file permission 0600 | Same as above. GitHub App private key referenced by file path. |
| Messenger bot tokens | `orchestrator.toml` under `[secrets]`, file permission 0600 | Telegram/Discord/Slack tokens. |
| Per-persona SSH keys | `config/persona-keys/{persona-id}/id_ed25519`, permission 0600 | File-based; standard ssh-agent integration. |

**v1 decision**: Plaintext in permission-restricted `orchestrator.toml`. No encryption at rest in v1 — the threat model is single-user on a trusted local machine. The config file MUST NOT be committed to git (enforced via `.gitignore` entry). For v2, add optional Age/SOPS encryption or OS keyring backend (keyring crate).

**Config structure:**
```toml
[secrets]
copilot_api_key = "sk-..."
github_pat = "ghp_..."  # OR github_app_private_key_path = "path/to/key.pem"

[secrets.messenger]
telegram_bot_token = "..."
discord_bot_token = "..."
slack_bot_token = "..."
```

#### Secret Rotation and Revocation

| Secret | Rotation trigger | Revocation path |
|---|---|---|
| Copilot API key | Manual operator replacement in config | Remove from config → orchestrator detects missing key on next session start |
| GitHub PAT | GitHub token expiry (90-day default) | Revoke in GitHub settings → sessions fail → operator updates config |
| GitHub App key | Annual rotation recommended | Generate new key in GitHub App settings → update config file path |
| Messenger tokens | Regenerate via platform dashboard | Revoke old token → update config → restart orchestrator |
| SSH keys | On persona compromise suspicion | Delete key file + remove deploy key from GitHub |

**v1 decision**: Manual rotation only. The orchestrator logs a **warning** at startup if any configured secret has been unchanged for >90 days (tracked via `orchestrator.toml` `[secrets_metadata]` section with `last_rotated` timestamps). No automatic rotation.

```toml
[secrets_metadata]
copilot_api_key_last_rotated = "2026-04-09"
github_pat_last_rotated = "2026-04-09"
```

#### Replay Protection for Secret Fetch

The one-time secret fetch (ADR-13) is protected by:

1. **Nonce**: Orchestrator generates a UUIDv4 nonce per session when creating the container. Nonce is passed as a container environment variable (`AOH_SECRET_NONCE`).
2. **TTL**: Nonce expires after 60 seconds (configurable). After expiry, the secret endpoint returns 410 Gone.
3. **Single-use**: First successful fetch marks the nonce as consumed in orchestrator memory. Subsequent requests return 404 Not Found.
4. **Container-scoped**: The secret endpoint binds to a Unix socket inside the container mount namespace (Linux) or a per-container localhost port with nonce validation (Docker Desktop/WSL2).
5. **No replay from other containers**: Each container gets a unique nonce. Even if a container reads another's nonce from shared state (not possible with proper isolation), the nonce is bound to the specific session ID.

**Docker Desktop/WSL2 path**: The orchestrator listens on `host.docker.internal:{port}` with a unique port per active session. The container fetches `http://host.docker.internal:{port}/secret?nonce={nonce}&session={session_id}`. The orchestrator validates session_id + nonce + TTL before responding.

**Linux CI path**: The orchestrator creates a Unix socket at `/tmp/aoh-{session-id}.sock`, bind-mounted into the container at `/aoh/secret.sock`. The container fetches `http://unix:/aoh/secret.sock:/secret?nonce={nonce}`.

#### Redaction Rules

All output channels apply these rules before any data leaves the orchestrator:

| Field pattern | TUI | Messenger | Archive | PR metadata |
|---|---|---|---|---|
| `*_token`, `*_key`, `*_secret`, `*_pat` | `***REDACTED***` | `***REDACTED***` | `***REDACTED***` | `***REDACTED***` |
| `Authorization:` header values | Strip | Strip | Strip | Strip |
| SSH private key content | Never logged | Never sent | Never archived | Never included |
| Container env vars containing `SECRET`, `TOKEN`, `KEY` | `***REDACTED***` | `***REDACTED***` | Stored as `[redacted]` | `***REDACTED***` |
| File paths to secret files | Shown (path only) | Hidden | Stored (path only) | Hidden |
| Agent stdout/stderr | Full (TUI only) | Truncated summary | Full | Diff summary only |
| Operator user IDs | Shown | Shown (own context) | Stored | Shown |
| Ticket content | Full | Summary | Full | Summary |

**Fail-safe**: If the redaction filter encounters an unrecognized field that matches the regex `(?i)(secret|token|key|password|credential|api[_-]?key|pat|private)`, it defaults to `***REDACTED***` rather than passing through. This is a **deny-by-default** policy for secret-looking values.

**Failure mode**: If the redaction pipeline itself fails (panic, OOM), the entire output is suppressed with an error log: `"Output suppressed: redaction filter failed. Review raw logs at {path}."` No partial output escapes.

#### Audit Log Format

Audit events are written to `.aoh/audit/audit.jsonl` (JSON Lines format, append-only):

```json
{"ts": "2026-04-09T14:30:00Z", "event": "session_start", "session_id": "...", "ticket_id": "...", "agent_id": "agent-petal", "operator": "local:os-user"}
{"ts": "2026-04-09T14:35:00Z", "event": "secret_fetch", "session_id": "...", "nonce": "...", "result": "ok", "ttl_remaining_ms": 42000}
{"ts": "2026-04-09T14:36:00Z", "event": "secret_fetch_replay", "session_id": "...", "nonce": "...", "result": "rejected:consumed"}
{"ts": "2026-04-09T15:00:00Z", "event": "operator_action", "action": "approve_pr", "ticket_id": "...", "operator": "telegram:12345678", "operator_name": "linus"}
{"ts": "2026-04-09T15:01:00Z", "event": "merge", "ticket_id": "...", "source_branch": "aoh/agent-petal/...", "target_branch": "main", "operator": "telegram:12345678"}
{"ts": "2026-04-09T15:05:00Z", "event": "budget_extend", "session_id": "...", "old_budget": 100000, "new_budget": 200000, "operator": "telegram:12345678"}
```

**Audit event types**: `session_start`, `session_complete`, `session_terminate`, `session_revive`, `secret_fetch`, `secret_fetch_replay`, `operator_action`, `merge`, `push_remote`, `budget_extend`, `conflict_detected`, `redaction_failure`.

**Retention**: Audit logs follow the same retention as session archives (indefinite, manual prune). They are NOT gitignored — committed to the `aoh-meta` branch for traceability.

#### Action Trust Tiers

| Trust level | Actions | Authorization required |
|---|---|---|
| **Low** | View status, list sessions, view diffs | Any allow-listed operator (TUI or messenger) |
| **Medium** | Approve/reject PR, extend budget, retry session | Any allow-listed operator |
| **High** | Terminate session, push to remote, merge to main | Any allow-listed operator (v1). Per-action grants in v2. |
| **Critical** | Modify allow-list, rotate secrets, prune archives | Local OS user only (TUI). Not available via messenger in v1. |

**v1 simplification**: All allow-listed operators have the same permissions (low + medium + high). Critical actions require local TUI access. Per-operator permission grants are deferred to v2.

## Questions to Resolve

### Operator identity and authorization
1. Which identities are trusted to control AOH: local OS user only, configured allow-list, GitHub identity mapping, or per-messenger user IDs?
2. Can messenger users approve merges in v1, or are messenger adapters notify-only until identity mapping exists?
3. Which actions require stronger trust than others?
   - approve/reject review
   - extend budget
   - terminate session
   - push remote branch
   - merge to main
4. How is operator identity represented in audit logs?

### Secret lifecycle
5. Which secrets exist in v1?
   - Copilot token
   - GitHub token/app credentials
   - messenger bot tokens
   - per-agent SSH keys, if used
6. Where are secrets stored at rest?
7. How are secrets scoped per session and expired?
8. How are secrets rotated and revoked?

### Container trust boundary
9. How does a container reach the secret server under Docker Desktop/WSL2 and Linux CI?
10. How do we prevent the secret-fetch path from being replayed by another process/container?
11. What host resources are intentionally exposed to containers, and which are forbidden?

### Log/output safety
12. Which fields must always be redacted from:
   - TUI logs
   - messenger notifications
   - session archives
   - PR metadata
13. What is the failure behavior if redaction cannot be proven?

## Alternatives to Consider

### Approval model alternatives
- **Option A**: TUI-only approval/merge in v1; messenger is notify-only
- **Option B**: Messenger approval allowed for low-risk actions only; merge still TUI-only
- **Option C**: Full messenger control with explicit operator allow-list and signed action tokens

### Secret delivery alternatives
- **Option A**: One-time HTTP/Unix-socket secret fetch with expiry and nonce
- **Option B**: tmpfs-mounted secret files created by orchestrator and removed on cleanup
- **Option C**: short-lived env vars accepted only for local dev mode, forbidden in CI/prod

## Deliverables

- Threat model and trust-boundary diagram
- Operator authorization model for TUI and messenger actions
- Secret inventory with storage, scope, lifetime, and rotation rules
- Redaction policy for logs, notifications, archives, and PR metadata
- Recommendation for v1 approval mode and secret transport

## Acceptance Criteria

- [x] Trusted operator identities and allowed control actions are defined
- [x] v1 decision made: messenger notify-only vs limited control vs full control
- [x] Secret inventory and lifecycle matrix documented for all v1 credentials
- [x] Secret delivery path specified for Docker Desktop/WSL2 and Linux CI
- [x] Replay protection / nonce / expiry rules defined for secret fetch and operator actions
- [x] Redaction rules documented for logs, notifications, archives, and PR metadata
- [x] Failure mode defined when auth or redaction checks fail
- [x] Architecture ticket updated to reference the approved trust model