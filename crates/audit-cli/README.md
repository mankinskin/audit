Back to [audit/README.md](../../README.md).

# audit-cli

CLI interface for `audit-api`.

## Interface

Use `audit` when you need repository quality metrics and grouped findings from the terminal.

- `run`: execute a full audit and return metrics, thresholds, and actionable findings.
- `links`: validate Markdown links (see below).
- `summary`: regroup findings by crate, category, severity, metric, or path.

`run` includes repository-guidance findings when the repository root is
missing or has a whitespace-empty `README.md`, `INSTALL.md`, or
`CONTRIBUTING.md`. A present non-empty file passes the initial structural
check without semantic content validation.

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

## Markdown link validation (`audit links`)

`audit links [REPO]` validates every Markdown link and image destination.
`audit run` performs the same check as part of the full audit.

Options:

- `--include-historical`: also scan `transcripts/` and `.workflow-tools/`.
  Config key `include_historical = true` in `.audit.toml` does the same for `audit run`.
- `--check-external`: opt in to network checks of `http(s)` links. Config key
  `check_external = true` does the same for `audit run`. Both default to `false`.

### Scope

- Sources are all `*.md` files in the repository and its submodules; gitignore
  is respected and symlinks are not followed. Each file belongs to its innermost repository.
- `transcripts/` and `.workflow-tools/` are excluded unless `--include-historical` is given.
  `audit run` additionally honours `exclude_paths` from `.audit.toml`.

### Guidance set

The guidance set is `.agents/**`, `AGENTS.md`, `.github/copilot-instructions.md`
and `.claude/**`. A guidance file may link only to other guidance files or to any
`README.md`; other targets are blocking `non_guidance_target`. Other Markdown
files may link to any existing file in the same repository or a descendant submodule.

### Cross-repository rule

Links may only point into descendant submodules. Same-repository links are
checked against the working tree; submodule targets are checked at the gitlink
commit recorded in the parent index, and deeper levels via the parent submodule's
tree at its recorded commit. A nested repository without a gitlink entry is
blocking (`submodule_gitlink_missing`), as are upward or sibling links
(`cross_repository`) and links that leave the repository (`unsafe_path`).
Absolute filesystem path links are blocking (`unsupported_dependency`).

### Anchors

A `#fragment` on a Markdown target (or a fragment-only link) must match a GitHub
heading slug or an HTML `id=`/`name=` attribute. Line anchors such as `#L10` are
ignored. Fragments on non-Markdown targets are not checked. A miss is blocking `missing_anchor`.

### External checks

With `--check-external`, each distinct URL gets a HEAD request, falling back to GET
on 403/405, with a 10 s timeout and at most 8 requests in flight. Status 400 or
above (except 429) or a transport error is blocking `external_unreachable`;
429 is reported as non-blocking `external_rate_limited`. Without the flag no network access happens.

### Exit code and finding classes

The report is printed on stdout (`--json`, `--toon`, or text) and the exit code
is non-zero if and only if at least one blocking finding exists.

Blocking classes: `missing_target`, `non_guidance_target`, `cross_repository`,
`unsupported_dependency`, `unsafe_path`, `unreadable_artifact`,
`submodule_missing_target`, `submodule_revision_unavailable`,
`submodule_gitlink_missing`, `missing_anchor`, `external_unreachable`.
Non-blocking: `guidance_target`, `external`, `fragment_only`, `external_rate_limited`.
Each finding category is prefixed with `markdown_link_`.

```bash
audit links .
audit --json links . --include-historical
audit links . --check-external
```

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
