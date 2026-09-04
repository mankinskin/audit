---
description: "Run an honest repository audit using the strongest available automated tools and return findings first."
name: "audit"
argument-hint: "[scope or path]"
agent: "agent"
---

# Automated Audit

Run automated audit tooling for the requested scope, gather the strongest supporting validation, and return findings first.

Reference [audit-cli](../../workflow-tools/audit/crates/audit-cli/README.md), [audit-mcp](../../workflow-tools/audit/crates/audit-mcp/README.md), [ticket-cli](../../memory-api/tools/cli/ticket-cli/README.md), [spec-cli](../../memory-api/tools/cli/spec-cli/README.md), and [log-viewer](../../workflow-tools/log/crates/log-viewer/README.md).

## Workflow

1. Treat the slash-command text as an optional audit scope.
2. Inspect the relevant tickets, specs, and recent validation context before running broad tools.
3. Prefer the strongest automated audit or validation surface available for the affected code.
4. Read the referenced code and tests directly instead of trusting summaries alone.
5. Report findings in severity order with the affected path or feature.
6. If no findings exist, say so explicitly and call out the main residual risks or coverage gaps.
7. Create or update tickets only when the user asks or when the audit clearly exposes missing tracked work.

## Response

Return:
- scope audited
- tools and validation used
- findings first, ordered by severity
- residual risks or coverage gaps
- recommended follow-up work, if any
