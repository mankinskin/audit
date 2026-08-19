Back to [memory-api/README.md](../../../README.md).

# audit-mcp

MCP server for `audit-api`.

## Interface

`audit-mcp` runs on stdio and computes repository audit results directly from the local checkout.

Named tools:

- `audit`: run the full repository audit and return the structured `AuditReport` payload.
- `audit_summary`: run the audit and regroup findings by crate, category, severity, metric, or path.

Each call accepts the repository root plus optional threshold overrides such as `max_file_lines`, `max_cyclomatic_complexity`, and `coverage_warn_below`.

## Workflow notes

When the target repository has a local `.ticket` store, the `audit` payload includes ticket-graph findings in addition to the normal repository metrics.

Those findings now cover dependency convergence as well as orphan tickets. The structured evidence includes dependent and prerequisite ids or paths, both states, `dependency_state_gap`, and reverse-dependent reach so clients can explain why a convergence risk was reported.

Use `audit_summary` with metric grouping when you want to isolate ticket-graph findings such as `dependency_convergence_count` from the rest of the audit output.

## Usage

Run the server on stdio:

```bash
cargo run -p audit-mcp
```

Example VS Code MCP configuration:

```json
{
  "servers": {
    "audit-mcp": {
      "type": "stdio",
      "command": "cargo",
      "args": ["run", "-p", "audit-mcp"]
    }
  }
}
```

## Examples

- Call `audit` when a client needs the full metrics and findings payload for a repository, including ticket-graph convergence findings when a local `.ticket` store is present.
- Call `audit_summary` when a client needs grouped issue counts, for example by metric during ticket-graph triage.
- Override the threshold fields per call when you want a stricter cleanup pass than the default repository policy.
