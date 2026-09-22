# Goal

Make the audit index responsive to file changes so that when relevant files are modified, a git hook automatically re-runs the audit and updates `.audit/README.md`.

## Problem

Currently, the audit index is stale by default. An agent must manually re-run `audit store-index` after running `audit` or after making code changes. This creates a mismatch between the actual audit state and what the index documents. The `.audit/README.md` provides no signal that the audit is out of date.

## Scope

**Per Q6 decision (hybrid approach):**

- Ingest the active git hooks configuration with any registered audit locations.
- Create a `pre-commit` hook that:
  - Detects which files are being staged.
  - Checks if those files match the audit's scope (configured via `.audit.toml` or environment).
  - If matched, runs `audit store-index` and stages the updated `.audit/README.md` if it changed.
  - Rejects the commit if audit findings became high-severity (optional policy enforcement).
- Document the hook setup in `.github/hooks/` and reference it in the audit-api README.
- Provide a CLI flag `audit store-index --watch-git-hooks` to enable this behavior during development.
- Store the last-rerun timestamp in a sidecar (`.audit/.index_last_rerun`) so agents can detect stale indexes.

## Acceptance criteria

- `pre-commit` hook runs `audit store-index` when relevant files are staged.
- The hook updates `.audit/README.md` if findings changed.
- Hook configuration is documented and tested.
- `.audit/.index_last_rerun` timestamp tracks the last successful index rerun.
- Agents can detect stale indexes by comparing file modification times to `.index_last_rerun`.

## Non-goals

- No persistent audit daemon that continuously monitors for changes (keep it hook-driven).
- No automatic remediation or fixes — only detection and index updates.
