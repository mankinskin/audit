Back to [memory-api/README.md](../../../README.md).

# audit-cli

CLI interface for `audit-api`.

## Interface

Use `audit` when you need repository quality metrics and grouped findings from the terminal.

- `run`: execute a full audit and return metrics, thresholds, and actionable findings.
- `summary`: regroup findings by crate, category, severity, metric, or path.

Global options:

- `--json`: emit machine-readable output instead of the human summary.
- `--toon`: emit machine-readable TOON output instead of the human summary.

## Usage

Build or run from a checkout of `memory-viewers/memory-api`:

```bash
cargo build -p audit-cli --bin audit
cargo run -p audit-cli --bin audit -- --help
```

Run the command from the repository you want to audit, or pass an explicit repo path to the subcommand.

For compact structured output in this repository, prefer `rtk audit --toon ...` over `rtk audit --json ...`. Rust-side decoding should use `toon-format` / `toon-rust`.

## Workflow notes

When the audited repository has a local `.ticket` store, `audit run` includes the ticket-graph trial alongside the existing repository metrics.

That trial now reports both orphan-ticket topology issues and dependency-convergence risk when a more advanced ticket is waiting on an earlier-state prerequisite. In JSON or TOON output, those findings include the dependent and prerequisite ids or paths, both states, `dependency_state_gap`, and reverse-dependent reach evidence.

The unified `audit run` interface also supports session audits when you pass `--latest-session` for the most recent capture or `--session-id` for an explicit capture. Pair either selector with `--session-store-root` and `--session-workspace-slug` when the session store lives outside the repo root or when you need to pin the workspace slug. Session-audit output includes `schema_version` so downstream tooling can validate the persisted-session format before relying on the report.

Use `audit summary --by metric` when you want to collapse the results around metrics such as `dependency_convergence_count` during triage.

## Examples

```bash
# Run the default audit for the current repository
audit run .

# Inspect structured ticket-graph findings, including dependency convergence
rtk audit --toon run .

# Tighten thresholds for a focused cleanup pass
audit run . --max-file-lines 300 --max-cyclomatic-complexity 10 --coverage-warn-below 85

# Group findings by metric to isolate ticket-graph issues
audit summary --by metric .

# Audit the latest session in a local session store
audit --json run . --latest-session --session-store-root .session --session-workspace-slug context-engine

# Audit an explicit session by id
audit --json run . --session-id <session-id> --session-store-root .session --session-workspace-slug context-engine
```
