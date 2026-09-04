---
description: "Use when editing or operating the audit tool. Covers CLI and MCP usage, repo config, and how to interpret audit output."
---


## Purpose

`audit` is the repository quality audit tool for this workspace. When audit guidance applies, execute the audit tool against the target context, surface the report's concrete findings, and summarize the required follow-up work in a canonical findings-and-recommendations format.

- Core library crate: `audit-api`
- CLI package: `audit-cli` with the `audit` binary
- MCP package: `audit-mcp`
- MCP tool: `audit`

Keep the layering thin and explicit:

1. `audit-api` owns audit logic, models, config loading, indexing, and trials.
2. `audit-cli` owns argument parsing and human/json rendering.
3. `audit-mcp` only translates MCP inputs into `audit-api` calls and serializes the result.

One audit run:

1. resolves the repo root
2. loads `.audit.toml`
3. syncs source files into `.audit/audit.sqlite3`
4. prunes stale index rows not seen in the latest scan
5. collects file length, compiler warning, test success, coverage, and static complexity metrics
6. returns raw metrics plus actionable findings and deduplicated fix instructions

Prefer structured output for automation and agent workflows so the run can be summarized canonically from the returned report. Prefer text output only for quick local inspection.

## CLI Usage

Basic audit:

```bash
cargo run -p audit-cli --bin audit -- run <target-context>
```

Machine-readable output:

```bash
cargo run -p audit-cli --bin audit -- --json run <target-context>
```

Override thresholds for a stricter audit:

```bash
cargo run -p audit-cli --bin audit -- run <target-context> \
  --max-file-lines 300 \
  --max-cyclomatic-complexity 10 \
  --coverage-warn-below 85
```

The default thresholds are:

- `max_file_lines = 400`
- `max_cyclomatic_complexity = 12`
- `coverage_warn_below = 80.0`

## MCP Usage

Run the server on stdio:

```bash
cargo run -p audit-mcp --bin audit-mcp
```

Tool input example:

```json
{
  "repo_root": "<target-context>",
  "max_file_lines": 350,
  "max_cyclomatic_complexity": 10,
  "coverage_warn_below": 85.0
}
```

The MCP tool always returns the full structured `AuditReport` payload. Use it as the single synchronized read for the target context's quality state and as the source for the canonical findings and recommendations summary.

## Repo Config

`audit` auto-loads a repo-root `.audit.toml` file.

Example:

```toml
max_file_lines = 400
max_cyclomatic_complexity = 12
coverage_warn_below = 80.0

[exclude]
paths = ["target", "node_modules"]
```

## Rule Audit Manual

Use this pass when maintaining prompt or instruction quality in the rule system. Apply the same pattern to any other target context you audit.

1. Resolve the target context to audit.

2. Run the audit on that target context. For a baseline CLI run:

```bash
audit run <target-context>
```

3. For compact structured output in this repository, prefer:

```bash
rtk audit --toon run <target-context>
```

4. Read structured `findings` first, then the deduplicated repair `instructions`.
5. Summarize the run in this canonical format:
- `Findings`
- one bullet per finding with severity, scope or path, and the failing signal
- `Recommendations`
- one bullet per remediation action, deduplicated when several findings share the same fix

6. When overlap is high, treat `rule_overlap` findings as a dedup/refactor signal:
- identify the overlapping rule ids or file scopes
- keep one canonical owner for repeated guidance
- remove duplicated wording from secondary rules and regenerate targets

7. After edits, rerun the audit on the same target context to confirm findings were reduced or resolved.
